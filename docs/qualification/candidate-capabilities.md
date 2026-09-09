# Candidate capability audit

Renderer issue [#15](https://github.com/justinredmondsmith-collab/arnis/issues/15),
consumer issue [#88](https://github.com/justinredmondsmith-collab/arnis-tiler/issues/88).
Audited base: `406306268934fe5a25337c709dd95fe71254cac0`.
Exact upstream Arnis v3.1.0: `3918513acb4e5e9ef4332418531a7c444d2b5acf`.

The nine claims below describe implemented ABI mechanisms in the conservative
`nyc-conservative-v1` profile. They do not attest release readiness, universal
whole/tile equality, Java 1.20.1 conversion, or Minecraft load/play results. The
consumer must still bind qualification to the exact executable, renderer source,
profile, inventory, converter, fixtures, and consumer revision before public trust
admission. The maintainer performs the pending Minecraft checks.

## Finite implementation audit

No concrete implementation gap was found in these nine mechanisms. This audit
supports removing the development report's forced empty capability list; it does
not add renderer features or change generation behavior.

| Claim | Implemented behavior | Existing evidence |
| --- | --- | --- |
| `master_grid_save` | Export snapshots the complete processed ground, preserving affine/provider/source/profile metadata and all four bands. `master_grid::save` validates before an atomic replacement and records a SHA-256 digest. | `ground::tests::master_export_roundtrip_preserves_processed_snapshot`; `master_grid::tests::roundtrip_all_bands_and_metadata` and `invalid_replacement_preserves_existing`; frozen export smoke. |
| `fetch_only` | The typed Export action performs admitted frozen-input decoding and master preparation, then saves the grid and returns without world generation. It is not an arbitrary online-fetch mode. | `tiler_contract::tests::complete_export_is_typed_without_io`; `tiler_runtime::run` Export branch; `scripts/smoke_frozen_export.py` exercises successful export and failed replacement. |
| `master_grid_slice` | Loader validates metadata, digest, samples, request identity and bounds; returns the requested half-open four-band slice and immutable full-master elevation context. | `master_grid::tests::nonzero_slice_and_bounds`, `streaming_slice_crosses_buffers_and_rows_in_all_bands`, and corruption/mutation rejection tests. |
| `tile_master_offset` | Strict unsigned integer controls feed the shared master coordinate frame; parser geometry, raster patterns and elevation lookups map back to the same master cells. | `tiler_contract::tests::render_offsets_are_strict_and_bounded`; coordinate transformation overlap/quantization tests; parser-through-generator regressions from #11. |
| `tile_dimension_override` | Explicit dimensions determine the validated slice and local editor bounds. Unsupported sizes and out-of-master requests fail before output creation. | `tiler_contract::tests::render_offsets_are_strict_and_bounded`; `master_slices_reject_invalid_bounds_without_overflow`; `tiler_runtime::ingress_tests::render_validates_grid_and_writes_only_regions`. |
| `tiled_postprocess` | Frozen master preparation applies global water/land/bridge/coastal processing once. Loaded slices are immutable and skip repeated repair or local renormalization. This claim does not establish equality for every possible generated scene. | `ground::tiler_master_tests::master_slice_preserves_shared_height_masks_and_climate`, `coastal_finalization_is_required_and_nonzero_overlaps_preserve_all_bands`, and shared integer-height/water-band tests. |
| `skip_railways` | Validated selector vocabulary gates tracks, bridges, tunnel shells/carving, catenary and related masks/endpoints. The conservative profile still pins exclusion to `subway`; reporting the selector vocabulary does not admit arbitrary profile changes. | Railway tests `excluded_railways_leave_no_visible_blocks_or_carve_points` and `excluded_railways_do_not_contribute_masks_or_bridge_endpoints`; CLI selector validation tests. |
| `no_ores` | Skips all fillground vein passes in both merged and per-region generation. It does not claim to remove every ore-like block, such as explicitly tagged quarry decoration. | `data_processing::tests::no_ores_preserves_stone_in_region_and_merged_passes`, including nonempty stock vein controls. |
| `suppress_tile_metadata` | External tiles suppress maps, signage, branding and global settings; generation writes region output without global world files. | `data_processing::tiler_output_tests::external_tile_writes_regions_without_global_metadata`; `tiler_runtime::ingress_tests::render_validates_grid_and_writes_only_regions`; CLI region-only smoke. |

Implementation entry points are [tiler_runtime.rs](../../src/tiler_runtime.rs),
[master_grid.rs](../../src/elevation/master_grid.rs), [ground.rs](../../src/ground.rs),
[transformation.rs](../../src/coordinate_system/transformation.rs),
[railways.rs](../../src/element_processing/railways.rs), and
[data_processing.rs](../../src/data_processing.rs). Historical frozen export
results and their scope are recorded in
[frozen validation](../plans/2026-09-07-v31-frozen-validation.md) and
[coastal validation](../plans/2026-09-07-v31-coastal-validation.md).

## Report and admission boundaries

The existing [capability template](../contracts/tiler-capability-template.json)
remains unchanged. `capability_report()` supplies the exact build identity and
returns its audited list. An independently written exact-list regression rejects
accidental additions; source identity tests remain separate. Historical RC reports
with empty capabilities retain their original identities and are not rewritten.

`--tiler-capabilities` remains a standalone query handled before stock startup
side effects. Ordinary invocations without integration controls retain stock
rendering. The native renderer still emits Java **1.21.1 / DataVersion 3955**;
consumer conversion to 1.20.1 requires its own inventory and qualification evidence.

The report's `provider_max_axis=16384` and `provider_max_cells=268435456` describe
the provider envelope. They do not override the conservative profile's enforced
**16,777,216 master-cell cap**, **4096-cell tile-axis cap**, source limits, memory
budget, or coastal admission thresholds. The profile and grid validators remain
unchanged.

A frozen candidate must be built from a clean committed tree. Its sidecar records
the executable SHA-256, exact build report, source and asset identities, and build
command. That artifact supplies consumer inventory and automated qualification;
capability claims alone do not install, publish, or trust the executable.
