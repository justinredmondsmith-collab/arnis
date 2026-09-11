# Source-backed coastal classification implementation plan

> **For agentic workers:** Use superpowers:executing-plans to implement this plan task by task. Parent and independent design review precede runtime edits; parent owns integration and acquisition.

**Goal:** Produce an authenticated, offline `master-coastal-water-v1` declaration for an exact master bbox from detailed ocean geometry and the renderer's actual positive OSM land footprints.

**Architecture:** A fixed standalone command reads authenticated master OSM and unsimplified clipped ocean geometry, computes exact master-sample classes, emits sample-footprint polygons, and reparses its output through the production coastal policy to prove equality at every sample. Shared pure OSM and natural-cliff footprint helpers preserve stock generation behavior. The existing nine-capability report and coastal elevation conflict policy stay unchanged.

**Tech stack:** Rust, existing serde/sha2/geo geometry and OSM parser, current master coordinate transform and flood-fill implementation; synthetic offline fixtures.

Issue: https://github.com/justinredmondsmith-collab/arnis/issues/22
Claim: https://github.com/justinredmondsmith-collab/arnis/issues/22#issuecomment-5641301439
Branch/worktree: `feat/22-coastal-classification`, `/var/home/bazzite/arnis-v31-coastal-classification`.
Base: `cf44a28e4b6b9aac7e5bf56d6d594a32f1b0d085`; exact upstream v3.1.0: `3918513acb4e5e9ef4332418531a7c444d2b5acf`.
Root acquisition design: `/var/home/bazzite/arnis-v31-normal-source-freeze-design-2026-09-11.md`.

## Scope and ownership

Renderer owns the new source role, strict input envelope, shared geometry extraction, bounded offline CLI, exact serialized classification verification, and provenance checks on use. Root owns authoritative dataset acquisition, complete spatial query/dissolve/clip, OSM normalization, source assembly, Python validator coordination, and supervision. A local geometry envelope authenticates the producer's claimed dataset/query evidence; it cannot cryptographically prove the upstream dataset's truth. Root must bind its reproducible query report and original archive digest before treating that claim as complete.

No network, Minecraft, world generation, public release, installation, inventory freeze, or live acquisition in this task. No new capability in `--tiler-capabilities`. No provider/profile changes, 15 m shore buffer, guessed coast, flat fallback, simplified ocean, bbox shrink, or water removal to accommodate infrastructure.

## Fixed offline interface and wire contract

Proposed exact argv, no aliases or extra flags:

```
arnis --describe-coastal-classification
arnis --produce-coastal-classification --bbox S,W,N,E --frozen-sources ABS_MANIFEST --classification-output ABS_ABSENT_DIRECTORY
```

Dispatch before normal rendering, following provider capture's strict standalone command convention. Descriptor schema `arnis-coastal-classification/v1`, command `--produce-coastal-classification`, arguments `["--bbox","--frozen-sources","--classification-output"]`, profile SHA, scale 1.0, network `none`, platform `linux`, output `["water-classification.json","classification.json"]`, explicit limits below. Describe remains independent of the fixed nine-capability schema.

The input is an existing authenticated source manifest containing at least `osm:master-osm` and new `coastal_geometry:master-ocean-geometry`; consume only those roles as classifier inputs. Additional admitted roles may be present for root assembly, but cannot substitute for these inputs. Preserve existing whole-manifest authentication: additional roles are streamed for hashes in fixed 64 KiB buffers with the same whole-stage deadline, and are never decoded or used as substitute evidence. No output classification required in the producer's input. Admit and authenticate bytes using the existing source manifest/path/hash machinery. Add only this role/key pair to the role allowlist; malformed or alternate keys fail. Root implements the corresponding Python source-role validator before use.

`coastal_geometry:master-ocean-geometry` is a strict JSON envelope (deny unknown fields):

```
{
  "schema_version": 1,
  "policy": "global-ocean-clip-v1",
  "profile_sha256": "<current profile SHA256>",
  "bbox": [south, west, north, east],
  "grid_dimensions": [width, height],
  "water": {"type": "MultiPolygon", "coordinates": []},
  "dataset": {
    "url": "https://osmdata.openstreetmap.de/download/water-polygons-split-4326.zip",
    "archive_sha256": "<downloaded archive SHA256>",
    "projection": "EPSG:4326",
    "parts": {
      "ocean.shp": {"sha256": "<SHA256>", "size_bytes": 1},
      "ocean.shx": {"sha256": "<SHA256>", "size_bytes": 1},
      "ocean.prj": {"sha256": "<SHA256>", "size_bytes": 1}
    }
  },
  "complete_dataset_scan": true,
  "selected_features": 0
}
```

Part sizes above are illustrative, not hardcoded acceptance sizes. Dataset URL is pinned; projection must be EPSG:4326, hashes lowercase 64 hex, and part sizes positive checked u64 values. The parts map has exactly the three named authenticated geometry/index/projection files. `complete_dataset_scan` must be true and selected_features a checked nonnegative integer. Root scans the complete global dataset, selects all intersecting features, dissolves overlapping split pieces then clips without simplification, and binds archive plus part digests. HTTP ETag/Last-Modified and acquisition/query execution evidence remain in root's acquisition report; they are not classifier input fields. Source manifest SHA binds the entire envelope and unsimplified rings. Coordinates must be finite, within bbox, closed valid rings with preserved holes. Empty MultiPolygon is permitted only with complete-scan evidence; Boundary-only line/point contacts cannot silently disappear: root must reject lower-dimensional ocean intersections not covered by retained polygon boundaries, since production intersects semantics include wet boundary samples and MultiPolygon cannot encode zero-area contacts. A positive selected_features count with empty polygon output is therefore an explicit geometry error, not proof of no ocean. Missing geometry is never inland. Exact bbox/profile/dimensions bind the geometry to the sample encoding frame.

Compute dimensions using existing `compute_grid_dims(bbox, 1.0)` and require world dimensions equal provider grid dimensions, axes 2..16384 and at most 16777216 cells. Do not change intrinsic provider downsampling or dimension computation. Exact bbox/profile/dimensions must match input. Exact input byte hashes, source manifest hash, envelope and OSM hashes, renderer build/upstream/executable hash, descriptor, frame, class counts, encoding mode, mapped-cliff coverage counts, elapsed time, and output SHA belong in `classification.json` (schema `arnis-coastal-classification-report/v1`). Output is labelled **master-sample footprints**, never claimed to reproduce literal detailed ocean rings. Modes: `sample-footprints`, `literal-ocean-no-wet-sample`, `complete-query-no-ocean`.

The generated classification retains the existing document schema unchanged: policy `master-coastal-water-v1`, default `inland`, exact bbox, references to both input roles, one domain containing water W and positive dry D, empty inland exclusions. Empty coastal domains are reserved for complete authoritative W-empty query. Report metadata is a sidecar, not extra unknown classification fields.

On use, `CoastalPolicy::load` authenticates and validates any referenced new geometry envelope, checks bbox/profile, and carries its expected dimensions. `capture` checks those dimensions against actual height/land-cover bands before classifying. New-role-backed declarations must reference both exact fixed input keys. Existing legacy declarations without the new role retain existing behavior and tests. The producer checks every sample after actual serialization and production parse, so downstream must retain the exact source-bound declaration bytes. Root's source manifest authenticates the generated declaration and referenced geometry. A report alone does not grant provenance or bypass these checks.

## Shared positive evidence, with stock behavior preserved

Extract the existing mask-building prefix of `src/land_cover/osm_land_override.rs` into a pure helper returning building/transport land L, over-water O, mapped water area/channel I, and existing any-area/any-land flags. `apply_osm_land_override` consumes the same masks and then runs its unchanged rim/past-outline heuristics. The new producer consumes only raw masks; never dilations or repaired ESA outputs.

Reuse existing GridMap, scanline `fill_rings`, `relation_rings`, closed-ring convention, clip helper, Bresenham line stamping, and actual highway widths. Existing details are contractual: over-water ways take precedence; pier/breakwater/groyne/dolphin/quay, bridges and floating ways veto land; tunnel/area/man_made rules retain their current exclusions; building and building:part rules and relation hole handling remain exact. I includes existing water polygons and non-underground channel footprints with existing widths, not a second Python interpretation. Store bitsets, not per-cell objects.

Extract a shared natural-way geometry helper in `src/element_processing/natural.rs` for cliff evidence C. It must reuse the actual flood-fill cache/master clipping and edge Bresenham cells. Existing natural generation processes each relation outer member independently with inherited tags; it does not merge outer members and ignores inner members. Preserve that convention explicitly and report mapped coverage, rather than claiming universal geological cliffs. Closed oversized rings with empty fill currently skip edges too; keep that rule. Open mapped cliffs contribute their actual line cells. Helper returns/visits exactly the fill and edge cells that generation would paint; stock material RNG and writes remain in generation. No WorldEditor construction or generated world is needed by the producer. Deadline/capacity failure aborts output, never silently omits an expensive cliff. Water masks still veto cliff evidence, including mapped inland water inside relation rings.

Parse authenticated OSM with the same `CoordTransformer::for_master_grid`, `parse_osm_data_with_frame`, bounds, and scale as `tiler_runtime` master export. Reject OSM server remarks/error payloads; root's normalized OSM closure validation remains mandatory. Use one outer worker and a Rayon pool of one, with the exact master frame, no translated tile frame. Check negative geographical coordinates and bbox-edge node mapping in fixtures.

For sample i:

```
wet[i] = detailed_ocean.intersects(exact_sample_point(i))
dry[i] = (L[i] || C[i]) && !O[i] && !I[i] && !wet[i]
class[i] = wet[i] ? 2 : dry[i] ? 1 : 0
```

When authoritative W is empty, all classes are 0 regardless of local L/C: no invented coastal domain in a proven inland bbox. When W is nonempty, mapped land across the requested master is positive evidence under the agreed L-O-I-W rule; there is no coast-distance heuristic. Ocean always wins even over a mapped inland-water feature, bridge, pier or building. Unmapped/ambiguous inland ESA water gets class 0. Protected dry preserves repaired height and prevents ESA-water carving; wet retains existing zero-surface policy and +2 m conflict rejection. No change to these consumer semantics.

## Exact sample-footprint encoding

Production-use audit: `CoastalPolicy::classify` is private in `src/coastal.rs`; the sole production caller is `CoastalPolicy::capture`. Other calls are unit tests. Capture evaluates only the exact regular master lattice:

```
lon = west + column * (east-west)/(width-1)
lat = north - row * (north-south)/(height-1)
```

Therefore polygon declarations can represent exact classes on this lattice without claiming continuous coastline equivalence. Bind the frame and verify it on use, as above. Any future continuous classifier would require a separate contract review.

Build merged horizontal runs of wet cells and wet|dry cells. Trace the union boundaries of those run rectangles in doubled integer grid coordinates, preserving holes, or use existing geo unary_union on exact integer-valued run rectangles; never repeated quadratic pairwise unions. Finalize one MultiPolygon per mask before mapping to geographical coordinates. Interior edges lie at half-sample coordinates; outermost edges are the bbox boundary. A first/last edge cell has half a sample interval of positive width, not a zero-width polygon. Reuse identical edge-coordinate mapping between water/domain, no epsilon offsets, snapping, or ring simplification. Serialized geometry must satisfy the existing production validity/coverage checks.

A nonempty authoritative W may fall entirely between all master samples. In that case retain literal unsimplified W as the declaration's water geometry and union it with the dry footprints for the domain. For disjoint pieces concatenate exact polygons; when the dry footprint already covers W retain that footprint. Only a true intersection requiring overlay uses BooleanOps; its result must pass the unchanged strict coverage and exhaustive sample tests, or fail explicitly. The geo overlay backend quantizes floating coordinates, so it must not touch disjoint literal W needlessly. This preserves a nonempty water declaration without inventing a wet sample or claiming a W-empty query. Mark mode `literal-ocean-no-wet-sample`. It may hit the existing geometry limit; then fail explicitly. In both encoding modes, reparse the serialized bytes through `CoastalPolicy::parse` and use the actual `classify` implementation to compare **every** master sample against expected classes. A mismatch, invalid polygon, missing/extra edge sample, or altered river-mouth priority is an error before publication.

## Bounded offline execution and failure

The standalone worker has no network path and root launches it with existing offline supervision (network namespace disabled). Proposed descriptor ceilings: 600 seconds whole operation, 8 GiB memory, 20 GiB disk, 8 MiB logs, 64 tasks, 256 file descriptors, 64 filesystem nodes including attempt directories, one worker; input manifest 1 MiB, OSM 512 MiB, coastal geometry envelope 64 MiB, 1000000 input geometry vertices, 16777216 master cells. Output classification retains the existing **16 MiB and 100000 total geometry vertex limits**; report at most 1 MiB. The scoped admission validator checks both required input declarations and their 512/64 MiB size bounds before opening any source file. Shared admission checks the deadline before each 64 KiB source hash chunk; the original stock wrapper supplies no-op hooks. Input file size is checked before bounded read and hash. Validate counts with checked arithmetic before allocation. Build compressed bitsets and indexed polygon bounds/run boundaries; do not allocate cell polygons or a full floating point object grid. Bound intermediate run/edge counts explicitly at 1000000 and fail if exceeded, before allocation. These are explicit resource ceilings, not a guarantee every legal profile bbox fits; complex legitimate geometry may require a later separately reviewed capacity change. Never reduce coverage to fit.

Deadline starts before manifest admission and covers hashing, parsing, OSM evidence, geometry operations, serialization and exhaustive replay. Check between loops/operations; geo/serde calls cannot be interrupted internally, so root's hard process deadline and cgroup memory limit are mandatory backstops. No catch-and-continue on allocation/resource failure. Long outside-bbox OSM segments are subject to the same abort policy; do not introduce a new approximate line clipping algorithm.

Publish only verified bytes to a private sibling staging directory, fsync files/directory, then atomic no-replace rename to absent destination. Reject existing files/directories/symlinks and parent traversal. Prepublication failures remove staging. Postrename parent-directory fsync failure returns a specific durability error and retained output path; caller must reject nonzero even when output exists. Reuse a small private publication helper extracted from provider capture only if this does not broaden its behavior; otherwise keep the existing tested primitive with a scoped wrapper. No existing candidate artifact is overwritten.

## Test-first implementation sequence

### 1. Wire admission and command boundary
Files: `src/tiler_contract.rs`, `src/main.rs`, new `src/coastal_classification.rs`, new `src/coastal_classification_tests.rs`.

- [x] Add failing tests for role/key admission, strict envelope bbox/profile/dimensions/query validation, fixed argv, missing input, unknown flag, stock dispatch and unchanged nine-capability descriptor.
- [x] Run the focused test filter and preserve RED output.
- [x] Implement minimal bounded envelope/admission and descriptor/dispatch; keep production command incomplete until its pipeline exists.
- [x] Verify focused tests; send root exact descriptor/envelope/report schemas for typed adapter coordination.

### 2. Shared evidence extraction
Files: `src/land_cover/osm_land_override.rs`, `src/element_processing/natural.rs`, `src/coastal_classification.rs`, tests.

- [x] Add actual-helper fixtures for road/building/railway land, relation holes, pier/bridge/floating veto, water area/channel veto, tunnels, negative coordinates, open/closed/relation cliffs, and oversized-ring behavior. Assert extracted cells equal cells used by current generation geometry, not duplicate expected Python masks.
- [x] Preserve RED witnesses for the new producer/helper boundary.
- [x] Extract pure helpers while keeping existing stock call ordering and output behavior; integrate producer masks in the exact master parser frame.
- [x] Run both existing osm_land_override/natural suites and new fixture filters. Investigate any stock differences before proceeding.

### 3. Exact polygons and consumer frame binding
Files: `src/coastal.rs`, new `src/coastal_classification_geometry.rs`, producer tests.

- [x] Add failing tests for wet priority, dry promenade/building/cliff height preservation, bridges/piers/crossing roads staying wet, elevated inland lakes/channels/unknown water unchanged, river mouths, holes, negative coordinates, all four bbox edges and corners, single edge cells, and complete W-empty query.
- [x] Add sub-sample W fixture requiring literal fallback; assert no invented wet sample and nonempty water declaration. Add mixed wet/dry/ordinary fixtures and checkerboard/resource rejection.
- [x] Implement run-union encoding and production parse/classify exhaustive verification, without exposing a continuous classification public API.
- [x] Add bad-dimension/profile/bbox rejection on production load/capture; preserve legacy source declarations. Run existing coastal +2 m and negative wet elevation regressions unchanged.

### 4. Boundaries, publication and provenance
Files: producer, optional small shared publication helper, tests.

- [x] Add failing tests for input/vertex/run/output limits, whole deadline at hashing and replay checkpoints, hash tampering, incomplete query, invalid geometry, existing destination/symlink, prepublication cleanup, postpublication durability error, and deterministic output classes across repeat runs.
- [x] Complete exact output hashes and report; bounded read of every input, no network construction; fixed argv rejects normal rendering flags.
- [x] Run focused tests and actual offline command with authenticated synthetic inputs. Re-admit output into a synthetic final source manifest and load/capture through production consumer, proving checks on use.

### 5. Review and delivery

- [ ] Run `CARGO_TARGET_DIR=/var/home/bazzite/arnis-v31-coastal-classification-target cargo test --no-default-features --locked coastal` and relevant natural/land-override filters, save logs and inspect results.
- [ ] Run full `cargo test --no-default-features --locked` under the same isolated target. Existing translator test needs authorized fixture network outside sandbox; this is not coastal live acquisition. Preserve any failed attempt, do not exclude the test.
- [ ] Run `cargo fmt --check`, `cargo clippy --no-default-features --locked --all-targets`, and `cargo build --release --no-default-features --locked`; report baseline warnings separately.
- [ ] Send final diff and evidence to parent/independent review before commit. Commit only reviewed issue22 changes, push fork, open draft PR against `feat/21-provider-capture` with exact upstream/test evidence.
- [ ] Rebuild from clean committed head, capture SHA, descriptor, unchanged nine-capability report and clean status. Parent owns final inventory and qualification; preserve earlier releases.

## Design review checkpoint

Parent full written design review passed with the budget correction applied: retain the existing 512 MiB OSM source bound and existing 8 GiB/20 GiB offline SupervisionLimits instead of creating narrower input/process limits. Independent design review passed; runtime implementation was authorized. Parent approved the sample-footprint representation and the literal-W fallback for nonempty ocean between samples, contingent on exhaustive serialized equality, authenticated unsimplified input and frame checks on use. Parent and independent written-plan approval was received before runtime edits. Implementation may resolve routine code structure details within these semantics; wire/schema or coverage changes return to parent for coordination.


## Implementation evidence (2026-09-11)

Evidence directory: `/var/home/bazzite/arnis-coastal-classification-evidence-2026-09-11`.

- RED: source role admission, missing descriptor/producer, actual OSM positive evidence, shifted frame, omitted ocean reference, literal-W union precision, and durability error behavior.
- GREEN: 37 coastal-filter tests; full headless initially 811 passed, 6 existing ignored; final shared-admission rerun 813 passed, 6 existing ignored. Full run includes the existing authorized translator network fixture; producer fixtures use no network or world generation.
- `cargo fmt --check`, `git diff --check`, locked all-target clippy passed; only 15 baseline warnings (11 test-target duplicates).
- Actual fixed-argv offline synthetic invocation emitted descriptor, classification and report under `interface-fixture`; parent received exact protocol paths. These carry interim dirty source identity and are not final candidate artifacts.
- Manifest presence of `coastal_geometry` requires both fixed source references in the policy, even if an edited declaration omits the ocean reference; bbox and dimensions are then checked at capture. Legacy manifests without this role retain their original behavior.
- Final independent spec and parent quality reviews passed. Clean committed release evidence remains before delivery.

- Final independent review found and corrected pre-admission accounting: generic manifest admission used to hash sources before the coastal payload limits. Two focused RED witnesses prove required oversize must fail before source open and a deadline must interrupt an additional-role source hash between chunks. The scoped validation/checkpoint helper preserves whole-manifest authentication and stock wrapper behavior.

- Final shared-admission regression: 813 passed, 6 existing ignored, 0 failed (53.86 s); authorized translator fixture passed. Final locked all-target clippy and formatter passed with baseline warnings only. Parent authorized commit/draft PR after this verification; final source identity comes from the subsequent clean build.
