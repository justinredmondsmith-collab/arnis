//! Pure validation of the external tiler's integration controls.
//! Read environment once at CLI ingress; downstream code receives this typed request.
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const ABI: &str = "1";
pub const PROFILE: &str = "nyc-conservative-v1";
pub const CONTROL_NAMES: [&str; 9] = [
    "ARNIS_TILER_ABI",
    "ARNIS_TILER_PROFILE",
    "ARNIS_TILER_SOURCE_MANIFEST",
    "ARNIS_SAVE_ELEVATION_GRID",
    "ARNIS_FETCH_ONLY",
    "ARNIS_TILED_RENDER",
    "ARNIS_USE_ELEVATION_GRID",
    "ARNIS_TILE_MASTER_OFFSET",
    "ARNIS_TILE_OVERRIDE_DIMS",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Export {
        destination: PathBuf,
    },
    Render {
        grid: PathBuf,
        col: usize,
        row: usize,
        width: usize,
        height: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub source_manifest: PathBuf,
    pub action: Action,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractError {
    pub exit_code: i32,
    pub message: String,
}
impl std::fmt::Display for ContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}
impl std::error::Error for ContractError {}
fn malformed(message: impl Into<String>) -> ContractError {
    ContractError {
        exit_code: 2,
        message: message.into(),
    }
}
fn incompatible(message: impl Into<String>) -> ContractError {
    ContractError {
        exit_code: 3,
        message: message.into(),
    }
}
fn required<'a>(env: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, ContractError> {
    env.get(key)
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| malformed(format!("Missing required integration control {key}")))
}
fn absolute_path(env: &BTreeMap<String, String>, key: &str) -> Result<PathBuf, ContractError> {
    let p = PathBuf::from(required(env, key)?);
    if !p.is_absolute() {
        return Err(malformed(format!("{key} must be an absolute path")));
    }
    Ok(p)
}
fn pair(value: &str, key: &str) -> Result<(usize, usize), ContractError> {
    let (a, b) = value
        .split_once(',')
        .ok_or_else(|| malformed(format!("{key} needs two decimal integers")))?;
    let parse = |s: &str| {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(malformed(format!(
                "{key} requires unsigned decimal integers without whitespace"
            )));
        }
        s.parse::<usize>()
            .map_err(|_| malformed(format!("{key} integer overflow")))
    };
    Ok((parse(a)?, parse(b)?))
}

pub fn parse_controls(env: &BTreeMap<String, String>) -> Result<Option<Request>, ContractError> {
    if !CONTROL_NAMES.iter().any(|k| env.contains_key(*k)) {
        return Ok(None);
    }
    if required(env, "ARNIS_TILER_ABI")? != ABI {
        return Err(incompatible("Unsupported ARNIS_TILER_ABI"));
    }
    if required(env, "ARNIS_TILER_PROFILE")? != PROFILE {
        return Err(incompatible("Unsupported ARNIS_TILER_PROFILE"));
    }
    let source_manifest = absolute_path(env, "ARNIS_TILER_SOURCE_MANIFEST")?;
    let exporting = ["ARNIS_SAVE_ELEVATION_GRID", "ARNIS_FETCH_ONLY"]
        .iter()
        .any(|k| env.contains_key(*k));
    let rendering = [
        "ARNIS_TILED_RENDER",
        "ARNIS_USE_ELEVATION_GRID",
        "ARNIS_TILE_MASTER_OFFSET",
        "ARNIS_TILE_OVERRIDE_DIMS",
    ]
    .iter()
    .any(|k| env.contains_key(*k));
    let action = match (exporting, rendering) {
        (true, false) => {
            if required(env, "ARNIS_FETCH_ONLY")? != "1" {
                return Err(malformed("ARNIS_FETCH_ONLY must be 1"));
            }
            Action::Export {
                destination: absolute_path(env, "ARNIS_SAVE_ELEVATION_GRID")?,
            }
        }
        (false, true) => {
            if required(env, "ARNIS_TILED_RENDER")? != "1" {
                return Err(malformed("ARNIS_TILED_RENDER must be 1"));
            }
            let grid = absolute_path(env, "ARNIS_USE_ELEVATION_GRID")?;
            let (col, row) = pair(
                required(env, "ARNIS_TILE_MASTER_OFFSET")?,
                "ARNIS_TILE_MASTER_OFFSET",
            )?;
            let (width, height) = pair(
                required(env, "ARNIS_TILE_OVERRIDE_DIMS")?,
                "ARNIS_TILE_OVERRIDE_DIMS",
            )?;
            if !(2..=4096).contains(&width) || !(2..=4096).contains(&height) {
                return Err(incompatible(
                    "Tile dimensions must be between 2 and 4096 cells",
                ));
            }
            col.checked_add(width)
                .and_then(|_| row.checked_add(height))
                .ok_or_else(|| malformed("Tile offset plus dimensions overflows"))?;
            Action::Render {
                grid,
                col,
                row,
                width,
                height,
            }
        }
        _ => {
            return Err(malformed(
                "Exactly one complete export or render control group is required",
            ))
        }
    };
    Ok(Some(Request {
        source_manifest,
        action,
    }))
}

pub fn controls_from_environment() -> Result<Option<Request>, ContractError> {
    let mut env = BTreeMap::new();
    for key in CONTROL_NAMES {
        if let Some(value) = std::env::var_os(key) {
            env.insert(
                key.to_string(),
                value
                    .into_string()
                    .map_err(|_| malformed(format!("{key} is not valid UTF-8")))?,
            );
        }
    }
    parse_controls(&env)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn export_env() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("ARNIS_TILER_ABI".into(), "1".into()),
            ("ARNIS_TILER_PROFILE".into(), "nyc-conservative-v1".into()),
            (
                "ARNIS_TILER_SOURCE_MANIFEST".into(),
                "/tmp/sources.json".into(),
            ),
            (
                "ARNIS_SAVE_ELEVATION_GRID".into(),
                "/tmp/master.grid".into(),
            ),
            ("ARNIS_FETCH_ONLY".into(), "1".into()),
        ])
    }

    #[test]
    fn no_controls_is_stock() {
        assert!(parse_controls(&BTreeMap::new()).unwrap().is_none());
    }

    #[test]
    fn complete_export_is_typed_without_io() {
        let r = parse_controls(&export_env()).unwrap().unwrap();
        assert!(matches!(r.action, Action::Export { .. }));
        assert_eq!(
            r.source_manifest,
            std::path::PathBuf::from("/tmp/sources.json")
        );
    }

    #[test]
    fn partial_controls_and_mixed_actions_fail() {
        let mut e = export_env();
        e.remove("ARNIS_TILER_ABI");
        assert_eq!(parse_controls(&e).unwrap_err().exit_code, 2);
        e = export_env();
        e.insert("ARNIS_USE_ELEVATION_GRID".into(), "/tmp/old.grid".into());
        assert_eq!(parse_controls(&e).unwrap_err().exit_code, 2);
        e = export_env();
        e.remove("ARNIS_FETCH_ONLY");
        assert_eq!(parse_controls(&e).unwrap_err().exit_code, 2);
    }

    #[test]
    fn unknown_abi_and_profile_are_incompatible() {
        let mut e = export_env();
        e.insert("ARNIS_TILER_ABI".into(), "2".into());
        assert_eq!(parse_controls(&e).unwrap_err().exit_code, 3);
        e = export_env();
        e.insert("ARNIS_TILER_PROFILE".into(), "flat".into());
        assert_eq!(parse_controls(&e).unwrap_err().exit_code, 3);
    }

    #[test]
    fn render_offsets_are_strict_and_bounded() {
        let mut e = export_env();
        e.remove("ARNIS_SAVE_ELEVATION_GRID");
        e.remove("ARNIS_FETCH_ONLY");
        e.insert("ARNIS_TILED_RENDER".into(), "1".into());
        e.insert("ARNIS_USE_ELEVATION_GRID".into(), "/tmp/grid".into());
        e.insert("ARNIS_TILE_MASTER_OFFSET".into(), "5,7".into());
        e.insert("ARNIS_TILE_OVERRIDE_DIMS".into(), "32,64".into());
        assert!(matches!(
            parse_controls(&e).unwrap().unwrap().action,
            Action::Render {
                col: 5,
                row: 7,
                width: 32,
                height: 64,
                ..
            }
        ));
        for bad in [
            "-1,2",
            "+1,2",
            "1, 2",
            "1,2,3",
            "1",
            "18446744073709551616,0",
        ] {
            e.insert("ARNIS_TILE_MASTER_OFFSET".into(), bad.into());
            assert!(parse_controls(&e).is_err(), "accepted {bad}");
        }
        e.insert("ARNIS_TILE_MASTER_OFFSET".into(), "0,0".into());
        for bad in ["1,32", "4097,32", "0,0"] {
            e.insert("ARNIS_TILE_OVERRIDE_DIMS".into(), bad.into());
            assert!(parse_controls(&e).is_err(), "accepted {bad}");
        }
    }
}

#[cfg(test)]
mod capability_tests {
    use super::*;
    #[test]
    fn capability_command_is_standalone() {
        assert!(!capability_command(&[]).unwrap());
        assert_eq!(
            capability_command(&["--tiler-capabilities=true".into()])
                .unwrap_err()
                .exit_code,
            2
        );
        assert!(capability_command(&["--tiler-capabilities".into()]).unwrap());
        assert_eq!(
            capability_command(&["--tiler-capabilities".into(), "--bbox=1,2,3,4".into()])
                .unwrap_err()
                .exit_code,
            2
        );
    }
    #[test]
    fn capability_report_has_exact_source_and_no_unimplemented_claims() {
        let report = capability_report();
        assert_eq!(
            report["upstream"]["commit"],
            "3918513acb4e5e9ef4332418531a7c444d2b5acf"
        );
        assert_eq!(report["schema_version"], 1);
        assert_eq!(report["build"]["commit"].as_str().unwrap().len(), 40);
        assert!(report["capabilities"].as_array().unwrap().is_empty());
    }
}

pub fn capability_command(args: &[std::ffi::OsString]) -> Result<bool, ContractError> {
    if args.iter().any(|a| {
        a.to_str()
            .is_some_and(|s| s.starts_with("--tiler-capabilities="))
    }) {
        return Err(malformed("--tiler-capabilities takes no value"));
    }
    let requested = args.iter().any(|a| a == "--tiler-capabilities");
    if requested && args.len() != 1 {
        return Err(malformed("--tiler-capabilities is a standalone command"));
    }
    Ok(requested)
}

pub fn capability_report() -> serde_json::Value {
    let mut report: serde_json::Value = serde_json::from_str(include_str!(
        "../docs/contracts/tiler-capability-template.json"
    ))
    .expect("embedded capability template must be valid JSON");
    report["build"] = serde_json::json!({
        "commit": env!("ARNIS_SOURCE_COMMIT"),
        "dirty": env!("ARNIS_SOURCE_DIRTY") == "true",
        "target": env!("ARNIS_BUILD_TARGET"),
        "rustc": env!("ARNIS_BUILD_RUSTC"),
    });
    // A partial development build must not pass the consumer's full capability schema.
    report["capabilities"] = serde_json::json!([]);
    report
}

#[cfg(test)]
mod source_tests {
    use super::*;
    #[test]
    fn source_manifest_validates_bytes_and_rejects_escape() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("osm.json"), b"{}").unwrap();
        let path = dir.path().join("sources.json");
        let mut entry = serde_json::json!({"schema_version":1,"profile_sha256":profile_hash(),"entries":[{"kind":"osm","key":"master-osm","path":"osm.json","sha256":"44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a","size_bytes":2}]});
        std::fs::write(&path, serde_json::to_vec(&entry).unwrap()).unwrap();
        let admitted = admit_sources(&path).unwrap();
        assert_eq!(admitted.entries.len(), 1);
        assert_eq!(admitted.sha256.len(), 64);
        std::fs::write(dir.path().join("osm.json"), b"[]").unwrap();
        assert!(admit_sources(&path).is_err());
        entry["entries"][0]["path"] = "../outside.json".into();
        std::fs::write(&path, serde_json::to_vec(&entry).unwrap()).unwrap();
        assert!(admit_sources(&path).is_err());
    }
    #[test]
    fn source_manifest_rejects_duplicate_roles_and_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("osm.json"), b"{}").unwrap();
        let path = dir.path().join("sources.json");
        let e = serde_json::json!({"kind":"osm","key":"master-osm","path":"osm.json","sha256":"44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a","size_bytes":2});
        let manifest = serde_json::json!({"schema_version":1,"profile_sha256":profile_hash(),"entries":[e.clone(),e]});
        std::fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(admit_sources(&path).is_err());
        std::fs::write(
            &path,
            b"{\"schema_version\":1,\"schema_version\":1,\"profile_sha256\":\"x\",\"entries\":[]}",
        )
        .unwrap();
        assert!(admit_sources(&path).is_err());
    }
}

#[derive(serde::Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct SourceManifest {
    schema_version: u32,
    profile_sha256: String,
    entries: Vec<SourceEntry>,
}
#[derive(serde::Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceEntry {
    pub kind: String,
    pub key: String,
    pub path: PathBuf,
    pub sha256: String,
    pub size_bytes: u64,
}
#[derive(Debug)]
pub(crate) struct AdmittedSources {
    pub sha256: String,
    pub entries: Vec<SourceEntry>,
}

impl AdmittedSources {
    /// Read only a bounded response, then authenticate the exact buffer consumed.
    pub(crate) fn resolve(&self, kind: &str, key: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
        use sha2::{Digest, Sha256};
        use std::io::Read;
        let entry = self
            .entries
            .iter()
            .find(|e| e.kind == kind && e.key == key)
            .ok_or_else(|| format!("Unlisted frozen source {kind}:{key}"))?;
        if entry.size_bytes > max_bytes || entry.size_bytes == u64::MAX {
            return Err(format!(
                "Frozen source exceeds response bound: {kind}:{key}"
            ));
        }
        let file = std::fs::File::open(&entry.path).map_err(|e| e.to_string())?;
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("Frozen source is not a regular file".into());
        }
        let mut bytes = Vec::new();
        file.take(entry.size_bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 != entry.size_bytes
            || format!("{:x}", Sha256::digest(&bytes)) != entry.sha256
        {
            return Err(format!(
                "Frozen source size/checksum mismatch: {kind}:{key}"
            ));
        }
        Ok(bytes)
    }
}

pub(crate) fn profile_hash() -> String {
    use sha2::{Digest, Sha256};
    // This profile is a flat JSON object; BTreeMap explicitly fixes key ordering
    // even if another dependency enables serde_json's preserve_order feature.
    let value: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(include_str!("../docs/contracts/tiler-profile.json"))
            .expect("embedded profile must be valid JSON");
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&value).expect("serializable profile"))
    )
}

pub(crate) fn admit_sources(path: &std::path::Path) -> Result<AdmittedSources, ContractError> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let io_error = |e: std::io::Error| ContractError {
        exit_code: 1,
        message: format!("Source manifest input: {e}"),
    };
    let root = path
        .parent()
        .ok_or_else(|| malformed("source manifest needs a parent"))?
        .canonicalize()
        .map_err(io_error)?;
    let mut input = std::fs::File::open(path).map_err(io_error)?.take(1_048_577);
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes).map_err(io_error)?;
    if bytes.len() > 1_048_576 {
        return Err(incompatible("Source manifest exceeds 1 MiB"));
    }
    let mut manifest: SourceManifest = serde_json::from_slice(&bytes)
        .map_err(|e| malformed(format!("Invalid source manifest: {e}")))?;
    if manifest.schema_version != 1 || manifest.profile_sha256 != profile_hash() {
        return Err(incompatible("Source manifest version/profile mismatch"));
    }
    if manifest.entries.is_empty() {
        return Err(malformed("Source manifest cannot be empty"));
    }
    let mut keys = std::collections::BTreeSet::new();
    for entry in &mut manifest.entries {
        if ![
            "osm",
            "elevation",
            "land_cover",
            "climate",
            "legacy_tree_asset",
            "water_classification",
        ]
        .contains(&entry.kind.as_str())
            || (entry.kind == "water_classification" && entry.key != "master-water-classification")
            || entry.key.is_empty()
            || !keys.insert((entry.kind.clone(), entry.key.clone()))
        {
            return Err(malformed("Invalid or duplicate source manifest kind/key"));
        }
        if entry.sha256.len() != 64
            || !entry
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(malformed("Invalid source SHA-256"));
        }
        if entry.path.as_os_str().is_empty()
            || entry.path.is_absolute()
            || entry
                .path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(malformed(
                "Source paths must stay relative to manifest root",
            ));
        }
        let resolved = root.join(&entry.path).canonicalize().map_err(io_error)?;
        if !resolved.starts_with(&root) {
            return Err(malformed("Source symlink escapes manifest root"));
        }
        let mut file = std::fs::File::open(&resolved).map_err(io_error)?;
        let metadata = file.metadata().map_err(io_error)?;
        if !metadata.is_file() || metadata.len() != entry.size_bytes {
            return Err(ContractError {
                exit_code: 1,
                message: "Source file size/type mismatch".into(),
            });
        }
        let mut digest = Sha256::new();
        let mut buffer = [0u8; 65536];
        let mut total = 0u64;
        loop {
            let n = file.read(&mut buffer).map_err(io_error)?;
            if n == 0 {
                break;
            }
            total = total
                .checked_add(n as u64)
                .ok_or_else(|| malformed("Source length overflow"))?;
            if total > entry.size_bytes {
                return Err(ContractError {
                    exit_code: 1,
                    message: "Source grew during admission".into(),
                });
            }
            digest.update(&buffer[..n]);
        }
        if total != entry.size_bytes || format!("{:x}", digest.finalize()) != entry.sha256 {
            return Err(ContractError {
                exit_code: 1,
                message: "Source checksum mismatch".into(),
            });
        }
        entry.path = resolved;
    }
    Ok(AdmittedSources {
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        entries: manifest.entries,
    })
}

#[cfg(test)]
pub(crate) mod frozen_tests {
    use super::*;
    pub(crate) fn fixture(
        kind: &str,
        key: &str,
        bytes: &[u8],
    ) -> (tempfile::TempDir, AdmittedSources) {
        use sha2::{Digest, Sha256};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source");
        std::fs::write(&path, bytes).unwrap();
        let manifest = dir.path().join("manifest.json");
        std::fs::write(
            &manifest,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1, "profile_sha256": profile_hash(), "entries": [{
                    "kind": kind, "key": key, "path": "source",
                    "sha256": format!("{:x}", Sha256::digest(bytes)), "size_bytes": bytes.len()
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let sources = admit_sources(&manifest).unwrap();
        (dir, sources)
    }
    #[test]
    fn coastal_classification_source_is_an_admitted_frozen_kind() {
        let (_dir, sources) = fixture("water_classification", "master-water-classification", b"{}");
        assert_eq!(
            sources
                .resolve(
                    "water_classification",
                    "master-water-classification",
                    16_777_216
                )
                .unwrap(),
            b"{}"
        );
    }
    #[test]
    fn coastal_classification_rejects_unknown_logical_key() {
        let (dir, _) = fixture("water_classification", "master-water-classification", b"{}");
        let path = dir.path().join("manifest.json");
        let bytes = std::fs::read_to_string(&path)
            .unwrap()
            .replace("master-water-classification", "unknown-classification");
        std::fs::write(&path, bytes).unwrap();
        assert!(admit_sources(&path).is_err());
    }
    #[test]
    fn frozen_returns_exact_verified_bytes_and_rechecks_mutation() {
        let (_dir, sources) = fixture("elevation", "aws:15:1:2", b"original");
        assert_eq!(
            sources.resolve("elevation", "aws:15:1:2", 8).unwrap(),
            b"original"
        );
        std::fs::write(&sources.entries[0].path, b"modified").unwrap();
        assert!(sources.resolve("elevation", "aws:15:1:2", 8).is_err());
    }
    #[test]
    fn frozen_rejects_missing_wrong_kind_oversize_and_growing_files() {
        let (_dir, sources) = fixture("land_cover", "url#bytes=0-7", b"original");
        assert!(sources.resolve("land_cover", "url#bytes=0-8", 9).is_err());
        assert!(sources.resolve("elevation", "url#bytes=0-7", 8).is_err());
        assert!(sources.resolve("land_cover", "url#bytes=0-7", 7).is_err());
        std::fs::write(&sources.entries[0].path, b"original-extra").unwrap();
        assert!(sources.resolve("land_cover", "url#bytes=0-7", 8).is_err());
    }
}
