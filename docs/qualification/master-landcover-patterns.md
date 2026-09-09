# Master-coordinate ESA land-cover patterns

Renderer issue #16 corrects the existing ground-generation path discovered during
tiler #88 qualification. It retains exact upstream v3.1.0
`3918513acb4e5e9ef4332418531a7c444d2b5acf`. Candidate capability claims remain
separate from consumer qualification and admission.

## Correction

ESA vegetation RNG, surface hashes, coherent noise, climate palettes and snow-edge
noise use the existing master-coordinate helper. Their seeds and salts are
unchanged. Block writes, occupancy queries and traversal retain local coordinates.
The helper returns the original coordinates without an admitted master.

External master generation also records pre-existing occupancy at each column's
effective ground height plus one before any ESA columns run. This uses the road
height override where present. ESA admission consults that bitmap and the existing
snow-cap predicate. Earlier ESA canopies therefore cannot suppress later roots;
pre-existing OSM obstacles and snow still suppress them. This intentionally changes
new external-master vegetation on affected slopes. Other ground, building, tunnel,
bridge and tree guards are unchanged. Stock generation retains its mutable
occupancy check.

The snapshot runs once per `generate_ground_region` invocation, before all chunk
loops. Sequential generation calls this after OSM processing. Native parallel
regions each have a fresh editor and call it once after their own OSM processing.
The existing bitmap costs one bit per iteration cell, at most 2 MiB under the
16,777,216-cell profile cap, plus one linear occupancy scan.

## Regression evidence

`ground_generation::master_pattern_tests` exercises the actual ground generator:

- Flat forest, shrubland, grassland, cropland, built-up, bare, snow/ice, wetland and
  mangrove cover; climate palettes and snow edges; all three steep rock thresholds.
- Legal translated slices, including negative local coordinates, with mixed
  materials and nonzero forest logs and grass.
- Nine genuine ESA spawn seeds linked by actual procedural Oak leaves. Removing
  the prefix root at master `(1087, 683)` previously changed the owned root at
  `(1104, 676)` with a read window starting at x=1088. Heights are
  `[70, 74, 79, 84, 87, 90, 95, 99, 102]`; cardinal slope is zero at every root.
  The corrected test requires all unblocked candidates and equal owned canopies.
- Pre-existing leaf obstruction at ordinary and overridden road heights, snow
  suppression of actual candidates, and snow suppression when an earlier canopy
  wins the snow block's set-if-absent write.
- A stock ground-generator output SHA-256 established on unchanged production
  source: `9320e9462389b6d7358deacf95c36a4b05df764d586afdc07c4174b2dc1ad708`.

Before correction, actual-generator translations failed for all fourteen tested
pattern cases. The nine-root regression separately failed after coordinate
alignment, isolating the admission dependency. Bounded seed-discovery code remains
outside the committed regression suite; the regression uses fixed genuine seeds.

## Limits

The profile disables canopy maps and schematic trees. Their local canopy-slot
hash is outside this correction. Cropland's local irrigation phase cannot admit
external water because the authoritative water gate requires the same cell's
LC_WATER classification. This work neither enables that branch nor changes the
water policy. Tests do not establish general OSM obstruction consistency or
universal equivalence across native 512-block editor merges. End-to-end matrix
results must be bound to the final frozen executable, independently of these
source-level regressions.
