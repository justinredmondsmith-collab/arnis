//! Frozen source-aware classification, used only while producing a master.
#[cfg(test)]
mod tests {
    use super::*;
    fn rectangle(w: f64, s: f64, e: f64, n: f64) -> serde_json::Value {
        serde_json::json!({"type":"MultiPolygon","coordinates":[[[[w,s],[e,s],[e,n],[w,n],[w,s]]]]})
    }
    fn fixture() -> serde_json::Value {
        serde_json::json!({"schema_version":1,"policy":"master-coastal-water-v1","bbox":[0.,0.,10.,10.],"default_classification":"inland","sources":[{"kind":"osm","key":"master-osm"}],"coastal_domains":[{"id":"harbor","domain":rectangle(0.,0.,8.,10.),"water":rectangle(0.,0.,3.,10.),"inland_exclusions":rectangle(5.,2.,7.,4.)}]})
    }
    fn parse(value: serde_json::Value) -> Result<CoastalPolicy, String> {
        CoastalPolicy::parse(
            &serde_json::to_vec(&value).unwrap(),
            &[0., 0., 10., 10.],
            &[("osm", "master-osm")],
        )
    }
    #[test]
    fn coastal_harbor_dry_cliff_and_inland_exclusion_are_explicit() {
        let policy = parse(fixture()).unwrap();
        assert_eq!(policy.classify(1., 5.), 2);
        assert_eq!(policy.classify(4., 5.), 1);
        assert_eq!(policy.classify(6., 3.), 0);
        assert_eq!(policy.classify(9., 5.), 0);
    }
    #[test]
    fn coastal_rejects_geometry_and_source_errors() {
        for field in ["domain", "water", "inland_exclusions"] {
            let mut value = fixture();
            value["coastal_domains"][0][field]["coordinates"][0][0][4] =
                serde_json::json!([1., 1.]);
            assert!(parse(value).is_err(), "unclosed {field}");
        }
        let mut value = fixture();
        value["sources"][0]["key"] = "missing".into();
        assert!(parse(value).is_err());
        let mut value = fixture();
        value["coastal_domains"][0]["water"] = rectangle(7., 0., 9., 5.);
        assert!(parse(value).is_err());
        let mut value = fixture();
        value["coastal_domains"][0]["inland_exclusions"] = rectangle(1., 1., 2., 2.);
        assert!(parse(value).is_err());
        let mut value = fixture();
        let domain = value["coastal_domains"][0].clone();
        value["coastal_domains"]
            .as_array_mut()
            .unwrap()
            .push(domain);
        assert!(parse(value).is_err());
    }
    #[test]
    fn coastal_restores_dry_false_positive_and_preserves_inland_levels() {
        let policy = parse(fixture()).unwrap();
        let mut heights = vec![vec![1., 1., 45., 32., 91., 70.]; 6];
        let mut lc = vec![vec![80, 80, 80, 50, 80, 80]; 6];
        let snapshot = policy.capture(&heights, &lc, &[0., 0., 10., 10.]).unwrap();
        heights[2] = vec![9.; 6];
        lc[2] = vec![10; 6];
        snapshot.restore(&mut heights, &mut lc);
        assert_eq!(heights[2], vec![0., 0., 45., 32., 91., 9.]);
        assert_eq!(lc[2], vec![80, 80, 0, 50, 0, 10]);
        // Exclusion (longitude 6, latitude 4) and inland lake remain independently processed.
        assert_eq!(heights[3][3], 32.);
        assert_eq!(heights[3][5], 70.);
    }
    #[test]
    fn coastal_wet_dem_conflict_checked_before_any_leveling() {
        let policy = parse(fixture()).unwrap();
        let mut heights = vec![vec![0.; 6]; 6];
        heights[2][1] = 2.01;
        let error = policy
            .capture(&heights, &vec![vec![80; 6]; 6], &[0., 0., 10., 10.])
            .err()
            .unwrap();
        assert!(error.contains("2") && error.contains("6"), "{error}");
        heights[2][1] = 2.;
        assert!(policy
            .capture(&heights, &vec![vec![80; 6]; 6], &[0., 0., 10., 10.])
            .is_ok());
    }
    #[test]
    fn coastal_negative_wet_dem_is_accepted_and_restored_to_sea_level() {
        let policy = parse(fixture()).unwrap();
        let mut heights = vec![vec![0.; 6]; 6];
        let mut mask = vec![vec![80; 6]; 6];
        for depth in [-2.01, -25., -1000.] {
            heights[2][1] = depth;
            let snapshot = policy
                .capture(&heights, &mask, &[0., 0., 10., 10.])
                .unwrap_or_else(|error| panic!("negative wet DEM {depth}m rejected: {error}"));
            snapshot.restore(&mut heights, &mut mask);
            assert_eq!(heights[2][1], 0.);
            assert_eq!(mask[2][1], crate::land_cover::LC_WATER);
        }
    }
    #[test]
    fn coastal_strict_schema_limits_and_holes() {
        let bytes = serde_json::to_vec(&fixture()).unwrap();
        let duplicate =
            String::from_utf8(bytes.clone())
                .unwrap()
                .replacen("{", "{\"schema_version\":1,", 1);
        assert!(CoastalPolicy::parse(
            duplicate.as_bytes(),
            &[0., 0., 10., 10.],
            &[("osm", "master-osm")]
        )
        .is_err());
        assert!(
            CoastalPolicy::parse(&bytes, &[0., 0., 9., 10.], &[("osm", "master-osm")]).is_err()
        );
        let mut value = fixture();
        value["extra"] = true.into();
        assert!(parse(value).is_err());
        let mut value = fixture();
        value["coastal_domains"][0]["water"]["type"] = "Polygon".into();
        assert!(parse(value).is_err());
        let mut value = fixture();
        value["coastal_domains"][0]["water"]["coordinates"][0][0] =
            serde_json::json!([[0., 0.], [2., 2.], [0., 2.], [2., 0.], [0., 0.]]);
        assert!(parse(value).is_err());
        let mut value = fixture();
        value["coastal_domains"][0]["water"]["coordinates"][0]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!([
                [0.5, 1.],
                [0.5, 2.],
                [1.5, 2.],
                [1.5, 1.],
                [0.5, 1.]
            ]));
        let policy = parse(value).unwrap();
        assert_eq!(policy.classify(1., 1.5), 1);
        let mut value = fixture();
        value["sources"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"kind":"osm","key":"master-osm"}));
        assert!(parse(value).is_err());
        let mut value = fixture();
        value["coastal_domains"][0]["inland_exclusions"] =
            serde_json::json!({"type":"MultiPolygon","coordinates":[]});
        assert!(parse(value).is_ok());
        let mut value = fixture();
        value["coastal_domains"][0]["water"] =
            serde_json::json!({"type":"MultiPolygon","coordinates":[]});
        assert!(parse(value).is_err());
        assert!(CoastalPolicy::parse(
            &vec![b' '; 16_777_217],
            &[0., 0., 10., 10.],
            &[("osm", "master-osm")]
        )
        .is_err());
        let mut value = fixture();
        value["coastal_domains"] =
            serde_json::Value::Array(vec![value["coastal_domains"][0].clone(); 1025]);
        assert!(parse(value).is_err());
        let mut value = fixture();
        value["coastal_domains"][0]["domain"]["coordinates"][0][0] =
            serde_json::Value::Array(vec![serde_json::json!([0., 0.]); 100001]);
        assert!(parse(value).is_err());
    }
    #[test]
    fn coastal_protection_reasserts_saved_y_and_mask_after_later_repairs() {
        let policy = parse(fixture()).unwrap();
        let mut meters = vec![vec![1., 1., 45., 32., 91., 70.]; 6];
        let mut mask = vec![vec![80, 80, 80, 50, 80, 80]; 6];
        let snapshot = policy.capture(&meters, &mask, &[0., 0., 10., 10.]).unwrap();
        snapshot.restore(&mut meters, &mut mask);
        let (scaled, _, _, _) =
            crate::elevation::postprocess::scale_to_minecraft(&meters, 1., 10, 10, false, 2031);
        let mut scaled: Vec<Vec<f32>> = scaled
            .into_iter()
            .map(|r| r.into_iter().map(|h| h as f32).collect())
            .collect();
        let protection = snapshot.into_protection(&scaled);
        let expected = scaled[2].clone();
        scaled[2] = vec![100.; 6];
        let mut lc = crate::land_cover::LandCoverData {
            grid: vec![vec![80; 6]; 6],
            width: 6,
            height: 6,
            water_distance: vec![vec![0; 6]; 6],
            water_blend_cache: once_cell::sync::OnceCell::new(),
            cells_per_meter: 1.,
        };
        protection.restore(&mut scaled, &mut lc);
        assert_eq!(&scaled[2][..5], &expected[..5]);
        assert_eq!(scaled[2][5], 100.);
        assert_eq!(lc.grid[2], vec![80, 80, 0, 50, 0, 80]);
        assert_eq!(lc.water_distance[2][2], 0);
    }
    #[test]
    fn coastal_exact_buffer_mutation_is_integrity_failure() {
        let (_dir, sources) = crate::tiler_contract::frozen_tests::fixture(
            "water_classification",
            "master-water-classification",
            b"{}",
        );
        std::fs::write(&sources.entries[0].path, b"[]").unwrap();
        let error = CoastalPolicy::load(&sources, &[0., 0., 10., 10.])
            .err()
            .unwrap();
        assert_eq!(error.exit_code, 1);
    }
    #[test]
    fn coastal_empty_domains_are_explicit_inland_only() {
        let mut value = fixture();
        value["coastal_domains"] = serde_json::json!([]);
        assert_eq!(parse(value).unwrap().classify(1., 5.), 0);
    }
}
use geo::{Intersects, LineString, MultiPolygon, Point, Polygon, Relate, Validation};
use serde::Deserialize;

const MAX_BYTES: u64 = 16 * 1024 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema_version: u32,
    policy: String,
    bbox: [f64; 4],
    default_classification: String,
    sources: Vec<SourceRef>,
    coastal_domains: Vec<DomainDocument>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceRef {
    kind: String,
    key: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DomainDocument {
    id: String,
    domain: Geometry,
    water: Geometry,
    inland_exclusions: Geometry,
}
#[derive(serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Geometry {
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) coordinates: Vec<Vec<Vec<[f64; 2]>>>,
}
struct Domain {
    domain: MultiPolygon,
    water: MultiPolygon,
    inland: MultiPolygon,
}
pub(crate) struct CoastalPolicy {
    domains: Vec<Domain>,
    frame: Option<crate::coastal_geometry::Frame>,
}
impl Geometry {
    fn validate(
        self,
        bbox: &[f64; 4],
        count: &mut usize,
        empty: bool,
    ) -> Result<MultiPolygon, String> {
        self.validate_limit(bbox, count, empty, 100_000)
    }
    pub(crate) fn validate_limit(
        self,
        bbox: &[f64; 4],
        count: &mut usize,
        empty: bool,
        limit: usize,
    ) -> Result<MultiPolygon, String> {
        if self.kind != "MultiPolygon" || (!empty && self.coordinates.is_empty()) {
            return Err("Coastal geometry requires a nonempty MultiPolygon".into());
        }
        let mut polygons = Vec::new();
        for rings in self.coordinates {
            if rings.is_empty() {
                return Err("Coastal polygon has no exterior ring".into());
            }
            let mut lines = Vec::new();
            for ring in rings {
                *count = count
                    .checked_add(ring.len())
                    .ok_or("Coastal vertex count overflow")?;
                if *count > limit {
                    return Err(format!("Coastal geometry exceeds {limit} vertices"));
                }
                if ring.len() < 4
                    || ring.first() != ring.last()
                    || ring.iter().any(|p| {
                        !p[0].is_finite()
                            || !p[1].is_finite()
                            || p[0] < bbox[1]
                            || p[0] > bbox[3]
                            || p[1] < bbox[0]
                            || p[1] > bbox[2]
                    })
                {
                    return Err("Coastal ring must be finite, closed and inside master bbox".into());
                }
                lines.push(LineString::from(
                    ring.into_iter().map(|p| (p[0], p[1])).collect::<Vec<_>>(),
                ));
            }
            let exterior = lines.remove(0);
            polygons.push(Polygon::new(exterior, lines));
        }
        let geometry = MultiPolygon(polygons);
        geometry
            .check_validation()
            .map_err(|e| format!("Invalid coastal geometry: {e}"))?;
        Ok(geometry)
    }
}
impl CoastalPolicy {
    pub(crate) fn load(
        sources: &crate::tiler_contract::AdmittedSources,
        bbox: &[f64; 4],
    ) -> Result<Self, crate::tiler_contract::ContractError> {
        let error = |message| crate::tiler_contract::ContractError {
            exit_code: 3,
            message,
        };
        let entry = sources
            .entries
            .iter()
            .find(|e| e.kind == "water_classification" && e.key == "master-water-classification")
            .ok_or_else(|| {
                error("Missing water_classification/master-water-classification source".into())
            })?;
        if entry.size_bytes > MAX_BYTES {
            return Err(error("Coastal input exceeds 16 MiB".into()));
        }
        let bytes = sources
            .resolve(
                "water_classification",
                "master-water-classification",
                MAX_BYTES,
            )
            .map_err(|message| crate::tiler_contract::ContractError {
                exit_code: 1,
                message,
            })?;
        let refs: Vec<_> = sources
            .entries
            .iter()
            .map(|e| (e.kind.as_str(), e.key.as_str()))
            .collect();
        let mut policy = Self::parse(&bytes, bbox, &refs).map_err(error)?;
        let doc: Document = serde_json::from_slice(&bytes).map_err(|e| error(e.to_string()))?;
        if sources.entries.iter().any(|s| s.kind == "coastal_geometry") {
            if !doc
                .sources
                .iter()
                .any(|s| s.kind == "osm" && s.key == "master-osm")
                || !doc
                    .sources
                    .iter()
                    .any(|s| s.kind == "coastal_geometry" && s.key == crate::coastal_geometry::KEY)
            {
                return Err(error(
                    "Coastal source-backed policy requires master OSM and ocean geometry".into(),
                ));
            }
            let geometry = sources
                .resolve(
                    "coastal_geometry",
                    crate::coastal_geometry::KEY,
                    crate::coastal_geometry::MAX_BYTES,
                )
                .map_err(error)?;
            let (frame, _) = crate::coastal_geometry::parse(&geometry, *bbox).map_err(error)?;
            policy.frame = Some(frame);
        }
        Ok(policy)
    }
    pub(crate) fn parse(
        bytes: &[u8],
        bbox: &[f64; 4],
        sources: &[(&str, &str)],
    ) -> Result<Self, String> {
        if bytes.len() as u64 > MAX_BYTES {
            return Err("Coastal input exceeds 16 MiB".into());
        }
        let doc: Document = serde_json::from_slice(bytes)
            .map_err(|e| format!("Invalid coastal classification: {e}"))?;
        if doc.schema_version != 1
            || doc.policy != "master-coastal-water-v1"
            || doc.default_classification != "inland"
            || doc.bbox != *bbox
            || !bbox.iter().all(|v| v.is_finite())
            || bbox[0] < -90.
            || bbox[2] > 90.
            || bbox[1] < -180.
            || bbox[3] > 180.
            || bbox[0] >= bbox[2]
            || bbox[1] >= bbox[3]
        {
            return Err("Coastal version/policy/bbox/default mismatch".into());
        }
        if doc.sources.is_empty() || doc.coastal_domains.len() > 1024 {
            return Err("Coastal sources required; at most 1024 domains".into());
        }
        let mut keys = std::collections::HashSet::new();
        for source in &doc.sources {
            if source.kind == "water_classification"
                || !sources.contains(&(source.kind.as_str(), source.key.as_str()))
                || !keys.insert((&source.kind, &source.key))
            {
                return Err("Missing or duplicate coastal source reference".into());
            }
        }
        let mut ids = std::collections::HashSet::new();
        let mut domains: Vec<Domain> = Vec::new();
        let mut count = 0;
        for d in doc.coastal_domains {
            if d.id.is_empty() || !ids.insert(d.id) {
                return Err("Empty or duplicate coastal domain id".into());
            }
            let domain = d.domain.validate(bbox, &mut count, false)?;
            let water = d.water.validate(bbox, &mut count, false)?;
            let inland = d.inland_exclusions.validate(bbox, &mut count, true)?;
            if !water.relate(&domain).is_coveredby()
                || (!inland.0.is_empty() && !inland.relate(&domain).is_coveredby())
                || water.intersects(&inland)
                || domains.iter().any(|d| d.domain.intersects(&domain))
            {
                return Err("Overlapping coastal domains, conflicting water/exclusions, or geometry outside domain".into());
            }
            domains.push(Domain {
                domain,
                water,
                inland,
            });
        }
        Ok(Self {
            domains,
            frame: None,
        })
    }
    /// Verify only the bound master lattice; do not expose continuous classification.
    pub(crate) fn verify_samples(
        &self,
        bbox: &[f64; 4],
        dims: [usize; 2],
        expected: &[u8],
        checkpoint: impl Fn() -> Result<(), String>,
    ) -> Result<(), String> {
        let [w, h] = dims;
        if w < 2 || h < 2 || w.checked_mul(h) != Some(expected.len()) {
            return Err("Coastal verification dimensions mismatch".into());
        }
        for row in 0..h {
            checkpoint()?;
            for col in 0..w {
                let lon = bbox[1] + col as f64 * (bbox[3] - bbox[1]) / (w - 1) as f64;
                let lat = bbox[2] - row as f64 * (bbox[2] - bbox[0]) / (h - 1) as f64;
                if self.classify(lon, lat) != expected[row * w + col] {
                    return Err(format!("Coastal serialized sample mismatch at {col},{row}"));
                }
            }
        }
        Ok(())
    }
    // 0: ordinary inland pipeline, 1: protected coastal dry, 2: coastal wet.
    fn classify(&self, lon: f64, lat: f64) -> u8 {
        let point = Point::new(lon, lat);
        for d in &self.domains {
            if d.domain.intersects(&point) {
                return if d.inland.intersects(&point) {
                    0
                } else if d.water.intersects(&point) {
                    2
                } else {
                    1
                };
            }
        }
        0
    }
}

// Only these temporary export buffers survive the first repair. 255 is inland;
// other class bytes are the authoritative coastal land-cover result.
pub(crate) struct CoastalSnapshot {
    classes: Vec<u8>,
    repaired_m: Vec<f64>,
    width: usize,
}
#[derive(Clone)]
pub(crate) struct CoastalProtection {
    classes: Vec<u8>,
    protected_y: Vec<f32>,
    width: usize,
}
impl CoastalPolicy {
    pub(crate) fn capture(
        &self,
        heights: &[Vec<f64>],
        lc: &[Vec<u8>],
        bbox: &[f64; 4],
    ) -> Result<CoastalSnapshot, String> {
        let height = heights.len();
        let width = heights.first().map_or(0, Vec::len);
        if width < 2
            || height < 2
            || height != lc.len()
            || heights.iter().any(|r| r.len() != width)
            || lc.iter().any(|r| r.len() != width)
        {
            return Err("Coastal master bands must be aligned".into());
        }
        if self
            .frame
            .is_some_and(|f| f.bbox != *bbox || f.dims != [width, height])
        {
            return Err("Coastal source-bound sample frame mismatch".into());
        }
        let mut classes = Vec::with_capacity(width * height);
        let mut repaired_m = Vec::with_capacity(width * height);
        for row in 0..height {
            let lat = bbox[2] - row as f64 * (bbox[2] - bbox[0]) / (height - 1) as f64;
            for col in 0..width {
                let lon = bbox[1] + col as f64 * (bbox[3] - bbox[1]) / (width - 1) as f64;
                let class = self.classify(lon, lat);
                let h = heights[row][col];
                if class != 0 && !h.is_finite() {
                    return Err(format!("Nonfinite coastal DEM at lon={lon}, lat={lat}"));
                }
                if class == 2 && h > 2.0 {
                    return Err(format!("Coastal wet DEM conflict at lon={lon}, lat={lat}: repaired DEM {h}m exceeds +2.0m above sea level"));
                }
                classes.push(match class {
                    2 => crate::land_cover::LC_WATER,
                    1 if lc[row][col] == crate::land_cover::LC_WATER => 0,
                    1 => lc[row][col],
                    _ => 255,
                });
                repaired_m.push(match class {
                    2 => 0.,
                    1 => h,
                    _ => f64::NAN,
                });
            }
        }
        Ok(CoastalSnapshot {
            classes,
            repaired_m,
            width,
        })
    }
}
impl CoastalSnapshot {
    pub(crate) fn restore(&self, heights: &mut [Vec<f64>], lc: &mut [Vec<u8>]) {
        for (i, &class) in self.classes.iter().enumerate() {
            if class != 255 {
                heights[i / self.width][i % self.width] = self.repaired_m[i];
                lc[i / self.width][i % self.width] = class;
            }
        }
    }
    pub(crate) fn into_protection(self, scaled: &[Vec<f32>]) -> CoastalProtection {
        let protected_y = self
            .classes
            .iter()
            .enumerate()
            .map(|(i, &class)| {
                if class == 255 {
                    f32::NAN
                } else {
                    scaled[i / self.width][i % self.width]
                }
            })
            .collect();
        CoastalProtection {
            classes: self.classes,
            protected_y,
            width: self.width,
        }
    }
}
impl CoastalProtection {
    pub(crate) fn restore(
        &self,
        heights: &mut [Vec<f32>],
        lc: &mut crate::land_cover::LandCoverData,
    ) {
        for (i, &class) in self.classes.iter().enumerate() {
            if class != 255 {
                heights[i / self.width][i % self.width] = self.protected_y[i];
                lc.grid[i / self.width][i % self.width] = class;
            }
        }
        lc.water_distance =
            crate::land_cover::compute_water_distance(&lc.grid, lc.width, lc.height);
        lc.invalidate_water_blend_grid();
    }
}

#[cfg(test)]
#[path = "coastal_diagnostic_tests.rs"]
mod diagnostics;
