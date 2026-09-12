//! Sample footprints: union integer-grid runs before geographical conversion.
use crate::coastal::Geometry;
use crate::coastal_geometry::Frame;
use geo::{BooleanOps, Intersects, LineString, MultiPolygon, Polygon, Relate};

pub(crate) fn geometry(value: &MultiPolygon) -> Geometry {
    Geometry {
        kind: "MultiPolygon".into(),
        coordinates: value
            .0
            .iter()
            .map(|p| {
                std::iter::once(p.exterior())
                    .chain(p.interiors())
                    .map(|r| r.0.iter().map(|c| [c.x, c.y]).collect())
                    .collect()
            })
            .collect(),
    }
}
pub(crate) fn footprints(
    frame: Frame,
    classes: &[u8],
    water_only: bool,
    checkpoint: impl Fn() -> Result<(), String>,
) -> Result<MultiPolygon, String> {
    let [w, h] = frame.dims;
    let mut rectangles = Vec::new();
    for z in 0..h {
        checkpoint()?;
        let mut x = 0;
        while x < w {
            let included = |x: usize| {
                if water_only {
                    classes[z * w + x] == 2
                } else {
                    classes[z * w + x] != 0
                }
            };
            if !included(x) {
                x += 1;
                continue;
            }
            let start = x;
            while x < w && included(x) {
                x += 1;
            }
            if rectangles.len() >= 200_000 {
                return Err("coastal_capacity: intermediate edges exceed 1000000".into());
            }
            let left = (2 * start).saturating_sub(1) as f64;
            let right = (2 * x - 1).min(2 * (w - 1)) as f64;
            let top = (2 * z).saturating_sub(1) as f64;
            let bottom = (2 * z + 1).min(2 * (h - 1)) as f64;
            rectangles.push(Polygon::new(
                LineString::from(vec![
                    (left, top),
                    (right, top),
                    (right, bottom),
                    (left, bottom),
                    (left, top),
                ]),
                vec![],
            ));
        }
    }
    checkpoint()?;
    let merged = geo::algorithm::bool_ops::unary_union(rectangles.iter());
    checkpoint()?;
    let convert = |ring: &LineString| {
        LineString::from(
            ring.0
                .iter()
                .map(|c| {
                    (
                        frame.bbox[1]
                            + c.x * (frame.bbox[3] - frame.bbox[1]) / (2 * (w - 1)) as f64,
                        frame.bbox[2]
                            - c.y * (frame.bbox[2] - frame.bbox[0]) / (2 * (h - 1)) as f64,
                    )
                })
                .collect::<Vec<_>>(),
        )
    };
    Ok(MultiPolygon(
        merged
            .0
            .iter()
            .map(|p| {
                Polygon::new(
                    convert(p.exterior()),
                    p.interiors().iter().map(convert).collect(),
                )
            })
            .collect(),
    ))
}
pub(crate) fn document(
    frame: Frame,
    water: &MultiPolygon,
    classes: &[u8],
    checkpoint: impl Fn() -> Result<(), String>,
) -> Result<(Vec<u8>, &'static str), String> {
    let (domains, mode) = if water.0.is_empty() {
        (vec![], "complete-query-no-ocean")
    } else {
        let wet = if classes.contains(&2) {
            footprints(frame, classes, true, &checkpoint)?
        } else {
            water.clone()
        };
        let mut domain = footprints(frame, classes, false, &checkpoint)?;
        let mode = if classes.contains(&2) {
            "sample-footprints"
        } else {
            // The overlay backend quantizes float coordinates, even for disjoint
            // input. Preserve literal ocean vertices whenever no overlay is needed.
            if !domain.intersects(&wet) {
                domain.0.extend(wet.0.iter().cloned());
            } else if !wet.relate(&domain).is_coveredby() {
                domain = domain.union(&wet);
            }
            "literal-ocean-no-wet-sample"
        };
        (
            vec![
                serde_json::json!({"id":"master-source-coast","domain":geometry(&domain),"water":geometry(&wet),"inland_exclusions":{"type":"MultiPolygon","coordinates":[]}}),
            ],
            mode,
        )
    };
    checkpoint()?;
    let bytes=serde_json::to_vec(&serde_json::json!({"schema_version":1,"policy":"master-coastal-water-v2","bbox":frame.bbox,"default_classification":"inland","sources":[{"kind":"osm","key":"master-osm"},{"kind":"coastal_geometry","key":crate::coastal_geometry::KEY}],"coastal_domains":domains})).map_err(|e|e.to_string())?;
    if bytes.len() > 16777216 {
        return Err("coastal_capacity: classification exceeds 16 MiB".into());
    }
    let policy = crate::coastal::CoastalPolicy::parse(
        &bytes,
        &frame.bbox,
        &[
            ("osm", "master-osm"),
            ("coastal_geometry", crate::coastal_geometry::KEY),
        ],
    )?;
    policy.verify_samples(&frame.bbox, frame.dims, classes, checkpoint)?;
    Ok((bytes, mode))
}
