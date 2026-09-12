//! Conservative geographic OSM areas. Never use clipped renderer rings or close gaps.
use geo::{Contains, Intersects, LineString, MultiPolygon, Point, Polygon, Validation};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

#[derive(Deserialize)]
struct Document {
    elements: Vec<Element>,
    remark: Option<serde_json::Value>,
}
#[derive(Deserialize)]
struct Element {
    #[serde(rename = "type")]
    kind: String,
    id: i64,
    lat: Option<f64>,
    lon: Option<f64>,
    nodes: Option<Vec<i64>>,
    members: Option<Vec<Member>>,
    #[serde(default)]
    tags: HashMap<String, String>,
}
#[derive(Deserialize)]
struct Member {
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "ref")]
    id: i64,
    role: String,
}
fn water(e: &Element) -> bool {
    matches!(
        e.tags.get("natural").map(String::as_str),
        Some("water" | "bay")
    )
}
fn ring(ids: &[i64], nodes: &HashMap<i64, &Element>) -> Option<LineString> {
    if ids.len() < 4 || ids.first() != ids.last() {
        return None;
    }
    let coordinates: Option<Vec<_>> = ids
        .iter()
        .map(|id| {
            let n = nodes.get(id)?;
            let (lon, lat) = (n.lon?, n.lat?);
            (lon.is_finite()
                && lat.is_finite()
                && (-180. ..=180.).contains(&lon)
                && (-90. ..=90.).contains(&lat))
            .then_some((lon, lat))
        })
        .collect();
    let line = LineString::from(coordinates?);
    Polygon::new(line.clone(), vec![]).check_validation().ok()?;
    Some(line)
}
fn rings(mut paths: Vec<Vec<i64>>, nodes: &HashMap<i64, &Element>) -> Option<Vec<LineString>> {
    // A degree other than two has either a gap or an ambiguous continuation.
    let mut degree = HashMap::new();
    for path in &paths {
        if path.len() < 2 {
            return None;
        }
        for id in [path.first()?, path.last()?] {
            *degree.entry(*id).or_insert(0) += 1;
        }
    }
    if degree.values().any(|&n| n != 2) {
        return None;
    }
    let mut result = vec![];
    while let Some(mut path) = paths.pop() {
        while path.first() != path.last() {
            let end = path.last()?;
            let index = paths
                .iter()
                .position(|p| p.first() == Some(end) || p.last() == Some(end))?;
            let mut next = paths.swap_remove(index);
            if next.last() == Some(end) {
                next.reverse();
            }
            path.extend(next.into_iter().skip(1));
        }
        result.push(ring(&path, nodes)?);
    }
    Some(result)
}
fn relation(
    e: &Element,
    ways: &HashMap<i64, &Element>,
    nodes: &HashMap<i64, &Element>,
) -> Option<Vec<Polygon>> {
    if e.tags.get("type").map(String::as_str) != Some("multipolygon") {
        return None;
    }
    let mut outer = vec![];
    let mut inner = vec![];
    let mut seen = HashSet::new();
    for m in e.members.as_ref()? {
        if m.kind != "way" || !seen.insert(m.id) {
            return None;
        }
        let path = ways.get(&m.id)?.nodes.as_ref()?.clone();
        match m.role.as_str() {
            "outer" => outer.push(path),
            "inner" => inner.push(path),
            _ => return None,
        }
    }
    let mut polygons: Vec<_> = rings(outer, nodes)?
        .into_iter()
        .map(|r| Polygon::new(r, vec![]))
        .collect();
    if polygons.is_empty() {
        return None;
    }
    for hole in rings(inner, nodes)? {
        let hp = Polygon::new(hole.clone(), vec![]);
        let owners: Vec<_> = polygons
            .iter()
            .enumerate()
            .filter_map(|(i, p)| (p.contains(&hp) && !p.exterior().intersects(&hole)).then_some(i))
            .collect();
        if owners.len() != 1 {
            return None;
        }
        polygons[owners[0]].interiors_push(hole);
    }
    MultiPolygon(polygons.clone()).check_validation().ok()?;
    Some(polygons)
}

pub(super) struct Corroboration {
    ocean: MultiPolygon,
    mapped: Vec<Polygon>,
}
impl Corroboration {
    pub(super) fn from_authenticated(bytes: &[u8], ocean: MultiPolygon) -> Result<Self, String> {
        let doc: Document =
            serde_json::from_slice(bytes).map_err(|e| format!("Invalid corroboration OSM: {e}"))?;
        if doc.remark.is_some() {
            return Err("OSM server remark rejected for corroboration".into());
        }
        let mut seen = HashSet::new();
        for e in &doc.elements {
            if !seen.insert((e.kind.as_str(), e.id)) {
                return Err("Duplicate OSM element ID in corroboration input".into());
            }
        }
        let nodes: HashMap<_, _> = doc
            .elements
            .iter()
            .filter(|e| e.kind == "node")
            .map(|e| (e.id, e))
            .collect();
        let ways: HashMap<_, _> = doc
            .elements
            .iter()
            .filter(|e| e.kind == "way")
            .map(|e| (e.id, e))
            .collect();
        let candidates: Vec<_> = doc
            .elements
            .iter()
            .filter(|e| e.kind == "relation" && water(e))
            .collect();
        // Suppress member ways even when a relation is incomplete or unsupported.
        let suppressed: HashSet<_> = candidates
            .iter()
            .flat_map(|e| e.members.iter().flatten())
            .filter(|m| m.kind == "way")
            .map(|m| m.id)
            .collect();
        let mut mapped = vec![];
        for e in candidates {
            if let Some(polygons) = relation(e, &ways, &nodes) {
                mapped.extend(polygons);
            }
        }
        for e in ways
            .values()
            .filter(|e| water(e) && !suppressed.contains(&e.id))
        {
            if let Some(line) = e.nodes.as_ref().and_then(|p| ring(p, &nodes)) {
                mapped.push(Polygon::new(line, vec![]));
            }
        }
        Ok(Self { ocean, mapped })
    }
    pub(super) fn covers(&self, lon: f64, lat: f64) -> bool {
        let point = Point::new(lon, lat);
        // Exclude mapped boundaries conservatively, including hole boundaries.
        self.ocean.intersects(&point) && self.mapped.iter().any(|p| p.contains(&point))
    }
}
