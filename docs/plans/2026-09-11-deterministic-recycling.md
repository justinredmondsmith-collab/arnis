# Deterministic external-master recycling (#19)

Base: d261d1beda582160ecdba8a05f5874e941f05688; exact upstream v3.1.0:
3918513acb4e5e9ef4332418531a7c444d2b5acf.

## Diagnosis and design

The actual amenity generator uses an unseeded RNG for barrel inventory and display
items. Its fallback item-frame helper independently shuffles sides, then filters
against local write bounds. Seeding alone would still select different sides at
tile boundaries. Inventory and enabled furniture displays must remain present.

Only when `editor.master_geometry()` exists, seed recycling randomness from the
source element ID and master anchor coordinates using existing `coord_rng`.
Keep the stock unseeded path otherwise. Pass the generator RNG into fallback side
selection; retain a separate thread RNG for stock side selection to preserve its
existing independence. In master mode filter candidate attachment positions by
master bounds, then allow existing `add_entity` local bounds to enforce ownership.
Never reroute a frame because its selected position is outside this tile.
Override recycling frame UUID in master mode using the existing deterministic UUID
helper with master target coordinates, absolute Y, and facing; expose that helper
crate-wide if needed. Shared entity behavior remains unchanged.
Keep decal precedence, loot probabilities, item contents, local block writes,
halo sizes, and stock side filtering unchanged.

## Test-first implementation sequence

1. Add actual `generate_amenities` fixtures using the existing translated master
   editor helper and real processed recycling nodes. Collect full barrel and
   item-frame compounds through `into_world`; normalize coordinate-bearing fields
   to master space and retain UUID in exact semantic comparisons.
2. Demonstrate failing repeated and translated inventory/display comparisons.
   Exercise single-category fallback and multi-category retained contents.
3. Demonstrate owned-boundary equality: compare whole output with the union of
   adjacent local-write windows, including an anchor outside one owner and a
   frame inside it. Add master-edge placement and stock generation assertions.
4. Implement the scoped RNG and side-bound changes, rerun focused regressions.
5. Audit full typed frame sidecar fields (including Item, Facing, block_pos,
   TileX/Y/Z, UUID, Pos, Rotation, Motion) and send concrete converter evidence
   to parent. Do not change consumer schemas or strip display features here.
6. Run headless tests, fmt, clippy, and locked release build in a dedicated Cargo
   target directory; distinguish existing upstream warnings. Parent schedules
   independent review and integrated qualification before any freeze.

No Minecraft, installed replacement, integrated world rendering, assembly,
public admission, origin push, or release publication is part of this change.

## Verification evidence

Evidence directory: `/var/home/bazzite/arnis-recycling-evidence-2026-09-11`.
The initial actual-generator run failed repeat, translation and owned-boundary
assertions (1 pass, 3 failures); all-category expansion also failed before runtime
changes (1 pass, 4 failures). Final focused suite passes all six tests, including
mixed populated inventory with no fallback frame and a nonvacuous crossing from
an outside anchor into an adjacent frame owner. Complete UUID values participate
in the semantic equality assertions.

Final full `cargo test --locked --offline --no-default-features --bin arnis`:
776 passed, 6 ignored, 0 failed. The first sandboxed attempt failed the existing
network-backed translator test; the unexcluded rerun with network access passed.
`cargo fmt --all --check`, `cargo clippy --locked --offline --no-default-features
--bin arnis`, and `cargo build --release --locked --offline --no-default-features
--bin arnis` passed. Clippy/release report 15 existing dead-code/unused warnings
in unchanged args, canopy, elevation/cache, land_cover, map_preview and models
sources; none originate in changed files. Cargo target directory is isolated at
`/var/home/bazzite/arnis-v31-recycling-target`.
