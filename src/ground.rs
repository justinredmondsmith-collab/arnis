use crate::args::Args;
use crate::canopy::{self, CanopyData};
use crate::coordinate_system::{
    cartesian::{XZBBox, XZPoint},
    geographic::LLBBox,
};
use crate::elevation::compute_grid_dims;
use crate::elevation_data::ElevationData;
use crate::land_cover::{self, LandCoverData};
use crate::osm_parser::ProcessedElement;
#[cfg(feature = "gui")]
use crate::telemetry::{send_log, LogLevel};
use colored::Colorize;
use image::{Rgb, RgbImage};

/// Parameters describing the inverse-rotation needed to check whether a world
/// coordinate falls inside the original (pre-rotation) bounding box.
#[derive(Clone)]
pub struct RotationMask {
    /// Center of rotation (world coordinates)
    pub cx: f64,
    pub cz: f64,
    /// sin/cos of the *negative* angle (inverse rotation)
    pub neg_sin: f64,
    pub cos: f64,
    /// Original axis-aligned bounding box before rotation
    pub orig_min_x: i32,
    pub orig_max_x: i32,
    pub orig_min_z: i32,
    pub orig_max_z: i32,
}

/// Global carve floors chosen before terrain and OSM postprocessing.
#[derive(Clone)]
struct ExportContext {
    water_floor: i32,
    sink_floor: i32,
    aws_only: bool,
}

/// Represents terrain data, land cover classification, and elevation settings
#[derive(Clone)]
pub struct Ground {
    pub elevation_enabled: bool,
    ground_level: i32,
    elevation_data: Option<ElevationData>,
    land_cover: Option<LandCoverData>,
    /// `None` when the option is off or the fetch failed, which restores the
    /// land-cover-only behaviour.
    canopy: Option<CanopyData>,
    /// World size in blocks, used to map coords onto the land-cover grid when elevation data is absent (flat mode).
    world_width: usize,
    world_height: usize,
    /// When set, coordinates outside the rotated original bbox are skipped.
    rotation_mask: Option<RotationMask>,
    /// Processed master terrain must not be normalized or repaired per tile.
    immutable_master: bool,
    master_offset: Option<(i32, i32)>,
    master_elevation: Option<(std::sync::Arc<[f32]>, usize, usize)>,
    export_context: Option<ExportContext>,
    coastal_protection: Option<crate::coastal::CoastalProtection>,
    /// Minecraft Y at/above which terrain is snow-capped; `i32::MAX` disables it.
    snow_threshold_y: i32,
    /// Climate at the bbox center, driving arid/polar surface palettes and biomes.
    climate: crate::climate::Climate,
}

/// Climatic snow line in metres by absolute latitude, piecewise-linear through
/// the cited anchors: equator 4500, subtropics (25 deg) 5700, mid-latitudes
/// (46 deg) 3000, poles 0. Source: Wikipedia "Snow line".
fn snow_line_meters(lat_deg: f64) -> f64 {
    let a = lat_deg.abs().min(90.0);
    if a <= 25.0 {
        4500.0 + (5700.0 - 4500.0) * (a / 25.0)
    } else if a <= 46.0 {
        5700.0 + (3000.0 - 5700.0) * ((a - 25.0) / (46.0 - 25.0))
    } else {
        (3000.0 * (1.0 - (a - 46.0) / (90.0 - 46.0))).max(0.0)
    }
}

/// Minecraft Y threshold for the snow line at this latitude, inverting the
/// affine metre->Y scaling. Returns `i32::MAX` (never) / `i32::MIN` (always)
/// for the flat-terrain extremes.
fn snow_threshold_for(ed: &ElevationData, lat_deg: f64, ground_level: i32) -> i32 {
    let snowline = snow_line_meters(lat_deg);
    if ed.blocks_per_meter <= 0.0 {
        return if ed.min_height_m >= snowline {
            i32::MIN
        } else {
            i32::MAX
        };
    }
    (ground_level as f64 + (snowline - ed.min_height_m) * ed.blocks_per_meter).round() as i32
}

impl Ground {
    pub(crate) fn master_geometry(&self) -> Option<crate::clipping::MasterGeometry> {
        let (_, width, height) = self.master_elevation.as_ref()?;
        Some(crate::clipping::MasterGeometry {
            bounds: XZBBox::rect_from_min_max(0, 0, *width as i32 - 1, *height as i32 - 1)
                .expect("admitted master dimensions"),
            offset: self.master_offset.expect("admitted master offset"),
        })
    }

    pub(crate) fn master_offset(&self) -> Option<(i32, i32)> {
        self.master_offset
    }

    pub(crate) fn is_external_tile(&self) -> bool {
        self.immutable_master
    }

    /// Terrain base actually in use. Differs from `args.ground_level` when the elevation
    /// scaler sank the base to reach the extended floor, so anything inverting the
    /// metre->Y affine (snow line, montane trees, filler chunks) must read it from here.
    pub fn base_level(&self) -> i32 {
        self.ground_level
    }

    #[cfg(test)]
    pub fn new_flat(ground_level: i32) -> Self {
        Self {
            elevation_enabled: false,
            ground_level,
            elevation_data: None,
            land_cover: None,
            canopy: None,
            world_width: 0,
            world_height: 0,
            rotation_mask: None,
            immutable_master: false,
            master_offset: None,
            master_elevation: None,
            export_context: None,
            coastal_protection: None,
            snow_threshold_y: i32::MAX,
            climate: crate::climate::Climate::Temperate,
        }
    }

    /// Flat ground (no elevation) that still carries land cover, so water bodies and land-cover surfaces render at the flat surface level.
    pub fn new_flat_with_land_cover(
        bbox: &LLBBox,
        scale: f64,
        ground_level: i32,
        canopy_height: bool,
    ) -> Self {
        let (world_w, world_h, grid_w, grid_h) = compute_grid_dims(bbox, scale);
        // Canopy depends on neither, so it downloads alongside the land cover.
        let (land_cover, canopy) = std::thread::scope(|s| {
            let job =
                canopy_height.then(|| s.spawn(|| canopy::fetch_canopy_data(bbox, grid_w, grid_h)));
            let lc = land_cover::fetch_land_cover_data(bbox, grid_w, grid_h);
            (lc, job.and_then(|h| h.join().ok()).flatten())
        });
        if land_cover.is_none() {
            eprintln!("Land cover fetch failed; generating flat ground without it.");
        }
        Self {
            elevation_enabled: false,
            ground_level,
            elevation_data: None,
            land_cover,
            canopy,
            world_width: world_w,
            world_height: world_h,
            rotation_mask: None,
            immutable_master: false,
            master_offset: None,
            master_elevation: None,
            export_context: None,
            coastal_protection: None,
            snow_threshold_y: i32::MAX,
            climate: crate::climate::Climate::classify(bbox),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_flat_land_cover_test(
        land_cover: LandCoverData,
        world_width: usize,
        world_height: usize,
    ) -> Self {
        Self {
            elevation_enabled: false,
            ground_level: 0,
            elevation_data: None,
            land_cover: Some(land_cover),
            canopy: None,
            world_width,
            world_height,
            rotation_mask: None,
            immutable_master: false,
            master_offset: None,
            master_elevation: None,
            export_context: None,
            coastal_protection: None,
            snow_threshold_y: i32::MAX,
            climate: crate::climate::Climate::Temperate,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_elevation_test(
        heights: Vec<Vec<f32>>,
        world_width: usize,
        world_height: usize,
    ) -> Self {
        let grid_height = heights.len();
        let grid_width = heights.first().map(Vec::len).unwrap_or(0);
        assert!(
            grid_height > 0 && grid_width > 0,
            "heights must be non-empty"
        );
        assert!(
            world_width > 0 && world_height > 0,
            "world dims must be > 0"
        );
        assert!(
            heights.iter().all(|r| r.len() == grid_width),
            "heights must be rectangular"
        );
        Self {
            elevation_enabled: true,
            ground_level: 0,
            elevation_data: Some(crate::elevation::ElevationData {
                heights,
                width: grid_width,
                height: grid_height,
                world_width,
                world_height,
                min_height_m: 0.0,
                blocks_per_meter: 1.0,
                ground_level: 0,
            }),
            land_cover: None,
            canopy: None,
            world_width,
            world_height,
            rotation_mask: None,
            immutable_master: false,
            master_offset: None,
            master_elevation: None,
            export_context: None,
            coastal_protection: None,
            snow_threshold_y: i32::MAX,
            climate: crate::climate::Climate::Temperate,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_enabled(
        bbox: &LLBBox,
        scale: f64,
        ground_level: i32,
        min_ground_level: i32,
        disable_height_limit: bool,
        extended_max_y: i32,
        aws_only_elevation: bool,
        benchmark: bool,
        canopy_height: bool,
    ) -> Self {
        Self::new_enabled_with_sources(
            bbox,
            scale,
            ground_level,
            min_ground_level,
            disable_height_limit,
            extended_max_y,
            aws_only_elevation,
            benchmark,
            canopy_height,
            None,
        )
        .expect("stock ground retains fallback")
    }

    #[allow(clippy::too_many_arguments)]
    fn new_enabled_with_sources(
        bbox: &LLBBox,
        scale: f64,
        ground_level: i32,
        min_ground_level: i32,
        disable_height_limit: bool,
        extended_max_y: i32,
        aws_only_elevation: bool,
        benchmark: bool,
        canopy_height: bool,
        sources: Option<&crate::tiler_contract::AdmittedSources>,
    ) -> Result<Self, String> {
        if sources.is_some() && (!aws_only_elevation || canopy_height) {
            return Err("Frozen ground requires AWS-only without canopy".into());
        }
        let mut bench = crate::bench::Bench::new(benchmark);
        // Fetch land cover FIRST so we can feed it into the elevation
        // post-processing pipeline for land-cover-aware artifact repair.
        // The elevation grid is built from the same (bbox, scale) so both
        // grids share dimensions (both use compute_grid_dims).
        let (world_w, world_h, grid_w, grid_h) = compute_grid_dims(bbox, scale);
        // Canopy needs neither of the other two, so it downloads behind both.
        std::thread::scope(|scope| {
            let canopy_job = canopy_height
                .then(|| scope.spawn(|| canopy::fetch_canopy_data(bbox, grid_w, grid_h)));
            let mut land_cover = {
                let lc = if sources.is_some() {
                    Some(
                        land_cover::fetch_land_cover_data_with_sources(
                            bbox, grid_w, grid_h, sources,
                        )
                        .map_err(|e| e.to_string())?,
                    )
                } else {
                    land_cover::fetch_land_cover_data(bbox, grid_w, grid_h)
                };
                if lc.is_some() {
                    println!("Land cover data loaded successfully");
                } else {
                    eprintln!("Warning: Land cover data unavailable, using default ground blocks");
                }
                lc
            };
            bench.mark("elev_landcover_fetch");

            // Raise the floor for the deepest water carve (elevation path only).
            let carve_floor = match &land_cover {
                Some(lc) => {
                    let max_depth =
                        crate::water_depth::estimate_max_carve_depth(&lc.grid, world_w, world_h);
                    crate::world_editor::min_y() + max_depth + 2
                }
                None => crate::world_editor::min_y(),
            };
            let water_floor = ground_level.max(carve_floor);
            // The terrain may sink to reach an extended floor, but never below the carve floor:
            // water would otherwise be cut straight through the bedrock layer.
            let sink_floor = min_ground_level.max(carve_floor).min(water_floor);

            let source_mode = if aws_only_elevation {
                crate::elevation::SourceMode::AwsOnly
            } else {
                crate::elevation::SourceMode::Auto
            };
            let mut coastal_protection = None;
            let elevation = if sources.is_some() {
                crate::elevation::fetch_elevation_data_with_sources(
                    bbox,
                    scale,
                    water_floor,
                    sink_floor,
                    disable_height_limit,
                    extended_max_y,
                    land_cover.as_mut(),
                    source_mode,
                    benchmark,
                    sources,
                    Some(&mut coastal_protection),
                )
            } else {
                crate::elevation_data::fetch_elevation_data(
                    bbox,
                    scale,
                    water_floor,
                    sink_floor,
                    disable_height_limit,
                    extended_max_y,
                    land_cover.as_mut(),
                    source_mode,
                    benchmark,
                )
            };
            match elevation {
                Ok(elevation_data) => {
                    let lat = (bbox.min().lat() + bbox.max().lat()) / 2.0;
                    // Must use the base the scaler actually settled on: snow_threshold_for
                    // inverts that exact affine, so a mismatched base misplaces every snow cap.
                    let base = elevation_data.ground_level;
                    let snow_threshold_y = snow_threshold_for(&elevation_data, lat, base);
                    let canopy = canopy_job.and_then(|h| h.join().ok()).flatten();
                    Ok(Self {
                        elevation_enabled: true,
                        ground_level: base,
                        elevation_data: Some(elevation_data),
                        land_cover,
                        canopy,
                        world_width: world_w,
                        world_height: world_h,
                        rotation_mask: None,
                        immutable_master: false,
                        master_offset: None,
                        master_elevation: None,
                        coastal_protection,
                        export_context: Some(ExportContext {
                            water_floor,
                            sink_floor,
                            aws_only: aws_only_elevation,
                        }),
                        snow_threshold_y,
                        climate: crate::climate::Climate::classify(bbox),
                    })
                }
                Err(e) => {
                    if sources.is_some() {
                        return Err(e.to_string());
                    }
                    eprintln!("Failed to fetch elevation data: {}", e);
                    #[cfg(feature = "gui")]
                    {
                        let short: String = e.to_string().chars().take(200).collect();
                        send_log(
                            LogLevel::Warning,
                            &format!("Elevation unavailable, using flat ground ({short})"),
                        );
                    }
                    // Graceful fallback: disable elevation and keep provided ground_level.
                    // Land cover we already fetched is discarded since it has no
                    // elevation grid to align against.
                    // Still has to be collected before the scope can close.
                    drop(canopy_job.and_then(|h| h.join().ok()));
                    Ok(Self {
                        elevation_enabled: false,
                        ground_level,
                        elevation_data: None,
                        land_cover: None,
                        canopy: None,
                        world_width: 0,
                        world_height: 0,
                        rotation_mask: None,
                        immutable_master: false,
                        master_offset: None,
                        master_elevation: None,
                        export_context: None,
                        coastal_protection: None,
                        snow_threshold_y: i32::MAX,
                        climate: crate::climate::Climate::classify(bbox),
                    })
                }
            }
        })
    }

    /// Minecraft Y at/above which terrain is snow-capped (`i32::MAX` = never,
    /// `i32::MIN` = always, e.g. a flat plateau above the snow line).
    #[inline(always)]
    pub fn snow_threshold_y(&self) -> i32 {
        self.snow_threshold_y
    }

    /// Climate at the bbox center (Temperate keeps the existing surface/biome behaviour).
    #[inline(always)]
    pub fn climate(&self) -> crate::climate::Climate {
        self.climate
    }

    /// Returns whether land cover data is available
    #[inline(always)]
    pub fn has_land_cover(&self) -> bool {
        self.land_cover.is_some()
    }

    /// Returns whether canopy height data is available.
    #[inline(always)]
    pub fn has_canopy(&self) -> bool {
        self.canopy.is_some()
    }

    /// Canopy top in metres, or `None` where the map has no measurement.
    /// A measured zero is bare ground, not missing.
    #[inline(always)]
    pub fn canopy_height_m(&self, coord: XZPoint) -> Option<u8> {
        let ch = self.canopy.as_ref()?;
        let (world_w, world_h) = self.world_dims();
        let x_ratio = (coord.x as f64 / (world_w - 1).max(1) as f64).clamp(0.0, 1.0);
        let z_ratio = (coord.z as f64 / (world_h - 1).max(1) as f64).clamp(0.0, 1.0);
        let x = ((x_ratio * (ch.width - 1) as f64).round() as usize).min(ch.width - 1);
        let z = ((z_ratio * (ch.height - 1) as f64).round() as usize).min(ch.height - 1);
        match ch.at(x, z) {
            canopy::CANOPY_NODATA => None,
            h => Some(h),
        }
    }

    /// Share of a `span` square at `origin` that carries canopy, or `None` where
    /// nothing in it was measured. Unmeasured columns stay out of the average,
    /// so a cell with none at all hands the decision back to the land cover.
    pub fn canopy_fraction(&self, origin: XZPoint, span: i32) -> Option<f64> {
        self.canopy.as_ref()?;
        if span <= 0 {
            return None;
        }
        let (mut measured, mut wooded) = (0u32, 0u32);
        for dz in 0..span {
            for dx in 0..span {
                if let Some(h) = self.canopy_height_m(XZPoint::new(origin.x + dx, origin.z + dz)) {
                    measured += 1;
                    if h >= canopy::CANOPY_MIN_M {
                        wooded += 1;
                    }
                }
            }
        }
        (measured > 0).then(|| f64::from(wooded) / f64::from(measured))
    }

    /// Force LC_WATER inside OSM water, sinking those cells onto that water's surface.
    pub fn apply_osm_water_override(&mut self, elements: &[ProcessedElement], xzbbox: &XZBBox) {
        if self.immutable_master {
            return;
        }
        let Ground {
            land_cover,
            elevation_data,
            ..
        } = self;
        let (Some(lc), Some(data)) = (land_cover.as_mut(), elevation_data.as_mut()) else {
            return;
        };
        let (world_width, world_height) = (data.world_width, data.world_height);
        crate::land_cover::osm_water_override::apply_osm_water_override(
            lc,
            &mut data.heights,
            world_width,
            world_height,
            elements,
            xzbbox,
        );
    }

    /// Trim ESA water back to land where OSM shows roads, buildings or its own shoreline.
    pub fn apply_osm_land_override(
        &mut self,
        elements: &[ProcessedElement],
        xzbbox: &XZBBox,
        scale: f64,
    ) {
        if self.immutable_master {
            return;
        }
        let (world_width, world_height) = self.world_dims();
        let Some(lc) = self.land_cover.as_mut() else {
            return;
        };
        crate::land_cover::osm_land_override::apply_osm_land_override(
            lc,
            world_width,
            world_height,
            elements,
            xzbbox,
            scale,
        );
    }

    /// Reclassify cells under bridges to the surrounding class, sinking new water.
    pub fn apply_bridge_land_cover_repair(
        &mut self,
        elements: &[ProcessedElement],
        xzbbox: &XZBBox,
        scale: f64,
    ) {
        if self.immutable_master {
            return;
        }
        let Ground {
            land_cover,
            elevation_data,
            ..
        } = self;
        let (Some(lc), Some(data)) = (land_cover.as_mut(), elevation_data.as_mut()) else {
            return;
        };
        let (world_width, world_height) = (data.world_width, data.world_height);
        crate::land_cover::bridge_repair::apply_bridge_land_cover_repair(
            lc,
            &mut data.heights,
            world_width,
            world_height,
            elements,
            xzbbox,
            scale,
        );
    }

    /// Local block bbox (min_x, min_z, max_x, max_z) covering all LC_WATER cells,
    /// derived from the land-cover grid; None if no land cover or no water.
    pub fn lc_water_block_bounds(&self) -> Option<(i32, i32, i32, i32)> {
        let (lc, data) = match (&self.land_cover, &self.elevation_data) {
            (Some(lc), Some(data)) => (lc, data),
            _ => return None,
        };
        let (mut gx0, mut gz0, mut gx1, mut gz1) = (usize::MAX, usize::MAX, 0usize, 0usize);
        let mut any = false;
        for (z, row) in lc.grid.iter().enumerate() {
            for (x, &c) in row.iter().enumerate() {
                if c == land_cover::LC_WATER {
                    gx0 = gx0.min(x);
                    gx1 = gx1.max(x);
                    gz0 = gz0.min(z);
                    gz1 = gz1.max(z);
                    any = true;
                }
            }
        }
        if !any {
            return None;
        }
        let (x0, x1) =
            crate::water_depth::grid_span_to_block_span(gx0, gx1, data.world_width, lc.width);
        let (z0, z1) =
            crate::water_depth::grid_span_to_block_span(gz0, gz1, data.world_height, lc.height);
        Some((x0, z0, x1, z1))
    }

    /// World size in blocks for land-cover mapping: from elevation data when present, else the stored flat-mode dims.
    #[inline(always)]
    pub(crate) fn world_dims(&self) -> (usize, usize) {
        match &self.elevation_data {
            Some(d) => (d.world_width, d.world_height),
            None => (self.world_width, self.world_height),
        }
    }

    /// Returns the ESA WorldCover land cover class at the given coordinates.
    /// Returns 0 if land cover data is not available.
    #[inline(always)]
    pub fn cover_class(&self, coord: XZPoint) -> u8 {
        if let Some(ref lc) = self.land_cover {
            if self.immutable_master {
                let x = (coord.x.max(0) as usize).min(lc.width - 1);
                let z = (coord.z.max(0) as usize).min(lc.height - 1);
                return lc.grid[z][x];
            }
            let (world_w, world_h) = self.world_dims();
            let x_ratio = (coord.x as f64 / (world_w - 1).max(1) as f64).clamp(0.0, 1.0);
            let z_ratio = (coord.z as f64 / (world_h - 1).max(1) as f64).clamp(0.0, 1.0);
            let x = ((x_ratio * (lc.width - 1) as f64).round() as usize).min(lc.width - 1);
            let z = ((z_ratio * (lc.height - 1) as f64).round() as usize).min(lc.height - 1);
            lc.grid[z][x]
        } else {
            0
        }
    }

    /// Returns the water distance-to-shore value at the given coordinates.
    /// 0 = non-water, 1 = shore, 2+ = progressively deeper water.
    #[inline(always)]
    pub fn water_distance(&self, coord: XZPoint) -> u8 {
        if let Some(ref lc) = self.land_cover {
            if self.immutable_master {
                let x = (coord.x.max(0) as usize).min(lc.width - 1);
                let z = (coord.z.max(0) as usize).min(lc.height - 1);
                return lc.water_distance[z][x];
            }
            let (world_w, world_h) = self.world_dims();
            let x_ratio = (coord.x as f64 / (world_w - 1).max(1) as f64).clamp(0.0, 1.0);
            let z_ratio = (coord.z as f64 / (world_h - 1).max(1) as f64).clamp(0.0, 1.0);
            let x = ((x_ratio * (lc.width - 1) as f64).round() as usize).min(lc.width - 1);
            let z = ((z_ratio * (lc.height - 1) as f64).round() as usize).min(lc.height - 1);
            lc.water_distance[z][x]
        } else {
            0
        }
    }

    /// True for a water cell at least four cells from shore. `water_distance` is 0 past
    /// its cap of 15, so 0 counts as interior here. Tells a step inside a body from a bank.
    #[inline(always)]
    pub fn is_interior_water(&self, coord: XZPoint) -> bool {
        if self.cover_class(coord) != land_cover::LC_WATER {
            return false;
        }
        let d = self.water_distance(coord);
        d == 0 || d >= 4
    }

    /// Returns a continuous 0.0–1.0 value indicating how "watery" a block is,
    /// using bilinear interpolation of the water classification grid.
    ///
    /// Nearest-neighbor grid lookups (`cover_class`) create rectangular water
    /// edges when the grid is coarser than block resolution.  Bilinear
    /// interpolation produces a smooth gradient across grid cell boundaries,
    /// allowing noise-based thresholding to create organic shorelines.
    #[inline(always)]
    pub fn water_blend(&self, coord: XZPoint) -> f64 {
        if let Some(ref lc) = self.land_cover {
            if self.immutable_master {
                let x = (coord.x.max(0) as usize).min(lc.width - 1);
                let z = (coord.z.max(0) as usize).min(lc.height - 1);
                return lc.water_blend_grid()[z][x] as f64;
            }
            let (world_w, world_h) = self.world_dims();
            // Continuous grid coordinates (no rounding — that's the key difference
            // from cover_class which uses .round())
            let fx = (coord.x as f64 / (world_w - 1).max(1) as f64).clamp(0.0, 1.0)
                * (lc.width - 1) as f64;
            let fz = (coord.z as f64 / (world_h - 1).max(1) as f64).clamp(0.0, 1.0)
                * (lc.height - 1) as f64;

            let x0 = (fx.floor() as usize).min(lc.width - 1);
            let x1 = (x0 + 1).min(lc.width - 1);
            let z0 = (fz.floor() as usize).min(lc.height - 1);
            let z1 = (z0 + 1).min(lc.height - 1);

            let tx = fx - fx.floor();
            let tz = fz - fz.floor();

            // Sample pre-smoothed water-ness at the 4 surrounding grid cells.
            // The grid was Gaussian-blurred from the binary LC_WATER mask so
            // that even at integer block positions (1-to-1 grid-to-world
            // mapping, where tx == tz == 0 below) the sampled value is
            // continuous — the renderer's hard `> 0.5` threshold then traces
            // a clean curved shoreline contour instead of the raw ESA 10 m
            // rectangular grid edge.
            // Widen f32 storage to f64 for the bilinear arithmetic. This
            // doesn't recover the ~10⁻⁷ precision lost at storage, but it
            // prevents extra rounding from accumulating in the four
            // multiply-adds + the threshold comparison downstream.
            let wb = lc.water_blend_grid();
            let w00 = wb[z0][x0] as f64;
            let w10 = wb[z0][x1] as f64;
            let w01 = wb[z1][x0] as f64;
            let w11 = wb[z1][x1] as f64;

            // Bilinear interpolation
            let top = w00 * (1.0 - tx) + w10 * tx;
            let bottom = w01 * (1.0 - tx) + w11 * tx;
            top * (1.0 - tz) + bottom * tz
        } else {
            0.0
        }
    }

    /// Computes terrain slope at the given coordinates.
    ///
    /// Slope is the difference between the maximum and minimum elevation of
    /// 4 cardinal neighbors sampled at a step distance. Higher values indicate
    /// steeper terrain.
    ///
    /// Returns 0 if elevation data is not available.
    #[inline(always)]
    pub fn slope(&self, coord: XZPoint) -> i32 {
        if !self.elevation_enabled {
            return 0;
        }

        const STEP: i32 = 4;
        let east = self.level(XZPoint::new(coord.x + STEP, coord.z));
        let west = self.level(XZPoint::new(coord.x - STEP, coord.z));
        let north = self.level(XZPoint::new(coord.x, coord.z - STEP));
        let south = self.level(XZPoint::new(coord.x, coord.z + STEP));

        let max_val = east.max(west).max(north).max(south);
        let min_val = east.min(west).min(north).min(south);
        // Saturate: pathological CLI input (e.g. very negative ground_level)
        // can push max - min past i32::MAX.
        max_val.saturating_sub(min_val)
    }

    /// Returns the ground level at the given coordinates
    #[inline(always)]
    pub fn level(&self, coord: XZPoint) -> i32 {
        if !self.elevation_enabled || self.elevation_data.is_none() {
            return self.ground_level;
        }

        let data: &ElevationData = self.elevation_data.as_ref().unwrap();
        if let Some((heights, width, height)) = &self.master_elevation {
            let (col, row) = self.master_offset.expect("admitted master offset");
            let x = (i64::from(coord.x) + i64::from(col)).clamp(0, *width as i64 - 1) as usize;
            let z = (i64::from(coord.z) + i64::from(row)).clamp(0, *height as i64 - 1) as usize;
            return f64::from(heights[z * width + x]).round() as i32;
        }
        let (x_ratio, z_ratio) = self.get_data_coordinates(coord, data);
        self.interpolate_height(x_ratio, z_ratio, data)
    }

    /// Returns the appropriate Y level for water placement.
    /// On steep terrain, snaps to the local minimum within a small radius to
    /// correct spatial misalignment between water classification (OSM/ESA) and
    /// the elevation DEM. The snap is skipped across a real cliff/falls (a drop
    /// larger than the snap radius), where the cell keeps its own level so the
    /// waterfront isn't terraced into a step.
    pub fn water_level(&self, coord: XZPoint) -> i32 {
        let center = self.level(coord);
        if !self.elevation_enabled || self.immutable_master {
            return center;
        }
        // Check if terrain is steep here; if flat, no snapping needed
        let slope = self.slope(coord);
        if slope <= 2 {
            return center;
        }
        // On steep terrain, snap to the local minimum within SNAP_RADIUS to
        // correct small DEM-vs-water misalignment.
        const SNAP_RADIUS: i32 = 3;
        let mut min_y = center;
        for r in 1..=SNAP_RADIUS {
            for &(dx, dz) in &[
                (-r, 0),
                (r, 0),
                (0, -r),
                (0, r),
                (-r, -r),
                (-r, r),
                (r, -r),
                (r, r),
            ] {
                let neighbor = self.level(XZPoint::new(coord.x + dx, coord.z + dz));
                min_y = min_y.min(neighbor);
            }
        }
        // A drop larger than the snap radius is a real cliff/falls, not
        // misalignment; snapping across it terraces the waterfront into a step.
        // saturating_sub guards against overflow on pathological elevations.
        if center.saturating_sub(min_y) > SNAP_RADIUS {
            return center;
        }
        min_y
    }

    #[allow(unused)]
    #[inline(always)]
    pub fn min_level<I: Iterator<Item = XZPoint>>(&self, coords: I) -> Option<i32> {
        if !self.elevation_enabled {
            return Some(self.ground_level);
        }
        coords.map(|c: XZPoint| self.level(c)).min()
    }

    #[allow(unused)]
    #[inline(always)]
    pub fn max_level<I: Iterator<Item = XZPoint>>(&self, coords: I) -> Option<i32> {
        if !self.elevation_enabled {
            return Some(self.ground_level);
        }
        coords.map(|c: XZPoint| self.level(c)).max()
    }

    /// Converts game coordinates to elevation data coordinates (0.0 to 1.0 ratio)
    #[inline(always)]
    fn get_data_coordinates(&self, coord: XZPoint, data: &ElevationData) -> (f64, f64) {
        let x_ratio: f64 = coord.x as f64 / (data.world_width - 1).max(1) as f64;
        let z_ratio: f64 = coord.z as f64 / (data.world_height - 1).max(1) as f64;
        (x_ratio.clamp(0.0, 1.0), z_ratio.clamp(0.0, 1.0))
    }

    /// Bilinearly interpolates height value from the elevation grid
    #[inline(always)]
    fn interpolate_height(&self, x_ratio: f64, z_ratio: f64, data: &ElevationData) -> i32 {
        let fx = x_ratio * (data.width - 1) as f64;
        let fz = z_ratio * (data.height - 1) as f64;
        let x0 = fx.floor() as usize;
        let z0 = fz.floor() as usize;
        let x1 = (x0 + 1).min(data.width - 1);
        let z1 = (z0 + 1).min(data.height - 1);
        let dx = fx - x0 as f64;
        let dz = fz - z0 as f64;
        // Widen f32 storage to f64 for the bilinear arithmetic. The real
        // property we rely on: across the Minecraft Y range (roughly −64 up
        // through a few thousand even with --disable-height-limit), f32's
        // mantissa gives ~10⁻⁷ precision per stored cell, which is far
        // smaller than the 0.5-block half-width used by `round()` below.
        // So for any value that isn't pathologically close to a half-integer
        // boundary, the final `result.round() as i32` matches the f64 path.
        let v00 = data.heights[z0][x0] as f64;
        let v10 = data.heights[z0][x1] as f64;
        let v01 = data.heights[z1][x0] as f64;
        let v11 = data.heights[z1][x1] as f64;
        let lerp_top = v00 + (v10 - v00) * dx;
        let lerp_bot = v01 + (v11 - v01) * dx;
        let result = lerp_top + (lerp_bot - lerp_top) * dz;
        result.round() as i32
    }

    /// Replace the elevation grid with new rotated/transformed data.
    /// Used by the rotation operator to update elevation after rotating.
    pub fn set_elevation_data(
        &mut self,
        heights: Vec<Vec<f64>>,
        grid_width: usize,
        grid_height: usize,
        world_width: usize,
        world_height: usize,
    ) {
        if let Some(ref mut data) = self.elevation_data {
            // Rotation operators build a fresh f64 work grid; downcast here to
            // match `ElevationData::heights`'s f32 storage layout.
            data.heights = heights
                .into_iter()
                .map(|row| row.into_iter().map(|v| v as f32).collect())
                .collect();
            data.width = grid_width;
            data.height = grid_height;
            data.world_width = world_width;
            data.world_height = world_height;
        }
    }

    /// Replace the land-cover grids with new rotated/transformed data.
    /// Used by the rotation operator to keep land cover aligned with elevation.
    pub fn set_land_cover_data(
        &mut self,
        grid: Vec<Vec<u8>>,
        water_distance: Vec<Vec<u8>>,
        width: usize,
        height: usize,
    ) {
        if let Some(ref mut lc) = self.land_cover {
            lc.grid = grid;
            lc.water_distance = water_distance;
            lc.width = width;
            lc.height = height;
            // The water-blend mask was derived from the pre-rotation grid —
            // refresh it from the rotated grid so the shoreline softening
            // stays aligned with the new classification.
            lc.invalidate_water_blend_grid();
        }
    }

    /// Replace the canopy grid after a rotation resamples it.
    pub fn set_canopy_data(&mut self, grid: Vec<u8>, width: usize, height: usize) {
        if self.canopy.is_some() {
            self.canopy = Some(CanopyData::from_grid(grid, width, height));
        }
    }

    /// Computes the lazy water-blend mask now; all grid mutations must be done.
    pub fn warm_water_blend(&self) {
        if let Some(ref lc) = self.land_cover {
            let _ = lc.water_blend_grid();
        }
    }

    /// Update the stored world size after a rotation resizes the bbox, so flat-mode land-cover lookups stay aligned.
    pub fn set_world_dims(&mut self, world_width: usize, world_height: usize) {
        self.world_width = world_width;
        self.world_height = world_height;
    }

    /// Store rotation parameters so we can mask out-of-bounds blocks later.
    pub fn set_rotation_mask(&mut self, mask: RotationMask) {
        self.rotation_mask = Some(mask);
    }

    /// Returns `true` if the coordinate is inside the rotated original bbox.
    /// When no rotation was applied, always returns `true`.
    #[inline(always)]
    pub fn is_in_rotated_bounds(&self, x: i32, z: i32) -> bool {
        let mask = match self.rotation_mask {
            Some(ref m) => m,
            None => return true,
        };
        // Inverse-rotate (x, z) back to original space
        let dx = x as f64 - mask.cx;
        let dz = z as f64 - mask.cz;
        let orig_x = dx * mask.cos + dz * mask.neg_sin + mask.cx;
        let orig_z = -dx * mask.neg_sin + dz * mask.cos + mask.cz;
        // Allow a tiny tolerance so points that land infinitesimally outside the
        // integer bbox due to floating-point rounding are still considered inside.
        const EPSILON: f64 = 1.0e-9;
        orig_x >= mask.orig_min_x as f64 - EPSILON
            && orig_x <= mask.orig_max_x as f64 + EPSILON
            && orig_z >= mask.orig_min_z as f64 - EPSILON
            && orig_z <= mask.orig_max_z as f64 + EPSILON
    }

    pub fn save_land_cover_debug_image(&self, filename: &str) {
        let Some(ref lc) = self.land_cover else {
            return;
        };
        if lc.height == 0 || lc.width == 0 {
            return;
        }
        let mut img: image::ImageBuffer<Rgb<u8>, Vec<u8>> =
            RgbImage::new(lc.width as u32, lc.height as u32);
        for (y, row) in lc.grid.iter().enumerate() {
            for (x, &class) in row.iter().enumerate() {
                let color = match class {
                    land_cover::LC_TREE_COVER => Rgb([0x00, 0x6e, 0x00]),
                    land_cover::LC_SHRUBLAND => Rgb([0xff, 0xbb, 0x22]),
                    land_cover::LC_GRASSLAND => Rgb([0xff, 0xff, 0x4c]),
                    land_cover::LC_CROPLAND => Rgb([0xf0, 0x96, 0xff]),
                    land_cover::LC_BUILT_UP => Rgb([0xfa, 0x00, 0x00]),
                    land_cover::LC_BARE => Rgb([0xb4, 0xb4, 0xb4]),
                    land_cover::LC_SNOW_ICE => Rgb([0xf0, 0xf0, 0xf0]),
                    land_cover::LC_WATER => Rgb([0x00, 0x64, 0xc8]),
                    land_cover::LC_WETLAND => Rgb([0x00, 0x96, 0xa0]),
                    land_cover::LC_MANGROVES => Rgb([0x00, 0xcf, 0x75]),
                    land_cover::LC_MOSS => Rgb([0xfa, 0xe6, 0xa0]),
                    _ => Rgb([0x00, 0x00, 0x00]),
                };
                img.put_pixel(x as u32, y as u32, color);
            }
        }
        let filename: String = if !filename.ends_with(".png") {
            format!("{filename}.png")
        } else {
            filename.to_string()
        };
        if let Err(e) = img.save(&filename) {
            eprintln!("Failed to save land cover debug image: {e}");
        }
    }

    /// Grey ramp of canopy heights, black where nothing was measured.
    pub fn save_canopy_debug_image(&self, filename: &str) {
        let Some(ref ch) = self.canopy else {
            return;
        };
        if ch.height == 0 || ch.width == 0 {
            return;
        }
        let mut img: image::ImageBuffer<Rgb<u8>, Vec<u8>> =
            RgbImage::new(ch.width as u32, ch.height as u32);
        for z in 0..ch.height {
            for x in 0..ch.width {
                let color = match ch.at(x, z) {
                    canopy::CANOPY_NODATA => Rgb([0x00, 0x00, 0x00]),
                    h if h < canopy::CANOPY_MIN_M => Rgb([0x30, 0x30, 0x30]),
                    // 3 m to 40 m over the green ramp, saturating at the top.
                    h => {
                        let t = (f32::from(h) / 40.0).min(1.0);
                        Rgb([(64.0 * (1.0 - t)) as u8, (64.0 + 191.0 * t) as u8, 48])
                    }
                };
                img.put_pixel(x as u32, z as u32, color);
            }
        }
        let filename: String = if !filename.ends_with(".png") {
            format!("{filename}.png")
        } else {
            filename.to_string()
        };
        if let Err(e) = img.save(&filename) {
            eprintln!("Failed to save canopy debug image: {e}");
        }
    }

    fn save_debug_image(&self, filename: &str) {
        let heights = &self
            .elevation_data
            .as_ref()
            .expect("Elevation data not available")
            .heights;
        if heights.is_empty() || heights[0].is_empty() {
            return;
        }

        let height: usize = heights.len();
        let width: usize = heights[0].len();
        let mut img: image::ImageBuffer<Rgb<u8>, Vec<u8>> =
            RgbImage::new(width as u32, height as u32);

        let mut min_height: f32 = f32::MAX;
        let mut max_height: f32 = f32::MIN;

        for row in heights {
            for &h in row {
                if h.is_finite() {
                    min_height = min_height.min(h);
                    max_height = max_height.max(h);
                }
            }
        }

        let range = max_height - min_height;
        for (y, row) in heights.iter().enumerate() {
            for (x, &h) in row.iter().enumerate() {
                let normalized: u8 = if range > 0.0 {
                    (((h - min_height) / range) * 255.0) as u8
                } else {
                    128
                };
                img.put_pixel(
                    x as u32,
                    y as u32,
                    Rgb([normalized, normalized, normalized]),
                );
            }
        }

        // Ensure filename has .png extension
        let filename: String = if !filename.ends_with(".png") {
            format!("{filename}.png")
        } else {
            filename.to_string()
        };

        if let Err(e) = img.save(&filename) {
            eprintln!("Failed to save debug image: {e}");
        }
    }
}

pub(crate) fn generate_ground_data_frozen(
    args: &Args,
    bbox: LLBBox,
    sources: &crate::tiler_contract::AdmittedSources,
) -> Result<Ground, String> {
    if !args.terrain() {
        return Err("Frozen master requires terrain".into());
    }
    let ground = Ground::new_enabled_with_sources(
        &bbox,
        args.scale,
        args.ground_level,
        min_ground_level_for(args),
        args.disable_height_limit,
        extended_max_y_for(args),
        args.aws_only_elevation,
        args.benchmark,
        args.canopy_height,
        Some(sources),
    )?;
    crate::world_editor::set_base_chunk_y(ground.base_level());
    crate::world_editor::set_terrain_floor_y(ground.base_level());
    Ok(ground)
}

pub fn generate_ground_data(args: &Args, bbox: LLBBox) -> Ground {
    if args.terrain() {
        println!("{} Fetching elevation...", "[3/7]".bold());
        let ground = Ground::new_enabled(
            &bbox,
            args.scale,
            args.ground_level,
            min_ground_level_for(args),
            args.disable_height_limit,
            extended_max_y_for(args),
            args.aws_only_elevation,
            args.benchmark,
            args.canopy_height,
        );
        // The scaler may have sunk the base to reach the extended floor. The bedrock plane and
        // the out-of-bbox filler chunks both key off that base, so pin them to it now.
        crate::world_editor::set_base_chunk_y(ground.base_level());
        crate::world_editor::set_terrain_floor_y(ground.base_level());
        if args.debug {
            ground.save_debug_image("elevation_debug");
            ground.save_land_cover_debug_image("landcover_debug");
            ground.save_canopy_debug_image("canopy_debug");
        }
        return ground;
    }
    println!("{} Fetching land cover...", "[3/7]".bold());
    let ground =
        Ground::new_flat_with_land_cover(&bbox, args.scale, args.ground_level, args.canopy_height);
    crate::world_editor::set_base_chunk_y(ground.base_level());
    crate::world_editor::set_terrain_floor_y(ground.base_level());
    ground
}

/// Per-format build-height cap when the user opts into extended build height:
/// 2031 for the Java datapack, 512 for the Bedrock behavior pack.
pub(crate) fn extended_max_y_for(args: &Args) -> i32 {
    if args.bedrock {
        512
    } else {
        2031
    }
}

/// World floor. The bundled Java datapack already declares the full range the engine allows
/// (dimension_type min_y=-2032, height=4064), so the only thing keeping Arnis at -64 was the
/// old constant. Java only: the Bedrock behavior pack declares -512, but the LevelDB subchunk
/// writer is unverified below -64, and Luanti has no such pack at all.
/// Dimension ceiling actually declared to the engine: the tall datapack's 2031, or vanilla's
/// 319. Gated identically to `extended_min_y_for` — chunk serialization sizes heightmaps from
/// the span between the two, so the pair must always describe the same dimension.
pub(crate) fn world_top_y_for(args: &Args) -> i32 {
    if args.disable_height_limit && !args.bedrock && !args.luanti {
        2031
    } else {
        crate::world_editor::DEFAULT_MAX_Y
    }
}

pub(crate) fn extended_min_y_for(args: &Args) -> i32 {
    if args.disable_height_limit && !args.bedrock && !args.luanti {
        -2032
    } else {
        crate::world_editor::DEFAULT_MIN_Y
    }
}

/// Lowest terrain base the elevation scaler may sink to, leaving room for the bedrock layer
/// beneath it (mirroring the vanilla -64 floor / -62 base relationship). With a vanilla floor
/// this returns the requested ground level, which disables the sink entirely — an explicit
/// --ground-level must not be silently overridden.
pub(crate) fn min_ground_level_for(args: &Args) -> i32 {
    let floor = extended_min_y_for(args);
    if floor >= crate::world_editor::DEFAULT_MIN_Y {
        args.ground_level
    } else {
        floor + 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinate_system::cartesian::XZPoint;
    use crate::elevation_data::ElevationData;

    fn ground_with(heights: Vec<Vec<f32>>) -> Ground {
        let h = heights.len();
        let w = heights[0].len();
        Ground {
            elevation_enabled: true,
            ground_level: 0,
            elevation_data: Some(ElevationData {
                heights,
                width: w,
                height: h,
                world_width: w,
                world_height: h,
                min_height_m: 0.0,
                blocks_per_meter: 1.0,
                ground_level: 0,
            }),
            land_cover: None,
            canopy: None,
            world_width: w,
            world_height: h,
            rotation_mask: None,
            immutable_master: false,
            master_offset: None,
            master_elevation: None,
            export_context: None,
            coastal_protection: None,
            snow_threshold_y: i32::MAX,
            climate: crate::climate::Climate::Temperate,
        }
    }

    fn export_fixture() -> (Ground, Args, LLBBox) {
        use clap::Parser;
        let mut ground = ground_with(vec![vec![72.0, 74.0], vec![76.0, 78.0]]);
        ground.ground_level = 70;
        let ed = ground.elevation_data.as_mut().unwrap();
        ed.ground_level = 70;
        ed.min_height_m = -3.0;
        ed.blocks_per_meter = 2.0;
        ground.export_context = Some(ExportContext {
            water_floor: 68,
            sink_floor: 65,
            aws_only: true,
        });
        ground.snow_threshold_y = 234;
        ground.climate = crate::climate::Climate::Boreal;
        ground.land_cover = Some(LandCoverData {
            grid: vec![vec![land_cover::LC_WATER, 10], vec![10, 10]],
            water_distance: vec![vec![1, 0], vec![0, 0]],
            water_blend_cache: once_cell::sync::OnceCell::new(),
            width: 2,
            height: 2,
            cells_per_meter: 0.5,
        });
        let mut args = Args::parse_from(["arnis"]);
        args.aws_only_elevation = true;
        (ground, args, LLBBox::new(40.0, -74.0, 41.0, -73.0).unwrap())
    }

    #[test]
    fn coastal_finalization_is_required_and_nonzero_overlaps_preserve_all_bands() {
        let rectangle = |east| serde_json::json!({"type":"MultiPolygon","coordinates":[[[[0.,0.],[east,0.],[east,3.],[0.,3.],[0.,0.]]]]});
        let bytes = serde_json::to_vec(&serde_json::json!({"schema_version":1,"policy":"master-coastal-water-v1","bbox":[0.,0.,3.,3.],"default_classification":"inland","sources":[{"kind":"osm","key":"master-osm"}],"coastal_domains":[{"id":"harbor","domain":rectangle(3.),"water":rectangle(1.),"inland_exclusions":{"type":"MultiPolygon","coordinates":[]}}]})).unwrap();
        let policy = crate::coastal::CoastalPolicy::parse(
            &bytes,
            &[0., 0., 3., 3.],
            &[("osm", "master-osm")],
        )
        .unwrap();
        let mut meters = vec![vec![1., 1., 25., 30.]; 4];
        let mut mask = vec![vec![80, 80, 80, 50]; 4];
        let snapshot = policy.capture(&meters, &mask, &[0., 0., 3., 3.]).unwrap();
        snapshot.restore(&mut meters, &mut mask);
        let scaled: Vec<Vec<f32>> = meters
            .iter()
            .map(|r| r.iter().map(|h| (h + 10.) as f32).collect())
            .collect();
        let protection = snapshot.into_protection(&scaled);
        let (template, args, _) = export_fixture();
        let mut ground = ground_with(scaled);
        ground.ground_level = 10;
        ground.elevation_data.as_mut().unwrap().ground_level = 10;
        ground.export_context = template.export_context;
        ground.land_cover = Some(LandCoverData {
            grid: mask,
            width: 4,
            height: 4,
            water_distance: vec![vec![0; 4]; 4],
            water_blend_cache: once_cell::sync::OnceCell::new(),
            cells_per_meter: 1.,
        });
        ground.coastal_protection = Some(protection);
        let bbox = LLBBox::new(0., 0., 3., 3.).unwrap();
        assert!(
            ground
                .to_master_grid(&bbox, &args, &"a".repeat(64), &"b".repeat(64))
                .is_err(),
            "unfinalized coastal export must fail"
        );
        ground.elevation_data.as_mut().unwrap().heights = vec![vec![999.; 4]; 4];
        ground.land_cover.as_mut().unwrap().grid = vec![vec![80; 4]; 4];
        ground.finalize_coastal_master();
        assert_eq!(
            ground.elevation_data.as_ref().unwrap().heights[1],
            vec![10., 10., 35., 40.]
        );
        assert_eq!(
            ground.land_cover.as_ref().unwrap().grid[1],
            vec![80, 80, 0, 50]
        );
        let grid = ground
            .to_master_grid(&bbox, &args, &"a".repeat(64), &"b".repeat(64))
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("coastal.grid");
        crate::elevation::master_grid::save(&path, &grid).unwrap();
        let a = crate::elevation::master_grid::load_slice(&path, 0, 1, 3, 3).unwrap();
        let b = crate::elevation::master_grid::load_slice(&path, 1, 0, 3, 3).unwrap();
        for row in 1..3 {
            for col in 1..3 {
                let ai = (row - 1) * 3 + col;
                let bi = row * 3 + col - 1;
                assert_eq!(a.elevation[ai], b.elevation[bi]);
                assert_eq!(a.land_cover[ai], b.land_cover[bi]);
                assert_eq!(a.water_distance[ai], b.water_distance[bi]);
                assert_eq!(a.water_blend[ai], b.water_blend[bi]);
            }
        }
        let before = a.elevation.clone();
        let mut tile = Ground::from_master_slice(a).unwrap();
        tile.finalize_coastal_master();
        assert_eq!(
            tile.elevation_data
                .unwrap()
                .heights
                .into_iter()
                .flatten()
                .collect::<Vec<_>>(),
            before
        );
    }

    #[test]
    fn master_export_roundtrip_preserves_processed_snapshot() {
        let (mut ground, args, bbox) = export_fixture();
        // Model the final bridge/OSM repair, after floors and affine were decided.
        ground.elevation_data.as_mut().unwrap().heights[0][0] = 81.0;
        let grid = ground
            .to_master_grid(&bbox, &args, &"a".repeat(64), &"b".repeat(64))
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("master.grid");
        crate::elevation::master_grid::save(&path, &grid).unwrap();
        let tile = crate::elevation::master_grid::load_slice(&path, 0, 0, 2, 2).unwrap();
        assert_eq!(tile.elevation, vec![81.0, 74.0, 76.0, 78.0]);
        assert_eq!(tile.metadata.water_floor, 68);
        assert_eq!(tile.metadata.sink_floor, 65);
        assert_eq!(tile.metadata.min_height_m, -3.0);
        assert_eq!(tile.metadata.blocks_per_meter, 2.0);
        assert_eq!(tile.metadata.effective_ground_level, 70);
        assert_eq!(tile.metadata.sea_level_y, 76.0);
        assert_eq!(tile.metadata.climate, "Boreal");
        assert_eq!(tile.metadata.snow_threshold_y, 234);
        assert_eq!(tile.metadata.climate_anchor, [40.5, -73.5]);
        assert_eq!(tile.metadata.selected_provider, "aws");
        assert_eq!(tile.metadata.provider_attempts[0].outcome, "success");
        assert_eq!(tile.land_cover, vec![land_cover::LC_WATER, 10, 10, 10]);
        assert_eq!(tile.water_distance, vec![1, 0, 0, 0]);
        assert_eq!(
            tile.water_blend,
            ground
                .land_cover
                .as_ref()
                .unwrap()
                .water_blend_grid()
                .iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn master_export_rejects_incomplete_or_unsupported_ground() {
        let (ground, args, bbox) = export_fixture();
        let export =
            |g: &Ground, a: &Args| g.to_master_grid(&bbox, a, &"a".repeat(64), &"b".repeat(64));
        assert!(export(&Ground::new_flat(62), &args).is_err());
        let mut bad = ground.clone();
        bad.export_context = None;
        assert!(export(&bad, &args).is_err());
        let mut bad = ground.clone();
        bad.land_cover = None;
        assert!(export(&bad, &args).is_err());
        let mut bad = ground.clone();
        bad.world_width = 4;
        assert!(export(&bad, &args).is_err());
        let mut bad = ground.clone();
        bad.world_width = 4097;
        bad.world_height = 4097;
        let ed = bad.elevation_data.as_mut().unwrap();
        ed.width = 4097;
        ed.height = 4097;
        ed.world_width = 4097;
        ed.world_height = 4097;
        let lc = bad.land_cover.as_mut().unwrap();
        lc.width = 4097;
        lc.height = 4097;
        assert!(export(&bad, &args)
            .unwrap_err()
            .to_string()
            .contains("profile limits"));
        let mut bad = ground.clone();
        bad.elevation_data.as_mut().unwrap().heights[0][0] = f32::NAN;
        assert!(export(&bad, &args).is_err());
        let (_, mut auto, _) = export_fixture();
        auto.aws_only_elevation = false;
        assert!(export(&ground, &auto).is_err());
        let mut bad = ground.clone();
        bad.export_context.as_mut().unwrap().aws_only = false;
        assert!(export(&bad, &args).is_err());
    }

    // An unmeasured cell hands the decision back to the land cover.
    #[test]
    fn canopy_fraction_separates_bare_from_unmeasured() {
        let nd = canopy::CANOPY_NODATA;
        let mut ground = ground_with(vec![vec![0.0; 4]; 4]);
        // Left half wooded, right half measured bare, bottom row unmeasured.
        ground.canopy = Some(CanopyData::from_grid(
            vec![
                12, 12, 0, 0, //
                12, 12, 0, 0, //
                12, 12, 0, 0, //
                nd, nd, nd, nd,
            ],
            4,
            4,
        ));
        assert_eq!(ground.canopy_fraction(XZPoint::new(0, 0), 2), Some(1.0));
        assert_eq!(ground.canopy_fraction(XZPoint::new(2, 0), 2), Some(0.0));
        assert_eq!(
            ground.canopy_fraction(XZPoint::new(0, 3), 1),
            None,
            "unmeasured is not bare"
        );
        // A cell straddling the two averages only over what was measured.
        assert_eq!(ground.canopy_fraction(XZPoint::new(0, 2), 2), Some(1.0));
        // Without a canopy grid there is nothing to say.
        ground.canopy = None;
        assert_eq!(ground.canopy_fraction(XZPoint::new(0, 0), 2), None);
    }

    // Flat mode (no elevation) still maps land-cover lookups via the stored world dims, with edge clamping.
    #[test]
    fn flat_land_cover_maps_and_clamps() {
        use crate::land_cover::{LandCoverData, LC_WATER};
        let lc = LandCoverData {
            grid: vec![vec![LC_WATER, 10], vec![10, 10]],
            water_distance: vec![vec![1, 0], vec![0, 0]],
            water_blend_cache: once_cell::sync::OnceCell::with_value(vec![
                vec![1.0, 0.0],
                vec![0.0, 0.0],
            ]),
            width: 2,
            height: 2,
            cells_per_meter: 1.0,
        };
        // world 4x4 over a 2x2 grid: x<=1 samples column 0, x>=2 samples column 1.
        let ground = Ground::new_flat_land_cover_test(lc, 4, 4);
        assert_eq!(ground.cover_class(XZPoint::new(0, 0)), LC_WATER);
        assert_eq!(ground.cover_class(XZPoint::new(3, 0)), 10);
        assert_eq!(ground.water_distance(XZPoint::new(0, 0)), 1);
        // Out-of-range coords clamp to the last grid cell instead of panicking.
        assert_eq!(ground.cover_class(XZPoint::new(1000, 1000)), 10);
        assert_eq!(ground.water_distance(XZPoint::new(1000, 1000)), 0);
    }

    // Water snaps to the local floor over small DEM steps, but not across a real cliff.
    #[test]
    fn water_level_snaps_small_steps_not_cliffs() {
        // Flat terrain: no snap, returns the cell's own level.
        let flat = ground_with(vec![vec![5.0; 16]; 16]);
        assert_eq!(flat.water_level(XZPoint::new(8, 8)), 5);

        // 3-block step: snaps down to the nearby floor.
        let step = ground_with(
            (0..16)
                .map(|_| (0..16).map(|x| if x <= 7 { 10.0 } else { 7.0 }).collect())
                .collect(),
        );
        assert_eq!(step.water_level(XZPoint::new(7, 8)), 7);

        // Real cliff (30-block drop): keeps its own level, no terracing.
        let cliff = ground_with(
            (0..16)
                .map(|_| (0..16).map(|x| if x <= 7 { 30.0 } else { 0.0 }).collect())
                .collect(),
        );
        assert_eq!(cliff.water_level(XZPoint::new(7, 8)), 30);
    }

    #[test]
    fn snow_line_follows_latitude() {
        assert!((snow_line_meters(0.0) - 4500.0).abs() < 1.0);
        assert!((snow_line_meters(25.0) - 5700.0).abs() < 1.0);
        assert!((snow_line_meters(46.0) - 3000.0).abs() < 1.0);
        assert!(snow_line_meters(90.0).abs() < 1.0);
        // Symmetric across the equator.
        assert_eq!(snow_line_meters(-46.0), snow_line_meters(46.0));
    }

    #[test]
    fn snow_threshold_inverts_the_scale() {
        let ed = |min_m: f64, bpm: f64| ElevationData {
            heights: vec![vec![0.0; 2]; 2],
            width: 2,
            height: 2,
            world_width: 2,
            world_height: 2,
            min_height_m: min_m,
            blocks_per_meter: bpm,
            ground_level: 0,
        };
        // 46 deg snow line is 3000 m; at 0.1 block/m from min 0 m, ground 64 => Y 364.
        assert_eq!(snow_threshold_for(&ed(0.0, 0.1), 46.0, 64), 364);
        // Flat terrain: never below the line, always above it.
        assert_eq!(snow_threshold_for(&ed(100.0, 0.0), 46.0, 64), i32::MAX);
        assert_eq!(snow_threshold_for(&ed(4000.0, 0.0), 46.0, 64), i32::MIN);
    }
}

#[cfg(test)]
mod tiler_master_tests {
    use super::*;
    #[test]
    fn tiler_water_uses_master_height_without_local_snapping() {
        use crate::elevation::master_grid;
        let root = tempfile::tempdir().unwrap();
        let mut grid = master_grid::tests::fixture();
        grid.metadata.width = 24;
        grid.metadata.height = 24;
        grid.metadata.world_width = 24;
        grid.metadata.world_height = 24;
        grid.metadata.payload_bytes = 24 * 24 * 10;
        grid.elevation = (0..24 * 24)
            .map(|i| if i % 24 < 12 { 70.0 } else { 67.0 })
            .collect();
        grid.land_cover = vec![80; 24 * 24];
        grid.water_distance = vec![5; 24 * 24];
        grid.water_blend = vec![1.0; 24 * 24];
        let path = root.path().join("master");
        master_grid::save(&path, &grid).unwrap();
        let slice = master_grid::load_slice(&path, 0, 0, 24, 24).unwrap();
        let ground = Ground::from_master_slice(slice).unwrap();
        assert_eq!(
            ground.water_level(XZPoint::new(11, 12)),
            70,
            "tile-local snapping moved a persisted master surface"
        );
        let slice = master_grid::load_slice(&path, 3, 4, 16, 16).unwrap();
        let ground = Ground::from_master_slice(slice).unwrap();
        assert_eq!(ground.master_offset(), Some((3, 4)));
        assert_eq!(ground.water_level(XZPoint::new(8, 8)), 70);
        let bounds = XZBBox::rect_from_min_max(0, 0, 15, 15).unwrap();
        let ll =
            crate::coordinate_system::geographic::LLBBox::from_str("24,45,24.01,45.01").unwrap();
        let mut editor =
            crate::world_editor::WorldEditor::new("/dev/null/unused".into(), &bounds, ll);
        editor.set_ground(std::sync::Arc::new(ground));
        assert_eq!(editor.master_coordinates(8, 8), (11, 12));
        editor.set_external_tile(true);
        assert_eq!(
            editor.climate(),
            crate::climate::Climate::Temperate,
            "editor recomputed climate from tile bbox"
        );
    }

    use crate::elevation::master_grid::{load_slice, save, tests::fixture};

    #[test]
    fn raw_terrain_reads_full_admitted_master_outside_slice() {
        use crate::world_editor::WorldEditor;
        use std::sync::Arc;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("master");
        let grid = fixture();
        save(&path, &grid).unwrap();
        let bounds = XZBBox::rect_from_min_max(0, 0, 1, 1).unwrap();
        let bbox = LLBBox::from_str("40,-74,41,-73").unwrap();
        let mut whole = WorldEditor::new("/dev/null/unused".into(), &bounds, bbox);
        whole.set_ground(Arc::new(
            Ground::from_master_slice(load_slice(&path, 0, 0, 4, 3).unwrap()).unwrap(),
        ));
        let mut tile = WorldEditor::new("/dev/null/unused".into(), &bounds, bbox);
        tile.set_ground(Arc::new(
            Ground::from_master_slice(load_slice(&path, 1, 1, 2, 2).unwrap()).unwrap(),
        ));
        // Includes negative tile coords, far endpoints and master-edge clamping.
        for x in -2..=5 {
            for z in -2..=4 {
                assert_eq!(
                    tile.terrain_level(x - 1, z - 1),
                    whole.terrain_level(x, z),
                    "master ({x},{z})"
                );
            }
        }
        // Local rendering extent and writes remain bounded by the supplied window.
        assert_eq!(tile.get_max_coords(), (1, 1));
    }

    fn differently_sized_slices() -> Vec<Ground> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("master");
        let mut grid = fixture();
        grid.metadata.width = 24;
        grid.metadata.height = 24;
        grid.metadata.world_width = 24;
        grid.metadata.world_height = 24;
        grid.metadata.payload_bytes = 24 * 24 * 10;
        grid.elevation = vec![60.5; 24 * 24];
        grid.land_cover = vec![10; 24 * 24];
        grid.water_distance = vec![0; 24 * 24];
        grid.water_blend = vec![0.0; 24 * 24];
        let shared = 15 * 24 + 15;
        grid.elevation[shared] = 70.5;
        grid.land_cover[shared] = land_cover::LC_WATER;
        grid.water_distance[shared] = 7;
        grid.water_blend[shared] = 0.5;
        save(&path, &grid).unwrap();
        [23, 24]
            .into_iter()
            .map(|width| {
                Ground::from_master_slice(load_slice(&path, 0, 0, width, width).unwrap()).unwrap()
            })
            .collect()
    }

    #[test]
    fn master_integer_heights_do_not_depend_on_slice_dimensions() {
        for ground in differently_sized_slices() {
            assert_eq!(ground.level(XZPoint::new(15, 15)), 71);
            assert_eq!(ground.level(XZPoint::new(-1, -1)), 61);
            assert_eq!(ground.level(XZPoint::new(i32::MAX, i32::MAX)), 61);
        }
    }

    #[test]
    fn master_integer_water_bands_are_exact_across_slice_dimensions() {
        for ground in differently_sized_slices() {
            let shared = XZPoint::new(15, 15);
            assert_eq!(ground.water_blend(shared), 0.5);
            assert_eq!(ground.cover_class(shared), land_cover::LC_WATER);
            assert_eq!(ground.water_distance(shared), 7);
            for edge in [XZPoint::new(-1, -1), XZPoint::new(i32::MAX, i32::MAX)] {
                assert_eq!(ground.water_blend(edge), 0.0);
                assert_eq!(ground.cover_class(edge), 10);
                assert_eq!(ground.water_distance(edge), 0);
            }
        }
    }

    #[test]
    fn master_slice_preserves_shared_height_masks_and_climate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("master");
        let grid = fixture();
        save(&path, &grid).unwrap();
        let tile = load_slice(&path, 1, 1, 2, 2).unwrap();
        let heights = tile.elevation.clone();
        let blend = tile.water_blend.clone();
        let snow = tile.metadata.snow_threshold_y;
        let base = tile.metadata.effective_ground_level;
        let mut ground = Ground::from_master_slice(tile).unwrap();
        assert_eq!(ground.base_level(), base);
        assert_eq!(ground.snow_threshold_y(), snow);
        assert_eq!(ground.level(XZPoint::new(0, 0)), heights[0].round() as i32);
        assert_eq!(ground.level(XZPoint::new(1, 1)), heights[3].round() as i32);
        assert_eq!(
            ground.land_cover.as_ref().unwrap().water_blend_grid()[0][0],
            blend[0]
        );
        let before = ground.elevation_data.as_ref().unwrap().heights.clone();
        let bbox = XZBBox::rect_from_min_max(0, 0, 1, 1).unwrap();
        ground.apply_osm_water_override(&[], &bbox);
        ground.apply_osm_land_override(&[], &bbox, 1.0);
        ground.apply_bridge_land_cover_repair(&[], &bbox, 1.0);
        assert_eq!(ground.elevation_data.as_ref().unwrap().heights, before);
        assert!(ground.immutable_master);
        assert_eq!(ground.master_offset(), Some((1, 1)));
    }
}

impl Ground {
    pub(crate) fn finalize_coastal_master(&mut self) {
        if self.immutable_master {
            return;
        }
        if let (Some(protection), Some(ed), Some(lc)) = (
            self.coastal_protection.take(),
            self.elevation_data.as_mut(),
            self.land_cover.as_mut(),
        ) {
            protection.restore(&mut ed.heights, lc);
            let _ = lc.water_blend_grid();
        }
    }

    /// Snapshot the complete master after the caller applies OSM and bridge repairs.
    /// Floors and affine parameters are retained from the original elevation pass.
    pub(crate) fn to_master_grid(
        &self,
        bbox: &LLBBox,
        args: &Args,
        source_hash: &str,
        profile_hash: &str,
    ) -> std::io::Result<crate::elevation::master_grid::Grid> {
        use crate::elevation::master_grid::{Grid, Metadata, ProviderAttempt, MAX_CELLS};
        let invalid = |message: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, message);
        if !self.elevation_enabled
            || self.immutable_master
            || self.rotation_mask.is_some()
            || self.coastal_protection.is_some()
        {
            return Err(invalid(
                "export requires a complete unrotated elevation master",
            ));
        }
        let ed = self
            .elevation_data
            .as_ref()
            .ok_or_else(|| invalid("missing elevation data"))?;
        let lc = self
            .land_cover
            .as_ref()
            .ok_or_else(|| invalid("missing processed land cover"))?;
        let context = self
            .export_context
            .as_ref()
            .ok_or_else(|| invalid("missing global floor/provider context"))?;
        if !args.aws_only_elevation || !context.aws_only {
            return Err(invalid(
                "master export supports only actual AWS-only terrain",
            ));
        }
        let cells = ed
            .width
            .checked_mul(ed.height)
            .ok_or_else(|| invalid("master dimensions overflow"))?;
        if ed.width < 2
            || ed.height < 2
            || ed.width > 16_384
            || ed.height > 16_384
            || cells as u64 > MAX_CELLS
            || ed.width != ed.world_width
            || ed.height != ed.world_height
            || ed.width != self.world_width
            || ed.height != self.world_height
            || ed.width != lc.width
            || ed.height != lc.height
        {
            return Err(invalid(
                "master requires aligned full-resolution bands within profile limits",
            ));
        }
        if ed.heights.len() != ed.height
            || ed.heights.iter().any(|r| r.len() != ed.width)
            || lc.grid.len() != ed.height
            || lc.grid.iter().any(|r| r.len() != ed.width)
            || lc.water_distance.len() != ed.height
            || lc.water_distance.iter().any(|r| r.len() != ed.width)
        {
            return Err(invalid("master band dimensions mismatch"));
        }
        if self.ground_level != ed.ground_level
            || ed.heights.iter().flatten().any(|v| !v.is_finite())
        {
            return Err(invalid("invalid effective elevation data"));
        }
        let south = bbox.min().lat();
        let west = bbox.min().lng();
        let north = bbox.max().lat();
        let east = bbox.max().lng();
        let metadata = Metadata {
            format_version: 2,
            contract: "arnis-tiler/v3.1/1".into(),
            bbox: [south, west, north, east],
            width: ed.width as u32,
            height: ed.height as u32,
            world_width: ed.world_width as u32,
            world_height: ed.world_height as u32,
            scale: args.scale,
            projection: "local".into(),
            orientation: "northwest-row-major".into(),
            min_height_m: ed.min_height_m,
            blocks_per_meter: ed.blocks_per_meter,
            effective_ground_level: self.ground_level,
            requested_ground_level: args.ground_level,
            min_ground_level: min_ground_level_for(args),
            extended_max_y: extended_max_y_for(args),
            water_floor: context.water_floor,
            sink_floor: context.sink_floor,
            sea_level_y: self.ground_level as f64 - ed.min_height_m * ed.blocks_per_meter,
            source_mode: "aws-only".into(),
            selected_provider: "aws".into(),
            provider_attempts: vec![ProviderAttempt {
                name: "aws".into(),
                outcome: "success".into(),
            }],
            source_manifest_sha256: source_hash.into(),
            profile_sha256: profile_hash.into(),
            postprocess: "master-once-v1".into(),
            height_units: "minecraft_y".into(),
            payload_bytes: cells as u64 * 10,
            land_cover_cells_per_meter: lc.cells_per_meter,
            climate: format!("{:?}", self.climate),
            snow_threshold_y: self.snow_threshold_y,
            climate_anchor: [(south + north) / 2.0, (west + east) / 2.0],
        };
        metadata.validate()?;
        // Materialize lazily cached blending only after all master mask repairs.
        let blend = lc.water_blend_grid();
        if blend.len() != ed.height
            || blend.iter().any(|r| r.len() != ed.width)
            || blend
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || lc.water_distance.iter().flatten().any(|v| *v > 15)
        {
            return Err(invalid("invalid processed water bands"));
        }
        Ok(Grid {
            metadata,
            elevation: ed.heights.iter().flatten().copied().collect(),
            land_cover: lc.grid.iter().flatten().copied().collect(),
            water_distance: lc.water_distance.iter().flatten().copied().collect(),
            water_blend: blend.iter().flatten().copied().collect(),
        })
    }

    /// Construct a tile from admitted processed bands without provider access or rescaling.
    pub(crate) fn from_master_slice(
        tile: crate::elevation::master_grid::TileGrid,
    ) -> std::io::Result<Self> {
        use crate::climate::Climate;
        let width = tile.width as usize;
        let height = tile.height as usize;
        let cells = width
            .checked_mul(height)
            .ok_or_else(|| std::io::Error::other("tile dimensions overflow"))?;
        tile.metadata.validate()?;
        if width < 2
            || height < 2
            || [
                tile.elevation.len(),
                tile.land_cover.len(),
                tile.water_distance.len(),
                tile.water_blend.len(),
            ]
            .iter()
            .any(|n| *n != cells)
        {
            return Err(std::io::Error::other("invalid admitted tile band lengths"));
        }
        if tile.master_elevation.len()
            != tile.metadata.width as usize * tile.metadata.height as usize
        {
            return Err(std::io::Error::other(
                "invalid admitted master elevation length",
            ));
        }
        let climate = match tile.metadata.climate.as_str() {
            "Temperate" => Climate::Temperate,
            "TropicalSavanna" => Climate::TropicalSavanna,
            "HotDesert" => Climate::HotDesert,
            "HotSteppe" => Climate::HotSteppe,
            "ColdDesert" => Climate::ColdDesert,
            "ColdSteppe" => Climate::ColdSteppe,
            "DryContinental" => Climate::DryContinental,
            "Boreal" => Climate::Boreal,
            "Tundra" => Climate::Tundra,
            "IceCap" => Climate::IceCap,
            _ => return Err(std::io::Error::other("invalid master climate")),
        };
        let meta = tile.metadata;
        Ok(Self {
            elevation_enabled: true,
            ground_level: meta.effective_ground_level,
            elevation_data: Some(ElevationData {
                heights: tile
                    .elevation
                    .chunks_exact(width)
                    .map(<[f32]>::to_vec)
                    .collect(),
                width,
                height,
                world_width: width,
                world_height: height,
                min_height_m: meta.min_height_m,
                blocks_per_meter: meta.blocks_per_meter,
                ground_level: meta.effective_ground_level,
            }),
            land_cover: Some(LandCoverData {
                grid: tile
                    .land_cover
                    .chunks_exact(width)
                    .map(<[u8]>::to_vec)
                    .collect(),
                water_distance: tile
                    .water_distance
                    .chunks_exact(width)
                    .map(<[u8]>::to_vec)
                    .collect(),
                water_blend_cache: once_cell::sync::OnceCell::from(
                    tile.water_blend
                        .chunks_exact(width)
                        .map(<[f32]>::to_vec)
                        .collect::<Vec<_>>(),
                ),
                width,
                height,
                cells_per_meter: meta.land_cover_cells_per_meter,
            }),
            canopy: None,
            world_width: width,
            world_height: height,
            rotation_mask: None,
            immutable_master: true,
            master_offset: Some((tile.col as i32, tile.row as i32)),
            master_elevation: Some((
                tile.master_elevation,
                meta.width as usize,
                meta.height as usize,
            )),
            export_context: None,
            coastal_protection: None,
            snow_threshold_y: meta.snow_threshold_y,
            climate,
        })
    }
}
