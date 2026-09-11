# Exact provider capture implementation plan (#21)

**Goal:** Give the bbox-driven local app a bounded way to acquire exact AWS and
ESA responses that the existing strict offline renderer will consume.

**Architecture:** Share production request planning and strict decoding with a
capture byte resolver. Capture is a separate acquisition command, never a render
mode. The tiler owns its supervised launch, OSM/climate/coastal sources, combined
manifest admission, and all subsequent world generation.

**Baseline:** `1212613d7d84ecbf0f1606e256907be491e28572`; exact upstream v3.1.0
`3918513acb4e5e9ef4332418531a7c444d2b5acf`. Worktree
`/var/home/bazzite/arnis-v31-provider-capture`, branch `feat/21-provider-capture`.
Owning renderer #21 and tiler #90 read; issue claim posted before this document.
Baseline verified by the preceding clean recycling work: 776 tests passed,
6 ignored; formatter/clippy/locked release passed. No runtime changes yet.
Parent reviews this document before test-first runtime implementation. Parent
coordinates independent review; no additional agents are spawned by this worker.

## Existing behavior and the boundary to share

`tiler_contract::AdmittedSources::resolve(kind, key, max_bytes)` is the actual
frozen response boundary (there is no current type named `FrozenSources`). It
checks listed kind/key, regular file, length and hash on every read. Keep all
these checks and the existing strict source manifest schema.

AWS `fetch_raw_frozen` uses `calculate_zoom_level` and
`frozen_tile_coordinates(bbox, zoom, grid_width, grid_height)`, including the
four bilinear sample pixels and their neighboring tiles. `compute_grid_dims`
sets grid dimensions from the profile's scale 1.0. Stock bbox tile selection is
different and must not replace the frozen planner. Keys remain
`elevation / aws:{zoom}:{x}:{y}`; decoded images remain exactly 256x256 RGB PNG.

ESA `EsaSource::Frozen` bypasses caches and `fetch_range` resolves
`land_cover / {url}#bytes={start}-{inclusive_end}`. It reads the first 65536 bytes,
possibly a separate 65536-byte IFD, external offset/count arrays, and compressed
internal tiles overlapping an inclusive global-pixel bbox. Requests cannot all
be enumerated before receiving TIFF metadata. The strict path rejects missing
layout/data, overlong arrays, oversized decoded tiles, and invalid compression.
Reuse these functions and their arithmetic; do not implement a second TIFF parser
or estimate capture by geographic tile names alone.

## Minimal shared implementation

1. Add an internal `SourceResponseReader` trait in `tiler_contract.rs` with the
   current bounded `resolve` signature. `AdmittedSources` implements it by calling
   its unchanged authenticated resolver. Provider strict entrypoints accept this
   trait; stock `None`/live behavior remains unchanged. The trait is read-only at
   the call boundary; capture owns bounded interior state for sequential requests.
2. In `aws_terrain.rs`, share an `AwsTileRequest` planner containing x/y/zoom,
   canonical fixed URL, existing key and 4 MiB encoded cap. Both frozen rendering
   and capture call it. Extract the current strict PNG decoder so capture validates
   each response using identical limits without allocating a full elevation grid.
3. In `land_cover/mod.rs`, keep `parse_ifd`, `read_ifd_array`, `fetch_range`, strict
   decompression and URL creation shared. Extract the existing header/IFD loading
   and pixel-window/internal-tile selection into shared helpers. A small raster
   window value holds x0/y0/width/height/ppd without allocating pixels; existing
   `EsaPixelRaster::covering` uses the same window helper. Both normal raster
   copying and capture iterate exactly the same required chunk descriptors.
   Capture decodes one chunk at a time and checks overlap/nonzero data without
   allocating the complete raster, class grids, smoothing or shoreline output.
   Pixel sizes across ESA tiles must agree, just as in the current raster path.
4. The new `provider_capture.rs` implements `SourceResponseReader` with fixed
   provider request parsing, bounded HTTP acquisition, on-disk recording and
   duplicate-key replay. Its entrypoint invokes the AWS planner and ESA strict
   chunk walk. No ground generation, master-grid creation, OSM download, profile
   relaxation, generic user URLs, or standard cache access occurs.
5. Add a standalone ingress in `main.rs` before ordinary Args/generation:
   `--capture-provider-sources --bbox south,west,north,east --capture-output ABS`.
   Reject other generation switches, existing output, invalid/profile-exceeding
   bbox/grid dimensions, unsupported ESA latitude coverage and malformed input
   before network or output effects. Profile scale is fixed to 1.0 and dimensions
   are recorded. Expose a separately named capture command/schema descriptor in
   standalone `--describe-provider-capture` output only after implemented; do not
   change `--tiler-capabilities` or its nine-capability schema.

## Bounded acquisition policy (proposed exact defaults)

The command has one immutable capture policy, recorded and hashed in its report;
no arbitrary CLI limit increases or automatic large-area fallback. These are
capture resource ceilings, not claims that every bbox fits:

- One network request at a time; one attempt per unique request, no hidden retries.
- 600 seconds total monotonic deadline, including planning, reads, decoding,
  hashing and replay; each HTTP request uses at most the remaining deadline and
  at most 30 seconds, with a 10-second connection timeout. Check deadline between
  chunks and before publication. Supervisor kills the entire process group at
  the deadline even if library decoding stalls.
- At most 1024 distinct responses and 256 MiB aggregate encoded response bytes.
  AWS response cap 4 MiB; ESA response cap min(requested length, 64 MiB). Checked
  arithmetic before request/allocation; read at most bound+1 to detect excess.
  Stream encoded data through a fixed buffer to a newly-created partial file and
  SHA-256; no unbounded `response.bytes()` call. Decoder reads one bounded saved
  response at a time. Decoded ESA chunks retain the existing 16 Mi-pixel limit.
- Source manifest at most 1 MiB; provenance report at most 1 MiB, with bounded
  header values (8 KiB each). At most 1040 filesystem nodes (including attempt/capture/response directories,
  private home/tmp/cache, manifests and logs alongside 1024 response files). Reject before exceeding limits.
- Tiler acquisition supervision reserves 1 GiB RAM, no swap, one provider worker,
  at most 64 tasks and 256 descriptors, 512 MiB disposable disk including logs,
  and 8 MiB logs. Application capture bytes are hard-counted; existing supervisor
  filesystem usage polling is supplementary and remains honestly identified as
  sampled enforcement. Admission counts existing disposable roots and available
  memory/filesystem capacity before launch.

The renderer's network client accepts only the exact AWS Terrarium and ESA v200
2021 HTTPS hosts and paths generated by shared planners. Disable automatic proxy
inheritance, redirects and content encoding (`Accept-Encoding: identity`). AWS
requires status 200 and a length within its cap. ESA requires status 206, exact
Content-Range start/end, consistent total object length, and exact body length;
200 responses never trigger whole-COG downloads.

For each ESA object, the first response must supply a strong ETag. Subsequent
ranges send If-Match and require identical ETag and total length; missing/weak
validators, changed objects and precondition failures abort capture. Record AWS
ETag/Last-Modified and ESA validators when present. Every response is hashed.
A duplicate key reuses its already authenticated captured bytes and is never
refetched. Capture is a bounded interval of individually fixed provider objects,
not a claim of an instantaneous global snapshot across independent providers.

## Supervisor integration boundary (tiler-owned, required before app use)

Current `renderer_supervisor.run_attempt` unconditionally launches bwrap with
`--unshare-net`. Do not remove that flag, add a general network boolean, or launch
capture using an ordinary unbounded subprocess as a workaround.

Add a separately typed `ProviderCaptureRequest` and `run_provider_capture` policy
in the tiler. Share the existing cgroup, lifeline, filesystem read-only binds,
owned writable attempt namespace, environment clearing, cancellation, parent
exit, memory/task/fd limits, log caps and disk metering. The capture-specific
launcher may use the host network namespace only for the pinned capture binary
and validated standalone capture argv; it has a distinct policy hash and reports
network enabled. It cannot accept arbitrary renderer argv or be used by render
attempts. Fixed HTTPS destinations are enforced by capture request construction
and client validation, not falsely represented as a kernel hostname firewall.
No fallback if namespaces/cgroups/resource admission cannot be enforced.

This renderer issue implements and tests the capture command and its limits.
The parent implements/tests the acquisition supervisor adapter before any app
end-to-end source acquisition. Existing world rendering remains network-disabled
and uses production `run_attempt` unchanged.

## Atomic output and tiler handoff

The requested output must not exist. Allocate a private sibling staging directory
with exclusive creation; only write owned regular files there, never follow a
preexisting symlink. Blob names are content SHA-256, under `responses/`. Identical
content may share a blob, but distinct request keys retain distinct entries.

Write `sources.json` using the existing schema: schema_version 1, exact profile
hash, entries sorted by kind/key, relative paths, sha256 and size_bytes. Only
`elevation` and `land_cover` entries are included. The separate `capture.json`
contains bbox/grid dimensions, capture schema/policy, exact renderer source and
binary identity supplied/verified by the tiler, start/end times, and per-entry
URL/range/status/validator/hash/length provenance. Timestamps do not contaminate
the stable sources-manifest semantics. Combined role completeness and final
source admission remain the tiler assembler's responsibility.

Before success, admit the staged manifest through existing `admit_sources` and
repeat the shared AWS decode/ESA chunk walk with that `AdmittedSources` resolver
only. Require exact request-key set equality with captured entries and identical
validation results. This proves capture completeness through strict offline
replay, without making a world. Flush files/manifests and directory as supported,
then publish the whole staging directory atomically without replacing an existing
destination. If no safe no-replace directory publication is available on the
supported platform, fail instead of silently overwriting. Before publication,
failure or cancellation leaves no completed output/manifest; cleanup only the
current owned staging directory. A post-rename parent-directory fsync failure
returns nonzero `capture_publish_durability` with the retained output path; the
supervising caller must reject it even if complete files remain for diagnosis.
Only exit success plus independent verification permits admission. Forced termination may leave a clearly incomplete
staging directory for the supervisor to retain or remove, never an admitted
result. Never delete or mutate previous candidate artifacts.

## Test-first tasks and review gates

- [ ] Write failing shared AWS planner coverage test at a bilinear tile edge;
  capture request keys must equal frozen production keys, including neighbors.
- [ ] Extract AWS planner/decoder and prove existing strict/fallback tests still
  pass. Validate wrong dimensions, oversized/chunked body and malformed PNG.
- [ ] Add synthetic classic-TIFF and BigTIFF fixtures whose IFD/arrays lie outside
  the initial header, and an inclusive bbox at an ESA/internal tile boundary.
  Record actual frozen requests and assert capture requests match exactly.
- [ ] Extract shared ESA window/layout/chunk traversal; preserve stock cache and
  best-effort behavior while strict/capture remain fail-closed. Test corrupted,
  missing, unsupported, oversized and all-nodata chunks.
- [ ] Implement transport behind a fixture adapter; test exact 206/Content-Range,
  wrong lengths, 200 rejection, ETag mutation, missing validator, overflow,
  deadline, request/byte limits, dedup and disabled redirect/proxy behavior.
  Local deterministic transport fixtures do not contact public providers.
- [ ] Test successful atomic publication and canonical manifest equality on two
  captures with identical responses; test failure after earlier successful
  chunks leaves no published result. Test preexisting/symlink destination safety.
- [ ] Replay captured fixtures using the unchanged strict admitted source reader;
  compare requested key sets and decoded values; make any fallback network client
  panic so offline replay cannot silently fetch missing inputs.
- [ ] Test CLI conflicts, invalid bbox and standalone behavior before side effects;
  validate exact report/provenance schema and preserved nine capabilities.
- [ ] Parent reviews implementation and tiler acquisition supervision separately.
  Run full headless tests, fmt, clippy and locked release in a separate target
  directory. Report preexisting warnings and network-dependent test requirements.
- [ ] Parent schedules one bounded real-provider capture/replay only after both
  sides' resource policy is ready. No world generation, installation, Minecraft
  launch, or public qualification is included in this renderer task.

Initial implementation should not add cache reuse/resume, parallel acquisition,
provider substitution, a second TIFF parser, or a permissive frozen reader.

## Review corrections accepted before implementation

The existing nine-capability report is unchanged. Capture describes itself only
through `--describe-provider-capture` and `capture.json`. ESA nodata rejection
aggregates nonzero pixels across the requested bbox overlap: zero chunks are
valid when another requested overlap contains data; nonzero pixels outside the
overlap do not rescue an all-nodata bbox. AWS computes and checks xs×ys
cardinality before Vec allocation (capture may fail capacity; normal frozen
production retains its current grid behavior). Capacity ceilings can reject legal
profile bboxes and must return an explicit `capture_capacity` error, never shrink
the bbox or sampling grid. At the 16384-axis provider-grid maximum, sparse
Cartesian samples can require vastly more than 1024 responses; even 65 maximal
4 MiB PNG responses exceed 256 MiB. Therefore capture availability depends on
actual exact requests/bytes, not only existing grid admission. No guarantee that
every legal render bbox fits the acquisition budget is made. The actual shared
planner fixture now establishes a legal profile example: bbox
[83.962, 0.009, 83.9988, 0.359] gives 4082×4092 cells (16,703,544), requires
1056 AWS responses at zoom 15, and receives an explicit capture_capacity error
without changing bbox/grid dimensions. Even below 1024 requests, encoded response
sizes can exceed 256 MiB under the existing per-response limits; aggregate bytes
therefore remain a separate admission condition.

Reject unexpected Content-Encoding, duplicate or ambiguous Content-Range values,
and Content-Range totals not greater than the inclusive end. Deadline applies
to planning, hashing, replay and publication as well as HTTP. These conditions
receive fixtures alongside the initially planned tests.

## Implementation and verification record

The shared ESA walk uses a capture-only window with no allocated raster data,
retaining the exact production TIFF parser, requested chunk selection and strict
decoder. `AdmittedSources` authentication remains unchanged behind a small reader
trait. AWS capture uses the existing exact coordinate planner with checked
cardinality before allocation and the existing PNG decoder. Standalone capture
has no path into ground/world generation. Strong ETags use an explicit ASCII-only
opaque-tag grammar; SP/HTAB/DQUOTE/control/obs-text are rejected. HTTP request
construction pins Range/If-Match and identity encoding; automatic retries,
redirects, proxy inheritance and automatic decompression are disabled.

Evidence directory: `/var/home/bazzite/arnis-provider-capture-evidence-2026-09-11`.
The baseline executable rejected the new descriptor command (exit 2). Subsequent
regressions witnessed and fixed short AWS body acceptance, invalid quoted
space/tab ETags, and insufficient filesystem-node accounting. Final focused
capture suite: 20 passed. Final complete headless suite: 796 passed, 6 ignored,
0 failed; includes the existing network-backed translator test run with network
access. Capture fixtures use synthetic transport only, not live providers.
Formatter and clippy pass; clippy has 15 existing unused/dead-code warnings in
unchanged sources. Locked offline release build passed in 1m43s with the same existing warnings.
Final independent spec review and parent quality review passed.

One intermediate fixture run encountered a replaced test-executable pathname
while another cargo invocation relinked it. Binary provenance now hashes Linux
`/proc/self/exe`, binding the actual executing inode rather than a mutable
pathname; the supervising adapter independently revalidates the package path.
The subsequent complete suite passes with this correction.

No integrated acquisition, world generation, installation, Minecraft launch or
candidate/public admission has been performed for this task.
