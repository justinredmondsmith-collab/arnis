# External-master vegetation and surface-pattern checkpoint

This draft checkpoint addresses the finite audited vegetation and surface-pattern mechanisms that disagreed between whole and tiled external-master renders. Upstream remains exact Arnis v3.1.0 `3918513acb4e5e9ef4332418531a7c444d2b5acf`. It builds on PR #13 head `97457ca357b9f193a380387a9808e237f06bf057`; refs #11.

## Behavior

External landuse, natural and leisure processing seed each column from master coordinates, the existing element ID and a purpose salt. Skipping an earlier column no longer changes later random choices. The stock path retains its existing element RNG stream. Audited forest and wetland noise, farmland/cemetery/orchard phases, natural rock hashes, parking surfaces and markings, cane choices, and road/rail tunnel textures use the shared master coordinate frame.

External procedural tree writes are restricted to positions above immutable terrain at the target column. This prevents a previously placed tree from occupying a future root's ground cell. All shared procedural leaf, apex, trunk, branch and micro-tree routes use the guard. Aboveground overlap retains its existing write precedence. The bounded eight-root slope regression demonstrates the original admission dependency beyond the 16-block halo and checks owned canopy output after the correction.

Appearance changes to newly generated external-master vegetation were explicitly approved. The guard trims embedded uphill branches and foliage. The bare-rock hash is translated, but this change does not activate the existing ineffective replacement mix. Parking cars remain excluded by the conservative profile's existing schematic setting.

## Validation

Implementation commit: `234643437ee5e5780ce14bf711a41e98cefe4af9`.
Executable SHA256: `5672cfbc38e14e3941fd0ef3f8abfc04f20f8d9f4ff88d1fea8c6b0c47262459`.
The capability report binds that clean implementation commit with `dirty=false` and empty capabilities. Later documentation commits do not change these frozen executable bytes.

Built using `cargo build --release --locked --offline --no-default-features` in the isolated worktree. Final headless suite: **751 passed, six ignored, zero failed**, including 23 new focused tests. Formatter, diff check, clippy and clean locked release passed. Clippy retains 15 unchanged upstream warnings. Independent specification and code-quality reviews approved the final source; the quality reviewer independently reran all 23 focused tests.

Regression evidence includes 16 pre-change generator failures, a bare-rock control, actual admitted wetland puddles/cane/moss, protected and masked cells, overlapping trees, all 14 procedural species, micro trees and the eight-root slope dependency. The stock-overlap golden was captured before the tree correction. An initial micro-tree test used an incorrect expected height; the corrected fixture separately reproduced the real failure before the guard and passed afterward. The incorrect fixture is not causal evidence.

Each synthetic 85×112 scene was rendered whole and as four core64 tiles with a 16-block halo, using the existing 8 GiB RAM / 20 GiB disk supervision. All 35 renders and 14 assemblies completed successfully. The executable, source manifest, saved master and tiler software identities were checked throughout. All seven owned-content hashes match exactly, including actual block states and other chunk fields.

| Scene | Baseline differing blocks | Candidate differing blocks |
|---|---:|---:|
| water_vegetation | 4,337 | 0 |
| built | 0 | 0 |
| fields_parking | 3,428 | 0 |
| natural_patterns | 8,878 | 0 |
| leisure_patterns | 3,252 | 0 |
| tunnel | 235 | 0 |
| rail_tunnel | 235 | 0 |

Exact source/master/plan/output hashes and material-pair appearance counts are in [vegetation-patterns-results.json](vegetation-patterns-results.json). The fresh water/vegetation baseline was 4,337 differences after the structural checkpoint; the older 4,111 count belongs to an earlier renderer and is not reused here.

Compared with the structural checkpoint's whole outputs, the approved new appearance policy changes 7,692 blocks in water/vegetation, 377 in built, 3,386 in fields/parking, 6,945 in natural patterns and 3,769 in parks. Both tunnel whole outputs retain identical owned content. All seven comparisons have zero changes to other chunk fields.

The additional frozen synthetic recipes reuse the existing provider/master inputs. Fields/parking has farmland at x3..35,z3..108; parking x40..82,z3..48; cemetery x40..82,z55..78; orchard x40..82,z84..109. Natural patterns has bare rock x3..38,z3..108; blockfield x45..82,z3..45; mixed wood x45..82,z52..109. Parks occupy x3..82,z3..108. Coordinates are inclusive rectangle vertices in the 85×112 master, with fixed node/way IDs. These small recipes exercise audited mechanisms, not real geography or provider coverage.

The local evidence bundle retains fixture recipes/hashes, commands, supervision records, baseline differences, final comparisons, test/review/build logs and binary provenance. A checked-in reusable qualification harness remains a separate release gate; these private checkpoint scripts do not satisfy that gate.

## Limits

This checkpoint does not establish generic tiled support or qualify a Minecraft release. The actual generator tests and small synthetic whole/four-tile comparisons exercise the audited mechanisms; they do not prove every obstruction dependency or arbitrary geography. Further qualification needs multiple layouts, order and origin cases, cache/resume, representative feature and block-entity coverage, resource evidence, a bounded real-area sample, exact-candidate Minecraft runtime tests, consumer inventory/trust rebinding and reviewed integration.

Capability admission, release publication, installation, full NYC/Fallout, GUI and Meld work remain separate.
