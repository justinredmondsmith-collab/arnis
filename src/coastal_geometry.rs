//! Authenticated, unsimplified ocean input and exact master sample frame.
use crate::coastal::Geometry;
use crate::coordinate_system::geographic::LLBBox;
use serde::Deserialize;

pub(crate) const KEY: &str = "master-ocean-geometry";
pub(crate) const MAX_BYTES: u64 = 64 * 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Frame {
    pub bbox: [f64; 4],
    pub dims: [usize; 2],
}
impl Frame {
    pub(crate) fn new(bbox: [f64; 4]) -> Result<Self, String> {
        if !bbox.iter().all(|v| v.is_finite()) {
            return Err("coastal_input: nonfinite bbox".into());
        }
        let ll = LLBBox::new(bbox[0], bbox[1], bbox[2], bbox[3])?;
        let (ww, wh, w, h) = crate::elevation::compute_grid_dims(&ll, 1.);
        if (ww, wh) != (w, h)
            || !(2..=16384).contains(&w)
            || !(2..=16384).contains(&h)
            || w.checked_mul(h).is_none_or(|n| n > 16777216)
        {
            return Err(
                "coastal_capacity: master dimensions exceed profile; bbox was not changed".into(),
            );
        }
        Ok(Self { bbox, dims: [w, h] })
    }
    pub(crate) fn point(&self, x: usize, z: usize) -> geo::Point {
        geo::Point::new(
            self.bbox[1] + x as f64 * (self.bbox[3] - self.bbox[1]) / (self.dims[0] - 1) as f64,
            self.bbox[2] - z as f64 * (self.bbox[2] - self.bbox[0]) / (self.dims[1] - 1) as f64,
        )
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    schema_version: u32,
    policy: String,
    profile_sha256: String,
    bbox: [f64; 4],
    grid_dimensions: [usize; 2],
    water: Geometry,
    dataset: Dataset,
    complete_dataset_scan: bool,
    selected_features: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Dataset {
    url: String,
    archive_sha256: String,
    projection: String,
    parts: Parts,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Parts {
    #[serde(rename = "ocean.shp")]
    shp: Part,
    #[serde(rename = "ocean.shx")]
    shx: Part,
    #[serde(rename = "ocean.prj")]
    prj: Part,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Part {
    sha256: String,
    size_bytes: u64,
}
fn digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(crate) fn parse(bytes: &[u8], bbox: [f64; 4]) -> Result<(Frame, geo::MultiPolygon), String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err("coastal_capacity: geometry input exceeds 64 MiB".into());
    }
    let e: Envelope = serde_json::from_slice(bytes).map_err(|e| format!("coastal_input: {e}"))?;
    let frame = Frame::new(bbox)?;
    if e.schema_version != 1
        || e.policy != "global-ocean-clip-v1"
        || e.profile_sha256 != crate::tiler_contract::profile_hash()
        || e.bbox != bbox
        || e.grid_dimensions != frame.dims
        || !e.complete_dataset_scan
    {
        return Err(
            "coastal_input: ocean version/policy/profile/frame/complete-scan mismatch".into(),
        );
    }
    if e.dataset.url != "https://osmdata.openstreetmap.de/download/water-polygons-split-4326.zip"
        || e.dataset.projection != "EPSG:4326"
        || !digest(&e.dataset.archive_sha256)
        || [
            &e.dataset.parts.shp,
            &e.dataset.parts.shx,
            &e.dataset.parts.prj,
        ]
        .iter()
        .any(|p| !digest(&p.sha256) || p.size_bytes == 0)
    {
        return Err("coastal_input: ocean dataset provenance mismatch".into());
    }
    let water = e.water.validate_limit(&bbox, &mut 0, true, 1_000_000)?;
    if water.0.is_empty() != (e.selected_features == 0) {
        return Err("coastal_input: selected ocean features and polygon coverage disagree".into());
    }
    Ok((frame, water))
}
