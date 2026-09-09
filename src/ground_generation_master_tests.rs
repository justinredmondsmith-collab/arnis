use super::*;
use crate::coordinate_system::geographic::LLBBox;
use crate::elevation::master_grid;
use clap::Parser;
use sha2::{Digest, Sha256};
use std::sync::Arc;

fn grid(sample: impl Fn(i32, i32) -> (f32, u8), climate: &str, snow: i32) -> master_grid::Grid {
    sized_grid(128, 128, sample, climate, snow)
}

fn sized_grid(
    width: u32,
    height: u32,
    sample: impl Fn(i32, i32) -> (f32, u8),
    climate: &str,
    snow: i32,
) -> master_grid::Grid {
    let mut grid = master_grid::tests::fixture();
    grid.metadata.width = width;
    grid.metadata.height = height;
    grid.metadata.world_width = width;
    grid.metadata.world_height = height;
    grid.metadata.payload_bytes = u64::from(width) * u64::from(height) * 10;
    grid.metadata.land_cover_cells_per_meter = 1.;
    grid.metadata.climate = climate.into();
    grid.metadata.snow_threshold_y = snow;
    let samples: Vec<_> = (0..height as i32)
        .flat_map(|z| (0..width as i32).map(move |x| (x, z)))
        .map(|(x, z)| sample(x, z))
        .collect();
    grid.elevation = samples.iter().map(|s| s.0).collect();
    grid.land_cover = samples.iter().map(|s| s.1).collect();
    grid.water_distance = vec![0; (width * height) as usize];
    grid.water_blend = vec![0.; (width * height) as usize];
    grid
}

fn editor<'a>(
    bounds: &'a XZBBox,
    slice: (u32, u32),
    grid: &master_grid::Grid,
) -> (WorldEditor<'a>, Ground) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("master");
    master_grid::save(&path, grid).unwrap();
    let ground = Ground::from_master_slice(
        master_grid::load_slice(
            &path,
            slice.0,
            slice.1,
            (bounds.max_x() - bounds.min_x() + 1) as u32,
            (bounds.max_z() - bounds.min_z() + 1) as u32,
        )
        .unwrap(),
    )
    .unwrap();
    let mut editor = WorldEditor::new(
        "/dev/null/unused".into(),
        bounds,
        LLBBox::from_str("40,-74,41,-73").unwrap(),
    );
    editor.set_ground(Arc::new(ground.clone()));
    editor.set_ground_origin(bounds.min_x(), bounds.min_z());
    editor.set_external_tile(true);
    editor.set_place_schematics(false);
    (editor, ground)
}

fn generate(editor: &mut WorldEditor, ground: &Ground, bounds: &XZBBox) {
    let args = Args::parse_from(["arnis", "--ground-level=0", "--no-3d", "--legacy-trees"]);
    let outlines = crate::element_processing::bridge_styles::BridgeOutlineIndex::build(&[]);
    let structures =
        crate::element_processing::bridges::BridgeStructureMap::build(&[], editor, &outlines);
    let bridges = BridgeSurfaceMap::build(&[], &structures, 1.);
    let mask = BuildingFootprintBitmap::new_empty();
    generate_ground_region(
        editor,
        ground,
        &args,
        bounds,
        &mask,
        &mask,
        &bridges,
        bounds.min_x(),
        bounds.max_x(),
        bounds.min_z(),
        bounds.max_z(),
        false,
    );
}

fn assert_translation(grid: &master_grid::Grid, local_min: i32, slice: (u32, u32)) {
    let whole_bounds = XZBBox::rect_from_min_max(0, 0, 127, 127).unwrap();
    let tile_bounds =
        XZBBox::rect_from_min_max(local_min, local_min, local_min + 79, local_min + 79).unwrap();
    let (mut whole, ground) = editor(&whole_bounds, (0, 0), grid);
    let (mut tile, tile_ground) = editor(&tile_bounds, slice, grid);
    generate(&mut whole, &ground, &whole_bounds);
    generate(&mut tile, &tile_ground, &tile_bounds);
    let mut above_ground = 0;
    let mut logs = 0;
    let mut grass = 0;
    let mut materials = std::collections::HashSet::new();
    let mut differences = Vec::new();
    for x in slice.0 as i32 + 16..slice.0 as i32 + 64 {
        for z in slice.1 as i32 + 16..slice.1 as i32 + 64 {
            for y in 65..105 {
                let a = whole.get_block_absolute(x, y, z);
                let b = tile.get_block_absolute(
                    x - slice.0 as i32 + local_min,
                    y,
                    z - slice.1 as i32 + local_min,
                );
                if y > 70 && a.is_some() {
                    above_ground += 1;
                }
                if a == Some(crate::block_definitions::OAK_LOG) {
                    logs += 1;
                }
                if a == Some(GRASS) {
                    grass += 1;
                }
                if y >= 70 {
                    if let Some(block) = a {
                        materials.insert(block);
                    }
                }
                if a != b {
                    differences.push((x, y, z, a, b));
                }
            }
        }
    }
    assert!(
        above_ground > 0 || grid.land_cover[0] != land_cover::LC_TREE_COVER,
        "empty forest fixture"
    );
    if grid.land_cover[0] == land_cover::LC_TREE_COVER {
        assert!(
            logs > 0 && grass > 0,
            "forest must contain trees and undergrowth"
        );
    }
    assert!(
        materials.len() >= 2,
        "pattern fixture must contain multiple materials"
    );
    assert!(
        differences.is_empty(),
        "{} translated differences; first {:?}",
        differences.len(),
        differences.first()
    );
}

#[test]
fn master_esa_forest_actual_generator_translates() {
    let grid = grid(
        |_, _| (70., land_cover::LC_TREE_COVER),
        "Temperate",
        i32::MAX,
    );
    assert_translation(&grid, 0, (16, 32));
    assert_translation(&grid, -16, (32, 16));
}

macro_rules! surface_test {
    ($name:ident, $cover:expr, $climate:expr, $snow:expr) => {
        #[test]
        fn $name() {
            let grid = grid(|_, _| (70., $cover), $climate, $snow);
            assert_translation(&grid, -16, (16, 32));
        }
    };
}
surface_test!(master_esa_shrubland, 20, "Temperate", i32::MAX);
surface_test!(master_esa_grassland, 30, "Temperate", i32::MAX);
surface_test!(master_esa_cropland, 40, "Temperate", i32::MAX);
surface_test!(master_esa_builtup, 50, "Temperate", i32::MAX);
surface_test!(master_esa_bare, 60, "Temperate", i32::MAX);
surface_test!(master_esa_snowice, 70, "Temperate", i32::MAX);
surface_test!(master_esa_wetland, 90, "Temperate", i32::MAX);
surface_test!(master_esa_mangrove, 95, "Temperate", i32::MAX);
surface_test!(master_esa_climate, 30, "HotSteppe", i32::MAX);
surface_test!(master_esa_snow_edge, 30, "Temperate", 70);

#[test]
fn stock_esa_ground_generator_golden() {
    let bounds = XZBBox::rect_from_min_max(0, 0, 31, 31).unwrap();
    let ground = Ground::new_flat_land_cover_test(
        land_cover::LandCoverData {
            grid: vec![vec![land_cover::LC_TREE_COVER; 32]; 32],
            water_distance: vec![vec![0; 32]; 32],
            water_blend_cache: once_cell::sync::OnceCell::with_value(vec![vec![0.; 32]; 32]),
            width: 32,
            height: 32,
            cells_per_meter: 1.,
        },
        32,
        32,
    );
    let mut editor = WorldEditor::new(
        "/dev/null/unused".into(),
        &bounds,
        LLBBox::from_str("40,-74,41,-73").unwrap(),
    );
    editor.set_ground(Arc::new(ground.clone()));
    editor.set_place_schematics(false);
    generate(&mut editor, &ground, &bounds);
    let mut digest = Sha256::new();
    for x in 0..32 {
        for z in 0..32 {
            for y in -1..95 {
                digest.update(format!(
                    "{x},{y},{z}:{:?}\n",
                    editor.get_block_absolute(x, y, z)
                ));
            }
        }
    }
    assert_eq!(
        format!("{:x}", digest.finalize()),
        "9320e9462389b6d7358deacf95c36a4b05df764d586afdc07c4174b2dc1ad708"
    );
}

macro_rules! steep_test {
    ($name:ident, $numerator:expr, $denominator:expr) => {
        #[test]
        fn $name() {
            let grid = grid(
                |x, _| {
                    (
                        (70 + x.rem_euclid(16) * $numerator / $denominator) as f32,
                        30,
                    )
                },
                "Temperate",
                i32::MAX,
            );
            assert_translation(&grid, -16, (16, 32));
        }
    };
}
steep_test!(master_esa_scree, 3, 4);
steep_test!(master_esa_rock_face, 1, 1);
steep_test!(master_esa_cliff, 2, 1);

#[test]
fn master_esa_slope_root_admission_has_bounded_context() {
    // Genuine accepted ESA seeds discovered once; no forced spawn or tree-type hook.
    let roots = [
        (1087, 683),
        (1089, 682),
        (1092, 681),
        (1093, 678),
        (1095, 678),
        (1096, 676),
        (1099, 675),
        (1102, 676),
        (1104, 676),
    ];
    for &(x, z) in &roots {
        assert_eq!(
            crate::deterministic_rng::coord_rng(x, z, 0).random_range(0..30),
            0
        );
    }
    let bounds = XZBBox::rect_from_min_max(0, 0, 127, 127).unwrap();
    let tile_bounds = XZBBox::rect_from_min_max(0, 0, 63, 127).unwrap();
    let make_grid = |heights: &[i32]| {
        let mut g = grid(|_, _| (70., 0), "Temperate", i32::MAX);
        g.metadata.width = 1152;
        g.metadata.height = 768;
        g.metadata.world_width = 1152;
        g.metadata.world_height = 768;
        g.metadata.payload_bytes = 1152 * 768 * 10;
        g.elevation = vec![70.; 1152 * 768];
        g.land_cover = vec![0; 1152 * 768];
        g.water_distance = vec![0; 1152 * 768];
        g.water_blend = vec![0.; 1152 * 768];
        for (i, &(x, z)) in roots.iter().enumerate() {
            g.land_cover[z as usize * 1152 + x as usize] = 10;
            g.elevation[z as usize * 1152 + x as usize] =
                heights.get(i).copied().unwrap_or(70) as f32;
        }
        g
    };
    let mut heights = vec![70];
    for pair in roots.windows(2) {
        let grid = make_grid(&heights);
        let (mut isolated, _) = editor(&bounds, (1024, 640), &grid);
        let ((x, z), (nx, nz)) = (pair[0], pair[1]);
        tree::Tree::create(&mut isolated, (x - 1024, 1, z - 640), None, None);
        let leaf_y = (heights.last().unwrap() + 1..250)
            .find(|&y| isolated.get_block_absolute(nx - 1024, y, nz - 640) == Some(OAK_LEAVES))
            .unwrap_or_else(|| {
                panic!(
                    "genuine candidate tree {x},{z} at {:?} has no leaf at successor {nx},{nz}",
                    heights.last()
                )
            });
        heights.push(leaf_y - 1);
    }
    let grid = make_grid(&heights);
    let (mut whole, ground) = editor(&bounds, (1024, 640), &grid);
    let (mut tile, tile_ground) = editor(&tile_bounds, (1088, 640), &grid);
    for &(x, z) in &roots {
        assert_eq!(
            ground.slope(XZPoint::new(x - 1024, z - 640)),
            0,
            "root slope must permit ESA vegetation"
        );
    }
    generate(&mut whole, &ground, &bounds);
    generate(&mut tile, &tile_ground, &tile_bounds);
    let admissions = |editor: &WorldEditor, offset: i32| {
        roots
            .iter()
            .enumerate()
            .filter(|&(_, &(x, _))| x >= offset)
            .filter_map(|(i, &(x, z))| {
                (editor.get_block_absolute(x - offset, heights[i] + 1, z - 640)
                    == Some(crate::block_definitions::OAK_LOG))
                .then_some(x)
            })
            .collect::<Vec<_>>()
    };
    let a = admissions(&whole, 1024);
    let b = admissions(&tile, 1088);
    assert_eq!(
        a.len(),
        roots.len(),
        "all unblocked genuine candidates must remain eligible"
    );
    assert_eq!(
        b.len(),
        roots.len() - 1,
        "the slice omits exactly one candidate"
    );
    for (i, &(x, z)) in roots.iter().enumerate().filter(|(_, &(x, _))| x >= 1104) {
        assert_eq!(
            whole.get_block_absolute(x - 1024, heights[i] + 1, z - 640),
            tile.get_block_absolute(x - 1088, heights[i] + 1, z - 640),
            "omitted root beyond halo16 changed actual ESA owned-root eligibility"
        );
    }
    for x in 1104..1112 {
        for z in 666..691 {
            for y in 70..130 {
                assert_eq!(
                    whole.get_block_absolute(x - 1024, y, z - 640),
                    tile.get_block_absolute(x - 1088, y, z - 640),
                    "owned canopy differs at {x},{y},{z}"
                );
            }
        }
    }
    // A predecessor's leaf can win set-if-absent against snow. Snow's
    // deterministic predicate must still reject the later ESA root.
    let edge = |(x, z)| (value_noise_01(x, z, 8) - 0.5) * 6.;
    let last = roots.len() - 1;
    let threshold = (70..110)
        .find(|&t| {
            (heights[last - 1] as f64) < t as f64 + edge(roots[last - 1])
                && heights[last] as f64 >= t as f64 + edge(roots[last])
        })
        .expect("snow edge separates last two roots");
    let mut snowy_grid = grid.clone();
    snowy_grid.metadata.snow_threshold_y = threshold;
    let (mut snowy, snow_ground) = editor(&tile_bounds, (1088, 640), &snowy_grid);
    generate(&mut snowy, &snow_ground, &tile_bounds);
    let (x, z) = roots[last];
    assert_eq!(
        snowy.get_block_absolute(x - 1088, heights[last] + 1, z - 640),
        Some(OAK_LEAVES),
        "earlier canopy can occupy snow cell, but must not turn off snow's ESA suppression"
    );
}

#[test]
fn master_esa_preserves_preexisting_obstacle_at_effective_road_height() {
    let bounds = XZBBox::rect_from_min_max(0, 0, 127, 127).unwrap();
    let root = (32..96)
        .flat_map(|x| (32..96).map(move |z| (x, z)))
        .find(|&(x, z)| crate::deterministic_rng::coord_rng(x, z, 0).random_range(0..30) == 0)
        .unwrap();
    let grid = grid(
        |x, z| (70., if (x, z) == root { 10 } else { 0 }),
        "Temperate",
        i32::MAX,
    );
    for road_height in [70, 80] {
        for obstacle in [false, true] {
            let (mut editor, ground) = editor(&bounds, (0, 0), &grid);
            if road_height != 70 {
                editor.register_road_surface_y(root.0, root.1, road_height);
                editor.set_block_absolute(STONE_BRICKS, root.0, road_height, root.1, None, None);
            }
            if obstacle {
                editor.set_block_absolute(OAK_LEAVES, root.0, road_height + 1, root.1, None, None);
            }
            generate(&mut editor, &ground, &bounds);
            assert_eq!(
                editor.get_block_absolute(root.0, road_height + 1, root.1),
                Some(if obstacle {
                    OAK_LEAVES
                } else {
                    crate::block_definitions::OAK_LOG
                }),
                "pre-existing obstruction must use effective road Y={road_height}"
            );
        }
    }
}

#[test]
fn master_esa_snow_caps_suppress_actual_tree_candidates() {
    let bounds = XZBBox::rect_from_min_max(0, 0, 127, 127).unwrap();
    let grid = grid(|_, _| (70., 10), "Temperate", 60);
    let (mut editor, ground) = editor(&bounds, (0, 0), &grid);
    generate(&mut editor, &ground, &bounds);
    let mut candidates = 0;
    for x in 16..112 {
        for z in 16..112 {
            if crate::deterministic_rng::coord_rng(x, z, 0).random_range(0..30) == 0 {
                candidates += 1;
                assert_eq!(editor.get_block_absolute(x, 71, z), Some(SNOW_LAYER));
                assert_ne!(
                    editor.get_block_absolute(x, 72, z),
                    Some(crate::block_definitions::OAK_LOG)
                );
            }
        }
    }
    assert!(candidates > 0);
}
