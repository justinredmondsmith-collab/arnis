# Coastal Build Repair Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans for the diagnostic checkpoint; independent plan review is required. Later runtime implementation follows test-driven development and independent review.

**Goal:** Restore the original coastal build in our Java 1.20.1 app without damaging shoreline land or tile consistency.

**Architecture:** Keep the frozen full-master terrain and shared coastal decision model. First reproduce and characterize the conflict at the actual pre-coastal repair boundary. Select a minimal source/repair correction or an explicitly versioned policy revision only after evidence; do not infer the patch from the first near-threshold sample.

**Tech Stack:** Rust renderer, Python application, frozen AWS/ESA/OSM/ocean sources, current typed NBT/1.20.1 validators.

## Scope and identities

Renderer #27; app #90. Branch fix/27-coastal-dem-conflict in /var/home/bazzite/arnis-v31-coastal-repair, from 2952cc7bf96af72e974ac7b96346e4df9cbf1edb. App baseline 3b3098fdb2c61eba86fab96600fda4593de3279e. Preserve installed app and every existing world/evidence directory. No Meld migration or Minecraft launch.

Frozen source manifest: /var/home/bazzite/.local/share/arnis-tiler-v31/work/arnis-tiler/v3.1/source-freeze/1/objects/3c4e041185a347e68c63fbc238cd84cf/sources/sources.json.
Bbox S,W,N,E: 40.688284354832476,-74.01989985008785,40.69345957957617,-74.0114401990945. Dimensions 714×576. Evidence directory: /var/home/bazzite/arnis27-coastal-repair-evidence-2026-09-12.

## Task 1 — reproduce and measure (fully executable first checkpoint)

Files: new test-only src/coastal_diagnostic_tests.rs included under cfg(test) from src/coastal.rs; external replay helper/evidence. Existing src/elevation/providers/aws_terrain.rs and src/elevation/postprocess.rs are called, not copied/reimplemented.

- [x] Run the qualified immutable renderer on the exact frozen inputs with fixed export flags, new disposable output and ARNIS_FETCH_ONLY=1. Record binary/manifest SHA256, argv/environment, exit status and log. Expect the recorded lon/lat and 2.0085670525732713m failure, not a world build success.
- [x] Run existing coastal tests to establish a clean focused baseline.
- [x] Add an opt-in ignored characterization test that uses admit_sources and CoastalPolicy::load, derives and asserts 714×576 through compute_grid_dims at scale 1, calls the real frozen AWS and ESA samplers, saves raw heights, runs filter_elevation_outliers → repair_terrain_anomalies → fill_nan_values, and classifies every sample through CoastalPolicy. Write summary plus per-conflict coordinates/raw/repaired values to an explicitly supplied absent output directory. Call capture and assert its exact retained error and sample value. No environment-driven production diagnostic hook or admission bypass.
- [x] Run the test with only the named local manifest/output paths; compare the first sample against the retained log and compute actual conflict count/range after sampling and EACH repair stage. Independently anchor the expected error in the retained original failure log. Corrected sources must have newly admitted identities; keep the historical bundle unchanged. Assert repairs cannot mutate the raw copy used for comparison. Count protected dry samples independently. Include representative high, shore-edge and off-edge conflicts rather than only the first failure.
- [x] Classify the evidence against ocean geometry and available mapped land/structures. Report unresolved source ambiguity honestly. If a versioned semantics change is needed, refine the design before writing a success regression.

Commands: cargo test --locked --no-default-features coastal_ ; explicit diagnostic with cargo test --locked --no-default-features diagnose_retained_coastal_failure -- --ignored --nocapture. Reuse a dedicated target directory or existing build cache only when no other build is using it. Test runs operate offline on frozen inputs. No generated assets go into git.

## Task 2 — evidence-driven fix (gated, refine after Task 1)

Likely files: src/coastal.rs, src/coastal_classification.rs, src/elevation/mod.rs; modify only the demonstrated cause. If policy changes, include the matching renderer contract/profile and app renderer_contract/source validators in a dedicated app worktree. Do not choose files or exact code before diagnosis.

- [ ] Record the selected cause, rejected alternatives and complete patch design in the spec; independent review.
- [ ] Add a failing behavioral regression for the corrected real-input mechanism plus counterexamples for dry land/cliffs, over-water infrastructure and elevated inland waters. Preserve the existing guard under its existing semantics.
- [ ] Implement the smallest correction. If semantics change, create new identities and invalidate old caches normally; never edit existing receipts or relabel old sources.
- [ ] Run focused red/green tests and independent code review. A plausible diagnosis is not a green result.

## Task 3 — targeted world qualification

- [ ] Build the same complete selection using frozen inputs; audit ocean surface, dry heights and mapped structures against source evidence.
- [ ] Compare whole output with a tile layout placing real coastal boundaries inside owned regions; verify actual blocks/entities, not only region presence. Check an inland-water/dry-cliff fixture and the retained Bowling Green comparison.
- [ ] Run required Rust formatter/headless tests/clippy/locked release build and relevant consumer tests. Broaden tests when a shared contract or master behavior changes; do not rerun the historical matrix merely as a ritual.
- [ ] Convert through the app's real 1.20.1 path and inspect complete output metadata, Player.Pos and respawn, palettes/entities, and a safe initial position. Do not count only a changed SpawnY as spawn verification.

## Task 4 — app delivery

- [ ] Qualify the exact final binary and source identities, package/install through the existing local candidate process, and run an actual installed-GUI build of this selection into a new world.
- [ ] Preserve rollback. Provide the updated app and optionally copy the fresh test world to the known Prism 1.20.1 saves folder without overwriting user saves. User handles load/save/reopen/visual tests; report automated evidence separately.

## Red team checklist

- A +0.008m first conflict may hide much larger conflicts; use the complete repaired grid.
- Coarse elevation can mix land and sea; authoritative geometry also can be stale. Neither wins by assertion.
- A rejected build and a visibly wrong completed build are both failures; do not turn rejection into silent flattening.
- Geometry-only masking cannot safely erase piers/buildings. Preserve water beneath structures and inspect placement.
- Ocean fixes must not flatten lakes/rivers or recreate Meld's per-tile statistics problem.
- New semantics without new profile/source/cache identities would invalidate qualification.
- Building one small world is not district-scale or full modpack qualification.
- This plan intentionally starts execution with a diagnostic checkpoint. The final patch is not designed yet; avoid open-ended parallel product work or promises of an unmeasured completion time.

Independent plan review: approved for diagnostics only, with real ESA sampling, independent expected-error anchors, per-stage measurements and historical/future source identity separation incorporated. Runtime design remains gated. Immutable baseline replay reproduced the exact logged failure; see baseline-result.json.

## First execution checkpoint — completed

The immutable failure replay reproduced the original guard exactly. Existing focused tests: 39 passed. Reviewed opt-in characterization: 1 passed in 118.94 seconds, using four threads and unchanged production sampling/repair functions; initial slow serial attempt was stopped and retained separately. Formatter and diff checks passed. No runtime fix is claimed.

Measured ocean samples: 88,450. Above +2m: 11,430 raw, 11,550 after outlier filtering, unchanged by anomaly repair/NaN fill. Maximum conflict 15.310985329501783m. No nonfinite ocean samples. Dry classification candidates: 95,873, not evidence of a preserved output world. Raw/repaired grids and actual ESA classes are preserved in compressed evidence. The worst-point OSM probe found a nearby coastline way, but is not a complete relation/structure audit; authoritative geometry versus source elevation remains a diagnosis question. Task 2 must resolve that question before selecting a success regression or policy revision.

## Refined runtime checkpoint

Complete source audit selects versioned alternative B. The refined spec defines
v2 authenticated ocean + complete OSM water/bay intersection, with conservative
mapped-boundary exclusion and member-way suppression for valid/invalid relations.
Independent design and native spec reviews found no remaining blockers after the
member-hole and mandatory-source clarifications. Parent code-quality review
confirmed strict source loading and no tile-local logic; a new test-only clippy
warning was corrected. App adapter has independent spec and quality approval.

New behavioral tests were observed red, then passed. An opt-in native geometry
check authenticates the old source bytes and corroborates all 11,550 retained
conflicts without relabeling or admitting the historical bundle as v2. Complete
headless run: 821 passed, one existing online test blocked by sandbox networking,
10 ignored; that sole online test passed when rerun with host network access.
Actual export, final-world comparisons and app delivery remain pending; unit and
geometry success is not a claim that the user's build is fixed yet.
