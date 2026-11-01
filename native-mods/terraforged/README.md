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
then maps the result from -1..1 to 0..1. It reports the `Noise` defaults 0..1. Its tests check raw bits against a Java
oracle for all three branches.

## Status: development only

This layer only adds the noise functions. It changes nothing in the game, so there is nothing for players to install
yet. Install instructions come with the layer that wires TerraForged into world generation.

## Development

The noise layer is ordinary Rust and can be tested without Minecraft resources. CI runs the same commands
(`.github/workflows/terraforged.yml`):

```sh
rustc --edition 2024 --test native-mods/terraforged/noise.rs -o /tmp/terraforged-tests
/tmp/terraforged-tests
```

The mixin pins Rusticraft commit `1b6e55d4f3bd14079551bcad173d0fac8f8225bb`, a `main` commit rather than a Nightly:
no Nightly is close enough (the nearest earlier one, `nightly-source/2026-10-31-d91cab6`, is 795 commits older and
`generator.rs` differs by 150 lines). The pin moves to a Nightly when the series is rebased onto one.

Translation inputs are TerraForged commit `6dd607ebfd41a3b7274e090b1f2cf565d53d1d44` and Engine 0.3.0 bytecode
shaded into the official `TerraForged-1.18.2-0.3.1-alpha-2.jar`. Tests compare raw `f32` bits with
values produced by those Java classes. The small Java programs that printed them run against that jar, so they stay
out of Git, in the maintainer's `.local/terraforged/oracles/` folder.
