# Frozen source consumption before candidate freezing

Owning issue: renderer #1; supplies the producer prerequisite for tiler #76.
Branch: feat/1-v31-frozen-sources, based on reviewed world fixes 2bd319d / PR #6.
Exact upstream remains v3.1.0 / 3918513acb4e5e9ef4332418531a7c444d2b5acf.
The maintainer authorized completing this migration; Project tracking is waived.

Implement the approved source resolver contract. In integration invocations,
all enabled provider requests consume exact manifest-bound buffers, with no
workstation cache or HTTP fallback. Stock invocations keep their current paths.
This is necessary producer work before RC freezing, not a qualification claim.

- [x] Introduce a bounded resolver over admitted sources. Recheck size and SHA-256
      on the exact buffer returned to the decoder. Reject unlisted requests,
      changed/truncated/grown files, and oversize requests. Preserve typed errors.
- [x] Route AWS Terrarium requests through elevation keys `aws:{z}:{x}:{y}`.
      Route ESA COG byte ranges through land_cover keys `{url}#bytes={start}-{end}`,
      inclusive end. Frozen paths bypass cache reads/writes and HTTP creation.
      Pass request context explicitly through provider code where practical.
- [x] Propagate missing or invalid enabled land-cover/elevation inputs as export
      errors, including paths that stock mode treats as optional. No flat export
      or partially zero-filled land cover can vouch missing frozen sources.
- [x] Bind compiled climate/legacy tree assets to copies in the source manifest;
      determine exact enabled source bytes before claiming this item complete.
- [x] Red/green tests cover exact consumption, mutation after admission, missing
      ranges, decoder failures and cache bypass; then network-isolated export.
- [x] Spec review, quality review, headless suite, fmt, clippy, locked release and
      existing offline stock comparison. Record exact binary/source identities.

The conservative profile stays AWS-only, provider concurrency one, capabilities
empty. Do not add fallback profiles, install a binary, mutate archived inputs,
or claim coastal/MC 1.20.1 qualification in this stage.

Validated producer stage; full qualification remains pending. See [validation](2026-09-07-v31-frozen-validation.md).
