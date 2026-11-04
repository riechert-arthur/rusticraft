/*
 * MIT License
 *
 * Copyright (c) 2021 TerraForged
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */

use std::sync::Arc;

use super::{
    terraforged::{
        CellFunction, DistanceFunction, Domain, EdgeFunction, Module, Source, SourceConfig,
        SourceKind, cell_vector, clamp, copy_sign, floor, java_max, java_min, map, pow_int, round,
        sin, value_coord_2d,
    },
    terraforged_engine::{
        BiomeType, Cell, ControlPoints, Seed, Terrain, TerrainProperties, TerrainRegistry,
        WorldControlPoints,
    },
};

/// Engine's `ClimateSettings.RangeValue`: one climate axis (temperature or moisture). `Default` is Engine's
/// no-argument constructor (scale 7, falloff 1, range 0..1, no bias); Engine's field initializers (falloff 2,
/// bias -0.1) are always overwritten by its constructors. The serializer's `@Range` limits are not enforced.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClimateRange {
    pub seed_offset: i32,
    pub scale: i32,
    pub falloff: i32,
    pub min: f32,
    pub max: f32,
    pub bias: f32,
}

impl ClimateRange {
    /// Engine's `RangeValue(scale, falloff, min, max, bias)`.
    pub const fn new(scale: i32, falloff: i32, min: f32, max: f32, bias: f32) -> Self {
        Self {
            seed_offset: 0,
            scale,
            falloff,
            min,
            max,
            bias,
        }
    }

    /// `getMin`: the smaller bound clamped to 0..1 (`Math.min` and `NoiseUtil.clamp`, so NaN passes through).
    pub fn min(self) -> f32 {
        clamp(java_min(self.min, self.max), 0.0, 1.0)
    }

    /// `getMax`: the larger bound clamped to `getMin()..1`.
    pub fn max(self) -> f32 {
        clamp(java_max(self.min, self.max), self.min(), 1.0)
    }

    /// `getBias`: the bias clamped to -1..1.
    pub fn bias(self) -> f32 {
        clamp(self.bias, -1.0, 1.0)
    }

    /// `apply(Module)`: `module.bias(getBias() / 2).clamp(getMin(), getMax())`.
    pub fn apply(self, module: Module) -> Module {
        let (min, max, bias) = (self.min(), self.max(), self.bias() / 2.0);
        module
            .bias(Module::constant(bias))
            .clamp(Module::constant(min), Module::constant(max))
    }
}

impl Default for ClimateRange {
    fn default() -> Self {
        Self::new(7, 1, 0.0, 1.0, 0.0)
    }
}

/// Engine's `ClimateSettings.BiomeShape`, with Engine's defaults.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BiomeShape {
    pub biome_size: i32,
    pub macro_noise_size: i32,
    pub biome_warp_scale: i32,
    pub biome_warp_strength: i32,
}

impl BiomeShape {
    /// Engine's `BiomeShape.DEFAULT_BIOME_SIZE`.
    pub const DEFAULT_BIOME_SIZE: i32 = 225;
}

impl Default for BiomeShape {
    fn default() -> Self {
        Self {
            biome_size: Self::DEFAULT_BIOME_SIZE,
            macro_noise_size: 8,
            biome_warp_scale: 150,
            biome_warp_strength: 80,
        }
    }
}

/// Engine's `ClimateSettings.BiomeNoise` (the biome-edge noise), with Engine's defaults. `kind` stands for
/// Engine's `Source` enum: only its octave noises are ported, and Engine's `SIMPLEX_RIDGE` builds a `Simplex2`
/// noise (its enum entry points at `Builder::simplex2`), so [`SourceKind::SimplexRidge`] does too here.
/// Engine's enum has no billow entry.
#[derive(Clone, Copy)]
pub struct BiomeNoise {
    pub kind: SourceKind,
    pub scale: i32,
    pub octaves: i32,
    pub gain: f32,
    pub lacunarity: f32,
    pub strength: i32,
}

impl BiomeNoise {
    /// `build(seed)`: `Source.build(seed, scale, octaves).gain(gain).lacunarity(lacunarity).build(type)
    /// .bias(-0.5)`. A negative octave count builds no octaves here; Engine throws for it
    /// (`ArrayIndexOutOfBoundsException` in the noise's bound).
    pub fn build(self, seed: i32) -> Module {
        let kind = match self.kind {
            SourceKind::SimplexRidge => SourceKind::Simplex2,
            kind => kind,
        };
        Module::source(Source::new(
            kind,
            SourceConfig {
                seed,
                octaves: usize::try_from(self.octaves).unwrap_or(0),
                gain: self.gain,
                frequency: 1.0 / self.scale as f32,
                lacunarity: self.lacunarity,
                ..SourceConfig::default()
            },
        ))
        .bias(Module::constant(-0.5))
    }
}

impl Default for BiomeNoise {
    fn default() -> Self {
        Self {
            kind: SourceKind::Simplex,
            scale: 24,
            octaves: 2,
            gain: 0.5,
            lacunarity: 2.65,
            strength: 14,
        }
    }
}

/// Engine's `ClimateSettings`, with Engine's defaults: what [`ClimateModule`] reads.
#[derive(Clone, Copy)]
pub struct ClimateSettings {
    pub temperature: ClimateRange,
    pub moisture: ClimateRange,
    pub biome_shape: BiomeShape,
    pub biome_edge_shape: BiomeNoise,
}

impl Default for ClimateSettings {
    fn default() -> Self {
        Self {
            temperature: ClimateRange::new(6, 2, 0.0, 0.98, 0.05),
            moisture: ClimateRange::new(6, 1, 0.0, 1.0, 0.0),
            biome_shape: BiomeShape::default(),
            biome_edge_shape: BiomeNoise::default(),
        }
    }
}

/// Engine's `Compressor` (`world.climate`): squeezes `module`'s middle band `lower_start..upper_end` into
/// `lower_end..upper_start` and stretches the two ends to fill the rest of 0..1. All float math in Engine's order.
#[derive(Clone)]
pub struct Compressor {
    lower_start: f32,
    lower_end: f32,
    lower_range: f32,
    lower_expand_range: f32,
    upper_start: f32,
    upper_end: f32,
    upper_range: f32,
    upper_expanded_range: f32,
    compression: f32,
    compression_range: f32,
    module: Module,
}

impl Compressor {
    /// `new Compressor(module, inset, amount)`.
    pub fn new(module: Module, inset: f32, amount: f32) -> Self {
        Self::with_ranges(
            module,
            inset,
            inset + amount,
            1.0 - inset - amount,
            1.0 - inset,
        )
    }

    /// `new Compressor(module, lowerStart, lowerEnd, upperStart, upperEnd)`.
    pub fn with_ranges(
        module: Module,
        lower_start: f32,
        lower_end: f32,
        upper_start: f32,
        upper_end: f32,
    ) -> Self {
        Self {
            module,
            lower_start,
            lower_end,
            lower_range: lower_start,
            lower_expand_range: lower_end,
            upper_start,
            upper_end,
            upper_range: 1.0 - upper_end,
            upper_expanded_range: 1.0 - upper_start,
            compression: upper_start - lower_end,
            compression_range: upper_end - lower_start,
        }
    }

    pub fn value(&self, x: f32, y: f32) -> f32 {
        let value = self.module.value(x, y);
        if value <= self.lower_start {
            let alpha = value / self.lower_range;
            return alpha * self.lower_expand_range;
        }
        if value >= self.upper_end {
            let delta = value - self.upper_end;
            let alpha = delta / self.upper_range;
            return self.upper_start + alpha * self.upper_expanded_range;
        }
        let delta = value - self.lower_start;
        let alpha = delta / self.compression_range;
        self.lower_end + alpha * self.compression
    }

    /// As a graph node, with the `Noise` default bounds 0..1 (Engine doesn't override them).
    pub fn module(self) -> Module {
        Module::custom(0.0, 1.0, move |x, y| self.value(x, y))
    }
}

/// Engine's `Temperature`: a latitude band, `sin(y * frequency)` from `NoiseUtil`'s lookup table raised to `power`
/// (sign kept) and mapped from -1..1 to 0..1. `x` is unused.
#[derive(Clone, Copy)]
pub struct Temperature {
    power: i32,
    frequency: f32,
}

impl Temperature {
    /// `new Temperature(frequency, power)`.
    pub const fn new(frequency: f32, power: i32) -> Self {
        Self { power, frequency }
    }

    pub fn value(self, _x: f32, y: f32) -> f32 {
        let sin = clamp(sin(y * self.frequency), -1.0, 1.0);
        let value = copy_sign(pow_int(sin, self.power), sin);
        map(value, -1.0, 1.0, 2.0)
    }

    /// As a graph node, with the `Noise` default bounds 0..1.
    pub fn module(self) -> Module {
        Module::custom(0.0, 1.0, move |x, y| self.value(x, y))
    }
}

/// Engine's `Moisture`: `source` sampled at half the x frequency; from `power` 2 up, recentred to -1..1, raised to
/// `power` (sign kept) and mapped back to 0..1.
#[derive(Clone)]
pub struct Moisture {
    source: Module,
    power: i32,
}

impl Moisture {
    /// `new Moisture(seed, scale, power)`: `Source.simplex(seed, scale, 1).clamp(0.125, 0.875).map(0, 1)`.
    pub fn new(seed: i32, scale: i32, power: i32) -> Self {
        let source = Module::source(Source::new(
            SourceKind::Simplex,
            SourceConfig {
                seed,
                frequency: 1.0 / scale as f32,
                ..SourceConfig::default()
            },
        ))
        .clamp(Module::constant(0.125), Module::constant(0.875))
        .map(Module::constant(0.0), Module::constant(1.0));
        Self::from_module(source, power)
    }

    /// `new Moisture(source, power)`: wraps `source.freq(0.5, 1.0)`.
    pub fn from_module(source: Module, power: i32) -> Self {
        Self {
            source: source.frequency(Module::constant(0.5), Module::constant(1.0)),
            power,
        }
    }

    pub fn value(&self, x: f32, y: f32) -> f32 {
        let mut noise = self.source.value(x, y);
        if self.power < 2 {
            return noise;
        }
        noise = (noise - 0.5) * 2.0;
        let value = copy_sign(pow_int(noise, self.power), noise);
        map(value, -1.0, 1.0, 2.0)
    }

    /// As a graph node, with the `Noise` default bounds 0..1.
    pub fn module(self) -> Module {
        Module::custom(0.0, 1.0, move |x, y| self.value(x, y))
    }
}

/// The part of Engine's `Continent` the climate module reads: `getLandValue(x, z)`. Any
/// `Fn(f32, f32) -> f32` closure is one.
pub trait LandValue: Send + Sync {
    fn land_value(&self, x: f32, z: f32) -> f32;
}

impl<F> LandValue for F
where
    F: Fn(f32, f32) -> f32 + Send + Sync,
{
    fn land_value(&self, x: f32, z: f32) -> f32 {
        self(x, z)
    }
}

/// Engine's `ClimateModule`: the warped Voronoi biome cells of the climate stage, with their region
/// id and edge, temperature, moisture, macro-biome noise and biome type.
pub struct ClimateModule {
    seed: i32,
    biome_frequency: f32,
    warp_strength: f32,
    warp_x: Module,
    warp_z: Module,
    moisture: Module,
    temperature: Module,
    macro_biome_noise: Module,
    continent: Arc<dyn LandValue>,
    control_points: ControlPoints,
    coast: Terrain,
}

impl ClimateModule {
    /// `new ClimateModule(continent, context)`: draws four seeds from `seed` (the context's seed),
    /// three from the moisture offset seed and two from the temperature one, in Engine's order. `settings`
    /// and `control_points` are the context's `settings.climate` and `settings.world.controlPoints`;
    /// `registry` is the world's, whose `coast` (`TerrainType.COAST`) the coast mask writes.
    pub fn new(
        continent: Arc<dyn LandValue>,
        seed: &mut Seed,
        settings: &ClimateSettings,
        control_points: WorldControlPoints,
        registry: &TerrainRegistry,
    ) -> Self {
        let biome_size = settings.biome_shape.biome_size;
        let temperature_scaler = settings.temperature.scale as f32;
        let moisture_scaler = settings.moisture.scale as f32 * 2.5;
        let biome_frequency = 1.0 / biome_size as f32;
        let moisture_size = moisture_scaler * biome_size as f32;
        let temperature_size = temperature_scaler * biome_size as f32;
        let moisture_scale = round(moisture_size * biome_frequency);
        let temperature_scale = round(temperature_size * biome_frequency);
        let warp_scale = settings.biome_shape.biome_warp_scale;
        let module_seed = seed.next();
        let warp_x = simplex(seed.next(), warp_scale, 2).bias(Module::constant(-0.5));
        let warp_z = simplex(seed.next(), warp_scale, 2).bias(Module::constant(-0.5));

        let mut moisture_seed = seed.offset(settings.moisture.seed_offset);
        let moisture = Moisture::new(
            moisture_seed.next(),
            moisture_scale,
            settings.moisture.falloff,
        );
        // The warp strengths are Java doubles (`moistScale / 4.0`, `tempScale * 4`) narrowed to float
        // by `Source.constant`.
        let moisture = settings
            .moisture
            .apply(moisture.module())
            .warp(perlin_warp(
                moisture_seed.next(),
                (moisture_scale / 2).max(1),
                1,
                f64::from(moisture_scale) / 4.0,
            ))
            .warp(perlin_warp(
                moisture_seed.next(),
                (moisture_scale / 6).max(1),
                2,
                f64::from(moisture_scale) / 12.0,
            ));

        let mut temperature_seed = seed.offset(settings.temperature.seed_offset);
        let temperature =
            Temperature::new(1.0 / temperature_scale as f32, settings.temperature.falloff);
        let temperature_scale4 = temperature_scale.wrapping_mul(4);
        let temperature = settings
            .temperature
            .apply(temperature.module())
            .warp(perlin_warp(
                temperature_seed.next(),
                temperature_scale4,
                2,
                f64::from(temperature_scale4),
            ))
            .warp(perlin_warp(
                temperature_seed.next(),
                temperature_scale,
                1,
                f64::from(temperature_scale),
            ));

        // `Source.cell(seed, macroNoiseSize)`: the builder's defaults (cell value, Euclidean,
        // displacement 1, lookup `Source.ZERO`).
        let macro_biome_noise = Module::cell_source(
            SourceConfig {
                seed: seed.next(),
                frequency: 1.0 / settings.biome_shape.macro_noise_size as f32,
                ..SourceConfig::default()
            },
            1.0,
            CellFunction::CellValue,
            DistanceFunction::Euclidean,
            Module::constant(0.0),
        );

        Self {
            seed: module_seed,
            biome_frequency,
            warp_strength: settings.biome_shape.biome_warp_strength as f32,
            warp_x,
            warp_z,
            moisture,
            temperature,
            macro_biome_noise,
            continent,
            control_points: ControlPoints::new(control_points),
            coast: registry.get_by_id(3),
        }
    }

    /// `apply(cell, x, y)`: [`Self::apply_masked`] with the mask.
    pub fn apply(&self, cell: &mut Cell, x: f32, z: f32) {
        self.apply_masked(cell, x, z, true);
    }

    /// `apply(cell, x, y, mask)`: finds the nearest of the 3x3 jittered cells around the warped point
    /// and writes, in Engine's order, the region id, moisture, temperature and macro-biome id sampled
    /// at its centre; with `mask` also the region edge and the coast mask (an overground terrain that
    /// doesn't override coasts becomes `coast` where the land value is at most the coast marker); then
    /// the continent-adjusted moisture and the biome type. Panics where Engine throws: a temperature
    /// or moisture whose biome-map index is outside 0..=255 (see [`BiomeType::get_curve`]).
    pub fn apply_masked(&self, cell: &mut Cell, mut x: f32, mut z: f32, mask: bool) {
        let offset_x = self.warp_x.value(x, z) * self.warp_strength;
        let offset_z = self.warp_z.value(x, z) * self.warp_strength;
        x = (x + offset_x) * self.biome_frequency;
        z = (z + offset_z) * self.biome_frequency;
        let x_region = floor(x);
        let z_region = floor(z);
        let mut cell_x = x_region;
        let mut cell_z = z_region;
        let mut center_x = x;
        let mut center_z = z;
        let mut edge_distance = 999_999.0;
        let mut edge_distance2 = 999_999.0;
        for delta_z in -1_i32..=1 {
            for delta_x in -1_i32..=1 {
                let candidate_x = x_region.wrapping_add(delta_x);
                let candidate_z = z_region.wrapping_add(delta_z);
                let vector = cell_vector(self.seed, candidate_x, candidate_z);
                let candidate_center_x = candidate_x as f32 + vector[0];
                let candidate_center_z = candidate_z as f32 + vector[1];
                let distance = DistanceFunction::Euclidean
                    .apply(candidate_center_x - x, candidate_center_z - z);
                if distance < edge_distance {
                    edge_distance2 = edge_distance;
                    edge_distance = distance;
                    center_x = candidate_center_x;
                    center_z = candidate_center_z;
                    cell_x = candidate_x;
                    cell_z = candidate_z;
                } else if distance < edge_distance2 {
                    edge_distance2 = distance;
                }
            }
        }

        cell.biome_region_id = map(value_coord_2d(self.seed, cell_x, cell_z), -1.0, 1.0, 2.0);
        cell.moisture = self.moisture.value(center_x, center_z);
        cell.temperature = self.temperature.value(center_x, center_z);
        cell.macro_biome_id = self.macro_biome_noise.value(center_x, center_z);
        let position_x = floor(center_x / self.biome_frequency);
        let position_z = floor(center_z / self.biome_frequency);
        let continent_edge = self
            .continent
            .land_value(position_x as f32, position_z as f32);
        if mask {
            cell.biome_region_edge = edge_value(edge_distance, edge_distance2);
            self.modify_terrain(cell, continent_edge);
        }
        modify_moisture(cell, continent_edge);
        cell.biome = BiomeType::at_climate(cell.temperature, cell.moisture);
    }

    fn modify_terrain(&self, cell: &mut Cell, continent_edge: f32) {
        // `cell.terrain = TerrainType.COAST`: the world registry's own coast.
        if cell.terrain.is_overground()
            && !cell.terrain.overrides_coast()
            && continent_edge <= self.control_points.coast_marker
        {
            cell.terrain = self.coast.clone();
        }
    }
}

/// `modifyMoisture`: wetter below a land value of 0.75 (clamped to 0..1), drier above it.
fn modify_moisture(cell: &mut Cell, continent_edge: f32) {
    let limit = 0.75;
    let range = 1.0 - limit;
    if continent_edge < limit {
        let alpha = (limit - continent_edge) / range;
        let multiplier = 1.0 + alpha * range;
        cell.moisture = clamp(cell.moisture * multiplier, 0.0, 1.0);
    } else {
        let alpha = (continent_edge - limit) / range;
        let multiplier = 1.0 - alpha * range;
        cell.moisture *= multiplier;
    }
}

/// `edgeValue`: `1 - DISTANCE_2_DIV` mapped from its bounds. Engine's `edgeClamp`/`edgeScale` (both
/// 1) are never read.
fn edge_value(distance: f32, distance2: f32) -> f32 {
    let edge = EdgeFunction::Distance2Div;
    let (min, max, range) = edge.bounds();
    1.0 - map(edge.apply(distance, distance2), min, max, range)
}

/// `Source.simplex(seed, scale, octaves)`.
fn simplex(seed: i32, scale: i32, octaves: usize) -> Module {
    source(SourceKind::Simplex, seed, scale, octaves)
}

/// `Module.warp(seed, scale, octaves, strength)`: a `DomainWarp` by two Perlin sources (`seed` and
/// `seed + 1`) and `Source.constant(strength)`.
fn perlin_warp(seed: i32, scale: i32, octaves: usize, strength: f64) -> Domain {
    Domain::domain_warp(
        source(SourceKind::Perlin, seed, scale, octaves),
        source(SourceKind::Perlin, seed.wrapping_add(1), scale, octaves),
        Module::constant(strength as f32),
    )
}

/// `Source.build(seed, scale, octaves).build(kind)`.
fn source(kind: SourceKind, seed: i32, scale: i32, octaves: usize) -> Module {
    Module::source(Source::new(
        kind,
        SourceConfig {
            seed,
            octaves,
            frequency: 1.0 / scale as f32,
            ..SourceConfig::default()
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Output of `.local/terraforged/oracles/climate/ClimateOracle.java` (run.sh) against Engine 0.3.0.
    const ORACLE: &str = r"
c 0 9e47820e6da50ed4
c 1 ca58e6dfac6956b6
c 2 b15cfb34f55662
c 3 5273a64c0ec741d1
c 4 9273261680a7d895
c 5 80030bc32344ce08
c 6 24b24dfdd719d007
c 7 d24edd85e3c0c4f
c 8 3f03b346e6bc0c4f
r 0 60ac147a37ddbfc9
r 1 5273a64c0ec741d1
r 2 7c8a7d78d89ab295
r 3 76cec387f5923ed
r 4 b15cfb34f55662
r 5 d24edd85e3c0c4f
t 0 a71d34adf7c0c4f 3b3565dd64f9381b 5a4af3056543c845 c31c8c02fbada914 eb41d1a1622f5639 f8aced0e8b906fb4 493273d6c6dd743a ac489289d28aaf4a a71d34adf7c0c4f a71d34adf7c0c4f a71d34adf7c0c4f
t 1 279b6a37af7c0c4f 99544e5f1ef1fb66 241c83784760e085 6529371a0ccf8e08 27421174319b0903 8de91d30e6622e28 7265c8edc838df7b b2c7557d40531fd5 279b6a37af7c0c4f 279b6a37af7c0c4f 279b6a37af7c0c4f
t 2 4835613d807c0c4f a4bc1ed3373ab69e 3cae512e5ecc94df 38525e1a85cf6e40 5f8ba0e0a7a8c15 d29a0e5f96a548a 157f8faa3a9818b6 dd97759e85120f48 4835613d807c0c4f 4835613d807c0c4f 4835613d807c0c4f
t 3 dc24d028bdfc0c4f a9a8c383b0ca03a8 9870f2533e993c4b 3805d0cabc605e0b f677f71000271fc6 c071d34661e70d9a f985fbb3b9c0fd2a b64092ce2db3523d dc24d028bdfc0c4f dc24d028bdfc0c4f dc24d028bdfc0c4f
t 4 1c15fe725dfc0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f 1c15fe725dfc0c4f 1c15fe725dfc0c4f 1c15fe725dfc0c4f
t 5 c73992f7d07c0c4f 56e8dccdf21d6b1c 62953ce1cf5e8066 9c332e986afa469e c38290360668337e e43a32d5e30a54c9 8a5cfbb19251f34d f5df9c0065fb324c c73992f7d07c0c4f c73992f7d07c0c4f c73992f7d07c0c4f
t 6 d45065ec8efc0c4f b45cb61faa0894c7 321711133bfb9c8c 7a170b075a6584f 9964ffa097f900ac 89d54141dac484d3 5583e7254f4e0070 18839cbbd6283f74 d45065ec8efc0c4f d45065ec8efc0c4f d45065ec8efc0c4f
t 7 1c15fe725dfc0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f 1c15fe725dfc0c4f 1c15fe725dfc0c4f 1c15fe725dfc0c4f
t 8 6f41f12ae07c0c4f 75092d206c1ccb9e ff99de31f284091d c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f c48b6b26dd7c0c4f 6f41f12ae07c0c4f 6f41f12ae07c0c4f 6f41f12ae07c0c4f
m 474ac1af09e680a2 474ac1af09e680a2 ecaecf1a11e7ffca 62f38e51983c5e4b 379e4519613f2205 1c5a0ebcf0e327fb 434562d962b96a3 66efb06b0ebf4632 474ac1af09e680a2 474ac1af09e680a2 474ac1af09e680a2
k 474ac1af09e680a2 474ac1af09e680a2 ecaecf1a11e7ffca 62f38e51983c5e4b 379e4519613f2205 1c5a0ebcf0e327fb 434562d962b96a3 66efb06b0ebf4632 474ac1af09e680a2 474ac1af09e680a2 474ac1af09e680a2
n 0 2d1077121f72a31e 2d1077121f72a31e d9ed3c1db03acb20 d04c1783cd1b995a 98b7297998dd6c15 74aa9106dc3dcc57 74aa9106dc3dcc57 e2d6bb4849a3731b 2e7a3485d05a0585 eaeefa3e50dbacac 44fc765383699a44 44fc765383699a44 ef5148e606c791d3 2465211565259e9f 813d040699cde75b 82ec65e973e9b192 82ec65e973e9b192 a38f7b11ccf5b9cc 71e1d471fbb443c5 f18151b0590d68fc 7f0f039ea7571d47 7f0f039ea7571d47 737000970bd9c8ca f7ba4da85f91cf7d d8152287d13a7a08 da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f
n 1 dc5e534bd879484d dc5e534bd879484d 241394aaf48d59f5 c3375527c9ab827f 4b7b4f1213c151b2 4ffe6833d0c45181 4ffe6833d0c45181 b08a2b9072bb2ade a708b8ba6a2e616c a2326e0013a85e86 3cbb8b606df8feec 3cbb8b606df8feec ed7f8bc2b57c693a fb117ebbf352ddc5 7b9f38d37b950941 3e4a32939b0a05cd 3e4a32939b0a05cd 31b02bbb26e14c75 410d28afe373d887 e7eb356198750bc1 6b23e5d49cf69304 6b23e5d49cf69304 85c7a8d60f8e1445 4b7bb65799ac90f8 bd49c8142ab9918a da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f
n -1 f127151d0a46af74 f127151d0a46af74 31ef3d0d1863941d 392e94cce69169a3 a4c442ab161ad328 c12bb785cde25e9b c12bb785cde25e9b d2395f675e714043 65bc3321769a910e 5882a33c881f1e55 163db6ea471abb98 163db6ea471abb98 d2993d444e488daf 13129ada9010fa0e b0b90e8847722e5f 3c3c37c455cdf89a 3c3c37c455cdf89a b8994c3ff582efb5 34a63a77e6c01670 75496c220e140bc9 e209daed716a5535 e209daed716a5535 cbc838e427df50d1 23026dece33bf884 87770f3e9a737f06 da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f
n 1337 be2b8d6c3e416dd4 be2b8d6c3e416dd4 5760169023b85dba 5b4fd348a22c17f1 efe2d6d72a9c886c 105f45640f45db89 105f45640f45db89 f9a4b30491077c9c 2fad81c433971775 d0149e49e29ca83 9ef33dabe9d1e111 9ef33dabe9d1e111 d8152f9de98e149b 27c06ba507b3cb79 257b303fee9378a8 4fc99c510d81941f 4fc99c510d81941f 8ba55a78e7668463 2fec0bf4ace64650 f04778d3a478e932 252aded5d22b948a 252aded5d22b948a d7609fb58106849d 8523781dedec75e 65ff27dc9b3c677a da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f
n 123456789 7edb3ce4924a3ace 7edb3ce4924a3ace c01cec06add5fcba ee531290189475a6 7bc4c691936433bb 12dfb842ca65977 12dfb842ca65977 2fdd730b353fe634 569b580e296b343b 431dc25a53c45f86 5ffc0df7c4451eed 5ffc0df7c4451eed 5e7216ca6a11a3d5 a1e106f57f450467 5b9b419ca5308c68 cfcdddbef358b249 cfcdddbef358b249 f4b4033f4ca9164a a35f9d5cbe793b43 91f1616e5663420c 4de0db2d426cca28 4de0db2d426cca28 3cf5ccb27db878a8 c160bc0a5e2fd111 1f2ca968f34ce945 da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f
n 2147483647 8648ff5f0fffa445 8648ff5f0fffa445 9111e0c0fdc69d01 6e933b288ce59014 6fb8d9768a4b2204 d12f9618c73207f1 d12f9618c73207f1 4e55aa790481b13a b03cc5de52f079ee 64267f85e5d16143 d5e1cfd2a6b1421 d5e1cfd2a6b1421 c95ac46e0b344eea bfe46355cfae13a6 b8644cfb3aa29280 c0038e4c9160ab11 c0038e4c9160ab11 5c7c4aa820f07428 d828f93dbc289636 44e3ac0be37eddae e4e47bcd1bc98e6f e4e47bcd1bc98e6f 7e675aa525e4019e c3a50e81de852ea7 4054f7a030590dd7 da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f
n -2147483648 e3ce1d5b8316cb3b e3ce1d5b8316cb3b 3eafc67c4368463e 5a7e2dd13f84614 5f07d8c158d7e6c1 493028a9d9bd9ad2 493028a9d9bd9ad2 2984462022c677c2 8acb2c74e508ed96 8bb7d732248588e9 60eb2df5257aa118 60eb2df5257aa118 7f3c636953fd0c22 ab14a5a2a8bbd45e 336e2b353604b9d1 5fffd8a50a6418ff 5fffd8a50a6418ff 1d66e3c7105ee2cd 939178bc9deb339d 59e349bb548d5873 369a6f284ec45604 369a6f284ec45604 d3552f5fbb355471 3da15ab874b79262 46a69b06746b706f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f da510e065e3c0c4f
s 3f000000 3f000000 3f1b40fc 3efc4e4a 3ef7a125 3ea0d008 3eeb3385 3f78daa6
";

    /// Output of `.local/terraforged/oracles/climate_settings/ClimateSettingsOracle.java` (run.sh) against Engine 0.3.0.
    const SETTINGS_ORACLE: &str = r"
d t 0 6 2 0 3f7ae148 3d4ccccd
d m 0 6 1 0 3f800000 0
d r 0 7 1 0 3f800000 0
d s 225 225 8 150 80
d e SIMPLEX 24 2 3f000000 4029999a 14
v 0 0:0:3c0a7a26ad651e56 80000000:0:3c0a7a26ad651e56 0:3e4ccccd:190f552be4d33d5f 0:3f000000:f5c5198ef07e7278 0:3f4ccccd:b05453da47ae8505 0:3f800000:4ceaf95ed0ea9b89 0:0:3c0a7a26ad651e56 0:3f800000:4ceaf95ed0ea9b89 0:3f800000:4ceaf95ed0ea9b89 0:0:3c0a7a26ad651e56 0:1:62bbb461bab7e796 0:0:3c0a7a26ad651e56 0:3f800000:4ceaf95ed0ea9b89 0:0:3c0a7a26ad651e56 7fc00000:7fc00000:1322b0c2aba351b2
v 1 80000000:0:3c0a7a26ad651e56 80000000:80000000:3c0a7a26ad651e56 80000000:3e4ccccd:190f552be4d33d5f 80000000:3f000000:f5c5198ef07e7278 80000000:3f4ccccd:b05453da47ae8505 80000000:3f800000:4ceaf95ed0ea9b89 0:80000000:3c0a7a26ad651e56 80000000:3f800000:4ceaf95ed0ea9b89 80000000:3f800000:4ceaf95ed0ea9b89 0:80000000:3c0a7a26ad651e56 80000000:1:62bbb461bab7e796 0:80000000:3c0a7a26ad651e56 80000000:3f800000:4ceaf95ed0ea9b89 0:80000000:3c0a7a26ad651e56 7fc00000:7fc00000:1322b0c2aba351b2
v 2 0:3e4ccccd:190f552be4d33d5f 80000000:3e4ccccd:190f552be4d33d5f 3e4ccccd:3e4ccccd:583026dc0a7892f6 3e4ccccd:3f000000:34e5eb3f1623c80f 3e4ccccd:3f4ccccd:ef75258a6d53da9c 3e4ccccd:3f800000:6dfe8280632092dc 0:3e4ccccd:190f552be4d33d5f 3e4ccccd:3f800000:6dfe8280632092dc 3e4ccccd:3f800000:6dfe8280632092dc 0:3e4ccccd:190f552be4d33d5f 1:3e4ccccd:46bb887838e1bb3f 0:3e4ccccd:190f552be4d33d5f 3e4ccccd:3f800000:6dfe8280632092dc 0:3e4ccccd:190f552be4d33d5f 7fc00000:7fc00000:1322b0c2aba351b2
v 3 0:3f000000:f5c5198ef07e7278 80000000:3f000000:f5c5198ef07e7278 3e4ccccd:3f000000:34e5eb3f1623c80f 3f000000:3f000000:ce99afa58d651e56 3f000000:3f4ccccd:8928e9f0e49530e3 3f000000:3f800000:7b246e6da61e923 0:3f000000:f5c5198ef07e7278 3f000000:3f800000:7b246e6da61e923 3f000000:3f800000:7b246e6da61e923 0:3f000000:f5c5198ef07e7278 1:3f000000:23714cdb448cf058 0:3f000000:f5c5198ef07e7278 3f000000:3f800000:7b246e6da61e923 0:3f000000:f5c5198ef07e7278 7fc00000:7fc00000:1322b0c2aba351b2
v 4 0:3f4ccccd:b05453da47ae8505 80000000:3f4ccccd:b05453da47ae8505 3e4ccccd:3f4ccccd:ef75258a6d53da9c 3f000000:3f4ccccd:8928e9f0e49530e3 3f4ccccd:3f4ccccd:e19188232a7892f6 3f4ccccd:3f800000:601ae51920454b36 0:3f4ccccd:b05453da47ae8505 3f4ccccd:3f800000:601ae51920454b36 3f4ccccd:3f800000:601ae51920454b36 0:3f4ccccd:b05453da47ae8505 1:3f4ccccd:de0087269bbd02e5 0:3f4ccccd:b05453da47ae8505 3f4ccccd:3f800000:601ae51920454b36 0:3f4ccccd:b05453da47ae8505 7fc00000:7fc00000:1322b0c2aba351b2
v 5 0:3f800000:4ceaf95ed0ea9b89 80000000:3f800000:4ceaf95ed0ea9b89 3e4ccccd:3f800000:6dfe8280632092dc 3f000000:3f800000:7b246e6da61e923 3f4ccccd:3f800000:601ae51920454b36 3f800000:3f800000:134a60491d651e56 0:3f800000:4ceaf95ed0ea9b89 3f800000:3f800000:134a60491d651e56 3f800000:3f800000:134a60491d651e56 0:3f800000:4ceaf95ed0ea9b89 1:3f800000:5c89e41c9189bb25 0:3f800000:4ceaf95ed0ea9b89 3f800000:3f800000:134a60491d651e56 0:3f800000:4ceaf95ed0ea9b89 7fc00000:7fc00000:1322b0c2aba351b2
v 6 0:0:3c0a7a26ad651e56 0:80000000:3c0a7a26ad651e56 0:3e4ccccd:190f552be4d33d5f 0:3f000000:f5c5198ef07e7278 0:3f4ccccd:b05453da47ae8505 0:3f800000:4ceaf95ed0ea9b89 0:0:3c0a7a26ad651e56 0:3f800000:4ceaf95ed0ea9b89 0:3f800000:4ceaf95ed0ea9b89 0:0:3c0a7a26ad651e56 0:1:62bbb461bab7e796 0:0:3c0a7a26ad651e56 0:3f800000:4ceaf95ed0ea9b89 0:0:3c0a7a26ad651e56 7fc00000:7fc00000:1322b0c2aba351b2
v 7 0:3f800000:4ceaf95ed0ea9b89 80000000:3f800000:4ceaf95ed0ea9b89 3e4ccccd:3f800000:6dfe8280632092dc 3f000000:3f800000:7b246e6da61e923 3f4ccccd:3f800000:601ae51920454b36 3f800000:3f800000:134a60491d651e56 0:3f800000:4ceaf95ed0ea9b89 3f800000:3f800000:134a60491d651e56 3f800000:3f800000:134a60491d651e56 0:3f800000:4ceaf95ed0ea9b89 1:3f800000:5c89e41c9189bb25 0:3f800000:4ceaf95ed0ea9b89 3f800000:3f800000:134a60491d651e56 0:3f800000:4ceaf95ed0ea9b89 7fc00000:7fc00000:1322b0c2aba351b2
v 8 0:3f800000:4ceaf95ed0ea9b89 80000000:3f800000:4ceaf95ed0ea9b89 3e4ccccd:3f800000:6dfe8280632092dc 3f000000:3f800000:7b246e6da61e923 3f4ccccd:3f800000:601ae51920454b36 3f800000:3f800000:134a60491d651e56 0:3f800000:4ceaf95ed0ea9b89 3f800000:3f800000:134a60491d651e56 3f800000:3f800000:134a60491d651e56 0:3f800000:4ceaf95ed0ea9b89 1:3f800000:5c89e41c9189bb25 0:3f800000:4ceaf95ed0ea9b89 3f800000:3f800000:134a60491d651e56 0:3f800000:4ceaf95ed0ea9b89 7fc00000:7fc00000:1322b0c2aba351b2
v 9 0:0:3c0a7a26ad651e56 0:80000000:3c0a7a26ad651e56 0:3e4ccccd:190f552be4d33d5f 0:3f000000:f5c5198ef07e7278 0:3f4ccccd:b05453da47ae8505 0:3f800000:4ceaf95ed0ea9b89 0:0:3c0a7a26ad651e56 0:3f800000:4ceaf95ed0ea9b89 0:3f800000:4ceaf95ed0ea9b89 0:0:3c0a7a26ad651e56 0:1:62bbb461bab7e796 0:0:3c0a7a26ad651e56 0:3f800000:4ceaf95ed0ea9b89 0:0:3c0a7a26ad651e56 7fc00000:7fc00000:1322b0c2aba351b2
v 10 0:1:62bbb461bab7e796 80000000:1:62bbb461bab7e796 1:3e4ccccd:46bb887838e1bb3f 1:3f000000:23714cdb448cf058 1:3f4ccccd:de0087269bbd02e5 1:3f800000:5c89e41c9189bb25 0:1:62bbb461bab7e796 1:3f800000:5c89e41c9189bb25 1:3f800000:5c89e41c9189bb25 0:1:62bbb461bab7e796 1:1:9067e7ae0ec66576 0:1:62bbb461bab7e796 1:3f800000:5c89e41c9189bb25 0:1:62bbb461bab7e796 7fc00000:7fc00000:1322b0c2aba351b2
v 11 0:0:3c0a7a26ad651e56 0:80000000:3c0a7a26ad651e56 0:3e4ccccd:190f552be4d33d5f 0:3f000000:f5c5198ef07e7278 0:3f4ccccd:b05453da47ae8505 0:3f800000:4ceaf95ed0ea9b89 0:0:3c0a7a26ad651e56 0:3f800000:4ceaf95ed0ea9b89 0:3f800000:4ceaf95ed0ea9b89 0:0:3c0a7a26ad651e56 0:1:62bbb461bab7e796 0:0:3c0a7a26ad651e56 0:3f800000:4ceaf95ed0ea9b89 0:0:3c0a7a26ad651e56 7fc00000:7fc00000:1322b0c2aba351b2
v 12 0:3f800000:4ceaf95ed0ea9b89 80000000:3f800000:4ceaf95ed0ea9b89 3e4ccccd:3f800000:6dfe8280632092dc 3f000000:3f800000:7b246e6da61e923 3f4ccccd:3f800000:601ae51920454b36 3f800000:3f800000:134a60491d651e56 0:3f800000:4ceaf95ed0ea9b89 3f800000:3f800000:134a60491d651e56 3f800000:3f800000:134a60491d651e56 0:3f800000:4ceaf95ed0ea9b89 1:3f800000:5c89e41c9189bb25 0:3f800000:4ceaf95ed0ea9b89 3f800000:3f800000:134a60491d651e56 0:3f800000:4ceaf95ed0ea9b89 7fc00000:7fc00000:1322b0c2aba351b2
v 13 0:0:3c0a7a26ad651e56 0:80000000:3c0a7a26ad651e56 0:3e4ccccd:190f552be4d33d5f 0:3f000000:f5c5198ef07e7278 0:3f4ccccd:b05453da47ae8505 0:3f800000:4ceaf95ed0ea9b89 0:0:3c0a7a26ad651e56 0:3f800000:4ceaf95ed0ea9b89 0:3f800000:4ceaf95ed0ea9b89 0:0:3c0a7a26ad651e56 0:1:62bbb461bab7e796 0:0:3c0a7a26ad651e56 0:3f800000:4ceaf95ed0ea9b89 0:0:3c0a7a26ad651e56 7fc00000:7fc00000:1322b0c2aba351b2
v 14 7fc00000:7fc00000:1322b0c2aba351b2 7fc00000:7fc00000:1322b0c2aba351b2 7fc00000:7fc00000:1322b0c2aba351b2 7fc00000:7fc00000:1322b0c2aba351b2 7fc00000:7fc00000:1322b0c2aba351b2 7fc00000:7fc00000:1322b0c2aba351b2 7fc00000:7fc00000:1322b0c2aba351b2 7fc00000:7fc00000:1322b0c2aba351b2 7fc00000:7fc00000:1322b0c2aba351b2 7fc00000:7fc00000:1322b0c2aba351b2 7fc00000:7fc00000:1322b0c2aba351b2 7fc00000:7fc00000:1322b0c2aba351b2 7fc00000:7fc00000:1322b0c2aba351b2 7fc00000:7fc00000:1322b0c2aba351b2 7fc00000:7fc00000:1322b0c2aba351b2
e SIMPLEX 55649b87ccb12fdf fe661d342297ea18 5c2e9112ac0f77b0 1350cad08192e8fd b0518f8e4dbc0c4f 65c963e9b8e7b27 a8555fd4dbc0c4f d0c5fadaaad9371b 21c3def6d7b14b86 26a9e5811d7540c7 151341d070dc1366 38ca48e0a56a9bf5 b0518f8e4dbc0c4f 4d971fbe69f43174 a8555fd4dbc0c4f 28d165df9426f631 c29985eb99562ae1 a2edcb8ca832df88 6855b0fb8655b3e9 e1a27f3966cedcf2 b0518f8e4dbc0c4f 9a17b13762d40baf a8555fd4dbc0c4f 932c8b6c2e73892 781716a7624dcc6c 7ade538b115cc6c9 2e3e984e3cacb0a1 35e0bf104e0a1777 b0518f8e4dbc0c4f 9159f4e308fbc82f a8555fd4dbc0c4f 8318d409310dd7db
e SIMPLEX2 92f0486baf3d621a 1bf5e148a85fab13 ebbd086d0c62ed3c fe06b74431144600 b0518f8e4dbc0c4f 64d71c2e63903507 a8555fd4dbc0c4f 23047e40621dcc1a 97cc657b5fd2f8d fc8a3e653dc84ea4 cb033f6d51d0db37 deb6f8f0fb7ccd52 b0518f8e4dbc0c4f ab4f117336df2ce5 a8555fd4dbc0c4f 25832cf8842291fd df9117d8fc5ad1ae 261d0122f2b982b 15baecc5e5cc477b cc057d2604e6e82e b0518f8e4dbc0c4f 21e69a33c8b72d79 a8555fd4dbc0c4f b98d0887c79af4cc aaf91200c46d4cab cac409368a1837d6 899abd049a70e6d7 64141deb6522f61f b0518f8e4dbc0c4f 99b539accb071832 a8555fd4dbc0c4f 3dc41d613e483841
e SIMPLEX_RIDGE 92f0486baf3d621a 1bf5e148a85fab13 ebbd086d0c62ed3c fe06b74431144600 b0518f8e4dbc0c4f 64d71c2e63903507 a8555fd4dbc0c4f 23047e40621dcc1a 97cc657b5fd2f8d fc8a3e653dc84ea4 cb033f6d51d0db37 deb6f8f0fb7ccd52 b0518f8e4dbc0c4f ab4f117336df2ce5 a8555fd4dbc0c4f 25832cf8842291fd df9117d8fc5ad1ae 261d0122f2b982b 15baecc5e5cc477b cc057d2604e6e82e b0518f8e4dbc0c4f 21e69a33c8b72d79 a8555fd4dbc0c4f b98d0887c79af4cc aaf91200c46d4cab cac409368a1837d6 899abd049a70e6d7 64141deb6522f61f b0518f8e4dbc0c4f 99b539accb071832 a8555fd4dbc0c4f 3dc41d613e483841
e PERLIN b7f2446c18481dae b84e7a2b6fb73c7d ecfd7a1efad0c84f a8555fd4dbc0c4f a8555fd4dbc0c4f 33609da20cfc0c4f a8555fd4dbc0c4f 532fa7dd70d52b3f faa23c51285bb967 c849ed4e0a930b97 497623d9742bd3bc a8555fd4dbc0c4f a8555fd4dbc0c4f 7cd6a932213c0c4f a8555fd4dbc0c4f 62be1b09e1b856ef 1980a6b834a09eda b58a54ba19e2600c 7ce030b54399977d a8555fd4dbc0c4f a8555fd4dbc0c4f 1023a03ac1fc0c4f a8555fd4dbc0c4f 9458668aaee46d25 faa23c51285bb967 c849ed4e0a930b97 497623d9742bd3bc a8555fd4dbc0c4f a8555fd4dbc0c4f 7cd6a932213c0c4f a8555fd4dbc0c4f 62be1b09e1b856ef
e PERLIN2 82258e0dde580c7f 815f29cf3e08510b 346b1169d1603a a8555fd4dbc0c4f a8555fd4dbc0c4f 92bf751b20bc0c4f a8555fd4dbc0c4f a066b75436100e9 f42cb392ec94312b 5d1cd674aa0495be f3d684cb7df99a25 a8555fd4dbc0c4f a8555fd4dbc0c4f 76ad6c8fc7fc0c4f a8555fd4dbc0c4f ef76bbc826feba4f 2b9bf3b231d0ee9f 419d80f897c48359 11d8e3aac622eddc a8555fd4dbc0c4f a8555fd4dbc0c4f 2811a5af147c0c4f a8555fd4dbc0c4f 2f091cf03ad30bc7 32ad7bfb71eac173 cee9db99f2a1ac76 8b570112b7d16cfb a8555fd4dbc0c4f a8555fd4dbc0c4f f951fb40a7fc0c4f a8555fd4dbc0c4f 4d0ccdf1d490a861
e RIDGE e8121f1fc52743c6 aaa5239a05d98a2b a65b167ac878a285 a8555fd4dbc0c4f a8555fd4dbc0c4f 8c135cf3326649eb a8555fd4dbc0c4f 36bfb7c81f0db448 5385267355591ae8 249c21607bc07030 d8c96994dc64dc4f a8555fd4dbc0c4f a8555fd4dbc0c4f 7f2e0da55ee02698 a8555fd4dbc0c4f 6057d597657e7d9d fba7d3d9129a2073 535cd356f94b5b5a 94a85b3b9e98751f a8555fd4dbc0c4f a8555fd4dbc0c4f 58cbe454f6c013ae a8555fd4dbc0c4f c761da2aa463afb2 5385267355591ae8 249c21607bc07030 d8c96994dc64dc4f a8555fd4dbc0c4f a8555fd4dbc0c4f 7f2e0da55ee02698 a8555fd4dbc0c4f 6057d597657e7d9d
e CUBIC acb77117056b46ed 459f7e336ccb6bc5 9f1400aae9db1433 d84379bbcdcb07a1 e3e2ae29fe6c0c4f 7c1d728d8c975406 ea08776ed898cbbe 4e4d891df7a221ed 5acc26681f8e6d1f 359c8e57359352a6 79ae991d26cfe779 67c556caf1ce15bd e3e2ae29fe6c0c4f 6985ee56fc28c219 8ed5e0b6be64c76d be5d23c0ecc5a657 d3db56a1a5c9a20f 40a492fd8ac5ff75 92e8c33bc4419fbd 77a03ff93194bb7d e3e2ae29fe6c0c4f 8ef168908d974296 9f36ac47b9124093 dfd501599d9f75fd 1536edcd5d40ed4d 84ecb84b1a142f4f f13604bf1151da8e d28650050a4f9908 e3e2ae29fe6c0c4f 96a6ab26f58d48bb 35fb37fec91cd6ed fa2c6bc21ba1454d
x 0 bdd59d54 3e314904 bea34c20 be89fdf4
";

    const V: [f32; 41] = [
        0.0,
        -0.0,
        1e-7,
        -1e-7,
        0.1,
        0.2,
        0.25,
        0.3,
        0.5,
        0.7,
        0.75,
        0.8,
        0.9,
        1.0,
        1.1,
        1.5,
        -1.0,
        -0.5,
        2.0,
        -2.0,
        3.3,
        10.0,
        -10.0,
        25.0,
        -25.0,
        100.7,
        -1000.3,
        12345.6,
        1e6,
        -1e6,
        3e7,
        -3e7,
        1e30,
        -1e30,
        f32::MAX,
        -f32::MAX,
        f32::from_bits(1),
        -f32::from_bits(1),
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ];
    const POWERS: [i32; 11] = [0, 1, 2, 3, 4, 5, 7, 8, -1, -3, i32::MIN];

    /// Java's `floatToIntBits`: NaNs canonical.
    fn fold(f: i64, value: f32) -> i64 {
        let bits = if value.is_nan() {
            0x7fc0_0000
        } else {
            value.to_bits()
        };
        f.wrapping_mul(31).wrapping_add(i64::from(bits as i32))
    }

    fn hash(module: &Module) -> i64 {
        let mut f = fold(fold(17, module.min_value()), module.max_value());
        for x in V {
            for y in V {
                f = fold(f, module.value(x, y));
            }
        }
        f
    }

    fn all(module: &Module) -> String {
        format!("{:x}", hash(module) as u64)
    }

    /// The oracle's identity lambda, which reports the `Noise` defaults 0..1.
    fn id() -> Module {
        Module::custom(0.0, 1.0, |x, _| x)
    }

    fn row(tag: String, hashes: impl IntoIterator<Item = String>) -> String {
        hashes
            .into_iter()
            .fold(tag, |line, hash| line + " " + &hash)
    }

    #[test]
    fn climate_primitives_match_engine_oracle() {
        let mut out = Vec::new();
        let nan = f32::NAN;
        let compressors = [
            (0.2, 0.1),
            (0.0, 0.1),
            (0.5, 0.0),
            (0.0, 0.0),
            (0.1, 0.5),
            (-0.1, 0.2),
            (0.3, 0.3),
            (nan, 0.1),
            (0.1, f32::INFINITY),
        ];
        for (i, (inset, amount)) in compressors.into_iter().enumerate() {
            out.push(format!(
                "c {i} {}",
                all(&Compressor::new(id(), inset, amount).module())
            ));
        }
        let ranges = [
            (0.1, 0.3, 0.6, 0.9),
            (0.0, 0.0, 1.0, 1.0),
            (0.3, 0.1, 0.9, 0.6),
            (-1.0, 2.0, -3.0, 4.0),
            (0.25, 0.25, 0.75, 0.75),
            (nan, 0.5, 0.5, nan),
        ];
        for (i, (a, b, c, d)) in ranges.into_iter().enumerate() {
            let compressor = Compressor::with_ranges(id(), a, b, c, d);
            out.push(format!("r {i} {}", all(&compressor.module())));
        }
        let frequencies = [
            0.125,
            1.0,
            1.0 / 2048.0,
            -0.5,
            0.0,
            1e-4,
            3.7,
            nan,
            f32::INFINITY,
        ];
        for (i, frequency) in frequencies.into_iter().enumerate() {
            let hashes = POWERS.map(|power| all(&Temperature::new(frequency, power).module()));
            out.push(row(format!("t {i}"), hashes));
        }
        out.push(row(
            "m".into(),
            POWERS.map(|p| all(&Moisture::from_module(id(), p).module())),
        ));
        let clamped = || id().clamp(Module::constant(0.0), Module::constant(1.0));
        let hashes = POWERS.map(|p| all(&Moisture::from_module(clamped(), p).module()));
        out.push(row("k".into(), hashes));
        for seed in [0, 1, -1, 1337, 123456789, i32::MAX, i32::MIN] {
            let hashes = [1, 7, 400, 2048, -100, 0].into_iter().flat_map(|scale| {
                [0, 1, 2, 3, 6].map(|power| all(&Moisture::new(seed, scale, power).module()))
            });
            out.push(row(format!("n {seed}"), hashes));
        }
        let moisture = Moisture::new(1337, 400, 3);
        let temperature = Temperature::new(1.0 / 2048.0, 2);
        let points = [(0.0, 0.0), (100.0, -250.0), (-5000.5, 7777.0), (1e6, -1e6)];
        out.push(row(
            "s".into(),
            points.into_iter().flat_map(|(x, y)| {
                [moisture.value(x, y), temperature.value(x, y)]
                    .map(|v| format!("{:x}", v.to_bits()))
            }),
        ));
        let expected: Vec<&str> = ORACLE.lines().skip(1).collect();
        assert_eq!(out.len(), expected.len());
        for (line, expected) in out.iter().zip(expected) {
            assert_eq!(line, expected);
        }
    }

    /// `Integer.toHexString(Float.floatToIntBits(value))` (NaNs canonical), as the oracle prints it.
    fn hex(value: f32) -> String {
        let bits = if value.is_nan() {
            0x7fc0_0000
        } else {
            value.to_bits()
        };
        format!("{bits:x}")
    }

    fn range_line(tag: &str, range: ClimateRange) -> String {
        format!(
            "d {tag} {} {} {} {} {} {}",
            range.seed_offset,
            range.scale,
            range.falloff,
            hex(range.min),
            hex(range.max),
            hex(range.bias)
        )
    }

    #[test]
    fn climate_settings_match_engine_oracle() {
        let mut out = Vec::new();
        let settings = ClimateSettings::default();
        out.push(range_line("t", settings.temperature));
        out.push(range_line("m", settings.moisture));
        out.push(range_line("r", ClimateRange::default()));
        let shape = settings.biome_shape;
        out.push(format!(
            "d s {} {} {} {} {}",
            BiomeShape::DEFAULT_BIOME_SIZE,
            shape.biome_size,
            shape.macro_noise_size,
            shape.biome_warp_scale,
            shape.biome_warp_strength
        ));
        let edge = settings.biome_edge_shape;
        assert!(matches!(edge.kind, SourceKind::Simplex));
        out.push(format!(
            "d e SIMPLEX {} {} {} {} {}",
            edge.scale,
            edge.octaves,
            hex(edge.gain),
            hex(edge.lacunarity),
            edge.strength
        ));
        let nan = f32::NAN;
        let tiny = f32::from_bits(1);
        let edges = [
            0.0,
            -0.0,
            0.2,
            0.5,
            0.8,
            1.0,
            -0.5,
            1.5,
            2.0,
            -2.0,
            tiny,
            -tiny,
            f32::INFINITY,
            f32::NEG_INFINITY,
            nan,
        ];
        for (i, min) in edges.into_iter().enumerate() {
            let cells = edges.map(|max| {
                let mut range = ClimateRange::new(7, 1, min, max, 0.0);
                let mut f = 17_i64;
                for bias in edges {
                    range.bias = bias;
                    f = fold(f, range.bias());
                    f = f.wrapping_mul(31).wrapping_add(hash(&range.apply(id())));
                }
                format!("{}:{}:{:x}", hex(range.min()), hex(range.max()), f as u64)
            });
            out.push(row(format!("v {i}"), cells));
        }
        let kinds = [
            ("SIMPLEX", SourceKind::Simplex),
            ("SIMPLEX2", SourceKind::Simplex2),
            ("SIMPLEX_RIDGE", SourceKind::SimplexRidge),
            ("PERLIN", SourceKind::Perlin),
            ("PERLIN2", SourceKind::Perlin2),
            ("RIDGE", SourceKind::Ridge),
            ("CUBIC", SourceKind::Cubic),
        ];
        // scale, octaves, gain, lacunarity
        let configs = [
            (24, 2, 0.5, 2.65),
            (1, 1, 0.5, 2.0),
            (500, 5, 5.5, 10.5),
            (7, 3, 0.0, 0.0),
            (0, 2, 0.5, 2.65),
            (-3, 4, -0.7, -1.5),
            (24, 0, 0.5, 2.65),
            (13, 8, 0.9, 1.3),
        ];
        for (name, kind) in kinds {
            let hashes = [42, 0, -1, i32::MIN].into_iter().flat_map(|seed| {
                configs.map(|(scale, octaves, gain, lacunarity)| {
                    let noise = BiomeNoise {
                        kind,
                        scale,
                        octaves,
                        gain,
                        lacunarity,
                        ..BiomeNoise::default()
                    };
                    all(&noise.build(seed))
                })
            });
            out.push(row(format!("e {name}"), hashes));
        }
        let edge = BiomeNoise::default().build(42);
        let points = [
            (0.0, 0.0),
            (1.0, 2.0),
            (24.0, 48.0),
            (-10.0, 7.0),
            (1000.0, -300.0),
        ];
        out.push(row("x".into(), points.map(|(x, y)| hex(edge.value(x, y)))));
        let expected: Vec<&str> = SETTINGS_ORACLE.lines().skip(1).collect();
        assert_eq!(out.len(), expected.len());
        for (line, expected) in out.iter().zip(expected) {
            assert_eq!(line, expected);
        }
    }

    /// Engine throws for a negative octave count; the port builds no octaves (the zero-octave noise).
    #[test]
    fn negative_biome_noise_octaves_build_no_octaves() {
        let noise = |octaves| BiomeNoise {
            octaves,
            ..BiomeNoise::default()
        };
        let (none, negative) = (noise(0).build(42), noise(-1).build(42));
        for (x, y) in [(0.0, 0.0), (5.0, -3.0)] {
            assert_eq!(hex(negative.value(x, y)), hex(none.value(x, y)));
        }
    }

    /// Output of `.local/terraforged/oracles/climate_module/ClimateModuleOracle.java` (run.sh) against Engine 0.3.0.
    const MODULE_ORACLE: &str = r"
s 0 0 4
e 0 1bf2a510bf7ff19d
g 0 9c3cf6b13db9e89d
f 0 aa49e4e0c9950f67
h 0 84dfa57615552864
x 0 4
p 0 0 0 0 3e185a4c 3f19d275 3f6687ed 3f061eb9 3e7aac1a 4 3 c2b20000 41600000
p 0 0 0 1 3e185a4c 3e000000 3f6687ed 3f061eb9 3e7aac1a 4 8 c2b20000 41600000
p 0 41440000 c2c58000 0 3de1c6f0 3f532640 3f0b9a40 3ef67559 3ea6991c 4 3 42600000 c3450000
p 0 41440000 c2c58000 1 3de1c6f0 3e000000 3f0b9a40 3ef67559 3ea6991c 4 8 42600000 c3450000
p 0 c5800400 46000100 0 3e2ebd08 3f7b8753 3f800000 3d33bf66 3f27bc80 9 3 c57df000 46001c00
p 0 c5800400 46000100 1 3e2ebd08 3e000000 3f800000 3d33bf66 3f27bc80 9 8 c57df000 46001c00
p 0 47c35000 c7c35000 0 3ea7429e 3ea8ce98 3eecd1dc 3f7ae148 3df97230 1 3 47c38800 c7c35c80
p 0 47c35000 c7c35000 1 3ea7429e 3e000000 3eecd1dc 3f7ae148 3df97230 1 8 47c38800 c7c35c80
s 1 1234 1238
e 1 90278581372f6191
g 1 58b34ee671c284c7
f 1 f340d1ffafcee7e8
h 1 5d9c5413c3562df
x 1 6
p 1 0 0 0 3f1ca982 3e271000 3f4925a7 3f0694a9 3f7847c6 4 3 c31f0000 c1b80000
p 1 0 0 1 3f1ca982 3e000000 3f4925a7 3f0694a9 3f7847c6 4 8 c31f0000 c1b80000
p 1 41440000 c2c58000 0 3f1ca982 3da128c0 3f4925a7 3f0694a9 3f7847c6 4 3 c31f0000 c1b80000
p 1 41440000 c2c58000 1 3f1ca982 3e000000 3f4925a7 3f0694a9 3f7847c6 4 8 c31f0000 c1b80000
p 1 c5800400 46000100 0 3effacc8 3e269bd8 3f42180a 3f065de6 3eb78122 4 3 c582c000 45fcb000
p 1 c5800400 46000100 1 3effacc8 3e000000 3f42180a 3f065de6 3eb78122 4 8 c582c000 45fcb000
p 1 47c35000 c7c35000 0 3e2fd59e 3f4be9bb 3f800000 3f69dff4 3f25bb1a 0 3 47c32400 c7c36000
p 1 47c35000 c7c35000 1 3e2fd59e 3e000000 3f800000 3f69dff4 3f25bb1a 0 8 47c32400 c7c36000
s 2 -1 3
e 2 8678ccb34da6cedd
g 2 4f00721ddb0bcb17
f 2 29adc4f1734f2098
h 2 8000b5e7d70f563d
x 2 0
s 3 -1698898192 -1698898188
e 3 ffdec253ff37de12
g 3 5a18f826b470a76d
f 3 762d6826e90f29e4
h 3 8e2a95f9d0991440
x 3 6
s 4 2147483647 -2147483645
e 4 e5af313820230565
g 4 38315aab8b1de7f8
f 4 384ce40411de157e
h 4 254e324e1e97ab89
x 4 2
s 5 -2147483648 -2147483644
e 5 60a1b9aa3988bb34
g 5 420fd5b860e6f19e
f 5 392281c1a91ec32b
h 5 3493d5427e7c4027
x 5 0
";

    /// The oracle's edge-case coordinates.
    const C: [f32; 25] = [
        0.0,
        -0.0,
        0.5,
        -0.5,
        1.0,
        -1.0,
        12.25,
        -98.75,
        224.9,
        -225.1,
        4096.5,
        -4096.5,
        1e5,
        -1e5,
        3e7,
        -3e7,
        1e9,
        -1e9,
        1e10,
        3e38,
        -3e38,
        f32::from_bits(1),
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ];
    /// The oracle continent's land values, around 0.75 and the default coast marker 0.475, and 9 (a
    /// negative moisture, whose biome lookup throws).
    const LAND: [f32; 16] = [
        0.0,
        9.0,
        0.5,
        0.6,
        0.74,
        0.75,
        0.76,
        0.9,
        1.0,
        1.2,
        -0.1,
        f32::NAN,
        0.47,
        0.475,
        0.4751,
        -0.0,
    ];

    /// The oracle's settings `i`: `(world seed, climate settings, control points)`.
    fn module_settings(i: usize) -> (i64, ClimateSettings, WorldControlPoints) {
        let mut climate = ClimateSettings::default();
        let mut points = WorldControlPoints::default();
        let seed = match i {
            0 => 0,
            1 => 1234,
            2 => {
                climate.biome_shape = BiomeShape {
                    biome_size: 1,
                    macro_noise_size: 1,
                    biome_warp_scale: 1,
                    biome_warp_strength: 0,
                };
                climate.temperature = ClimateRange::new(1, 0, 0.0, 1.0, 0.0);
                climate.temperature.seed_offset = 7;
                climate.moisture = ClimateRange::new(1, 3, 0.0, 1.0, 0.0);
                climate.moisture.seed_offset = -3;
                -1
            }
            3 => {
                climate.biome_shape = BiomeShape {
                    biome_size: 600,
                    macro_noise_size: 20,
                    biome_warp_scale: 400,
                    biome_warp_strength: 300,
                };
                climate.temperature = ClimateRange::new(13, 4, 0.2, 0.8, 0.3);
                climate.temperature.seed_offset = i32::MAX;
                climate.moisture = ClimateRange::new(5, 2, 0.1, 0.9, -0.2);
                climate.moisture.seed_offset = i32::MIN;
                points.inland = 2.0;
                0x1234_5678_9abc_def0
            }
            4 => {
                climate.biome_shape.biome_size = 7;
                climate.moisture = ClimateRange::new(0, 1, 0.0, 1.0, 0.0);
                climate.temperature = ClimateRange::new(3, -2, 0.0, 1.0, 0.1);
                points.coast = 0.3;
                points.inland = 0.6;
                i64::from(i32::MAX)
            }
            _ => {
                climate.biome_shape.biome_size = 3;
                climate.biome_shape.biome_warp_strength = -80;
                climate.temperature = ClimateRange::new(7, 3, 0.98, 0.02, 0.0);
                climate.moisture = ClimateRange::new(11, 2, 0.0, 1.0, 0.4);
                i64::from(i32::MIN)
            }
        };
        (seed, climate, points)
    }

    /// The oracle's continent: a land value picked by a hash of its arguments, which it records.
    fn module_continent(last: &Arc<std::sync::Mutex<(f32, f32)>>) -> Arc<dyn LandValue> {
        let last = Arc::clone(last);
        Arc::new(move |x: f32, z: f32| {
            *last.lock().unwrap() = (x, z);
            let index = (x.to_bits() as i32)
                .wrapping_mul(31)
                .wrapping_add(z.to_bits() as i32)
                & 15;
            LAND[index as usize]
        })
    }

    /// Engine's climate stage samples one `ClimateModule` from every tile-generation thread.
    #[test]
    fn climate_module_is_shared_between_threads() {
        fn shareable<T: Send + Sync>() {}
        shareable::<ClimateModule>();
    }

    #[test]
    fn climate_module_matches_engine_oracle() {
        let registry = TerrainRegistry::new();
        let lines: Vec<Vec<&str>> = MODULE_ORACLE
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| line.split(' ').collect())
            .collect();
        let expect = |kind: &str, i: usize| -> String {
            let line = lines
                .iter()
                .find(|line| line[0] == kind && line[1] == i.to_string())
                .unwrap_or_else(|| panic!("no {kind} {i} line"));
            line[2..].join(" ")
        };
        let last = Arc::new(std::sync::Mutex::new((0.0, 0.0)));
        for i in 0..6 {
            let (world_seed, climate, points) = module_settings(i);
            let mut seed = Seed::from_long(world_seed);
            let module = ClimateModule::new(
                module_continent(&last),
                &mut seed,
                &climate,
                points,
                &registry,
            );
            assert_eq!(
                format!("{} {}", seed.root(), seed.get()),
                expect("s", i),
                "seed draws {i}"
            );
            let mut sample = 0;
            let mut dead = 0;
            let mut run = |f: i64, x: f32, z: f32, mask: bool| -> i64 {
                let mut cell = Cell::new(&registry);
                cell.biome_region_edge = 0.125;
                cell.terrain = registry.get_by_id(sample & 15);
                sample += 1;
                *last.lock().unwrap() = (7.0, 7.0);
                let applied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    module.apply_masked(&mut cell, x, z, mask);
                }));
                if applied.is_err() {
                    dead += 1;
                    return f.wrapping_mul(31).wrapping_add(0xdead);
                }
                let (last_x, last_z) = *last.lock().unwrap();
                let mut f = f;
                for value in [
                    cell.biome_region_id,
                    cell.biome_region_edge,
                    cell.moisture,
                    cell.temperature,
                    cell.macro_biome_id,
                ] {
                    f = fold(f, value);
                }
                f = f.wrapping_mul(31).wrapping_add(i64::from(cell.biome.id()));
                // `cell.terrain` by identity: one of the world registry's own terrains.
                let id = cell.terrain.id();
                assert!(cell.terrain.is_same(&registry.get_by_id(id)));
                f = f.wrapping_mul(31).wrapping_add(i64::from(id));
                fold(fold(f, last_x), last_z)
            };
            for (kind, grid, mask) in [
                ("e", false, true),
                ("g", true, true),
                ("f", false, false),
                ("h", true, false),
            ] {
                let mut f = 17;
                if grid {
                    for gz in (-2000..=2000).step_by(211) {
                        for gx in (-2000..=2000).step_by(211) {
                            f = run(f, gx as f32 + 0.3, gz as f32 - 0.7, mask);
                        }
                    }
                } else {
                    for x in C {
                        for z in C {
                            f = run(f, x, z, mask);
                        }
                    }
                }
                assert_eq!(format!("{f:x}"), expect(kind, i), "{kind} {i}");
            }
            assert_eq!(dead.to_string(), expect("x", i), "throws {i}");
            for line in lines
                .iter()
                .filter(|line| line[0] == "p" && line[1] == i.to_string())
            {
                let x = f32::from_bits(u32::from_str_radix(line[2], 16).unwrap());
                let z = f32::from_bits(u32::from_str_radix(line[3], 16).unwrap());
                let mut cell = Cell::new(&registry);
                cell.biome_region_edge = 0.125;
                cell.terrain = registry.get_by_id(8);
                module.apply_masked(&mut cell, x, z, line[4] == "0");
                let (last_x, last_z) = *last.lock().unwrap();
                let got: Vec<String> = [
                    cell.biome_region_id,
                    cell.biome_region_edge,
                    cell.moisture,
                    cell.temperature,
                    cell.macro_biome_id,
                ]
                .iter()
                .map(|value| format!("{:x}", value.to_bits()))
                .chain([cell.biome.id().to_string(), cell.terrain.id().to_string()])
                .chain(
                    [last_x, last_z]
                        .iter()
                        .map(|value| format!("{:x}", value.to_bits())),
                )
                .collect();
                assert_eq!(got.join(" "), line[5..].join(" "), "{}", line.join(" "));
                assert!(cell.terrain.is_same(&registry.get_by_id(cell.terrain.id())));
            }
        }
        // `apply(cell, x, y)` is the masked form.
        let (_, climate, points) = module_settings(1);
        let module = ClimateModule::new(
            module_continent(&last),
            &mut Seed::new(1234),
            &climate,
            points,
            &registry,
        );
        let mut cell = Cell::new(&registry);
        cell.biome_region_edge = 0.125;
        cell.terrain = registry.get_by_id(8);
        module.apply(&mut cell, 12.25, -98.75);
        assert_eq!(cell.biome_region_edge.to_bits(), 0x3da1_28c0);
        assert!(cell.terrain.is_same(&registry.get_by_id(3)));
        // Another registry's coast is not `TerrainType.COAST`.
        assert!(!cell.terrain.is_same(&TerrainRegistry::new().get_by_id(3)));
    }
}
