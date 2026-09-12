//! Exercise actual generation across the nested 512-block region threshold.
use super::*;
use crate::elevation::master_grid;
use clap::Parser;
use fastanvil::Chunk;

fn render(
    root: &std::path::Path,
    name: &str,
    grid_path: &std::path::Path,
    offset: u32,
    width: u32,
) -> PathBuf {
    let slice = master_grid::load_slice(grid_path, offset, 0, width, 16).unwrap();
    let bbox = LLBBox::new(slice.bbox[0], slice.bbox[1], slice.bbox[2], slice.bbox[3]).unwrap();
    let bounds = XZBBox::rect_from_min_max(0, 0, width as i32 - 1, 15).unwrap();
    let node = |id, x, z| ProcessedNode {
        id,
        tags: HashMap::new(),
        x: x - offset as i32,
        z,
    };
    let park = ProcessedElement::Way(ProcessedWay {
        id: 123,
        nodes: vec![
            node(1, 0, 0),
            node(2, 1024, 0),
            node(3, 1024, 15),
            node(4, 0, 15),
            node(1, 0, 0),
        ],
        tags: HashMap::from([("leisure".into(), "park".into())]),
    });
    let road = ProcessedElement::Way(ProcessedWay {
        id: 124,
        nodes: vec![node(5, 0, 8), node(6, 1024, 8)],
        tags: HashMap::from([
            ("highway".into(), "residential".into()),
            ("width".into(), "10".into()),
        ]),
    });
    let output = root.join(name);
    std::fs::create_dir(&output).unwrap();
    let args = Args::parse_from([
        "arnis",
        "--bbox=40,-74,40.01,-73.99",
        "--output-dir=/tmp/unused",
        "--no-3d",
        "--legacy-trees",
        "--canopy-height=false",
        "--map-item=false",
        "--signage=none",
        "--no-ores",
    ]);
    generate_world_with_options(
        vec![park, road],
        bounds,
        bbox,
        Ground::from_master_slice(slice).unwrap(),
        &args,
        GenerationOptions {
            path: output.clone(),
            format: WorldFormat::JavaAnvil,
            level_name: None,
            spawn_point: None,
            luanti_game: None,
            ground_level: 10,
        },
        Default::default(),
        Default::default(),
    )
    .unwrap();
    output
}
fn owned_blocks(path: &std::path::Path, start: i32, end: i32) -> Vec<String> {
    let mut result = vec![];
    for cx in start / 16..end / 16 {
        let file = std::fs::File::open(path.join(format!("region/r.{}.0.mca", cx / 32))).unwrap();
        let mut region = fastanvil::Region::from_stream(file).unwrap();
        let bytes = region.read_chunk((cx % 32) as usize, 0).unwrap().unwrap();
        let chunk = fastanvil::JavaChunk::from_bytes(&bytes).unwrap();
        for x in 0..16 {
            for z in 0..16 {
                for y in 8..40 {
                    result.push(
                        chunk
                            .block(x, y, z)
                            .map_or("minecraft:air", |b| b.name())
                            .to_owned(),
                    );
                }
            }
        }
    }
    result
}
#[test]
fn external_master_vegetation_matches_across_internal_region_threshold() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("master");
    let mut grid = master_grid::tests::fixture();
    grid.metadata.width = 1025;
    grid.metadata.world_width = 1025;
    grid.metadata.height = 16;
    grid.metadata.world_height = 16;
    grid.metadata.payload_bytes = 1025 * 16 * 10;
    grid.metadata.land_cover_cells_per_meter = 1.;
    grid.metadata.climate = "Temperate".into();
    grid.metadata.snow_threshold_y = 200;
    grid.elevation = vec![10.; 1025 * 16];
    grid.land_cover = vec![crate::land_cover::LC_GRASSLAND; 1025 * 16];
    grid.water_distance = vec![0; 1025 * 16];
    grid.water_blend = vec![0.; 1025 * 16];
    master_grid::save(&path, &grid).unwrap();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    let (whole, tile) = pool.install(|| {
        (
            render(root.path(), "whole", &path, 0, 1025),
            render(root.path(), "tile", &path, 384, 256),
        )
    });
    let a = owned_blocks(&whole, 448, 576);
    let b = owned_blocks(&tile, 64, 192);
    assert!(
        b.iter().any(|block| block == "minecraft:short_grass"),
        "park must exercise vegetation"
    );
    assert!(
        b.iter()
            .any(|block| block == "minecraft:gray_concrete_powder"),
        "road must exercise vegetation cleanup"
    );
    let differences: Vec<_> = a
        .iter()
        .zip(&b)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .take(20)
        .collect();
    assert!(
        differences.is_empty(),
        "whole vs external window differs at former internal halo: {differences:?}"
    );
}
