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
        SourceKind, cell_vector, floor, noise_map, sin_lookup, value_coord_2d,
    },
    terraforged_engine::{
        BiomeType, Cell, ControlPoints, Seed, Terrain, TerrainProperties, TerrainRegistry,
        WorldControlPoints,
    },
};

#[derive(Clone, Copy)]
pub struct ClimateRange {
    pub seed_offset: i32,
    pub scale: i32,
    pub falloff: i32,
    pub min: f32,
    pub max: f32,
    pub bias: f32,
}

impl ClimateRange {
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

    pub fn min(self) -> f32 {
        self.min.min(self.max).clamp(0.0, 1.0)
    }

    pub fn max(self) -> f32 {
        self.min.max(self.max).clamp(self.min(), 1.0)
    }

    pub fn bias(self) -> f32 {
        self.bias.clamp(-1.0, 1.0)
    }

    pub fn apply(self, module: Module) -> Module {
        module
            .bias(Module::constant(self.bias() / 2.0))
            .clamp(Module::constant(self.min()), Module::constant(self.max()))
    }
}

impl Default for ClimateRange {
    fn default() -> Self {
        Self::new(7, 1, 0.0, 1.0, 0.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BiomeShape {
    pub biome_size: i32,
    pub macro_noise_size: i32,
    pub biome_warp_scale: i32,
    pub biome_warp_strength: i32,
}

impl BiomeShape {
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
    pub fn build(self, seed: i32) -> Module {
        Module::source(Source::new(
            self.kind,
            SourceConfig {
                seed,
                octaves: self.octaves as usize,
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

pub struct ClimateModule {
    seed: i32,
    edge_clamp: f32,
    edge_scale: f32,
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
    pub fn new(
        continent: Arc<dyn LandValue>,
        seed: &mut Seed,
        settings: ClimateSettings,
        control_points: WorldControlPoints,
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
        )
        .module();
        let moisture = settings
            .moisture
            .apply(moisture)
            .warp(noise_warp(
                moisture_seed.next(),
                (moisture_scale / 2).max(1),
                1,
                moisture_scale as f32 / 4.0,
            ))
            .warp(noise_warp(
                moisture_seed.next(),
                (moisture_scale / 6).max(1),
                2,
                moisture_scale as f32 / 12.0,
            ));

        let mut temperature_seed = seed.offset(settings.temperature.seed_offset);
        let temperature =
            Temperature::new(1.0 / temperature_scale as f32, settings.temperature.falloff).module();
        let temperature = settings
            .temperature
            .apply(temperature)
            .warp(noise_warp(
                temperature_seed.next(),
                temperature_scale.wrapping_mul(4),
                2,
                temperature_scale.wrapping_mul(4) as f32,
            ))
            .warp(noise_warp(
                temperature_seed.next(),
                temperature_scale,
                1,
                temperature_scale as f32,
            ));

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
            edge_clamp: 1.0,
            edge_scale: 1.0,
            biome_frequency,
            warp_strength: settings.biome_shape.biome_warp_strength as f32,
            warp_x,
            warp_z,
            moisture,
            temperature,
            macro_biome_noise,
            continent,
            control_points: ControlPoints::new(control_points),
            coast: TerrainRegistry::new().get_by_id(3),
        }
    }

    pub fn apply(&self, cell: &mut Cell, x: f32, z: f32) {
        self.apply_masked(cell, x, z, true);
    }

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

        cell.biome_region_id = noise_map(value_coord_2d(self.seed, cell_x, cell_z), -1.0, 1.0, 2.0);
        cell.moisture = self.moisture.value(center_x, center_z);
        cell.temperature = self.temperature.value(center_x, center_z);
        cell.macro_biome_id = self.macro_biome_noise.value(center_x, center_z);
        let position_x = floor(center_x / self.biome_frequency);
        let position_z = floor(center_z / self.biome_frequency);
        let continent_edge = self
            .continent
            .land_value(position_x as f32, position_z as f32);
        if mask {
            cell.biome_region_edge = self.edge_value(edge_distance, edge_distance2);
            self.modify_terrain(cell, continent_edge);
        }
        self.modify_moisture(cell, continent_edge);
        cell.biome = BiomeType::at_climate(cell.temperature, cell.moisture);
    }

    fn modify_moisture(&self, cell: &mut Cell, continent_edge: f32) {
        let limit = 0.75;
        let range = 1.0 - limit;
        if continent_edge < limit {
            let alpha = (limit - continent_edge) / range;
            let multiplier = 1.0 + alpha * range;
            cell.moisture = (cell.moisture * multiplier).clamp(0.0, 1.0);
        } else {
            let alpha = (continent_edge - limit) / range;
            let multiplier = 1.0 - alpha * range;
            cell.moisture *= multiplier;
        }
    }

    fn modify_terrain(&self, cell: &mut Cell, continent_edge: f32) {
        if cell.terrain.is_overground()
            && !cell.terrain.overrides_coast()
            && continent_edge <= self.control_points.coast_marker
        {
            cell.terrain = self.coast.clone();
        }
    }

    fn edge_value(&self, distance: f32, distance2: f32) -> f32 {
        let value = EdgeFunction::Distance2Div.apply(distance, distance2);
        1.0 - noise_map(value, -1.0, 0.0, 1.0) * self.edge_scale / self.edge_clamp
    }
}

fn simplex(seed: i32, scale: i32, octaves: usize) -> Module {
    Module::source(Source::new(
        SourceKind::Simplex,
        SourceConfig {
            seed,
            octaves,
            frequency: 1.0 / scale as f32,
            ..SourceConfig::default()
        },
    ))
}

fn noise_warp(seed: i32, scale: i32, octaves: usize, strength: f32) -> Domain {
    Domain::domain_warp(
        source(SourceKind::Perlin, seed, scale, octaves),
        source(SourceKind::Perlin, seed.wrapping_add(1), scale, octaves),
        Module::constant(strength),
    )
}

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

fn round(value: f32) -> i32 {
    if value >= 0.0 {
        (value + 0.5) as i32
    } else {
        (value - 0.5) as i32
    }
}

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
    pub fn new(module: Module, inset: f32, amount: f32) -> Self {
        Self::with_ranges(
            module,
            inset,
            inset + amount,
            1.0 - inset - amount,
            1.0 - inset,
        )
    }

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

    pub fn module(self) -> Module {
        Module::custom(0.0, 1.0, move |x, y| self.value(x, y))
    }
}

#[derive(Clone, Copy)]
pub struct Temperature {
    power: i32,
    frequency: f32,
}

impl Temperature {
    pub const fn new(frequency: f32, power: i32) -> Self {
        Self { power, frequency }
    }

    pub fn value(self, x: f32, mut y: f32) -> f32 {
        let _ = x;
        y *= self.frequency;
        let sin = sin_lookup(y).clamp(-1.0, 1.0);
        let value = java_pow(sin, self.power as f32).copysign(sin);
        map_unit(value)
    }

    pub fn module(self) -> Module {
        Module::custom(0.0, 1.0, move |x, y| self.value(x, y))
    }
}

#[derive(Clone)]
pub struct Moisture {
    source: Module,
    power: i32,
}

impl Moisture {
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
        let value = java_pow(noise, self.power as f32).copysign(noise);
        map_unit(value)
    }

    pub fn module(self) -> Module {
        Module::custom(0.0, 1.0, move |x, y| self.value(x, y))
    }
}

fn java_pow(value: f32, power: f32) -> f32 {
    f64::from(value).powf(f64::from(power)) as f32
}

fn map_unit(value: f32) -> f32 {
    (value + 1.0) / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(values: impl IntoIterator<Item = f32>) -> Vec<u32> {
        values.into_iter().map(f32::to_bits).collect()
    }

    #[test]
    fn climate_primitives_match_engine_0_3_0() {
        let x = Module::custom(f32::NEG_INFINITY, f32::INFINITY, |x, _| x);
        let compressor = Compressor::new(x.clone(), 0.2, 0.1);
        assert_eq!(
            bits(
                [-0.1, 0.0, 0.1, 0.2, 0.3, 0.5, 0.7, 0.8, 0.9, 1.0, 1.1]
                    .map(|value| compressor.value(value, 0.0))
            ),
            [
                0xbe19999a, 0x00000000, 0x3e19999a, 0x3e99999a, 0x3ebbbbbc, 0x3f000000, 0x3f222222,
                0x3f333333, 0x3f599999, 0x3f800000, 0x3f933334,
            ]
        );

        let temperature = Temperature::new(0.125, 3);
        assert_eq!(
            bits([-25.0, -10.0, -2.0, 0.0, 2.0, 10.0, 25.0].map(|y| { temperature.value(0.0, y) })),
            [
                0x3effffa4, 0x3d96b3dc, 0x3efc39b9, 0x3f000000, 0x3f01ebfb, 0x3f6d547b, 0x3f000023,
            ]
        );

        let moisture = Moisture::from_module(x, 3);
        assert_eq!(
            bits([0.0, 0.25, 0.5, 1.0, 1.5, 2.0].map(|x| moisture.value(x, 0.0))),
            [
                0x00000000, 0x3e940000, 0x3ee00000, 0x3f000000, 0x3f100000, 0x3f800000,
            ]
        );
    }

    #[test]
    fn climate_settings_match_engine_0_3_0() {
        let settings = ClimateSettings::default();
        assert_eq!(
            (
                settings.temperature.seed_offset,
                settings.temperature.scale,
                settings.temperature.falloff,
                settings.temperature.min.to_bits(),
                settings.temperature.max.to_bits(),
                settings.temperature.bias.to_bits(),
            ),
            (0, 6, 2, 0x00000000, 0x3f7ae148, 0x3d4ccccd)
        );
        assert_eq!(
            (
                settings.moisture.seed_offset,
                settings.moisture.scale,
                settings.moisture.falloff,
                settings.moisture.min.to_bits(),
                settings.moisture.max.to_bits(),
                settings.moisture.bias.to_bits(),
            ),
            (0, 6, 1, 0x00000000, 0x3f800000, 0x00000000)
        );
        assert_eq!(settings.biome_shape, BiomeShape::default());
    }

    #[test]
    fn climate_range_and_edge_noise_match_engine_0_3_0() {
        let range = ClimateRange {
            min: 0.8,
            max: 0.2,
            bias: 2.0,
            ..ClimateRange::default()
        };
        assert_eq!(
            bits([range.min(), range.max(), range.bias()]),
            [0x3e4ccccd, 0x3f4ccccd, 0x3f800000]
        );
        assert_eq!(
            range
                .apply(Module::constant(0.25))
                .value(0.0, 0.0)
                .to_bits(),
            0x3f400000
        );

        let edge = BiomeNoise::default().build(42);
        assert_eq!(
            bits(
                [
                    (0.0, 0.0),
                    (1.0, 2.0),
                    (24.0, 48.0),
                    (-10.0, 7.0),
                    (1000.0, -300.0)
                ]
                .map(|(x, y)| edge.value(x, y))
            ),
            [0x00000000, 0xbdd59d54, 0x3e314904, 0xbea34c20, 0xbe89fdf4]
        );
    }

    #[test]
    fn climate_module_matches_engine_0_3_0() {
        let mut seed = Seed::new(1234);
        let module = ClimateModule::new(
            Arc::new(|_, _| 0.3),
            &mut seed,
            ClimateSettings::default(),
            WorldControlPoints::default(),
        );
        let points = [
            (
                0.0,
                0.0,
                [0x3f1ca982, 0x3e271000, 0x3f26aa2b, 0x3f0694a9, 0x3f7847c6],
                4,
            ),
            (
                12.25,
                -98.75,
                [0x3f1ca982, 0x3da128c0, 0x3f26aa2b, 0x3f0694a9, 0x3f7847c6],
                4,
            ),
            (
                -4096.5,
                8192.25,
                [0x3effacc8, 0x3e269bd8, 0x3f20d217, 0x3f065de6, 0x3eb78122],
                4,
            ),
            (
                100_000.0,
                -100_000.0,
                [0x3e2fd59e, 0x3f4be9bb, 0x3f800000, 0x3f69dff4, 0x3f25bb1a],
                0,
            ),
        ];
        for (x, z, expected, biome) in points {
            let mut cell = Cell::default();
            cell.terrain = TerrainRegistry::new().get_by_id(8);
            module.apply(&mut cell, x, z);
            assert_eq!(
                [
                    cell.biome_region_id.to_bits(),
                    cell.biome_region_edge.to_bits(),
                    cell.moisture.to_bits(),
                    cell.temperature.to_bits(),
                    cell.macro_biome_id.to_bits(),
                ],
                expected
            );
            assert_eq!(cell.biome.id(), biome);
            assert_eq!(cell.terrain.name(), "coast");
        }

        let mut cell = Cell::default();
        cell.biome_region_edge = 0.25;
        cell.terrain = TerrainRegistry::new().get_by_id(8);
        module.apply_masked(&mut cell, 12.25, -98.75, false);
        assert_eq!(
            [
                cell.biome_region_id.to_bits(),
                cell.biome_region_edge.to_bits(),
                cell.moisture.to_bits(),
                cell.temperature.to_bits(),
                cell.macro_biome_id.to_bits(),
            ],
            [0x3f1ca982, 0x3e800000, 0x3f26aa2b, 0x3f0694a9, 0x3f7847c6]
        );
        assert_eq!(cell.biome.id(), 4);
        assert_eq!(cell.terrain.name(), "flats");
    }
}
