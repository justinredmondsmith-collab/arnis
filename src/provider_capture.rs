//! Standalone bounded provider acquisition; never a world-generation path.
use crate::coordinate_system::geographic::LLBBox;
use crate::tiler_contract::{AdmittedSources, SourceResponseReader};
use reqwest::header::HeaderMap;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_RESPONSES: usize = 1024;
const MAX_BYTES: u64 = 256 * 1024 * 1024;
const DEADLINE_SECONDS: u64 = 600;
const MANIFEST_BYTES: usize = 1024 * 1024;
const ESA_BASE: &str = "https://esa-worldcover.s3.eu-central-1.amazonaws.com/v200/2021/map/";

pub(crate) fn descriptor() -> serde_json::Value {
    serde_json::json!({
        "schema":"arnis-provider-capture/v1", "command":"--capture-provider-sources",
        "arguments":["--bbox", "--capture-output"], "profile_sha256":crate::tiler_contract::profile_hash(),
        "scale":1.0, "network":"fixed-provider-https", "platform":"linux",
        "endpoints":{"aws":"https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png", "esa":ESA_BASE},
        "output":["sources.json","capture.json","responses/"],
        "limits":{"response_count":MAX_RESPONSES,"response_bytes":MAX_BYTES,
            "deadline_seconds":DEADLINE_SECONDS,"http_seconds":30,"connect_seconds":10,
            "aws_response_bytes":4*1024*1024,"esa_response_bytes":64*1024*1024,
            "memory_bytes":1024_u64*1024*1024,"disk_bytes":512_u64*1024*1024,
            "log_bytes":8*1024*1024,"max_files":1040,"tasks":64,"file_descriptors":256,
            "provider_concurrency":1,"master_max_axis":16384,"master_max_cells":16777216}
    })
}

enum Command {
    Describe,
    Capture { bbox: LLBBox, output: PathBuf },
}
fn parse_command(args: &[OsString]) -> Result<Option<Command>, String> {
    let selected = args.iter().any(|a| {
        a.to_str().is_some_and(|a| {
            a.starts_with("--capture-provider-sources")
                || a.starts_with("--describe-provider-capture")
                || a.starts_with("--capture-output")
        })
    });
    if !selected {
        return Ok(None);
    }
    if args == [OsString::from("--describe-provider-capture")] {
        return Ok(Some(Command::Describe));
    }
    if args.len() != 5
        || args[0] != "--capture-provider-sources"
        || args[1] != "--bbox"
        || args[3] != "--capture-output"
    {
        return Err("capture_input: expected --capture-provider-sources --bbox S,W,N,E --capture-output ABS".into());
    }
    let bbox = LLBBox::from_str(
        args[2]
            .to_str()
            .ok_or("capture_input: bbox must be UTF-8")?,
    )
    .map_err(|e| format!("capture_input: {e}"))?;
    let output = PathBuf::from(&args[4]);
    if !output.is_absolute()
        || output.file_name().is_none()
        || output
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("capture_input: output must be absolute without parent traversal".into());
    }
    validate_bbox(&bbox)?;
    Ok(Some(Command::Capture { bbox, output }))
}
fn validate_bbox(bbox: &LLBBox) -> Result<(usize, usize), String> {
    if bbox.min().lat() < -60.0 || bbox.max().lat() >= 84.0 {
        return Err("capture_input: complete ESA latitude coverage [-60,84) required".into());
    }
    let (w, h, gw, gh) = crate::elevation::compute_grid_dims(bbox, 1.0);
    if !(2..=16384).contains(&w)
        || !(2..=16384).contains(&h)
        || w.checked_mul(h).is_none_or(|n| n > 16777216)
    {
        return Err(
            "capture_capacity: bbox exceeds profile master dimensions; bbox was not changed".into(),
        );
    }
    Ok((gw, gh))
}

/// Returns None for ordinary renderer invocations. Capability reporting is independent.
pub(crate) fn run_command(args: &[OsString]) -> Option<Result<(), String>> {
    let command = match parse_command(args) {
        Ok(None) => return None,
        Ok(Some(c)) => c,
        Err(e) => return Some(Err(e)),
    };
    Some((|| {
        if std::env::vars_os().any(|(key, _)| key.to_str().is_some_and(|s| s.starts_with("ARNIS_")))
        {
            return Err(
                "capture_input: integration environment controls conflict with standalone capture"
                    .into(),
            );
        }
        match command {
            Command::Describe => println!("{}", descriptor()),
            Command::Capture { bbox, output } => {
                let client = reqwest::blocking::Client::builder()
                    .no_proxy()
                    .no_gzip()
                    .no_brotli()
                    .no_zstd()
                    .no_deflate()
                    .retry(reqwest::retry::never())
                    .redirect(reqwest::redirect::Policy::none())
                    .connect_timeout(Duration::from_secs(10))
                    .user_agent(concat!(
                        "Arnis-provider-capture/",
                        env!("CARGO_PKG_VERSION")
                    ))
                    .build()
                    .map_err(|e| e.to_string())?;
                capture(
                    &bbox,
                    &output,
                    Box::new(HttpTransport(client)),
                    Duration::from_secs(DEADLINE_SECONDS),
                )?;
                println!(
                    "{}",
                    serde_json::json!({"status":"complete","output":output})
                );
            }
        }
        Ok(())
    })())
}

#[derive(Clone, Debug)]
struct Request {
    kind: String,
    key: String,
    url: String,
    range: Option<(u64, u64)>,
    max_bytes: u64,
}
impl Request {
    fn from_key(kind: &str, key: &str, max_bytes: u64) -> Result<Self, String> {
        let (url, range, cap) = match kind {
            "elevation" => {
                let parts: Vec<_> = key.split(':').collect();
                if parts.len() != 4 || parts[0] != "aws" {
                    return Err("capture_input: invalid AWS key".into());
                }
                let z: u32 = parts[1]
                    .parse()
                    .map_err(|_| "capture_input: invalid zoom")?;
                let x: u32 = parts[2]
                    .parse()
                    .map_err(|_| "capture_input: invalid tile")?;
                let y: u32 = parts[3]
                    .parse()
                    .map_err(|_| "capture_input: invalid tile")?;
                if !(10..=15).contains(&z)
                    || x >= 1 << z
                    || y >= 1 << z
                    || key != format!("aws:{z}:{x}:{y}")
                {
                    return Err("capture_input: invalid AWS tile".into());
                }
                (
                    format!(
                        "https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png"
                    ),
                    None,
                    4 * 1024 * 1024,
                )
            }
            "land_cover" => {
                let (url, r) = key
                    .split_once("#bytes=")
                    .ok_or("capture_input: invalid ESA key")?;
                let name = url
                    .strip_prefix(ESA_BASE)
                    .ok_or("capture_input: invalid ESA destination")?;
                let code = name
                    .strip_prefix("ESA_WorldCover_10m_2021_v200_")
                    .and_then(|s| s.strip_suffix("_Map.tif"))
                    .ok_or("capture_input: invalid ESA filename")?;
                let c = code.as_bytes();
                if c.len() != 7
                    || !matches!(c[0], b'N' | b'S')
                    || !matches!(c[3], b'E' | b'W')
                    || !c[1..3].iter().chain(&c[4..7]).all(u8::is_ascii_digit)
                {
                    return Err("capture_input: invalid ESA coordinates".into());
                }
                let (start, end) = r
                    .split_once('-')
                    .ok_or("capture_input: invalid ESA range")?;
                let start: u64 = start
                    .parse()
                    .map_err(|_| "capture_input: invalid ESA range")?;
                let end: u64 = end
                    .parse()
                    .map_err(|_| "capture_input: invalid ESA range")?;
                let len = end
                    .checked_sub(start)
                    .and_then(|n| n.checked_add(1))
                    .ok_or("capture_input: invalid ESA range")?;
                if len > 64 * 1024 * 1024 || len > max_bytes || r != format!("{start}-{end}") {
                    return Err("capture_capacity: invalid or oversized ESA range".into());
                }
                (url.to_string(), Some((start, end)), len)
            }
            _ => return Err("capture_input: unsupported provider kind".into()),
        };
        Ok(Self {
            kind: kind.into(),
            key: key.into(),
            url,
            range,
            max_bytes: max_bytes.min(cap),
        })
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
struct ObjectVersion {
    etag: Option<String>,
    total: Option<u64>,
    last_modified: Option<String>,
}
fn one_header(headers: &HeaderMap, name: &str) -> Result<Option<String>, String> {
    let values: Vec<_> = headers.get_all(name).iter().collect();
    if values.len() > 1 {
        return Err(format!("capture_response: duplicate {name}"));
    }
    values
        .first()
        .map(|v| {
            let value = v
                .to_str()
                .map_err(|_| format!("capture_response: invalid {name}"))?;
            if value.len() > 8192 {
                return Err("capture_capacity: response header too large".into());
            }
            Ok(value.to_string())
        })
        .transpose()
}
fn validate_response(
    req: &Request,
    status: u16,
    h: &HeaderMap,
    previous: Option<&ObjectVersion>,
) -> Result<ObjectVersion, String> {
    if one_header(h, "content-encoding")?.is_some_and(|v| !v.eq_ignore_ascii_case("identity")) {
        return Err("capture_response: unexpected content encoding".into());
    }
    let etag = one_header(h, "etag")?;
    let last_modified = one_header(h, "last-modified")?;
    let total = if let Some((start, end)) = req.range {
        if status != 206 {
            return Err("capture_response: ESA requires 206".into());
        }
        let value =
            one_header(h, "content-range")?.ok_or("capture_response: missing Content-Range")?;
        let prefix = format!("bytes {start}-{end}/");
        let suffix = value
            .strip_prefix(&prefix)
            .ok_or("capture_response: wrong Content-Range")?;
        let total: u64 = suffix
            .parse()
            .map_err(|_| "capture_response: invalid range total")?;
        if total <= end || suffix != total.to_string() {
            return Err("capture_response: invalid range total".into());
        }
        let validator = etag
            .as_deref()
            .ok_or("capture_response: ESA requires strong ETag")?;
        if validator.len() < 2
            || !validator.starts_with('"')
            || !validator.ends_with('"')
            // Deliberately ASCII-only: opaque etagc excludes SP, HTAB, DQUOTE and controls.
            || !validator.as_bytes()[1..validator.len() - 1].iter().all(|&b| b == 0x21 || (0x23..=0x7e).contains(&b))
        {
            return Err("capture_response: ESA requires strong ETag".into());
        }
        Some(total)
    } else {
        if status != 200 {
            return Err("capture_response: AWS requires 200".into());
        }
        None
    };
    if let Some(length) = one_header(h, "content-length")? {
        let length: u64 = length
            .parse()
            .map_err(|_| "capture_response: invalid Content-Length")?;
        if length > req.max_bytes || req.range.is_some_and(|(s, e)| length != e - s + 1) {
            return Err("capture_response: Content-Length mismatch".into());
        }
    }
    let version = ObjectVersion {
        etag,
        total,
        last_modified,
    };
    if previous.is_some_and(|p| p.etag != version.etag || p.total != version.total) {
        return Err("capture_response: provider object changed".into());
    }
    Ok(version)
}
struct Response {
    status: u16,
    headers: HeaderMap,
    body: Box<dyn Read>,
}
trait Transport {
    fn get(
        &mut self,
        request: &Request,
        version: Option<&ObjectVersion>,
        timeout: Duration,
    ) -> Result<Response, String>;
}
struct HttpTransport(reqwest::blocking::Client);
impl Transport for HttpTransport {
    fn get(
        &mut self,
        req: &Request,
        version: Option<&ObjectVersion>,
        timeout: Duration,
    ) -> Result<Response, String> {
        let request = http_request(&self.0, req, version, timeout)?;
        let response = self
            .0
            .execute(request)
            .map_err(|e| format!("capture_network: {e}"))?;
        Ok(Response {
            status: response.status().as_u16(),
            headers: response.headers().clone(),
            body: Box::new(response),
        })
    }
}
fn http_request(
    client: &reqwest::blocking::Client,
    req: &Request,
    version: Option<&ObjectVersion>,
    timeout: Duration,
) -> Result<reqwest::blocking::Request, String> {
    let mut call = client
        .get(&req.url)
        .header("Accept-Encoding", "identity")
        .timeout(timeout);
    if let Some((s, e)) = req.range {
        call = call.header("Range", format!("bytes={s}-{e}"));
    }
    if let Some(etag) = version.and_then(|v| v.etag.as_ref()) {
        call = call.header("If-Match", etag);
    }
    call.build().map_err(|e| format!("capture_input: {e}"))
}

#[derive(Clone, Serialize)]
struct Entry {
    kind: String,
    key: String,
    path: PathBuf,
    sha256: String,
    size_bytes: u64,
}
#[derive(Serialize)]
struct Record {
    #[serde(flatten)]
    entry: Entry,
    url: String,
    range: Option<(u64, u64)>,
    status: u16,
    #[serde(flatten)]
    version: ObjectVersion,
}
struct State {
    transport: Box<dyn Transport>,
    entries: BTreeMap<(String, String), Record>,
    versions: BTreeMap<String, ObjectVersion>,
    bytes: u64,
}
struct Session {
    root: PathBuf,
    started: Instant,
    timeout: Duration,
    state: RefCell<State>,
}
impl SourceResponseReader for Session {
    fn checkpoint(&self) -> Result<(), String> {
        if self.started.elapsed() >= self.timeout {
            Err("capture_deadline: acquisition deadline exceeded".into())
        } else {
            Ok(())
        }
    }
    fn resolve(&self, kind: &str, key: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
        self.checkpoint()?;
        let request = Request::from_key(kind, key, max_bytes)?;
        let pair = (kind.to_string(), key.to_string());
        let mut state = self.state.borrow_mut();
        if let Some(record) = state.entries.get(&pair) {
            return read_entry(&self.root, &record.entry, max_bytes);
        }
        if state.entries.len() >= MAX_RESPONSES {
            return Err("capture_capacity: response count exceeded".into());
        }
        if state.bytes >= MAX_BYTES {
            return Err("capture_capacity: aggregate response bytes exceeded".into());
        }
        let previous = state.versions.get(&request.url).cloned();
        let remaining = self
            .timeout
            .checked_sub(self.started.elapsed())
            .ok_or("capture_deadline: acquisition deadline exceeded")?;
        let response = state.transport.get(
            &request,
            previous.as_ref(),
            remaining.min(Duration::from_secs(30)),
        )?;
        let version = validate_response(
            &request,
            response.status,
            &response.headers,
            previous.as_ref(),
        )?;
        let declared_length = one_header(&response.headers, "content-length")?
            .map(|v| {
                v.parse::<u64>()
                    .map_err(|_| "capture_response: invalid Content-Length")
            })
            .transpose()?;
        let mut partial = tempfile::NamedTempFile::new_in(self.root.join("responses"))
            .map_err(|e| e.to_string())?;
        let mut body = response.body;
        let mut digest = Sha256::new();
        let mut count = 0u64;
        let mut buffer = [0u8; 65536];
        let bound = request.max_bytes.min(MAX_BYTES - state.bytes);
        loop {
            self.checkpoint()?;
            let allowance = (bound - count).saturating_add(1).min(buffer.len() as u64) as usize;
            let n = body
                .read(&mut buffer[..allowance])
                .map_err(|e| format!("capture_network: {e}"))?;
            if n == 0 {
                break;
            }
            count += n as u64;
            if count > bound {
                return Err("capture_capacity: response or aggregate byte limit exceeded".into());
            }
            partial.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
            digest.update(&buffer[..n]);
        }
        if count == 0
            || declared_length.is_some_and(|length| length != count)
            || request.range.is_some_and(|(s, e)| count != e - s + 1)
        {
            return Err("capture_response: incomplete response body".into());
        }
        self.checkpoint()?;
        let sha256 = format!("{:x}", digest.finalize());
        let path = PathBuf::from("responses").join(&sha256);
        partial.as_file().sync_all().map_err(|e| e.to_string())?;
        if let Err(e) = partial.persist_noclobber(self.root.join(&path)) {
            if e.error.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(e.error.to_string());
            }
        }
        let entry = Entry {
            kind: request.kind,
            key: request.key,
            path,
            sha256,
            size_bytes: count,
        };
        let bytes = read_entry(&self.root, &entry, max_bytes)?;
        state.bytes += count;
        state.versions.insert(request.url.clone(), version.clone());
        state.entries.insert(
            pair,
            Record {
                entry,
                url: request.url,
                range: request.range,
                status: response.status,
                version,
            },
        );
        self.checkpoint()?;
        Ok(bytes)
    }
}
fn read_entry(root: &Path, entry: &Entry, max: u64) -> Result<Vec<u8>, String> {
    let sources = AdmittedSources {
        sha256: String::new(),
        entries: vec![crate::tiler_contract::SourceEntry {
            kind: entry.kind.clone(),
            key: entry.key.clone(),
            path: root.join(&entry.path),
            sha256: entry.sha256.clone(),
            size_bytes: entry.size_bytes,
        }],
    };
    sources.resolve(&entry.kind, &entry.key, max)
}
struct Replay<'a> {
    sources: &'a AdmittedSources,
    session: &'a Session,
    keys: RefCell<BTreeSet<(String, String)>>,
}
impl SourceResponseReader for Replay<'_> {
    fn checkpoint(&self) -> Result<(), String> {
        self.session.checkpoint()
    }
    fn resolve(&self, kind: &str, key: &str, max: u64) -> Result<Vec<u8>, String> {
        self.checkpoint()?;
        let bytes = self.sources.resolve(kind, key, max)?;
        self.keys.borrow_mut().insert((kind.into(), key.into()));
        self.checkpoint()?;
        Ok(bytes)
    }
}
fn visit(
    bbox: &LLBBox,
    dims: (usize, usize),
    reader: &dyn SourceResponseReader,
) -> Result<(), String> {
    crate::elevation::providers::aws_terrain::capture_tiles(
        bbox,
        dims.0,
        dims.1,
        reader,
        MAX_RESPONSES,
    )?;
    crate::land_cover::capture_ranges(bbox, reader).map_err(|e| e.to_string())
}
fn bounded_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if bytes.len() > MANIFEST_BYTES {
        return Err("capture_capacity: manifest exceeds 1 MiB".into());
    }
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())
}
fn capture(
    bbox: &LLBBox,
    output: &Path,
    transport: Box<dyn Transport>,
    timeout: Duration,
) -> Result<(), String> {
    let started = Instant::now();
    let started_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let dims = validate_bbox(bbox)?;
    if !output.is_absolute() || output.symlink_metadata().is_ok() {
        return Err("capture_input: output must be a fresh absolute path".into());
    }
    let parent = output
        .parent()
        .ok_or("capture_input: output has no parent")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let output = parent.join(
        output
            .file_name()
            .ok_or("capture_input: output has no name")?,
    );
    let stage = tempfile::Builder::new()
        .prefix(".provider-capture-incomplete-")
        .tempdir_in(&parent)
        .map_err(|e| e.to_string())?;
    std::fs::create_dir(stage.path().join("responses")).map_err(|e| e.to_string())?;
    let session = Session {
        root: stage.path().into(),
        started,
        timeout,
        state: RefCell::new(State {
            transport,
            entries: BTreeMap::new(),
            versions: BTreeMap::new(),
            bytes: 0,
        }),
    };
    visit(bbox, dims, &session)?;
    let entries: Vec<_> = session
        .state
        .borrow()
        .entries
        .values()
        .map(|r| r.entry.clone())
        .collect();
    bounded_json(
        &stage.path().join("sources.json"),
        &serde_json::json!({"schema_version":1,"profile_sha256":crate::tiler_contract::profile_hash(),"entries":entries}),
    )?;
    session.checkpoint()?;
    let sources = crate::tiler_contract::admit_sources(&stage.path().join("sources.json"))
        .map_err(|e| e.to_string())?;
    session.checkpoint()?;
    let replay = Replay {
        sources: &sources,
        session: &session,
        keys: RefCell::new(BTreeSet::new()),
    };
    visit(bbox, dims, &replay)?;
    if *replay.keys.borrow() != session.state.borrow().entries.keys().cloned().collect() {
        return Err("capture_replay: request set mismatch".into());
    }
    // Hash the executing inode, even when a build or updater replaces its pathname.
    #[cfg(target_os = "linux")]
    let executable = PathBuf::from("/proc/self/exe");
    #[cfg(not(target_os = "linux"))]
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut file = std::fs::File::open(executable).map_err(|e| e.to_string())?;
    let mut binary_digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        session.checkpoint()?;
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        binary_digest.update(&buffer[..n]);
    }
    let capabilities = crate::tiler_contract::capability_report();
    bounded_json(
        &stage.path().join("capture.json"),
        &serde_json::json!({
            "schema":"arnis-provider-capture/v1","descriptor":descriptor(),"build":capabilities["build"],"upstream":capabilities["upstream"],
            "binary_sha256":format!("{:x}",binary_digest.finalize()),"bbox":[bbox.min().lat(),bbox.min().lng(),bbox.max().lat(),bbox.max().lng()],
            "grid_dimensions":[dims.0,dims.1],"started_unix_seconds":started_unix,"elapsed_millis":started.elapsed().as_millis(),
            "sources_sha256":sources.sha256,"responses":session.state.borrow().entries.values().collect::<Vec<_>>()
        }),
    )?;
    std::fs::File::open(stage.path().join("responses"))
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    std::fs::File::open(stage.path())
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    session.checkpoint()?;
    publish_noreplace(stage.path(), &output)?;
    // The original staging path no longer exists; its TempDir cannot delete the result.
    if let Err(error) = std::fs::File::open(&parent).and_then(|f| f.sync_all()) {
        // The namespace is already published. The supervising caller must reject this
        // nonzero result even if files remain for diagnosis; it is not a success token.
        return Err(format!(
            "capture_publish_durability: {} exists but parent sync failed: {error}",
            output.display()
        ));
    }
    Ok(())
}
#[cfg(target_os = "linux")]
fn publish_noreplace(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    unsafe extern "C" {
        fn renameat2(
            olddirfd: i32,
            oldpath: *const std::ffi::c_char,
            newdirfd: i32,
            newpath: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }
    let source =
        std::ffi::CString::new(source.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let destination =
        std::ffi::CString::new(destination.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    // Both NUL-terminated paths remain alive; AT_FDCWD and RENAME_NOREPLACE are Linux ABI constants.
    if unsafe { renameat2(-100, source.as_ptr(), -100, destination.as_ptr(), 1) } != 0 {
        return Err(format!(
            "capture_publish: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn publish_noreplace(_source: &Path, _destination: &Path) -> Result<(), String> {
    Err("capture_platform: atomic publication requires Linux".into())
}

#[cfg(test)]
#[path = "provider_capture_tests.rs"]
mod tests;
