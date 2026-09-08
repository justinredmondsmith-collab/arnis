# External-master geometry and terrain checkpoint

This draft checkpoint fixes inconsistent polygon coverage, terrain reads and tunnel endpoint classification between external tiles. It is not a qualified renderer release. Upstream remains exact Arnis v3.1.0 `3918513acb4e5e9ef4332418531a7c444d2b5acf`; the implementation builds on PR #12 head `4d215d9ffd7240258cb94dfb8c98e2ade48e882e`. Refs #11; #11 remains open for the remaining tiling work.

## Behavior

Standalone polygons and assembled building/water relation rings use one master clipping frame. External strict-interior fills crop integer cell coverage to the supplied window using exact rational scanlines. Cached fills, fallback relation fills and building footprint masks share that policy; hole subtraction and standalone-building priority are retained. Water keeps its inclusive edge convention, evaluating crossings in master coordinates. Ordinary railway centerlines retain master geometry so clipping does not restart tunnel-light spacing.

Ground retains immutable full-master elevations from the same checksum-validated payload buffers, shared through `Arc`. The retained band is at most 64 MiB; transient allocation overhead is additional. Reads translate to master coordinates and clamp at master edges, while local write bounds, masks and road override precedence remain unchanged. Highway endpoint classification also uses translated master bounds: a tile read-window edge cannot turn a real portal into an underground continuation.

The stock path without external-master controls retains its existing algorithms. No capability, trusted-release registry, profile or installed executable is activated or replaced.

## Verified result

Final executable SHA256: `36efede75c46563e99c25f022cd9021c5143a1a86b47324bf348d3d5c26c5839`.
Implementation commit: `37b081ff32f205f9f7a267f60b9f19c9a4cd7bc8` (initial geometry/terrain commit `6555806dd2b251364d52b736e388e5b8e5adbef3`). Its capability report records that implementation commit, `dirty=false`, and empty capabilities. Documentation commits after the freeze do not change those executable bytes; no rebuild from a later documentation HEAD is claimed.

Built with Rust 1.97.0 using `cargo build --release --locked --offline --no-default-features`, in an isolated worktree with its own target directory. Headless tests: **728 passed, six ignored, zero failed**. Formatter, diff check and clippy passed; clippy reports 15 warnings in unchanged upstream code, with 11 duplicated by test compilation. An initial sandbox run failed an existing network-dependent translation test; the final network-enabled run passed it. Independent specification and code-quality reviews approved both implementation commits with no blocking findings.

Each synthetic 85×112 scene was rendered whole and as four core64 tiles under the existing 8 GiB RAM / 20 GiB disk supervision policy, then assembled and compared by actual owned block states and other chunk fields. Inputs, software, command lines and binary hashes were checked throughout. The final candidate was held unchanged for all four scenes.

| Scene | Before | Final | Result |
|---|---:|---:|---|
| Slanted fixed-material polygon | 10 differing blocks | 0 | Missing polished-andesite cells restored |
| Road tunnel | 276 differing blocks | 235 | All 41 structural differences removed; exactly the original texture differences remain |
| Ordinary rail tunnel | 240 differing blocks | 235 | All five misplaced lights fixed; exactly the original texture differences remain |
| Original built scene | 0 whole/tiled differences | 0 | Whole/tiled equality retained |

All final comparisons have zero other chunk-field differences. Source, master, plan and output hashes are in [geometry-context-results.json](geometry-context-results.json). The rail fixture derives from the original tunnel line by replacing highway=service with railway=rail, retaining tunnel/layer, coordinates and IDs. The polygon uses landuse=education with master points [(0,0),(84,73),(84,110),(0,110)] and closure; its baseline loses the owned cell at (23,18,20). These focused fixtures contain no supported block-entity runtime coverage.

The built scene's new whole output differs from the previous candidate by 158 vegetation/air blocks (grass, leaves, ferns and flowers), with no other chunk-field changes. The new raster visitation order changes consumption of existing element RNG streams. This is recorded appearance drift for newly generated external-master worlds, not proof that the remaining vegetation determinism problem is fixed.

## Corrected causal finding

The earlier width80→85 tunnel experiment changed both available terrain and endpoint classification against the tile boundary. It did not isolate terrain alone. The first implementation candidate (`6478e73994788add7590de08c58e2779306ad42e9618fc60e4165a54b31fc77e`, clean commit `6555806`) supplied full-master terrain and passed terrain tests, but still reproduced all 41 structural differences. Its polygon and rail-light regressions passed.

The follow-up held terrain constant and reproduced both false internal-endpoint classification and downstream tunnel-profile divergence. Both tests failed before the boundary fix and passed afterward; true master-edge continuation remains covered. The final executable removes all 41 original structural differences, with no new differences. Full terrain access remains an independently tested correction. The initial candidate and all baseline evidence are preserved; the local diagnosis report carries this correction explicitly.

## Remaining gates

This is the completed structural checkpoint, not generic tiled support or a Minecraft-ready release. Known local-coordinate textures, noise, element-stream RNG and vegetation eligibility/overlap behavior remain unresolved. Public capability admission stays disabled. The next implementation block must address those finite audited mechanisms before separate qualification.

Later qualification still needs multiple tile layouts/order/origin cases, cache/resume, representative feature and block-entity coverage, resource/size evidence, a bounded real-area sample, exact-candidate Minecraft runtime checks, consumer inventory/trust rebinding and reviewed integration. A direct translated-water raster comparison would strengthen the current ring-geometry test. Full NYC/Fallout transforms, GUI work, installation and Meld features remain outside this checkpoint.
