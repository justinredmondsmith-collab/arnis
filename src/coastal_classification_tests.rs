use super::*;
#[test]
fn descriptor_is_separate_and_offline() {
    let d = descriptor();
    assert_eq!(d["network"], "none");
    assert_eq!(d["limits"]["osm_bytes"], 536870912_u64);
    assert_eq!(d["limits"]["memory_bytes"], 8589934592_u64);
    assert_eq!(
        crate::tiler_contract::capability_report()["capabilities"]
            .as_array()
            .unwrap()
            .len(),
        9
    );
    assert!(run_command(&["--tiler-capabilities".into()]).is_none());
    assert!(run_command(&["--describe-coastal-classification=1".into()])
        .unwrap()
        .is_err());
    assert!(run_command(&["--produce-coastal-classification".into()])
        .unwrap()
        .is_err());
}
fn ocean_document(bbox: [f64; 4]) -> serde_json::Value {
    let ll = LLBBox::new(bbox[0], bbox[1], bbox[2], bbox[3]).unwrap();
    let (w, h, _, _) = crate::elevation::compute_grid_dims(&ll, 1.);
    serde_json::json!({"schema_version":1,"policy":"global-ocean-clip-v1",
        "profile_sha256":crate::tiler_contract::profile_hash(),"bbox":bbox,"grid_dimensions":[w,h],
        "water":{"type":"MultiPolygon","coordinates":[[[[bbox[1],bbox[0]],[bbox[1]+(bbox[3]-bbox[1])/3.,bbox[0]],[bbox[1]+(bbox[3]-bbox[1])/3.,bbox[2]],[bbox[1],bbox[2]],[bbox[1],bbox[0]]]]]},
        "dataset":{"url":"https://osmdata.openstreetmap.de/download/water-polygons-split-4326.zip","archive_sha256":"a".repeat(64),"projection":"EPSG:4326","parts":{
            "ocean.shp":{"sha256":"b".repeat(64),"size_bytes":1},"ocean.shx":{"sha256":"c".repeat(64),"size_bytes":1},"ocean.prj":{"sha256":"d".repeat(64),"size_bytes":1}}},
        "complete_dataset_scan":true,"selected_features":1})
}
fn inputs(ocean: &serde_json::Value, osm: &serde_json::Value) -> tempfile::TempDir {
    use sha2::{Digest, Sha256};
    let d = tempfile::tempdir().unwrap();
    let mut entries = vec![];
    for (kind, key, name, value) in [
        ("osm", "master-osm", "osm.json", osm),
        (
            "coastal_geometry",
            "master-ocean-geometry",
            "ocean.json",
            ocean,
        ),
    ] {
        let b = serde_json::to_vec(value).unwrap();
        std::fs::write(d.path().join(name), &b).unwrap();
        entries.push(serde_json::json!({"kind":kind,"key":key,"path":name,"sha256":format!("{:x}",Sha256::digest(&b)),"size_bytes":b.len()}));
    }
    std::fs::write(d.path().join("sources.json"),serde_json::to_vec(&serde_json::json!({"schema_version":1,"profile_sha256":crate::tiler_contract::profile_hash(),"entries":entries})).unwrap()).unwrap();
    d
}
fn run_fixture(d: &tempfile::TempDir, bbox: [f64; 4]) -> Result<(), String> {
    run_command(&[
        "--produce-coastal-classification".into(),
        "--bbox".into(),
        format!("{},{},{},{}", bbox[0], bbox[1], bbox[2], bbox[3]).into(),
        "--frozen-sources".into(),
        d.path().join("sources.json").into_os_string(),
        "--classification-output".into(),
        d.path().join("result").into_os_string(),
    ])
    .unwrap()
}
#[test]
fn coastal_producer_serializes_verified_ocean_and_rejects_bad_envelopes() {
    let bbox = [-0.0001, -0.0001, 0., 0.];
    let ocean = ocean_document(bbox);
    let d = inputs(&ocean, &serde_json::json!({"elements":[]}));
    run_fixture(&d, bbox).unwrap();
    let policy: serde_json::Value = serde_json::from_slice(
        &std::fs::read(d.path().join("result/water-classification.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(policy["policy"], "master-coastal-water-v2");
    let profile: serde_json::Value =
        serde_json::from_str(include_str!("../docs/contracts/tiler-profile.json")).unwrap();
    assert_eq!(profile["coastal_policy"], "master-coastal-water-v2");
    assert_eq!(
        profile["coastal_dem_corroboration"],
        "complete-osm-water-ocean-intersection-v1"
    );
    let report: serde_json::Value = serde_json::from_slice(
        &std::fs::read(d.path().join("result/classification.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(report["encoding"], "sample-footprints");
    assert!(report["classes"]["wet"].as_u64().unwrap() > 0);
    assert_eq!(report["classes"]["dry"], 0);
    for (field, value) in [
        ("complete_dataset_scan", serde_json::json!(false)),
        ("profile_sha256", serde_json::json!("0".repeat(64))),
        ("grid_dimensions", serde_json::json!([2, 2])),
        ("extra", serde_json::json!(true)),
    ] {
        let mut bad = ocean.clone();
        bad[field] = value;
        let d = inputs(&bad, &serde_json::json!({"elements":[]}));
        assert!(run_fixture(&d, bbox).is_err(), "{field}");
        assert!(!d.path().join("result").exists());
    }
}
fn mapped_way(
    frame: crate::coastal_geometry::Frame,
    points: &[(usize, usize)],
    tags: serde_json::Value,
) -> serde_json::Value {
    let mut elements = vec![];
    let mut ids = vec![];
    for (i, &(x, z)) in points.iter().enumerate() {
        let p = frame.point(x, z);
        let id = if i + 1 == points.len() && points[0] == (x, z) {
            1
        } else {
            i + 1
        };
        ids.push(id);
        if id == i + 1 {
            elements.push(serde_json::json!({"type":"node","id":id,"lat":p.y(),"lon":p.x()}));
        }
    }
    elements.push(serde_json::json!({"type":"way","id":100,"nodes":ids,"tags":tags}));
    serde_json::json!({"elements":elements})
}
#[test]
fn coastal_actual_osm_promenade_building_and_cliff_are_positive_dry_evidence() {
    let bbox = [-0.0001, -0.0001, 0., 0.];
    let frame = crate::coastal_geometry::Frame::new(bbox).unwrap();
    for (points, tags) in [
        (
            vec![(7, 3), (9, 8)],
            serde_json::json!({"highway":"footway"}),
        ),
        (
            vec![(7, 3), (10, 3), (10, 8), (7, 8), (7, 3)],
            serde_json::json!({"building":"yes"}),
        ),
        (vec![(7, 3), (9, 8)], serde_json::json!({"natural":"cliff"})),
    ] {
        let d = inputs(
            &ocean_document(bbox),
            &mapped_way(frame, &points, tags.clone()),
        );
        run_fixture(&d, bbox).unwrap();
        let report: serde_json::Value = serde_json::from_slice(
            &std::fs::read(d.path().join("result/classification.json")).unwrap(),
        )
        .unwrap();
        assert!(
            report["classes"]["dry"].as_u64().unwrap() > 0,
            "missing positive evidence for {tags}"
        );
    }
}
#[test]
fn coastal_source_bound_policy_rejects_shifted_bbox_at_same_dimensions() {
    use sha2::{Digest, Sha256};
    let bbox = [-0.0001, -0.0001, 0., 0.];
    let ocean = ocean_document(bbox);
    let d = inputs(&ocean, &serde_json::json!({"elements":[]}));
    let bytes=serde_json::to_vec(&serde_json::json!({"schema_version":1,"policy":"master-coastal-water-v1","bbox":bbox,"default_classification":"inland","sources":[{"kind":"osm","key":"master-osm"},{"kind":"coastal_geometry","key":"master-ocean-geometry"}],"coastal_domains":[{"id":"ocean","domain":ocean["water"],"water":ocean["water"],"inland_exclusions":{"type":"MultiPolygon","coordinates":[]}}]})).unwrap();
    std::fs::write(d.path().join("classification.json"), &bytes).unwrap();
    let path = d.path().join("sources.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    manifest["entries"].as_array_mut().unwrap().push(serde_json::json!({"kind":"water_classification","key":"master-water-classification","path":"classification.json","sha256":format!("{:x}",Sha256::digest(&bytes)),"size_bytes":bytes.len()}));
    std::fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let sources = crate::tiler_contract::admit_sources(&path).unwrap();
    let policy = crate::coastal::CoastalPolicy::load(&sources, &bbox).unwrap();
    let frame = crate::coastal_geometry::Frame::new(bbox).unwrap();
    let [w, h] = frame.dims;
    assert!(policy
        .capture(&vec![vec![0.; w]; h], &vec![vec![0; w]; h], &bbox)
        .is_ok());
    assert!(policy
        .capture(
            &vec![vec![0.; w]; h],
            &vec![vec![0; w]; h],
            &[bbox[0], bbox[1] + 0.001, bbox[2], bbox[3] + 0.001]
        )
        .is_err());
    assert!(policy
        .capture(&vec![vec![0.; w + 1]; h], &vec![vec![0; w + 1]; h], &bbox)
        .is_err());
    // A bundle carrying ocean evidence cannot omit its reference to evade frame checks.
    let mut document: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    document["sources"] = serde_json::json!([{"kind":"osm","key":"master-osm"}]);
    let changed = serde_json::to_vec(&document).unwrap();
    std::fs::write(d.path().join("classification.json"), &changed).unwrap();
    manifest["entries"][2]["sha256"] = format!("{:x}", Sha256::digest(&changed)).into();
    manifest["entries"][2]["size_bytes"] = changed.len().into();
    std::fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let changed_sources = crate::tiler_contract::admit_sources(&path).unwrap();
    assert!(crate::coastal::CoastalPolicy::load(&changed_sources, &bbox).is_err());
}
fn classify_fixture(
    ocean: &serde_json::Value,
    osm: &serde_json::Value,
) -> (
    crate::coastal_geometry::Frame,
    geo::MultiPolygon,
    Vec<u8>,
    usize,
    usize,
) {
    let bbox: [f64; 4] = serde_json::from_value(ocean["bbox"].clone()).unwrap();
    let (frame, water) =
        crate::coastal_geometry::parse(&serde_json::to_vec(ocean).unwrap(), bbox).unwrap();
    let ll = LLBBox::new(bbox[0], bbox[1], bbox[2], bbox[3]).unwrap();
    let (classes, ways, cells) = classify_inputs(
        serde_json::from_value(osm.clone()).unwrap(),
        ll,
        frame,
        &water,
        &Deadline {
            started: Instant::now(),
            limit: Duration::from_secs(60),
        },
    )
    .unwrap();
    (frame, water, classes, ways, cells)
}
fn assert_serialized(
    frame: crate::coastal_geometry::Frame,
    water: &geo::MultiPolygon,
    classes: &[u8],
) -> Vec<u8> {
    let (bytes, _) =
        crate::coastal_classification_geometry::document(frame, water, classes, || Ok(())).unwrap();
    let policy = crate::coastal::CoastalPolicy::parse(
        &bytes,
        &frame.bbox,
        &[
            ("osm", "master-osm"),
            ("coastal_geometry", "master-ocean-geometry"),
        ],
    )
    .unwrap();
    policy
        .verify_samples(&frame.bbox, frame.dims, classes, || Ok(()))
        .unwrap();
    let [w, h] = frame.dims;
    let mut heights = vec![vec![0.; w]; h];
    for (i, c) in classes.iter().enumerate() {
        heights[i / w][i % w] = if *c == 2 { -4. } else { 37. };
    }
    let snapshot = policy
        .capture(&heights, &vec![vec![80; w]; h], &frame.bbox)
        .unwrap();
    let mut after = vec![vec![999.; w]; h];
    let mut cover = vec![vec![99; w]; h];
    snapshot.restore(&mut after, &mut cover);
    for (i, c) in classes.iter().enumerate() {
        assert_eq!(
            after[i / w][i % w],
            match c {
                2 => 0.,
                1 => 37.,
                _ => 999.,
            }
        );
        assert_eq!(
            cover[i / w][i % w],
            match c {
                2 => 80,
                1 => 0,
                _ => 99,
            }
        );
    }
    bytes
}
#[test]
fn coastal_actual_water_priority_and_overwater_veto_preserve_unknown_inland() {
    let bbox = [-0.0001, -0.0001, 0., 0.];
    let frame = crate::coastal_geometry::Frame::new(bbox).unwrap();
    let ocean = ocean_document(bbox);
    let empty = classify_fixture(&ocean, &serde_json::json!({"elements":[]})).2;
    for tags in [
        serde_json::json!({"highway":"footway","bridge":"yes"}),
        serde_json::json!({"man_made":"pier"}),
        serde_json::json!({"highway":"footway","floating":"yes"}),
        serde_json::json!({"highway":"footway","tunnel":"yes"}),
        serde_json::json!({"waterway":"stream"}),
    ] {
        let (f, w, c, _, _) =
            classify_fixture(&ocean, &mapped_way(frame, &[(0, 5), (11, 5)], tags.clone()));
        assert_eq!(c, empty, "unexpected land from {tags}");
        assert_serialized(f, &w, &c);
    }
    let (f, w, c, _, _) = classify_fixture(
        &ocean,
        &mapped_way(
            frame,
            &[(0, 5), (11, 5)],
            serde_json::json!({"highway":"footway"}),
        ),
    );
    assert!(c.contains(&1));
    for (old, new) in empty.iter().zip(&c) {
        if *old == 2 {
            assert_eq!(*new, 2);
        }
    }
    assert!(c.contains(&0));
    assert_serialized(f, &w, &c);
}
#[test]
fn coastal_sample_footprints_cover_all_edges_holes_and_negative_coordinates() {
    let bbox = [-0.0001, -0.0001, 0., 0.];
    let (f, w, _, _, _) =
        classify_fixture(&ocean_document(bbox), &serde_json::json!({"elements":[]}));
    let [width, height] = f.dims;
    for corner in [0, width - 1, (height - 1) * width, width * height - 1] {
        let mut c = vec![0; width * height];
        c[corner] = 2;
        c[width + 1] = 1;
        let first = assert_serialized(f, &w, &c);
        assert_eq!(first, assert_serialized(f, &w, &c));
    }
    let c = (0..width * height)
        .map(|i| {
            if i / width == 0 || i / width == height - 1 || i % width == 0 || i % width == width - 1
            {
                2
            } else if i % 3 == 0 {
                1
            } else {
                0
            }
        })
        .collect::<Vec<_>>();
    assert_serialized(f, &w, &c);
}
#[test]
fn coastal_subsample_ocean_is_literal_and_complete_empty_ocean_stays_inland() {
    let bbox = [-0.0001, -0.0001, 0., 0.];
    let mut ocean = ocean_document(bbox);
    ocean["water"] = serde_json::json!({"type":"MultiPolygon","coordinates":[[[[-0.000095,-0.000095],[-0.000094,-0.000095],[-0.000094,-0.000094],[-0.000095,-0.000094],[-0.000095,-0.000095]]]]});
    let frame = crate::coastal_geometry::Frame::new(bbox).unwrap();
    let osm = mapped_way(
        frame,
        &[(7, 3), (9, 8)],
        serde_json::json!({"highway":"footway"}),
    );
    let (f, w, c, _, _) = classify_fixture(&ocean, &osm);
    assert!(!c.contains(&2));
    assert!(c.contains(&1));
    let (bytes, mode) =
        crate::coastal_classification_geometry::document(f, &w, &c, || Ok(())).unwrap();
    assert_eq!(mode, "literal-ocean-no-wet-sample");
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["coastal_domains"][0]["water"], ocean["water"]);
    assert_serialized(f, &w, &c);
    ocean["water"]["coordinates"] = serde_json::json!([]);
    ocean["selected_features"] = 0.into();
    let (f, w, c, _, _) = classify_fixture(&ocean, &osm);
    assert!(c.iter().all(|c| *c == 0));
    let (bytes, mode) =
        crate::coastal_classification_geometry::document(f, &w, &c, || Ok(())).unwrap();
    assert_eq!(mode, "complete-query-no-ocean");
    assert!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["coastal_domains"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
fn combine_osm(parts: Vec<serde_json::Value>) -> serde_json::Value {
    let mut elements = vec![];
    for (i, part) in parts.into_iter().enumerate() {
        let shift = (i as u64) * 1000;
        for mut e in part["elements"].as_array().unwrap().clone() {
            e["id"] = (e["id"].as_u64().unwrap() + shift).into();
            if let Some(nodes) = e.get_mut("nodes").and_then(serde_json::Value::as_array_mut) {
                for n in nodes {
                    *n = (n.as_u64().unwrap() + shift).into();
                }
            }
            elements.push(e);
        }
    }
    serde_json::json!({"elements":elements})
}
#[test]
fn coastal_actual_inland_water_and_overwater_masks_veto_real_land() {
    let bbox = [-0.0001, -0.0001, 0., 0.];
    let frame = crate::coastal_geometry::Frame::new(bbox).unwrap();
    let ocean = ocean_document(bbox);
    let road = mapped_way(
        frame,
        &[(5, 5), (11, 5)],
        serde_json::json!({"highway":"footway"}),
    );
    let base = classify_fixture(&ocean, &road).2;
    assert!(base.contains(&1));
    for tag in [
        serde_json::json!({"man_made":"pier"}),
        serde_json::json!({"waterway":"river"}),
        serde_json::json!({"highway":"footway","bridge":"yes"}),
    ] {
        let overlap = mapped_way(frame, &[(5, 5), (11, 5)], tag.clone());
        let (f, w, c, _, _) = classify_fixture(&ocean, &combine_osm(vec![road.clone(), overlap]));
        assert_eq!(c[5 * frame.dims[0] + 8], 0, "overlap must veto {tag}");
        assert_serialized(f, &w, &c);
    }
    let ring = [(5, 2), (11, 2), (11, 10), (5, 10), (5, 2)];
    for kind in ["water", "cliff"] {
        let building = mapped_way(frame, &ring, serde_json::json!({"building":"yes"}));
        let lake = mapped_way(frame, &ring, serde_json::json!({"natural":"water"}));
        let base = if kind == "cliff" {
            mapped_way(frame, &ring, serde_json::json!({"natural":"cliff"}))
        } else {
            building
        };
        let (f, w, c, _, _) = classify_fixture(&ocean, &combine_osm(vec![base, lake]));
        assert_eq!(c[5 * frame.dims[0] + 8], 0);
        assert_serialized(f, &w, &c);
    }
}
#[test]
fn coastal_subsample_ocean_contained_by_dry_footprint_preserves_literal_water() {
    let bbox = [-0.0001, -0.0001, 0., 0.];
    let frame = crate::coastal_geometry::Frame::new(bbox).unwrap();
    let mut ocean = ocean_document(bbox);
    ocean["water"] = serde_json::json!({"type":"MultiPolygon","coordinates":[[[[-0.000031,-0.000051],[-0.000030,-0.000051],[-0.000030,-0.000050],[-0.000031,-0.000050],[-0.000031,-0.000051]]]]});
    let osm = mapped_way(
        frame,
        &[(5, 2), (11, 2), (11, 10), (5, 10), (5, 2)],
        serde_json::json!({"building":"yes"}),
    );
    let (f, w, c, _, _) = classify_fixture(&ocean, &osm);
    assert!(!c.contains(&2));
    assert!(c.contains(&1));
    assert_serialized(f, &w, &c);
}
#[test]
fn coastal_limits_deadline_tamper_and_existing_outputs_fail_without_publication() {
    let bbox = [-0.0001, -0.0001, 0., 0.];
    let ll = LLBBox::new(bbox[0], bbox[1], bbox[2], bbox[3]).unwrap();
    let d = inputs(&ocean_document(bbox), &serde_json::json!({"elements":[]}));
    assert!(produce(
        &ll,
        &d.path().join("sources.json"),
        &d.path().join("result"),
        Duration::ZERO
    )
    .unwrap_err()
    .contains("deadline"));
    assert!(!d.path().join("result").exists());
    std::fs::write(d.path().join("ocean.json"), b"{}").unwrap();
    assert!(run_fixture(&d, bbox).is_err());
    assert!(!d.path().join("result").exists());
    std::fs::create_dir(d.path().join("result")).unwrap();
    std::fs::write(d.path().join("result/keep"), b"keep").unwrap();
    assert!(run_fixture(&d, bbox).is_err());
    assert_eq!(
        std::fs::read(d.path().join("result/keep")).unwrap(),
        b"keep"
    );
    assert!(crate::coastal_geometry::Frame::new([0., 0., 1., 1.])
        .unwrap_err()
        .contains("capacity"));
    let frame = crate::coastal_geometry::Frame {
        bbox: [0., 0., 1., 1.],
        dims: [1000, 500],
    };
    let classes = (0..500000)
        .map(|i| if (i / 1000 + i % 1000) % 2 == 0 { 2 } else { 0 })
        .collect::<Vec<_>>();
    assert!(
        crate::coastal_classification_geometry::footprints(frame, &classes, true, || Ok(()))
            .unwrap_err()
            .contains("capacity")
    );
}
#[cfg(unix)]
#[test]
fn coastal_destination_symlink_is_not_followed() {
    let bbox = [-0.0001, -0.0001, 0., 0.];
    let d = inputs(&ocean_document(bbox), &serde_json::json!({"elements":[]}));
    std::os::unix::fs::symlink(d.path().join("absent"), d.path().join("result")).unwrap();
    assert!(run_fixture(&d, bbox).is_err());
    assert!(!d.path().join("absent").exists());
}
#[test]
fn coastal_actual_relation_cliff_matches_outer_way_and_inland_holes_remain_unknown() {
    let bbox = [-0.0001, -0.0001, 0., 0.];
    let frame = crate::coastal_geometry::Frame::new(bbox).unwrap();
    let ocean = ocean_document(bbox);
    let mut way = mapped_way(
        frame,
        &[(7, 2), (10, 2), (10, 9), (7, 9), (7, 2)],
        serde_json::json!({"natural":"cliff"}),
    );
    let direct = classify_fixture(&ocean, &way);
    let elems = way["elements"].as_array_mut().unwrap();
    elems.last_mut().unwrap()["tags"] = serde_json::json!({});
    elems.push(serde_json::json!({"type":"relation","id":500,"tags":{"type":"multipolygon","natural":"cliff"},"members":[{"type":"way","ref":100,"role":"outer"}]}));
    let (f, w, c, ways, cells) = classify_fixture(&ocean, &way);
    assert_eq!(c, direct.2);
    assert!(ways > 0 && cells > 0);
    assert_serialized(f, &w, &c);
    let mut with_hole = ocean.clone();
    with_hole["water"]["coordinates"][0]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!([
            [-0.000095, -0.00007],
            [-0.000095, -0.00003],
            [-0.000075, -0.00003],
            [-0.000075, -0.00007],
            [-0.000095, -0.00007]
        ]));
    let (f, w, c, _, _) = classify_fixture(&with_hole, &serde_json::json!({"elements":[]}));
    assert_eq!(c[5 * frame.dims[0] + 1], 0);
    assert_eq!(c[0], 2);
    assert_serialized(f, &w, &c);
}
#[test]
fn coastal_replay_checks_deadline_and_wrong_expected_samples() {
    let bbox = [-0.0001, -0.0001, 0., 0.];
    let (f, w, c, _, _) =
        classify_fixture(&ocean_document(bbox), &serde_json::json!({"elements":[]}));
    let bytes = assert_serialized(f, &w, &c);
    let p = crate::coastal::CoastalPolicy::parse(
        &bytes,
        &bbox,
        &[
            ("osm", "master-osm"),
            ("coastal_geometry", "master-ocean-geometry"),
        ],
    )
    .unwrap();
    let checks = std::cell::Cell::new(0);
    assert!(p
        .verify_samples(&bbox, f.dims, &c, || {
            checks.set(checks.get() + 1);
            if checks.get() == 3 {
                Err("deadline fixture".into())
            } else {
                Ok(())
            }
        })
        .unwrap_err()
        .contains("deadline fixture"));
    assert_eq!(checks.get(), 3);
    let mut wrong = c;
    wrong[0] = 0;
    assert!(p
        .verify_samples(&bbox, f.dims, &wrong, || Ok(()))
        .unwrap_err()
        .contains("sample mismatch"));
}
#[test]
fn coastal_publication_durability_failure_retains_verified_path() {
    let d = tempfile::tempdir().unwrap();
    let stage = d.path().join("stage");
    std::fs::create_dir(&stage).unwrap();
    std::fs::write(stage.join("verified"), b"content").unwrap();
    let out = d.path().join("out");
    let result = publish_verified(&stage, &out, || {
        Err(std::io::Error::other("injected fsync failure"))
    });
    assert!(result.unwrap_err().contains("coastal_publish_durability"));
    assert_eq!(std::fs::read(out.join("verified")).unwrap(), b"content");
}
#[test]
fn coastal_admission_rejects_required_source_sizes_before_opening() {
    let bbox = [-0.0001, -0.0001, 0., 0.];
    for (index, size) in [(0, 536870913_u64), (1, 67108865)] {
        let d = inputs(&ocean_document(bbox), &serde_json::json!({"elements":[]}));
        let path = d.path().join("sources.json");
        let mut v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        v["entries"][index]["size_bytes"] = size.into();
        v["entries"][index]["path"] = "must-not-open".into();
        std::fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
        assert!(run_fixture(&d, bbox)
            .unwrap_err()
            .contains("coastal_capacity"));
        assert!(!d.path().join("result").exists());
    }
}
#[test]
fn coastal_admission_checks_deadline_between_source_hash_chunks() {
    let (d, _) = crate::tiler_contract::frozen_tests::fixture(
        "elevation",
        "fixture-extra-role",
        &vec![42; 262144],
    );
    let calls = std::cell::Cell::new(0);
    let result = crate::tiler_contract::admit_sources_with_checks(
        &d.path().join("manifest.json"),
        |_| Ok(()),
        || {
            calls.set(calls.get() + 1);
            if calls.get() == 6 {
                Err(crate::tiler_contract::ContractError {
                    exit_code: 1,
                    message: "injected chunk deadline".into(),
                })
            } else {
                Ok(())
            }
        },
    );
    assert!(result
        .unwrap_err()
        .message
        .contains("injected chunk deadline"));
    assert_eq!(calls.get(), 6);
}
