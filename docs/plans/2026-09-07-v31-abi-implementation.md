# Arnis v3.1 minimum tiler ABI implementation plan

> Use superpowers:subagent-driven-development for bounded implementation tasks and independent spec/quality reviews. The maintainer has authorized execution; Project tracking is waived.

**Goal:** Implement arnis#1 against the reviewed contract in arnis-tiler#73 on exact v3.1.0.

**Architecture:** Typed integration requests enter before stock CLI side effects. A versioned atomic master artifact supplies immutable aligned Ground data. Generation receives explicit ownership/output policies. Stock invocation wrappers keep upstream defaults.

**Tech stack:** Rust 2021, Clap, serde JSON, SHA-256, tempfile, existing Arnis headless test suite.

Authoritative contract: sibling `arnis-tiler-wt-v31/docs/plans/2026-09-07-arnis-v31-renderer-contract.md` and its JSON schema/profile. Source-hook notes are supplementary and predate final contract amendments; the contract wins.

## 1. Capability and strict controls

- [x] Add tests for standalone capability invocation without bbox, no banner/network/output; combined args rejected.
- [x] Add `src/tiler_contract.rs` with typed export/render requests, pure parsing, strict profile/control checks and exit mapping.
- [x] Add build identity tracking in existing `src/build.rs`; source archives need explicit build identity, never invented hashes.
- [x] Wire early dispatch in `src/main.rs`; do not advertise unimplemented capabilities.

## 2. Master artifact

- [x] Add failing tests for exact four-band roundtrip, nonzero-offset slice/affine/climate retention, corrupt/truncated/wrong-version/oversized inputs, invalid metadata and atomic replacement.
- [x] Add `src/elevation/master_grid.rs` using exact ARNTGRID2 contract, checked sizes, full streaming integrity verification, and bounded slice reads on the admitted file handle.
- [x] Add only necessary locked dependencies; no broad cargo update.

## 3. Ground and coordinates

- [x] Add tests for immutable shared cells and exact OSM alignment between neighboring slices.
- [x] Factor fallible master preparation, preserving source/fallback and global floor metadata; apply master OSM and bridge repair before publication.
- [x] Construct Ground directly from processed elevation/landcover/distance/blend/climate/snow bands; no tile refetch or renormalization.
- [x] Introduce dimension-aware coordinate transform and OSM parser wrapper for tile samples; stock wrapper remains unchanged.
- [x] Export exits before world creation; rejected input never falls back to stock fetching or flat ground.

## 4. Domain and output policies

- [x] Add block-level tests for railway exclusion across track/bridge/tunnel/catenary paths and both ore generation paths.
- [x] Add `--skip-railways`/`--no-ores`, typed predicates, and gates at every relevant call site.
- [x] Add external-tile output policy to coordinator and internal editors; test no maps, signage, branding frames, settings or global metadata.
- [x] Propagate generation/output errors in integration mode rather than reporting success.

## 5. Verify and deliver

- [x] Run targeted tests after each test-first increment, then full headless Rust suite.
- [x] Run locked release build, rustfmt and clippy; compare baseline diagnostics.
- [ ] Validate complete capability report against schema only when all claimed invariants are implemented.
- [ ] Run full fixed stock/default terrain oracle; bounded flat smoke passed. Do not qualify or install if required evidence remains missing.
- [x] Independent spec then quality review completed; delivery evidence is in the validation report.

This draft delivers the implementation above with deliberate incomplete capability admission.
See `2026-09-07-v31-abi-validation.md` for exact evidence and downstream qualification limits.
