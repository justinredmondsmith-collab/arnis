# Development RC1 build and evidence recipe

**Frozen unqualified development RC1.** Source: `a3d39ca44a78a6539cc920f5f4b029818859cc21`; verified immutable ref: `refs/tags/tiler-v3.1.0-dev-rc1`. Final executable SHA-256: `e6957b956396c288b1ac3017b618d897eabf96be828769b03b3efc131a2ed2db`, size 49,444,376 bytes. Capabilities are empty; final pinned headless tests and binary smokes passed; remote tag resolves to the pinned source. PR9 lint and benchmark passed; broader all-features/GUI build was pending at this development freeze and its final result is tracked on PR9. No production, Java 1.20.1 or successful Battery shoreline qualification is claimed.

## Exact build inputs and command

1. Obtain an independent clean checkout of the pinned source/ref; verify its Git tree, Cargo.lock, Cargo.toml, all tracked assets and source-recipe hashes against the tuple. Preserve previous renderer artifacts and worlds.
2. Use the official versioned Rust `1.97.0` toolchain selected by checked-in `rust-toolchain.toml`. Its compiler, Cargo, bundled linker and support-library binary hashes are recorded in `toolchain.json`; the former moving `stable` location is not the pin. The three reviewed build-only inputs are hashed individually: `rust-toolchain.toml`, `.cargo/config.toml`, `.github/actions/setup-arnis-linux/action.yml`.
3. Checked-in `.cargo/config.toml` supplies `SOURCE_DATE_EPOCH=1788828777`. This deliberately derives from initial coastal commit `940a587ee2fda3537add7d7cb735c27ce2e51935`, **not** the later build-pin commit timestamp. An externally supplied different value overrides Cargo's default environment and must be rejected for this candidate. On the pinned x86_64 GNU Linux host, use:

   ```bash
   cargo build --locked --offline --release --no-default-features
   sha256sum target/release/arnis
   target/release/arnis --tiler-capabilities
   ```

   No explicit `--target`, `--bin` or root feature is added. Release settings are `lto="thin"`, `overflow-checks=true`. Verify the expected executable hash and exact parsed capability report, including clean source identity. Never use an old executable after a failed build. Installed GCC/GNU ld and bundled LLD versions are captured; original final linker argv was not retained.
4. Use a fresh separate target directory for any new independent reproduction. Existing reproduction evidence was: two independent fresh fixed-epoch builds of coastal 940a587 matched, then both independent clones checked out build-only a3d39ca and rebuilt using their separately built dependencies. Final a3d outputs match complete bytes/SHA-256. **The final a3d runs were not two cold builds.** Logs and paths are bound in the tuple. Byte reproducibility was checked on the recorded host in independent source/target trees; cross-host native-toolchain reproducibility is not claimed.
5. Final a3d39ca validation now passes: 714 headless tests, 6 ignored; format; isolated CLI smoke with 1,024 readable region-only chunks; 1,024 normalized chunks equal to exact stock; synthetic provider checks and repeated real-provider export. Exact logs are hashed in the tuple. Earlier stage evidence remains attributed to coastal 940a587. The broad suite includes live Overpass; do not call it hermetic or general terrain parity.

## Source recipe and prior binary distinction

The six-entry provider recipe binds exact input sizes/hashes, including empty synthetic OSM and the coastal declaration, for a 108-cell decoder check. Reacquisition uses network; export checks run with isolated network/caches. Its previously repeated grid SHA is `f98b1b63488ae6cfdf0d6fb763f7d01b91297bd5afd040d6586f1b8654852f4c`; the final-binary rerun verified the same grid hash.

The original coastal PR executable SHA `81e964454d9d35311c24313bfa00b922c97a38c6be9b0b49e897cd0904a4e616` is preserved under `original_pr_artifact`, not used as this RC's checksum. It failed clean-clone reproduction because libmimalloc-sys 0.1.49 options.c embedded C __DATE__/__TIME__. Fixing the epoch changed the build input; subsequently pinning the build-only source commit changed embedded source identity.

## Invalidation and qualification limits

Preserve the immutable tuple and its pinned qualification fixtures. Determine invalidation by the changed input:

| Changed input | Identity consequence | Required evidence |
| --- | --- | --- |
| Renderer source, dependency/lockfile, compiler/library/linker, target/features/flags, epoch or embedded assets | New renderer RC tuple; preserve RC1 | Rebuild and capture identity; affected focused regressions, full headless tests and isolated stock/provider smokes; two-build comparison for changed build inputs |
| ABI, grid, profile, capability semantics or output layout | Version affected contract/profile and matching producer/consumer tuple; new affected master/cache namespace | Positive/negative admission, corruption/bounds, export/render/seam and relevant compatibility checks |
| Tiler-only implementation, unchanged renderer contract/profile | New tiler commit and generation identity; renderer RC can remain unchanged | Affected consumer/cache/orchestration/assembly/retarget tests; downstream evidence binds the new tiler commit |
| This RC's pinned fixture bytes, recipe or hashes | New fixture version and evidence; revised candidate/evidence tuple because those hashes belong to RC1 | Exact readmission, source/decoder rejection and repeatability checks; rerun affected golden/seam/stock checks |
| New project location/bbox or independently admitted source dataset, unchanged contract/profile | New project source manifest, master/tile/cache and generation identity; **not a new renderer RC merely because location changed** | Coverage/hash/classification preflight plus bounded project-quality checks; never mutate RC1's pinned fixture or inherit qualification for untested data |

Matching version text or printable strings is insufficient. New project input is distinct from changing the frozen qualification dataset; a newly admitted dataset does not retroactively qualify Battery or any other rejected source.

Battery bbox `[40.7027,-74.0186,40.7042,-74.0166]` was correctly rejected: repaired DEM 13.585780053858947 m exceeds the approved coastal-water +2 m limit at (-74.0186,40.7042). No master/world was emitted. Preserve the threshold and failure; acquisition completeness/coarse-sampling/repair investigation remains outstanding. Newly hashed OSM cache bytes do not inherit the mismatched historical source identity.

Consumer/cache/resource enforcement, full terrain/seam/concurrency/failure-recovery qualification, Java 1.20.1 vocabulary/NBT conversion, bounded NYC/Forge validation and qualified publication/install remain separate gates. Empty capabilities intentionally prevent full-contract admission.

## Stage-status supersession

Coastal implementation and focused reviews completed at 940a587. Current coastal validation supersedes earlier stage statements that coastal implementation was pending and completed-but-unchecked implementation items in the world/coastal plans. Earlier test counts and five-source recipe hashes remain historical, commit-scoped evidence. The three-file a3d39ca build pin changes no renderer behavior; it does not turn the rejected Battery candidate into qualified evidence or close downstream qualification.

`rc-tuple.json` binds all supporting JSON manifests. `SHA256SUMS` binds the final bundle files; preserve this frozen bundle. Later CI or qualification outcomes belong to a separately identified evidence addendum.
