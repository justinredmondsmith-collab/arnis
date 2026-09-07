use crate::args::Args;
use crate::block_definitions::*;
use crate::bresenham::bresenham_line;
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
            "lighthouse" => place_lighthouse_way(editor, element),
            _ => {} // Unknown man_made type, ignore
        }
    }
}

/// Stamp the bundled lighthouse at the centroid of a lighthouse way/footprint.
fn place_lighthouse_way(editor: &mut WorldEditor, element: &ProcessedElement) {
    if let ProcessedElement::Way(way) = element {
        if way.nodes.is_empty() {
            return;
        }
        let (mut sx, mut sz) = (0i64, 0i64);
        for nd in &way.nodes {
            sx += nd.x as i64;
            sz += nd.z as i64;
        }
        let n = way.nodes.len() as i64;
        crate::structures::lighthouse::place(editor, (sx / n) as i32, (sz / n) as i32);
    }
}

/// Generate a pier structure with OAK_SLAB planks and OAK_LOG support pillars
fn generate_pier(editor: &mut WorldEditor, element: &ProcessedElement) {
    if let ProcessedElement::Relation(relation) = element {
        generate_pier_relation(editor, relation);
        return;
    }
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

/// Assemble exact-coordinate fragments before rasterization. Every open endpoint
/// must have exactly two incident fragments: dangling or branching outlines fail
/// closed rather than choosing a member-order-dependent artificial connection.
fn pier_rings(
    relation: &ProcessedRelation,
    role: ProcessedMemberRole,
) -> Option<Vec<geo::LineString<f64>>> {
    use std::collections::HashMap;
    let mut fragments: Vec<Vec<(i32, i32)>> = relation
        .members
        .iter()
        .filter(|member| member.role == role)
        .map(|member| member.way.nodes.iter().map(|n| (n.x, n.z)).collect())
        .collect();
    let mut endpoints: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (index, fragment) in fragments.iter_mut().enumerate() {
        fragment.dedup();
        if fragment.len() < 2 {
            return None;
        }
        if fragment.first() != fragment.last() {
            endpoints.entry(fragment[0]).or_default().push(index);
            endpoints.entry(*fragment.last()?).or_default().push(index);
        }
    }
    if endpoints.values().any(|incident| incident.len() != 2) {
        return None;
    }
    let mut used = vec![false; fragments.len()];
    let mut rings = Vec::new();
    for start in 0..fragments.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut ring = fragments[start].clone();
        while ring.first() != ring.last() {
            let endpoint = *ring.last()?;
            let next = *endpoints
                .get(&endpoint)?
                .iter()
                .find(|&&index| !used[index])?;
            used[next] = true;
            let fragment = &fragments[next];
            if fragment[0] == endpoint {
                ring.extend(fragment.iter().skip(1).copied());
            } else {
                ring.extend(fragment.iter().rev().skip(1).copied());
            }
        }
        if ring.len() < 4 {
            return None;
        }
        rings.push(geo::LineString::from(
            ring.into_iter()
                .map(|(x, z)| (f64::from(x), f64::from(z)))
                .collect::<Vec<_>>(),
        ));
    }
    Some(rings)
}

fn pier_polygon(relation: &ProcessedRelation) -> Option<geo::MultiPolygon<f64>> {
    use geo::{Contains, Validation};
    let outers = pier_rings(relation, ProcessedMemberRole::Outer)?;
    let inners = pier_rings(relation, ProcessedMemberRole::Inner)?;
    if outers.is_empty() {
        return None;
    }
    let mut polygons: Vec<_> = outers
        .into_iter()
        .map(|ring| geo::Polygon::new(ring, vec![]))
        .collect();
    if polygons.iter().any(|polygon| !polygon.is_valid()) {
        return None;
    }
    for inner in inners {
        let hole = geo::Polygon::new(inner.clone(), vec![]);
        if !hole.is_valid() {
            return None;
        }
        let owners: Vec<_> = polygons
            .iter()
            .enumerate()
            .filter(|(_, polygon)| {
                geo::Polygon::new(polygon.exterior().clone(), vec![]).contains(&hole)
            })
            .map(|(index, _)| index)
            .collect();
        if owners.len() != 1 {
            return None;
        }
        polygons[owners[0]].interiors_push(inner);
    }
    let polygon = geo::MultiPolygon::new(polygons);
    polygon.is_valid().then_some(polygon)
}

fn generate_pier_relation(editor: &mut WorldEditor, relation: &ProcessedRelation) {
    use geo::{BoundingRect, Contains};
    if relation.tags.get("type").map(String::as_str) != Some("multipolygon") {
        return;
    }
    let Some(polygon) = pier_polygon(relation) else {
        eprintln!(
            "Skipping pier relation {}: invalid or incomplete multipolygon rings",
            relation.id
        );
        return;
    };
    let Some(bounds) = polygon.bounding_rect() else {
        return;
    };
    let (min_x, min_z) = editor.get_min_coords();
    let (max_x, max_z) = editor.get_max_coords();
    // Never walk the unbounded OSM footprint or clip fragments before assembly.
    for x in min_x.max(bounds.min().x as i32)..=max_x.min(bounds.max().x as i32) {
        for z in min_z.max(bounds.min().y as i32)..=max_z.min(bounds.max().y as i32) {
            if polygon.contains(&geo::Point::new(f64::from(x) + 0.5, f64::from(z) + 0.5)) {
                editor.set_block(OAK_SLAB, x, 1, z, None, None);
                let (master_x, master_z) = editor.master_coordinates(x, z);
                if master_x.rem_euclid(4) == 0 && master_z.rem_euclid(4) == 0 {
                    editor.set_block(OAK_LOG, x, 0, z, None, None);
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
            "lighthouse" => crate::structures::lighthouse::place(editor, node.x, node.z),
            _ => {} // Unknown man_made type, ignore
        }
    }
}

#[cfg(test)]
mod pier_tests {
    use super::*;
    use crate::coordinate_system::{cartesian::XZBBox, geographic::LLBBox};
    use crate::osm_parser::{ProcessedMember, ProcessedMemberRole, ProcessedRelation};
    use clap::Parser;
    use std::{collections::HashMap, sync::Arc};

    fn member(points: &[(i32, i32)], inner: bool) -> ProcessedMember {
        ProcessedMember {
            role: if inner {
                ProcessedMemberRole::Inner
            } else {
                ProcessedMemberRole::Outer
            },
            way: Arc::new(ProcessedWay {
                id: 1,
                tags: HashMap::new(),
                nodes: points
                    .iter()
                    .map(|&(x, z)| ProcessedNode {
                        id: 0,
                        tags: HashMap::new(),
                        x,
                        z,
                    })
                    .collect(),
            }),
        }
    }
    fn relation() -> ProcessedRelation {
        ProcessedRelation {
            id: 42,
            tags: [
                ("type".into(), "multipolygon".into()),
                ("man_made".into(), "pier".into()),
            ]
            .into(),
            members: vec![
                member(&[(2, 2), (14, 2)], false),
                member(&[(14, 2), (14, 14)], false),
                member(&[(14, 14), (2, 14)], false),
                member(&[(2, 14), (2, 2)], false),
            ],
        }
    }
    fn render(rel: ProcessedRelation) -> HashMap<(i32, i32, i32), Block> {
        let bounds = XZBBox::rect_from_min_max(0, 0, 16, 16).unwrap();
        let ll = LLBBox::from_str("40,-74,40.01,-73.99").unwrap();
        let mut editor = WorldEditor::new("/dev/null/unused".into(), &bounds, ll);
        let args = Args::parse_from(["arnis"]);
        generate_man_made(&mut editor, &ProcessedElement::Relation(rel), &args);
        let mut blocks = HashMap::new();
        for x in 0..=16 {
            for z in 0..=16 {
                for y in 0..=1 {
                    if let Some(block) = editor.get_block_absolute(x, y, z) {
                        blocks.insert((x, y, z), block);
                    }
                }
            }
        }
        blocks
    }
    #[test]
    fn pier_four_fragments_fill_deck_and_coordinate_supports() {
        let blocks = render(relation());
        assert_eq!(blocks.get(&(7, 1, 7)), Some(&OAK_SLAB));
        assert_eq!(blocks.get(&(8, 0, 8)), Some(&OAK_LOG));
        assert!(!blocks.contains_key(&(7, 0, 7)));
        assert!(!blocks.contains_key(&(1, 1, 7)));
    }
    #[test]
    fn pier_inner_hole_and_member_reversal() {
        let mut rel = relation();
        rel.members
            .push(member(&[(6, 6), (10, 6), (10, 10), (6, 10), (6, 6)], true));
        let original = render(rel.clone());
        assert_eq!(original.get(&(4, 1, 4)), Some(&OAK_SLAB));
        assert!(!original.contains_key(&(8, 1, 8)));
        assert!(!original.contains_key(&(8, 0, 8)));
        rel.members.reverse();
        for mem in &mut rel.members {
            Arc::make_mut(&mut mem.way).nodes.reverse();
        }
        assert_eq!(render(rel), original);
    }
    #[test]
    fn pier_invalid_geometry_and_negative_tags_produce_no_blocks() {
        for tag in ["layer", "level"] {
            let mut rel = relation();
            rel.tags.insert(tag.into(), "-1".into());
            assert!(render(rel).is_empty());
        }
        let mut rel = relation();
        rel.members.pop();
        assert!(render(rel).is_empty());
        let mut rel = relation();
        rel.members.push(member(&[(6, 6), (10, 6), (10, 10)], true));
        assert!(render(rel).is_empty());
        let mut rel = relation();
        rel.members = vec![member(&[(2, 2), (14, 14), (2, 14), (14, 2), (2, 2)], false)];
        assert!(render(rel).is_empty());
    }
    #[test]
    fn pier_large_ring_is_clipped_to_editor_bounds() {
        let mut rel = relation();
        rel.members = vec![member(
            &[
                (-1_000_000, -1_000_000),
                (1_000_000, -1_000_000),
                (1_000_000, 1_000_000),
                (-1_000_000, 1_000_000),
                (-1_000_000, -1_000_000),
            ],
            false,
        )];
        assert_eq!(render(rel).get(&(8, 1, 8)), Some(&OAK_SLAB));
    }
    #[test]
    fn pier_overlapping_master_slices_keep_identical_decks_and_supports() {
        use crate::elevation::master_grid;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("master");
        let mut grid = master_grid::tests::fixture();
        grid.metadata.width = 32;
        grid.metadata.height = 32;
        grid.metadata.world_width = 32;
        grid.metadata.world_height = 32;
        grid.metadata.payload_bytes = 32 * 32 * 10;
        grid.elevation = vec![70.0; 32 * 32];
        grid.land_cover = vec![80; 32 * 32];
        grid.water_distance = vec![5; 32 * 32];
        grid.water_blend = vec![1.0; 32 * 32];
        master_grid::save(&path, &grid).unwrap();
        let mut outputs = Vec::new();
        for (col, row) in [(0, 0), (3, 5)] {
            let bounds = XZBBox::rect_from_min_max(0, 0, 16, 16).unwrap();
            let ll = LLBBox::from_str("40,-74,40.01,-73.99").unwrap();
            let ground = crate::ground::Ground::from_master_slice(
                master_grid::load_slice(&path, col, row, 17, 17).unwrap(),
            )
            .unwrap();
            let mut editor = WorldEditor::new("/dev/null/unused".into(), &bounds, ll);
            editor.set_ground(Arc::new(ground));
            editor.set_external_tile(true);
            let mut rel = relation();
            rel.members
                .push(member(&[(6, 6), (10, 6), (10, 10), (6, 10), (6, 6)], true));
            for member in &mut rel.members {
                for node in &mut Arc::make_mut(&mut member.way).nodes {
                    node.x -= col as i32;
                    node.z -= row as i32;
                }
            }
            if col != 0 {
                rel.members.reverse();
                for member in &mut rel.members {
                    Arc::make_mut(&mut member.way).nodes.reverse();
                }
            }
            generate_man_made(
                &mut editor,
                &ProcessedElement::Relation(rel),
                &Args::parse_from(["arnis"]),
            );
            let mut blocks = HashMap::new();
            for x in 3..=16 {
                for z in 5..=16 {
                    for y in 70..=71 {
                        if let Some(block) =
                            editor.get_block_absolute(x - col as i32, y, z - row as i32)
                        {
                            blocks.insert((x, y, z), block);
                        }
                    }
                }
            }
            assert_eq!(blocks.get(&(4, 70, 12)), Some(&OAK_LOG));
            assert!(!blocks.contains_key(&(8, 70, 8)));
            outputs.push(blocks);
        }
        assert_eq!(outputs[0], outputs[1]);
    }

    #[test]
    fn pier_way_preserves_stock_width_and_segment_supports() {
        let bounds = XZBBox::rect_from_min_max(0, 0, 16, 16).unwrap();
        let ll = LLBBox::from_str("40,-74,40.01,-73.99").unwrap();
        let mut editor = WorldEditor::new("/dev/null/unused".into(), &bounds, ll);
        let mut way = (*member(&[(3, 3), (9, 3)], false).way).clone();
        way.tags.insert("man_made".into(), "pier".into());
        generate_man_made(
            &mut editor,
            &ProcessedElement::Way(way),
            &Args::parse_from(["arnis"]),
        );
        assert_eq!(editor.get_block_absolute(2, 1, 2), Some(OAK_SLAB));
        assert_eq!(editor.get_block_absolute(2, 0, 3), Some(OAK_LOG));
        assert_eq!(editor.get_block_absolute(6, 0, 3), Some(OAK_LOG));
        assert_eq!(editor.get_block_absolute(5, 0, 3), None);
    }
}
