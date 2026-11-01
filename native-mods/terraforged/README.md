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

## Install

Use `native-mods/terraforged` as a local native-mod bundle in the Rusticraft launcher:

```sh
horizon-launcher native-build native-mods/terraforged --trust-native-code
```

Native mods compile into a separate game build. Back up a world before opening it with a different
generator, and keep the mod enabled for that world's unexplored chunks.

## Development

The noise layer is ordinary Rust and can be tested without Minecraft resources:

```sh
rustc --edition 2024 --test native-mods/terraforged/noise.rs -o /tmp/terraforged-tests
/tmp/terraforged-tests
```

The mixin pins Rusticraft commit `1b6e55d4f3bd14079551bcad173d0fac8f8225bb`. Translation inputs are
TerraForged commit `6dd607ebfd41a3b7274e090b1f2cf565d53d1d44` and Engine 0.3.0 bytecode
shaded into the official `TerraForged-1.18.2-0.3.1-alpha-2.jar`. Tests compare raw `f32` bits with
values produced by those Java classes.
