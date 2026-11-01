# TerraForged for Rusticraft

An exact Rust translation of
[TerraForged](https://github.com/TerraForged/TerraForged), delivered as a stacked series of small
native-mod changes. This foundation ports the deterministic Noise2D primitives used by later terrain
modules. It intentionally does not alter world generation yet.

On top of them sit TerraForged's octave sources (Perlin, Perlin2, legacy simplex, improved simplex and
cubic) with their configuration, output bounds and the original explicit-seed behavior.

The ridged sources (Ridge, Billow and SimplexRidge) and the `Rand` white-noise source follow, including Engine's
quirks: `Rand.nextInt` divides by `range + range` (so it is almost always 0, and wraps for large ranges), and Engine
throws for ridged sources with more than 30 octaves, where this port keeps going.

The graph layer adds Engine's `Module` graph: constants, source nodes, the Abs, Alpha, Bias, Boost, Clamp, Invert, Map,
Power, Scale and Threshold modifiers and the Add, Multiply and Max combiners, with the shortcuts of Engine's builder
methods (`map`, `clamp`, `pow`, `bias`...). Its tests check the bounds each node reports as well as raw sample bits,
including Engine's quirks: Threshold reports its source's bounds rather than 0..1, and Max's minimum is the larger of
the first source's minimum and the other sources' maxima. `Module::constant` is `Source.constant`, which returns its
shared `ZERO` for -0.0, so it samples +0.0.

The domain layer adds Engine's coordinate warps (`Domain.DIRECT`, `DomainWarp`, `DirectionWarp`, `AddWarp`,
`CompoundWarp` and `CumulativeWarp`) and the `Warp` module that samples its source at the warped point. Its tests check
raw bits, including Engine's quirks: `DirectionWarp` reads `NoiseUtil`'s 4096-entry sine table, `DomainWarp` maps a
constant axis to NaN, and `CumulativeWarp` returns the second warp's coordinate rather than its offset. `CacheWarp`
(`Domain.cache()`) is not ported: Engine never sets its `cached` flag, so it only passes samples through. The seed/scale
shorthands that build noise warps (`Domain.warp(seed, scale, octaves, strength)`, `Module.warp(seed, ...)`...) are not
ported yet either.

The selector layer adds Engine's `Base`, `Blend`, `VariableBlend` (`blendVar`) and `Select` modules, with the bounds
Engine reports for them: `Base` keeps its base's minimum and the larger maximum, the others the smaller minimum and
larger maximum of their two sources. Its tests check raw bits, including Engine's quirks: with a zero falloff `Select`
only picks its upper source above `upperBound`, and `Blend` fixes its blend window from the control's bounds when it is
built.

The multi-blend layer adds `MultiBlend` (`Module.blend(blend, sources...)`): it splits the control's 0..1 range into
one evenly spaced slot per source and blends neighbouring slots, with Engine's slot edges, `NoiseUtil.round` slot
lookup and reported bounds (the smallest minimum and largest maximum of the sources, 0 and 0 without sources). Where
Engine throws (a control far enough outside 0..1 to pick a slot that does not exist, or sampling a `MultiBlend` without
sources), this port panics.

The terrace layer adds `Terrace` (`Module.terrace(ramp, cliff, rampHeight, steps, blendRange)`): it clamps its source
to 0..0.999999 and snaps it to flat steps, rising to the next step through a ramp (scaled by `rampHeight`) and a cliff.
As in Engine, the steps are fixed when it is built and start at 0, not at the source's minimum, spaced by the source's
range over `steps - 1`; it reports the source's bounds. Where Engine throws (sampling a NaN, or a terrace with no
steps), this port panics.

The legacy terrace layer adds `LegacyTerrace` (`Module.legacyTerrace(lowerCurve, upperCurve, steps, blendRange)`): it
clamps its source to 0..1 and snaps it to the nearest of the same evenly spaced steps as `Terrace`, easing into the
neighbouring steps with Curve3 transitions whose height is scaled by `lowerCurve` (towards the next step) and
`upperCurve` (from the previous one); it reports the source's bounds. Its tests check raw bits against a Java oracle,
including Engine's quirks: a NaN sample rounds to the first step, and below the first step's window it returns that
step even when a blend range above 1 inverts the windows. Where Engine throws (sampling a legacy terrace with no steps),
this port panics.

The advanced terrace layer adds `AdvancedTerrace` (`Module.terrace(modulation, mask, slope, blendMin, blendMax, steps,
octaves)`): above `blendMin` it snaps its source to `steps * i` steps for each octave `i`, pulls each result back
towards the source by `slope`, adds `modulation` and divides by `modRange` (the source's maximum plus the modulation's,
fixed when it is built), then blends that in, fully above `blendMax` and scaled by `mask`; a zero mask or a sample at
or below `blendMin` returns the source sample unchanged. It reports the source's bounds. Its tests check raw bits
against a Java oracle, including Engine's quirks: zero steps turn every terraced sample into NaN, `steps * i` wraps
like Java's `int`, and a zero `modRange` divides by zero. Where Engine loops forever (`octaves` = `Integer.MAX_VALUE`), this port stops.

The curve layer adds Engine's curve modifiers: `Curve` (`Module.curve(CurveFunc)`, with an `Interpolation`, a
`MidPointCurve` or an `SCurve`), `VariableCurve` (`Module.curve(mid, steepness)` with noise controls), `PowerCurve`
(`Module.powCurve(n)`) and `Steps` (`Module.steps(steps, slopeMin, slopeMax, curve)`). All of them report their source's
bounds, as Engine's modifiers do, even where the output is 0..1. Their tests check raw bits against a Java oracle,
including Engine's quirks: midpoint and variable curves use `NoiseUtil.exp`, `(1 + x / 256)^256`, rather than
`Math.exp`; `SCurve` raises a negative `upper` to `-lower`; `PowerCurve` fixes its curve from the source's bounds when
it is built, so a constant source samples NaN (or 1 for a zero power); `Steps` truncates like Java's `(int)`, so without
a slope window (`slopeMax <= slopeMin`) it rounds samples towards zero onto its steps. `NoiseUtil.pow` keeps
`Math.pow`'s NaN for a NaN power and for a base of ±1 with an infinite power.

The scalar layer adds `Freq` (`Module.freq(x, y)`), which scales the coordinates before sampling its source, `Grad`
(`Module.grad(lower, upper, strength)`), which raises samples at or below `upper` to a power falling to `1 - strength`
at `lower`, `Modulate` (`Module.mod(direction, strength)`), which offsets the coordinates along a direction read through
`NoiseUtil`'s sine table, and the `Min` and `Sub` combiners (`new Min(...)`, `new Sub(...)`). The three modifiers report
their source's bounds. As in Engine, `Min`'s maximum is the smallest of the first source's maximum and the other
sources' minima, and `Sub` subtracts the other sources' maxima from the first source's minimum (and their minima from
its maximum). Their tests check raw bits against a Java oracle, including NaN results: `NoiseUtil.pow` returns the NaN
`Math.pow` returns (its NaN operand, so `Grad` with `lower == upper` gives Java's -NaN from 0/0).

The cache layer adds `Cache` (`Module.cache()`, which returns a module that already is a cache unchanged): it remembers
its last coordinate and sample, shared by every graph that uses it, and reports its source's bounds. Engine's entry is
not synchronised; here it sits behind a lock that is only tried while the entry is copied in or out, so world-generation
threads never wait on one another and a busy cache simply samples its source. Unlike Engine, which compares coordinates
with `==`, the cache compares their bits: -0.0 does not hit +0.0's entry (and a NaN coordinate can hit), so a cached
sample is always the sample of the same input, whichever coordinate another thread sampled last.

The cellular primitive layer adds `NoiseUtil`'s 256-point cell table, the `DistanceFunc` metrics (Euclidean,
Manhattan, Natural), the five `EdgeFunc` functions with their bounds, and the 3×3 searches of `Noise.cell` (for the
`CELL_VALUE` cell function only) and `Noise.cellEdge`. As in Engine, neighbour cell coordinates wrap like Java `int`s
and, among tied cells, the first one found (row by row from the lower corner) wins. Their tests check raw bits against a
Java oracle, including NaN and infinite coordinates.

The cellular source layer adds the graph sources built on them: `CellNoise` (`Builder.cell()`, `Module::cell_source`)
and `CellEdgeNoise` (`Builder.cellEdge()`, `Module::cell_edge_source`). Both sample a single octave at the builder's
frequency and displacement. `CELL_VALUE` maps the nearest cell's value from -1..1 to 0..1, `NOISE_LOOKUP` samples its
lookup module, unmapped, at the nearest cell's feature point (not scaled by the displacement), and `DISTANCE` always
returns 0, as in Engine; `CellEdgeNoise` maps its edge function's value to 0..1 from that function's bounds. Both report
the `Noise` defaults 0..1, whatever the cell function or the lookup's bounds. Their tests check raw bits against a Java
oracle. Where Engine throws (`NOISE_LOOKUP` when no cell's distance is below `Float.MAX_VALUE`: at NaN or infinite
coordinates, or, with the Euclidean and Natural metrics, scaled coordinates beyond about 1.8e19, whose squares overflow),
this port panics.

The sine source layer adds `Sin` (`Builder.sin()`, `Source.sin(scale, alpha)`, `Module::sine_source`): it samples its
`alpha` module, scales the coordinates by its frequency and reads `NoiseUtil`'s 4096-entry sine table at x when alpha is
exactly 0, at y when it is exactly 1, and blends the two by alpha otherwise (so an alpha outside 0..1 extrapolates),
then maps the result from -1..1 to 0..1. It reports the `Noise` defaults 0..1. A NaN alpha samples NaN, but the table
itself never returns one: as in Engine, a NaN coordinate reads entry 0 and an infinite one saturates before it is masked
to an index. Its tests check raw bits against a Java oracle for all three branches, including NaN, infinite and
extrapolating alphas and NaN, infinite and huge coordinates.

The line source layer adds Engine's `Line` (`Line::new`, `Module::line`): a segment whose value falls from 1 on the
segment to 0 where the squared distance reaches its `radius` module's sample (plus an optional minimum), narrowed near
each end by the `fade_in`/`fade_out` modules (fractions of the squared length; zero, negative and NaN fades are
ignored) and scaled by its feather. It reports the `Noise` defaults 0..1. It also ports `Line`'s clipping tests and
static helpers (`dist2`, `sign`, `intersect`, `distance_on_line`, `distance2_line`,
`distance2_line_including_ends`). As in Engine, a zero-length line and the line's own endpoints sample NaN (0/0). Its
tests check raw bits against a Java oracle, including those NaNs, NaN and infinite points and every collinear case of
`intersect`.

The engine type layer starts on Engine's terrain model, in a second module, `terraforged_engine` (`engine.rs`). It ports
`Seed`, whose `next()` returns its cursor and then increments it, wrapping like Java's `int` (`split()` restarts from the
root, `offset(n)` from root + n), the `ITerrain` property flags with their defaults (`is_submerged`, `overrides_river`
and `overrides_coast` follow from the other flags), and the `TerrainCategory` enum: its eleven categories in Engine's
order, each one's flags, and `get_dominant`, which keeps the later category. Engine shares one `Seed` by reference;
this `Seed` is `Copy`, so code that draws from it takes `&mut Seed`. Its tests check every category's flags, the
ordinals and dominance, and the seed cursor's wrapping against a Java oracle.

The terrain descriptor layer adds Engine's `Terrain` (`engine.rs`): an id, a name and a category, with every `ITerrain`
flag delegated to its category or a parent terrain, plus the `ConfiguredTerrain` variants (their own erosion modifier,
mountain and river-override flags), the volcano terrains (which also override coasts and are volcanoes) and
`CompositeTerrain`, named `first-second`, flat only when both are and with the smaller erosion modifier (`Math.min`, so
-0.0 beats +0.0 and NaN wins). As in Engine, terrains compare by identity (`Terrain::is_same`, no `==`), the render hue
is `NoiseUtil.valCoord2D` of the name's Java (UTF-16) string hash, `with_id` delegates to the terrain's parent or to the
terrain itself (so a composite's own flat flag and erosion are not carried over), and a composite always takes its first
terrain's category and flags: Engine's `getDominant` compares the first terrain's category with itself. Its tests check
ids, names, categories, hue and erosion bits and every flag against a Java oracle. The `TerrainType` registry, which
assigns the ids, is not part of this layer.

The terrain registry layer adds Engine's `TerrainType` registry (`TerrainRegistry`): the 16 built-in terrains with
ids 0 to 15 in Engine's order, `register` (which returns the terrain already registered under the name, else re-ids
the new one with `with_id`), `register_composite` (the same object returns itself; otherwise the lower-id terrain comes
first, and each `lower-higher` composite is registered once), lookups by id (unknown ids give `none`) and by name, and
`TerrainHelper.getOrCreate` (`get_or_create`). Names compare like Java's `equalsIgnoreCase`, with Unicode simple case
mappings one code point at a time (`K` matches the Kelvin sign, `ß` does not match `SS`). Engine's registry is one
static list per Java process, so there the ids of composites and custom terrains depend on what earlier worlds
registered; here each world owns a registry, and its built-in terrains are its own objects. Its tests check every
registered terrain's id, name, category, hue, erosion and flags against a Java oracle.

The world-level layer adds Engine's world settings and `Levels` (`engine.rs`): `WorldProperties` (spawn type, world
height 256 and sea level 63 by default), `WorldControlPoints` (the deep ocean, shallow ocean, beach, coast and inland
thresholds) and `ControlPoints`, which falls back to the defaults unless the points rise strictly from a deep ocean of
at least 0 to an inland of at most 1 (a NaN point fails) and adds `coastMarker`, halfway from coast to inland. `Levels`
converts between block heights and Engine's 0..1 heightmap values: the world height is at least 1, `waterY` is the sea
level minus one and `groundLevel` the sea level plus one (wrapping like Java's `int`), `water` is `waterY` and `ground`
the sea level over the world height (each block height capped at the world height), and `scale`, `elevation`,
`water(n)` and `ground(n)` keep Engine's float math and `(int)` truncation. Heights count from block 0, not from the
world's minimum Y: TerraForged's continent noise passes its `maxY` and sea level (480 and 62 by default) although its
terrain starts at y = -64. Its tests check every field and conversion against a Java oracle, including those levels,
320 and 384 block heights, `int` overflow of the sea level and of `y - waterY`, and heights beyond 2^24. Engine's
static `Levels.scale(steps, settings)` is not ported: neither Engine nor TerraForged calls it.

The world-settings layer adds Engine's `WorldSettings` (`engine.rs`): the seed (0 by default; `transient`, so never
serialized in Engine), the properties and control points above, and `Continent` with Engine's defaults: the
`MULTI_IMPROVED` generator (`ContinentType`, with Engine's indices 0 to 3), the Euclidean shape, a scale of 3000, a
jitter of 0.7, skipping and size variance of 0.25, and 5 distortion octaves with a gain of 0.26 and a lacunarity of
4.33. Its test checks every default's bits against Engine 0.3.0. Engine's serializer and its `@Range` limits are not
ported. These are Engine's defaults, not the values TerraForged generates with: its `NoiseGenerator` sets the seed,
sea level and world height from its own levels and replaces the continent scale and control points.

The terrain-settings layer adds Engine's `TerrainSettings` (`engine.rs`) with Engine's defaults: `General`
(`GeneralTerrainSettings`: a terrain seed offset of 0, a terrain region size of 1200, a global vertical scale of
0.98, a global horizontal scale of 1 and fancy mountains on) and one `Terrain` entry (`TerrainFormSettings`: weight,
base scale, vertical scale and horizontal scale) per landform, with weights of 1 for steppe, 2 for plains, 2 for
hills, 1.5 for dales, 1.5 for plateau, 1 for badlands, 2 for torridonian, 2.5 for mountains and 5 for volcano, and
every scale at 1. Its test checks every default's bits, and those of both `Terrain` constructors, against Engine
0.3.0. Engine's serializer, its `@Range` limits and `Terrain.apply` are not ported. These are Engine's defaults, not
what TerraForged generates with: its `ModTerrains` builds its landforms from these settings with a global vertical
scale of 1 (and fancy mountains off for the two `mountains_ridge` terrains), and weights its terrains with its own
values rather than these: 1.5 for steppe, 2.5 for plains, 2 for each hills terrain, 1.5 for dales, 2 for plateau,
1.75 for badlands, 2.5 for torridonian and 1.25 for each mountains terrain and dolomites, with no volcano entry.

The first LandForms layer adds Engine's `LandForms` (`landforms.rs`, module `terraforged_landforms`) with its first two
landforms, `steppe` and `plains`. Each draws five seeds from the `Seed` in Engine's order: a 3-octave Perlin erosion
noise (scale `2 * scaleH`, lacunarity 3.75, alpha 0.45), the two 3-octave Perlin axes of a warp by `scaleH / 4`
(lacunarity 3 for steppe, 3.5 for plains), the 1-octave Perlin base (scale `scaleH`) that the erosion multiplies, and
the seed of a 1-octave Perlin warp at scale 256 (strength 200 for steppe, 256 for plains; its second axis uses that
seed + 1). `scaleH` is `Math.round(250 * horizontalScale)` of the landform's settings. Steppe then scales by 0.08,
plains by `0.15 * globalVerticalScale`, and both subtract 0.02. Its tests check the seed afterwards, the bounds and the
bits of every sample over a grid of positive, negative and large coordinates against a Java oracle, for Engine's
default settings, ModTerrains' (a global vertical scale of 1) and degenerate scales (zero, negative, NaN, huge, and
`Math.round`'s edges).

The hill LandForms layer adds `hills1`, `hills2` and `dales`. `hills1` draws four seeds: a 3-octave Perlin at scale 200
multiplied by a 3-octave Billow at scale 400 (alpha 0.5), then two Perlin noise warps (scale 30, 3 octaves, strength
20; scale 400, 3 octaves, strength 200), scaled by `0.6 * globalVerticalScale`. `hills2` draws five: a 2-octave cubic
at scale 128 times a 4-octave Perlin at scale 32 (alpha 0.075), the same two warps, then times a 2-octave Ridge at
scale 512 (alpha 0.8, with the builder's ridge gain of 0.975), scaled by `0.55 * globalVerticalScale`. `dales` draws
four and ignores the settings: a 1-octave Perlin at scale 400, clamped to 0.3..0.6 and mapped to 0..1, blends linearly
(midpoint 0.4, range 0.75) from a 4-octave Billow at scale 300 (gain 0.8, lacunarity 4, power curve 0.5, times 0.75)
to a 3-octave Billow at scale 350 (gain 0.8, lacunarity 4, raised to 1.25); the result is raised to 1.125, warped
(scale 300, 1 octave, strength 100) and scaled by 0.4. As in Engine, a vertical scale whose product with 0.6 or 0.55
is exactly 1 leaves that hill landform unscaled. The same grid test covers the three, with the same settings cases and
two such vertical scales.

The plateau LandForms layer adds `plateau`, which draws nine seeds and ignores the horizontal scales. Its valley is an
inverted 1-octave Ridge at scale 500 warped twice (1 octave each: scale 100, strength 150; scale 20, strength 15). Its
top, a 3-octave Ridge at scale 150 (lacunarity 2.45) warped twice (scale 300, strength 150; scale 40, 2 octaves,
strength 20) and scaled by 0.15, is multiplied by the valley clamped to 0.02..0.1 and mapped to 0..1. The valley times
a 1-octave cubic at scale 500 (times 0.6, plus 0.3), plus the top, goes through a four-step terrace (lower curve 0.9,
upper curve 0.15, lower height 0.35, blend 0.4); a 3-octave Perlin surface at scale 20 (times 0.05, warped at scale
40, 2 octaves, strength 20) is added and the sum is scaled by `0.475 * globalVerticalScale`, computed in double as in
Engine. The grid test covers it with the same settings cases, a start seed where the surface warp's second axis seed
wraps, and a vertical scale whose product with 0.475 is exactly 1.

The terraced LandForms layer adds `badlands` and `torridonian`; both ignore the settings. `badlands` draws six seeds.
Its hills, a 4-octave Ridge at scale 275 (the builder's ridge gain of 0.975) warped twice (scale 400, 2 octaves,
strength 100; scale 18, 1 octave, strength 20), are multiplied by a 3-octave Perlin mask at scale 270 clamped to
0.35..0.65 and mapped to 0..1. Its detail adds two step layers, the hills in 4 and in 10 linear steps (slope 0.6..0.7)
times 0.6, each plus the hills warped once more (scale 100, 1 octave, strength 50) times 0.4. Its shape is the hills in
4 Curve3 steps (slope 0.65..0.75) times 0.6, plus the hills times a 3-octave Perlin at scale 200 times 0.4, all times
0.6. The shape times the detail at alpha 0.5 is scaled by 0.55 and raised by 0.025. `torridonian` draws nine: a
3-octave Perlin at scale 200 blends linearly (midpoint 0.6, range 0.6) from plains, a 3-octave Perlin at scale 100
warped twice (scale 300, 1 octave, strength 150; scale 20, 1 octave, strength 40) times 0.15, to boosted hills, a
4-octave Perlin at scale 150 warped twice (scale 300, 1 octave, strength 200; scale 20, 2 octaves, strength 20). The
blend goes through an advanced terrace (modulation: a 1-octave Perlin at scale 120 times 0.25; mask: a 1-octave Perlin
at scale 200 times 0.5 plus 0.5; slope 0.5, blend 0..0.3, 6 steps, 1 octave), is boosted again and scaled by 0.5. The
grid test covers both with the same cases, whose start seeds include ones where a warp's second axis seed wraps.

The ocean LandForms layer gives `LandForms::new` Engine's other two constructor arguments, the world's `Levels` and the
`ground` module, and adds `deepOcean` and the two bases: `getOceanBase` is `Source.ZERO` and `getLandBase` returns
the `ground` module. Unlike the other landforms, `deepOcean` takes a plain `int` seed, which it increments six times
(wrapping like Java's `int`) without touching any `Seed`. It scales by the `Levels`' `water` level (`seaLevel` in
Engine): its hills, a 3-octave Perlin at scale 150 times `water * 0.7` plus a 1-octave Perlin at scale 200 times
`water * 0.2`, and its canyons, a 4-octave Perlin at scale 150 through a 0.2 power curve, inverted and times `water *
0.7`, plus a 1-octave Perlin at scale 170 times `water * 0.15`, are blended linearly (midpoint 0.6, range 0.65) by a
1-octave Perlin at scale 500 and warped (scale 50, 2 octaves, strength 50). As in Engine, `water * 0.7` is computed in
double and the other two products in float. Its test checks the water level, the bounds and every grid sample's bits
against a Java oracle for Engine's default levels (256 high, sea level 63), TerraForged's continent levels (480 and
62), ModTerrains' swapped `new Levels(63, 255)` (a water level of 1), a negative and a zero water level and a sea
level whose `- 1` wraps, with start seeds where the increments and the warp's second axis seed wrap. The other
landforms and TerraForged's `RandSeed` (ModTerrains builds its landforms from a `SplittableRandom` seed rather than
Engine's counter) are not ported yet.

The erosion Ridge layer adds Engine's `Ridge` (`com.terraforged.engine.module.Ridge`, `ErosionRidge` in `noise.rs`;
not the `RidgeNoise` source) and its `Ridge.Noise` module (`ErosionRidge::wrap`, `Module::erosion_ridge`). For each
octave it finds, in each of the 3×3 cells around the point (`NoiseUtil.cell`'s jittered feature points at the octave's
grid size), the segment from that cell's point to the lowest-sampled point among its own 3×3 neighbours, and takes the
distance to the nearest such segment over the grid size, clamped to 0..1. Each of an octave's 25 distinct candidate
points is sampled once (a sample of exactly -1, the cache's empty marker, is
sampled again). Octaves start at the grid size with a gain of 1; each later one multiplies the gain by
`amplitude`, the grid size by `distanceFallOff` and the coordinates by `lacunarity`, and the weighted sum is divided by
the summed gains (so zero or negative octaves give NaN, as in Engine). The module then lerps from that erosion to the
source sample by `1 - strength` (`CONSTANT`), `1 - strength * value` (`INPUT_LINEAR`) or `1 - strength * erosion`
(`OUTPUT_LINEAR`). The short constructors default to an amplitude of `1 / (octaves + 1)`, a lacunarity of 2.25 and a
fall-off of 0.75. Like every Engine `Noise` it reports 0..1, whatever its output. Its tests check raw bits against a
Java oracle over a grid of positive, negative and large coordinates and at huge, NaN and infinite points, for all
three modes, a source sampling -1 or NaN, zero and negative octaves, negative, zero and tiny grid sizes, and the
`int` wrap of `octaves + 1`.

The mountain LandForms layer adds `mountains`, `mountains2` and `mountains3`. `mountains` multiplies a 4-octave Ridge
(scale `410 * mountains.horizontalScale` through `Math.round`, gain 1.15, lacunarity 2.35) by a Perlin detail and warps
it; `mountains2` and `mountains3` multiply a warped, clamped cell-edge source (cells of 360 and 400, `DISTANCE_2`) by a
Perlin blur and a Ridge surface and raise it to 1.1, and `mountains3` adds an advanced terrace. With
`general.fancyMountains` on (the default), Engine's `makeFancy` wraps each in the erosion Ridge (2 octaves, strength
0.65, grid 128, `CONSTANT`) warped along a Perlin direction and draws two more seeds; with it off (TerraForged's
`ModTerrains` turns it off for `mountains_ridge_1` and `mountains_ridge_2`) the graph is left as is. They scale by `0.7`
and `0.645` times the global vertical scale, computed in double. The landform grid test checks them against the Java
oracle for every settings case, with fancy mountains on and off, including mountain scales whose `Math.round` gives 0
or keeps an odd integer above 2^23 and vertical scales whose `scale(1)` leaves the graph unscaled.

The cell layer adds Engine's `Cell` (`engine.rs`), the per-column sample the later stages fill in: its height value,
erosion, sediment, gradient, moisture and temperature, continent, terrain-region and biome-region ids and edges, the
terrain region's packed centre (a `long`), macro-biome id, river mask, continent cell coordinates, erosion mask,
terrain and biome type, with Engine's defaults (moisture and temperature 0.5, biome-region edge and river mask 1, the
`none` terrain, `GRASSLAND`). As in Engine, `copy_from` copies every field except the terrain region's centre, and
`reset` copies the defaults the same way, so the centre survives it; an empty cell (`Cell.empty()`) stays absent
through both. Engine's default terrain is the static `TerrainType.NONE`; here `Cell::new`, `Cell::empty` and `reset`
take the world's `TerrainRegistry` and use its `none`. Engine's `Cell.empty()` is one shared mutable object and its
cells come from per-thread pools; here `empty` returns a new cell and callers own their cells. It also adds
`BiomeType`'s eleven types in Engine's order with their ids and `isExtreme`; their colours and the climate ranges and
lookup map Engine loads from TerraForged's biome-type image come later. Its test checks every field's bits after
construction, `copy_from` and `reset` against a Java oracle.

The terrain-populator layer adds Engine's `TerrainPopulator` and `ScaledPopulator` (`populator.rs`), which write one
landform into a cell: `value` becomes base plus variance (each multiplied by its scale in `ScaledPopulator`), clamped
to 0..1 with NaN left as NaN, and `terrain` becomes the populator's terrain, which must come from the world's
`TerrainRegistry`; no other cell field changes. `TerrainPopulator.of` picks `ScaledPopulator` unless the base and
vertical scales are both 1, and `TerrainConfig` keeps a populator's terrain, variance and weight to rebuild it on
another base (its data spec is not ported). Engine's `Populator` is also a noise `Module`; here its `getValue` is
`Populator::value`, which takes the world's registry for its scratch cell. Its test checks every cell field, on a
new and on a fully set cell, against a Java oracle running Engine's populators on Engine cells, including the hills
and mountains landforms; the oracle lives outside Git, in `.local/terraforged/oracles/populator/`.

The region-module layer adds Engine's `RegionModule` and `RegionConfig` (`region.rs`), the populator that splits the
world into terrain regions. It offsets the point by a `DomainWarp` of the config's two axes at its strength, scales it
by `1 / scale` (the region size; Engine's `Heightmap` passes `terrainRegionSize`, 1200 by default, and 1-octave simplex
axes at scale 400 with a strength of 200) and finds the nearest of the 3×3 neighbouring cells' feature points (jitter
0.7, natural distance, seeded with the config's seed plus 7, wrapping like Java's `int`). It writes only three cell
fields: `terrain_region_id`, the nearest cell's `valCoord2D` mapped to 0..1; `terrain_region_edge`, the
`DISTANCE_2_DIV` edge value mapped to 0..1, inverted, raised to 1.5 (`NoiseUtil.pow`) and stretched from 0..0.5 to
0..1; and `terrain_region_center`, the nearest feature point in blocks packed like `PosUtil.pack` (each half truncated
like Java's `(int)`: saturating, NaN to 0). Its test checks every cell field, on a new and on a fully set cell, over a
grid of negative, large, NaN and infinite coordinates against a Java oracle running Engine's `RegionModule` on Engine
cells, for `Heightmap`'s configuration and for overflowing seeds and zero, negative and huge scales and strengths; the
oracle lives outside Git, in `.local/terraforged/oracles/region/`.

The region-selection layer adds Engine's `RegionSelector` and `RegionLerper` (`region.rs`), which Engine's `Heightmap`
uses to pick each terrain region's landform. `RegionSelector` repeats each populator, in list order, by its weight
divided by the smallest non-zero weight (rounded like `Math.round`; a populator that is not a `TerrainPopulator`, such
as a selector, counts as weight 1, and zero-weight populators are left out), falling back to the unweighted list when
no weight is non-zero or every count rounds to 0 (a NaN weight does this, through `Math.min`). It then applies the
populator at `NoiseUtil.round(terrain_region_id * maxIndex)` (`maxIndex` is the repeated list's length minus 1); where Engine throws for an index outside the list,
this port panics. `RegionLerper` reads `terrain_region_edge`: at exactly 0 it applies only its lower populator, at
exactly 1 only its upper one, and otherwise both on the same cell, setting `value` to `NoiseUtil.lerp` of the two
(other fields, the terrain included, keep what the upper one wrote). Heightmap passes a FLATS plains populator as the
lower one (the region borders) and the selector as the upper one. Its test checks the weighted lists and every cell
field against a Java oracle running Engine's selector and lerper on Engine cells, including Heightmap's chain
(`RegionModule`, then the lerper) over real landforms with ModTerrains' weights; the oracle lives outside Git, in
`.local/terraforged/oracles/region_selection/`.

The volcano layer adds Engine's `VolcanoPopulator` (`populator.rs`), the populator TerraForged registers for volcano
regions. It writes only `value` and `terrain`: the levels' ground plus a cone built from the region config's cell edge
distance and scaled by a per-cell height, turning down into a throat near its top, where the terrain becomes
`volcano_pipe`, and blending into ridged lowlands at its foot, where it is `volcano`; between those bands the cell keeps
its terrain. It draws four seeds in Engine's order and weighs in `RegionSelector` by its own weight, as a
`TerrainPopulator` does. `modify_volcano_type` (run by Heightmap after the rivers) turns a `volcano_pipe` cell below
the water level or with a river mask under 0.85 back into `volcano`. Where Engine's cell lookup throws (non-finite
coordinates) this port panics. Its test checks every cell field over grids and around pipes, for Heightmap's region
config, a small one and overflowing seeds, plus `modifyVolcanoType` at its edges and a `RegionSelector` holding
volcanoes, against a Java oracle running Engine's classes on Engine cells; the oracle lives outside Git, in
`.local/terraforged/oracles/volcano/`.

The standard-provider layer adds Engine's `StandardTerrainProvider` (`provider.rs`): it registers Engine's default
landforms in Engine's order, each drawing its seeds from the provider's seed (the world seed offset by
`terrainSeedOffset`): steppe and plains as mixable flats, dales, both hills and torridonian as mixable hills, plateau and
badlands as mixable, then a second badlands, the three mountains and the volcano as unmixable. Each landform becomes a
`TerrainPopulator`, or a `ScaledPopulator` when its settings scale it, on the land base. `getPopulators` keeps the
mixable populators with a weight above 0 (so a NaN weight drops one too), adds a composite for every pair of them (a
Perlin control at half the region size, warped by a Perlin warp, blending the two variances; registering
`lower-higher` composite terrains, or the terrain itself for two populators of one terrain, and drawing two seeds
each), then the unmixable populators, and shuffles them as `Collections.shuffle` does with a `java.util.Random`
seeded from the next seed; every call draws new seeds and so returns a different order. Composites go into the
world's registry, which the caller passes in, so populator terrains are the world's own (`is_same`). Variant lookups
fall back to the default populator for a negative variant or an unregistered terrain and cap high variants at the
last one; name lookups compare like Java's `equalsIgnoreCase`. Engine's `GeneratorContext` is not ported: the
provider takes the world seed, levels and terrain settings directly. Its tests rebuild a Java oracle's text over two
configurations (Engine's defaults; a seed offset, zero and NaN weights, scaled landforms and a negative long seed):
the seed cursor, registered populators, variant and name lookups, both shuffled lists with samples on a grid, and the
registry's composites; the oracle lives outside Git, in `.local/terraforged/oracles/provider/`.

The heightmap-blending layer adds Engine's `Blender`, `ContinentLerper2` and `ContinentLerper3` (`blend.rs`), the
three populators Heightmap chains into its root: continent-edge lerps from deep to shallow ocean to coast, and from
those oceans to land, where a mountain-shape module blends region terrain with mountain chains. Below a range's lower
bound only the lower populator applies and above its upper bound only the upper one (both bounds belong to the blend);
inside it both apply to the same cell in turn and `value` is lerped (`ContinentLerper2` linearly by default,
`ContinentLerper3` with Curve3, `Blender` always linearly). `Blender` keeps the lower populator's terrain while its
select is below the midpoint the split sets; the lerpers leave the terrain the last populator wrote. NaN selects fall
through to the blend, as in Java. A `Blender` controlled by a populator reads it as Engine's `getValue` does, on a fresh
cell of the world's registry; its tag threshold is stored and, as in Engine, never read. Nothing wires them into world
generation yet. Their tests check every cell field and `getValue` at each boundary, one ulp either side, signed zeros,
NaN and infinities, degenerate and reversed ranges, populators that read the cell, and Heightmap's root chain over a
grid, against a Java oracle running Engine's classes on Engine cells; the oracle lives outside Git, in
`.local/terraforged/oracles/blend/`.

The position-utility layer adds Engine's `PosUtil`, `PosIterator` and `MutableVeci` (`util.rs`). `pack` puts two ints
in a `long` (left in the high half); the float variants either truncate like Java's `(int)` (`pack(float, float)`, which
`RegionModule`'s region centre now uses) or keep the raw bits, NaN payloads included, as `floatToRawIntBits` does
(`packf`, `packMix`). `contains` is half-open, `iterate` visits a rectangle row by row, and `PosIterator` walks x, then
z, then y with Engine's quirks: moving to the next y layer does not advance the index, its size is `w * h * l - 1`, and
all of it wraps like Java `int`s. Its tests fold every result over 0, -1, `MIN`/`MAX`, quiet and signalling NaNs,
-0.0, infinities, subnormals and the `(int)` saturation edges, and every iterator factory (Engine's six-argument
`range2D` is `range_3d` here), against a Java oracle running Engine's classes; the oracle lives outside Git, in
`.local/terraforged/oracles/posutil/`.

The simple-continent layer adds Engine's `MultiContinentGenerator` and `SingleContinentGenerator`
(`continent.rs`): continents are jittered cells of a cellular lattice (frequency `1 / (4 * continentScale)`, the
configured shape as distance function, `continentJitter` as jitter) sampled through two compounded domain warps, and
their `DISTANCE_2_DIV` edge is remapped from 0.2..1 to 0..1 and faded by a simplex shape mask below the `inland` control point.
`apply` writes only `continentId`, `continentEdge`, `continentX` and `continentZ`; the single-continent generator
zeroes those four outside the continent nearest to (0, 0), so its `getValue` reads 0 there while `getEdgeValue`
does not. It also ports the nearest-centre lookup and the edge and ocean distance searches rivers use; the river
map itself is not ported yet. Its tests fold every cell field and every lookup over a grid with -0, huge, infinite
and NaN coordinates for six configs (all three shapes, jitter 0, 1 and NaN, scale 1 and `Integer.MAX_VALUE`, custom
control points), against a Java oracle running Engine's classes on Engine cells; the oracle lives outside Git, in
`.local/terraforged/oracles/continent/`.

The advanced-continent layer adds Engine's `AdvancedContinentGenerator` (`advanced_continent.rs`), the generator
of Engine's default continent type: continents are jittered cells of a lattice (frequency `1 / (4 * continentScale)`)
sampled through a Perlin2 domain warp built from the continent noise octaves, gain and lacunarity, and their edge
is the distance to the nearest cell bisector, scaled per continent by `continentSizeVariance`, remapped and shaped
by cliff and bay noise near the coast. `apply` writes only `continentId`, `continentEdge`, `continentX` and
`continentZ` (the centre pulled 35% towards its neighbours' mean), and nothing at all for a continent that
`continentSkipping` skips (never the one at cell 0, 0), so `getEdgeValue` and `getNearestCenter` read 0 there. Engine
does not override `getValue`, so it reads the scratch cell's untouched `value`: always 0. The edge and ocean distance
searches rivers use are ported; the river map is not. One difference: Engine throws for a negative
`continentNoiseOctaves` (an octave table lookup out of bounds); the port uses 0 octaves. Its tests fold every cell
field and every lookup over the simple-continent grid for six configs (skipping and size variance off, 1 and NaN,
scale 0, 1, 2 and `Integer.MAX_VALUE`, jitter 0 and NaN, zero octaves with a negative gain, custom control
points), against a second Java oracle in `.local/terraforged/oracles/continent/`.

The fancy-island layer adds the island geometry of Engine's fancy continents (`fancy.rs`): `Segment`, a line
segment with a scale at each end, and `Island`, a polyline of segments with its centre (the mean of its points), its
bounding box (the segments' box grown by the coast distance, then by the coast times the largest scale, at least 1), overlap and
containment tests (strict), and translation. `getEdgeValue` reads the squared distance to the nearest segment,
divided by the squared scale interpolated along it, and maps it to 0 beyond the deep ocean, up to 0.25 at the
shallow ocean, up to 1 at the coast and 1 inland; `getLandValue` curves the distance inside the shallow ocean. As
in Engine, the box's maximum starts at `Float.MIN_VALUE`, and a zero-length segment or a NaN coordinate makes the
distance NaN (Java's `Math.min` keeps it). Its tests fold every segment field, the bounds and the edge, land and containment samples over a grid
with -0, subnormal, huge, infinite and NaN coordinates, before and after translation, for seven islands (empty,
zero coast, zero-length, NaN and infinite segments, zero and NaN scales), against a third Java oracle in
`.local/terraforged/oracles/continent/`.

The fancy-continent layer ports `FancyContinent` and `FancyContinentGenerator` (Engine's experimental continent
type, `fancy.rs`). The continent draws from a `java.util.Random` seeded with the generator's first seed: a main
island of three segments (id 0), then for each of four directions one yaw and three ever smaller islands (ids 1 to 12
in build order), each placed one radius away from the previous island of its arm, with `NoiseUtil`'s sin/cos lookup.
Its value is the largest island edge value, its land value the largest island land value, and `getValueId` packs the
id of the first island with the largest edge value (-1 when none is above 0) with that value. The generator samples it
through three summed domain warps (Simplex at half the continent scale, then Perlin at 80 and 20 blocks), maps the land
value from 0.2..0.4, writes only `continentEdge` (and 0 to `continentX`, `continentZ` and `continentId`), and its
`getValue` is Engine's default: the scratch cell's `value`, always 0. As in Engine, `getNearestCenter` ignores its
arguments and returns half of `getMax - getMin` in blocks, and `getMax` takes the largest island *minimum*, starting
at `Float.MIN_VALUE`. The river map it holds (`FancyRiverGenerator`) is not ported. Its tests fold every island's
geometry, `getMin`/`getMax`, the continent's values over a grid in continent units and every cell field and lookup
over the block grid for seven configs (continent scale 0, 1, 2, -3001 and `Integer.MAX_VALUE`, seeds at the `int`
edges, custom control points), against a fourth Java oracle in `.local/terraforged/oracles/continent/`.

The river-configuration layer adds Engine's river settings and the configs its river map builds from them
(`river.rs`). `RiverSettings` (in `engine.rs`) keeps Engine 0.3.0's defaults: seed offset 0, 8 main rivers per
continent; main rivers with bed depth 5, bank heights 2 to 6, bed width 8, bank width 20 and fade 0.75; branch
rivers with 4, 1 to 4, 5, 14 and 0.975; lakes with chance 0.3, start distance 0 to 0.03, depth 10, size 75 to 150
and bank heights 2 to 10; wetlands with chance 0.6 and size 175 to 225. `RiverConfig` and its builder (defaults
bed width 4, bank width 15, bed depth 5, bank heights 1, length 1000, fade 0.2) turn the block counts into heights
normalized by `Levels` (`water(-depth)`, `water(bank)`), with `length2 = length * length` wrapping as a Java `int`;
Engine's river generator builds main rivers with length 5000 and branches with 4500. `createFork` scales a river's
widths down to a connecting width (rounded with `NoiseUtil.round`) and drops its banks one block, to at least
ground level. `LakeConfig` subtracts its sizes as floats, and `WetlandConfig` keeps Engine's `Variance.of(sizeMin,
sizeMax)`, whose second argument is a range: wetlands are 175 to 400 blocks long, not 175 to 225. These are
Engine's settings, not what TerraForged 0.3.1 carves with: TerraForged's `NoiseGenerator` builds Engine's `Settings`
with these river defaults untouched, but its own continent noise never builds Engine's river map and carves rivers
with its own continent river config (`ContinentConfig.rivers`: seed 0, lake density 0.75; rivers with erosion 0.075, bed width
1 to 7, bank width 3 to 30, valley width 80 to 200, bed depth 1.25 to 5 and bank depth 1.25 to 3; lakes with bed
width 8 to 15, bank width 30 to 45, valley width 80 to 120, bed depth 2 to 8 and bank depth 1 to 1.5), which this
layer does not port. Its tests fold every field of the river, fork, lake, wetland and variance configs over edge
cases (`int` limits, NaN, infinite and negative widths, six `Levels`, including TerraForged's swapped
`Levels(63, 255)`), against a Java oracle in `.local/terraforged/oracles/river/`, which also prints TerraForged's
own config.

The river-warp layer adds Engine's `GenWarp` (`river.rs`), the pair of domain warps its river generators build per
continent to carve lakes and river channels. The lake warp adds a Perlin `Domain.warp` with scale 200, 1 octave and
strength 300 to one with scale 50, 2 octaves and strength 50; the river warp adds scale 95, 1 octave, strength 25 to
scale 16, 1 octave, strength 5. As in Engine, the four warps take seeds `seed + 1` to `seed + 4` (wrapping as Java
`int`s), each warp's y noise uses its seed + 1, the continent scale argument is ignored and `GenWarp.EMPTY` is
`GenWarp::direct()`. Engine's private, unused `scaleMegaWarp` is kept as `scale_mega_warp`. Nothing uses these warps
yet: the river networks that do are not ported. Its tests fold every warp's offsets and coordinates over a grid with
-0, huge, infinite and NaN coordinates for eight seeds (the `int` edges among them) against a Java oracle running
Engine's `GenWarp`, `GenWarpOracle.java` in `.local/terraforged/oracles/river/`.

The river-geometry layer adds Engine's `River` and `RiverWarp` (`river.rs`), the channel segments its river
generators lay out. A `River` keeps its direction, normal (NaN for a zero-length segment, as in Engine), length
(`Math.sqrt` of the `float` squared length) and bounds padded by twice the valley radius (275 blocks, 300 from
nodes), and samples positions, angles, segment intersections (optionally extended at both ends), bound overlaps
and shortened copies (`NoiseUtil.round`). A `RiverWarp` bends a river sideways by simplex noise, faded in over
`0..lower` and out over `upper..1`, plus a wiggle of 2 to 45 blocks faded in over the first 7.5% of the river;
its `create` and `createChild` draw from `java.util.Random` in Engine's order. Engine compares `RiverWarp.NONE` by
identity; the port marks it with a flag. Nothing uses these yet: the river generators are not ported. Its tests
fold every field and method over 2560 segments (zero-length, NaN, infinite and -0 ends, odd radii), every pair's
intersections and overlaps, and warps with degenerate and NaN fades over distances outside 0..1, against a Java
oracle running Engine's classes, `RiverGeometryOracle.java` in `.local/terraforged/oracles/river/`.

The river-carver layer adds Engine's `RiverCarver` (`river.rs`), which carves one river segment into a cell. Within
the valley radius (275 blocks by default; `create` draws 0.8 to 1.5 times that) it lowers `value` toward the bank
height along the valley curve and lowers `riverMask`; nearer in, the banks (widened toward the mouth, where the
cell's continent edge is below 0.5) and then the bed lower `value` to the bed height, setting `erosionMask` and, at or below the water
level, the world registry's `river` terrain, except over wetland-style terrains that override rivers outside
bed-to-water heights. Widths and depths taper in over the first `fade` of the river. `createForkConfig` sizes a fork
from the bed and banks at a point, `getValleyType` and `create` draw from `java.util.Random` in Engine's order. Its
`TerrainPopulator` base (an empty `apply`) and its unused `compareTo` are not ported. Nothing uses the carver yet:
the river generators are not ported. Its tests fold every cell field over 337920 carves of fresh and filled cells
(river, wetland, lake and mountain terrains, NaN values, masks and edges) at points on, beside, before and past
four rivers (zero-length and NaN among them), for degenerate configs (wrapping widths, NaN heights, zero and NaN
fades), plus fork configs and random draws, against a Java oracle running Engine's class, `RiverCarverOracle.java`
in `.local/terraforged/oracles/river/`.

The lake layer adds circular beds, depth blending, shoreline valleys, bank-height interpolation,
river-mask shaping, overlap checks and the float bounds accumulator used by river networks.

## Status: development only

These layers only add the noise functions, Engine's terrain types, its world levels, its settings, its landforms, its cell model, its terrain populators, its terrain regions, its volcano populator, its standard terrain provider, its heightmap blenders, its position utilities, its simple and advanced continent generators, the fancy continents (island geometry and generator), Engine's river configuration, its river warps, its river geometry and its river carver. They change nothing in the game, so there is
nothing for players to install yet. Install instructions come with the layer that wires TerraForged into world generation.

## Development

`noise.rs` and `engine.rs` are ordinary Rust, each self-contained, and can be tested without Minecraft resources. CI
runs the same commands for both files, in debug and with `-O` (`.github/workflows/terraforged.yml`):

```sh
rustc --edition 2024 --test native-mods/terraforged/noise.rs -o /tmp/terraforged-tests
/tmp/terraforged-tests
rustc --edition 2024 --test native-mods/terraforged/engine.rs -o /tmp/terraforged-engine-tests
/tmp/terraforged-engine-tests
```

The mixin pins Rusticraft commit `1b6e55d4f3bd14079551bcad173d0fac8f8225bb`, a `main` commit rather than a Nightly:
no Nightly is close enough (the nearest earlier one, `nightly-source/2026-10-31-d91cab6`, is 795 commits older and
`generator.rs` differs by 150 lines). The pin moves to a Nightly when the series is rebased onto one.

Translation inputs are TerraForged commit `6dd607ebfd41a3b7274e090b1f2cf565d53d1d44` and Engine 0.3.0 bytecode
shaded into the official `TerraForged-1.18.2-0.3.1-alpha-2.jar`. Tests compare raw `f32` bits with
values produced by those Java classes. The small Java programs that printed them run against that jar, so they stay
out of Git, in the maintainer's `.local/terraforged/oracles/` folder.
