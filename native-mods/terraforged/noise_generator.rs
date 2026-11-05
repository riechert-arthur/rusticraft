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

use super::{
    terraforged::{Module, Source, SourceConfig, SourceKind, lerp},
    terraforged_continent_noise::ContinentNoise,
    terraforged_continent_points::{BEACH, COAST, SHALLOW_OCEAN},
    terraforged_engine::{ControlPoints, Terrain},
    terraforged_terrain_blender::{Blender, TerrainBlender},
    terraforged_terrain_data::NoiseSample,
    terraforged_terrain_levels::TerrainLevels,
};

/// `NoiseGenerator.OCEAN_OFFSET`: the ocean floor noise's own seed (the runtime seed is added to it, `int` wrap).
const OCEAN_OFFSET: i32 = 8_763_214;
/// `NoiseGenerator.TERRAIN_OFFSET`: the seed `createLandTerrain` builds the [`TerrainBlender`] with
/// (`new TerrainBlender(TERRAIN_OFFSET, 800, 0.8F, 0.4F, terrainNoises)`).
pub const TERRAIN_OFFSET: i64 = 45_763_218;
/// `NoiseGenerator.CONTINENT_OFFSET`: the world seed of the settings `createContinentNoise` builds the
/// [`ContinentNoise`]'s context from.
pub const CONTINENT_OFFSET: i64 = 18_749_560;
/// `NoiseGenerator.heightMultiplier`.
const HEIGHT_MULTIPLIER: f32 = 1.2;

// TerraForged samples one generator from its worker threads (each with its own blender and sample).
const _: fn() = || {
    fn shareable<T: Send + Sync>() {}
    shareable::<NoiseGenerator>();
};

/// The newer source tree's `NoiseGenerator` (6dd607eb, `worldgen.noise`): a block's terrain sample. It scales the
/// block position by the noise levels' frequency (`getNoiseCoord`), samples the continent
/// ([`ContinentNoise::sample_continent`]), then the height by the continent noise's band (deep ocean below
/// `SHALLOW_OCEAN`, ocean floor up to sea level below `BEACH`, sea level up to the land height below `COAST`, the land
/// height above it, all with the runtime seed), then carves the rivers ([`ContinentNoise::sample_river`]). The land
/// height is the seed-first [`TerrainBlender`] (`* 1.2`) on top of the continent's base noise
/// (`NoiseLevels.toHeightNoise`).
///
/// The constructor takes the continent and the blender as built (`createContinentNoise`/`createLandTerrain` build them
/// from `CONTINENT_OFFSET`/[`TERRAIN_OFFSET`] and the terrain noises); the deep-ocean terrain is the continent
/// context's registry's, so it compares `is_same` with the world's built-ins, and the blender's terrain noises must
/// hold that registry's terrains too. The 1.18.2-0.3.1 jar's `NoiseGenerator` is a different class (a world seed in
/// its constructor, no runtime seed, no deep-ocean branch of its own); this is the newer source's.
pub struct NoiseGenerator {
    levels: TerrainLevels,
    ocean: Module,
    land: TerrainBlender,
    continent: ContinentNoise,
    control_points: ControlPoints,
    deep_ocean: Terrain,
}

impl NoiseGenerator {
    /// `new NoiseGenerator(levels, terrainNoises)` with the continent and blender already built (see
    /// [`NoiseGenerator`]); `createOceanTerrain` is built here.
    pub fn new(levels: TerrainLevels, continent: ContinentNoise, land: TerrainBlender) -> Self {
        let control_points = continent.control_points();
        let deep_ocean = continent.context().terrain.get_by_id(1);
        Self {
            levels,
            ocean: create_ocean_terrain(),
            land,
            continent,
            control_points,
            deep_ocean,
        }
    }

    /// `getTerrainLevels`.
    pub fn levels(&self) -> TerrainLevels {
        self.levels
    }

    /// `getContinent`.
    pub fn continent(&self) -> &ContinentNoise {
        &self.continent
    }

    pub fn control_points(&self) -> ControlPoints {
        self.control_points
    }

    /// `getBlenderResource` (a fresh [`Blender`]; Java's is thread-local, which is the same since every land sample
    /// overwrites its state).
    pub fn blender_resource(&self) -> Blender {
        self.land.blender_resource()
    }

    /// `sample(seed, x, z, sample, blender)`: [`Self::sample_terrain`] then [`Self::sample_river`] at
    /// `x * frequency`, `z * frequency` (int to float, then a float multiply). `Err` when a terrain noise the blender
    /// picks has no seed-first form (a custom module): the sample is then partly written (continent fields set).
    /// Panics where Java throws (the blender's cell search on NaN, infinite or huge noise coordinates).
    pub fn sample<'a>(
        &self,
        seed: i32,
        x: i32,
        z: i32,
        sample: &'a mut NoiseSample,
        blender: &mut Blender,
    ) -> Result<&'a mut NoiseSample, &'static str> {
        let frequency = self.levels.noise_levels.frequency;
        let noise_x = x as f32 * frequency;
        let noise_z = z as f32 * frequency;
        self.sample_terrain(seed, noise_x, noise_z, sample, blender)?;
        Ok(self.sample_river(seed, noise_x, noise_z, sample))
    }

    /// `sampleTerrain(seed, nx, nz, sample, blender)`: the continent, then `getOcean`, `getBlend` or `getInland` by
    /// the continent noise (NaN fails every band test and goes inland, as Java's float compares).
    pub fn sample_terrain<'a>(
        &self,
        seed: i32,
        x: f32,
        z: f32,
        sample: &'a mut NoiseSample,
        blender: &mut Blender,
    ) -> Result<&'a mut NoiseSample, &'static str> {
        self.continent.sample_continent(seed, x, z, sample);
        let continent_noise = sample.continent_noise;
        if continent_noise < SHALLOW_OCEAN {
            self.ocean(seed, x, z, sample)?;
        } else if continent_noise < COAST {
            self.blend(seed, x, z, sample, blender)?;
        } else {
            self.inland(seed, x, z, sample, blender)?;
        }
        Ok(sample)
    }

    /// `sampleRiver(seed, nx, nz, sample)`.
    pub fn sample_river<'a>(
        &self,
        seed: i32,
        x: f32,
        z: f32,
        sample: &'a mut NoiseSample,
    ) -> &'a mut NoiseSample {
        self.continent.sample_river(seed, x, z, sample);
        sample
    }

    /// `getOcean`: the ocean floor's depth noise; the terrain becomes deep ocean.
    fn ocean(
        &self,
        seed: i32,
        x: f32,
        z: f32,
        sample: &mut NoiseSample,
    ) -> Result<(), &'static str> {
        let raw_noise = self.ocean.try_value_seed_first(seed, x, z)?;
        sample.height_noise = self.levels.noise_levels.to_depth_noise(raw_noise);
        sample.terrain_type = self.deep_ocean.clone();
        Ok(())
    }

    /// `getInland`: the land height; the terrain becomes the blender's.
    fn inland(
        &self,
        seed: i32,
        x: f32,
        z: f32,
        sample: &mut NoiseSample,
        blender: &mut Blender,
    ) -> Result<(), &'static str> {
        let base_noise = sample.base_noise;
        let height_noise =
            self.land.try_value_with_seed_first(seed, x, z, blender)? * HEIGHT_MULTIPLIER;
        sample.height_noise = self
            .levels
            .noise_levels
            .to_height_noise(base_noise, height_noise);
        sample.terrain_type = self.land.terrain(blender).clone();
        Ok(())
    }

    /// `getBlend`: below `BEACH`, ocean floor to sea level (the continent's terrain stays); below `COAST`, sea level
    /// to the land height (the terrain becomes the blender's). `NoiseUtil.lerp` both times.
    fn blend(
        &self,
        seed: i32,
        x: f32,
        z: f32,
        sample: &mut NoiseSample,
        blender: &mut Blender,
    ) -> Result<(), &'static str> {
        if sample.continent_noise < BEACH {
            let lower_raw = self.ocean.try_value_seed_first(seed, x, z)?;
            let lower = self.levels.noise_levels.to_depth_noise(lower_raw);
            let upper = self.levels.noise_levels.height_min;
            let alpha = (sample.continent_noise - SHALLOW_OCEAN) / (BEACH - SHALLOW_OCEAN);
            sample.height_noise = lerp(lower, upper, alpha);
        } else if sample.continent_noise < COAST {
            let lower = self.levels.noise_levels.height_min;
            let base_noise = sample.base_noise;
            let upper_raw =
                self.land.try_value_with_seed_first(seed, x, z, blender)? * HEIGHT_MULTIPLIER;
            let upper = self
                .levels
                .noise_levels
                .to_height_noise(base_noise, upper_raw);
            let alpha = (sample.continent_noise - BEACH) / (COAST - BEACH);
            sample.height_noise = lerp(lower, upper, alpha);
            sample.terrain_type = self.land.terrain(blender).clone();
        }
        Ok(())
    }
}

/// `createOceanTerrain`: `Source.simplex(OCEAN_OFFSET, 64, 3).scale(0.4)`.
fn create_ocean_terrain() -> Module {
    Module::source(Source::new(
        SourceKind::Simplex,
        SourceConfig {
            seed: OCEAN_OFFSET,
            octaves: 3,
            frequency: 1.0 / 64.0,
            ..SourceConfig::default()
        },
    ))
    .scale(Module::constant(0.4))
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged_context::{GeneratorContext, GeneratorSettings},
        terraforged_terrain_blender::TerrainNoise,
    };
    use super::*;
    use std::{
        panic::{AssertUnwindSafe, catch_unwind},
        sync::Arc,
    };

    /// `NoiseGeneratorOracle.java` (`.local/terraforged/oracles/noise_generator/`, `run.sh`; its `branches` lines are not
    /// copied, the test checks the coverage itself): the newer source's own
    /// `NoiseGenerator` constructor over Noise2D 2cb4843, continent caches bypassed, a fresh generator per runtime seed.
    const EXPECTED: &str = "\
grid 0 0 0 cccfc1e3f1470373\n\
edges 0 0 0 278c0722de69dac2\n\
fedges 0 0 0 191cd9eb3274e819\n\
grid 0 0 42 ce26ac8d7e67bc44\n\
grid 0 0 -17 648fa9a5248d77ea\n\
grid 0 0 2147483647 f897187f984c850c\n\
grid 0 0 -2147483648 cba8134c4ba7512c\n\
edges 0 0 -2147483648 c6a1e778cd886032\n\
fedges 0 0 -2147483648 9adc7d79941cd78a\n\
grid 0 0 -8763214 36c44c3e457a6622\n\
grid 0 0 -21491124 9ffb966b3f81929\n\
grid 0 1 0 cfc225d15311d598\n\
edges 0 1 0 69a11925fce4d140\n\
fedges 0 1 0 3812527c12ec3214\n\
grid 0 1 42 688d79dda6134bd0\n\
grid 0 1 -17 27451b0e39b9a1e7\n\
grid 0 1 2147483647 3810b22d5c0b6b7f\n\
grid 0 1 -2147483648 d3c9eb71daed0555\n\
edges 0 1 -2147483648 6534be3e100000c0\n\
fedges 0 1 -2147483648 581431ee737ea4df\n\
grid 0 1 -8763214 d7c3cdc63608776a\n\
grid 0 1 -21491124 85b47edc33b79587\n\
grid 1 0 0 2401f7f2d2315767\n\
edges 1 0 0 bc334141902df3fd\n\
fedges 1 0 0 57ee311302bf1298\n\
grid 1 0 42 7a2735abfa17bf9a\n\
grid 1 0 -17 7e2b5311023397ea\n\
grid 1 0 2147483647 510e7bb6ba49b491\n\
grid 1 0 -2147483648 9501f40fba47a51c\n\
edges 1 0 -2147483648 7ec7e2d52ef831fa\n\
fedges 1 0 -2147483648 ec6124621821786f\n\
grid 1 0 -8763214 119f5b38e304be01\n\
grid 1 0 -21491124 1227208c8d3e0879\n\
grid 1 1 0 2f2d9d688739122f\n\
edges 1 1 0 af9be7ef4d224467\n\
fedges 1 1 0 4de9d76f2c4f62e6\n\
grid 1 1 42 cbc649d719956dbe\n\
grid 1 1 -17 a5c59bce1dd16754\n\
grid 1 1 2147483647 5a3d4115522e4549\n\
grid 1 1 -2147483648 ea92522f6d33d0cf\n\
edges 1 1 -2147483648 5b291e89a07eac0\n\
fedges 1 1 -2147483648 632e55550729c892\n\
grid 1 1 -8763214 a13b82e11809726\n\
grid 1 1 -21491124 c10ec9d1b3fa570b\n\
grid 2 0 0 39493eff9afa0654\n\
edges 2 0 0 1c6756b2a0c91acc\n\
fedges 2 0 0 7e599ad772d4f67d\n\
grid 2 0 42 afe78db9342efa50\n\
grid 2 0 -17 e539f916f7cac028\n\
grid 2 0 2147483647 442b9628f06721ed\n\
grid 2 0 -2147483648 c3d7d47aa2c9b5ea\n\
edges 2 0 -2147483648 cb8a848a182ba19e\n\
fedges 2 0 -2147483648 1782deb5c726f847\n\
grid 2 0 -8763214 6764195a5f73ac53\n\
grid 2 0 -21491124 fa0b72ab62fa389\n\
grid 2 1 0 6de99002797f6d8f\n\
edges 2 1 0 7c3ea12f2a197021\n\
fedges 2 1 0 ca1485566ab1df21\n\
grid 2 1 42 b917d2e117fb6114\n\
grid 2 1 -17 b2d263bd5f1412d\n\
grid 2 1 2147483647 c5a04e54d619bc24\n\
grid 2 1 -2147483648 92d3f1fdb3faef18\n\
edges 2 1 -2147483648 ff38609f76ea11e\n\
fedges 2 1 -2147483648 88854414c13b7c74\n\
grid 2 1 -8763214 7ed4dab378e57239\n\
grid 2 1 -21491124 3ed3d59af446ef82";

    const RUNTIME: [i32; 7] = [0, 42, -17, i32::MAX, i32::MIN, -8_763_214, -21_491_124];
    const EDGE_INTS: [i32; 10] = [
        0,
        1,
        -1,
        i32::MAX,
        i32::MIN,
        16_777_217,
        -16_777_217,
        30_000_000,
        -29_999_999,
        123_456_789,
    ];
    const EDGE_FLOATS: [f32; 12] = [
        -0.0,
        0.0,
        f32::from_bits(1),
        -f32::from_bits(1),
        0.5,
        -1.75,
        1e6,
        -1e6,
        1e9,
        16_777_217.0,
        f32::NAN,
        f32::INFINITY,
    ];

    fn levels(variant: usize) -> TerrainLevels {
        match variant {
            0 => TerrainLevels::new(false, 1.0, -64, 480, 128, 62, 22),
            1 => TerrainLevels::new(false, 1.0, -64, 256, 128, 62, 22),
            _ => TerrainLevels::new(true, 2.5, 0, 480, 200, 63, 30),
        }
    }

    /// `createContinentNoise(levels)`'s settings. Java sets the continent scale and control points after building the
    /// context; the context reads only the seed and the world properties, so setting them first is the same.
    fn continent_settings(levels: TerrainLevels) -> GeneratorSettings {
        let mut settings = GeneratorSettings::default();
        settings.world.seed = CONTINENT_OFFSET;
        settings.world.properties.sea_level = levels.sea_level;
        settings.world.properties.world_height = levels.max_y;
        settings.climate.biome_shape.biome_size = 220;
        settings.climate.temperature.falloff = 2;
        settings.climate.temperature.bias = 0.1;
        settings.climate.moisture.falloff = 1;
        settings.climate.moisture.bias = -0.05;
        settings.world.continent.continent_scale = 400;
        settings.world.control_points.deep_ocean = 0.05;
        settings.world.control_points.shallow_ocean = 0.3;
        settings.world.control_points.beach = 0.45;
        settings.world.control_points.coast = 0.75;
        settings.world.control_points.inland = 0.80;
        settings
    }

    /// `Source.<kind>(seed, scale, octaves)`: Noise2D's builder defaults, where an unset gain is 0.975 for
    /// `ridge()`/`simplexRidge()` and 0.5 otherwise.
    fn source(kind: SourceKind, seed: i32, scale: f32, octaves: usize) -> Module {
        let gain = match kind {
            SourceKind::Ridge | SourceKind::SimplexRidge => 0.975,
            _ => 0.5,
        };
        Module::source(Source::new(
            kind,
            SourceConfig {
                seed,
                octaves,
                gain,
                frequency: 1.0 / scale,
                ..SourceConfig::default()
            },
        ))
    }

    /// `new NoiseGenerator(levels, terrainNoises)`: the oracle's terrain noise sets, from the context's registry.
    fn generator_with(
        variant: usize,
        set: impl FnOnce(&GeneratorContext) -> Vec<Arc<TerrainNoise>>,
    ) -> NoiseGenerator {
        let levels = levels(variant);
        let context = GeneratorContext::new(continent_settings(levels));
        let noises = set(&context);
        let land = TerrainBlender::new(TERRAIN_OFFSET, 800, 0.8, 0.4, noises);
        NoiseGenerator::new(levels, ContinentNoise::new(levels, context), land)
    }

    fn terrain_set(context: &GeneratorContext, set: usize) -> Vec<Arc<TerrainNoise>> {
        let terrain = |name: &str, weight: f32, noise: Module| {
            Arc::new(TerrainNoise::new(
                context.terrain.get_by_name(name).unwrap(),
                weight,
                noise,
            ))
        };
        if set == 0 {
            return vec![terrain("hills", 1.0, Module::constant(0.25))];
        }
        vec![
            terrain("flats", 1.0, source(SourceKind::Simplex, 11, 90.0, 2)),
            terrain("hills", 2.0, source(SourceKind::Perlin, 3, 150.0, 3)),
            terrain(
                "mountains",
                1.5,
                source(SourceKind::Ridge, -7, 300.0, 4).scale(Module::constant(1.3)),
            ),
            terrain(
                "plateau",
                1.0,
                source(SourceKind::Billow, i32::MAX, 200.0, 3),
            ),
        ]
    }

    fn generator(variant: usize, set: usize) -> NoiseGenerator {
        generator_with(variant, |context| terrain_set(context, set))
    }

    struct Fold(u64);

    impl Fold {
        fn new() -> Self {
            Self(0xcbf2_9ce4_8422_2325)
        }

        fn add(&mut self, value: u64) {
            self.0 = (self.0 ^ value).wrapping_mul(0x100_0000_01b3);
        }

        fn float(&mut self, value: f32) {
            let bits = if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits()
            };
            self.add(u64::from(bits));
        }

        fn sample(&mut self, sample: &NoiseSample) {
            self.add(sample.continent_center as u64);
            self.float(sample.continent_noise);
            self.float(sample.base_noise);
            self.float(sample.height_noise);
            self.float(sample.river_noise);
            self.add(i64::from(sample.terrain_type.id()) as u64);
        }

        fn line(&mut self, name: String) -> String {
            let line = format!("{name} {:x}", self.0);
            *self = Self::new();
            line
        }
    }

    fn generator_lines(variant: usize, set: usize, seed: i32) -> Vec<String> {
        let generator = generator(variant, set);
        let terrains = Arc::clone(&generator.continent().context().terrain);
        let mut blender = generator.blender_resource();
        let mut fold = Fold::new();
        let mut branches = [0; 5];
        let mut lines = Vec::new();
        // One sample for the whole grid, as chunk generation reuses one: fields a branch leaves alone carry over.
        let mut sample = NoiseSample::new(&terrains);
        for j in -8..=8 {
            for i in -8..=8 {
                generator
                    .sample(seed, i * 613 - 211, j * 587 + 97, &mut sample, &mut blender)
                    .unwrap();
                let c = sample.continent_noise;
                let branch = if c < 0.25 {
                    0
                } else if c < 0.5 {
                    1
                } else if c < 0.55 {
                    2
                } else if c < 0.6 {
                    3
                } else {
                    4
                };
                branches[branch] += 1;
                fold.sample(&sample);
            }
        }
        let id = format!("{variant} {set} {seed}");
        lines.push(fold.line(format!("grid {id}")));
        // The grid reaches all four height branches (deep ocean, ocean floor to sea level, sea level to land, land).
        assert!(
            branches[0] > 0 && branches[1] > 0 && branches[2] > 0 && branches[4] > 0,
            "{id}: {branches:?}"
        );
        if seed == 0 || seed == i32::MIN {
            for z in EDGE_INTS {
                for x in EDGE_INTS {
                    let mut sample = NoiseSample::new(&terrains);
                    generator
                        .sample(seed, x, z, &mut sample, &mut blender)
                        .unwrap();
                    fold.sample(&sample);
                }
            }
            lines.push(fold.line(format!("edges {id}")));
            for z in EDGE_FLOATS {
                for x in EDGE_FLOATS {
                    let mut sample = NoiseSample::new(&terrains);
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        generator
                            .sample_terrain(seed, x, z, &mut sample, &mut blender)
                            .unwrap();
                        generator.sample_river(seed, x, z, &mut sample);
                    }));
                    match result {
                        Ok(()) => fold.sample(&sample),
                        // Java throws (the blender's cell search finds no cell).
                        Err(_) => fold.add(0xffff_ffff),
                    }
                }
            }
            lines.push(fold.line(format!("fedges {id}")));
        }
        lines
    }

    #[test]
    fn samples_match_current_java_generator() {
        let mut lines = Vec::new();
        for variant in 0..3 {
            for set in 0..2 {
                for seed in RUNTIME {
                    lines.extend(generator_lines(variant, set, seed));
                }
            }
        }
        let expected: Vec<&str> = EXPECTED.lines().collect();
        assert_eq!(lines.len(), expected.len());
        for (actual, expected) in lines.iter().zip(&expected) {
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn deep_ocean_is_the_world_registrys() {
        let generator = generator(0, 0);
        let terrains = Arc::clone(&generator.continent().context().terrain);
        let deep_ocean = terrains.get_by_id(1);
        let mut blender = generator.blender_resource();
        let mut found = false;
        for i in -40..40 {
            let mut sample = NoiseSample::new(&terrains);
            generator
                .sample(0, i * 613, 97, &mut sample, &mut blender)
                .unwrap();
            if sample.continent_noise < SHALLOW_OCEAN {
                assert!(sample.terrain_type.is_same(&deep_ocean));
                found = true;
            }
        }
        assert!(found);
    }

    /// A custom terrain noise has no seed-first form: land samples fail, ocean samples (which never read the
    /// blender) still succeed and equal a seed-first generator's.
    #[test]
    fn custom_land_noise_is_an_error_not_a_seedless_sample() {
        let custom = generator_with(0, |context| {
            vec![Arc::new(TerrainNoise::new(
                context.terrain.get_by_name("hills").unwrap(),
                1.0,
                Module::custom(0.0, 1.0, |_, _| 0.25),
            ))]
        });
        let reference = generator(0, 0);
        let terrains = Arc::clone(&custom.continent().context().terrain);
        let (mut blender, mut reference_blender) =
            (custom.blender_resource(), reference.blender_resource());
        let (mut ocean, mut land) = (0, 0);
        for j in -8..=8 {
            for i in -8..=8 {
                let (x, z) = (i * 613 - 211, j * 587 + 97);
                let mut expected = NoiseSample::new(&terrains);
                reference
                    .sample(0, x, z, &mut expected, &mut reference_blender)
                    .unwrap();
                let mut sample = NoiseSample::new(&terrains);
                let result = custom.sample(0, x, z, &mut sample, &mut blender);
                if expected.continent_noise < BEACH {
                    ocean += 1;
                    let sample = result.unwrap();
                    assert_eq!(
                        sample.height_noise.to_bits(),
                        expected.height_noise.to_bits()
                    );
                    assert_eq!(sample.river_noise.to_bits(), expected.river_noise.to_bits());
                } else {
                    land += 1;
                    assert!(result.is_err());
                }
            }
        }
        assert!(ocean > 0 && land > 0);
    }
}
