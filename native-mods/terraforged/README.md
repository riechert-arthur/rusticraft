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
