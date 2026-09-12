# Coastal repair scope and decision gates

Maintainer authorization: stay with our app; create/red-team a plan and start execution. Issue renderer #27, app #90. Retain upstream v3.1.0 and the qualified master-coordinate architecture. Java 1.20.1 is the target; user owns Minecraft tests.

Desired outcome: the original failed coastal area builds in the updated GUI with correct shoreline, real dry land and infrastructure retained, common sea height across tiles, and preserved independent inland waters. No threshold bump, invented ocean closure, bbox shrink or per-tile water decision.

Three alternatives: (A) fix an evidenced acquisition/classification/repair defect, preferred if demonstrated; (B) explicitly revise the conservative coastal contract if sound inputs expose an invalid assumption, with new provenance and both consumer/producer changes; (C) raise the threshold or suppress the guard, rejected because it may flatten real land. We cannot select A versus B before the full repaired-grid diagnosis.

First implementation checkpoint is diagnostics only: exact frozen-input failure replay and test-only characterization using the real AWS sampler and existing repair functions. No runtime behavior changes before the real-data findings and refined fix design are reviewed. This is a deliberate staged design, not a claim that a final patch is already known. Proceed under the user's instruction to plan, review and execute without another routine permission request.

## Refined repair after source audit

The frozen ocean equals the clipped complement of complete OSM coastline rings
(0 square metres difference). Independent AWS decoding reproduces the Rust
samples. All 11,550 repaired-grid conflicts are covered by complete explicit OSM
water/bay relations (2389630, 2389633, 2389632). This supports alternative B: a
versioned source precedence rule, not a sampler correction. Both mapped sources
share OSM lineage; corroboration is not independent ground truth.

New policy `master-coastal-water-v2` keeps the +2 metre rejection except where a
master sample is simultaneously classified wet, covered by authenticated ocean
geometry, and covered by a complete valid explicit OSM water or bay polygon.
Only that intersection may normalize finite conflicting elevation to sea level.
V1 documents retain their existing behavior. Nonfinite classified samples still
fail. Inland samples, ocean holes, protected dry terrain and independent inland
water retain their existing processing. Piers/buildings are not subtracted from
ocean; normal structure rendering must be qualified above the common sea level.

Corroboration is derived by the consumer from authenticated master OSM and ocean
sources, never trusted from a caller-provided boolean. Accept explicit closed
water/bay ways and complete multipolygon water/bay relations. Stitch ways only
through identical node IDs, never invent a closure or use clipped renderer
rings. Require all geometry members and coordinates, valid rings, and correctly
assigned holes; ambiguous, missing, incomplete or invalid areas supply no
corroboration. Suppress standalone corroboration from member ways of every candidate water relation, valid or invalid, so outer ways cannot refill relation holes. V2 loading unconditionally requires both source references and authenticated source bytes; parse-only policies cannot enable corroboration.
Compute using the geographic master lattice before tile processing.

The producer emits v2, and active renderer/app profile content identifies v2 and
the corroboration rule. Archived profiles and old source bundles are immutable.
Generate and admit a new derived source bundle with the real producer and new
profile hash; do not relabel old evidence. Tests cover legacy rejection, v2
corroborated normalization, unsupported rejection, malformed/missing members,
holes, dry cliffs, inland water, and whole-versus-tiled final output. Replay the
original bbox and verify Java 1.20.1 conversion and initial/respawn metadata
before replacing the installed app. Independent design and code reviews remain
required. No Minecraft runtime launch; user performs that test.
