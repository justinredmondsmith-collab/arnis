# Coastal policy validation

Renderer #2, stacked on frozen-source producer d11a9bf. Exact upstream Arnis v3.1.0
3918513acb4e5e9ef4332418531a7c444d2b5acf. This stage is not NYC qualification.

The draft profile binds master-coastal-water-v1 and its +2 m wet DEM conflict limit.
Canonical profile SHA-256 is
02e80bc12bdc17483b94031ca1f78a842371226c8f997909f8fded5adfe2f81b.
Earlier development master grids are incompatible with this profile.

## Behavior and review

Authenticated, bounded classifications distinguish declared coastal water and dry
coastal ground from independently leveled inland water. Coastal raw-height checks
use the repaired DEM before land-cover leveling, then restore the protected height
and class after OSM/bridge repairs. Rendering uses the immutable master water mask
and saved surface. Swimming pools and swimming areas retain their explicit path.

Spec review caught and resolved negative wet DEM rejection and natural-way water
painting outside the mask. Quality review identified procedural wetland, mangrove,
and cropland water as additional paths requiring the same guard. Regression tests
reproduce these failures before their fixes. Stock behavior remains separately checked.

## Provider and coastal evidence

Synthetic 108-cell export checks exercise real PNG/TIFF decoding, known heights,
classification admission and conflict failures, and preservation of an existing
master on failure. The tiny real-provider fixture has six checksum-pinned inputs;
two network-isolated exports are byte-identical, grid SHA-256
f98b1b63488ae6cfdf0d6fb763f7d01b91297bd5afd040d6586f1b8654852f4c.
This is provider repeatability evidence, not shoreline qualification.

A separate Battery shoreline acquisition candidate at bbox
[40.7027,-74.0186,40.7042,-74.0166] was classified using oriented complete OSM
coastline segments. Four partitions had consistent water/land side probes, with
no unresolved partitions or internal dangles. The cache's original acquisition
query is absent, and its current hash differs from the archived identity; this is
new, unqualified source material. A coarse bay relation overlaps declared dry land.

Offline export with all required provider inputs correctly rejects the candidate:
at longitude -74.0186, latitude 40.7042, repaired DEM 13.585780053858947 m exceeds
the approved +2 m coastal-water limit. No master or world was emitted. Do not
raise the limit or alter classification just to make this fixture pass. Investigate
source coverage, coarse DEM sampling and repair effects during qualification.

Local evidence: /tmp/arnis-v31-battery-coastal-export/ and
/tmp/arnis-v31-coastal-declaration-candidate/ retain the exact manifest, classification,
source provenance and rejection log. These are not historical baseline artifacts.

## Verification

- Full reviewed headless suite: 714 passed, 0 failed, 6 ignored. The existing live
  Overpass test failed under the network-restricted sandbox; the network-enabled
  rerun passed. This broad suite is not hermetic.
- Format and diff checks pass. Python Ruff passes. Clippy passes with the existing
  15 binary / 11 test warnings and no new warnings.
- Producer/consumer profile JSON agrees byte-semantically. The consumer's qualified
  capability schema correctly rejects this development renderer solely for its
  deliberately empty capability list.

- Locked offline headless release build passes.
- Network-isolated release CLI smoke emits 1024 readable Java chunks with only
  region output. All 1024 normalized chunk NBT payloads match exact stock v3.1.0
  on the established flat geo-only oracle, not a general terrain parity claim.
- Release synthetic and real-provider export checks pass with the hashes above.

Logs: /tmp/arnis-v31-coastal-full-reviewed.log,
/tmp/coastal-decoration-clippy.log, /tmp/arnis-v31-coastal-release-reviewed.log,
/tmp/arnis-v31-coastal-cli-stock-reviewed.log and
/tmp/arnis-v31-coastal-export-reviewed.log.

Capabilities remain empty; the installed v2 renderer and canonical worlds are unchanged.
The final clean commit/binary identity is recorded on the draft PR after rebuilding.
