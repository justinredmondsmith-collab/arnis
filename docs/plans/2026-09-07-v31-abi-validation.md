# Minimum v3.1 ABI draft validation

Owning issue: justinredmondsmith-collab/arnis#1. Contract: arnis-tiler#73 / PR#82.
Base: upstream v3.1.0, `3918513acb4e5e9ef4332418531a7c444d2b5acf`.
The maintainer authorized execution and waived GitHub Project tracking.

## Implemented and reviewed

- Standalone capability JSON and strict typed export/render ingress before stock side effects.
- Explicit conservative CLI settings; conflicting options rejected before generation.
- Exact ARNTGRID2 four-band atomic master publication, metadata/size/hash admission,
  bounded streaming slices and exact shared master coordinates.
- Immutable Ground samples, affine/floor/climate/snow metadata and master-only repairs.
- Manifest checksum/path admission and checksumming the exact OSM buffer consumed.
- Railway exclusions through track/bridge/tunnel/catenary paths; both ore-generation gates.
- External tile output contains region files without maps/signage/branding/global settings.
- Provider concurrency 1 in the admitted export process, Rayon pool 1 for integration.
- Export destination cannot replace its manifest or admitted source files.

Independent spec and quality reviews found and verified fixes for provider naming,
f64 roundtripping, coordinate and height/blend seams, integrity exit codes, source
archive build identity, export/source collisions, and grid mutation between validation
and consumption. Returned slice bytes now come from the same buffers that were hashed.

## Evidence

On 2026-09-07, Rust 1.97.0, Linux x86_64:

- Exact stock headless suite: **609 passed, 6 ignored**.
- Final fork headless suite: **651 passed, 6 ignored**.
  `cargo test --locked --offline --no-default-features --bin arnis`
  The existing upstream translation test contacts Overpass; the Cargo dependency
  resolver is offline, but this full suite is not a hermetic qualification run.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- `cargo clippy --locked --offline --no-default-features --bin arnis --tests`:
  passed with the stock headless warning baseline (15 binary / 11 test warnings).
- Locked headless release build passed. Stock and fork artifacts must be kept
  separately; sharing Cargo's target directory caused a stale stock build-script
  fingerprint during comparison. Cleaning only the renderer's release package and
  rebuilding fixed it; do not use a failed build's prior executable.
- `python3 scripts/test_tiler_build_identity.py`: passed, including an archive
  nested in an unrelated Git repository and actual worktree identity.
- `scripts/smoke_tiler_cli.py CANDIDATE --stock STOCK`: passed inside bubblewrap's
  isolated network namespace, with fresh disposable XDG caches and output.
  Capability query made no filesystem changes; malformed requests returned 2.
  A synthetic master produced **1,024 readable Java chunks** with region-only output.
  The bounded flat stock oracle produced **1,024 identical normalized chunk NBT
  payloads** with integration controls absent. Compound-key ordering and region
  timestamp headers are normalized; block/entity values are retained.
  Stock rejected the capability probe. This is not the full terrain/default/NYC oracle.
  `--stock` uses `nbtlib`, already a dependency of the sibling tiler environment.
- Python smoke/identity scripts pass Ruff.

Workstation evidence logs are `/tmp/arnis-v31-fork-final-tests.log`,
`/tmp/arnis-v31-clippy-final.log`, `/tmp/arnis-v31-release-final.log`,
`/tmp/arnis-v31-cli-smoke.log`, and `/tmp/arnis-v31-build-identity-green.log`.

## Deliberate limitations and next deliveries

This is a **partial development ABI**, not an installed upgrade or qualified RC.
The capability list is deliberately empty, so the full consumer schema rejects it.
Do not change it to a complete capability list until all advertised invariants pass.

- arnis#2 still owns surface-only water/bathymetry and selected world-fix reconciliation.
- arnis-tiler#74 still owns v3 consumer/cache identity and enforced process resource limits.
- arnis-tiler#75 still owns Minecraft 1.20.1 compatibility; native upstream DataVersion
  3955 remains unchanged. A readable native Java chunk is not proof of 1.20.1 safety.
- arnis-tiler#76 still owns frozen provider consumption, provider-failure injection,
  full stock/default oracle and profile qualification. Manifest preflight alone does
  not prove AWS/land-cover/climate provider reads consumed those frozen files.
- The integration OSM reader currently admits JSON up to 512 MiB; XML and larger
  inputs need a separately reviewed bounded parser policy.
- arnis-tiler#77 and renderer#3/#4 still own NYC qualification, RC freeze and publication.
- No GUI build or playtest is claimed by these headless checks.

The active `~/.local/bin/arnis` still resolves to `arnis.v2.8.0-speed1`.
Old branches, tags, binaries, production caches and worlds are preserved.
