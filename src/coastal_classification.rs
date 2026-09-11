//! Offline source-backed master-sample coastal classification.
use crate::coordinate_system::geographic::LLBBox;
use std::ffi::OsString;
use std::path::PathBuf;

pub(crate) fn descriptor() -> serde_json::Value {
    serde_json::json!({
        "schema":"arnis-coastal-classification/v1",
        "command":"--produce-coastal-classification",
        "arguments":["--bbox","--frozen-sources","--classification-output"],
        "profile_sha256":crate::tiler_contract::profile_hash(),"scale":1.0,
        "network":"none","platform":"linux",
        "output":["water-classification.json","classification.json"],
        "limits":{"deadline_seconds":600,"memory_bytes":8589934592_u64,
            "disk_bytes":21474836480_u64,"log_bytes":8388608,"max_files":64,
            "tasks":64,"file_descriptors":256,"workers":1,"rayon_threads":1,
            "manifest_bytes":1048576,"osm_bytes":536870912,"geometry_bytes":67108864,
            "input_vertices":1000000,"intermediate_edges":1000000,
            "classification_bytes":16777216,"classification_vertices":100000,
            "report_bytes":1048576,"master_max_axis":16384,"master_max_cells":16777216}
    })
}

pub(crate) fn run_command(args: &[OsString]) -> Option<Result<(), String>> {
    if !args.iter().any(|a| {
        a.to_str().is_some_and(|s| {
            s.starts_with("--produce-coastal-classification")
                || s.starts_with("--describe-coastal-classification")
                || s.starts_with("--classification-output")
        })
    }) {
        return None;
    }
    Some((|| {
        if args == [OsString::from("--describe-coastal-classification")] {
            println!("{}", descriptor());
            return Ok(());
        }
        if args.len() != 7
            || args[0] != "--produce-coastal-classification"
            || args[1] != "--bbox"
            || args[3] != "--frozen-sources"
            || args[5] != "--classification-output"
        {
            return Err("coastal_input: expected --produce-coastal-classification --bbox S,W,N,E --frozen-sources ABS --classification-output ABS".into());
        }
        let bbox = LLBBox::from_str(
            args[2]
                .to_str()
                .ok_or("coastal_input: bbox must be UTF-8")?,
        )
        .map_err(|e| e.to_string())?;
        for p in [&args[4], &args[6]] {
            let p = PathBuf::from(p);
            if !p.is_absolute()
                || p.file_name().is_none()
                || p.components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(
                    "coastal_input: paths must be absolute without parent traversal".into(),
                );
            }
        }
        produce(
            &bbox,
            &PathBuf::from(&args[4]),
            &PathBuf::from(&args[6]),
            std::time::Duration::from_secs(600),
        )
    })())
}

#[cfg(test)]
#[path = "coastal_classification_tests.rs"]
mod tests;

use geo::{BoundingRect, Intersects};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};
struct Deadline {
    started: Instant,
    limit: Duration,
}
impl Deadline {
    fn check(&self) -> Result<(), String> {
        if self.started.elapsed() >= self.limit {
            Err("coastal_deadline: whole-stage deadline exceeded".into())
        } else {
            Ok(())
        }
    }
}
fn write_synced(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(bytes)
        .and_then(|_| f.sync_all())
        .map_err(|e| e.to_string())
}
fn produce(bbox: &LLBBox, manifest: &Path, output: &Path, limit: Duration) -> Result<(), String> {
    let deadline = Deadline {
        started: Instant::now(),
        limit,
    };
    deadline.check()?;
    if !output.is_absolute()
        || output.file_name().is_none()
        || output
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        || output.symlink_metadata().is_ok()
    {
        return Err("coastal_input: output must be a fresh absolute path".into());
    }
    let parent = output
        .parent()
        .ok_or("coastal_input: missing parent")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let output = parent.join(output.file_name().ok_or("coastal_input: missing name")?);
    let contract_error = |message| crate::tiler_contract::ContractError {
        exit_code: 1,
        message,
    };
    let sources = crate::tiler_contract::admit_sources_with_checks(
        manifest,
        |entries| {
            for (kind, key, max) in [
                ("osm", "master-osm", 536870912_u64),
                (
                    "coastal_geometry",
                    crate::coastal_geometry::KEY,
                    crate::coastal_geometry::MAX_BYTES,
                ),
            ] {
                let entry = entries
                    .iter()
                    .find(|e| e.kind == kind && e.key == key)
                    .ok_or_else(|| {
                        contract_error(format!("coastal_input: missing {kind}:{key}"))
                    })?;
                if entry.size_bytes > max {
                    return Err(contract_error(format!(
                        "coastal_capacity: {kind}:{key} exceeds {max} bytes before admission"
                    )));
                }
            }
            Ok(())
        },
        || deadline.check().map_err(contract_error),
    )
    .map_err(|e| e.message)?;
    deadline.check()?;
    let ocean_bytes = sources.resolve(
        "coastal_geometry",
        crate::coastal_geometry::KEY,
        crate::coastal_geometry::MAX_BYTES,
    )?;
    deadline.check()?;
    let (frame, water) = crate::coastal_geometry::parse(
        &ocean_bytes,
        [
            bbox.min().lat(),
            bbox.min().lng(),
            bbox.max().lat(),
            bbox.max().lng(),
        ],
    )?;
    let geometry_sha256 = format!("{:x}", Sha256::digest(&ocean_bytes));
    drop(ocean_bytes);
    deadline.check()?;
    let osm_bytes = sources.resolve("osm", "master-osm", 536870912)?;
    deadline.check()?;
    let osm_sha256 = format!("{:x}", Sha256::digest(&osm_bytes));
    let osm: crate::osm_parser::OsmData =
        serde_json::from_slice(&osm_bytes).map_err(|e| format!("coastal_input: OSM {e}"))?;
    drop(osm_bytes);
    if osm.remark.is_some() {
        return Err("coastal_input: OSM server remark rejected".into());
    }
    deadline.check()?;
    let (classes, cliff_ways, cliff_cells) = classify_inputs(osm, *bbox, frame, &water, &deadline)?;
    let (bytes, mode) =
        crate::coastal_classification_geometry::document(frame, &water, &classes, || {
            deadline.check()
        })?;
    let mut binary = std::fs::File::open("/proc/self/exe").map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        deadline.check()?;
        let n = binary.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    let caps = crate::tiler_contract::capability_report();
    let wet = classes.iter().filter(|c| **c == 2).count();
    let dry = classes.iter().filter(|c| **c == 1).count();
    let report=serde_json::to_vec(&serde_json::json!({
        "schema":"arnis-coastal-classification-report/v1","descriptor":descriptor(),"build":caps["build"],"upstream":caps["upstream"],
        "binary_sha256":format!("{:x}",hash.finalize()),"bbox":frame.bbox,"grid_dimensions":frame.dims,
        "sources_sha256":sources.sha256,"osm_sha256":osm_sha256,"geometry_sha256":geometry_sha256,
        "classification_sha256":format!("{:x}",Sha256::digest(&bytes)),"encoding":mode,"representation":"master-sample footprints",
        "classes":{"ordinary":classes.len()-wet-dry,"dry":dry,"wet":wet},"cliffs":{"ways":cliff_ways,"cells":cliff_cells},"elapsed_millis":deadline.started.elapsed().as_millis()
    })).map_err(|e|e.to_string())?;
    if report.len() > 1048576 {
        return Err("coastal_capacity: report exceeds 1 MiB".into());
    }
    let stage = tempfile::Builder::new()
        .prefix(".coastal-incomplete-")
        .tempdir_in(&parent)
        .map_err(|e| e.to_string())?;
    write_synced(&stage.path().join("water-classification.json"), &bytes)?;
    write_synced(&stage.path().join("classification.json"), &report)?;
    std::fs::File::open(stage.path())
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    deadline.check()?;
    publish_verified(stage.path(), &output, || {
        std::fs::File::open(&parent).and_then(|f| f.sync_all())
    })
}

fn apply_evidence(
    osm: crate::osm_parser::OsmData,
    bbox: LLBBox,
    frame: crate::coastal_geometry::Frame,
    classes: &mut [u8],
    deadline: &Deadline,
) -> Result<(usize, usize), String> {
    use crate::coordinate_system::transformation::CoordTransformer;
    use crate::element_processing::natural::{natural_edge_cells, natural_way_fill};
    use crate::osm_parser::{ProcessedElement, ProcessedMemberRole};
    let [w, h] = frame.dims;
    let (transform, bounds) = CoordTransformer::for_master_grid(&bbox, w as u32, h as u32)?;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .map_err(|e| e.to_string())?;
    pool.install(|| {
        let (elements, bounds, _, _) =
            crate::osm_parser::parse_osm_data_with_frame(osm, bbox, 1., transform, bounds);
        deadline.check()?;
        let masks = crate::land_cover::osm_land_override::raw_osm_evidence(
            frame.dims, frame.dims, &elements, &bounds, 1.,
        );
        deadline.check()?;
        let cache = crate::floodfill_cache::FloodFillCache::precompute_master(
            &[],
            None,
            crate::clipping::MasterGeometry {
                bounds: bounds.clone(),
                offset: (0, 0),
            },
            bounds,
        );
        let mut cliff = vec![0u64; (w * h).div_ceil(64)];
        let mut cliff_ways = 0;
        let mut visit = |way: &crate::osm_parser::ProcessedWay| -> Result<(), String> {
            deadline.check()?;
            cliff_ways += 1;
            let Some(fill) = natural_way_fill(way, &cache, None) else {
                return Ok(());
            };
            let mut mark = |x: i32, z: i32| {
                if x >= 0 && z >= 0 && (x as usize) < w && (z as usize) < h {
                    let i = z as usize * w + x as usize;
                    cliff[i / 64] |= 1u64 << (i % 64);
                }
            };
            for pair in way.nodes.windows(2) {
                deadline.check()?;
                for (x, _, z) in natural_edge_cells((pair[0].x, pair[0].z), (pair[1].x, pair[1].z))
                {
                    mark(x, z);
                }
            }
            for (i, &(x, z)) in fill.iter().enumerate() {
                if i % 1024 == 0 {
                    deadline.check()?;
                }
                mark(x, z);
            }
            Ok(())
        };
        for element in &elements {
            if element.tags().get("natural").map(String::as_str) != Some("cliff") {
                continue;
            }
            match element {
                ProcessedElement::Way(way) => visit(way)?,
                ProcessedElement::Relation(rel) => {
                    for member in &rel.members {
                        if member.role == ProcessedMemberRole::Outer {
                            visit(&member.way)?;
                        }
                    }
                }
                _ => {}
            }
        }
        let cliff_cells = cliff.iter().map(|v| v.count_ones() as usize).sum();
        for (i, c) in classes.iter_mut().enumerate() {
            if i % 1024 == 0 {
                deadline.check()?;
            }
            if *c != 2 && !masks.veto(i) && (masks.land(i) || (cliff[i / 64] >> (i % 64)) & 1 == 1)
            {
                *c = 1;
            }
        }
        Ok((cliff_ways, cliff_cells))
    })
}

fn classify_inputs(
    osm: crate::osm_parser::OsmData,
    bbox: LLBBox,
    frame: crate::coastal_geometry::Frame,
    water: &geo::MultiPolygon,
    deadline: &Deadline,
) -> Result<(Vec<u8>, usize, usize), String> {
    let [w, h] = frame.dims;
    let mut classes = vec![0u8; w * h];
    let indexed: Vec<_> = water.0.iter().map(|p| (p.bounding_rect(), p)).collect();
    for z in 0..h {
        deadline.check()?;
        let lat = frame.point(0, z).y();
        let candidates: Vec<_> = indexed
            .iter()
            .filter(|(r, _)| r.is_some_and(|r| lat >= r.min().y && lat <= r.max().y))
            .collect();
        for x in 0..w {
            if x % 1024 == 0 {
                deadline.check()?;
            }
            let point = frame.point(x, z);
            if candidates.iter().any(|(r, p)| {
                r.is_some_and(|r| point.x() >= r.min().x && point.x() <= r.max().x)
                    && p.intersects(&point)
            }) {
                classes[z * w + x] = 2;
            }
        }
    }
    let (cliff_ways, cliff_cells) = if water.0.is_empty() {
        (0, 0)
    } else {
        apply_evidence(osm, bbox, frame, &mut classes, deadline)?
    };
    Ok((classes, cliff_ways, cliff_cells))
}

fn publish_verified(
    stage: &Path,
    output: &Path,
    sync: impl FnOnce() -> std::io::Result<()>,
) -> Result<(), String> {
    crate::provider_capture::publish_noreplace(stage, output)?;
    if let Err(e) = sync() {
        return Err(format!(
            "coastal_publish_durability: {} exists but parent sync failed: {e}",
            output.display()
        ));
    }
    Ok(())
}
