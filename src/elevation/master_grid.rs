//! Master elevation grid serialization for arnis-tiler.
//!
//! When `ARNIS_SAVE_ELEVATION_GRID=<path>` is set, after fetching+processing
//! raw elevation, write the height grid to disk as a binary file. The
//! arnis-tiler runner pre-fetches a master grid covering the entire metro
//! bbox once, then each per-tile arnis invocation sets
//! `ARNIS_USE_ELEVATION_GRID=<path>` and sources its elevation by SLICING
//! the master grid via bilinear interpolation. Adjacent tiles read the same
//! master cells at their shared edges → identical elevation values →
//! eliminates the visible terrain seams that result from independent
//! per-tile USGS 3DEP fetches.
//!
//! File format (little-endian throughout):
//!   bytes  0..12  magic = "ARNS_ELEV_v1"
//!   bytes 12..20  master_min_lat (f64)
//!   bytes 20..28  master_min_lng (f64)
//!   bytes 28..36  master_max_lat (f64)
//!   bytes 36..44  master_max_lng (f64)
//!   bytes 44..48  master_grid_w  (u32)
//!   bytes 48..52  master_grid_h  (u32)
//!   bytes 52..60  scale          (f64)  [for forward compat / sanity]
//!   bytes 60..    heights        (master_grid_w × master_grid_h × f32)
//!                                 row-major; row 0 is at master_max_lat
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::coordinate_system::geographic::LLBBox;

const MAGIC: &[u8; 12] = b"ARNS_ELEV_v1";

pub struct MasterGrid {
    pub min_lat: f64,
    pub min_lng: f64,
    pub max_lat: f64,
    pub max_lng: f64,
    pub width: usize,
    pub height: usize,
    pub scale: f64,
    pub data: Vec<f32>,
}

pub fn save_grid(
    path: &Path,
    bbox: &LLBBox,
    scale: f64,
    height_grid: &[Vec<f64>],
) -> std::io::Result<()> {
    let h = height_grid.len();
    let w = if h > 0 { height_grid[0].len() } else { 0 };

    let mut f = File::create(path)?;
    f.write_all(MAGIC)?;
    f.write_all(&bbox.min().lat().to_le_bytes())?;
    f.write_all(&bbox.min().lng().to_le_bytes())?;
    f.write_all(&bbox.max().lat().to_le_bytes())?;
    f.write_all(&bbox.max().lng().to_le_bytes())?;
    f.write_all(&(w as u32).to_le_bytes())?;
    f.write_all(&(h as u32).to_le_bytes())?;
    f.write_all(&scale.to_le_bytes())?;

    let mut buf = Vec::with_capacity(w * 4);
    for row in height_grid {
        buf.clear();
        for &v in row {
            let v32 = v as f32;
            buf.extend_from_slice(&v32.to_le_bytes());
        }
        f.write_all(&buf)?;
    }
    Ok(())
}

pub fn load_grid(path: &Path) -> std::io::Result<MasterGrid> {
    let mut f = File::open(path)?;
    let mut header = [0u8; 60];
    f.read_exact(&mut header)?;
    if &header[..12] != MAGIC {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "ARNIS_USE_ELEVATION_GRID file: bad magic",
        ));
    }
    let mut rd_f64 = |off: usize| -> f64 {
        let mut a = [0u8; 8];
        a.copy_from_slice(&header[off..off + 8]);
        f64::from_le_bytes(a)
    };
    let min_lat = rd_f64(12);
    let min_lng = rd_f64(20);
    let max_lat = rd_f64(28);
    let max_lng = rd_f64(36);
    let mut rd_u32 = |off: usize| -> u32 {
        let mut a = [0u8; 4];
        a.copy_from_slice(&header[off..off + 4]);
        u32::from_le_bytes(a)
    };
    let w = rd_u32(44) as usize;
    let h = rd_u32(48) as usize;
    let scale = rd_f64(52);

    let mut data = vec![0f32; w * h];
    let mut bytes = vec![0u8; w * h * 4];
    f.read_exact(&mut bytes)?;
    for (i, chunk) in bytes.chunks_exact(4).enumerate() {
        let mut a = [0u8; 4];
        a.copy_from_slice(chunk);
        data[i] = f32::from_le_bytes(a);
    }

    Ok(MasterGrid {
        min_lat,
        min_lng,
        max_lat,
        max_lng,
        width: w,
        height: h,
        scale,
        data,
    })
}

/// Read just the [row_off..row_off+grid_h) × [col_off..col_off+grid_w) sub-region
/// from a master grid file as a Vec<Vec<f64>>. Avoids loading the full 19 GB
/// master into memory when we only need ~1 GB for one tile.
///
/// Returns NaN for any cells outside the master's actual width/height (so the
/// caller can fill_nan_values them like the in-memory direct-slice path does).
pub fn load_grid_slice(
    path: &Path,
    col_off: usize,
    row_off: usize,
    grid_w: usize,
    grid_h: usize,
) -> std::io::Result<Vec<Vec<f64>>> {
    const HEADER_LEN: u64 = 60;
    let mut f = File::open(path)?;
    let mut header = [0u8; 60];
    f.read_exact(&mut header)?;
    if &header[..12] != MAGIC {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "ARNIS_USE_ELEVATION_GRID file: bad magic",
        ));
    }
    let mut rd_u32 = |off: usize| -> u32 {
        let mut a = [0u8; 4];
        a.copy_from_slice(&header[off..off + 4]);
        u32::from_le_bytes(a)
    };
    let master_w = rd_u32(44) as usize;
    let master_h = rd_u32(48) as usize;

    let mut out = vec![vec![f64::NAN; grid_w]; grid_h];
    if master_w == 0 || master_h == 0 {
        return Ok(out);
    }

    // Largest valid (master) col we can read for this tile slice.
    let read_w = grid_w.min(master_w.saturating_sub(col_off));
    if read_w == 0 {
        return Ok(out);
    }
    let row_stride_bytes: u64 = (master_w as u64) * 4;
    let mut row_buf = vec![0u8; read_w * 4];
    for r in 0..grid_h {
        let mr = row_off + r;
        if mr >= master_h {
            continue;
        }
        let row_start = HEADER_LEN + (mr as u64) * row_stride_bytes + (col_off as u64) * 4;
        f.seek(SeekFrom::Start(row_start))?;
        f.read_exact(&mut row_buf)?;
        let row_out = &mut out[r];
        for (c, chunk) in row_buf.chunks_exact(4).enumerate() {
            let mut a = [0u8; 4];
            a.copy_from_slice(chunk);
            row_out[c] = f32::from_le_bytes(a) as f64;
        }
    }
    Ok(out)
}

/// Bilinearly sample the master grid at (lat, lng).
/// Returns f64 NaN if out of bounds.
pub fn sample_at(grid: &MasterGrid, lat: f64, lng: f64) -> f64 {
    if lat < grid.min_lat
        || lat > grid.max_lat
        || lng < grid.min_lng
        || lng > grid.max_lng
        || grid.width < 2
        || grid.height < 2
    {
        return f64::NAN;
    }
    let col_f = (lng - grid.min_lng) / (grid.max_lng - grid.min_lng) * (grid.width - 1) as f64;
    let row_f = (grid.max_lat - lat) / (grid.max_lat - grid.min_lat) * (grid.height - 1) as f64;
    let c0 = col_f.floor() as i64;
    let r0 = row_f.floor() as i64;
    let c1 = (c0 + 1).min(grid.width as i64 - 1);
    let r1 = (r0 + 1).min(grid.height as i64 - 1);
    let c0 = c0.max(0).min(grid.width as i64 - 1) as usize;
    let r0 = r0.max(0).min(grid.height as i64 - 1) as usize;
    let c1 = c1.max(0) as usize;
    let r1 = r1.max(0) as usize;
    let fx = (col_f - c0 as f64).clamp(0.0, 1.0);
    let fy = (row_f - r0 as f64).clamp(0.0, 1.0);

    let v00 = grid.data[r0 * grid.width + c0] as f64;
    let v01 = grid.data[r0 * grid.width + c1] as f64;
    let v10 = grid.data[r1 * grid.width + c0] as f64;
    let v11 = grid.data[r1 * grid.width + c1] as f64;

    if !v00.is_finite() || !v01.is_finite() || !v10.is_finite() || !v11.is_finite() {
        return f64::NAN;
    }

    let top = v00 * (1.0 - fx) + v01 * fx;
    let bot = v10 * (1.0 - fx) + v11 * fx;
    top * (1.0 - fy) + bot * fy
}

/// Reconstruct a per-tile height grid by sampling the master at each tile cell.
pub fn slice_for_tile(
    grid: &MasterGrid,
    tile_bbox: &LLBBox,
    tile_grid_w: usize,
    tile_grid_h: usize,
) -> Vec<Vec<f64>> {
    let mut out = vec![vec![f64::NAN; tile_grid_w]; tile_grid_h];
    if tile_grid_h < 2 || tile_grid_w < 2 {
        return out;
    }
    let lat_top = tile_bbox.max().lat();
    let lat_bot = tile_bbox.min().lat();
    let lng_left = tile_bbox.min().lng();
    let lng_right = tile_bbox.max().lng();
    for r in 0..tile_grid_h {
        let frac_r = r as f64 / (tile_grid_h - 1) as f64;
        let lat = lat_top - frac_r * (lat_top - lat_bot);
        for c in 0..tile_grid_w {
            let frac_c = c as f64 / (tile_grid_w - 1) as f64;
            let lng = lng_left + frac_c * (lng_right - lng_left);
            out[r][c] = sample_at(grid, lat, lng);
        }
    }
    out
}

/// Compute the tightest master-cell range covering the tile bbox, plus an
/// integer number of master cells. Returns (col_start, col_end_inclusive,
/// row_start, row_end_inclusive). Each tile's range is contiguous in master
/// coords; adjacent tiles share the boundary cell, eliminating cell-stride
/// drift between independently-resampled tile grids.
pub fn aligned_slice_range(
    grid: &MasterGrid,
    tile_bbox: &LLBBox,
) -> (usize, usize, usize, usize) {
    let mw = grid.width as i64;
    let mh = grid.height as i64;
    if mw < 2 || mh < 2 {
        return (0, 0, 0, 0);
    }
    let m_lng_range = grid.max_lng - grid.min_lng;
    let m_lat_range = grid.max_lat - grid.min_lat;

    let lng_to_col = |lng: f64| -> i64 {
        ((lng - grid.min_lng) / m_lng_range * (mw - 1) as f64).round() as i64
    };
    let lat_to_row = |lat: f64| -> i64 {
        ((grid.max_lat - lat) / m_lat_range * (mh - 1) as f64).round() as i64
    };

    let c_start = lng_to_col(tile_bbox.min().lng()).clamp(0, mw - 1) as usize;
    let c_end = lng_to_col(tile_bbox.max().lng()).clamp(0, mw - 1) as usize;
    let r_top = lat_to_row(tile_bbox.max().lat()).clamp(0, mh - 1) as usize;
    let r_bot = lat_to_row(tile_bbox.min().lat()).clamp(0, mh - 1) as usize;

    let (c_lo, c_hi) = (c_start.min(c_end), c_start.max(c_end));
    let (r_lo, r_hi) = (r_top.min(r_bot), r_top.max(r_bot));
    (c_lo, c_hi, r_lo, r_hi)
}

/// Direct (non-resampled) slice of the master grid covering the tile bbox.
/// Returns the slice and its bounds. Cells in the slice are master cells —
/// adjacent tiles produce slices that share their boundary cell, so MC
/// blocks rendered at the master seam pull elevation from the SAME master
/// cell on both sides.
pub fn aligned_slice(
    grid: &MasterGrid,
    tile_bbox: &LLBBox,
) -> (Vec<Vec<f64>>, usize, usize, usize, usize) {
    let (c_lo, c_hi, r_lo, r_hi) = aligned_slice_range(grid, tile_bbox);
    let w = c_hi - c_lo + 1;
    let h = r_hi - r_lo + 1;
    let mut out = vec![vec![f64::NAN; w]; h];
    for r in 0..h {
        let mr = r_lo + r;
        for c in 0..w {
            let mc = c_lo + c;
            out[r][c] = grid.data[mr * grid.width + mc] as f64;
        }
    }
    (out, c_lo, c_hi, r_lo, r_hi)
}
