//! Opt-in characterization of the retained failed build; not a runtime feature.
use super::CoastalPolicy;
use crate::coordinate_system::geographic::LLBBox;
use crate::elevation::postprocess::{
    fill_nan_values, filter_elevation_outliers, repair_terrain_anomalies,
};
use rayon::prelude::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

fn summary(stage: &str, heights: &[Vec<f64>], classes: &[Vec<u8>]) -> Value {
    let mut wet = 0usize;
    let mut dry = 0usize;
    let mut above = Vec::new();
    let mut nonfinite = 0usize;
    for (row, codes) in heights.iter().zip(classes) {
        for (&height, &class) in row.iter().zip(codes) {
            if class == 1 {
                dry += 1;
            }
            if class != 2 {
                continue;
            }
            wet += 1;
            if !height.is_finite() {
                nonfinite += 1;
            } else if height > 2.0 {
                above.push(height);
            }
        }
    }
    above.sort_by(f64::total_cmp);
    json!({"stage":stage,"ocean_samples":wet,"classified_dry_samples":dry,
        "nonfinite_ocean_samples":nonfinite,"ocean_above_2m":above.len(),
        "conflict_min_m":above.first(),"conflict_max_m":above.last()})
}

#[test]
#[ignore = "requires the authenticated retained local source bundle and a fresh diagnostic output path"]
fn diagnose_retained_coastal_failure() {
    let manifest =
        PathBuf::from(std::env::var("ARNIS27_DIAGNOSTIC_SOURCES").expect("explicit manifest"));
    let output =
        PathBuf::from(std::env::var("ARNIS27_DIAGNOSTIC_OUTPUT").expect("explicit new output"));
    assert!(manifest.is_absolute() && output.is_absolute());
    assert!(!output.exists(), "never replace prior evidence");
    let manifest_sha = format!("{:x}", Sha256::digest(std::fs::read(&manifest).unwrap()));
    assert_eq!(
        manifest_sha,
        "f55f3a6e648d75043709ec0de87915833dbd31ed577979e14ba11744197f1067"
    );
    let sources = crate::tiler_contract::admit_sources(&manifest).unwrap();
    let bbox = [
        40.688284354832476,
        -74.01989985008785,
        40.69345957957617,
        -74.0114401990945,
    ];
    let ll = LLBBox::new(bbox[0], bbox[1], bbox[2], bbox[3]).unwrap();
    let dimensions = crate::elevation::compute_grid_dims(&ll, 1.0);
    assert_eq!(dimensions, (714, 576, 714, 576));
    let (_, _, width, height) = dimensions;
    let policy = CoastalPolicy::load(&sources, &bbox).unwrap();
    let lc =
        crate::land_cover::fetch_land_cover_data_with_sources(&ll, width, height, Some(&sources))
            .unwrap();
    let raw = crate::elevation::providers::aws_terrain::AwsTerrain
        .fetch_raw_frozen(&ll, width, height, &sources)
        .unwrap()
        .heights_meters;
    let raw_copy = raw.clone();
    let mut repaired = raw.clone();
    let classes: Vec<Vec<u8>> = (0..height)
        .into_par_iter()
        .map(|row| {
            (0..width)
                .map(|col| {
                    let lat = bbox[2] - row as f64 * (bbox[2] - bbox[0]) / (height - 1) as f64;
                    let lon = bbox[1] + col as f64 * (bbox[3] - bbox[1]) / (width - 1) as f64;
                    policy.classify(lon, lat)
                })
                .collect()
        })
        .collect();
    let mut stages = vec![summary("raw_aws", &raw, &classes)];
    filter_elevation_outliers(&mut repaired);
    stages.push(summary("after_outlier_filter", &repaired, &classes));
    repair_terrain_anomalies(&mut repaired);
    stages.push(summary("after_anomaly_repair", &repaired, &classes));
    fill_nan_values(&mut repaired);
    stages.push(summary(
        "after_nan_fill_pre_coastal_capture",
        &repaired,
        &classes,
    ));
    assert_eq!(raw, raw_copy);
    // Independent values retained from the original September 12 renderer.log.
    assert_eq!(repaired[26][41], 2.0085670525732713);
    let error = policy
        .capture(&repaired, &lc.grid, &bbox)
        .err()
        .expect("retained build must reproduce rejection");
    assert_eq!(error,"Coastal wet DEM conflict at lon=-74.01941339049357, lat=40.69322556941385: repaired DEM 2.0085670525732713m exceeds +2.0m above sea level");
    let mut conflicts = Vec::new();
    let mut changed = 0usize;
    for row in 0..height {
        for col in 0..width {
            if raw[row][col] != repaired[row][col] {
                changed += 1;
            }
            if classes[row][col] != 2
                || !repaired[row][col].is_finite()
                || repaired[row][col] <= 2.0
            {
                continue;
            }
            conflicts.push(json!({"row":row,"col":col,
            "lat":bbox[2]-row as f64*(bbox[2]-bbox[0])/(height-1) as f64,
            "lon":bbox[1]+col as f64*(bbox[3]-bbox[1])/(width-1) as f64,
            "raw_m":raw[row][col],"repaired_m":repaired[row][col],"esa_class":lc.grid[row][col]}));
        }
    }
    std::fs::create_dir(&output).unwrap();
    let writer = std::fs::File::create(output.join("grids.json.gz")).unwrap();
    let mut gz = flate2::write::GzEncoder::new(writer, flate2::Compression::fast());
    serde_json::to_writer(
        &mut gz,
        &json!({"raw_m":raw,"repaired_m":repaired,"coastal_classes":classes,"esa_classes":lc.grid}),
    )
    .unwrap();
    gz.finish().unwrap();
    let diagnostic_sha = format!(
        "{:x}",
        Sha256::digest(include_bytes!("coastal_diagnostic_tests.rs"))
    );
    let report = json!({"diagnostic_source_sha256":diagnostic_sha,"source_manifest_sha256":manifest_sha,"bbox":bbox,"dimensions":[width,height],
        "stages":stages,"changed_samples":changed,"error":error,"conflicts":conflicts,
        "note":"Actual frozen AWS/ESA samplers and production repair sequence; no coastal guard bypass or world generated."});
    std::fs::write(
        output.join("repaired-diagnosis.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!(
        "{}",
        serde_json::to_string_pretty(&report["stages"]).unwrap()
    );
}
