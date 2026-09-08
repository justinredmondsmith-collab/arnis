# Frozen provider validation

Owning renderer #1, supplies tiler #76. Exact upstream v3.1.0 commit
3918513acb4e5e9ef4332418531a7c444d2b5acf, stacked on world fixes 2bd319d.
This producer stage does not qualify the conservative profile or install a binary.

## Implemented behavior

Integration uses explicit admitted-source references through Ground, elevation and
ESA paths. Exact bounded decoder buffers are size/hash checked again after admission.
AWS requests include every bilinear interpolation neighbor, read serially, with no
HTTP client or cache path. ESA byte ranges bypass live/cache paths and reject partial
geographic coverage, missing ranges/layouts, incomplete directories and malformed or
oversized decoded data. Stock wrappers keep their prior behavior.

Climate requires the manifest copy of koppen_0p1.bin to match compiled bytes.
Legacy trees are procedural Rust code; schematic tree packs are disabled by this
profile and no external legacy tree asset is consumed. Both export and render check
climate before generation. Failed exports leave an existing master intact.

Encoded AWS PNG is capped at 4 MiB and must decode to 256x256. ESA ranges are capped
at 64 MiB and decoded tiles at 16 MiB. The frozen LZW decoder requires EOI and rejects
invalid codes, early termination and extra pixels. The uncompressed decoder checks
length before copying. ESA requires the whole bbox within [-60,84) latitude;
unsupported world-edge AWS interpolation fails explicitly.

## Evidence

- Headless full suite: **692 passed, 0 failed, 6 ignored**. Includes existing live
  Overpass connectivity test, so this broad suite is not described as hermetic.
- Focused source tests: **11 passed**, with red/green regressions for postadmission
  mutation, missing sources, AWS interpolation neighbors, incomplete ESA coverage,
  incomplete TIFF directories, LZW overflow, decoder bounds and climate mismatch.
- Formatter, diff checks and Python Ruff pass. Locked offline clippy passes with
  existing headless warnings (15 binary / 11 test); no new warnings.
- Locked offline headless release build passes.
- Network-isolated CLI smoke emits 1024 readable Java chunks with region-only output.
  Stock comparison matches all 1024 normalized chunk NBT payloads on the established
  flat geo-only oracle. This does not claim full stock terrain/provider parity.
- Synthetic provider smoke passes: 108 cells, actual AWS PNG and ESA TIFF decoding,
  known 20-metre source elevation, missing/corrupt provider and climate failures,
  and failed replacement preserving an existing master.
- Five real provider fixture sources were reacquired with exact pinned hashes.
  Two network-isolated exports produce identical grid bytes, SHA-256
  a2f9f021184fa8062a6d86d1f87993c917599afe86f4a3ba73e00c3023fd3eed.
  The 108-cell fixture uses real AWS/ESA data and empty synthetic OSM. It proves
  provider decoding and repeatability only, not coastal geometry or NYC quality.

The evidence above describes the five-source recipe at frozen-stage commit d11a9bf.
The current recipe adds the coastal declaration and new profile; see
[v31 coastal validation](2026-09-07-v31-coastal-validation.md) for its distinct hashes.
The source recipe is [frozen-provider-fixture.json](../contracts/frozen-provider-fixture.json).
It records exact source hashes/sizes. Acquisition explicitly uses HTTP and rejects
changed responses; source manifests are only emitted after every file verifies.

```bash
python3 scripts/acquire_frozen_provider_fixture.py /tmp/new-provider-fixture
python3 scripts/smoke_frozen_export.py target/release/arnis --real-sources /tmp/new-provider-fixture
python3 scripts/smoke_tiler_cli.py target/release/arnis --stock /path/to/exact-stock-v3.1.0
```

The smoke uses bwrap network namespaces and a disposable writable directory. Source
review and frozen-path tests establish cache/HTTP bypass; namespace isolation alone
is not evidence of zero attempted network calls. No production cache was modified.

Local logs: /tmp/arnis-v31-frozen-full-tests-reviewed.log,
/tmp/arnis-frozen-halo-clippy.log, /tmp/arnis-v31-frozen-release-reviewed.log,
/tmp/arnis-v31-frozen-cli-stock-smoke.log, /tmp/arnis-v31-frozen-export-final.log,
/tmp/arnis-v31-fixture-acquisition.log. Independent spec and quality reviews approved
this bounded stage after their decoder, coverage and interpolation findings were fixed.

## Remaining gates

Capabilities remain empty. Source-aware coastal policy, full renderer/assembly seam
qualification, resource enforcement, frozen candidate tuple, Java 1.20.1 vocabulary
and NBT conversion, and isolated Forge client/server validation remain downstream.
The installed v2.8 renderer, archived references, caches and worlds are preserved.
