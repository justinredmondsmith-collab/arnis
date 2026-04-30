pub mod cache;
mod master_grid;
pub mod postprocess;
pub mod provider;
pub mod providers;
pub mod selector;

use crate::{
    coordinate_system::{geographic::LLBBox, transformation::geo_distance},
    land_cover::LandCoverData,
    progress::emit_gui_progress_update,
};
use postprocess::{
    apply_land_cover_repair, fill_nan_values, filter_elevation_outliers, repair_terrain_anomalies,
    scale_to_minecraft,
};
use provider::ElevationProvider;
use selector::select_provider;

/// Holds processed elevation data and metadata
#[derive(Clone)]
pub struct ElevationData {
    /// Height values in Minecraft Y coordinates.
    ///
    /// Stored as `f32` on purpose: heights are already rounded to integer
    /// block Ys at placement time, so the full f64 precision was wasted on a
    /// grid that can easily hit 10+ million cells on a city-sized bbox
    /// (≈80 MB at f64, halved at f32). Postprocess still runs in f64 for
    /// numerical stability; the downcast happens once at construction.
    pub(crate) heights: Vec<Vec<f32>>,
    /// Width of the elevation grid (may be smaller than world width due to capping)
    pub(crate) width: usize,
    /// Height of the elevation grid (may be smaller than world height due to capping)
    pub(crate) height: usize,
    /// Width of the world in blocks (used for coordinate mapping)
    pub(crate) world_width: usize,
    /// Height of the world in blocks (used for coordinate mapping)
    pub(crate) world_height: usize,
}

/// Maximum elevation grid dimension requested from providers per axis.
///
/// This is the *stitched* ceiling — internally each single-request
/// provider (USGS 3DEP, IGN France, IGN Spain) splits the bbox into
/// sub-tiles that respect its own documented per-request cap and stitches
/// the results. Tile-based providers (AWS Terrain Tiles, Japan GSI)
/// already fetch at their native tile granularity and ignore this cap
/// structurally.
///
/// Per-provider single-request caps (defined in `providers::regional`
/// as `USGS_MAX_SINGLE` / `IGN_*_MAX_SINGLE` — see those constants for
/// the empirical measurements behind each value):
///   - USGS 3DEP (ArcGIS ImageServer): 2048 × 2048 per request
///     (documented cap is 8000, but the server returns HTTP 500 past
///     ~3000 even though the LiDAR is 1 m native; 2048 is the reliable
///     sweet spot)
///   - IGN France (WMS 1.3.0): 4096 × 4096 per request
///   - IGN Spain (WCS 2.0.1): 4096 × 4096 per request
///
/// PATCHED for arnis-tiler chunked sub-grid prefetch (Phase 3): bumped
/// from 16384 → 32768. Sub-grids in the chunked prefetch path are sized
/// to ~16500 cells each (master / K_x + edge_overlap), which exceeds the
/// original 16384 cap and got clamped — dropping ~150-250 cells off
/// each sub's east edge and producing visible 16 m+ cliffs at every
/// X sub-grid boundary in the assembled master grid.
///
/// 32768 covers bboxes up to ~1100 km² per sub at default scale (well
/// above our 16500-cell sub width), so the next bump is far away.
///
/// Memory note: a full 32768 × 32768 f64 grid is ~8 GB; peak with
/// water_blend_grid + repair snapshot is ~24 GB. Each chunked-prefetch
/// sub-process owns its own ~1 GB grid (sub width ~16500), so the cap
/// only matters for the unusual case of running arnis on a single bbox
/// of >268 km² without arnis-tiler. Typical use stays well under.
pub const MAX_ELEVATION_GRID_DIM: usize = 32768;

/// Compute world and grid dimensions for the given bbox and scale.
///
/// Exposed so callers (e.g. `Ground::new_enabled`) can fetch land cover at the
/// same dimensions as the elevation grid before elevation fetch starts.
///
/// Returns `(world_width, world_height, grid_width, grid_height)`.
pub fn compute_grid_dims(bbox: &LLBBox, scale: f64) -> (usize, usize, usize, usize) {
    // PATCHED for arnis-tiler master-grid alignment: when
    // ARNIS_TILE_OVERRIDE_DIMS="W,H" is set, return those dimensions
    // verbatim. This lets the per-tile grid (and the xzbbox via the
    // matching override in llbbox_to_xzbbox) be exactly sized to
    // span N master cells, so MC block at offset i samples master
    // cell (col_offset + i) directly and adjacent tiles agree at
    // their shared master cell.
    if let Ok(s) = std::env::var("ARNIS_TILE_OVERRIDE_DIMS") {
        let parts: Vec<&str> = s.split(',').collect();
        if parts.len() == 2 {
            if let (Ok(w), Ok(h)) = (parts[0].parse::<usize>(), parts[1].parse::<usize>()) {
                if w >= 2 && h >= 2 {
                    return (w, h, w.min(MAX_ELEVATION_GRID_DIM), h.min(MAX_ELEVATION_GRID_DIM));
                }
            }
        }
    }
    let (base_scale_z, base_scale_x) = geo_distance(bbox.min(), bbox.max());
    // Apply same floor() and scale operations as CoordTransformer.llbbox_to_xzbbox()
    let scale_factor_z: f64 = base_scale_z.floor() * scale;
    let scale_factor_x: f64 = base_scale_x.floor() * scale;
    // World block positions span 0..=scale_factor (inclusive), so there are
    // scale_factor+1 distinct positions.
    let world_width: usize = scale_factor_x as usize + 1;
    let world_height: usize = scale_factor_z as usize + 1;
    // Cap grid dimensions to avoid WMS server rejections.
    let grid_width: usize = world_width.clamp(2, MAX_ELEVATION_GRID_DIM);
    let grid_height: usize = world_height.clamp(2, MAX_ELEVATION_GRID_DIM);
    (world_width, world_height, grid_width, grid_height)
}

/// Fetch elevation data for the given bounding box.
///
/// Automatically selects the best available elevation provider for the region,
/// falling back to AWS Terrain Tiles for global coverage.
///
/// If `land_cover` is provided, applies land-cover-aware artifact repair
/// (water leveling, built-up smoothing) before scaling. This fixes LiDAR
/// classification errors at urban structures (tunnel portals, overpasses)
/// and coastal tile-boundary artifacts.
///
/// The returned ElevationData contains heights in Minecraft Y coordinates.
pub fn fetch_elevation_data(
    bbox: &LLBBox,
    scale: f64,
    ground_level: i32,
    disable_height_limit: bool,
    extended_max_y: i32,
    land_cover: Option<&mut LandCoverData>,
    aws_only: bool,
) -> Result<ElevationData, Box<dyn std::error::Error>> {
    let (world_width, world_height, grid_width, grid_height) = compute_grid_dims(bbox, scale);

    // arnis-tiler integration: when ARNIS_USE_ELEVATION_GRID points at a
    // master grid file (pre-fetched once for the metro bbox), bilinear-sample
    // it for THIS tile's bbox instead of running an independent USGS fetch.
    // Adjacent tiles read the same master cells at their shared edges, so
    // their elevation values agree to floating-point precision — no terrain
    // seams.
    if let Ok(grid_path) = std::env::var("ARNIS_USE_ELEVATION_GRID") {
        let path = std::path::PathBuf::from(&grid_path);

        // Fast path: if ARNIS_TILE_MASTER_OFFSET is set, load ONLY this
        // tile's slice from the master file. With a metro-scale master
        // grid this is the difference between a 19 GB Vec per arnis
        // process (8 parallel processes → OOM) and a ~1 GB slice. Each
        // tile only needs its own slice anyway.
        let direct_offset = std::env::var("ARNIS_TILE_MASTER_OFFSET")
            .ok()
            .and_then(|s| {
                let parts: Vec<&str> = s.split(',').collect();
                if parts.len() != 2 {
                    return None;
                }
                let c_off = parts[0].parse::<usize>().ok()?;
                let r_off = parts[1].parse::<usize>().ok()?;
                Some((c_off, r_off))
            });

        if let Some((c_off, r_off)) = direct_offset {
            match master_grid::load_grid_slice(&path, c_off, r_off, grid_width, grid_height) {
                Ok(mut height_grid) => {
                    emit_gui_progress_update(
                        16.0,
                        "Slicing elevation from master grid (arnis-tiler, slice-read)",
                    );
                    eprintln!(
                        "[arnis-tiler] direct master slice (slice-read) at ({},{}) size {}×{}",
                        c_off, r_off, grid_width, grid_height
                    );
                    fill_nan_values(&mut height_grid);

                    let mc_heights = scale_to_minecraft(
                        &height_grid,
                        scale,
                        ground_level,
                        disable_height_limit,
                        extended_max_y,
                    );
                    let heights_f32: Vec<Vec<f32>> = mc_heights
                        .into_iter()
                        .map(|row| row.into_iter().map(|v| v as f32).collect())
                        .collect();
                    return Ok(ElevationData {
                        heights: heights_f32,
                        width: grid_width,
                        height: grid_height,
                        world_width,
                        world_height,
                    });
                }
                Err(e) => {
                    eprintln!(
                        "ARNIS_USE_ELEVATION_GRID slice-read failed at {}: {} — falling back to full load",
                        grid_path, e
                    );
                }
            }
        }

        // Fallback: ARNIS_TILE_MASTER_OFFSET not set (e.g. caller wants
        // bilinear interpolation rather than direct master-cell sampling).
        // Load the full grid, then either bilinear-resample or direct-slice.
        match master_grid::load_grid(&path) {
            Ok(master) => {
                emit_gui_progress_update(
                    16.0,
                    "Slicing elevation from master grid (arnis-tiler, full-load)",
                );
                let mut height_grid =
                    master_grid::slice_for_tile(&master, bbox, grid_width, grid_height);
                fill_nan_values(&mut height_grid);
                let mc_heights = scale_to_minecraft(
                    &height_grid,
                    scale,
                    ground_level,
                    disable_height_limit,
                    extended_max_y,
                );
                let heights_f32: Vec<Vec<f32>> = mc_heights
                    .into_iter()
                    .map(|row| row.into_iter().map(|v| v as f32).collect())
                    .collect();
                return Ok(ElevationData {
                    heights: heights_f32,
                    width: grid_width,
                    height: grid_height,
                    world_width,
                    world_height,
                });
            }
            Err(e) => {
                eprintln!(
                    "ARNIS_USE_ELEVATION_GRID set but failed to load {}: {} — falling back to per-tile fetch",
                    grid_path, e
                );
            }
        }
    }

    // Select the best provider for this region. When `aws_only` is set the
    // user opted out of the regional high-res providers in favor of a faster
    // run, so we skip straight to AWS Terrain Tiles.
    let provider = select_provider(bbox, aws_only);
    let provider_name = provider.name();
    let is_fallback = provider_name == "aws";

    emit_gui_progress_update(16.0, "Fetching elevation...");

    // Fetch raw elevation data in meters, falling back to AWS on regional provider failure
    let raw = match provider.fetch_raw(bbox, grid_width, grid_height) {
        Ok(raw) if !is_fallback => {
            // Check if the regional provider returned mostly empty data (out-of-coverage area).
            // This catches cases where the provider's rectangular bbox over-claims coverage
            // (e.g., IGN France bbox covers Belgium, but returns no data for Belgian coordinates).
            let nan_ratio = compute_nan_ratio(&raw.heights_meters);
            if nan_ratio > 0.5 {
                eprintln!(
                    "Warning: Regional provider '{}' returned {:.0}% empty data. Falling back to AWS Terrain Tiles.",
                    provider_name, nan_ratio * 100.0
                );
                #[cfg(feature = "gui")]
                crate::telemetry::send_log(
                    crate::telemetry::LogLevel::Warning,
                    &format!(
                        "Regional provider '{}' returned mostly empty data, using AWS fallback.",
                        provider_name
                    ),
                );
                let fallback = providers::aws_terrain::AwsTerrain;
                fallback.fetch_raw(bbox, grid_width, grid_height)?
            } else {
                raw
            }
        }
        Ok(raw) => raw,
        Err(e) if !is_fallback => {
            eprintln!(
                "Warning: Regional provider '{}' failed: {}. Falling back to AWS Terrain Tiles.",
                provider_name, e
            );
            #[cfg(feature = "gui")]
            crate::telemetry::send_log(
                crate::telemetry::LogLevel::Warning,
                &format!(
                    "Regional elevation provider '{}' failed, using AWS fallback.",
                    provider_name
                ),
            );
            let fallback = providers::aws_terrain::AwsTerrain;
            emit_gui_progress_update(16.0, "Regional provider failed, fetching from AWS...");
            fallback.fetch_raw(bbox, grid_width, grid_height)?
        }
        Err(e) => return Err(e),
    };

    emit_gui_progress_update(17.0, "Processing elevation...");

    // Shared post-processing pipeline
    let mut height_grid = raw.heights_meters;
    // PATCHED for tiled rendering (arnis-tiler):
    // The IQR outlier filter computes Q1/Q3 from this tile's own elevation
    // distribution, so adjacent tiles end up filtering different cells at
    // their shared edges — same physical location, different filtered
    // values, visible terrain seams when tiles are stitched.
    //
    // Skipping the IQR filter when ARNIS_TILED_RENDER=1 keeps the raw
    // USGS values intact; tiny outliers may show up as 1-2 block bumps
    // but those are far less visible than a 10-block tile-boundary
    // cliff. The arnis-tiler runner sets this env var; standalone
    // single-bbox runs preserve the original filter behavior.
    let tiled_render = std::env::var("ARNIS_TILED_RENDER").ok().as_deref() == Some("1");
    if !tiled_render {
        filter_elevation_outliers(&mut height_grid);
        // 5x5 median filter — tile-edge neighborhoods differ between
        // adjacent tiles, producing different filtered values at the seam.
        repair_terrain_anomalies(&mut height_grid);
    }
    // fill_nan_values uses neighbor cells to fill missing data; safe to
    // run unconditionally because USGS 3DEP rarely has NaN cells, and
    // when it does the fill propagates only locally. Skip however since
    // any neighbor-based op contributes to seam variance.
    if !tiled_render {
        fill_nan_values(&mut height_grid);
    }

    // arnis-tiler integration: serialize the post-processed master grid for
    // re-use by per-tile runs. Triggered by ARNIS_SAVE_ELEVATION_GRID;
    // ARNIS_FETCH_ONLY=1 cleanly exits after save (skipping world generation
    // entirely) so the master fetch is just an elevation extraction and
    // doesn't waste compute generating a throwaway world.
    if let Ok(save_path) = std::env::var("ARNIS_SAVE_ELEVATION_GRID") {
        let path = std::path::PathBuf::from(&save_path);
        match master_grid::save_grid(&path, bbox, scale, &height_grid) {
            Ok(()) => eprintln!(
                "Saved master elevation grid: {} ({}x{} cells)",
                save_path,
                if height_grid.is_empty() { 0 } else { height_grid[0].len() },
                height_grid.len()
            ),
            Err(e) => eprintln!("Failed to save elevation grid to {}: {}", save_path, e),
        }
        if std::env::var("ARNIS_FETCH_ONLY").ok().as_deref() == Some("1") {
            std::process::exit(0);
        }
    }

    // Land-cover-aware repair: built-up Gaussian smoothing targets urban
    // LiDAR/DSM classification errors, coastal pull-down flattens the
    // shoreline cliff across all land classes.
    //
    // Both scales are in meters and converted to grid cells via the actual
    // meters-per-cell, so the smoothing covers the same physical scale
    // regardless of world size or provider resolution.
    //
    // σ = 30 m for the built-up Gaussian: wide enough that a typical
    // 20 m-wide DSM artifact (tunnel portal, overpass, parking deck) is
    // reduced to a residual indistinguishable from one Minecraft block.
    // Hilly cities (SF, Pittsburgh) still keep their macro shape — the
    // kernel falls off long before a real urban slope does. On coarse
    // providers (AWS fallback when σ < 1.5 cells) the Gaussian pass is
    // skipped internally.
    //
    // 25 m coastal pull range: short enough to leave the inland interior
    // alone, long enough that a 7-10 m urban embankment (Munich Isar,
    // Vienna Donaukanal) becomes a slope-tier-free ramp instead of a
    // cliff with stepped stone walls.
    const BUILT_UP_SIGMA_M: f64 = 30.0;
    const COASTAL_PULL_M: f64 = 25.0;
    let (bbox_height_m, bbox_width_m) = geo_distance(bbox.min(), bbox.max());
    let m_per_cell = (bbox_width_m / grid_width as f64 + bbox_height_m / grid_height as f64) * 0.5;
    let (built_up_sigma_cells, coastal_pull_cells) = if m_per_cell > 0.0 {
        (
            BUILT_UP_SIGMA_M / m_per_cell,
            (COASTAL_PULL_M / m_per_cell).round() as u32,
        )
    } else {
        (0.0, 0)
    };

    // PATCHED for tiled rendering: the land-cover gaussian blur (σ=30m)
    // has the same boundary-condition problem — kernels at tile edges
    // see different neighborhoods than they would in a master-grid pass,
    // so the same physical cell ends up with different smoothed values
    // in adjacent tiles. Skip when ARNIS_TILED_RENDER=1.
    if !tiled_render {
        if let Some(lc) = land_cover {
            apply_land_cover_repair(
                &mut height_grid,
                lc,
                built_up_sigma_cells,
                coastal_pull_cells,
            );
        }
    }

    let mc_heights = scale_to_minecraft(
        &height_grid,
        scale,
        ground_level,
        disable_height_limit,
        extended_max_y,
    );

    // Log min/max block heights
    let mut min_block_height = f64::MAX;
    let mut max_block_height = f64::MIN;
    for row in &mc_heights {
        for &height in row {
            if height.is_finite() {
                min_block_height = min_block_height.min(height);
                max_block_height = max_block_height.max(height);
            }
        }
    }

    // Downcast the f64 postprocess output to the f32 storage format. One-time
    // cost paid here so the large grid sits at half the memory for the rest
    // of the generation run. NaN/infinity preservation is a requirement —
    // downstream `is_finite` checks rely on non-finite sentinels surviving.
    let mc_heights_f32: Vec<Vec<f32>> = mc_heights
        .into_iter()
        .map(|row| row.into_iter().map(|v| v as f32).collect())
        .collect();

    Ok(ElevationData {
        heights: mc_heights_f32,
        width: grid_width,
        height: grid_height,
        world_width,
        world_height,
    })
}

/// Clean up old cached elevation tiles/files from all providers.
pub fn cleanup_old_cached_tiles() {
    cache::cleanup_old_cached_files();
}

/// Compute the fraction of NaN/non-finite values in a height grid (0.0 to 1.0).
fn compute_nan_ratio(heights: &[Vec<f64>]) -> f64 {
    let mut total = 0usize;
    let mut nan_count = 0usize;
    for row in heights {
        for &h in row {
            total += 1;
            if !h.is_finite() {
                nan_count += 1;
            }
        }
    }
    if total == 0 {
        return 1.0;
    }
    nan_count as f64 / total as f64
}
