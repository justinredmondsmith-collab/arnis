use crate::args::Args;
use crate::block_definitions::*;
use crate::bresenham::bresenham_line;
use crate::deterministic_rng::element_rng;
use crate::element_processing::surfaces::get_blocks_for_surface;
use crate::element_processing::tree::Tree;
use crate::floodfill_cache::{BuildingFootprintBitmap, FloodFillCache};
use crate::osm_parser::{ProcessedMemberRole, ProcessedRelation, ProcessedWay};
use crate::world_editor::WorldEditor;
use rand::Rng;

pub fn generate_leisure(
    editor: &mut WorldEditor,
    element: &ProcessedWay,
    args: &Args,
    flood_fill_cache: &FloodFillCache,
    building_footprints: &BuildingFootprintBitmap,
) {
    if let Some(leisure_type) = element.tags.get("leisure") {
        // 2026-06-06 Morris Canal Basin water-loss fix.
        // OSM way 53865549 carries BOTH `leisure=marina` AND `water=lake`.
        // The `water` tag gives it water-priority in the element sort, so it
        // ran BEFORE the basin water relation, but it dispatches here, where
        // `marina` is unmatched and falls through to `_ => GRASS_BLOCK`,
        // painting the whole basin with grass at the waterline. The water
        // relation's v5 no-overwrite scanline then (correctly) refused to fill
        // water over that grass. An element that ALSO carries a `water=*` tag
        // IS a water body per OSM semantics — the water generators own its
        // surface, so we must not paint a land default over it here. The only
        // leisure values this function itself renders as actual WATER
        // (swimming_pool / swimming_area) keep their behaviour: painting water
        // over a water-tagged pool is intended and harmless.
        if element.tags.contains_key("water")
            && !matches!(leisure_type.as_str(), "swimming_pool" | "swimming_area")
        {
            return;
        }

        let mut previous_node: Option<(i32, i32)> = None;
        let mut corner_addup: (i32, i32, i32) = (0, 0, 0);
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
            "swimming_pool" | "swimming_area" => WATER, //Swimming area: Area in a larger body of water for swimming
            "bathing_place" => SMOOTH_SANDSTONE,        // Could be sand or concrete
            "outdoor_seating" => SMOOTH_STONE,          //Usually stone or stone bricks
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

        // Process leisure area nodes
        for node in &element.nodes {
            if let Some(prev) = previous_node {
                // Draw a line between the current and previous node
                let bresenham_points: Vec<(i32, i32, i32)> =
                    bresenham_line(prev.0, 0, prev.1, node.x, 0, node.z);
                for (bx, _, bz) in bresenham_points {
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
                corner_addup.0 += node.x;
                corner_addup.1 += node.z;
                corner_addup.2 += 1;
            }
            previous_node = Some((node.x, node.z));
        }

        // Flood-fill the interior of the leisure area using cache
        if corner_addup != (0, 0, 0) {
            let filled_area = flood_fill_cache.get_or_compute(element, args.timeout.as_ref());

            // Use deterministic RNG seeded by element ID for consistent results across region boundaries
            let mut rng = element_rng(element.id);

            for &(x, z) in filled_area.iter() {
                editor.set_block(block_type, x, 0, z, Some(&[GRASS_BLOCK]), None);

                // Add decorative elements for parks and gardens
                if matches!(leisure_type.as_str(), "park" | "garden" | "nature_reserve")
                    && editor.check_for_block(x, 0, z, Some(&[GRASS_BLOCK]))
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
                            // Tree
                            Tree::create(editor, (x, 1, z), Some(building_footprints));
                        }
                        _ => {}
                    }
                }

                // Add playground or recreation ground features
                if matches!(leisure_type.as_str(), "playground" | "recreation_ground") {
                    let random_choice: i32 = rng.random_range(0..5000);

                    match random_choice {
                        0..10 => {
                            // Swing set
                            for y in 1..=3 {
                                editor.set_block(OAK_FENCE, x - 1, y, z, None, None);
                                editor.set_block(OAK_FENCE, x + 1, y, z, None, None);
                            }
                            editor.set_block(OAK_PLANKS, x - 1, 4, z, None, None);
                            editor.set_block(OAK_SLAB, x, 4, z, None, None);
                            editor.set_block(OAK_PLANKS, x + 1, 4, z, None, None);
                            editor.set_block(STONE_BLOCK_SLAB, x, 2, z, None, None);
                        }
                        10..20 => {
                            // Slide
                            editor.set_block(OAK_SLAB, x, 1, z, None, None);
                            editor.set_block(OAK_SLAB, x + 1, 2, z, None, None);
                            editor.set_block(OAK_SLAB, x + 2, 3, z, None, None);

                            editor.set_block(OAK_PLANKS, x + 2, 2, z, None, None);
                            editor.set_block(OAK_PLANKS, x + 2, 1, z, None, None);

                            editor.set_block(LADDER, x + 2, 2, z - 1, None, None);
                            editor.set_block(LADDER, x + 2, 1, z - 1, None, None);
                        }
                        20..30 => {
                            // Sandpit
                            editor.fill_blocks(
                                SAND,
                                x - 3,
                                0,
                                z - 3,
                                x + 3,
                                0,
                                z + 3,
                                Some(&[GREEN_STAINED_HARDENED_CLAY]),
                                None,
                            );
                        }
                        _ => {}
                    }
                }
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
) {
    if rel.tags.get("leisure") == Some(&"park".to_string()) {
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
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_definitions::GRASS_BLOCK;
    use crate::coordinate_system::cartesian::XZBBox;
    use crate::ground::Ground;
    use crate::osm_parser::ProcessedNode;
    use clap::Parser;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    const TW: usize = 17; // 17×17 world, blocks 0..=16, 1:1 grid↔world

    fn node(x: i32, z: i32) -> ProcessedNode {
        ProcessedNode {
            id: 0,
            tags: HashMap::new(),
            x,
            z,
        }
    }

    fn flat_ground() -> Arc<Ground> {
        // Flat Y10 ground, arbitrary land-cover class everywhere.
        Arc::new(Ground::new_synthetic(
            vec![vec![10.0; TW]; TW],
            vec![vec![30u8; TW]; TW],
        ))
    }

    /// Minimal valid Args for element processing (Java, dummy bbox/output).
    fn test_args() -> Args {
        let tmpdir = std::env::temp_dir();
        let cmd = [
            "arnis",
            "--output-dir",
            tmpdir.to_str().unwrap(),
            "--bbox",
            "1,2,3,4",
        ];
        Args::parse_from(cmd.iter())
    }

    fn new_editor<'a>(xzbbox: &'a XZBBox, ground: &Arc<Ground>) -> WorldEditor<'a> {
        let mut editor = WorldEditor::new(
            PathBuf::from("/tmp/arnis-leisure-test"),
            xzbbox,
            crate::coordinate_system::geographic::LLBBox::new(0.0, 0.0, 0.001, 0.001).unwrap(),
        );
        editor.set_ground(Arc::clone(ground));
        editor
    }

    /// Closed square polygon covering blocks [2,14]² with the given tags.
    fn closed_square(tags: HashMap<String, String>) -> ProcessedWay {
        ProcessedWay {
            id: 1,
            nodes: vec![node(2, 2), node(14, 2), node(14, 14), node(2, 14), node(2, 2)],
            tags,
        }
    }

    /// Probe coordinates well inside the polygon interior.
    const PROBES: &[(i32, i32)] = &[(8, 8), (5, 5), (11, 11), (8, 5), (5, 11)];

    /// Morris Canal Basin class: a closed `leisure=marina` + `water=lake`
    /// polygon must paint NOTHING here — the element is a water body per OSM
    /// semantics and the water generators own its surface. (Pre-fix this
    /// painted GRASS_BLOCK across the whole interior at the waterline.)
    #[test]
    fn marina_with_water_tag_paints_nothing() {
        let xzbbox = XZBBox::rect_from_xz_lengths(16.0, 16.0).unwrap();
        let ground = flat_ground();
        let mut editor = new_editor(&xzbbox, &ground);
        let mut tags = HashMap::new();
        tags.insert("leisure".to_string(), "marina".to_string());
        tags.insert("water".to_string(), "lake".to_string());
        let way = closed_square(tags);
        let cache = FloodFillCache::new();
        let footprints = BuildingFootprintBitmap::new_empty();
        let args = test_args();

        generate_leisure(&mut editor, &way, &args, &cache, &footprints);

        for &(x, z) in PROBES {
            // No block of any kind should have been written at the waterline.
            assert!(
                !editor.block_exists_absolute(x, 10, z),
                "marina+water polygon must not paint anything at interior ({x},{z}); \
                 the water generators own this surface"
            );
            assert!(
                !editor.check_for_block(x, 0, z, Some(&[GRASS_BLOCK])),
                "marina+water polygon must not paint GRASS_BLOCK at ({x},{z})"
            );
        }
    }

    /// Guard must NOT over-fire: a plain `leisure=park` way (no water tag)
    /// still paints grass as before.
    #[test]
    fn park_without_water_still_paints_grass() {
        let xzbbox = XZBBox::rect_from_xz_lengths(16.0, 16.0).unwrap();
        let ground = flat_ground();
        let mut editor = new_editor(&xzbbox, &ground);
        let mut tags = HashMap::new();
        tags.insert("leisure".to_string(), "park".to_string());
        let way = closed_square(tags);
        let cache = FloodFillCache::new();
        let footprints = BuildingFootprintBitmap::new_empty();
        let args = test_args();

        generate_leisure(&mut editor, &way, &args, &cache, &footprints);

        // At least the central interior cell must be grass.
        assert!(
            editor.check_for_block(8, 0, 8, Some(&[GRASS_BLOCK])),
            "plain leisure=park must still paint GRASS_BLOCK in its interior"
        );
    }

    /// The guard must not break leisure values that this function renders as
    /// actual WATER: a `leisure=swimming_pool` + `water=*` polygon keeps
    /// painting water (the water generators do not own a stand-alone pool).
    #[test]
    fn swimming_pool_with_water_tag_still_paints_water() {
        let xzbbox = XZBBox::rect_from_xz_lengths(16.0, 16.0).unwrap();
        let ground = flat_ground();
        let mut editor = new_editor(&xzbbox, &ground);
        let mut tags = HashMap::new();
        tags.insert("leisure".to_string(), "swimming_pool".to_string());
        tags.insert("water".to_string(), "pool".to_string());
        let way = closed_square(tags);
        let cache = FloodFillCache::new();
        let footprints = BuildingFootprintBitmap::new_empty();
        let args = test_args();

        generate_leisure(&mut editor, &way, &args, &cache, &footprints);

        assert!(
            editor.check_for_block(8, 0, 8, Some(&[WATER])),
            "leisure=swimming_pool with a water tag must still paint WATER"
        );
    }
}
