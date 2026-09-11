use super::*;
use reqwest::header::{HeaderMap, HeaderValue};

fn range_request() -> Request {
    Request::from_key("land_cover", "https://esa-worldcover.s3.eu-central-1.amazonaws.com/v200/2021/map/ESA_WorldCover_10m_2021_v200_N39W075_Map.tif#bytes=8-11", 4).unwrap()
}
fn headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("content-range", HeaderValue::from_static("bytes 8-11/100"));
    h.insert("etag", HeaderValue::from_static("\"object-v1\""));
    h
}

#[test]
fn descriptor_is_separate_from_nine_capabilities() {
    let value = descriptor();
    assert_eq!(value["schema"], "arnis-provider-capture/v1");
    assert_eq!(value["limits"]["response_count"], 1024);
    assert_eq!(value["limits"]["max_files"], 1040);
    let caps = crate::tiler_contract::capability_report();
    assert_eq!(caps["capabilities"].as_array().unwrap().len(), 9);
    assert!(caps.get("provider_capture").is_none());
}

#[test]
fn exact_range_response_is_required() {
    let req = range_request();
    let version = validate_response(&req, 206, &headers(), None).unwrap();
    assert_eq!(version.total, Some(100));
    for value in [
        "bytes 8-12/100",
        "bytes 7-10/100",
        "bytes 8-11/11",
        "bytes 8-11/*",
        "bytes 8-11/100, bytes 8-11/100",
    ] {
        let mut h = headers();
        h.insert("content-range", value.parse().unwrap());
        assert!(validate_response(&req, 206, &h, None).is_err(), "{value}");
    }
    assert!(validate_response(&req, 200, &headers(), None).is_err());
    let mut h = headers();
    h.append("content-range", HeaderValue::from_static("bytes 8-11/100"));
    assert!(validate_response(&req, 206, &h, None).is_err());
    let mut h = headers();
    h.insert("content-encoding", HeaderValue::from_static("gzip"));
    assert!(validate_response(&req, 206, &h, None).is_err());
}

#[test]
fn esa_versions_are_strong_and_stable() {
    let req = range_request();
    let version = validate_response(&req, 206, &headers(), None).unwrap();
    for value in [
        "W/\"weak\"",
        "unquoted",
        "\"changed\"",
        "\"a b\"",
        "\"a\tb\"",
    ] {
        let mut h = headers();
        h.insert("etag", value.parse().unwrap());
        assert!(validate_response(&req, 206, &h, Some(&version)).is_err());
    }
    for value in ["\"a b\"", "\"a\tb\""] {
        let mut h = headers();
        h.insert("etag", value.parse().unwrap());
        assert!(validate_response(&req, 206, &h, None).is_err(), "{value:?}");
    }
    let mut h = headers();
    h.remove("etag");
    assert!(validate_response(&req, 206, &h, None).is_err());
    let mut h = headers();
    h.insert("content-range", HeaderValue::from_static("bytes 8-11/101"));
    assert!(validate_response(&req, 206, &h, Some(&version)).is_err());
}

#[test]
fn request_keys_cannot_choose_other_destinations() {
    for key in [
        "https://evil.invalid/a#bytes=0-3",
        "https://esa-worldcover.s3.eu-central-1.amazonaws.com/other#bytes=0-3",
        "https://esa-worldcover.s3.eu-central-1.amazonaws.com/v200/2021/map/../secret#bytes=0-3",
    ] {
        assert!(Request::from_key("land_cover", key, 4).is_err());
    }
    assert!(Request::from_key("elevation", "aws:15:1:2", 4 * 1024 * 1024).is_ok());
    assert!(Request::from_key("elevation", "aws:15:999999:2", 4 * 1024 * 1024).is_err());
}

#[test]
fn capture_cli_rejects_conflicts_before_io() {
    for argv in [
        vec!["--describe-provider-capture", "--bbox", "0,0,1,1"],
        vec![
            "--capture-provider-sources",
            "--bbox",
            "0,0,1,1",
            "--capture-output",
            "relative",
        ],
        vec![
            "--capture-provider-sources",
            "--bbox",
            "0,0,1,1",
            "--capture-output",
            "/tmp/x",
            "--scale",
            "2",
        ],
    ] {
        assert!(parse_command(&argv.into_iter().map(Into::into).collect::<Vec<_>>()).is_err());
    }
}

fn tiff_fixture(big: bool) -> Vec<u8> {
    let mut bytes = vec![0; 200000];
    bytes[..2].copy_from_slice(b"II");
    bytes[2..4].copy_from_slice(&(if big { 43u16 } else { 42 }).to_le_bytes());
    let ifd = if big { 70000 } else { 16 };
    if big {
        bytes[4..6].copy_from_slice(&8u16.to_le_bytes());
        bytes[8..16].copy_from_slice(&(ifd as u64).to_le_bytes());
        bytes[ifd..ifd + 8].copy_from_slice(&7u64.to_le_bytes());
    } else {
        bytes[4..8].copy_from_slice(&(ifd as u32).to_le_bytes());
        bytes[ifd..ifd + 2].copy_from_slice(&7u16.to_le_bytes());
    }
    let entries = [
        (256, 4, 1, 6),
        (257, 4, 1, 6),
        (322, 4, 1, 2),
        (323, 4, 1, 2),
        (259, 3, 1, 1),
        (324, if big { 16 } else { 4 }, 9, 140000),
        (325, if big { 16 } else { 4 }, 9, 141000),
    ];
    for (i, (tag, typ, count, value)) in entries.into_iter().enumerate() {
        let at = ifd + if big { 8 + i * 20 } else { 2 + i * 12 };
        bytes[at..at + 2].copy_from_slice(&(tag as u16).to_le_bytes());
        bytes[at + 2..at + 4].copy_from_slice(&(typ as u16).to_le_bytes());
        if big {
            bytes[at + 4..at + 12].copy_from_slice(&(count as u64).to_le_bytes());
            bytes[at + 12..at + 20].copy_from_slice(&(value as u64).to_le_bytes());
        } else {
            bytes[at + 4..at + 8].copy_from_slice(&(count as u32).to_le_bytes());
            bytes[at + 8..at + 12].copy_from_slice(&(value as u32).to_le_bytes());
        }
    }
    for i in 0..9 {
        let size = if big { 8 } else { 4 };
        let offset = (150000 + i * 4) as u64;
        bytes[140000 + i * size..140000 + (i + 1) * size]
            .copy_from_slice(&offset.to_le_bytes()[..size]);
        bytes[141000 + i * size..141000 + (i + 1) * size]
            .copy_from_slice(&4u64.to_le_bytes()[..size]);
        bytes[150000 + i * 4..150000 + i * 4 + 4].fill(10);
    }
    bytes
}

struct FixtureTransport {
    calls: std::rc::Rc<RefCell<Vec<String>>>,
    tiff: Vec<u8>,
    fail_at: Option<usize>,
}
impl Transport for FixtureTransport {
    fn get(
        &mut self,
        req: &Request,
        version: Option<&ObjectVersion>,
        _timeout: Duration,
    ) -> Result<Response, String> {
        if self.fail_at == Some(self.calls.borrow().len()) {
            return Err("fixture transport failure".into());
        }
        let prior = self
            .calls
            .borrow()
            .iter()
            .any(|key| key.starts_with(&req.url));
        self.calls.borrow_mut().push(req.key.clone());
        let (status, headers, body) = if let Some((s, e)) = req.range {
            if prior {
                assert_eq!(version.unwrap().etag.as_deref(), Some("\"fixture\""));
            }
            let mut h = HeaderMap::new();
            h.insert(
                "content-range",
                format!("bytes {s}-{e}/{}", self.tiff.len())
                    .parse()
                    .unwrap(),
            );
            h.insert("etag", HeaderValue::from_static("\"fixture\""));
            (206, h, self.tiff[s as usize..=e as usize].to_vec())
        } else {
            let mut png = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                256,
                256,
                image::Rgb([128, 42, 0]),
            ))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
            (200, HeaderMap::new(), png.into_inner())
        };
        Ok(Response {
            status,
            headers,
            body: Box::new(std::io::Cursor::new(body)),
        })
    }
}
fn fixture_transport(
    big: bool,
    fail_at: Option<usize>,
) -> (Box<dyn Transport>, std::rc::Rc<RefCell<Vec<String>>>) {
    let calls = std::rc::Rc::new(RefCell::new(Vec::new()));
    (
        Box::new(FixtureTransport {
            calls: calls.clone(),
            tiff: tiff_fixture(big),
            fail_at,
        }),
        calls,
    )
}

#[test]
fn captures_classic_and_bigtiff_then_replays_without_transport() {
    let bbox = LLBBox::new(40.7000, -74.0000, 40.7001, -73.9999).unwrap();
    for big in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("capture");
        let (transport, calls) = fixture_transport(big, None);
        capture(&bbox, &output, transport, Duration::from_secs(60)).unwrap();
        let sources = crate::tiler_contract::admit_sources(&output.join("sources.json")).unwrap();
        assert_eq!(
            calls.borrow().len(),
            sources.entries.len(),
            "offline replay or duplicate request made HTTP calls"
        );
        assert!(
            calls.borrow().iter().any(|k| k.contains("#bytes=140000-")),
            "external TIFF arrays were not captured"
        );
        assert_eq!(
            calls
                .borrow()
                .iter()
                .any(|k| k.ends_with("#bytes=70000-135535")),
            big
        );
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(output.join("capture.json")).unwrap()).unwrap();
        assert_eq!(report["sources_sha256"], sources.sha256);
        let next = root.path().join("repeat");
        capture(
            &bbox,
            &next,
            fixture_transport(big, None).0,
            Duration::from_secs(60),
        )
        .unwrap();
        assert_eq!(
            std::fs::read(output.join("sources.json")).unwrap(),
            std::fs::read(next.join("sources.json")).unwrap()
        );
    }
}

#[test]
fn capture_failure_is_atomic_and_existing_output_is_preserved() {
    let bbox = LLBBox::new(40.7000, -74.0000, 40.7001, -73.9999).unwrap();
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("capture");
    let (transport, calls) = fixture_transport(false, Some(2));
    assert!(capture(&bbox, &output, transport, Duration::from_secs(60)).is_err());
    assert_eq!(calls.borrow().len(), 2);
    assert!(!output.exists());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    std::fs::create_dir(&output).unwrap();
    std::fs::write(output.join("keep"), b"previous").unwrap();
    let (transport, calls) = fixture_transport(false, None);
    assert!(capture(&bbox, &output, transport, Duration::from_secs(60)).is_err());
    assert!(calls.borrow().is_empty());
    assert_eq!(std::fs::read(output.join("keep")).unwrap(), b"previous");
}

#[test]
fn capture_deadline_precedes_network_and_publication_is_noreplace() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("capture");
    let (transport, calls) = fixture_transport(false, None);
    let bbox = LLBBox::new(40.7000, -74.0000, 40.7001, -73.9999).unwrap();
    assert!(capture(&bbox, &output, transport, Duration::ZERO)
        .unwrap_err()
        .starts_with("capture_deadline"));
    assert!(calls.borrow().is_empty());
    let source = root.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(&output).unwrap();
    assert!(publish_noreplace(&source, &output).is_err());
    assert!(source.is_dir());
}

#[test]
fn strict_capture_nodata_uses_only_requested_overlap() {
    // At this bbox the 6x6 fixture covers global tile pixel (2,2), chunk 4 offset 0.
    let bbox = LLBBox::new(40.7000, -74.0000, 40.7001, -73.9999).unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut tiff = tiff_fixture(false);
    tiff[150000 + 4 * 4] = 0; // Other pixels in this same chunk remain nonzero.
    let transport = Box::new(FixtureTransport {
        calls: Default::default(),
        tiff,
        fail_at: None,
    });
    let error = capture(
        &bbox,
        &root.path().join("capture"),
        transport,
        Duration::from_secs(60),
    )
    .unwrap_err();
    assert!(error.contains("requested overlap has no data"), "{error}");
}

#[test]
fn zero_chunks_are_valid_when_other_requested_overlap_has_data() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("responses")).unwrap();
    let mut tiff = tiff_fixture(false);
    tiff[150000..150004].fill(0);
    let session = Session {
        root: root.path().into(),
        started: Instant::now(),
        timeout: Duration::from_secs(60),
        state: RefCell::new(State {
            transport: Box::new(FixtureTransport {
                calls: Default::default(),
                tiff,
                fail_at: None,
            }),
            entries: BTreeMap::new(),
            versions: BTreeMap::new(),
            bytes: 0,
        }),
    };
    // Direct synthetic provider test spans four fixture chunks (no world generation).
    let bbox = LLBBox::new(40.5, -74.1, 41.5, -73.9).unwrap();
    crate::land_cover::capture_ranges(&bbox, &session).unwrap();
    let keys: BTreeSet<_> = session.state.borrow().entries.keys().cloned().collect();
    let entries = session
        .state
        .borrow()
        .entries
        .values()
        .map(|r| crate::tiler_contract::SourceEntry {
            kind: r.entry.kind.clone(),
            key: r.entry.key.clone(),
            path: root.path().join(&r.entry.path),
            sha256: r.entry.sha256.clone(),
            size_bytes: r.entry.size_bytes,
        })
        .collect();
    let sources = AdmittedSources {
        sha256: String::new(),
        entries,
    };
    let reader = Replay {
        sources: &sources,
        session: &session,
        keys: RefCell::new(BTreeSet::new()),
    };
    let data =
        crate::land_cover::fetch_land_cover_data_with_reader(&bbox, 4, 4, Some(&reader)).unwrap();
    assert!(data.grid.iter().flatten().any(|&value| value != 0));
    assert_eq!(
        *reader.keys.borrow(),
        keys,
        "capture and production requested different ranges"
    );
}

struct BytesTransport {
    bytes: Vec<u8>,
    length: Option<u64>,
}
impl Transport for BytesTransport {
    fn get(
        &mut self,
        _req: &Request,
        _version: Option<&ObjectVersion>,
        _timeout: Duration,
    ) -> Result<Response, String> {
        let mut headers = HeaderMap::new();
        if let Some(length) = self.length {
            headers.insert("content-length", length.to_string().parse().unwrap());
        }
        Ok(Response {
            status: 200,
            headers,
            body: Box::new(std::io::Cursor::new(self.bytes.clone())),
        })
    }
}
fn byte_session(root: &Path, bytes: Vec<u8>, length: Option<u64>) -> Session {
    std::fs::create_dir(root.join("responses")).unwrap();
    Session {
        root: root.into(),
        started: Instant::now(),
        timeout: Duration::from_secs(60),
        state: RefCell::new(State {
            transport: Box::new(BytesTransport { bytes, length }),
            entries: BTreeMap::new(),
            versions: BTreeMap::new(),
            bytes: 0,
        }),
    }
}
#[test]
fn byte_and_count_caps_fail_before_publishing_entries() {
    let root = tempfile::tempdir().unwrap();
    let session = byte_session(root.path(), vec![1; 5], None);
    assert!(session
        .resolve("elevation", "aws:15:1:2", 4)
        .unwrap_err()
        .starts_with("capture_capacity"));
    assert!(session.state.borrow().entries.is_empty());
    assert_eq!(
        std::fs::read_dir(root.path().join("responses"))
            .unwrap()
            .count(),
        0
    );
    session.state.borrow_mut().bytes = MAX_BYTES - 3;
    assert!(session
        .resolve("elevation", "aws:15:1:2", 8)
        .unwrap_err()
        .starts_with("capture_capacity"));
    for i in 0..MAX_RESPONSES {
        let entry = Entry {
            kind: "elevation".into(),
            key: format!("unused:{i}"),
            path: PathBuf::new(),
            sha256: String::new(),
            size_bytes: 0,
        };
        session.state.borrow_mut().entries.insert(
            (entry.kind.clone(), entry.key.clone()),
            Record {
                entry,
                url: String::new(),
                range: None,
                status: 200,
                version: ObjectVersion {
                    etag: None,
                    total: None,
                    last_modified: None,
                },
            },
        );
    }
    assert!(session
        .resolve("elevation", "aws:15:1:2", 8)
        .unwrap_err()
        .contains("response count"));
}
#[test]
fn aws_body_must_match_declared_content_length() {
    let root = tempfile::tempdir().unwrap();
    let session = byte_session(root.path(), vec![1; 3], Some(4));
    assert!(session.resolve("elevation", "aws:15:1:2", 8).is_err());
}

#[test]
fn duplicate_key_replays_verified_bytes_without_spending_budget() {
    let root = tempfile::tempdir().unwrap();
    let session = byte_session(root.path(), vec![1, 2, 3], None);
    assert_eq!(
        session.resolve("elevation", "aws:15:1:2", 8).unwrap(),
        [1, 2, 3]
    );
    assert_eq!(
        session.resolve("elevation", "aws:15:1:2", 8).unwrap(),
        [1, 2, 3]
    );
    assert_eq!(session.state.borrow().bytes, 3);
    assert_eq!(session.state.borrow().entries.len(), 1);
    let path = session
        .state
        .borrow()
        .entries
        .values()
        .next()
        .unwrap()
        .entry
        .path
        .clone();
    std::fs::write(root.path().join(path), b"bad").unwrap();
    assert!(session.resolve("elevation", "aws:15:1:2", 8).is_err());
}

#[test]
fn corrupt_png_never_publishes() {
    let root = tempfile::tempdir().unwrap();
    let bbox = LLBBox::new(40.7000, -74.0000, 40.7001, -73.9999).unwrap();
    let output = root.path().join("capture");
    assert!(capture(
        &bbox,
        &output,
        Box::new(BytesTransport {
            bytes: b"not png".to_vec(),
            length: None
        }),
        Duration::from_secs(60)
    )
    .is_err());
    assert!(!output.exists());
}

#[test]
fn actual_http_request_pins_ranges_and_if_match() {
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap();
    let req = range_request();
    let version = validate_response(&req, 206, &headers(), None).unwrap();
    let request = http_request(&client, &req, Some(&version), Duration::from_secs(3)).unwrap();
    assert_eq!(request.url().as_str(), req.url);
    assert_eq!(request.headers()["range"], "bytes=8-11");
    assert_eq!(request.headers()["if-match"], "\"object-v1\"");
    assert_eq!(request.headers()["accept-encoding"], "identity");
    assert_eq!(request.timeout(), Some(&Duration::from_secs(3)));
    let first = http_request(&client, &req, None, Duration::from_secs(3)).unwrap();
    assert!(!first.headers().contains_key("if-match"));
}

#[cfg(unix)]
#[test]
fn capture_rejects_symlink_destination_before_transport() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("capture");
    std::os::unix::fs::symlink(root.path().join("missing"), &output).unwrap();
    let (transport, calls) = fixture_transport(false, None);
    let bbox = LLBBox::new(40.7000, -74.0000, 40.7001, -73.9999).unwrap();
    assert!(capture(&bbox, &output, transport, Duration::from_secs(60)).is_err());
    assert!(calls.borrow().is_empty());
    assert!(output.is_symlink());
}

struct ShortRangeTransport(Vec<u8>);
impl Transport for ShortRangeTransport {
    fn get(
        &mut self,
        _req: &Request,
        _version: Option<&ObjectVersion>,
        _timeout: Duration,
    ) -> Result<Response, String> {
        Ok(Response {
            status: 206,
            headers: headers(),
            body: Box::new(std::io::Cursor::new(self.0.clone())),
        })
    }
}
#[test]
fn short_and_excess_esa_bodies_never_become_entries() {
    for body in [vec![1, 2], vec![1, 2, 3, 4, 5]] {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("responses")).unwrap();
        let session = Session {
            root: root.path().into(),
            started: Instant::now(),
            timeout: Duration::from_secs(60),
            state: RefCell::new(State {
                transport: Box::new(ShortRangeTransport(body)),
                entries: BTreeMap::new(),
                versions: BTreeMap::new(),
                bytes: 0,
            }),
        };
        let req = range_request();
        assert!(session.resolve(&req.kind, &req.key, req.max_bytes).is_err());
        assert!(session.state.borrow().entries.is_empty());
        assert_eq!(
            std::fs::read_dir(root.path().join("responses"))
                .unwrap()
                .count(),
            0
        );
    }
}

struct DelayedBody(std::rc::Rc<std::cell::Cell<bool>>);
impl Read for DelayedBody {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        std::thread::sleep(Duration::from_millis(150));
        self.0.set(true);
        buffer[0] = 1;
        Ok(1)
    }
}
struct DelayedTransport(std::rc::Rc<std::cell::Cell<bool>>);
impl Transport for DelayedTransport {
    fn get(
        &mut self,
        _req: &Request,
        _version: Option<&ObjectVersion>,
        _timeout: Duration,
    ) -> Result<Response, String> {
        Ok(Response {
            status: 200,
            headers: HeaderMap::new(),
            body: Box::new(DelayedBody(self.0.clone())),
        })
    }
}
#[test]
fn midstream_deadline_drops_partial_response() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("responses")).unwrap();
    let read = std::rc::Rc::new(std::cell::Cell::new(false));
    let session = Session {
        root: root.path().into(),
        started: Instant::now(),
        timeout: Duration::from_millis(100),
        state: RefCell::new(State {
            transport: Box::new(DelayedTransport(read.clone())),
            entries: BTreeMap::new(),
            versions: BTreeMap::new(),
            bytes: 0,
        }),
    };
    assert!(session
        .resolve("elevation", "aws:15:1:2", 8)
        .unwrap_err()
        .starts_with("capture_deadline"));
    assert!(read.get(), "deadline fixture did not reach the stream");
    assert!(session.state.borrow().entries.is_empty());
    assert_eq!(
        std::fs::read_dir(root.path().join("responses"))
            .unwrap()
            .count(),
        0
    );
}
