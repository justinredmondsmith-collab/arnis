# Selected v3.1 world fixes implementation

Owning issue: arnis#2. Stacked on reviewed ABI draft PR#5, commit
825d49b3ba314656983c0bd5ccce4316b3346ff0, exact upstream v3.1.0.
The authorized migration design and all-23 patch audit live in tiler PR#82.
Project tracking is waived by the maintainer. Old refs/worlds/binaries stay intact.

Use subagent-driven-development for bounded implementations and independent reviews,
with failing block-level regressions before behavior changes. Do not cherry-pick old
patches mechanically. Keep changes scoped to the selected invariants; no aesthetic
traffic-signal palette change or global all-water sea-level clamp.

- [x] Negation-aware leisure water guard and natural water-label suppression,
      preserving actual water dispatch and pools.
- [x] Explicit external surface-only water policy: retain surface water, omit
      renderer bathymetry/bed/dunes/decoration; protect occupied non-water surfaces.
      Keep stock behavior with no external ownership policy. Prove OSM and LC
      truth tables and road/tunnel preservation in common and regional paths.
- [ ] Source-aware coastal protection using master water references, preserving
      elevated inland water and real cliffs; resolve any required metadata extension
      explicitly in the contract rather than inventing an unrecorded per-tile mask.
- [x] Multipolygon piers: deterministic fragment assembly, inner holes, invalid
      ring handling, negative level/layer guards and master-anchored support positions.
- [x] Supported interior foundations and no surface fallback for tunnel roads,
      retaining upstream behavior when its tests establish the invariant.
- [ ] Run focused then full headless tests, locked release build, formatter/clippy,
      stock comparison and independent reviews; publish a separate draft PR.

The full capability list stays empty until all advertised ABI/profile invariants and
qualification harnesses pass. This stage alone does not authorize installation or
Minecraft 1.20.1/NYC qualification claims.
