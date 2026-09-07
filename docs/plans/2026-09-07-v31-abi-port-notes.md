# Concrete v3.1 renderer ABI port notes

2026-09-07; read-only source investigation. Input contract:
`arnis-tiler-wt-v31/docs/plans/2026-09-07-arnis-v31-renderer-contract.md`.
All upstream anchors are commit `3918513acb4e5e9ef4332418531a7c444d2b5acf` at
`/var/home/bazzite/arnis-v31`. Proposed new paths below do not exist yet.
No code, builds, tests, network or world outputs were produced.

## Resolve these contract gaps before advertising all capabilities

1. **Surface-only water is not implemented by upstream.** Profile says
   `upstream-surface-tiler-bathymetry-v1`, but stock water placement calls
   `water_depth::carve_water_column`, which writes depth, bed materials, dunes
   and vegetation. Merely gating `carve_lc_water_pass` leaves OSM scanline depth
   active. Define surface-only precisely (water at resolved surface; no renderer
   carve, bed, dunes or underwater vegetation), include it in the producer
   capability and gate every OSM/land-cover call. Decide whether upstream
   waterways, swimming pools and small water features retain intrinsic geometry;
   blanket suppression of every below-surface water block would break them.
   This is an unresolved scope definition, not an impossible port.
2. **Post-fetch bridge repair mutates elevations.** `main.rs:434–439` calls
   OSM overrides then `Ground::apply_bridge_land_cover_repair`; `ground.rs:423`
   passes `&mut data.heights` into bridge repair. That is not merely local
   shoreline metadata and contradicts loaded tiles consuming unmodified master
   heights. Recommended v1 decision: perform bridge elevation repair in master
   export using full master OSM input, before serialization; never run it on
   loaded tile heights. If the master-once sequence requires this before affine
   conversion, refactor the repair's units explicitly; it currently receives
   Minecraft heights. Alternatively explicitly disable this repair in v1,
   accepting and measuring the visual difference. Do not silently leave it on.
3. **Frozen source manifest is underspecified.** Contract names a JSON file and
   hashes but has no schema, roles, resolution rules, master-versus-tile OSM
   mapping, allowed cache roots, provider-tile inventory or offline miss policy.
   Define these before implementation. Recommended manifest: version, stable
   role/source keys, relative immutable file paths, byte lengths and SHA256,
   provider tile keys/projection, full master OSM source identity, embedded asset
   hashes. Resolve relative to manifest directory; reject unavailable or changed
   bytes without live fallback. Hash exact manifest bytes (or explicitly define
   canonicalization); master/tile identity must use the same manifest, even when
   a tile consumes a subset. Hashes alone do not pin providers if stock clients
   continue fetching arbitrary URLs or reusing unrelated caches.
4. **Ground context is not fully in the grid.** Metadata persists affine and
   water floors but not processed land-cover, climate or snow policy. Land-cover
   postprocess mutates its input; separately fetching/slicing land-cover can give
   different shore masks. `Climate::classify` samples each bbox center from the
   embedded `assets/climate/koppen_0p1.bin`; `snow_threshold_for` uses bbox-center
   latitude. Adjacent tiles can differ despite one master. Specify a master
   climate/snow anchor and a processed master land-cover artifact in the frozen
   manifest, or explicitly admit these as unqualified seam lanes. Full seam
   invariance cannot be claimed from elevation grid alone.
5. **Flat and auto-fallback test profiles lack invocation definitions.** The only
   accepted profile environment value is conservative geo-terrain/aws-only;
   separate flat and auto tests cannot use that exact fixed profile without
   contradiction. Define named fixture-only profiles or clearly test lower-level
   provider/flat routines outside qualified integration invocation. Similarly,
   P/T concurrency variants need explicit profile identity rather than changing
   the fixed profile silently.
6. **Preflight wording needs stages.** Validation of a newly exported grid's
   digest/header necessarily occurs after provider fetch. Read 'all grid-header
   checks before providers' as applying to *input* artifacts; export validates
   request/manifest before fetch, then validates generated samples before atomic
   publication. Existing output placeholder validation also needs specification:
   upstream Java Args requires an existing output parent. Export should accept
   an absolute unused placeholder without creating it and not need an existing
   world directory.
7. **Build provenance isn't self-authenticating.** An environment-provided source
   archive identity can be required but 'authenticated' needs an external trust
   mechanism (signed release manifest or trusted build pipeline). Producer can
   report an explicit identity source and must not attest authentication itself.
8. **Stock parity and world fixes must be scoped.** Applying water, leisure,
   natural and foundation changes unconditionally conflicts with exact stock
   output preservation when integration controls are absent. Gate selected world
   fixes behind the integration/profile context, or redefine parity to exclude
   deliberately changed behavior. Recommended: preserve stock branch unchanged.

## A. CLI ingress and typed integration context

Add `src/tiler_contract.rs` (pure request/profile validation and capability JSON)
and `src/elevation/master_grid.rs` (format and streaming I/O). Register modules
in `main.rs`; the application is a binary crate, with no src/lib.rs or src/cli.rs.

Intercept raw `args_os()` in `main.rs::main:597`, after Windows console attachment
and before GUI dispatch or `run_cli:100`. Exact sole `--tiler-capabilities` prints
JSON and exits. Any occurrence combined with generation args rejects with 2;
never send it through normal Args, which requires generation context. Do not
print banner, query updates or initialize Rayon. Keep stock `--help` behavior
unless deliberately changing it is accepted by parity.

For integration requests, parse known environment controls once into a typed
`IntegrationRequest::{Export,Render,FlatFixture}`. Never let independent modules
reread mutable environment or partially enable hooks. Detect known controls even
without ABI; reject missing ABI, unknown ABI, partial groups, conflicting groups,
invalid profile, invalid strict numeric syntax and unsupported dimensions. New
flags `--skip-railways` and `--no-ores` belong in Args; their stock absent defaults
must be no-op. The contract must say whether using these two flags alone requires
ABI (current wording singles out environment controls, not these flags).

Normal `run_cli` currently configures Rayon, cleans caches, prints banner, starts
an update-check thread, then parses Args. For integration route split a validated
entrypoint before all these operations. Use Clap ArgMatches value_source to
separate explicitly conflicting metadata flags from stock defaults. Construct a
resolved profile Args only after checking explicit supplied options; an absent
map flag must become false while explicit true must reject. Avoid positional
scans that miss `--map-item=true` forms. Retain stock execution path when no
integration controls exist.

Preflight loads profile and source manifest, validates all referenced bytes,
opens and fully validates input grid (Render), checks dimensions/bbox/scale,
resource limits and output path before any provider or directory creation. Own
error mapping: 2 syntax/partial controls, 3 unsupported request/ABI, 1 I/O or
integrity. Normal upstream validate_args currently returns exit 1 and validates
existing Java output parent; separate pure compatibility checks from stock path
checks to avoid wrong exit codes and export placeholder restrictions.

Then set world bounds using `ground::{extended_min_y_for,world_top_y_for}`. Rayon
must be configured only after preflight. RAYON_NUM_THREADS is honored by
`floodfill_cache::configure_rayon_thread_pool:554`, but provider pools are separate.

## B. Export: capture one processed master without constructing a world

Do not enter `world_utils::create_new_world`, which runs before current fetch and
writes level.dat. Dispatch Export directly into a fallible new ground/master
preparation function. Reuse algorithms, not `Ground::new_enabled`'s fallback.

`ground.rs::Ground::new_enabled:203` currently fetches land-cover, optionally
canopy, estimates maximum water depth, derives carve_floor/water_floor/sink_floor,
then calls `elevation::fetch_elevation_data:120`. Its error branch silently makes
flat Ground; forbidden for ABI. Factor a fallible prepared terrain result with
ElevationData, processed land-cover, global floors, provider report and climate
context. Keep existing public wrapper's fallback only for stock calls.

`elevation/mod.rs::fetch_elevation_data` should delegate to a shared internal
routine returning a richer result; stock wrapper returns just ElevationData.
Capture selected provider and every success/error/insufficient-coverage attempt
inside `fetch_raw_with_fallback:264`, where that information exists. RawElevationGrid
currently contains only heights, so it cannot supply provider evidence afterward.
Do not infer selected provider by rerunning selector. Distinguish an insufficient
coverage fallback from an I/O error. Preserve upstream last-provider semantics
for stock; ABI ultimately rejects nonfinite processed samples.

Keep existing order: outlier filter, anomaly repair, fill NaNs, land-cover repair,
scale_to_minecraft and f32 downcast. Export after all agreed master repairs,
including the bridge policy selected above. Capture the returned effective base
and affine, requested/min settings, water/sink floors; compute finite sea datum
from affine. Enforce cell limits and no downsampling via compute_grid_dims before
fetch/allocation. Contract <=16M cells and provider-axis limits are simultaneous
requirements (a skinny >16384-axis master still fails).

Publish ARNTGRID2 with strict typed metadata, deny unknown/duplicate keys,
checked length arithmetic, row length checks, finite f32 samples and digest.
The digest excludes the digest field; writer may reserve 32 bytes, stream/hash
payload, then seek back before fsync. Unique exclusive sibling tempfile, cleanup
only its own path, replace, parent sync. Return failed export if any step fails;
no process::exit(0) inside terrain functions.

## C. Render: admitted grid into Ground and OSM coordinates

`Ground` keeps elevation fields private and existing `set_elevation_data:721`
only replaces heights/dimensions of an already populated Ground; it does not
safely construct affine-aware Ground. Add a dedicated fallible constructor from
validated slice plus explicit companion land-cover/climate context. Set enabled,
base level, ElevationData world/grid dimensions, heights, affine and effective
ground_level; derive snow from agreed master anchor. Set canopy=None for profile.
Call set_base_chunk_y and set_terrain_floor_y exactly as
`generate_ground_data:939` does. Never call new_enabled, provider fetch, scale,
water-floor estimation or debug output on loaded slices.

Reader keeps one opened file through full digest/finiteness validation and slice
seek; do not reopen a path between admission and sampling. File replacement is
then harmless, but concurrent in-place mutation still requires immutable-file
admission or a private copy/lock. A path stat alone does not establish immutability.
Reject malformed headers before allocating; stream full validation in bounded
memory, then allocate only requested rows.

Add an explicit dimension-aware local CoordTransformer constructor using
lengths width-1,height-1, not floor((width-1)/scale)*scale. Thread the validated
frame into `osm_parser::parse_osm_data:802` via a new internal function/optional
context; preserve the existing wrapper for stock and GUI callers. It currently
constructs its own transformer at :817. Also use this frame for any spawn mapper
(main.rs:483), though tile metadata profile should reject user spawn options.
Do not change global compute_grid_dims based on environment: it is also used by
land-cover/canopy and would accidentally change provider dimensions.

Supply exact derived tile bbox and validate at 1e-10 degrees. OSM coordinates
must use the same frame. No per-tile bridge height mutation, rotation or scaling.
Source loading must use manifest subsets and fail closed; existing OSM/Overture/
ground parallel scope in main.rs:334 must not initiate uncontrolled sources.
Land-cover water/land overrides may stay only under the explicit context policy
and require seam evidence, because mask changes affect surfaces even without
renormalizing terrain.

## D. Domain gates across internal and merged render paths

Add one railway classification predicate taking raw railway type plus effective
subway tagging (railway=subway or subway=yes). Apply before:

- `railways::generate_railways:70`, including its bridge and catenary branch
  (`catenary_wanted:281`, `generate_catenary:385`);
- `collect_rail_bridge_internal_endpoints:186`;
- `collect_at_grade_rail_mask:423`;
- `add_tunnel_footprint:462`;
- `carve_rail_tunnel_interior:986` and point collection.

Collectors receive elements before per-element dispatch, so an early renderer
return alone leaves phantom protected footprints. Filter through the predicate
in collectors; avoid deleting whole multi-tagged elements blindly, which could
remove another domain's geometry. Separate electrical power ways lack automatic
railway ownership; do not suppress arbitrary power=* infrastructure just because
it crosses excluded tracks. Contract's power-line clause is implementable for
railway catenary; independent power features need explicit semantics if intended.

Ore gates: data_processing.rs:948 generate_ores_region inside tile workers, and
:1243 generate_ores on merged ground. Both require fillground && !no_ores.
Do not gate generate_ground_region or normal stone fill. Thread context/Args to
all paths; use actual block inspection tests rather than flag parser tests only.

Water surface-only gate belongs at shared placement policy, not just one caller:
water_areas::scanline_fill_water:688 has normal carve and tunnel-safe fill;
water_depth::carve_lc_water_region:704 is shared by full/region paths.
Preserve upstream resolved surfaces while removing final-depth ownership as
specified above. Selected non-ABI world fixes remain separately gated/profiled.

## E. Tile output and metadata suppression

Integration Render must allocate an exact disposable tile directory directly;
never invoke world_utils::create_new_world (writes template level.dat and picks
'Arnis World N'). Export does not allocate a tile directory at all. Keep the
existing stock directory creation unchanged.

Add `OutputPolicy::{Standalone,ExternalTile}` to GenerationOptions/WorldEditor,
default standalone at existing callers. Guard all metadata at point of emission:

- data_processing.rs:550 set_map_decals and :556 signage context construction;
- :580 preview epoch/context, :586 place_branding, :587 preview allocation;
- :871 internal tile editor decal flag and :875 signage transfer;
- :1329 branding map/frame placement; :1352 map writing, :1358 branding-only map;
- :1364 signage map emission and :1377 preview finalization;
- :1420 apply_java_world_settings and GUI spawn update below it;
- main.rs post-generation set_spawn_in_level_dat;
- world_editor/java.rs::save_java:91 -> save_metadata, which emits metadata.json.

Do not call settings functions and merely tolerate missing level.dat warnings:
external tiles intentionally have no global metadata. Guards on both coordinator
and internal editors prevent block entities/map frames as well as data/*.dat.
Audit WorldEditor save error propagation: current save wrappers log errors and
may continue; ABI must return nonzero for output I/O failure. Test emitted path
allowlist and NBT entity/block-entity absence, not only flag values.

## F. Build identity and provider/resource enforcement

Upstream Cargo.toml explicitly selects `src/build.rs`; extend it without altering stock output content;
embed HEAD, dirty state, rustc -Vv identity and target plus constant upstream pin.
Track .git indirection, worktree common dir, HEAD/ref, index and relevant source,
assets, Cargo.toml/Cargo.lock/build/config inputs. Explicit archive provenance
must have a defined trust/admission mechanism. Build metadata changes should not
change procedural generation seed; contract/binary pin handles qualification.

AWS `elevation/providers/aws_terrain.rs:75` creates its own Rayon pool with
MAX_CONCURRENT_DOWNLOADS, ignoring global one-thread choice. Add an integration
source execution policy threaded into fetch, or a global request semaphore at
shared network dispatch that covers *all* enabled providers. Serializing only
provider selection still allows concurrent HTTP requests within one provider.
Prefer immutable local provider fixtures for qualification; disable unlisted
cache repair/redownload paths (AWS :350+ re-downloads corrupt cache entries).
Land-cover and any other enabled network clients need the same policy. Climate
is an embedded grid, not a network provider; inventory the asset hash.

Memory preflight estimates must include f64 raw grid, postprocess scratch copies,
land-cover grids, final f32 master, and buffers. 16M cells does not itself prove
8GiB safety. The consumer owns cgroup/process-tree/disk monitoring as ADR says;
producer owns per-request bounded allocations and provider concurrency. Neither
side should advertise the other's runtime enforcement as implemented locally.

## Implementation and verification order

1. Resolve manifest, water, bridge/master-context and profile gaps above.
2. Add pure invocation/profile parser, standalone capability route and build
   identity; advertise partial support honestly until remaining steps pass.
3. Add grid metadata/stream reader/atomic writer with malformed, corruption,
   failure-injection, concurrent publication and affine/slice tests.
4. Factor fallible master preparation and source execution policy; export has
   no world/cache/update side effects outside declared source/cache behavior.
5. Add admitted Ground/frame construction and manifest-based source loading;
   prove exact nonzero-offset seams and no fallback on corrupt input.
6. Add domain gates, surface-only policy and output policy in both execution
   modes. Test ore blocks, railway masks/shells/catenary and metadata absence.
7. Run required Rust tests/build for CLI and GUI compilation where applicable;
   fixed offline stock/candidate semantic oracle verifies no-controls behavior.
   Only then report full capabilities. Pin resulting binary separately and run
   downstream real-fixture qualification; source inspection is not qualification.
