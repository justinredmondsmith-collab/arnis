# Enabled recycling fallback item-frame sidecar audit

Source base d261d1beda582160ecdba8a05f5874e941f05688, exact upstream
v3.1.0 3918513acb4e5e9ef4332418531a7c444d2b5acf.

The actual recycling generator first tries registered furniture decals. When no
registered pictogram is available and the loot pool has one category, it emits
`minecraft:item_frame` through `WorldEditor::add_entity`. This feature remains
enabled. A multi-category inventory does not get this fallback frame.

| Field | NBT type | Meaning |
| --- | --- | --- |
| id | String | minecraft:item_frame |
| Pos | List<Double> | local target X/Z + 0.5, absolute Y |
| Motion | List<Double> | three zeroes |
| Rotation | List<Float> | two zeroes |
| UUID | IntArray | four words; must retain stable master identity |
| Facing | Byte | 2 north, 3 south, 4 west, 5 east |
| ItemRotation | Byte | zero |
| ItemDropChance | Float | 1.0 |
| Fixed | Byte | 1 |
| block_pos | List<Int> | local target X, absolute Y, local target Z |
| TileX, TileY, TileZ | Int | same attachment position |
| Item | Compound | populated item stack, described below |
| OnGround | Byte | 1 |
| FallDistance | Float | 0 |
| Fire, Air | Short | -20, 300 |
| PortalCooldown | Int | 0 |

All displayed stacks contain `id: String` and `Count: Byte`. Most display stacks
have only these fields. Leather displays additionally carry legacy
`tag: Compound` with `Damage: Int` and optional `display: Compound` containing
`color: Int`; they also carry `components: Compound` with
`minecraft:damage: Int`. Conversion must resolve these representations without
losing damage or optional color. Display categories are glass bottle, paper,
glass (block/pane pool), leather (clothes/shoes), empty bucket (cans), scrap metal,
and green waste. Inventory item compounds are distinct from display compounds;
barrels retain `Items: List<Compound>` with byte `Slot`/`Count` and string `id`.

Source locations: amenities `place_item_frame_on_random_side`,
`make_display_item`, `build_leather_display_item`, `build_display_item_for_category`;
world_editor `add_entity`. This audit is source evidence, not a Minecraft runtime
qualification or downstream converter compatibility claim.
