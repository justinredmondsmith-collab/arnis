use crate::args::Args;
use crate::block_definitions::*;
use crate::bresenham::bresenham_line;
use crate::floodfill::flood_fill_area;
use crate::osm_parser::{
    ProcessedElement, ProcessedMemberRole, ProcessedNode, ProcessedRelation, ProcessedWay,
};
use crate::world_editor::WorldEditor;
use std::collections::HashSet;

pub fn generate_man_made(editor: &mut WorldEditor, element: &ProcessedElement, args: &Args) {
    // Skip if 'layer' or 'level' is negative in the tags
    if let Some(layer) = element.tags().get("layer") {
        if layer.parse::<i32>().unwrap_or(0) < 0 {
            return;
        }
    }

    if let Some(level) = element.tags().get("level") {
        if level.parse::<i32>().unwrap_or(0) < 0 {
            return;
        }
    }

    if let Some(man_made_type) = element.tags().get("man_made") {
        match man_made_type.as_str() {
            "pier" => generate_pier(editor, element),
            "antenna" => generate_antenna(editor, element),
            "chimney" => generate_chimney(editor, element),
            "water_well" => generate_water_well(editor, element),
            "water_tower" | "silo" | "storage_tank" => {
                generate_tank_structure(editor, element, args);
            }
            "mast" => generate_antenna(editor, element),
            _ => {} // Unknown man_made type, ignore
        }
    }
}

/// Dispatch `man_made=*` MULTIPOLYGON relations.
///
/// Most `man_made` features are ways or nodes, but some — notably large
/// `man_made=pier` decks (e.g. Hudson River Park Pier 25/26) — are encoded
/// as `type=multipolygon` relations whose outer-role member ways are open
/// fragments of a single perimeter ring. The way-level `generate_man_made`
/// only matches `ProcessedElement::Way`, so these relations rendered nothing,
/// leaving lidar-elevated deck pads surfaced as bare sand/water at the fringe.
pub fn generate_man_made_from_relation(
    editor: &mut WorldEditor,
    rel: &ProcessedRelation,
    _args: &Args,
) {
    // Respect `layer`/`level` semantics exactly as the way-level path does:
    // skip anything routed below ground.
    if let Some(layer) = rel.tags.get("layer") {
        if layer.parse::<i32>().unwrap_or(0) < 0 {
            return;
        }
    }
    if let Some(level) = rel.tags.get("level") {
        if level.parse::<i32>().unwrap_or(0) < 0 {
            return;
        }
    }

    if rel.tags.get("man_made").map(|s| s.as_str()) == Some("pier") {
        generate_pier_from_relation(editor, rel);
    }
}

/// Render a `man_made=pier` multipolygon relation as a filled deck.
///
/// The outer-role member ways are assembled into closed rings using the same
/// pattern as `water_areas::generate_water_areas_from_relation`
/// (`merge_way_segments` + closed-ring verification), then each ring's interior
/// is flood-filled with the same pier deck (OAK_SLAB at ground+1) and support
/// pillars that the way-level `generate_pier` produces.
fn generate_pier_from_relation(editor: &mut WorldEditor, rel: &ProcessedRelation) {
    let mut outers: Vec<Vec<ProcessedNode>> = vec![];
    for mem in &rel.members {
        if mem.role == ProcessedMemberRole::Outer {
            outers.push(mem.way.nodes.clone());
        }
    }
    if outers.is_empty() {
        return;
    }

    // Chain the open perimeter fragments into closed rings.
    super::merge_way_segments(&mut outers);

    for ring in &outers {
        if ring.len() < 4 {
            continue;
        }
        // flood_fill_area rejects open polylines (requires first == last);
        // close the ring if merging left it within tolerance, mirroring the
        // water-area relation handling.
        let mut coords: Vec<(i32, i32)> = ring.iter().map(|n| (n.x, n.z)).collect();
        let first = coords[0];
        let last = *coords.last().unwrap();
        if first != last {
            let dx = (first.0 - last.0).abs();
            let dz = (first.1 - last.1).abs();
            if dx <= 1 && dz <= 1 {
                coords.push(first);
            } else {
                // Not closeable within tolerance — skip rather than fabricate
                // a long straight closing edge across the river.
                println!("Skipping pier relation {} due to invalid polygon", rel.id);
                continue;
            }
        }

        let filled = flood_fill_area(&coords, None);
        for &(x, z) in &filled {
            // Deck one block above the (lidar-elevated) ground, same as the
            // way-pier deck height.
            editor.set_block(OAK_SLAB, x, 1, z, None, None);
            // Sparse support pillars on a 4-block grid, dropped from the deck.
            if x % 4 == 0 && z % 4 == 0 {
                editor.set_block(OAK_LOG, x, 0, z, None, None);
            }
        }
    }
}

/// Generate a pier structure with OAK_SLAB planks and OAK_LOG support pillars
fn generate_pier(editor: &mut WorldEditor, element: &ProcessedElement) {
    if let ProcessedElement::Way(way) = element {
        let nodes = &way.nodes;
        if nodes.len() < 2 {
            return;
        }

        // Extract pier dimensions from tags
        let pier_width = element
            .tags()
            .get("width")
            .and_then(|w| w.parse::<i32>().ok())
            .unwrap_or(3); // Default 3 blocks wide

        let pier_height = 1; // Pier deck height above ground
        let support_spacing = 4; // Support pillars every 4 blocks

        // Generate the pier walkway using bresenham line algorithm
        for i in 0..nodes.len() - 1 {
            let start_node = &nodes[i];
            let end_node = &nodes[i + 1];

            let line_points =
                bresenham_line(start_node.x, 0, start_node.z, end_node.x, 0, end_node.z);

            for (index, (center_x, _y, center_z)) in line_points.iter().enumerate() {
                // Create pier deck (3 blocks wide)
                let half_width = pier_width / 2;
                for x in (center_x - half_width)..=(center_x + half_width) {
                    for z in (center_z - half_width)..=(center_z + half_width) {
                        editor.set_block(OAK_SLAB, x, pier_height, z, None, None);
                    }
                }

                // Add support pillars every few blocks
                if index % support_spacing == 0 {
                    let half_width = pier_width / 2;

                    // Place support pillars at the edges of the pier
                    let support_positions = [
                        (center_x - half_width, center_z), // Left side
                        (center_x + half_width, center_z), // Right side
                    ];

                    for (pillar_x, pillar_z) in support_positions {
                        // Support pillars going down from pier level
                        editor.set_block(OAK_LOG, pillar_x, 0, *pillar_z, None, None);
                    }
                }
            }
        }
    }
}

/// Generate an antenna/radio tower
fn generate_antenna(editor: &mut WorldEditor, element: &ProcessedElement) {
    if let Some(first_node) = element.nodes().next() {
        let x = first_node.x;
        let z = first_node.z;

        // Extract antenna configuration from tags
        let height = match element.tags().get("height") {
            Some(h) => h.parse::<i32>().unwrap_or(20).min(40), // Max 40 blocks
            None => match element.tags().get("tower:type").map(|s| s.as_str()) {
                Some("communication") => 20,
                Some("cellular") => 15,
                _ => 20,
            },
        };

        // Build the main tower pole
        editor.set_block(IRON_BLOCK, x, 3, z, None, None);
        for y in 4..height {
            editor.set_block(IRON_BARS, x, y, z, None, None);
        }

        // Add structural supports every 7 blocks
        for y in (7..height).step_by(7) {
            editor.set_block(IRON_BLOCK, x, y, z, Some(&[IRON_BARS]), None);
            let support_positions = [(1, 0), (-1, 0), (0, 1), (0, -1)];
            for (dx, dz) in support_positions {
                editor.set_block(IRON_BLOCK, x + dx, y, z + dz, None, None);
            }
        }

        // Equipment housing at base
        editor.fill_blocks(
            GRAY_CONCRETE,
            x - 1,
            1,
            z - 1,
            x + 1,
            2,
            z + 1,
            Some(&[GRAY_CONCRETE]),
            None,
        );
    }
}

/// Generate a chimney structure
fn generate_chimney(editor: &mut WorldEditor, element: &ProcessedElement) {
    if let Some(first_node) = element.nodes().next() {
        let x = first_node.x;
        let z = first_node.z;
        let height = 25;

        // Build 3x3 brick chimney with hole in the middle
        for y in 0..height {
            for dx in -1..=1 {
                for dz in -1..=1 {
                    // Skip center block to create hole
                    if dx == 0 && dz == 0 {
                        continue;
                    }
                    editor.set_block(BRICK, x + dx, y, z + dz, None, None);
                }
            }
        }
    }
}

/// Generate a water well structure
fn generate_water_well(editor: &mut WorldEditor, element: &ProcessedElement) {
    if let Some(first_node) = element.nodes().next() {
        let x = first_node.x;
        let z = first_node.z;

        // Build stone well structure (3x3 base with water in center)
        for dx in -1..=1 {
            for dz in -1..=1 {
                if dx == 0 && dz == 0 {
                    // Water in the center
                    editor.set_block(WATER, x, -1, z, None, None);
                    editor.set_block(WATER, x, 0, z, None, None);
                } else {
                    // Stone well walls
                    editor.set_block(STONE_BRICKS, x + dx, 0, z + dz, None, None);
                    editor.set_block(STONE_BRICKS, x + dx, 1, z + dz, None, None);
                }
            }
        }

        // Add wooden well frame structure
        editor.fill_blocks(OAK_LOG, x - 2, 1, z, x - 2, 4, z, None, None);
        editor.fill_blocks(OAK_LOG, x + 2, 1, z, x + 2, 4, z, None, None);

        // Crossbeam with pulley system
        editor.set_block(OAK_SLAB, x - 1, 5, z, None, None);
        editor.set_block(OAK_FENCE, x, 4, z, None, None);
        editor.set_block(OAK_SLAB, x, 5, z, None, None);
        editor.set_block(OAK_SLAB, x + 1, 5, z, None, None);

        // Bucket hanging from center
        editor.set_block(IRON_BLOCK, x, 3, z, None, None);
    }
}

/// Polygon-aware footprint summary for tank-style structures (water_tower,
/// silo, storage_tank). For node elements `nodes` is a single point and
/// `radius` defaults to a small fixed value.
struct TankFootprint {
    center_x: i32,
    center_z: i32,
    /// Approximate radius in blocks. For polygon ways this is half the
    /// average of width and length of the bounding box.
    radius: f64,
    /// Cells *inside* the polygon. For node elements this is just the
    /// single centre cell. Used to clip the cylinder so it never extends
    /// past the OSM-mapped outline.
    cells: HashSet<(i32, i32)>,
}

impl TankFootprint {
    fn from_element(element: &ProcessedElement) -> Self {
        let nodes: Vec<(i32, i32)> = element.nodes().map(|n| (n.x, n.z)).collect();
        if nodes.is_empty() {
            return Self {
                center_x: 0,
                center_z: 0,
                radius: 2.0,
                cells: HashSet::new(),
            };
        }

        if nodes.len() < 3 {
            // Single-node mapping - use a default 5×5 footprint around the
            // point so tank structures still have visible bulk even when
            // mapped as a POI.
            let (cx, cz) = nodes[0];
            let mut cells = HashSet::new();
            for dx in -2..=2 {
                for dz in -2..=2 {
                    cells.insert((cx + dx, cz + dz));
                }
            }
            return Self {
                center_x: cx,
                center_z: cz,
                radius: 2.5,
                cells,
            };
        }

        let (mut min_x, mut max_x) = (i32::MAX, i32::MIN);
        let (mut min_z, mut max_z) = (i32::MAX, i32::MIN);
        for &(x, z) in &nodes {
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_z = min_z.min(z);
            max_z = max_z.max(z);
        }
        let center_x = (min_x + max_x) / 2;
        let center_z = (min_z + max_z) / 2;
        let half_w = (max_x - min_x).max(1) as f64 / 2.0;
        let half_l = (max_z - min_z).max(1) as f64 / 2.0;
        let radius = (half_w + half_l) / 2.0;

        // Rasterize the polygon interior using a point-in-polygon test on
        // every cell of the bounding box. Tanks are small so this is cheap.
        let mut cells = HashSet::new();
        for x in min_x..=max_x {
            for z in min_z..=max_z {
                if point_in_polygon(x, z, &nodes) {
                    cells.insert((x, z));
                }
            }
        }
        Self {
            center_x,
            center_z,
            radius,
            cells,
        }
    }

    /// Iterates the cells of a filled disc of `disc_radius` centred on
    /// the footprint, clipped to the polygon `cells`.
    fn cells_in_disc(&self, disc_radius: f64) -> impl Iterator<Item = (i32, i32)> + '_ {
        let r2 = disc_radius * disc_radius;
        let r_int = disc_radius.ceil() as i32 + 1;
        let cx = self.center_x;
        let cz = self.center_z;
        (-r_int..=r_int)
            .flat_map(move |dx| (-r_int..=r_int).map(move |dz| (dx, dz)))
            .filter(move |(dx, dz)| ((*dx as f64).powi(2) + (*dz as f64).powi(2)) <= r2)
            .map(move |(dx, dz)| (cx + dx, cz + dz))
            .filter(|cell| self.cells.contains(cell))
    }
}

/// Ray-cast point-in-polygon test sampling the cell centre.
fn point_in_polygon(px: i32, pz: i32, polygon: &[(i32, i32)]) -> bool {
    let px = px as f64 + 0.5;
    let pz = pz as f64 + 0.5;
    let mut inside = false;
    let n = polygon.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (xi, zi) = polygon[i];
        let (xj, zj) = polygon[j];
        let zi = zi as f64;
        let zj = zj as f64;
        let xi = xi as f64;
        let xj = xj as f64;
        let intersect = ((zi > pz) != (zj > pz)) && (px < (xj - xi) * (pz - zi) / (zj - zi) + xi);
        if intersect {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Reads a building height from `height=*` (in metres / blocks) with a
/// caller-supplied default. Stripping a trailing 'm' keeps OSM values like
/// `height=18m` working.
fn read_height(element: &ProcessedElement, default: i32, scale_factor: f64) -> i32 {
    element
        .tags()
        .get("height")
        .and_then(|s| s.trim_end_matches('m').trim().parse::<f64>().ok())
        .map(|h| (h * scale_factor).round() as i32)
        .unwrap_or(default)
        .max(3)
}

/// Public entry point used by [buildings::generate_buildings] when a way
/// is tagged as a tank structure (`man_made=*` or `building=*` of
/// `water_tower` / `silo` / `storage_tank`). Dispatches to the right
/// renderer based on the most specific tag available.
pub fn generate_tank_structure(editor: &mut WorldEditor, element: &ProcessedElement, args: &Args) {
    // Skip relations and other elements that have no node geometry; the
    // renderers below would otherwise build at world origin (0, 0).
    if element.nodes().next().is_none() {
        return;
    }

    let pick_tank = |key: &str| {
        element
            .tags()
            .get(key)
            .map(|s| s.as_str())
            .filter(|k| matches!(*k, "water_tower" | "silo" | "storage_tank"))
    };
    let kind = pick_tank("man_made").or_else(|| pick_tank("building"));

    match kind {
        Some("water_tower") => generate_water_tower(editor, element, args),
        Some("silo") => generate_silo(editor, element, args),
        Some("storage_tank") => generate_storage_tank(editor, element, args),
        _ => {}
    }
}

/// Generate a water tower - a tall cylindrical / rectangular tank
/// elevated on legs. Polygon-aware: legs are placed at the polygon
/// corners (or 4 cardinal points for round mappings), and the tank
/// itself is a filled cylinder clipped to the polygon outline.
fn generate_water_tower(editor: &mut WorldEditor, element: &ProcessedElement, args: &Args) {
    let footprint = TankFootprint::from_element(element);
    let total_height = read_height(element, 20, args.scale);
    // Lower 60% is the supports, upper 40% is the tank itself.
    let support_height = (total_height as f64 * 0.6).round() as i32;
    let tank_height = total_height - support_height;
    if tank_height < 2 {
        return;
    }

    // --- Support structure ---
    // Legs at the 4 cardinal extremes of the footprint. We pick cells
    // that sit roughly at radius * 0.85 from centre so the legs frame
    // the structure without poking outside the polygon.
    let leg_offset = (footprint.radius * 0.85).max(1.0).round() as i32;
    let leg_positions: [(i32, i32); 4] = [
        (-leg_offset, 0),
        (leg_offset, 0),
        (0, -leg_offset),
        (0, leg_offset),
    ];
    for &(dx, dz) in &leg_positions {
        let lx = footprint.center_x + dx;
        let lz = footprint.center_z + dz;
        // Only place legs on cells inside the polygon (so weird-shaped
        // mapped polygons don't get legs floating in air).
        if !footprint.cells.contains(&(lx, lz)) {
            continue;
        }
        for y in 0..support_height {
            editor.set_block(IRON_BLOCK, lx, y, lz, None, None);
        }
    }

    // Cross-bracing every 5 blocks of height - gives the tower its
    // characteristic lattice silhouette. Bracing follows the polygon
    // outline (bresenham between consecutive nodes) at the tier height.
    if let ProcessedElement::Way(way) = element {
        for tier_y in (5..support_height).step_by(5) {
            let mut prev: Option<(i32, i32)> = None;
            for node in &way.nodes {
                if let Some((px, pz)) = prev {
                    let pts = bresenham_line(px, tier_y, pz, node.x, tier_y, node.z);
                    for (bx, by, bz) in pts {
                        editor.set_block(SMOOTH_STONE, bx, by, bz, None, None);
                    }
                }
                prev = Some((node.x, node.z));
            }
        }
    }

    // Central pipe / column down to the ground.
    for y in 0..support_height {
        editor.set_block(
            POLISHED_ANDESITE,
            footprint.center_x,
            y,
            footprint.center_z,
            None,
            None,
        );
    }

    // Tank sits at one absolute Y so it stays level on sloped terrain.
    let tank_base =
        editor.get_ground_level(footprint.center_x, footprint.center_z) + support_height;
    for y in tank_base..(tank_base + tank_height) {
        for (cx, cz) in footprint.cells_in_disc(footprint.radius) {
            editor.set_block_absolute(POLISHED_ANDESITE, cx, y, cz, None, None);
        }
    }
    let cap_y = tank_base + tank_height;
    for (cx, cz) in footprint.cells_in_disc(footprint.radius) {
        editor.set_block_absolute(SMOOTH_STONE_SLAB, cx, cap_y, cz, None, None);
    }
}

/// Generate a silo - a tall cylindrical narrow tower. Polygon-aware
/// filled cylinder running floor-to-cap. Material follows
/// `building:material=*` (cement/stone → smooth stone; metal → iron;
/// default smooth stone).
fn generate_silo(editor: &mut WorldEditor, element: &ProcessedElement, args: &Args) {
    let footprint = TankFootprint::from_element(element);
    let height = read_height(element, 25, args.scale);

    let material_tag = element
        .tags()
        .get("building:material")
        .or_else(|| element.tags().get("material"))
        .map(|s| s.to_lowercase());
    let body_block = match material_tag.as_deref() {
        Some("metal" | "steel" | "aluminium" | "aluminum" | "iron" | "tin") => IRON_BLOCK,
        Some("concrete" | "cement" | "reinforced_concrete") => GRAY_CONCRETE,
        _ => SMOOTH_STONE,
    };

    let base = editor.get_ground_level(footprint.center_x, footprint.center_z);
    for y in base..(base + height) {
        for (cx, cz) in footprint.cells_in_disc(footprint.radius) {
            editor.set_block_absolute(body_block, cx, y, cz, None, None);
        }
    }
    // Domed cap: small slab on top to suggest a rounded lid.
    for (cx, cz) in footprint.cells_in_disc(footprint.radius) {
        editor.set_block_absolute(SMOOTH_STONE_SLAB, cx, base + height, cz, None, None);
    }
}

/// Generate a storage tank - short squat cylinder. Material follows
/// `content=*` for a colour hint (water → light grey, oil → black,
/// gas/lng → white).
fn generate_storage_tank(editor: &mut WorldEditor, element: &ProcessedElement, args: &Args) {
    let footprint = TankFootprint::from_element(element);
    let default_h = ((footprint.radius * 1.2).round() as i32).max(6);
    let height =
        read_height(element, default_h, args.scale).min((footprint.radius * 1.5) as i32 + 4);

    let content = element.tags().get("content").map(|s| s.to_lowercase());
    let body_block = match content.as_deref() {
        Some("oil" | "fuel" | "diesel" | "petroleum" | "tar") => BLACK_TERRACOTTA,
        Some("gas" | "lng" | "methane" | "lpg") => WHITE_CONCRETE,
        Some("water" | "wastewater") => LIGHT_GRAY_CONCRETE,
        _ => SMOOTH_STONE,
    };

    let base = editor.get_ground_level(footprint.center_x, footprint.center_z);
    for y in base..(base + height) {
        for (cx, cz) in footprint.cells_in_disc(footprint.radius) {
            editor.set_block_absolute(body_block, cx, y, cz, None, None);
        }
    }
    // Flat lid.
    for (cx, cz) in footprint.cells_in_disc(footprint.radius) {
        editor.set_block_absolute(SMOOTH_STONE_SLAB, cx, base + height, cz, None, None);
    }
}

/// Returns true if the element is one of the tank-style structures
/// handled by [generate_tank_structure]. Used by the building dispatcher
/// to decide whether to short-circuit normal building generation.
pub fn is_tank_structure(way: &ProcessedWay) -> bool {
    matches!(
        way.tags.get("man_made").map(|s| s.as_str()),
        Some("water_tower" | "silo" | "storage_tank")
    ) || matches!(
        way.tags.get("building").map(|s| s.as_str()),
        Some("water_tower" | "silo" | "storage_tank")
    )
}

/// Generate man_made structures for node elements
pub fn generate_man_made_nodes(editor: &mut WorldEditor, node: &ProcessedNode, args: &Args) {
    if let Some(man_made_type) = node.tags.get("man_made") {
        let element = ProcessedElement::Node(node.clone());

        match man_made_type.as_str() {
            "antenna" => generate_antenna(editor, &element),
            "chimney" => generate_chimney(editor, &element),
            "water_well" => generate_water_well(editor, &element),
            "water_tower" | "silo" | "storage_tank" => {
                generate_tank_structure(editor, &element, args);
            }
            "mast" => generate_antenna(editor, &element),
            _ => {} // Unknown man_made type, ignore
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinate_system::cartesian::XZBBox;
    use crate::ground::Ground;
    use crate::osm_parser::ProcessedMember;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    const TW: usize = 17; // 17×17 world, blocks 0..=16, 1:1 grid↔world

    fn node(id: u64, x: i32, z: i32) -> ProcessedNode {
        ProcessedNode {
            id,
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

    fn test_args() -> Args {
        use clap::Parser;
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
            PathBuf::from("/tmp/arnis-man-made-test"),
            xzbbox,
            crate::coordinate_system::geographic::LLBBox::new(0.0, 0.0, 0.001, 0.001).unwrap(),
        );
        editor.set_ground(Arc::clone(ground));
        editor
    }

    /// A `man_made=pier` multipolygon relation whose square deck perimeter
    /// (blocks [2,14]²) is split across FOUR open outer-role member ways —
    /// exactly the Pier 25 encoding (4 outer fragments chaining into one ring).
    fn pier_relation(tags: HashMap<String, String>) -> ProcessedRelation {
        // Shared corner node IDs so adjacent fragments chain end-to-end.
        let c0 = node(100, 2, 2);
        let c1 = node(101, 14, 2);
        let c2 = node(102, 14, 14);
        let c3 = node(103, 2, 14);
        let frag = |id: u64, a: &ProcessedNode, b: &ProcessedNode| ProcessedMember {
            role: ProcessedMemberRole::Outer,
            way: Arc::new(ProcessedWay {
                id,
                nodes: vec![a.clone(), b.clone()],
                tags: HashMap::new(),
            }),
        };
        ProcessedRelation {
            id: 20784500,
            tags,
            members: vec![
                frag(1, &c0, &c1),
                frag(2, &c1, &c2),
                frag(3, &c2, &c3),
                frag(4, &c3, &c0),
            ],
        }
    }

    fn pier_tags() -> HashMap<String, String> {
        let mut tags = HashMap::new();
        tags.insert("man_made".to_string(), "pier".to_string());
        tags.insert("type".to_string(), "multipolygon".to_string());
        tags
    }

    /// Probe coordinates well inside the deck polygon interior.
    const INTERIOR: &[(i32, i32)] = &[(8, 8), (5, 5), (11, 11), (8, 5), (5, 11)];

    /// RED-first acceptance: a `man_made=pier` multipolygon relation must lay a
    /// deck (OAK_SLAB at ground+1) across its polygon interior. Before the fix
    /// the relation dispatched to the way-only `generate_pier` and rendered
    /// nothing here.
    #[test]
    fn pier_relation_renders_deck_blocks_inside_polygon() {
        let xzbbox = XZBBox::rect_from_xz_lengths(16.0, 16.0).unwrap();
        let ground = flat_ground();
        let mut editor = new_editor(&xzbbox, &ground);
        let rel = pier_relation(pier_tags());

        generate_man_made_from_relation(&mut editor, &rel, &test_args());

        for &(x, z) in INTERIOR {
            assert!(
                editor.check_for_block(x, 1, z, Some(&[OAK_SLAB])),
                "pier relation must lay an OAK_SLAB deck at interior ({x},{z})"
            );
        }
    }

    /// A pier relation routed below ground (`layer=-1`) must render nothing,
    /// matching the way-level `generate_man_made` layer guard.
    #[test]
    fn pier_relation_negative_layer_renders_nothing() {
        let xzbbox = XZBBox::rect_from_xz_lengths(16.0, 16.0).unwrap();
        let ground = flat_ground();
        let mut editor = new_editor(&xzbbox, &ground);
        let mut tags = pier_tags();
        tags.insert("layer".to_string(), "-1".to_string());
        let rel = pier_relation(tags);

        generate_man_made_from_relation(&mut editor, &rel, &test_args());

        for &(x, z) in INTERIOR {
            assert!(
                !editor.check_for_block(x, 1, z, Some(&[OAK_SLAB])),
                "negative-layer pier relation must not lay deck at ({x},{z})"
            );
        }
    }
}
