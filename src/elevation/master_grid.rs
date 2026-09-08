//! Versioned, integrity-checked master terrain snapshots.
use serde::{Deserialize, Serialize};
use std::{io, path::Path};

pub(crate) type BBox = [f64; 4];
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderAttempt {
    pub name: String,
    pub outcome: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Metadata {
    pub format_version: u32,
    pub contract: String,
    pub bbox: BBox,
    pub width: u32,
    pub height: u32,
    pub world_width: u32,
    pub world_height: u32,
    pub scale: f64,
    pub projection: String,
    pub orientation: String,
    pub min_height_m: f64,
    pub blocks_per_meter: f64,
    pub effective_ground_level: i32,
    pub requested_ground_level: i32,
    pub min_ground_level: i32,
    pub extended_max_y: i32,
    pub water_floor: i32,
    pub sink_floor: i32,
    pub sea_level_y: f64,
    pub source_mode: String,
    pub selected_provider: String,
    pub provider_attempts: Vec<ProviderAttempt>,
    pub source_manifest_sha256: String,
    pub profile_sha256: String,
    pub postprocess: String,
    pub height_units: String,
    pub payload_bytes: u64,
    pub land_cover_cells_per_meter: f64,
    pub climate: String,
    pub snow_threshold_y: i32,
    pub climate_anchor: [f64; 2],
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Grid {
    pub metadata: Metadata,
    pub elevation: Vec<f32>,
    pub land_cover: Vec<u8>,
    pub water_distance: Vec<u8>,
    pub water_blend: Vec<f32>,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TileGrid {
    pub metadata: Metadata,
    pub bbox: BBox,
    pub col: u32,
    pub row: u32,
    pub width: u32,
    pub height: u32,
    /// Full immutable elevation context from the same admitted payload buffers.
    /// Metadata admission bounds this band to MAX_CELLS (64 MiB).
    pub master_elevation: std::sync::Arc<[f32]>,
    pub elevation: Vec<f32>,
    pub land_cover: Vec<u8>,
    pub water_distance: Vec<u8>,
    pub water_blend: Vec<f32>,
}
const MAGIC: &[u8; 9] = b"ARNTGRID2";
const MAX_METADATA: usize = 1_048_576;
pub(crate) const MAX_CELLS: u64 = 16_777_216;
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn require(ok: bool, message: &str) -> io::Result<()> {
    if ok {
        Ok(())
    } else {
        Err(invalid(message))
    }
}
fn hash_valid(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl Metadata {
    pub(crate) fn validate(&self) -> io::Result<()> {
        require(
            self.format_version == 2 && self.contract == "arnis-tiler/v3.1/1",
            "unsupported master grid version/contract",
        )?;
        require(
            self.width >= 2
                && self.height >= 2
                && self.width <= 16384
                && self.height <= 16384
                && self.world_width == self.width
                && self.world_height == self.height,
            "invalid master dimensions",
        )?;
        let cells = u64::from(self.width)
            .checked_mul(u64::from(self.height))
            .ok_or_else(|| invalid("cell count overflow"))?;
        require(
            cells <= MAX_CELLS && cells.checked_mul(10) == Some(self.payload_bytes),
            "master size limit or payload length mismatch",
        )?;
        let [s, w, n, e] = self.bbox;
        require(
            self.bbox.iter().all(|x| x.is_finite())
                && s >= -90.
                && n <= 90.
                && w >= -180.
                && e <= 180.
                && s < n
                && w < e,
            "invalid WGS84 bbox",
        )?;
        require(
            self.scale.is_finite()
                && self.scale > 0.
                && self.projection == "local"
                && self.orientation == "northwest-row-major",
            "invalid scale/projection/orientation",
        )?;
        require(
            self.min_height_m.is_finite()
                && self.blocks_per_meter.is_finite()
                && self.blocks_per_meter >= 0.,
            "invalid affine",
        )?;
        let expected =
            f64::from(self.effective_ground_level) - self.min_height_m * self.blocks_per_meter;
        require(
            self.sea_level_y.is_finite() && expected.is_finite() && self.sea_level_y == expected,
            "invalid sea level reference",
        )?;
        require(
            matches!(self.source_mode.as_str(), "aws-only" | "auto")
                && !self.selected_provider.is_empty()
                && !self.provider_attempts.is_empty(),
            "invalid source selection",
        )?;
        for (i, a) in self.provider_attempts.iter().enumerate() {
            require(
                !a.name.is_empty()
                    && matches!(
                        a.outcome.as_str(),
                        "success" | "error" | "insufficient-coverage"
                    ),
                "invalid provider attempt",
            )?;
            require(
                a.outcome != "success" || i + 1 == self.provider_attempts.len(),
                "provider attempts after success",
            )?;
        }
        let last = self.provider_attempts.last().unwrap();
        require(
            last.name == self.selected_provider && last.outcome == "success",
            "selected provider must be final successful attempt",
        )?;
        require(
            self.source_mode != "aws-only"
                || (self.provider_attempts.len() == 1
                    && self.selected_provider == "aws"
                    && self.provider_attempts[0].name == "aws"),
            "AWS-only requires exactly one successful aws attempt",
        )?;
        require(
            hash_valid(&self.source_manifest_sha256) && hash_valid(&self.profile_sha256),
            "invalid source/profile hash",
        )?;
        require(
            self.postprocess == "master-once-v1" && self.height_units == "minecraft_y",
            "invalid processing metadata",
        )?;
        require(
            self.land_cover_cells_per_meter.is_finite() && self.land_cover_cells_per_meter > 0.,
            "invalid land-cover resolution",
        )?;
        require(
            matches!(
                self.climate.as_str(),
                "Temperate"
                    | "TropicalSavanna"
                    | "HotDesert"
                    | "HotSteppe"
                    | "ColdDesert"
                    | "ColdSteppe"
                    | "DryContinental"
                    | "Boreal"
                    | "Tundra"
                    | "IceCap"
            ),
            "invalid climate",
        )?;
        require(
            self.climate_anchor == [(s + n) / 2., (w + e) / 2.],
            "climate anchor must be master center",
        )
    }
    pub(crate) fn slice_bbox(
        &self,
        col: u32,
        row: u32,
        width: u32,
        height: u32,
    ) -> io::Result<BBox> {
        self.validate()?;
        require(
            width >= 2
                && height >= 2
                && width <= 4096
                && height <= 4096
                && col.checked_add(width).is_some_and(|x| x <= self.width)
                && row.checked_add(height).is_some_and(|y| y <= self.height),
            "invalid slice bounds",
        )?;
        let [s, w, n, e] = self.bbox;
        let longitude = |c: u32| w + f64::from(c) * (e - w) / f64::from(self.width - 1);
        let latitude = |r: u32| n - f64::from(r) * (n - s) / f64::from(self.height - 1);
        Ok([
            latitude(row + height - 1),
            longitude(col),
            latitude(row),
            longitude(col + width - 1),
        ])
    }
}
impl TileGrid {
    /// Call during preflight, before generation or output creation.
    pub(crate) fn validate_request(
        &self,
        bbox: BBox,
        scale: f64,
        profile_sha256: &str,
        source_manifest_sha256: &str,
    ) -> io::Result<()> {
        require(
            bbox.iter()
                .zip(self.bbox)
                .all(|(a, b)| a.is_finite() && (a - b).abs() <= 1e-10),
            "tile bbox differs from master samples",
        )?;
        require(
            scale.is_finite()
                && scale == self.metadata.scale
                && profile_sha256 == self.metadata.profile_sha256
                && source_manifest_sha256 == self.metadata.source_manifest_sha256,
            "tile scale/profile/source differs from master",
        )
    }
}
fn valid_land_cover(v: u8) -> bool {
    matches!(v, 0 | 10 | 20 | 30 | 40 | 50 | 60 | 70 | 80 | 90 | 95 | 100)
}
fn validate_band(band: usize, bytes: &[u8]) -> io::Result<()> {
    match band {
        0 | 3 => {
            for chunk in bytes.chunks_exact(4) {
                let v = f32::from_le_bytes(chunk.try_into().unwrap());
                require(
                    v.is_finite() && (band == 0 || (0.0..=1.0).contains(&v)),
                    "nonfinite/out-of-range float sample",
                )?;
            }
        }
        1 => require(
            bytes.iter().copied().all(valid_land_cover),
            "invalid land-cover sample",
        )?,
        2 => require(
            bytes.iter().all(|v| *v <= 15),
            "invalid water-distance sample",
        )?,
        _ => unreachable!(),
    }
    Ok(())
}
pub(crate) fn save(path: &Path, grid: &Grid) -> io::Result<()> {
    use sha2::{Digest, Sha256};
    use std::io::{Seek, SeekFrom, Write};
    grid.metadata.validate()?;
    let cells = (u64::from(grid.metadata.width) * u64::from(grid.metadata.height)) as usize;
    require(
        [
            grid.elevation.len(),
            grid.land_cover.len(),
            grid.water_distance.len(),
            grid.water_blend.len(),
        ]
        .iter()
        .all(|n| *n == cells),
        "band length mismatch",
    )?;
    // Validate before creating even the temporary file, preserving existing grids on rejection.
    require(
        grid.elevation.iter().all(|v| v.is_finite()),
        "nonfinite elevation",
    )?;
    require(
        grid.land_cover.iter().copied().all(valid_land_cover),
        "invalid land-cover sample",
    )?;
    require(
        grid.water_distance.iter().all(|v| *v <= 15),
        "invalid water-distance sample",
    )?;
    require(
        grid.water_blend
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
        "invalid water blend",
    )?;
    let meta = serde_json::to_vec(&grid.metadata).map_err(io::Error::other)?;
    require(
        !meta.is_empty() && meta.len() <= MAX_METADATA,
        "metadata size limit",
    )?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    let mut hash = Sha256::new();
    let len = (meta.len() as u32).to_le_bytes();
    for part in [MAGIC.as_slice(), len.as_slice(), meta.as_slice()] {
        temp.write_all(part)?;
        hash.update(part);
    }
    let digest_offset = 13 + meta.len() as u64;
    temp.write_all(&[0; 32])?;
    // Bounded conversion buffers avoid a second complete payload allocation.
    for band in 0..4 {
        if band == 1 || band == 2 {
            let bytes = if band == 1 {
                &grid.land_cover
            } else {
                &grid.water_distance
            };
            temp.write_all(bytes)?;
            hash.update(bytes);
        } else {
            let values = if band == 0 {
                &grid.elevation
            } else {
                &grid.water_blend
            };
            let mut buffer = [0u8; 65536];
            for chunk in values.chunks(buffer.len() / 4) {
                for (v, dst) in chunk.iter().zip(buffer.chunks_exact_mut(4)) {
                    dst.copy_from_slice(&v.to_le_bytes());
                }
                let bytes = &buffer[..chunk.len() * 4];
                temp.write_all(bytes)?;
                hash.update(bytes);
            }
        }
    }
    temp.seek(SeekFrom::Start(digest_offset))?;
    temp.write_all(&hash.finalize())?;
    temp.flush()?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}
pub(crate) fn load_slice(
    path: &Path,
    col: u32,
    row: u32,
    width: u32,
    height: u32,
) -> io::Result<TileGrid> {
    let file = std::fs::File::open(path)?;
    let file_len = file.metadata()?.len();
    load_slice_reader(file, file_len, col, row, width, height)
}

fn load_slice_reader(
    mut file: impl std::io::Read,
    file_len: u64,
    col: u32,
    row: u32,
    width: u32,
    height: u32,
) -> io::Result<TileGrid> {
    use sha2::{Digest, Sha256};
    let mut prefix = [0u8; 13];
    file.read_exact(&mut prefix)?;
    require(&prefix[..9] == MAGIC, "unsupported master grid magic")?;
    let meta_len = u32::from_le_bytes(prefix[9..].try_into().unwrap()) as usize;
    require(
        (1..=MAX_METADATA).contains(&meta_len),
        "metadata size limit",
    )?;
    let mut meta_bytes = vec![0; meta_len];
    file.read_exact(&mut meta_bytes)?;
    // Direct typed deserialization rejects duplicate keys as well as unknown fields.
    let metadata: Metadata = serde_json::from_slice(&meta_bytes)
        .map_err(|e| invalid(&format!("invalid grid metadata: {e}")))?;
    metadata.validate()?;
    let bbox = metadata.slice_bbox(col, row, width, height)?;
    let payload_start = 45u64
        .checked_add(meta_len as u64)
        .ok_or_else(|| invalid("header length overflow"))?;
    let expected_size = payload_start
        .checked_add(metadata.payload_bytes)
        .ok_or_else(|| invalid("file length overflow"))?;
    require(
        file_len == expected_size,
        "truncated grid or trailing bytes",
    )
    .map_err(io::Error::other)?;
    let mut expected_digest = [0u8; 32];
    file.read_exact(&mut expected_digest)?;
    let mut hash = Sha256::new();
    hash.update(prefix);
    hash.update(&meta_bytes);
    let cells = u64::from(metadata.width) * u64::from(metadata.height);
    let mut buffer = [0u8; 65536];
    let mut bands = Vec::with_capacity(4);
    let mut master_elevation = Vec::with_capacity(cells as usize);
    // Retain requested bands and the full elevation context from buffers admitted by validation and
    // hashing. A second read, even through the same handle, could observe an in-place
    // mutation after admission. No payload bytes are reread here.
    for (band, stride) in [4u64, 1, 1, 4].into_iter().enumerate() {
        let slice_bytes = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|n| n.checked_mul(stride))
            .ok_or_else(|| invalid("slice allocation overflow"))?;
        let mut bytes =
            vec![0; usize::try_from(slice_bytes).map_err(|_| invalid("slice too large"))?];
        let master_row_bytes = u64::from(metadata.width) * stride;
        let slice_row_bytes = u64::from(width) * stride;
        let band_bytes = cells * stride;
        let mut consumed = 0u64;
        while consumed < band_bytes {
            let n = (band_bytes - consumed).min(buffer.len() as u64) as usize;
            file.read_exact(&mut buffer[..n])?;
            validate_band(band, &buffer[..n]).map_err(io::Error::other)?;
            hash.update(&buffer[..n]);
            if band == 0 {
                master_elevation.extend(
                    buffer[..n]
                        .chunks_exact(4)
                        .map(|c| f32::from_le_bytes(c.try_into().unwrap())),
                );
            }
            let chunk_end = consumed + n as u64;
            let first_row = (consumed / master_row_bytes).max(u64::from(row));
            let end_row = chunk_end
                .div_ceil(master_row_bytes)
                .min(u64::from(row + height));
            for r in first_row..end_row {
                let wanted_start = r * master_row_bytes + u64::from(col) * stride;
                let wanted_end = wanted_start + slice_row_bytes;
                let start = consumed.max(wanted_start);
                let end = chunk_end.min(wanted_end);
                if start < end {
                    let destination = (r - u64::from(row)) * slice_row_bytes + start - wanted_start;
                    let count = (end - start) as usize;
                    let source = (start - consumed) as usize;
                    let destination = destination as usize;
                    bytes[destination..destination + count]
                        .copy_from_slice(&buffer[source..source + count]);
                }
            }
            consumed = chunk_end;
        }
        bands.push(bytes);
    }
    require(
        hash.finalize().as_slice() == expected_digest,
        "master grid checksum mismatch",
    )
    .map_err(io::Error::other)?;
    let floats = |bytes: Vec<u8>| {
        bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
            .collect()
    };
    let mut bands = bands.into_iter();
    Ok(TileGrid {
        metadata,
        bbox,
        col,
        row,
        width,
        height,
        master_elevation: master_elevation.into(),
        elevation: floats(bands.next().unwrap()),
        land_cover: bands.next().unwrap(),
        water_distance: bands.next().unwrap(),
        water_blend: floats(bands.next().unwrap()),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn fixture() -> Grid {
        let metadata = serde_json::from_value(serde_json::json!({
 "format_version":2,"contract":"arnis-tiler/v3.1/1","bbox":[40.0,-74.0,41.0,-73.0],
 "width":4,"height":3,"world_width":4,"world_height":3,"scale":1.0,"projection":"local","orientation":"northwest-row-major",
 "min_height_m":-3.0,"blocks_per_meter":2.0,"effective_ground_level":70,"requested_ground_level":62,"min_ground_level":-64,"extended_max_y":319,"water_floor":-60,"sink_floor":-61,"sea_level_y":76.0,
 "source_mode":"aws-only","selected_provider":"aws","provider_attempts":[{"name":"aws","outcome":"success"}],
 "source_manifest_sha256":"a".repeat(64),"profile_sha256":"b".repeat(64),"postprocess":"master-once-v1","height_units":"minecraft_y","payload_bytes":120,
 "land_cover_cells_per_meter":0.1,"climate":"Temperate","snow_threshold_y":150,"climate_anchor":[40.5,-73.5]
 })).unwrap();
        Grid {
            metadata,
            elevation: (0..12).map(|n| n as f32 + 0.25).collect(),
            land_cover: vec![0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 95, 100],
            water_distance: (0..12).collect(),
            water_blend: (0..12).map(|n| n as f32 / 12.).collect(),
        }
    }
    #[test]
    fn roundtrip_all_bands_and_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grid");
        let g = fixture();
        save(&path, &g).unwrap();
        let tile = load_slice(&path, 0, 0, 4, 3).unwrap();
        assert_eq!(tile.metadata, g.metadata);
        assert_eq!(tile.bbox, g.metadata.bbox);
        assert_eq!(tile.elevation, g.elevation);
        assert_eq!(tile.land_cover, g.land_cover);
        assert_eq!(tile.water_distance, g.water_distance);
        assert_eq!(tile.water_blend, g.water_blend);
    }
    #[test]
    fn nonzero_slice_and_bounds() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grid");
        let g = fixture();
        save(&path, &g).unwrap();
        let t = load_slice(&path, 1, 1, 2, 2).unwrap();
        assert_eq!(t.elevation, vec![5.25, 6.25, 9.25, 10.25]);
        assert_eq!(t.land_cover, vec![50, 60, 90, 95]);
        assert_eq!(t.water_distance, vec![5, 6, 9, 10]);
        assert_eq!(t.water_blend, vec![5. / 12., 6. / 12., 9. / 12., 10. / 12.]);
        assert_eq!(t.bbox, [40., -74. + 1. / 3., 40.5, -74. + 2. / 3.]);
        for (c, r, w, h) in [
            (3, 0, 2, 2),
            (0, 2, 2, 2),
            (u32::MAX, 0, 2, 2),
            (0, 0, 1, 2),
            (0, 0, 2, 0),
            (0, 0, 4097, 2),
        ] {
            assert!(load_slice(&path, c, r, w, h).is_err());
        }
    }
    #[test]
    fn invalid_replacement_preserves_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grid");
        let g = fixture();
        save(&path, &g).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mut invalid = vec![];
        let mut x = g.clone();
        x.elevation[0] = f32::NAN;
        invalid.push(x);
        let mut x = g.clone();
        x.water_blend[0] = 1.1;
        invalid.push(x);
        let mut x = g.clone();
        x.water_distance[0] = 16;
        invalid.push(x);
        let mut x = g.clone();
        x.land_cover[0] = 255;
        invalid.push(x);
        let mut x = g.clone();
        x.elevation.pop();
        invalid.push(x);
        let mut x = g.clone();
        x.metadata.format_version = 1;
        invalid.push(x);
        let mut x = g.clone();
        x.metadata.sea_level_y = 2.;
        invalid.push(x);
        let mut x = g.clone();
        x.metadata.climate = "unknown".into();
        invalid.push(x);
        for x in invalid {
            assert!(save(&path, &x).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn corrupt_truncated_trailing_and_oversized_headers_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grid");
        save(&path, &fixture()).unwrap();
        let original = std::fs::read(&path).unwrap();
        for pos in [0, 9, 20, original.len() - 1] {
            let mut b = original.clone();
            b[pos] ^= 1;
            std::fs::write(&path, b).unwrap();
            assert!(load_slice(&path, 0, 0, 2, 2).is_err());
        }
        for len in [0, 8, 12, 40, original.len() - 1] {
            std::fs::write(&path, &original[..len]).unwrap();
            assert!(load_slice(&path, 0, 0, 2, 2).is_err());
        }
        let mut b = original.clone();
        b.push(0);
        std::fs::write(&path, b).unwrap();
        assert!(load_slice(&path, 0, 0, 2, 2).is_err());
        let mut b = b"ARNTGRID2".to_vec();
        b.extend(u32::MAX.to_le_bytes());
        std::fs::write(&path, b).unwrap();
        assert!(load_slice(&path, 0, 0, 2, 2).is_err());
    }
    #[test]
    fn duplicate_unknown_and_oversized_metadata_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grid");
        let m = serde_json::to_string(&fixture().metadata).unwrap();
        for json in [
            m.replacen('{', "{\"width\":4,", 1),
            m.replacen('{', "{\"unexpected\":0,", 1),
            m.replace("\"width\":4", "\"width\":4294967295"),
        ] {
            let mut b = b"ARNTGRID2".to_vec();
            b.extend((json.len() as u32).to_le_bytes());
            b.extend(json.as_bytes());
            b.extend([0; 32]);
            std::fs::write(&path, b).unwrap();
            assert!(load_slice(&path, 0, 0, 2, 2).is_err());
        }
    }
    #[test]
    fn valid_checksum_cannot_hide_invalid_samples_outside_slice() {
        use sha2::{Digest, Sha256};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grid");
        save(&path, &fixture()).unwrap();
        let original = std::fs::read(&path).unwrap();
        let n = u32::from_le_bytes(original[9..13].try_into().unwrap()) as usize;
        let payload = 45 + n;
        for (offset, bytes) in [
            (payload + 11 * 4, f32::NAN.to_le_bytes().to_vec()),
            (payload + 48 + 11, vec![255]),
            (payload + 60 + 11, vec![16]),
            (payload + 72 + 11 * 4, f32::INFINITY.to_le_bytes().to_vec()),
            (payload + 72 + 11 * 4, (-0.1f32).to_le_bytes().to_vec()),
        ] {
            let mut b = original.clone();
            b[offset..offset + bytes.len()].copy_from_slice(&bytes);
            let mut h = Sha256::new();
            h.update(&b[..13 + n]);
            h.update(&b[payload..]);
            b[13 + n..payload].copy_from_slice(&h.finalize());
            std::fs::write(&path, b).unwrap();
            assert!(load_slice(&path, 0, 0, 2, 2).is_err());
        }
    }
    #[test]
    fn metadata_ranges_and_identity_are_strict() {
        let base = serde_json::to_value(fixture().metadata).unwrap();
        for (field, value) in [
            ("world_width", serde_json::json!(3)),
            ("width", serde_json::json!(16385)),
            ("height", serde_json::json!(4294967295u32)),
            ("scale", serde_json::json!(0)),
            ("blocks_per_meter", serde_json::json!(-1)),
            ("payload_bytes", serde_json::json!(119)),
            ("bbox", serde_json::json!([41, -74, 40, -73])),
            ("bbox", serde_json::json!([40, 179, 41, -179])),
            ("climate_anchor", serde_json::json!([40.4, -73.5])),
            ("land_cover_cells_per_meter", serde_json::json!(0)),
            ("source_manifest_sha256", serde_json::json!("A".repeat(64))),
            ("profile_sha256", serde_json::json!("b".repeat(63))),
            ("provider_attempts", serde_json::json!([])),
            (
                "provider_attempts",
                serde_json::json!([{"name":"aws","outcome":"error"}]),
            ),
            ("contract", serde_json::json!("old")),
            ("height_units", serde_json::json!("metres")),
            ("projection", serde_json::json!("mercator")),
            ("orientation", serde_json::json!("southwest-row-major")),
            ("source_mode", serde_json::json!("live")),
            ("postprocess", serde_json::json!("tile-local")),
        ] {
            let mut m = base.clone();
            m[field] = value;
            let parsed: Metadata = serde_json::from_value(m).unwrap();
            assert!(parsed.validate().is_err(), "accepted {field}");
        }
        let mut m = fixture().metadata;
        m.width = 8192;
        m.height = 8192;
        m.world_width = 8192;
        m.world_height = 8192;
        m.payload_bytes = 8192 * 8192 * 10;
        assert!(m.validate().is_err());
        let mut m = fixture().metadata;
        m.blocks_per_meter = 0.;
        m.sea_level_y = f64::from(m.effective_ground_level);
        m.validate().unwrap();
    }
    #[test]
    fn request_requires_matching_bbox_scale_and_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grid");
        save(&path, &fixture()).unwrap();
        let t = load_slice(&path, 1, 1, 2, 2).unwrap();
        t.validate_request(
            t.bbox,
            1.,
            &t.metadata.profile_sha256,
            &t.metadata.source_manifest_sha256,
        )
        .unwrap();
        let mut b = t.bbox;
        b[0] += 2e-10;
        assert!(t
            .validate_request(
                b,
                1.,
                &t.metadata.profile_sha256,
                &t.metadata.source_manifest_sha256
            )
            .is_err());
        assert!(t
            .validate_request(
                t.bbox,
                2.,
                &t.metadata.profile_sha256,
                &t.metadata.source_manifest_sha256
            )
            .is_err());
        assert!(t
            .validate_request(
                t.bbox,
                1.,
                &"c".repeat(64),
                &t.metadata.source_manifest_sha256
            )
            .is_err());
        assert!(t
            .validate_request(t.bbox, 1., &t.metadata.profile_sha256, &"c".repeat(64))
            .is_err());
    }
    #[test]
    fn realistic_affine_and_coordinate_metadata_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grid");
        let mut g = fixture();
        g.metadata.min_height_m = 45.48765574464247;
        g.metadata.blocks_per_meter = 2.0;
        g.metadata.effective_ground_level = 70;
        g.metadata.sea_level_y = -20.975311489284934;
        g.metadata.bbox = [
            40.71281234567891,
            -74.00601234567891,
            40.7345987654321,
            -73.9812987654321,
        ];
        let [s, w, n, e] = g.metadata.bbox;
        g.metadata.climate_anchor = [(s + n) / 2.0, (w + e) / 2.0];
        g.metadata.validate().unwrap();
        save(&path, &g).unwrap();
        let tile = load_slice(&path, 0, 0, 4, 3).unwrap();
        assert_eq!(tile.metadata, g.metadata);
    }

    #[test]
    fn aws_only_rejects_other_or_unknown_provider_names() {
        for name in ["mapterhorn", "unknown", "AWS Terrain Tiles"] {
            let mut m = fixture().metadata;
            m.selected_provider = name.into();
            m.provider_attempts[0].name = name.into();
            assert!(m.validate().is_err(), "accepted AWS-only provider {name}");
        }
        let mut m = fixture().metadata;
        m.provider_attempts[0].name = "mapterhorn".into();
        assert!(m.validate().is_err());
        let mut m = fixture().metadata;
        m.provider_attempts.insert(
            0,
            ProviderAttempt {
                name: "mapterhorn".into(),
                outcome: "error".into(),
            },
        );
        assert!(m.validate().is_err());
        fixture().metadata.validate().unwrap();
    }
    #[test]
    fn returns_only_bytes_admitted_by_digest_despite_in_place_mutation() {
        use std::io::{Cursor, Read, Seek, SeekFrom};
        struct MutatingReader {
            cursor: Cursor<Vec<u8>>,
            payload: usize,
            mutated: bool,
        }
        impl Read for MutatingReader {
            fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
                let n = self.cursor.read(dst)?;
                if !self.mutated && self.cursor.position() == self.cursor.get_ref().len() as u64 {
                    self.cursor.get_mut()[self.payload..self.payload + 4]
                        .copy_from_slice(&999.0f32.to_le_bytes());
                    self.mutated = true;
                }
                Ok(n)
            }
        }
        impl Seek for MutatingReader {
            fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
                self.cursor.seek(pos)
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grid");
        let g = fixture();
        save(&path, &g).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let len = bytes.len() as u64;
        let payload = 45 + u32::from_le_bytes(bytes[9..13].try_into().unwrap()) as usize;
        let reader = MutatingReader {
            cursor: Cursor::new(bytes),
            payload,
            mutated: false,
        };
        let tile = load_slice_reader(reader, len, 0, 0, 2, 2).unwrap();
        assert_eq!(tile.elevation, vec![0.25, 1.25, 4.25, 5.25]);
        assert_eq!(&*tile.master_elevation, &g.elevation);
        assert!(std::sync::Arc::ptr_eq(
            &tile.master_elevation,
            &tile.clone().master_elevation
        ));
    }

    #[test]
    fn streaming_slice_crosses_buffers_and_rows_in_all_bands() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grid");
        let mut g = fixture();
        let (mw, mh) = (257u32, 270u32);
        g.metadata.width = mw;
        g.metadata.height = mh;
        g.metadata.world_width = mw;
        g.metadata.world_height = mh;
        g.metadata.payload_bytes = u64::from(mw) * u64::from(mh) * 10;
        let cells = (mw * mh) as usize;
        g.elevation = (0..cells).map(|i| i as f32 + 0.25).collect();
        g.land_cover = (0..cells).map(|i| [0, 10, 80, 95][i % 4]).collect();
        g.water_distance = (0..cells).map(|i| (i % 16) as u8).collect();
        g.water_blend = (0..cells).map(|i| (i % 16) as f32 / 16.0).collect();
        save(&path, &g).unwrap();
        let t = load_slice(&path, 51, 61, 129, 205).unwrap();
        assert_eq!(t.elevation.len(), 129 * 205);
        for r in 0..205usize {
            for c in 0..129usize {
                let src = (61 + r) * mw as usize + 51 + c;
                let dst = r * 129 + c;
                assert_eq!(t.elevation[dst], g.elevation[src]);
                assert_eq!(t.land_cover[dst], g.land_cover[src]);
                assert_eq!(t.water_distance[dst], g.water_distance[src]);
                assert_eq!(t.water_blend[dst], g.water_blend[src]);
            }
        }
    }
}
