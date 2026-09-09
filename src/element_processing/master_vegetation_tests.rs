//! Parser-through-generator regressions for the external master pattern contract.
use super::*;
use crate::args::Args;
use crate::block_definitions::*;
use crate::coordinate_system::{
    cartesian::XZBBox, geographic::LLBBox, transformation::CoordTransformer,
};
use crate::floodfill_cache::{BuildingFootprintBitmap, FloodFillCache, RoadMaskBitmap};
use crate::osm_parser::{parse_osm_data_with_frame, OsmData, ProcessedElement};
use clap::Parser;

fn assert_polygon_translation(key: &str, value: &str) {
    for offset in [(16, 16), (48, 32)] {
        assert_polygon_offset(key, value, offset, false);
    }
}

fn assert_polygon_offset(key: &str, value: &str, offset: (u32, u32), wet_mask: bool) {
    let bbox = LLBBox::from_str("40,-74,41,-73").unwrap();
    let whole_bounds = XZBBox::rect_from_min_max(0, 0, 127, 127).unwrap();
    let tile_bounds =
        XZBBox::rect_from_min_max(0, 0, 127 - offset.0 as i32, 127 - offset.1 as i32).unwrap();
    let sample = |x, z| {
        (
            70.0,
            if (wet_mask && (x % 7 == 0 || z % 11 == 0)) || (!wet_mask && (x, z) == (5, 20)) {
                crate::land_cover::LC_WATER
            } else {
                10
            },
        )
    };
    let mut whole = sampled_editor(&whole_bounds, (0, 0), sample);
    let mut tile = sampled_editor(&tile_bounds, offset, sample);
    let args = Args::parse_from(["arnis", "--mode", "geo-terrain", "--legacy-trees"]);
    // The parser keeps shared geometry; the translated ring has negative local coordinates.
    let points = [(-32, 2), (120, 18), (120, 120), (-32, 120), (-32, 2)];
    let mut raw: Vec<_> = points.iter().enumerate().map(|(i, &(x,z))| serde_json::json!({
        "type":"node", "id":i+1, "lat":41.0-f64::from(z)/127.0, "lon":-74.0+f64::from(x)/127.0
    })).collect();
    // Closed OSM rings repeat their first node ID.
    raw.push(serde_json::json!({"type":"way", "id":77, "nodes":[1,2,3,4,1], "tags":{key:value}}));
    let raw = serde_json::json!({"elements":raw});
    for (editor, col, row, width, height, bounds) in [
        (&mut whole, 0, 0, 128, 128, &whole_bounds),
        (
            &mut tile,
            offset.0,
            offset.1,
            128 - offset.0,
            128 - offset.1,
            &tile_bounds,
        ),
    ] {
        editor.set_place_schematics(false);
        // Include a protected eligible-prefix interruption and one shared obstruction.
        for (mx, mz) in [(8, 20), (60, 50)] {
            let (x, z) = (mx - col as i32, mz - row as i32);
            if x >= 0 && z >= 0 {
                editor.set_block(BLACK_CONCRETE, x, 0, z, None, None);
            }
        }
        let (frame, parser_bounds) =
            CoordTransformer::for_master_slice(&bbox, 128, 128, col, row, width, height).unwrap();
        let data: OsmData = serde_json::from_value(raw.clone()).unwrap();
        let (elements, _, _, _) = parse_osm_data_with_frame(data, bbox, 1.0, frame, parser_bounds);
        let cache = FloodFillCache::precompute_master(
            &elements,
            None,
            editor.master_geometry().unwrap(),
            bounds.clone(),
        );
        let footprints = BuildingFootprintBitmap::new_empty();
        let roads = RoadMaskBitmap::new_empty();
        let outlines = bridge_styles::BridgeOutlineIndex::build(&[]);
        let structures = bridges::BridgeStructureMap::build(&[], editor, &outlines);
        let surface = bridges::BridgeSurfaceMap::build(&[], &structures, 1.0);
        for element in &elements {
            let ProcessedElement::Way(way) = element else {
                continue;
            };
            match key {
                "landuse" => landuse::generate_landuse(
                    editor,
                    way,
                    &args,
                    &cache,
                    &footprints,
                    &roads,
                    &surface,
                ),
                "natural" => {
                    natural::generate_natural(editor, element, &args, &cache, &footprints, &surface)
                }
                "leisure" => {
                    leisure::generate_leisure(editor, way, &args, &cache, &footprints, &surface)
                }
                "amenity" => amenities::generate_amenities(editor, element, &args, &cache, &roads),
                _ => unreachable!(),
            }
        }
    }
    let mut blocks = 0;
    let mut cane = 0;
    let mut water = 0;
    let mut moss = 0;
    let mut materials = std::collections::HashSet::new();
    let mut mismatches = Vec::new();
    for z in 48..112 {
        for x in 64..112 {
            for y in 70..95 {
                let expected = whole.get_block_absolute(x, y, z);
                let actual = tile.get_block_absolute(x - offset.0 as i32, y, z - offset.1 as i32);
                blocks += usize::from(expected.is_some());
                if let Some(block) = expected {
                    materials.insert(block);
                }
                cane += usize::from(expected == Some(crate::block_definitions::SUGAR_CANE));
                water += usize::from(expected == Some(crate::block_definitions::WATER));
                moss += usize::from(expected == Some(crate::block_definitions::MOSS_BLOCK));
                if expected != actual && mismatches.len() < 10 {
                    mismatches.push((x, y, z, expected, actual));
                }
            }
        }
    }
    assert!(blocks > 3000, "fixture must generate substantial area");
    match value {
        "parking" => assert!(
            [
                GRAY_CONCRETE_POWDER,
                CYAN_TERRACOTTA,
                WHITE_CONCRETE,
                SEA_LANTERN
            ]
            .iter()
            .all(|b| materials.contains(b)),
            "parking must exercise both asphalt materials, markings and lamps"
        ),
        "blockfield" => assert!(
            materials.contains(&COBBLESTONE) && materials.contains(&ANDESITE),
            "rock variation must survive placement"
        ),
        "forest" | "wood" | "orchard" | "park" => assert!(
            materials.contains(&OAK_LOG) || materials.contains(&BIRCH_LOG),
            "fixture must generate actual trees"
        ),
        _ => {}
    }
    if wet_mask {
        assert!(
            cane > 0 && water > 0 && moss > 0,
            "wetland fixture must exercise cane, puddles and rings: {cane}/{water}/{moss}"
        );
    }
    assert!(
        mismatches.is_empty(),
        "{key}={value} translation mismatches: {mismatches:?}"
    );
}

macro_rules! polygon_test {
    ($name:ident, $key:literal, $value:literal) => {
        #[test]
        fn $name() {
            assert_polygon_translation($key, $value);
        }
    };
}
polygon_test!(master_parking_surface_markings_lamps, "amenity", "parking");
polygon_test!(master_landuse_cemetery, "landuse", "cemetery");
polygon_test!(master_landuse_farmland, "landuse", "farmland");
polygon_test!(master_landuse_orchard, "landuse", "orchard");
polygon_test!(master_landuse_forest, "landuse", "forest");
polygon_test!(master_landuse_industrial, "landuse", "industrial");
polygon_test!(master_landuse_military, "landuse", "military");
polygon_test!(master_landuse_quarry, "landuse", "quarry");
polygon_test!(master_natural_rock_variation, "natural", "blockfield");
polygon_test!(master_natural_bare_rock, "natural", "bare_rock");
polygon_test!(master_natural_wood, "natural", "wood");
polygon_test!(master_natural_wetland, "natural", "wetland");
polygon_test!(master_natural_grassland, "natural", "grassland");
polygon_test!(master_natural_scrub, "natural", "scrub");
polygon_test!(master_leisure_park, "leisure", "park");

fn assert_tunnel_translation(rail: bool) {
    use crate::floodfill_cache::CoordinateBitmap;
    let whole_bounds = XZBBox::rect_from_min_max(0, 0, 127, 127).unwrap();
    let tile_bounds = XZBBox::rect_from_min_max(0, 0, 79, 95).unwrap();
    let mut whole = crate::world_editor::translated_pattern_test_editor(&whole_bounds, (0, 0));
    let mut tile = crate::world_editor::translated_pattern_test_editor(&tile_bounds, (48, 32));
    let args = Args::parse_from(["arnis", "--mode", "geo-terrain"]);
    for (editor, offset) in [(&mut whole, (0, 0)), (&mut tile, (48, 32))] {
        let tags = if rail {
            vec![("railway", "rail"), ("tunnel", "yes")]
        } else {
            vec![("highway", "service"), ("tunnel", "yes")]
        };
        let mut way = building_test_support::rect_way(77, 2, 80, 120, 80, &tags);
        way.nodes.truncate(2);
        for node in &mut way.nodes {
            node.x -= offset.0;
            node.z -= offset.1;
        }
        let outlines = bridge_styles::BridgeOutlineIndex::build(&[]);
        let empty = CoordinateBitmap::new_empty();
        if rail {
            railways::generate_railways(
                editor,
                &way,
                &mut Vec::new(),
                &Default::default(),
                &outlines,
                &empty,
                &empty,
                &empty,
                None,
            );
        } else {
            let structures = bridges::BridgeStructureMap::build(&[], editor, &outlines);
            let surface = bridges::BridgeSurfaceMap::build(&[], &structures, 1.0);
            highways::generate_highways(
                editor,
                &ProcessedElement::Way(way),
                &args,
                &Default::default(),
                &FloodFillCache::new(),
                &empty,
                &structures,
                &surface,
                &Default::default(),
                &Default::default(),
                &empty,
                &mut Vec::new(),
            );
        }
    }
    let mut variants = 0;
    let mut mismatches = Vec::new();
    for z in 76..85 {
        for x in 64..112 {
            for y in 55..71 {
                let expected = whole.get_block_absolute(x, y, z);
                let actual = tile.get_block_absolute(x - 48, y, z - 32);
                variants +=
                    usize::from(expected == Some(crate::block_definitions::CRACKED_STONE_BRICKS));
                if expected != actual && mismatches.len() < 10 {
                    mismatches.push((x, y, z, expected, actual));
                }
            }
        }
    }
    assert!(variants > 10, "must exercise shell material variation");
    assert!(mismatches.is_empty(), "rail={rail}: {mismatches:?}");
}
#[test]
fn master_road_tunnel_texture() {
    assert_tunnel_translation(false);
}
#[test]
fn master_rail_tunnel_texture() {
    assert_tunnel_translation(true);
}

fn sampled_editor<'a>(
    bounds: &'a XZBBox,
    offset: (u32, u32),
    sample: impl Fn(i32, i32) -> (f32, u8),
) -> crate::world_editor::WorldEditor<'a> {
    use crate::elevation::master_grid;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("master");
    let mut grid = master_grid::tests::fixture();
    grid.metadata.width = 128;
    grid.metadata.height = 128;
    grid.metadata.world_width = 128;
    grid.metadata.world_height = 128;
    grid.metadata.payload_bytes = 128 * 128 * 10;
    let samples: Vec<_> = (0..128)
        .flat_map(|z| (0..128).map(move |x| (x, z)))
        .map(|(x, z)| sample(x, z))
        .collect();
    grid.elevation = samples.iter().map(|s| s.0).collect();
    grid.land_cover = samples.iter().map(|s| s.1).collect();
    grid.water_distance = vec![0; 128 * 128];
    grid.water_blend = vec![0.0; 128 * 128];
    master_grid::save(&path, &grid).unwrap();
    let ground = crate::ground::Ground::from_master_slice(
        master_grid::load_slice(
            &path,
            offset.0,
            offset.1,
            (bounds.max_x() + 1) as u32,
            (bounds.max_z() + 1) as u32,
        )
        .unwrap(),
    )
    .unwrap();
    let mut editor = crate::world_editor::WorldEditor::new(
        "/dev/null/unused".into(),
        bounds,
        LLBBox::from_str("40,-74,41,-73").unwrap(),
    );
    editor.set_ground(std::sync::Arc::new(ground));
    editor.set_external_tile(true);
    editor.set_place_schematics(false);
    editor
}

#[test]
fn master_wetland_accepted_puddles_cane_and_rings() {
    assert_polygon_offset("natural", "wetland", (16, 16), true);
    assert_polygon_offset("natural", "wetland", (48, 32), true);
}

#[test]
fn master_eight_root_slope_dependency_is_bounded_by_terrain() {
    use crate::block_definitions::{GRASS_BLOCK, OAK_LEAVES};
    use tree::{Tree, TreeType};
    let bounds = XZBBox::rect_from_min_max(0, 0, 127, 127).unwrap();
    let tile_bounds = XZBBox::rect_from_min_max(0, 0, 79, 127).unwrap();
    let z = 64;
    let roots: Vec<i32> = (0..8).map(|i| 47 + i * 3).collect();
    let mut heights = vec![70];
    // Determine a real non-gap leaf level at each next root from an isolated legacy oak.
    // This finite eight-candidate fixture forces placement choices, but exercises actual
    // ground painting, eligibility checks, tree shapes and writes, not a tree mock.
    for &x in roots.iter().take(7) {
        let height = *heights.last().unwrap();
        let mut isolated = sampled_editor(&bounds, (0, 0), |cx, cz| {
            (
                if (cx, cz) == (x, z) {
                    height as f32
                } else {
                    70.0
                },
                10,
            )
        });
        Tree::create_of_type(&mut isolated, (x, 1, z), TreeType::Oak, None, None, false);
        let next_height = (height + 1..height + 24)
            .find(|&y| isolated.get_block_absolute(x + 3, y, z) == Some(OAK_LEAVES))
            .expect("oak must reach the next candidate with an actual leaf");
        heights.push(next_height);
    }
    let sample = |x, z0| {
        (
            if z0 == z {
                roots
                    .iter()
                    .position(|&r| r == x)
                    .map_or(70, |i| heights[i])
            } else {
                70
            } as f32,
            10,
        )
    };
    let mut whole = sampled_editor(&bounds, (0, 0), sample);
    let mut tile = sampled_editor(&tile_bounds, (48, 0), sample);
    let mut admissions = Vec::new();
    for (editor, offset) in [(&mut whole, 0), (&mut tile, 48)] {
        let mut admitted = Vec::new();
        for &mx in &roots {
            if mx < offset {
                continue;
            }
            let x = mx - offset;
            // Same ground write and GRASS_BLOCK eligibility as landuse=forest.
            editor.set_block(GRASS_BLOCK, x, 0, z, None, None);
            if editor.check_for_block(x, 0, z, Some(&[GRASS_BLOCK])) {
                admitted.push(mx);
                Tree::create_of_type(editor, (x, 1, z), TreeType::Oak, None, None, false);
            }
        }
        admissions.push(admitted);
    }
    for &x in &roots[6..] {
        assert_eq!(admissions[0].contains(&x),admissions[1].contains(&x),"earlier tree changed owned root admission beyond halo16; heights={heights:?}, admissions={admissions:?}");
    }
    for x in 64..72 {
        for y in 70..heights[7] + 25 {
            assert_eq!(
                whole.get_block_absolute(x, y, z),
                tile.get_block_absolute(x - 48, y, z),
                "owned slope canopy differs at {x},{y},{z}"
            );
        }
    }
    assert_eq!(
        admissions[0].len(),
        8,
        "trees must not replace future terrain cells"
    );
}

#[test]
fn master_procedural_branch_logs_stay_above_terrain() {
    use crate::block_definitions::OAK_LOG;
    use tree::{Tree, TreeType};
    let bounds = XZBBox::rect_from_min_max(0, 0, 127, 127).unwrap();
    let mut branch = None;
    for x in 20..36 {
        let mut isolated = sampled_editor(&bounds, (0, 0), |_, _| (70.0, 10));
        Tree::create_of_type(&mut isolated, (x, 1, 64), TreeType::Oak, None, None, false);
        for dx in -2..=2 {
            for dz in -2..=2 {
                for y in 71..90 {
                    if (dx, dz) != (0, 0)
                        && isolated.get_block_absolute(x + dx, y, 64 + dz) == Some(OAK_LOG)
                    {
                        branch = Some((x, x + dx, y, 64 + dz));
                    }
                }
            }
        }
        if branch.is_some() {
            break;
        }
    }
    let (root_x, bx, by, bz) = branch.expect("finite fixture must exercise a real branch log");
    let mut slope = sampled_editor(&bounds, (0, 0), |x, z| {
        (if (x, z) == (bx, bz) { by as f32 } else { 70.0 }, 10)
    });
    Tree::create_of_type(
        &mut slope,
        (root_x, 1, 64),
        TreeType::Oak,
        None,
        None,
        false,
    );
    assert_eq!(
        slope.get_block_absolute(bx, by, bz),
        None,
        "branch must not occupy a future terrain cell"
    );
}

#[test]
fn master_micro_tree_leaves_stay_above_terrain() {
    use tree::{Tree, TreeType};
    let bounds = XZBBox::rect_from_min_max(0, 0, 127, 127).unwrap();
    let mut slope = sampled_editor(&bounds, (0, 0), |x, z| {
        (if (x, z) == (65, 64) { 73.0 } else { 70.0 }, 10)
    });
    slope.set_projection_info("local", 0.12);
    Tree::create_of_type(&mut slope, (64, 1, 64), TreeType::Oak, None, None, false);
    assert_eq!(
        slope.get_block_absolute(65, 73, 64),
        None,
        "micro canopy must not occupy a future terrain cell"
    );
    assert!(
        slope.get_block_absolute(63, 73, 64).is_some(),
        "the same canopy remains on the downhill side"
    );
}

#[test]
fn stock_overlapping_legacy_trees_keep_their_blocks() {
    use sha2::{Digest, Sha256};
    use tree::{Tree, TreeType};
    let bounds = XZBBox::rect_from_min_max(0, 0, 127, 127).unwrap();
    let mut stock = crate::world_editor::WorldEditor::new(
        "/dev/null/unused".into(),
        &bounds,
        LLBBox::from_str("40,-74,41,-73").unwrap(),
    );
    stock.set_ground(std::sync::Arc::new(crate::ground::Ground::new_flat(70)));
    Tree::create_of_type(&mut stock, (63, 1, 64), TreeType::Oak, None, None, false);
    Tree::create_of_type(&mut stock, (64, 1, 64), TreeType::Birch, None, None, false);
    let tile_bounds = XZBBox::rect_from_min_max(0, 0, 79, 127).unwrap();
    let mut whole = sampled_editor(&bounds, (0, 0), |_, _| (70.0, 10));
    let mut tile = sampled_editor(&tile_bounds, (48, 0), |_, _| (70.0, 10));
    for (editor, offset) in [(&mut whole, 0), (&mut tile, 48)] {
        Tree::create_of_type(
            editor,
            (63 - offset, 1, 64),
            TreeType::Oak,
            None,
            None,
            false,
        );
        Tree::create_of_type(
            editor,
            (64 - offset, 1, 64),
            TreeType::Birch,
            None,
            None,
            false,
        );
    }
    let mut digest = Sha256::new();
    for z in 56i32..73 {
        for x in 55i32..73 {
            for y in 69i32..101 {
                let expected = stock.get_block_absolute(x, y, z);
                assert_eq!(
                    expected,
                    whole.get_block_absolute(x, y, z),
                    "external flat overlap must preserve stock blocks"
                );
                assert_eq!(
                    expected,
                    tile.get_block_absolute(x - 48, y, z),
                    "core-boundary overlapping trees must share leaf winners"
                );
                if let Some(block) = expected {
                    digest.update(x.to_le_bytes());
                    digest.update(y.to_le_bytes());
                    digest.update(z.to_le_bytes());
                    digest.update(block.id().to_le_bytes());
                }
            }
        }
    }
    assert_eq!(
        format!("{:x}", digest.finalize()),
        "af816f81663c0d6360508ee62317489f7e730d0d402960544e362777aabf5eed"
    );
}

#[test]
fn master_all_legacy_species_and_micro_canopies_preserve_uphill_ground() {
    use tree::{Tree, TreeType::*};
    let bounds = XZBBox::rect_from_min_max(0, 0, 127, 127).unwrap();
    for species in [
        Oak,
        Spruce,
        Birch,
        DarkOak,
        Jungle,
        Acacia,
        Cherry,
        TallOak,
        Pine,
        Bush,
        AzaleaBush,
        Willow,
        FloweringOak,
        Mangrove,
    ] {
        let species_index = species as usize;
        for scale in [1.0, 0.12] {
            let mut editor = sampled_editor(&bounds, (0, 0), |x, z| {
                (if (x, z) == (64, 64) { 70.0 } else { 90.0 }, 10)
            });
            editor.set_projection_info("local", scale);
            Tree::create_of_type(&mut editor, (64, 1, 64), species, None, None, false);
            assert!(
                (71..95).any(|y| editor.get_block_absolute(64, y, 64).is_some()),
                "{species_index} scale{scale} must generate a tree"
            );
            for z in 61..68 {
                for x in 61..68 {
                    if (x, z) == (64, 64) {
                        continue;
                    }
                    for y in 70..=90 {
                        assert_eq!(
                            editor.get_block_absolute(x, y, z),
                            None,
                            "{species_index} scale{scale} writes into uphill terrain at{x},{y},{z}"
                        );
                    }
                }
            }
        }
    }
}
