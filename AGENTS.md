# Arnis renderer fork collaboration

Use a dedicated issue and branch for renderer changes. Preserve all v2 branches,
tags, archived binaries, caches and worlds. Record the exact upstream tag and
commit in each migration PR; no unrelated upstream-main drift. Read the owning
issue and the linked arnis-tiler contract before editing.

The maintainer has explicitly authorized the v3.1.0 migration and waived GitHub
Project tracking for this solo project. Keep issue/PR evidence without making
Project access an execution prerequisite.

Minimum ABI work is arnis#1 and references arnis-tiler#73. Selected world fixes
are arnis#2; RC freezing and publication remain #3 and #4. Use separate branches
and PRs for each delivery. No installed renderer replacement, canonical-world
mutation or release publication before qualification.

For the v3 lineage use exact upstream v3.1.0 commit
3918513acb4e5e9ef4332418531a7c444d2b5acf. Follow test-first development for runtime
changes. Run headless tests, locked release build, formatter and clippy; report
preexisting upstream warnings separately. Preserve stock behavior when integration
controls are absent. Contract capability reports must never claim unimplemented
behavior. Use disposable output and fixed fixtures for integration tests.
