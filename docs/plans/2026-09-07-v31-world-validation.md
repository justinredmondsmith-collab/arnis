# Selected v3.1 world fixes: validation and remaining work

Owning issue: justinredmondsmith-collab/arnis#2. This draft is stacked on ABI
commit `825d49b3ba314656983c0bd5ccce4316b3346ff0` (PR#5), whose upstream base
is v3.1.0, `3918513acb4e5e9ef4332418531a7c444d2b5acf`.
The maintainer authorized the migration and waived GitHub Project tracking.

## Implemented scope

- Water-tagged leisure features avoid land painting; negated water values and
  pools retain their appropriate behavior. Natural bay/strait/sound/fjord labels
  cannot independently flood their geometry; actual water dispatch is retained.
- Tiled renders emit resolved surface water without renderer bathymetry, bed
  materials or aquatic decoration. Both ground generation paths and OSM/channel
  paths honor this ownership. Road masks and occupied non-water surfaces remain
  protected. Loaded master heights, climate and water surfaces are not locally
  re-estimated.
- Multipolygon piers preserve complete parser fragments, assemble valid closed
  rings deterministically, honor holes and negative levels, and anchor support
  spacing to master coordinates. Invalid assemblies fail without invented edges.
- Terrain buildings receive non-overwriting interior foundation support, excluding
  passages, elevated parts and non-terrain generation.
- Explicit underground highways cannot fall back to surface roads when a bore
  cannot fit. Flooded tunnels suppress dry geometry; other tunnel tag values retain
  their classified surface behavior. Road masks and portal eligibility agree.

Focused block-level regressions failed before their fixes and passed afterward.
They cover overlapping pier tiles through the real OSM parser, master context,
water placement truth tables, steep and flat ground, supported building floors,
and actual public highway dispatch. Individual spec and quality reviews and a
final cross-component review approved this bounded scope.

## Verification

On 2026-09-07 with Rust 1.97.0:

- Full headless suite: **681 passed, 6 ignored** using
  `cargo test --locked --offline --no-default-features --bin arnis`.
  The existing translation test requires live Overpass: the restricted-network
  attempt had 680 passes and that one connection failure; the network-enabled run
  passed all active tests. This is not a hermetic qualification result.
- Formatter passed. Clippy passed with the upstream headless warning baseline
  of 15 binary and 11 test warnings.
- Locked offline headless release build passed. The network-isolated CLI smoke
  produced 1,024 readable region-only Java chunks, accepted standalone capability
  JSON and rejected malformed ingress with exit 2. The bounded flat stock oracle
  produced 1,024 identical normalized chunk NBT payloads with integration controls
  absent. This comparison does not exercise terrain, all default settings or NYC.
- `git diff --check` passed.

Logs: `/tmp/arnis-v31-world-tests-network.log`,
`/tmp/arnis-v31-world-tests-final.log`, `/tmp/arnis-v31-world-clippy.log`,
`/tmp/arnis-v31-world-release.log`, `/tmp/arnis-v31-world-smoke.log`.

## Remaining acceptance gates

This draft does **not** close issue #2 or establish an installable RC.
Source-aware coastal classification remains pending. Occupied-land protection
does not prove that a vacant coastal ESA false-positive stays dry. The remaining
policy must use a shared master reference while preserving harbor/marina water,
elevated inland lakes/rivers, real cliffs and cross-tile consistency. The old
global all-water sea-level clamp is explicitly rejected by the patch audit.

A follow-up source investigation found that existing four bands can encode the
resolved result if every render path obeys the master wet mask. Clearing the
land-cover water class alone is insufficient: OSM polygons can still paint water.
Classification must be authenticated by the source manifest and versioned profile,
with explicit coastal/inland coverage rather than inferred bbox-edge connectivity
or proximity. Preserve original DEM and land evidence before master repairs.
The historical cached OSM candidate examined during investigation did not match
its archived inventory checksum and is not admitted as frozen evidence.

Upstream reports were checked during this work:
[coastal terrain gaps #1132](https://github.com/louis-e/arnis/issues/1132) remained
open, while [railway generation #994](https://github.com/louis-e/arnis/issues/994)
was closed. Neither status establishes qualification of this fork.

Capabilities remain empty until the complete contract is implemented and tested.
Frozen provider consumption, consumer/cache/resource integration, Minecraft 1.20.1
retargeting, bounded NYC and Forge qualification, and RC freeze/publication remain
separate tracked deliveries. The installed renderer and old worlds/caches/refs
are unchanged.
