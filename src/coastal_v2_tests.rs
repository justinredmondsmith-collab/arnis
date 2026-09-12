use super::*;
use serde_json::{json, Value};
const BBOX: [f64; 4] = [0., 0., 0.0001, 0.0001];
fn area(w: f64, e: f64) -> Value {
    json!({"type":"MultiPolygon","coordinates":[[[[w,0.],[e,0.],[e,0.0001],[w,0.0001],[w,0.]]]]})
}
fn osm() -> Value {
    json!({"elements":[
        {"type":"node","id":1,"lon":-0.001,"lat":-0.001},
        {"type":"node","id":2,"lon":0.001,"lat":-0.001},
        {"type":"node","id":3,"lon":0.001,"lat":0.001},
        {"type":"node","id":4,"lon":-0.001,"lat":0.001},
        {"type":"way","id":10,"nodes":[1,2,3,4,1],"tags":{"natural":"water"}}
    ]})
}
fn classification() -> Value {
    json!({"schema_version":1,"policy":"master-coastal-water-v2","bbox":BBOX,"default_classification":"inland",
        "sources":[{"kind":"osm","key":"master-osm"},{"kind":"coastal_geometry","key":"master-ocean-geometry"}],
        "coastal_domains":[{"id":"sea","domain":area(0.,0.0001),"water":area(0.,0.00006),"inland_exclusions":{"type":"MultiPolygon","coordinates":[]}}]})
}
fn load(
    osm: Value,
    classification: Value,
    ocean_water: Value,
    omit: &str,
) -> Result<CoastalPolicy, String> {
    use sha2::{Digest, Sha256};
    let dir = tempfile::tempdir().unwrap();
    let frame = crate::coastal_geometry::Frame::new(BBOX).unwrap();
    let ocean = json!({"schema_version":1,"policy":"global-ocean-clip-v1","profile_sha256":crate::tiler_contract::profile_hash(),"bbox":BBOX,"grid_dimensions":frame.dims,"water":ocean_water,
        "dataset":{"url":"https://osmdata.openstreetmap.de/download/water-polygons-split-4326.zip","archive_sha256":"a".repeat(64),"projection":"EPSG:4326","parts":{"ocean.shp":{"sha256":"b".repeat(64),"size_bytes":1},"ocean.shx":{"sha256":"c".repeat(64),"size_bytes":1},"ocean.prj":{"sha256":"d".repeat(64),"size_bytes":1}}},"complete_dataset_scan":true,"selected_features":1});
    let mut entries = vec![];
    for (kind, key, value) in [
        ("osm", "master-osm", osm),
        ("coastal_geometry", "master-ocean-geometry", ocean),
        (
            "water_classification",
            "master-water-classification",
            classification,
        ),
    ] {
        if kind == omit {
            continue;
        }
        let bytes = serde_json::to_vec(&value).unwrap();
        std::fs::write(dir.path().join(key), &bytes).unwrap();
        entries.push(json!({"kind":kind,"key":key,"path":key,"sha256":format!("{:x}",Sha256::digest(&bytes)),"size_bytes":bytes.len()}));
    }
    let manifest = dir.path().join("manifest.json");
    std::fs::write(&manifest,serde_json::to_vec(&json!({"schema_version":1,"profile_sha256":crate::tiler_contract::profile_hash(),"entries":entries})).unwrap()).unwrap();
    let sources = crate::tiler_contract::admit_sources(&manifest).map_err(|e| e.message)?;
    if let Some(kind) = omit.strip_prefix("tamper:") {
        let entry = sources.entries.iter().find(|e| e.kind == kind).unwrap();
        std::fs::write(&entry.path, b"{}").unwrap();
    }
    CoastalPolicy::load(&sources, &BBOX).map_err(|e| e.message)
}
fn capture(policy: &CoastalPolicy, value: f64) -> Result<CoastalSnapshot, String> {
    let [w, h] = crate::coastal_geometry::Frame::new(BBOX).unwrap().dims;
    let mut heights = vec![vec![0.; w]; h];
    heights[h / 2][w / 3] = value;
    heights[h / 2][w - 1] = 90.;
    policy.capture(&heights, &vec![vec![80; w]; h], &BBOX)
}
#[test]
fn coastal_v2_authenticated_intersection_normalizes_high_dem_preserves_dry() {
    let p = load(osm(), classification(), area(0., 0.0001), "").unwrap();
    let s = capture(&p, 100.).unwrap();
    let [w, h] = crate::coastal_geometry::Frame::new(BBOX).unwrap().dims;
    let mut heights = vec![vec![999.; w]; h];
    let mut mask = vec![vec![80; w]; h];
    s.restore(&mut heights, &mut mask);
    assert_eq!(heights[h / 2][w / 3], 0.);
    assert_eq!(heights[h / 2][w - 1], 90.);
    assert_eq!(mask[h / 2][w - 1], 0);
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(capture(&p, v).is_err());
    }
}
#[test]
fn coastal_v2_requires_sources_even_without_domains_and_parse_is_strict() {
    for kind in ["osm", "coastal_geometry"] {
        let mut c = classification();
        c["coastal_domains"] = json!([]);
        c["sources"]
            .as_array_mut()
            .unwrap()
            .retain(|s| s["kind"] != kind);
        assert!(load(osm(), c, area(0., 0.0001), kind).is_err());
    }
    let p = CoastalPolicy::parse(
        &serde_json::to_vec(&classification()).unwrap(),
        &BBOX,
        &[
            ("osm", "master-osm"),
            ("coastal_geometry", "master-ocean-geometry"),
        ],
    )
    .unwrap();
    assert!(capture(&p, 100.).is_err());
    let mut c = classification();
    c["policy"] = json!("master-coastal-water-v1");
    assert!(capture(&load(osm(), c, area(0., 0.0001), "").unwrap(), 100.).is_err());
}
#[test]
fn coastal_v2_missing_corroboration_and_incomplete_relations_reject() {
    let mut cases = vec![json!({"elements":[]})];
    let mut open = osm();
    open["elements"][4]["nodes"] = json!([1, 2, 3, 4]);
    cases.push(open);
    let mut missing = osm();
    missing["elements"].as_array_mut().unwrap().remove(0);
    cases.push(missing);
    let mut duplicate = osm();
    duplicate["elements"]
        .as_array_mut()
        .unwrap()
        .push(osm()["elements"][0].clone());
    cases.push(duplicate);
    for member in [
        json!({"type":"way","ref":99,"role":"inner"}),
        json!({"type":"node","ref":1,"role":"inner"}),
        json!({"type":"way","ref":10,"role":"unsupported"}),
    ] {
        let mut relation = osm();
        relation["elements"].as_array_mut().unwrap().push(json!({"type":"relation","id":20,"tags":{"type":"multipolygon","natural":"water"},"members":[{"type":"way","ref":10,"role":"outer"},member]}));
        cases.push(relation);
    }
    for value in cases {
        if let Ok(p) = load(value.clone(), classification(), area(0., 0.0001), "") {
            assert!(capture(&p, 100.).is_err(), "{value}");
        }
    }
    let p = load(osm(), classification(), area(0.00007, 0.0001), "").unwrap();
    assert!(capture(&p, 100.).is_err());
}
#[test]
fn coastal_v2_relation_node_stitching_holes_and_tagged_outer() {
    let mut value = osm();
    value["elements"][4]["nodes"] = json!([1, 2, 3]);
    value["elements"].as_array_mut().unwrap().extend([
        json!({"type":"way","id":11,"nodes":[1,4,3]}),
        json!({"type":"relation","id":20,"tags":{"type":"multipolygon","natural":"bay"},"members":[{"type":"way","ref":10,"role":"outer"},{"type":"way","ref":11,"role":"outer"}]})]);
    assert!(capture(
        &load(value, classification(), area(0., 0.0001), "").unwrap(),
        100.
    )
    .is_ok());
    let mut value = osm();
    for (id, lon, lat) in [
        (5, 0.00002, 0.00002),
        (6, 0.00005, 0.00002),
        (7, 0.00005, 0.00008),
        (8, 0.00002, 0.00008),
    ] {
        value["elements"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"node","id":id,"lon":lon,"lat":lat}));
    }
    value["elements"].as_array_mut().unwrap().extend([json!({"type":"way","id":11,"nodes":[5,6,7,8,5],"tags":{"natural":"water"}}),json!({"type":"relation","id":20,"tags":{"type":"multipolygon","natural":"water"},"members":[{"type":"way","ref":10,"role":"outer"},{"type":"way","ref":11,"role":"inner"}]})]);
    assert!(capture(
        &load(value, classification(), area(0., 0.0001), "").unwrap(),
        100.
    )
    .is_err());
}
#[test]
fn coastal_v2_osm_server_remark_cannot_corroborate() {
    let mut value = osm();
    value["remark"] = json!("runtime error: incomplete query");
    assert!(load(value, classification(), area(0., 0.0001), "").is_err());
}
#[test]
fn coastal_v2_strict_geometry_adversaries() {
    let mut cases = vec![];
    let mut same_coordinate = osm();
    same_coordinate["elements"][4]["nodes"] = json!([1, 2, 3, 4, 5]);
    same_coordinate["elements"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"node","id":5,"lon":-0.001,"lat":-0.001}));
    cases.push(same_coordinate);
    let mut bowtie = osm();
    bowtie["elements"][4]["nodes"] = json!([1, 3, 2, 4, 1]);
    cases.push(bowtie);
    let mut out_of_range = osm();
    out_of_range["elements"][0]["lon"] = json!(-181.);
    cases.push(out_of_range);
    for tags in [
        json!({"natural":"water","type":"boundary"}),
        json!({"natural":"water","type":"multipolygon"}),
    ] {
        let mut value = osm();
        value["elements"].as_array_mut().unwrap().push(json!({"type":"relation","id":20,"tags":tags,"members":[{"type":"way","ref":10,"role":"inner"}]}));
        cases.push(value);
    }
    // Conflicting duplicate relation IDs and repeated members fail safely.
    let relation = json!({"type":"relation","id":20,"tags":{"natural":"water","type":"multipolygon"},"members":[{"type":"way","ref":10,"role":"outer"},{"type":"way","ref":10,"role":"outer"}]});
    let mut value = osm();
    value["elements"]
        .as_array_mut()
        .unwrap()
        .push(relation.clone());
    cases.push(value.clone());
    value["elements"].as_array_mut().unwrap().push(relation);
    cases.push(value);
    for value in cases {
        if let Ok(p) = load(value.clone(), classification(), area(0., 0.0001), "") {
            assert!(capture(&p, 100.).is_err(), "{value}");
        }
    }
    // A mapped boundary alone is insufficient, even inside the ocean.
    let frame = crate::coastal_geometry::Frame::new(BBOX).unwrap();
    let x = frame.point(frame.dims[0] / 3, frame.dims[1] / 2).x();
    let mut value = osm();
    value["elements"][0]["lon"] = json!(x);
    value["elements"][3]["lon"] = json!(x);
    assert!(capture(
        &load(value, classification(), area(0., 0.0001), "").unwrap(),
        100.
    )
    .is_err());
    // An ocean hole also removes corroboration without changing classification.
    let mut ocean = area(0., 0.0001);
    ocean["coordinates"][0].as_array_mut().unwrap().push(json!([
        [0.00002, 0.00002],
        [0.00005, 0.00002],
        [0.00005, 0.00008],
        [0.00002, 0.00008],
        [0.00002, 0.00002]
    ]));
    assert!(capture(&load(osm(), classification(), ocean, "").unwrap(), 100.).is_err());
}
#[test]
fn coastal_v2_missing_references_and_inland_preservation() {
    for kind in ["osm", "coastal_geometry"] {
        let mut c = classification();
        c["sources"]
            .as_array_mut()
            .unwrap()
            .retain(|s| s["kind"] != kind);
        assert!(load(osm(), c, area(0., 0.0001), "").is_err());
    }
    let mut c = classification();
    c["coastal_domains"][0]["domain"] = area(0., 0.00008);
    let p = load(osm(), c, area(0., 0.0001), "").unwrap();
    let s = capture(&p, 100.).unwrap();
    let [w, h] = crate::coastal_geometry::Frame::new(BBOX).unwrap().dims;
    let mut heights = vec![vec![42.; w]; h];
    let mut mask = vec![vec![50; w]; h];
    s.restore(&mut heights, &mut mask);
    assert_eq!(heights[h / 2][w - 1], 42.);
    assert_eq!(mask[h / 2][w - 1], 50);
}

#[test]
#[ignore = "requires retained source manifest and diagnostic conflicts; geometric audit only, not v2 source admission"]
fn coastal_v2_retained_conflicts_have_strict_geographic_corroboration() {
    use sha2::{Digest, Sha256};
    let manifest = std::path::PathBuf::from(
        std::env::var("ARNIS27_DIAGNOSTIC_SOURCES").expect("retained manifest"),
    );
    let bytes = std::fs::read(&manifest).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "f55f3a6e648d75043709ec0de87915833dbd31ed577979e14ba11744197f1067"
    );
    let doc: Value = serde_json::from_slice(&bytes).unwrap();
    // Authenticate unchanged old-source bytes for a geometric characterization only.
    // Do not admit or relabel the old profile as a v2 source bundle.
    let source = |kind: &str| {
        let entry = doc["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["kind"] == kind)
            .unwrap();
        let bytes = std::fs::read(
            manifest
                .parent()
                .unwrap()
                .join(entry["path"].as_str().unwrap()),
        )
        .unwrap();
        assert_eq!(bytes.len() as u64, entry["size_bytes"].as_u64().unwrap());
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            entry["sha256"].as_str().unwrap()
        );
        bytes
    };
    let ocean: Value = serde_json::from_slice(&source("coastal_geometry")).unwrap();
    let bbox: [f64; 4] = serde_json::from_value(ocean["bbox"].clone()).unwrap();
    let geometry: Geometry = serde_json::from_value(ocean["water"].clone()).unwrap();
    let water = geometry
        .validate_limit(&bbox, &mut 0, true, 1_000_000)
        .unwrap();
    let c = corroboration::Corroboration::from_authenticated(&source("osm"), water).unwrap();
    let diagnosis: Value = serde_json::from_slice(
        &std::fs::read(
            std::env::var("ARNIS27_DIAGNOSTIC_CONFLICTS").expect("diagnostic conflicts"),
        )
        .unwrap(),
    )
    .unwrap();
    let conflicts = diagnosis["conflicts"].as_array().unwrap();
    assert_eq!(conflicts.len(), 11_550);
    for point in conflicts {
        assert!(
            c.covers(
                point["lon"].as_f64().unwrap(),
                point["lat"].as_f64().unwrap()
            ),
            "{point}"
        );
    }
    println!("All 11,550 retained conflicts covered by strict raw OSM geometry and authenticated ocean; old bundle remains unmodified and unadmitted as v2.");
}

#[test]
fn coastal_v2_reauthenticates_each_consumed_source() {
    for kind in ["osm", "coastal_geometry", "water_classification"] {
        assert!(load(
            osm(),
            classification(),
            area(0., 0.0001),
            &format!("tamper:{kind}")
        )
        .is_err());
    }
}
