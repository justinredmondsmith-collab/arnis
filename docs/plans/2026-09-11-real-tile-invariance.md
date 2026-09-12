# Real-area tile invariance investigation and implementation plan

> Use the executing-plans and test-driven-development skills task by task. Parent review precedes runtime edits; direct cause witnesses are required.

**Goal:** Resolve issue25's retained Bowling Green whole-versus-tile block mismatch without changing stock behavior, enabled features, equality rules, the fixed halo or the external-master profile.

**Base:** clean508037ef9bfdf7150c49764cd4465883a3eec33a, exact upstream v3.1.0 3918513acb4e5e9ef4332418531a7c444d2b5acf. Worktree `/var/home/bazzite/arnis-v31-real-tile-invariance`, branch `fix/25-real-tile-invariance`. Old clean binaries, evidence and source worktrees remain untouched.

**Architecture:** Whole-feature decisions must use a common master frame and complete relevant master context; local crops determine emitted writes. First prove which decisions diverge on the actual retained input. Then change only proven decision boundaries, retaining stock paths when external controls are absent. No arbitrary larger halo or suppressed feature is an acceptable solution.

**Tech stack:** Existing Rust parser, bitmap/flood-fill helpers, facade planner, editor and tests; unchanged Python qualification/assembly/conversion harness.

## Evidence and hypotheses

`/var/home/bazzite/arnis-v31-final-app-evidence-2026-09-11` preserves the unchanged56 synthetic PASS and the ongoing unchanged21 supplemental baseline. Bowling Green core64 differs in297blocks across14chunks, no biome or non-section NBT differences. The same retained source exported byte-identical161x167 master bytes. Independent invocation/ownership/frame audits passed. `real-diff-details.json` lists every actual cell/state.

1. Building doors and adjacent full-height facade columns: parts1002205137 and213924949/950. `compute_facade_plan` samples whole segments, but road and footprint bitmaps are currently bounded to the render's local bbox. A missing remote road/neighbor can change a selected facade or corner inside the owned region. This is a hypothesis until actual facade inputs/outputs are compared.
2. Park trees/grass/walls: parser locally filters ways before `sort_ground_fill_areas`, whose slot-preserving area permutation is not stable under filtering. A pure counterexample exists; establish that the actual retained park and barrier order differs before treating it as the real cause.

## Step 1: Direct witnesses, no runtime edits

- [ ] Record actual parsed element order before/after existing ground-area sorting for whole/core64/core80 master slices. Compare common retained park/barrier elements. Use the real parser and sort helper, not a Python replacement algorithm.
- [ ] Record actual facade plans for the three implicated building parts in whole and owning tiles, including front segment, corner, per-segment class/normal and relevant road/neighbor masks. Compare after exact master-coordinate normalization.
- [ ] Reduce each proven cause to a small deterministic Rust regression using actual helpers. Keep an optional ignored/env-path retained-source audit for full attribution if embedding licensed real input is inappropriate.
- [ ] Run RED tests and retain complete failure logs; distinguish assertion failure from harness/compile errors. Do not make runtime edits until causality is demonstrated and parent reviews the concrete design.

## Step 2: Minimal runtime corrections after proof

Expected files: `src/osm_parser.rs`, `src/data_processing.rs`, `src/element_processing/building_facade.rs` and existing context/bitmap helpers only as required by witnesses.

- [ ] If ordering is proven: derive the existing stock whole-master ordering before local feature selection, and consume its stable filtered subsequence in each tile. Preserve stock ordering exactly when integration controls are absent. Avoid inventing a new feature priority.
- [ ] If facade masks are proven: separate master decision context from local write/context crops, reuse exact existing raster geometry and membership rules. Compute decisions from complete master road/neighbor context, translate only outputs. Do not duplicate road geometry or use a guessed search radius. Assess existing admitted master grid/resource bounds; fail explicitly on capacity rather than changing coverage.
- [ ] Preserve contextual lifetime ordering and cache correctness; no stale fill eviction, no unbounded world writes. Verify paths consuming global context versus local rendering.
- [ ] Add repeat, translation, tile boundary, stock and mixed-feature regressions; each identified cause must have RED→GREEN evidence and actual changed-cell reproduction.

## Step 3: Delivery verification

- [ ] Focused regressions, formatter, full headless tests (authorized normal translator network fixture), clippy and locked release. Report baseline warnings separately. Use a new target directory; never overwrite oldcandidate artifacts.
- [ ] Parent and independent review, commit and push only fork, draft PR referencing issue25 and exact upstream.
- [ ] Build locked release from the final clean commit; record source/head, binary SHA, unchanged nine-cap report, provider/coastal descriptors and actual source inventory.
- [ ] Rerun unchanged56 synthetic +real7+wider7+recycling7 on the final candidate. Retain the oldfailed21 as baseline; do not reinterpret subsets as overallPASS.
- [ ] Run real7+recycling7 target conversions and built/water whole representatives with full typed payload/UUID equality and all expected witnesses. Report stock target block mappings explicitly.
- [ ] Parent coordinates final installed-app acquisition/render/assembly/target smoke and package identity. No Minecraft, install/public trust or publication from this workstream.

Only metadata/unit witnesses may overlap the currently running baseline matrix. All native world pipelines remain serial under8GiB/20GiB supervision/Rayon1. A new source or feature mismatch reopens investigation; passing synthetics alone cannot close this issue.

## Proven entry point and approved concrete correction

The actual external entry is `tiler_runtime.rs` parse-with-frame → generate-world; it bypasses `main.rs` priority sorting. Preserve that external whole ordering exactly. The original `real-order-red.json` witness is faithful; the priority-added diagnostic remains labeled separately. The final faithful pair `native-real-order-red.json` and `native-real-facade-red.json` uses the actual entry order. Both assertions fail on the unmodified runtime.

Actual native order: whole grass823196029 and grass823196030 precede fence823196027; park64 fence precedes both grass areas. These first-writer order changes align with grass/wall differences. Actual facade plans: part1002205137 chooses front1 whole versus22 southwest; part213924950 chooses front10 whole versus18 east. Full cached footprint counts also change1089→812 and1749→1020. Missing remote road/neighbor context is directly visible in segment classifications.

Proposed minimal shared-context correction:

1. Add one explicit `MasterGeometry` helper returning the admitted master bbox translated into the current local frame. It may have negative minima; existing bitmaps support those minima. Local editor bounds are unchanged.
2. In external parser mode, use this complete master extent for node/way selection and clipping, retaining the same master geometry and complete original element order for every slice. Stock mode still uses its original bbox and paths. Relations already use master clipping; audit that all element kinds are retained consistently. Never add the main CLI priority sort to external rendering.
3. Derive full-master decision bounds in `generate_world_with_options`. Sort the complete retained master element list using the existing area-slot permutation. Precompute **one** flood-fill cache using the full master decision window, replacing the tile-window cache in external mode. Build road, building-footprint and building-passage context from the same complete master elements and decision bounds. Keep emitted world writes/editor geometry strictly local. Preserve stock mode unchanged.
4. Initially retain the complete sorted master element list through shared context creation and processing; the existing generators/local write checks crop output. This is the smallest correctness change, avoiding a new subtly different selection rule. If profiling shows remote processing dominates, a separately reviewed conservative selection pass may follow shared context creation, preserving ordered subsequence and related-fill lifetimes; it is not required to conceal cost or pass this regression.
5. Existing sequential last-reader/group-aware eviction remains valid because it sees the complete master order and all group members. No second full cache or permanent per-building copy is added. Relation holes and sibling membership keep using existing helpers. Generated local world chunks remain bounded by existing renderer/supervisor policy.

Memory and cost: the admitted master has at most16million cells; each full-master bitmap costs at most2MiB. Cache coordinate vectors use8bytes per retained filled sample plus existing allocator/Arc/map overhead; overlapping polygons can multiply total cached samples, as in the existing full-master whole render. This proposal adds no new arbitrary feature/area cap and does not claim that16million cells alone bounds all overlapping-cache memory. The existing8GiB process and20GiB disk supervisor remain authoritative and failures remain explicit; local-window cache savings are traded for correct whole-feature decisions. Reuse existing cache eviction and one Rayon worker. If parent requires a tighter pre-allocation context budget or streaming per-feature full fills, design that explicitly before runtime edits rather than silently narrowing accepted input. The source itself remains under existing512MiB admission.

First verification after these corrections: both metadata witnesses plus a reduced park/fence ordering regression and whole/translated/owned-boundary facade/door tests. Then run the original retained real whole/core comparison in a fresh evidence output after the old21 baseline finishes. Any remaining cell difference gets a new causal audit; no claim that these two defects necessarily explain all297 until measured.


## Clean checkpoint status

Parent and independent source reviews passed the concrete correction. Faithful retained source metadata tests:2RED→2GREEN; reduced parser/area/fence regression RED→GREEN. Broader master tests:89PASS,2fixturediagnosticsignored; the two diagnostics were explicitly executed separately. Full headless suite814PASS,8ignored (sixexisting plus twoexternalfixtures). Formatter/diffcheck/clippyPASS;15preexisting main warnings and11duplicate test warnings, no new warnings. The initial extra-priority diagnostic and compile-only missing-import attempt remain preserved and labeled under external evidence.

The facade metadata witness updates its context construction to use the same full decision-bound arguments as production. Expected facade values were not changed: exact normalized plan equality remains the assertion. Localworld writes have not been widened. Actual297-cell closure and oldwhole/newwhole equality remain pending fresh clean-candidate native evidence; this checkpoint does not claim final app qualification.
