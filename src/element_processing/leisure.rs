use crate::args::Args;
use crate::block_definitions::*;
use crate::bresenham::bresenham_line;
use crate::deterministic_rng::element_rng;
use crate::element_processing::bridges::BridgeSurfaceMap;
use crate::element_processing::surfaces::get_blocks_for_surface;
use crate::element_processing::tree::Tree;
use crate::floodfill_cache::{is_oversized_ring, BuildingFootprintBitmap, FloodFillCache};
use crate::land_cover::osm_water_override::has_explicit_water_tag;
use crate::osm_parser::{ProcessedMemberRole, ProcessedRelation, ProcessedWay};
use crate::world_editor::WorldEditor;
use rand::Rng;

pub fn generate_leisure(
    editor: &mut WorldEditor,
    element: &ProcessedWay,
    args: &Args,
    flood_fill_cache: &FloodFillCache,
    building_footprints: &BuildingFootprintBitmap,
    bridge_surface: &BridgeSurfaceMap,
) {
    if let Some(leisure_type) = element.tags.get("leisure") {
        // Explicit water bodies belong to water rendering, including when a
        // leisure surface override would otherwise paint land over them.
        if has_explicit_water_tag(&element.tags)
            && !matches!(leisure_type.as_str(), "swimming_pool" | "swimming_area")
        {
            return;
        }

        let mut previous_node: Option<(i32, i32)> = None;
        let mut corner_count: i32 = 0;
        let mut current_leisure: Vec<(i32, i32)> = vec![];

        // Determine block type based on leisure type
        let mut block_type: Block = match leisure_type.as_str() {
            "park" | "nature_reserve" | "garden" | "disc_golf_course" | "golf_course" => {
                GRASS_BLOCK
            }
            "schoolyard" => BLACK_CONCRETE,
            "playground" | "recreation_ground" | "pitch" | "beach_resort" | "dog_park" => {
                GREEN_STAINED_HARDENED_CLAY
            }
            "swimming_pool" | "swimming_area" => WATER, // Swimming area: Area in a larger body of water for swimming
            "marina" => WATER, // A sort of parking lot for small watercraft
            "bathing_place" => SMOOTH_SANDSTONE, // Could be sand or concrete
            "outdoor_seating" => SMOOTH_STONE, //Usually stone or stone bricks
            "water_park" | "slipway" => LIGHT_GRAY_CONCRETE, // Water park area, not the pool. Usually is concrete
            "ice_rink" => PACKED_ICE, // TODO: Ice for Ice Rink, needs building defined
            _ => GRASS_BLOCK,
        };
        // Explicit surface=* overrides the category default. Leave
        // `block_type` untouched for unknown surface values so existing
        // behaviour is preserved.
        if let Some(surface) = element.tags.get("surface") {
            if let Some(blocks) = get_blocks_for_surface(surface) {
                block_type = blocks[0];
            }
        }
        let master_water = editor.tiler_owns_bathymetry()
            && block_type == WATER
            && !matches!(leisure_type.as_str(), "swimming_pool" | "swimming_area");

        // Resolve the fill before painting the edge, for the same reason as in natural.rs:
        // a closed ring the fill refused must not leave a border around unfilled ground.
        let filled_area = flood_fill_cache.get_or_compute(element, args.timeout.as_ref());
        if filled_area.is_empty() && is_oversized_ring(element) {
            return;
        }

        // Process leisure area nodes
        for node in &element.nodes {
            if let Some(prev) = previous_node {
                // Draw a line between the current and previous node
                let bresenham_points: Vec<(i32, i32, i32)> =
                    bresenham_line(prev.0, 0, prev.1, node.x, 0, node.z);
                for (bx, _, bz) in bresenham_points {
                    if master_water {
                        if let Some(surface) = editor.master_water_surface(bx, bz) {
                            editor.set_block_if_absent_absolute(WATER, bx, surface, bz);
                        }
                        continue;
                    }
                    editor.set_block(
                        block_type,
                        bx,
                        0,
                        bz,
                        Some(&[
                            GRASS_BLOCK,
                            STONE_BRICKS,
                            SMOOTH_STONE,
                            LIGHT_GRAY_CONCRETE,
                            COBBLESTONE,
                            GRAY_CONCRETE,
                        ]),
                        None,
                    );
                }

                current_leisure.push((node.x, node.z));
                corner_count += 1;
            }
            previous_node = Some((node.x, node.z));
        }

        // Flood-fill the interior of the leisure area using cache
        if corner_count > 0 {
            // Use deterministic RNG seeded by element ID for consistent results across region boundaries
            let mut rng = element_rng(element.id);

            for &(x, z) in filled_area.iter() {
                if master_water {
                    if let Some(surface) = editor.master_water_surface(x, z) {
                        editor.set_block_if_absent_absolute(WATER, x, surface, z);
                    }
                    continue;
                }
                editor.set_block(block_type, x, 0, z, Some(&[GRASS_BLOCK]), None);

                // Land-cover water is skipped because a park often spans its
                // own lake, and the carve after this leaves plants floating.
                if matches!(leisure_type.as_str(), "park" | "garden" | "nature_reserve")
                    && editor.check_for_block(x, 0, z, Some(&[GRASS_BLOCK]))
                    && !editor.is_lc_water(x, z)
                {
                    let random_choice: i32 = rng.random_range(0..1000);

                    match random_choice {
                        0..30 => {
                            // Plants
                            let plant_choice = match random_choice {
                                0..5 => RED_FLOWER,
                                5..10 => YELLOW_FLOWER,
                                10..16 => BLUE_FLOWER,
                                16..22 => WHITE_FLOWER,
                                22..30 => FERN,
                                _ => unreachable!(),
                            };
                            editor.set_block(plant_choice, x, 1, z, None, None);
                        }
                        30..90 => {
                            // Grass
                            editor.set_block(GRASS, x, 1, z, None, None);
                        }
                        90..105 => {
                            // Oak leaves
                            editor.set_block(OAK_LEAVES, x, 1, z, None, None);
                        }
                        105..120 => {
                            // Only where land cover says woody, else a park
                            // canopies its own meadows. 1/1000 for specimens.
                            if random_choice == 105 || editor.land_cover_backs_trees(x, z) {
                                Tree::create(
                                    editor,
                                    (x, 1, z),
                                    Some(building_footprints),
                                    Some(bridge_surface),
                                );
                            } else {
                                editor.set_block(GRASS, x, 1, z, None, None);
                            }
                        }
                        _ => {}
                    }
                }
            }

            // Stamp bundled playground structures (replaces the old procedural props).
            if matches!(leisure_type.as_str(), "playground" | "recreation_ground") {
                crate::structures::playground::scatter_playgrounds(editor, filled_area.as_slice());
            }

            if leisure_type == "pitch" {
                // Clear park/ground vegetation scattered onto the pitch before marking.
                let vegetation: &[Block] = &[
                    GRASS,
                    FERN,
                    RED_FLOWER,
                    YELLOW_FLOWER,
                    BLUE_FLOWER,
                    WHITE_FLOWER,
                    OAK_LEAVES,
                ];
                for &(x, z) in filled_area.iter() {
                    editor.set_block(AIR, x, 1, z, Some(vegetation), None);
                }
                crate::element_processing::sport_pitches::draw_pitch_markings(
                    editor,
                    element,
                    filled_area.as_slice(),
                    block_type,
                );
            }
        }
    }
}

pub fn generate_leisure_from_relation(
    editor: &mut WorldEditor,
    rel: &ProcessedRelation,
    args: &Args,
    flood_fill_cache: &FloodFillCache,
    building_footprints: &BuildingFootprintBitmap,
    bridge_surface: &BridgeSurfaceMap,
) {
    if rel.tags.get("leisure").map(String::as_str) == Some("park") {
        // Process each outer member way individually using cached flood fill.
        // We intentionally do not combine all outer nodes into one mega-way,
        // because that creates a nonsensical polygon spanning the whole relation
        // extent, misses the flood fill cache, and can cause multi-GB allocations.
        for member in &rel.members {
            if member.role == ProcessedMemberRole::Outer {
                // Use relation tags so the member inherits the relation's leisure=* type
                let way_with_rel_tags = ProcessedWay {
                    id: member.way.id,
                    nodes: member.way.nodes.clone(),
                    tags: rel.tags.clone(),
                };
                generate_leisure(
                    editor,
                    &way_with_rel_tags,
                    args,
                    flood_fill_cache,
                    building_footprints,
                    bridge_surface,
                );
            }
        }
    }
}

#[cfg(test)]
mod water_guard_tests {
    use super::*;

    use crate::coordinate_system::cartesian::XZBBox;
    use crate::coordinate_system::geographic::LLBBox;
    use crate::element_processing::bridges::BridgeStructureMap;
    use crate::osm_parser::ProcessedNode;
    use clap::Parser;
    use std::collections::HashMap;

    fn fixture() -> (WorldEditor<'static>, Args, BridgeSurfaceMap) {
        static BBOX: std::sync::LazyLock<XZBBox> =
            std::sync::LazyLock::new(|| XZBBox::rect_from_min_max(0, 0, 15, 15).unwrap());
        let editor = WorldEditor::new(
            std::path::PathBuf::from("/dev/null/unused"),
            &BBOX,
            LLBBox::new(54.6, 9.9, 54.61, 9.91).unwrap(),
        );
        let outlines = crate::element_processing::bridge_styles::BridgeOutlineIndex::build(&[]);
        let structures = BridgeStructureMap::build(&[], &editor, &outlines);
        let bridges = BridgeSurfaceMap::build(&[], &structures, 1.0);
        (
            editor,
            Args::parse_from(["arnis", "--bbox", "1,2,3,4"]),
            bridges,
        )
    }

    fn ring(tags: &[(&str, &str)]) -> ProcessedWay {
        ProcessedWay {
            id: 42,
            tags: tags
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            nodes: [(1, 2, 2), (2, 10, 2), (3, 10, 10), (4, 2, 10), (1, 2, 2)]
                .into_iter()
                .map(|(id, x, z)| ProcessedNode {
                    id,
                    x,
                    z,
                    tags: HashMap::new(),
                })
                .collect(),
        }
    }

    fn assert_empty(editor: &WorldEditor) {
        for x in 0..=15 {
            for z in 0..=15 {
                for y in 0..=3 {
                    assert!(
                        !editor.block_exists_absolute(x, y, z),
                        "unexpected block at {x},{y},{z}"
                    );
                }
            }
        }
    }

    fn render(tags: &[(&str, &str)]) -> WorldEditor<'static> {
        let (mut editor, args, bridges) = fixture();
        generate_leisure(
            &mut editor,
            &ring(tags),
            &args,
            &FloodFillCache::new(),
            &BuildingFootprintBitmap::new_empty(),
            &bridges,
        );
        editor
    }

    #[test]
    fn explicit_water_marina_surface_override_paints_nothing() {
        assert_empty(&render(&[
            ("leisure", "marina"),
            ("water", "harbour"),
            ("surface", "concrete"),
        ]));
    }

    #[test]
    fn explicit_water_unmatched_leisure_paints_nothing() {
        assert_empty(&render(&[("leisure", "unmatched"), ("water", "lake")]));
    }

    #[test]
    fn nonwater_park_keeps_its_ground() {
        let editor = render(&[("leisure", "park")]);
        for (x, z) in [(2, 2), (5, 5)] {
            assert!(editor.check_for_block(x, 0, z, Some(&[GRASS_BLOCK])));
        }
    }

    #[test]
    fn negated_water_keeps_upstream_leisure_surface_behavior() {
        for negative in ["no", "0", "false"] {
            let editor = render(&[("leisure", "marina"), ("water", negative)]);
            assert!(
                editor.check_for_block(5, 0, 5, Some(&[WATER])),
                "water={negative}"
            );
            let editor = render(&[("leisure", "park"), ("water", negative)]);
            assert!(
                editor.check_for_block(5, 0, 5, Some(&[GRASS_BLOCK])),
                "water={negative}"
            );
        }
    }

    #[test]
    fn swimming_features_keep_water_generation_with_explicit_water() {
        for leisure in ["swimming_pool", "swimming_area"] {
            let editor = render(&[("leisure", leisure), ("water", "pool")]);
            for (x, z) in [(2, 2), (5, 5)] {
                assert!(editor.check_for_block(x, 0, z, Some(&[WATER])), "{leisure}");
            }
        }
    }

    #[test]
    fn coastal_marina_obeys_master_mask_but_swimming_features_remain_explicit() {
        for leisure in ["marina", "swimming_pool", "swimming_area"] {
            let (mut editor, args, bridges) = fixture();
            editor.set_ground(std::sync::Arc::new(
                crate::water_depth::tests::master_ground(16, 70.0, &[(2, 2), (5, 5)]),
            ));
            editor.set_external_tile(true);
            generate_leisure(
                &mut editor,
                &ring(&[("leisure", leisure)]),
                &args,
                &FloodFillCache::new(),
                &BuildingFootprintBitmap::new_empty(),
                &bridges,
            );
            for (x, z) in [(2, 2), (5, 5)] {
                assert_eq!(
                    editor.get_block_absolute(x, 70, z),
                    if leisure == "marina" {
                        None
                    } else {
                        Some(WATER)
                    },
                    "{leisure}"
                );
            }
            assert_eq!(
                editor.get_block_absolute(6, 70, 5),
                Some(WATER),
                "{leisure}"
            );
        }
    }
}
