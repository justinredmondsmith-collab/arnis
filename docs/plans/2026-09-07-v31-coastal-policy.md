# Source-aware master coastal water policy

Owning renderer #2, stacked on frozen producer d11a9bf / PR #7 and selected
world fixes PR #6. Exact upstream remains v3.1.0 / 3918513acb4e5e9ef4332418531a7c444d2b5acf.
The approved 23-patch audit explicitly selects declared coastal/inland classification,
shared master surfaces, dry coastal protection and elevated inland/cliff preservation.
The maintainer authorized completion; Project tracking remains waived.

## Contract decision

Keep ARNTGRID2 four bands. Add a required source manifest kind water_classification,
key master-water-classification. Bind its semantics by adding coastal_policy =
master-coastal-water-v1 and coastal_max_dem_conflict_m = 2.0 to the draft conservative
profile. This changes the profile hash; all old development masters are incompatible.
No qualified profile/artifact exists yet, and capabilities stay empty.

Classification JSON has exactly schema_version=1, policy=master-coastal-water-v1,
bbox=[south,west,north,east], default_classification=inland, sources=[{kind,key}],
and coastal_domains=[{id,domain,water,inland_exclusions}]. Geometry fields are GeoJSON
MultiPolygon objects (type,coordinates only), longitude/latitude coordinate pairs.
Require finite valid closed rings and holes; reject invalid/overlapping domains,
water/exclusions outside their domain, water/exclusion overlap, missing source
references, duplicate IDs/keys, bbox mismatch and unbounded geometry. Empty domains
are allowed for an explicitly declared inland-only fixture; within a coastal domain,
water is nonempty and exclusions may be an empty MultiPolygon. Declaration bytes
are capped at16MiB, with <=100000 vertices and <=1024 domains. Geometry validation
must happen before provider requests/output creation. References bind provenance,
not independent proof that the producer's coastal/inland assertion is correct.

The domain includes coastal water and adjacent dry land; water is its authoritative
wet footprint. Inland exclusions retain the ordinary inland policy. Default inland
is an explicit acquisition assertion, never inferred from missing coastline tags,
low DEM, bbox-edge connectivity or distance to ocean. A bounded OSM-derived polygon
may be prepared separately and reviewed; never invent closure of incomplete ways.

## Implementation tasks

- [ ] Strict frozen declaration parser and deterministic master-coordinate classifier.
      Require source resolution on exact authenticated bytes. Reject unsupported or
      contradictory declarations with actionable errors before fetching.
- [ ] Resolve coastal policy only on the complete master. Before land-cover coastal
      smoothing, retain finite repaired DEM heights for declared dry coastal cells;
      after repairs these cells remain dry without lowering terrain. Accepted coastal
      water uses real sea elevation zero and the final affine sea_level_y. A claimed
      wet cell whose repaired DEM is >2 metres above sea fails with coordinates rather
      than flattening a possible cliff. Apply coastal classification before affine
      scaling and reassert it after OSM/bridge repairs, preserving the same dry heights.
      Inland cells (including explicit exclusions) retain their existing independent
      elevations; no global sea clamp. Recompute all processed LC bands once afterward.
- [ ] Loaded master wet mask/surface governs OSM polygon, waterway, film and carve
      placement in external mode; rejected dry cells cannot be repainted by OSM.
      Preserve occupied land, road/tunnel guards, pools, stock behavior and renderer
      surface-only bathymetry ownership. Loaded tiles never rerun classification.
- [ ] Red/green tests: coastal false-water dry land; harbor/marina; elevated lake and
      river; real dry cliff; conflicting wet DEM failure; invalid geometry/source;
      consistent nonzero overlapping slices across all water placement routes.
- [ ] Update producer/consumer draft contract/profile fixtures and synthetic export
      sources for the new required declaration. Record hash incompatibility explicitly.
- [ ] Independent spec then quality reviews, full tests/fmt/clippy/release, offline
      synthetic/real provider and stock oracles, bounded coastal qualification evidence.

Ordinary-height Java1.20.1 retarget and full NYC/Forge qualification remain separate.
No installed renderer, archived source/cache or canonical world is mutated.

Coastal domain pairs must be disjoint even at boundaries; water and inland exclusions must also not touch. Ambiguous boundary cells are rejected rather than assigned by input order.
