//! Integration ingress. Profile validation precedes all generation side effects.
use crate::args::{Args, GameMode, GenerationMode, SignageLevel};
use crate::tiler_contract::ContractError;
use clap::{CommandFactory, FromArgMatches};
use std::ffi::OsString;

fn incompatible(message: impl Into<String>) -> ContractError {
    ContractError {
        exit_code: 3,
        message: message.into(),
    }
}
fn io_error(error: impl std::fmt::Display) -> ContractError {
    ContractError {
        exit_code: 1,
        message: error.to_string(),
    }
}
fn profile_args(raw: &[OsString]) -> Result<Args, ContractError> {
    let matches = Args::command()
        .try_get_matches_from(std::iter::once(OsString::from("arnis")).chain(raw.iter().cloned()))
        .map_err(|e| ContractError {
            exit_code: 2,
            message: e.to_string(),
        })?;
    let mut args = Args::from_arg_matches(&matches).map_err(|e| ContractError {
        exit_code: 2,
        message: e.to_string(),
    })?;
    macro_rules! pin {
        ($field:ident, $value:expr) => {{
            let value = $value;
            if matches.value_source(stringify!($field))
                == Some(clap::parser::ValueSource::CommandLine)
                && args.$field != value
            {
                return Err(incompatible(concat!(
                    "Profile conflicts with ",
                    stringify!($field)
                )));
            }
            args.$field = value;
        }};
    }
    pin!(scale, 1.0);
    pin!(projection, crate::projection::ProjectionKind::Local);
    pin!(ground_level, 10);
    pin!(mode, GenerationMode::GeoTerrain);
    pin!(fillground, true);
    pin!(no_ores, true);
    pin!(legacy_trees, true);
    pin!(
        skip_railways,
        Some("subway".parse().expect("fixed railway class"))
    );
    pin!(max_tree_size, crate::trees::tree_library::TreeSize::Giant);
    pin!(canopy_height, false);
    pin!(overture, false);
    pin!(use_3d, false);
    pin!(interior, false);
    pin!(rotation, 0.0);
    pin!(disable_height_limit, false);
    pin!(aws_only_elevation, true);
    pin!(bake_lighting, false);
    pin!(map_preview, false);
    pin!(map_item, false);
    pin!(gamemode, GameMode::Survival);
    pin!(world_time, 6000);
    pin!(signage, SignageLevel::None);
    pin!(bedrock, false);
    pin!(luanti, false);
    pin!(debug, false);
    if args.bbox.is_none() || args.path.as_ref().is_none_or(|p| !p.is_absolute()) {
        return Err(ContractError {
            exit_code: 2,
            message: "Integration requires --bbox and an absolute --output-dir".into(),
        });
    }
    if args.spawn_lat.is_some()
        || args.spawn_lng.is_some()
        || args.save_json_file.is_some()
        || args.timeout.is_some()
    {
        return Err(incompatible(
            "Spawn, OSM export and custom timeouts are outside this profile",
        ));
    }
    crate::args::validate_args(&args).map_err(incompatible)?;
    Ok(args)
}

fn validate_export_destination(
    destination: &std::path::Path,
    manifest: &std::path::Path,
    sources: &crate::tiler_contract::AdmittedSources,
) -> Result<(), ContractError> {
    let parent = destination
        .parent()
        .ok_or_else(|| io_error("Export destination needs a parent"))?
        .canonicalize()
        .map_err(io_error)?;
    if !parent.is_dir() || destination.is_dir() {
        return Err(io_error("Invalid export destination"));
    }
    let resolved = if destination.exists() {
        destination.canonicalize().map_err(io_error)?
    } else {
        parent.join(
            destination
                .file_name()
                .ok_or_else(|| io_error("Export destination needs a filename"))?,
        )
    };
    if resolved == manifest.canonicalize().map_err(io_error)?
        || sources.entries.iter().any(|entry| entry.path == resolved)
    {
        return Err(io_error(
            "Export destination must not replace the source manifest or admitted source files",
        ));
    }
    Ok(())
}

// Set once at admitted CLI ingress; stock invocations retain provider defaults.
static SERIAL_PROVIDERS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub(crate) fn provider_threads(stock: usize) -> usize {
    if SERIAL_PROVIDERS.load(std::sync::atomic::Ordering::Relaxed) {
        1
    } else {
        stock
    }
}

pub(crate) fn run(
    request: crate::tiler_contract::Request,
    raw: &[OsString],
) -> Result<(), ContractError> {
    use crate::coordinate_system::{geographic::LLBBox, transformation::CoordTransformer};
    use crate::elevation::master_grid;
    use crate::ground::Ground;
    use crate::tiler_contract::{admit_sources, profile_hash, Action};
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let args = profile_args(raw)?;
    let bbox = args.bbox.expect("validated bbox");
    let output = args.path.as_ref().expect("validated path");
    let sources = admit_sources(&request.source_manifest)?;
    if let Action::Export { destination } = &request.action {
        validate_export_destination(destination, &request.source_manifest, &sources)?;
    }
    let osm_entry = sources
        .entries
        .iter()
        .find(|e| e.kind == "osm" && e.key == "master-osm")
        .ok_or_else(|| incompatible("Manifest must contain osm/master-osm"))?;
    if let Some(path) = &args.file {
        if std::fs::canonicalize(path).map_err(io_error)? != osm_entry.path {
            return Err(incompatible(
                "--file differs from the admitted master OSM source",
            ));
        }
    }
    // Check exactly the buffer consumed by the parser, including a bounded read.
    if osm_entry.size_bytes > 536_870_912 {
        return Err(incompatible(
            "OSM input exceeds 512 MiB profile admission limit",
        ));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&osm_entry.path)
        .map_err(io_error)?
        .take(osm_entry.size_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 != osm_entry.size_bytes
        || format!("{:x}", Sha256::digest(&bytes)) != osm_entry.sha256
    {
        return Err(io_error("OSM source changed after admission"));
    }
    let osm: crate::osm_parser::OsmData = serde_json::from_slice(&bytes).map_err(io_error)?;
    drop(bytes);
    let profile = profile_hash();
    match request.action {
        Action::Render {
            grid,
            col,
            row,
            width,
            height,
        } => {
            let convert = |v: usize| {
                u32::try_from(v).map_err(|_| incompatible("Grid coordinate exceeds u32"))
            };
            let tile = master_grid::load_slice(
                &grid,
                convert(col)?,
                convert(row)?,
                convert(width)?,
                convert(height)?,
            )
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::InvalidData {
                    incompatible(e.to_string())
                } else {
                    io_error(e)
                }
            })?;
            tile.validate_request(
                [
                    bbox.min().lat(),
                    bbox.min().lng(),
                    bbox.max().lat(),
                    bbox.max().lng(),
                ],
                args.scale,
                &profile,
                &sources.sha256,
            )
            .map_err(|e| incompatible(e.to_string()))?;
            if tile.metadata.requested_ground_level != args.ground_level
                || tile.metadata.min_ground_level != crate::ground::min_ground_level_for(&args)
                || tile.metadata.extended_max_y != crate::ground::extended_max_y_for(&args)
            {
                return Err(incompatible("Master vertical settings differ from profile"));
            }
            let m = &tile.metadata;
            let master_bbox = LLBBox::from_str(&format!(
                "{},{},{},{}",
                m.bbox[0], m.bbox[1], m.bbox[2], m.bbox[3]
            ))
            .map_err(incompatible)?;
            let (frame, bounds) = CoordTransformer::for_master_slice(
                &master_bbox,
                m.width,
                m.height,
                tile.col,
                tile.row,
                tile.width,
                tile.height,
            )
            .map_err(incompatible)?;
            let ground = Ground::from_master_slice(tile).map_err(io_error)?;
            // Never reuse a destination: failed attempts are evidence to inspect, not overwrite.
            if output.exists() {
                return Err(io_error(
                    "Tile output already exists; use a new disposable directory",
                ));
            }
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .build()
                .map_err(io_error)?;
            pool.install(|| {
                let (elements, bounds, suppression, parts) =
                    crate::osm_parser::parse_osm_data_with_frame(
                        osm, bbox, args.scale, frame, bounds,
                    );
                crate::world_editor::set_world_bounds(
                    crate::ground::extended_min_y_for(&args),
                    crate::ground::world_top_y_for(&args),
                );
                crate::world_editor::set_base_chunk_y(ground.base_level());
                crate::world_editor::set_terrain_floor_y(ground.base_level());
                std::fs::create_dir(output).map_err(io_error)?;
                crate::data_processing::generate_world_with_options(
                    elements,
                    bounds,
                    bbox,
                    ground,
                    &args,
                    crate::data_processing::GenerationOptions {
                        path: output.clone(),
                        format: crate::world_editor::WorldFormat::JavaAnvil,
                        level_name: None,
                        spawn_point: None,
                        luanti_game: None,
                        ground_level: args.ground_level,
                    },
                    suppression,
                    parts,
                )
                .map_err(io_error)?;
                Ok(())
            })
        }
        Action::Export { destination } => {
            let (world_w, world_h, w, h) = crate::elevation::compute_grid_dims(&bbox, args.scale);
            if (world_w, world_h) != (w, h) {
                return Err(incompatible("Downsampled masters are outside this profile"));
            }
            let (frame, bounds) = CoordTransformer::for_master_grid(
                &bbox,
                u32::try_from(w).map_err(io_error)?,
                u32::try_from(h).map_err(io_error)?,
            )
            .map_err(incompatible)?;
            SERIAL_PROVIDERS.store(true, std::sync::atomic::Ordering::Relaxed);
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .build()
                .map_err(io_error)?;
            pool.install(|| {
                let (elements, bounds, _, _) = crate::osm_parser::parse_osm_data_with_frame(
                    osm, bbox, args.scale, frame, bounds,
                );
                let mut ground = crate::ground::generate_ground_data(&args, bbox);
                ground.apply_osm_water_override(&elements, &bounds);
                ground.apply_osm_land_override(&elements, &bounds, args.scale);
                ground.apply_bridge_land_cover_repair(&elements, &bounds, args.scale);
                let grid = ground
                    .to_master_grid(&bbox, &args, &sources.sha256, &profile)
                    .map_err(io_error)?;
                master_grid::save(&destination, &grid).map_err(io_error)
            })
        }
    }
}

#[cfg(test)]
mod ingress_tests {
    use super::*;
    #[test]
    fn export_destination_cannot_replace_admitted_sources() {
        use crate::tiler_contract::{AdmittedSources, SourceEntry};
        let root = tempfile::tempdir().unwrap();
        let manifest = root.path().join("manifest.json");
        let osm = root.path().join("osm.json");
        std::fs::write(&manifest, b"manifest").unwrap();
        std::fs::write(&osm, b"osm").unwrap();
        let sources = AdmittedSources {
            sha256: String::new(),
            entries: vec![SourceEntry {
                kind: "osm".into(),
                key: "master-osm".into(),
                path: osm.clone(),
                sha256: String::new(),
                size_bytes: 3,
            }],
        };
        assert_eq!(
            validate_export_destination(&manifest, &manifest, &sources)
                .unwrap_err()
                .exit_code,
            1
        );
        assert_eq!(
            validate_export_destination(&osm, &manifest, &sources)
                .unwrap_err()
                .exit_code,
            1
        );
        #[cfg(unix)]
        {
            let alias = root.path().join("alias");
            std::os::unix::fs::symlink(&osm, &alias).unwrap();
            assert_eq!(
                validate_export_destination(&alias, &manifest, &sources)
                    .unwrap_err()
                    .exit_code,
                1
            );
        }
        let grid = root.path().join("master.grid");
        assert!(validate_export_destination(&grid, &manifest, &sources).is_ok());
        std::fs::write(&grid, b"previous grid").unwrap();
        assert!(validate_export_destination(&grid, &manifest, &sources).is_ok());
        assert_eq!(std::fs::read(&manifest).unwrap(), b"manifest");
        assert_eq!(std::fs::read(&osm).unwrap(), b"osm");
    }

    #[test]
    fn render_validates_grid_and_writes_only_regions() {
        use crate::elevation::master_grid;
        use crate::tiler_contract::{profile_hash, Action, Request};
        use sha2::{Digest, Sha256};
        let root = tempfile::tempdir().unwrap();
        let osm = br#"{"elements":[]}"#;
        std::fs::write(root.path().join("osm.json"), osm).unwrap();
        let manifest = serde_json::to_vec(&serde_json::json!({"schema_version":1,"profile_sha256":profile_hash(),"entries":[{"kind":"osm","key":"master-osm","path":"osm.json","sha256":format!("{:x}",Sha256::digest(osm)),"size_bytes":osm.len()}]})).unwrap();
        let manifest_path = root.path().join("sources.json");
        std::fs::write(&manifest_path, &manifest).unwrap();
        let mut grid = master_grid::tests::fixture();
        grid.metadata.profile_sha256 = profile_hash();
        grid.metadata.requested_ground_level = 10;
        grid.metadata.min_ground_level = 10;
        grid.metadata.extended_max_y = 2031;
        grid.metadata.source_manifest_sha256 = format!("{:x}", Sha256::digest(&manifest));
        let grid_path = root.path().join("master.grid");
        master_grid::save(&grid_path, &grid).unwrap();
        let bbox = grid.metadata.slice_bbox(0, 0, 2, 2).unwrap();
        let output = root.path().join("output");
        let argv: Vec<OsString> = vec![
            format!("--bbox={},{},{},{}", bbox[0], bbox[1], bbox[2], bbox[3]).into(),
            format!("--output-dir={}", output.display()).into(),
        ];
        let request = Request {
            source_manifest: manifest_path,
            action: Action::Render {
                grid: grid_path.clone(),
                col: 0,
                row: 0,
                width: 2,
                height: 2,
            },
        };
        // Corruption anywhere in the master must fail before creating output.
        let original = std::fs::read(&grid_path).unwrap();
        let mut bad = original.clone();
        let last = bad.len() - 1;
        bad[last] ^= 1;
        std::fs::write(&grid_path, bad).unwrap();
        assert_eq!(run(request.clone(), &argv).unwrap_err().exit_code, 1);
        assert!(!output.exists());
        let mut unsupported = original.clone();
        unsupported[8] = b'1';
        std::fs::write(&grid_path, unsupported).unwrap();
        assert_eq!(run(request.clone(), &argv).unwrap_err().exit_code, 3);
        assert!(!output.exists());
        std::fs::write(&grid_path, original).unwrap();
        let mut outside = request.clone();
        if let Action::Render { col, .. } = &mut outside.action {
            *col = 4096;
        }
        assert_eq!(run(outside, &argv).unwrap_err().exit_code, 3);
        assert!(!output.exists());
        let destination = root.path().join("too-large.grid");
        let exporting = Request {
            source_manifest: request.source_manifest.clone(),
            action: Action::Export {
                destination: destination.clone(),
            },
        };
        let enormous: Vec<OsString> = vec![
            "--bbox=39,-75,41,-73".into(),
            format!("--output-dir={}", output.display()).into(),
        ];
        assert_eq!(run(exporting, &enormous).unwrap_err().exit_code, 3);
        assert!(!destination.exists() && !output.exists());
        run(request.clone(), &argv).unwrap();
        assert!(std::fs::read_dir(output.join("region"))
            .unwrap()
            .next()
            .is_some());
        assert_eq!(std::fs::read_dir(&output).unwrap().count(), 1);
        assert!(
            run(request, &argv).is_err(),
            "must never overwrite existing output"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_makes_defaults_explicit_and_rejects_conflicts() {
        let args = profile_args(&[
            "--bbox=40,-74,40.01,-73.99".into(),
            "--output-dir=/tmp/unused".into(),
        ])
        .unwrap();
        assert!(!args.map_item);
        assert!(!args.canopy_height);
        assert!(args.legacy_trees && args.no_ores && args.fillground);
        assert_eq!(args.ground_level, 10);
        assert!(profile_args(&[
            "--bbox=40,-74,40.01,-73.99".into(),
            "--output-dir=/tmp/unused".into(),
            "--map-item=true".into()
        ])
        .is_err());
        assert!(profile_args(&[
            "--bbox=40,-74,40.01,-73.99".into(),
            "--output-dir=/tmp/unused".into(),
            "--mode=geo-only".into()
        ])
        .is_err());
    }
}
