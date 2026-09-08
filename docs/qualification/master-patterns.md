# Master-coordinate pattern correction

Owning issue: https://github.com/justinredmondsmith-collab/arnis/issues/10.
Exact upstream v3.1.0 base: `3918513acb4e5e9ef4332418531a7c444d2b5acf`.
This change is stacked on RC1 record source `55b3900`; it does not alter frozen
RC1 tags, install a renderer, or claim production capabilities.

## Demonstrated defect and change

The frozen 85×112 building fixture produced 173 different owned blocks in seven
chunks when rendered as four 64-block-core tiles. Repeating the whole render
reproduced its original content, isolating translation-dependent patterns.

Building facade substitutions, damage variation, window and balcony decorations,
rooftop equipment, parking walls, and ceiling lights now use master coordinates
for random seeds and arithmetic phases. `BuildingConfig` captures the editor's
master offset once. Local block placement, footprint membership, terrain queries,
and reserved equipment footprints retain local coordinates.

Procedural tree species and shape selection use the master root coordinate.
Leaf gaps and accent colors use each leaf's master coordinate. The latter is
necessary even when both renders select the same tree shape.

Without an external master, `WorldEditor::master_coordinates(x,z)` is `(x,z)`;
the captured building offset is zero. Existing stock seeds and phase arithmetic
therefore remain unchanged. No process-global RNG state or offset was added.

## Tests

Three tests first failed on the previous behavior, then passed after the fix:
translated facade blocks (including periods four, five and six), rooftop equipment
placement, and complete tree blocks (random species, oak, spruce, flowering oak).
They use offset `(17,29)`, which intentionally does not preserve these phases.
The test editors load real persisted master slices rather than mocking the
coordinate transform. Format, clippy, full headless tests, and locked release
build results are recorded in the issue/PR handoff.

## Remaining coordinate audit findings

These findings are not covered by this building/tree correction and are not
silently qualified by its small fixture:

- The two amenities `surfaces::semirandom_surface` calls still take local
  coordinates, changing mixed parking surfaces. Highway calls are corrected below.
- Highway and railway tunnel shell hashes take local horizontal coordinates.
  Beam bridge support spacing also uses the local `x+z` phase.
- Natural rock variation and wetland edge sugar-cane hashes take local coordinates.
- Landuse cemetery, farmland water, and orchard patterns use local modulo phases.
- Landuse, natural, and leisure use an element-seeded stream inside polygon loops.
  Flood-fill itself computes from complete way coordinates; that alone does not
  prove clipping. However, conditional RNG draws after local world/terrain checks
  can advance streams differently when earlier columns lie outside a tile. This
  needs a translated/clipped fixture before claiming equivalent decoration.
- Interior/loot coordinate hashes are outside the conservative profile, whose
  interiors are disabled. They were not changed.

Whole-versus-tile fixture comparison remains an independent qualification gate;
passing unit tests does not establish arbitrary footprint clipping equivalence.

## Reproduced road follow-up

The first corrected candidate left 78 surface/marking differences along the
fixture road. Highway surface selection now maps all three mixed-surface calls
(including the tunnel repaving pass) to master coordinates. Zebra crossing parity
uses that same frame. Coordinates supplied to placement and terrain lookup are
unchanged. No general polygon RNG redesign is included.

The road regression compares actual multi-lane asphalt and zebra-crossing blocks
under offset `(17,29)`, with the segment starting outside the translated tile.
It first failed on asphalt. After only the hash fix it passed the asphalt and
lane-divider comparison but failed zebra parity; the parity fix makes both pass.
Lane divider dash counters are relative to each original segment and were not
changed. This unit fixture does not substitute for the exact qualification rerun.

The earlier `7784a544` candidate was built before a test-only helper was moved
above an existing test module to satisfy clippy. That move changed the source diff
although it did not alter production behavior. It was therefore not an immutable
source snapshot. The follow-up handoff records a complete patch (including this
otherwise untracked audit), per-file hashes, and binary hash only after all edits.
No source, test, or documentation edits are permitted during its rerun.

## Parser clipping and the final dash phase

The next exact rerun reduced the mismatch to 15 white/asphalt blocks. The parser
had clipped the road's starting node to each tile boundary, restarting the
segment-relative dash counter. A manually translated full segment did not test
that preprocessing step.

The added regression parses real OSM JSON through `parse_osm_data_with_frame`
before rendering. It first failed on a white/asphalt difference at master
`(64,70,50)`. Its cases include a straight segment, a bend, and a source endpoint
outside the master. Each asserts exact retained master geometry, bounded nodes,
and equivalent painted blocks in the tile core.

For non-area highways in an external master frame, tile clipping still decides
whether a way intersects the tile. Retained rendering nodes are clipped in the
master frame before integer translation, preserving the whole render's segment
starts and clipping-rounding behavior. Thus each segment axis is bounded by the
admitted master limit (16,384 samples); arbitrary external source endpoints are
not retained. Stock Local/WebMercator parsing and other element classes keep their
existing tile clipping. Highway segment counters and marking semantics are not
changed. Full headless tests are rerun on this final parser correction before the
next source-and-binary freeze.

This correction qualifies only the owning issue's fixed small comparison when
that external comparison passes. Other coordinate audit findings above remain
limits on broader qualification; they are not release capability claims.

## Final bounded qualification and delivery

The frozen candidate `2f598143539fb4fad1f9295cfe11a14e573ae827bc0e31fc1fb49d5acab83554`
was built from base `55b3900f683f4bf061add2353aed8c2dc2e4cf80` plus the complete
uncommitted patch SHA256
`b5062d1083dfebece4b3fe85aafd6568c0d3392f3ac52afeccab8518bf9654e2`.
Source, tests and documentation stayed frozen throughout this rerun. The adjacent
`master-patterns-provenance.json` preserves the original per-file hashes and
input/output identities. Delivery adds this documentation and evidence only;
production and test bytes still match the tested patch. The binary predates the
delivery commit; no byte-identical rebuild of that commit is claimed.

- **Built fixture PASS:** the 85×112 building/tree/road scene rendered whole and
  as four 64-block-core tiles matches the original reference exactly. All 173
  original differing blocks are resolved. All three assembled hashes are
  `63b5f86264e80a3d941879a449467bfbd203457749a86b499358faf42fd1f3b8`.
- **Water/vegetation fixture FAIL:** four tiles differ from the whole render by
  4,111 blocks. Whole assembled hash:
  `6fd01fa503efcbd068be59c9a3448068ebaf2eb77eb3216d66bd947b1359fed8`.
  Tiled assembled hash:
  `f4ba65a555be7a01d8ff1f923a12b386e311b270368d531ef9d37439e94437d2`.
  Landuse/natural clipped RNG streams and local tunnel hashes remain a separate
  qualification blocker; this delivery does not claim that fixture passes.

Final code verification: **719 tests passed, 6 ignored**; formatter, clippy,
`git diff --check`, and locked headless release build passed. Existing headless
unused/dead-code warnings remain. The final delivery-only additions received a
fresh diff check; no renderer rebuild was needed for documentation.

Development evidence: `/tmp/arnis-v31-parser-seams-27e88f19/results.json` and
`differences.json`. This resolves the fixed built-scene acceptance in issue #10.
It does not qualify general tiling, change the empty capability report, publish a
release, merge branches, install a renderer, or mutate canonical worlds.

Separate unresolved water/vegetation blocker: [renderer #11](https://github.com/justinredmondsmith-collab/arnis/issues/11).
