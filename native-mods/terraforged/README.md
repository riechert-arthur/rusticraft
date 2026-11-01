# TerraForged for Rusticraft

An exact Rust translation of
[TerraForged](https://github.com/TerraForged/TerraForged), delivered as a stacked series of small
native-mod changes. This foundation ports the deterministic Noise2D primitives used by later terrain
modules. It intentionally does not alter world generation yet.

The next layer adds TerraForged's octave configuration and Perlin, Perlin2, legacy simplex,
improved simplex and cubic sources, including the original explicit-seed behavior.

The graph layer translates TerraForged's constants, source nodes, core modifiers and arithmetic
combiners. Its tests preserve Engine 0.3.0's reported bounds as well as raw sample bits, including
the original combiner bound behavior.

The domain layer adds direct, axis-noise, directional, additive, compound and cumulative warps,
plus warped module sampling. Directional warps reproduce Engine 0.3.0's 4096-entry sine lookup.

The selector layer adds base blending, fixed and variable blends and bounded selection, preserving
Engine 0.3.0's interpolation, edge conditions and reported source bounds.

The multi-blend layer adds TerraForged's evenly spaced multi-source selector, including its exact
slot boundaries, Java rounding behavior and interpolation across neighboring sources.

The terrace layer adds TerraForged's ramp, cliff and ramp-height controlled step modifier,
preserving its original step spacing, clamping and boundary behavior.

The legacy terrace layer adds TerraForged's asymmetric Curve3 transitions with independent lower
and upper curve controls.

The advanced terrace layer adds masked, sloped, modulated and multi-octave stepping with the
original blend thresholds and normalization range.

The curve layer adds midpoint and S-curves, variable curves, symmetric power curves and configurable
step smoothing with TerraForged's original approximate exponential and mapping behavior.

The scalar graph layer adds coordinate frequency, gradient shaping, directional modulation,
minimum and subtraction nodes, including the original combiner bound calculations.

The cache layer adds TerraForged's shared single-coordinate memoization while remaining safe for
Rusticraft's parallel world-generation workers.

The cellular primitive layer adds TerraForged's pinned feature-point table, distance metrics and
nearest/second-nearest cell searches used by cell value and edge sources.

The cellular source layer adds cell value, lookup, distance and edge modules, preserving the
Engine's single-frequency sampling, feature-point lookup coordinates and reported `0..1` bounds.

The sine source layer adds axis-selectable and blended wave sampling using TerraForged's exact sine
lookup table behavior and `0..1` remapping.

The line source layer adds dynamic radius, endpoint fading, feathering, clipping, intersection and
distance helpers for TerraForged's finite line masks.

The engine type layer begins terrain composition with TerraForged's wrapping seed cursor and full
terrain-category property model, including dominance ordering and derived override behavior.

The terrain descriptor layer adds delegated, configured, volcanic and composite terrain types,
preserving Engine 0.3.0's object identity, Java string hues and original composite-dominance quirk.

The terrain registry layer adds the 16 built-in terrain descriptors, case-insensitive registration,
composite caching, ID fallback and parent-based custom terrain creation.

The world-level layer adds spawn and height properties, validated ocean-to-inland control points and
the exact normalized block-level conversions used by TerraForged's heightmap pipeline.

The world-settings layer completes TerraForged's root settings object with continent generator mode,
shape, scale, jitter, skipping, size variance and octave-distortion defaults.

The terrain-settings layer adds exact global and per-landform weights plus vertical, horizontal and
base scaling controls consumed by TerraForged's terrain distribution and LandForms graph.

The first LandForms layer adds executable steppe and plains graphs with exact octave configuration,
seed consumption, erosion alpha, axis warping, large-scale warping and final vertical scaling.

The hill LandForms layer adds both warped hill variants and the blended dales graph, preserving gain,
lacunarity, alpha, curve, power, clamp, map and global vertical-scale behavior.

The plateau LandForms layer adds the warped valley, ridge top, surface detail and controlled four-step
terrace composition used by TerraForged's plateau terrain.

The terraced LandForms layer adds badlands and torridonian graphs with masks, dual step detail,
modulation, linear and Curve3 slopes, advanced terracing and final boost shaping.

The ocean LandForms layer adds exact level and ground context, ocean/land base accessors and the
deep-ocean hills/canyons blend with its mixed float/double sea-level scaling and six seed increments.

The erosion Ridge layer adds TerraForged's cellular valley-distance graph node with octave controls,
source-value caching and constant, input-linear and output-linear blending modes.

The mountain LandForms layer adds all three ridge and cell-edge mountain graphs, including optional
directional fancy-mountain erosion and the original advanced terrace variant.

The cell-model layer adds TerraForged's complete terrain sample state, Java defaults, copy/reset and
absent-cell behavior, plus the ordered biome-type identities carried by each sample.

The terrain-populator layer adds regular and scaled cell population, weight and variance metadata,
configuration round-tripping, output clamping and Java-compatible NaN behavior.

The region-module layer adds warped natural-distance terrain cells, deterministic region identities,
edge shaping and packed world-space centers, including Java integer-overflow behavior.

The region-selection layer adds weighted populator slot expansion and edge interpolation, preserving
zero-weight fallback, Engine rounding and the upper region's terrain identity during blends.

The volcano layer adds cell-driven cone height, inversion throats, lowland blending and river/water
pipe demotion with exact Engine seed consumption and terrain identity behavior.

The standard-provider layer registers every LandForm and volcano variant, builds all weighted terrain
composites, preserves variant fallback and performs Java-compatible deterministic shuffling.

The heightmap-blending layer adds module- and populator-controlled terrain blending plus two- and
three-way continent interpolation with exact boundary and terrain-tag behavior.

The position-utility layer adds mixed integer/float packing, half-open containment, rectangular
visitation and TerraForged's exact 2D/3D iterator state transitions, including its Y-step sentinel.

The simple-continent layer adds warped tectonic cells, shape modulation, nearest-center and boundary
searches, ocean-distance solving and the single-continent center filter.

The advanced-continent layer adds variable-size warped Voronoi continents, skipping, corrected
centers, cliff and bay shaping, and exact iterative edge and ocean-distance searches.

The fancy-island layer adds scaled line segments, translated island bounds, overlap and containment,
deep/shallow/coast edge shaping and curved land-distance values.

The fancy-continent layer assembles the main island and twelve Java-random satellite islands, then
adds exact multi-scale domain warping, land masks, nearest-center metadata and cell population.

The river-configuration layer adds TerraForged's default river, lake and wetland settings, normalized
river profiles, fork scaling and the original variance semantics used by later network generation.

The river-warp layer adds the paired lake and channel domain transforms used while carving generated
networks, including TerraForged's original seed increments and currently unused mega-warp scaling.

The river-geometry layer adds normalized channel segments, expanded bounds, intersections, shortening,
position sampling and simplex-driven local warping with the original endpoint and wiggle fades.

The river-carver layer adds tapered valley, bank and bed shaping, mouth scaling, terrain tagging,
fork-profile derivation and Java-random valley curves with exact normalized height behavior.

The lake layer adds circular beds, depth blending, shoreline valleys, bank-height interpolation,
river-mask shaping, overlap checks and the float bounds accumulator used by river networks.

The wetland layer adds line-bounded valleys, pools, banks and seeded mound noise, including separate
warped geometry and terrain-noise coordinates plus exact wetland tagging and erosion masks.

The river-network layer recursively composes warped channel carvers, lakes, wetlands and child
branches, including aggregate bounds and TerraForged's branch-overlap checks.

The rivermap layer applies paired generation warps and dispatches terrain samples to bounded river
networks while retaining continent coordinates and cache timestamps.

The base-river-generator layer adds deterministic root shuffling, recursive forks, overlap rejection,
lake and wetland placement, and complete rivermap assembly from Java-compatible random streams.

The fancy-river layer projects roots from island segments and endpoints, extends them through matching
island IDs, and immediately generates their branches and wetlands in the original Java random order.

The river-cache layer memoizes packed continent coordinates with TerraForged's five-minute expiry,
preserves the legacy seed layout, and exposes cached fancy-continent rivermaps through shared handles.

The simple-river layer distributes integer-ended roots around simple and advanced continent centers,
then feeds their lakes, forks and wetlands through the shared Java-compatible network generator.

Simple continents now expose legacy-keyed cached rivermaps, while advanced continents use packed
center keys; both retain their generator settings and share maps across repeated terrain samples.

The heightmap-river stage applies a continent's cached rivermap and then performs TerraForged's
volcano-pipe demotion for submerged or river-cut terrain.

The heightmap cache memoizes packed block coordinates, copies stable cell snapshots, and performs the
same coast-to-beach correction used by TerraForged's direct world lookup.

Contextual heightmap sampling now reuses a current rivermap while the continent center is unchanged
and resolves a replacement only when sampling crosses into another continent.

Contextual cache misses execute base terrain, rivermap carving and climate in Java order, while cache
hits copy the completed cell without rerunning any generation stage.

The filter foundation adds TerraForged's synchronized erosion-instance cache, reusing a filter while
its map size matches and atomically rebuilding it when generation dimensions change.

The smoothing filter preserves TerraForged's in-place circular weighted pass, including erosion-mask,
terrain-edge, river-mask, and elevation-range strength modifiers.

The steepness filter measures each cell against TerraForged's asymmetric sixteen-sample neighborhood,
clamps submerged neighbors to water level, and records the scaled result as terrain gradient.

Beach detection samples coast gradients eight blocks in each cardinal direction and reclassifies only
low-edge, low-gradient coast cells with the shared beach terrain identity.

Hydraulic-erosion primitives reproduce TerraForged's filter settings, paired-seed fast random stream,
and radius-four normalized brush cache, including its reused interior brush layout.

Hydraulic erosion runs TerraForged's deterministic chunk-seeded droplet simulation, bilinear gradient
sampling, weighted erosion, deposition, evaporation, and terrain-aware strength modifier.

The world-filter pipeline caches erosion by map size, runs optional erosion then smoothing, and always
runs required steepness then beach detection in TerraForged's original order.

The tile foundation reproduces TerraForged's block/chunk size, border, masking, indexing, and coordinate
conversion model with Java-width wrapping arithmetic.

Tiles now own TerraForged's region, chunk, and block coordinates plus bordered cell backing, and expose
the same raw linear-index filter region consumed by the complete world-filter pipeline.

Chunk views reproduce TerraForged's masked chunk lookup, world and region block coordinates, sixteen by
sixteen cell iteration, and mutable generation visitor contract over the tile backing.

Tile heightmap traversal now runs base terrain, contextual rivermap selection, river carving, and climate
in TerraForged's chunk-major order while reusing the current continent's rivermap across cells.

Cached tile traversal delegates each integer block to TerraForged's contextual heightmap cache, copying
completed cells on hits without rerunning base, river, lookup, or climate stages.

World filters now accept real TerraForged tiles, borrow their filter region, and use the tile's region
coordinates for optional and required passes exactly like the Java `WorldFilters.apply` entry point.

Zoomed tile traversal applies TerraForged's centered translation and zoom to world block coordinates
before the same base, rivermap, river, and climate stages.

Cached zoomed traversal floors transformed samples with TerraForged's Engine 0.3.0 semantics and reuses
completed cache cells when multiple zoomed blocks resolve to the same integer coordinate.

Tile-wide visitors now traverse only the unbordered generation area with local coordinates, while the
generation visitor exposes mutable cells and tile batch sizing preserves TerraForged's ceil division.

The synchronous tile generator now owns TerraForged's heightmap and filter stages, maps chunks to
regions, creates empty tiles, generates regular or centered zoomed terrain, and post-processes filters.

TerraForged's lazy callable now computes exactly once under concurrent access, reports completion and
non-cancellation like the Java source, and backs lazy regular and zoomed tile generation handles.

Batched and striped generators now preserve TerraForged's ceil-sized chunk partitions, per-chunk river
context, clipped final tasks, centered zoom transforms, and post-processing flags.

The tile-generator builder now mirrors TerraForged's factor, border, batch, and striped configuration,
selecting direct generation unless a positive batch size requests batched or striped traversal.

Cache entries now wrap TerraForged's lazy computation with completion reporting and refresh their
millisecond access timestamp on every `get`, providing the expiration primitive used by tile caches.

Cache entries can now start computation immediately on a worker thread, report task completion before
the value is requested, and block `get` on the in-flight result like TerraForged's future-backed entry.

The bounded long-key map now preserves insertion-order eviction during compute-only insertion, while
the cache wrapper supports explicit close-on-remove and strict timestamp expiration sweeps.

The tile cache now keys asynchronous generation by packed region coordinates, reuses in-flight and
completed entries, supports blocking and present-only lookup, and gates queueing above four workers.

Cached chunk readers now retain their tile entry, expose chunk and block coordinates, and reproduce
TerraForged's cell lookup, full iteration, and original bounded-visit behavior safely across threads.

Tile disposal now counts unbordered chunks with Java-width atomic increments and removes a completed
region from its provider cache when the source threshold is reached.

The async tile-factory wrapper now starts regular and zoomed generation immediately, delegates region
mapping, remains idempotently asynchronous, and creates tile caches with the same worker configuration.

Climate primitives now include TerraForged's range compressor, latitude temperature curve, and
frequency-scaled moisture shaping as composable noise modules with Java-oracle raw-bit fixtures.

Biome climate classification now reproduces TerraForged's complete 256-square lookup map, climate
ranges, display colors, curved and linear sampling, and direct cell application without a PNG decoder.

Climate settings now preserve TerraForged's temperature and moisture ranges, biome shape defaults,
range clamping and biasing, and configurable edge-noise module construction.

The climate module now reproduces warped biome cells, region identity and edges, macro grouping,
continent-adjusted moisture, biome classification, and coastal terrain masking.

The outer climate stage now adds biome-edge resampling, submerged coast conversion, deterministic
climate randomness and offsets, and TerraForged's elevation-based temperature adjustments.

A shared continent interface now exposes terrain population, edge and land sampling, nearest centers,
and river maps across multi, single, improved, and fancy generator factory modes.

Continent construction now accepts the aggregate context's river settings and levels in every mode,
while preserving default convenience constructors and TerraForged's seed order.

Generator settings and context now aggregate world, climate, terrain, river, and filter configuration,
with TerraForged's root-seed copy, split, offset, and level initialization semantics.

The concrete heightmap now assembles TerraForged's continent, terrain-region, mountain, ocean, river,
and climate graph and implements the tile generator's heightmap source contract.

The world-generator factory now owns and shares the concrete heightmap and filter graph, returning
lightweight generator views with TerraForged's original factory lifecycle.

The mod terrain-level layer now preserves TerraForged's configurable build, sea-floor, sea-level, and
base-height bounds plus exact automatic horizontal-frequency and normalized-height transforms.
Constructor bounds use the original requested maximum height for the sea-level limit, including
Java's lower-bound-first clamp behavior when limits are inverted; seven Java boundary fixtures
cover below-minimum and above-maximum build heights as well as valid endpoints.

Bordered noise maps and chunk terrain data now preserve TerraForged's coordinate indexing, sampled
height, base, river, terrain, gradient, and extrema conversion from the current source implementation.

The current-source terrain-generator contract now owns reusable terrain data, delegates bordered noise
generation, scales direct height queries, and preserves TerraForged's object-pool behavior exactly.

Terrain blending utilities now preserve weighted noise bands, identity-keyed sample caching, hashed
random selection, and square-spiral terrain searches from TerraForged's current source.

The concrete terrain blender now reproduces TerraForged's warped nine-cell terrain selection,
minimum-noise bias, identity-cached terrain sampling, and squared border-weight blending.

Terrain searches now traverse TerraForged's warped square spiral, filter weighted terrain bands by
name, jitter matching region cells, and return world-scaled nearest positions.

Noise graphs now support TerraForged's Cereal-style recursive reseeding by adding the context seed to
every nested source, domain, erosion node, and cached branch, completing terrain-blender `withSeed`.

Cave-noise assets now preserve global and unique cave modes, recursively reseeded elevation, shape,
and floor modules, integer cave scaling, and TerraForged's jittered cellular cave distribution.

Biome viability now composes edge, height, noise, saturation, and slope fitness through TerraForged's
ordered multiply and weighted-sum rules, including the original scaling and falloff formulas.

Climate samples and biome overrides now preserve TerraForged's deep and shallow ocean climates,
beach variants, exact river masking, and base-only sample reset behavior.

Continent control points now classify deep ocean, shallow ocean, coast, and inland terrain and map
custom world settings through TerraForged's clamped five-segment falloff curve with exact float order.

Current-source continent cells now preserve mutable point samples, square and staggered-hexagon
jitter placement, coordinate adjustment, and Perlin, Simplex, and Cubic source normalization.

Current-source continent configuration now preserves shape and noise defaults, mutable float ranges,
river and lake width/depth presets, copy behavior, and width-only frequency scaling.

## Install

Use `native-mods/terraforged` as a local native-mod bundle in the Rusticraft launcher:

```sh
horizon-launcher native-build native-mods/terraforged --trust-native-code
```

Native mods compile into a separate game build. Back up a world before opening it with a different
generator, and keep the mod enabled for that world's unexplored chunks.

## Development

The current-source `ContinentNoise` wrapper now owns the cached cell/shape/river stages, consumes
context seeds in Java order, applies continent-scale coordinates and domain warp, and offsets both
sampling paths by the selected spawn location. Four original-Java fixtures cover two scales and
two world seeds; worker-thread tests check shared ownership. Full terrain-height blending and
playable generator integration remain separate steps.

The current-source lossy cache now preserves bucket rounding, FastUtil mixing, key-only hits
(the supplied seed is not part of the cache key), eviction order, and post-unlock removal callbacks.
An original-Java eviction trace and concurrent-hit tests cover the Rust implementation. Rust uses
mutexes and shared ownership instead of optimistic stamped locks; speculative listener races and
invalid Java allocation sizes are not emulated. Cells now use the original 2,048-entry capacity and
river pieces use 1,024 entries, with explicit concurrency constructors for deterministic checks.
Spawn search still bypasses the cell cache as in Java. Native shared ownership replaces Java object
reuse; pipeline fixtures also pass against the original Java caches and pools.

The current-source Noise2D seed-first API now has a separate `value_seed_first` entry point:
it preserves Perlin/Perlin2's ignored seed argument and adds the configured seed for other sources
with Java wrapping, without changing the legacy replacement-seed API. All eight source kinds have
raw-bit fixtures against upstream Noise2D commit
`2cb48431148a321f74319d7ef0ea61b26bf8e59c`, whose seed refactor predates the pinned TerraForged source.

Current-source river networks now generate downhill connections, displaced segments, ocean
extensions, and source lakes. Six 13×13-cell Java-source grids check counts and a checksum of every
node's raw float bits across both layouts and all noise sources. Nearest-node sampling now preserves
3×3 traversal, coordinate adjustment, distance ties and absent-node sentinels, checked on six 7×7
Java-source grids. Warped sampling and ridge erosion now connect the shape/network/carver stages;
six more grids verify combined outputs against the actual current Noise2D sources, without noise
substitutes. The pipeline now uses bounded cell/network caches; playable game integration remains
unfinished.

The current-source river carver now applies valley/bank/bed shaping, erosion modifiers, coastline
clipping, and river-then-lake precedence. Seven focused Java-source fixtures use an explicitly
supplied erosion value, while the combined pipeline tests use the real seeded erosion source.
This checks the river pipeline, not complete terrain generation or playable integration.

The current-source river layer now includes displaced river-node geometry, node/carver sample
sentinels, and reusable river/lake piece collections. Geometry fixtures compare raw float bits
against the original Java RiverNode, including projection clamping and radial falloff.

The current-source cell generator now supplies seeded jittered cells and nearest-cell lookup to the
shape blender. Tests compare cell fields and combined shape outputs for both cell layouts and all
three noise sources against the local Java source, including overflowing seed additions. This
cell cache preserves Java's key-only hits while direct computation remains available for references.
Game integration remains separate work. Spawn offsets follow the original spiral search, origin skip, asymmetric
neighborhood check, and retained first-seed result. Synchronized reads and writes preserve Java's
separate volatile check-and-set, including the possibility of duplicate concurrent searches. Six
Java-source fixtures verify the offsets across both cell layouts and all three noise sources.

The current-source continent shape layer now blends the nearest 25 cells, preserving threshold,
falloff, and separate primary/base noise semantics. Mixed-cell outputs are checked against the
Java ShapeGenerator bytecode (with a plain NoiseSample holder replacing Minecraft registry setup).
Its cell-provider interface is ready for the current-source continent generator; this does not yet
connect the mod to playable world generation.

Runtime-seeded scalar graphs now propagate the seed through constants, octave sources, absolute
value, alpha, bias, clamp, inversion, mapping, powers, scaling, thresholds and all five arithmetic
combiners. `Module::try_value_seed_first` returns an error for nodes not ported to runtime seeding;
it never silently falls back to legacy sampling. Legacy sampling and serialized reseeding remain
unchanged. Custom closure nodes intentionally remain unsupported because their callback has no seed
parameter; all serializable graph nodes now support this path for current-source terrain blending.

Legacy terraces now propagate runtime seeds through the source and asymmetric lower/upper curve
modules while preserving cubic interpolation and branch-lazy curve sampling. The current Noise2D
oracle covers all four runtime seeds and transition directions.

Advanced terraces now propagate runtime seeds through source, mask, slope and modulation modules
while preserving Java's early exits, octave stepping and blend behavior. A current Noise2D oracle
checks the complete graph across four runtime seeds.

Fixed, variable and power curve modifiers now propagate runtime seeds through every sampled child
while retaining the current Noise2D curve functions and Java floating-point operation order.

Step, frequency, gradient and coordinate-modulation nodes now propagate runtime seeds through all
children while preserving original-coordinate controls, transformed source coordinates and lazy
gradient bounds.

Boost, cellular value/lookup, cellular edge and sine sources now support runtime-seeded sampling.
Cell seeds use Java wrapping addition, lookup children retain the runtime seed, and sine controls
sample the original coordinates before axis interpolation.

Line sources now propagate runtime seeds through radius and fade modules while preserving endpoint
short-circuits, fade ordering, radius evaluation and feathering.

Erosion ridge now applies runtime seeds to both its cell lattice and every nested source sample,
preserving octave cache reuse and matching recursive serialized reseeding bit-for-bit.

Current-source terrain blending now forwards runtime seeds through its coordinate warp and every
selected terrain-noise module, including cached border blends and nearest-terrain iteration.

The current height sampler now selects deep ocean, ocean-to-beach, beach-to-coast and inland
branches at TerraForged's exact control points. It preserves runtime-seeded ocean and terrain noise,
Java float interpolation order, the 1.2 land-height multiplier and branch-specific terrain labels.
`tools/terraforged/TerraForgedNoiseHeightOracle.java` records direct current-source Java bits for
all four branches.

The sampler implements the terrain-generator contract with isolated per-call sample and blender
resources. Chunk generation visits TerraForged's complete bordered 18-by-18 grid in Java loop order,
including wrapping chunk-coordinate arithmetic, before exposing the populated noise data once.

Nearest-terrain search now follows the warped terrain-region spiral, skips zero sentinels and
rejects ocean/coast or river-adjacent candidates exactly like the current generator.

The current erosion pipeline's noise-tile geometry now preserves Java overflow arithmetic,
relative indexing, default dimensions and diagnostic formatting. Its standalone Java fixture is
`tools/terraforged/TerraForgedNoiseTileSizeOracle.java`.

Current heightmap smoothing now performs the in-place, distance-weighted post-erosion passes with
the current settings defaults, early exits, radius rounding and Java float operation order.

The current hydraulic filter now has its radius-seven erosion brush: one normalized interior disc
is shared across the map while clipped edge cells retain Java's ordered, reweighted subsets.

The current hydraulic droplet simulation now preserves region and legacy-tile seeding, bilinear
gradients, height falloff, sediment capacity, deposition, erosion, gravity and evaporation order.

The eroded-region cache now uses current eight-chunk regions, four-chunk margins and sixteen-entry
access-order eviction. Concurrent callers share one build, seed changes invalidate all entries, and
failed builders are removed for retry while negative chunk and block coordinates retain Java layout.
`tools/terraforged/TerraForgedErosionRegionOracle.java` records the signed-coordinate fixtures.

The eroded-noise wrapper now builds each bordered 256-by-256 height region from raw terrain samples,
applies hydraulic erosion and optional smoothing in current-source order, and exposes the expensive
single-column height accessor that reapplies rivers after reading the cached eroded surface.

Bordered chunk emission now preserves the raw base, continent and terrain metadata, replaces only the
height from the cached eroded region, then applies rivers. Scratch chunks and samples are pooled per
call so a blocked worker cannot let nested work overwrite its in-flight generation data.

The current terrain cache now starts one shared asynchronous generation task per seed and chunk,
supports readiness probes and eager hints, and removes completed entries after decoration. Shared
terrain handles defer pool restoration until their final concurrent reader releases the data.

The generator query facade captures Java's narrowed and raw world seeds, exposes level and cached
chunk data, and reproduces structure heightmap and base-column queries using raw noise samples for
both, as in the pinned source. Chunk filling still consumes eroded terrain. The height-query oracle
executes the original Java method, and a smoothing-enabled regression distinguishes raw query
heights from eroded block heights for all six heightmap types.

The first playable terrain stage now fills Rusticraft chunks from TerraForged height data with stone
and water, including river/lake base-height water levels, build-range clipping, and the ocean-floor and
world-surface generation heightmaps used by later surface, structure, and carving stages.
The generator facade now joins the cached terrain for a chunk's own position, fills its real block
storage, and returns the same pooled handle so the remaining terrain stages can consume it safely.

Structure-terrain column operations now preserve stable piece ordering, ground-level offsets,
distance-based raising, highest-piece carving and elliptical roof clearance. The caller supplies
eligible nearby rigid pieces. The chunk adapter edits real block storage and generation heightmaps,
then updates terrain heights without changing generation extrema. Selection from Horizon's referenced
structure starts now filters BEARD_BOX/BEARD_THIN, includes pieces within the inclusive 20-block
chunk margin, excludes non-rigid pool elements, and preserves stable minimum-Y ordering and ground
offsets. The original constructor oracle covers adaptation, margin boundaries and equal-Y ties.
The generator facade now offers a structure-aware terrain stage: it fills blocks and generation
heightmaps before applying structure edits under one cached-terrain write lock. All retained handles
observe the updated heights; explicit read/write guards replace unrestricted shared references.
Concurrent writes serialize, and eviction still delays pool restoration until the final handle drops.
Invocation from the complete game-generation pipeline is still pending.
The surface erosion pass now applies the original depth-scaled gradient threshold and sea-level
cutoff to real chunk storage. It searches strictly above max(0, surface-20), preserves the first
non-erodible material (including air or fluid), and leaves cached terrain heights unchanged.
The caller supplies dirt/snow tag membership; vanilla surface rules, production state bindings and
game-pipeline invocation remain pending. Eleven original-Java column fixtures cover the search
boundaries, low terrain, material replacement and threshold decisions.
Water-edge smoothing now limits eligibility to river/lake terrain with an exactly zero river mask
and source water. It searches the radius-six disk in original row order, reads local blocks directly
and neighboring chunks through the supplied region lookup, then writes level-two water on the first
air hit. Java fixtures check ordered neighboring reads, negative chunk coordinates, boundary points,
non-source water, negative zero and NaN masks. Production water-state bindings and region wiring are
still pending, alongside full game integration.
Snow postprocessing now smooths layers using the original fractional-height formula and Noise2D
floor behavior, and erodes steep snow with the original air fallback and strict 15-block/zero-height
scan bounds. Thirteen Java fixtures cover fractional and negative heights, snow blocks versus layers,
non-snow surfaces, early stopping and low terrain. Production block-class/tag/property bindings and
full surface-pipeline invocation remain pending.
The original Java method oracle checks ordered block edits and resulting heights across six columns.

Biome chunk filling samples one 4-by-4 biome buffer at quart y=-1 in Java call order and repeats
it through every section. It does not inject a separate cave layer. Sampling completes before
section writes, so a source failure cannot partially fill the chunk. The biome-fill oracle runner
extracts and executes the pinned Java method with minimal chunk/section/buffer stand-ins.
The cave-biome sampler reproduces independent GLOBAL and UNIQUE pools, equal-weight selection,
and Java-width seed offsets. Empty pools remain empty, matching Java's null result; no coverage mask
is applied. `tools/terraforged/run_cave_biome_oracle.py` compiles the original CaveBiomeSampler,
WeightMap and MathUtil with minimal Minecraft registry stand-ins rather than restating their logic.

The scalar graph fixtures cover 15 graphs, four seeds and four coordinates (240 Java samples),
including wrapped seed addition and mixed Perlin/seed-sensitive sources. Regenerate their hashes
with `tools/terraforged/TerraForgedSeededGraphOracle.java` on a classpath containing compiled
Noise2D commit `2cb48431148a321f74319d7ef0ea61b26bf8e59c` and its Cereal dependency.

Runtime-seeded domain sampling now covers direct, axis-noise, directional, additive, compound and
cumulative warps, including nested combinations and warped module evaluation. Both axes use the
original coordinates independently, and cumulative offsets retain the Java implementation's
coordinate addition. Seven domains across four seeds and four coordinates compare five outputs
each against Java (560 float samples). The companion oracle is
`tools/terraforged/TerraForgedSeededDomainOracle.java`; unsupported children return errors.

Runtime seeding now reaches base, fixed and variable blends, including their controls and variable
blend radii. Only selected branches are evaluated; blend boundaries retain Java's evaluation of
both children and zero-width midpoint NaNs. The oracle
`tools/terraforged/TerraForgedSeededBlendOracle.java` checks 144 samples across all three interpolation
modes, four seeds and four coordinates, plus zero-width cases.

Seeded bounded selection and multi-blends now preserve Java's falloff branches, source selection,
rounding and neighboring-slot interpolation. Reversed zero-falloff bounds retain lower-bound-first
evaluation. `tools/terraforged/TerraForgedSeededSelectionOracle.java` checks 240 samples across all
interpolation modes and nine exact falloff-boundary samples; Rust also checks lazy child evaluation.

Runtime-seeded caches now preserve Noise2D's coordinate-only key: a hit retains the first seed's
value, signed zero coordinates compare equal, NaN coordinates always miss, and failed child samples
are not stored. `tools/terraforged/TerraForgedSeededCacheOracle.java` records the direct Java bits.

Standard terraces now propagate runtime seeds through source, ramp, cliff, and ramp-height modules
while retaining branch-lazy child evaluation and exact step boundaries. The direct Java oracle is
`tools/terraforged/TerraForgedSeededTerraceOracle.java`.

The noise layer is ordinary Rust and can be tested without Minecraft resources:

```sh
rustc --edition 2024 --test native-mods/terraforged/noise.rs -o /tmp/terraforged-tests
/tmp/terraforged-tests
```

The mixin pins Rusticraft commit `1b6e55d4f3bd14079551bcad173d0fac8f8225bb`. Translation inputs are
TerraForged commit `6dd607ebfd41a3b7274e090b1f2cf565d53d1d44` and Engine 0.3.0 bytecode
shaded into the official `TerraForged-1.18.2-0.3.1-alpha-2.jar`. Tests compare raw `f32` bits with
values produced by those Java classes.
