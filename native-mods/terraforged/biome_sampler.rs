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
    terraforged::{hash, java_max},
    terraforged_biome_map::BiomePools,
    terraforged_engine::{BiomeType, TerrainProperties, TerrainRegistry},
    terraforged_terrain_blend_util::random,
    terraforged_terrain_data::NoiseSample,
    terraforged_terrain_levels::TerrainLevels,
};
use std::sync::Arc;

/// `ContinentPoints.DEEP_OCEAN`: continent noise of the deep-ocean control point. `DEEP_OCEAN`, `COAST` and
/// `INLAND` are unused here until the continent control points (#113) re-export all five.
pub const DEEP_OCEAN: f32 = 0.1;
/// `ContinentPoints.SHALLOW_OCEAN`: at or below it, `getBiomeOverride` picks a deep ocean.
pub const SHALLOW_OCEAN: f32 = 0.25;
/// `ContinentPoints.BEACH`: at or below it (above `SHALLOW_OCEAN`), a shallow ocean.
pub const BEACH: f32 = 0.5;
/// `ContinentPoints.COAST`.
pub const COAST: f32 = 0.55;
/// `ContinentPoints.INLAND`.
pub const INLAND: f32 = 0.6;
/// `BiomeSampler.beachSize`: the beach band is `BEACH < noise <= BEACH + BEACH_SIZE` (javac folds the sum
/// to the float `0.505f`, as this f32 constant sum does).
pub const BEACH_SIZE: f32 = COAST - BEACH;
/// Reimagined's `BiomeSampler.SLOPE_STEP`: blocks between the slope gate's opposite height samples.
pub const SLOPE_STEP: i32 = 24;
/// Reimagined's `maxFlatGradient`: blocks of rise per block above which ground is steep (about 23 degrees).
pub const MAX_FLAT_GRADIENT: f32 = 0.42;
/// The salt of `lowlandPool`'s wetland roll.
pub const WETLAND_SALT: i32 = 4153;
/// The salt of `getInitialBiome`'s mountain-usage roll.
pub const MOUNTAIN_SALT: i32 = 7309;
/// The biome `getInitialBiome` falls back to when the lowland pool has nothing for the column (`Biomes.PLAINS`).
pub const PLAINS: &str = "minecraft:plains";

/// TerraForged's `ClimateSample`: a `NoiseSample` plus the climate fields `ClimateNoise` fills.
#[derive(Clone)]
pub struct ClimateSample {
    pub noise: NoiseSample,
    pub biome_noise: f32,
    pub biome_edge_noise: f32,
    pub moisture: f32,
    pub temperature: f32,
    pub climate_type: BiomeType,
}

impl ClimateSample {
    /// `new ClimateSample()`: `NoiseSample`'s defaults (`terrains`' `none` as `TerrainType.NONE`), climate
    /// fields 0 and `GRASSLAND`.
    pub fn new(terrains: &TerrainRegistry) -> Self {
        Self {
            noise: NoiseSample::new(terrains),
            biome_noise: 0.0,
            biome_edge_noise: 0.0,
            moisture: 0.0,
            temperature: 0.0,
            climate_type: BiomeType::Grassland,
        }
    }

    /// `reset()`: resets only the `NoiseSample` part (`NoiseSample.reset`); the climate fields keep their values.
    pub fn reset(&mut self, terrains: &TerrainRegistry) -> &mut Self {
        self.noise.reset(terrains);
        self
    }
}

/// What `BiomeSampler.getBiomeOverride` returns: the input biome, or one of Minecraft's biomes. The jar
/// reads them as `Biomes` keys through `BiomeMapManager.get`; Minecraft's holders don't exist here, so
/// each variant names its key (`key`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BiomeOverride {
    Input,
    DeepColdOcean,
    DeepFrozenOcean,
    DeepLukewarmOcean,
    DeepOcean,
    ColdOcean,
    FrozenOcean,
    WarmOcean,
    Ocean,
    SnowyBeach,
    StonyShore,
    Beach,
    FrozenRiver,
    River,
}

impl BiomeOverride {
    /// The Minecraft biome key (`minecraft:<path>`) of the override, or `None` for `Input`.
    pub const fn key(self) -> Option<&'static str> {
        Some(match self {
            Self::Input => return None,
            Self::DeepColdOcean => "minecraft:deep_cold_ocean",
            Self::DeepFrozenOcean => "minecraft:deep_frozen_ocean",
            Self::DeepLukewarmOcean => "minecraft:deep_lukewarm_ocean",
            Self::DeepOcean => "minecraft:deep_ocean",
            Self::ColdOcean => "minecraft:cold_ocean",
            Self::FrozenOcean => "minecraft:frozen_ocean",
            Self::WarmOcean => "minecraft:warm_ocean",
            Self::Ocean => "minecraft:ocean",
            Self::SnowyBeach => "minecraft:snowy_beach",
            Self::StonyShore => "minecraft:stony_shore",
            Self::Beach => "minecraft:beach",
            Self::FrozenRiver => "minecraft:frozen_river",
            Self::River => "minecraft:river",
        })
    }
}

/// `BiomeSampler.getBiomeOverride(input, sample)`: checks, in the jar's order, deep ocean
/// (`continentNoise <= SHALLOW_OCEAN`), shallow ocean (`<= BEACH`), beach (`<= BEACH + BEACH_SIZE`), then a
/// river or lake terrain whose river noise `== 0` (either signed zero); anything else keeps the input. A NaN
/// continent noise fails the three bands, a NaN river noise the river test, as Java's float compares do.
/// The climate type picks the variant (taiga and cold steppe cold, tundra frozen, desert, savanna and
/// tropical rainforest warm; at the beach tundra is snowy and cold steppe a stony shore).
pub fn biome_override(sample: &ClimateSample) -> BiomeOverride {
    let climate = sample.climate_type;
    let continent = sample.noise.continent_noise;
    if continent <= SHALLOW_OCEAN {
        return match climate {
            BiomeType::Taiga | BiomeType::ColdSteppe => BiomeOverride::DeepColdOcean,
            BiomeType::Tundra => BiomeOverride::DeepFrozenOcean,
            BiomeType::Desert | BiomeType::Savanna | BiomeType::TropicalRainforest => {
                BiomeOverride::DeepLukewarmOcean
            }
            _ => BiomeOverride::DeepOcean,
        };
    }
    if continent <= BEACH {
        return match climate {
            BiomeType::Taiga | BiomeType::ColdSteppe => BiomeOverride::ColdOcean,
            BiomeType::Tundra => BiomeOverride::FrozenOcean,
            BiomeType::Desert | BiomeType::Savanna | BiomeType::TropicalRainforest => {
                BiomeOverride::WarmOcean
            }
            _ => BiomeOverride::Ocean,
        };
    }
    if continent <= BEACH + BEACH_SIZE {
        return match climate {
            BiomeType::Tundra => BiomeOverride::SnowyBeach,
            BiomeType::ColdSteppe => BiomeOverride::StonyShore,
            _ => BiomeOverride::Beach,
        };
    }
    let terrain = &sample.noise.terrain_type;
    if (terrain.is_river() || terrain.is_lake()) && sample.noise.river_noise == 0.0 {
        return if climate == BiomeType::Tundra {
            BiomeOverride::FrozenRiver
        } else {
            BiomeOverride::River
        };
    }
    BiomeOverride::Input
}

// The surface-biome decisions below are adapted from TerraForged Reimagined
// (https://github.com/Romoslayer/TerraForged-Reimagined, a24d450): `BiomeSampler`'s `lowlandPool`, `cellRoll`,
// `isSteep`, `freezes`, `keepsSnowCover`, `fromPool` and `getInitialBiome` (its mountain-usage gate and the assembly in
// `InitialBiomeSelector`). TerraForged 6dd607eb and the 1.18.2-0.3.1 jar pick from one pool with no wetland, slope
// or snow checks. MIT License, Copyright (c) 2020
// TerraForged, Copyright (c) 2026 Romoslayer; see `LICENSE-TERRAFORGED-REIMAGINED`.

/// Reimagined's `TerraSettings.Rivers.Wetlands`, as `BiomeSampler.lowlandPool` reads it: wetland biomes are
/// eligible where `riverNoise < river_size` or `continentNoise < coast_size`, and `chance` thins them per
/// biome cell. Reimagined's defaults are `chance` 1, `river_size` 0.75 and `coast_size` 0.7.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WetlandSettings {
    pub river_size: f32,
    pub coast_size: f32,
    pub chance: f32,
}

/// Which flat-ground pool `BiomeSampler.lowlandPool` picks: `getBiomeMap()` (wetlands included) or
/// `getDryBiomeMap()`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LowlandPool {
    Wet,
    Dry,
}

/// `BiomeSampler.lowlandPool`: wet when the column is eligible (river or coast reach, see [`WetlandSettings`]) and
/// `chance >= 1` or the cell's [`cell_roll`] with salt 4153 is below `chance`. Java's compares: a NaN noise or
/// setting fails its test.
pub fn lowland_pool(sample: &ClimateSample, wetlands: WetlandSettings) -> LowlandPool {
    let eligible = sample.noise.river_noise < wetlands.river_size
        || sample.noise.continent_noise < wetlands.coast_size;
    let wet = eligible
        && (wetlands.chance >= 1.0
            || cell_roll(sample.biome_noise, WETLAND_SALT) < wetlands.chance);
    if wet {
        LowlandPool::Wet
    } else {
        LowlandPool::Dry
    }
}

/// `getInitialBiome`'s mountain-biome-usage gate (`miscellaneous.mountainBiomeUsage`, Reimagined default 1):
/// `usage <= 0` never takes the highland pool; otherwise it does unless `usage < 1` and the cell's [`cell_roll`]
/// with salt 7309 is `>= usage`. A NaN usage passes both of Java's tests, so it always does. Java rolls this
/// before measuring the slope ([`is_steep_at`]); both are pure, so the order only saves work.
pub fn uses_mountain_pool(biome_noise: f32, mountain_usage: f32) -> bool {
    if mountain_usage <= 0.0 {
        return false;
    }
    !(mountain_usage < 1.0 && cell_roll(biome_noise, MOUNTAIN_SALT) >= mountain_usage)
}

/// `BiomeSampler.isSteep(seed, x, z)`: reads the height noise `half = max(1, SLOPE_STEP / 2)` blocks west, east,
/// north and south of the block column (Java's order and `int` wrap), then applies [`is_steep`]. `x`/`z` are
/// block coordinates (Reimagined's `Source.compute` passes quart `x << 2`, `z << 2`). `height_noise` is
/// `getHeightNoise(seed, x, z)` for the world's seed, the UN-eroded height (Reimagined's `ErodedNoiseGenerator`
/// delegates it to the raw generator on purpose, as `ErodedNoiseGenerator`'s `NoiseGenerator` impl does here);
/// its first error is returned. `max_y` is the generator's `TerrainLevels.maxY`.
pub fn is_steep_at<E>(
    max_y: i32,
    x: i32,
    z: i32,
    mut height_noise: impl FnMut(i32, i32) -> Result<f32, E>,
) -> Result<bool, E> {
    let half = 1.max(SLOPE_STEP / 2);
    let west = height_noise(x.wrapping_sub(half), z)?;
    let east = height_noise(x.wrapping_add(half), z)?;
    let north = height_noise(x, z.wrapping_sub(half))?;
    let south = height_noise(x, z.wrapping_add(half))?;
    Ok(is_steep(max_y, west, east, north, south))
}

/// `isSteep`'s arithmetic over the four height-noise samples: `max(|(east - west) * max_y|,
/// |(south - north) * max_y|) / (2 * half)` in Java's float order, steep when above [`MAX_FLAT_GRADIENT`].
/// `Math.max` keeps a NaN, so a NaN sample is never steep.
pub fn is_steep(max_y: i32, west: f32, east: f32, north: f32, south: f32) -> bool {
    let half = 1.max(SLOPE_STEP / 2);
    let dx = (east - west) * max_y as f32;
    let dz = (south - north) * max_y as f32;
    let gradient = java_max(dx.abs(), dz.abs()) / (half * 2) as f32;
    gradient > MAX_FLAT_GRADIENT
}

/// `BiomeSampler.freezes(biome, height)`, vanilla's snow rule without its height noise: above `seaLevel + 17`
/// the base temperature drops `0.05` per 40 blocks (Java `int` wrap, then `(float) d * 0.05F / 40F`), and
/// anything below `0.15` takes snow.
pub fn freezes(base_temperature: f32, height: i32, sea_level: i32) -> bool {
    let threshold = sea_level.wrapping_add(17);
    let mut temperature = base_temperature;
    if height > threshold {
        temperature -= height.wrapping_sub(threshold) as f32 * 0.05 / 40.0;
    }
    temperature < 0.15
}

/// `BiomeSampler.keepsSnowCover`: a highland substitute is accepted only when it [`freezes`] exactly where the
/// normal biome does at this height. `getInitialBiome` passes the column's centre height,
/// `levels.getHeight(levels.getScaledHeight(getHeightNoise(seed, x, z)))` (terrain_levels.rs `height`/
/// `scaled_height`), and each biome's base temperature.
pub fn keeps_snow_cover(
    normal_temperature: f32,
    highland_temperature: f32,
    height: i32,
    sea_level: i32,
) -> bool {
    freezes(normal_temperature, height, sea_level)
        == freezes(highland_temperature, height, sea_level)
}

/// What Reimagined's `BiomeSampler.getInitialBiome` reads besides the sample: the wet (`getBiomeMap`), dry
/// (`getDryBiomeMap`) and highland (`getHighlandBiomeMap`) pools, the registry's [`PLAINS`] biome, the wetland and
/// mountain-usage settings and the generator's `TerrainLevels` (`maxY` for the slope gate, the column height and
/// `seaLevel` for the snow check). Biomes are compared by identity (`Arc::ptr_eq`, Java's `==` on registry holders),
/// so the pools must share one `Arc` per biome.
pub struct InitialBiomeSelector<'a, T> {
    pub wet: &'a BiomePools<T>,
    pub dry: &'a BiomePools<T>,
    pub highland: &'a BiomePools<T>,
    pub plains: &'a Arc<T>,
    pub wetlands: WetlandSettings,
    pub mountain_usage: f32,
    pub levels: TerrainLevels,
}

impl<T> InitialBiomeSelector<'_, T> {
    /// `BiomeSampler.getInitialBiome(seed, x, z, sample)`, in Java's order: the normal biome from the
    /// [`lowland_pool`] (missing or empty climate pool, or a noise past its last band or NaN: [`PLAINS`], returned
    /// at once); the [`uses_mountain_pool`] gate; the centre height noise, then [`is_steep_at`]; the highland pool's
    /// biome (none, or the normal biome itself: normal; the identity test only saves the snow check, which would keep
    /// the same biome too); then [`keeps_snow_cover`] at the column's height
    /// (`levels.getHeight(levels.getScaledHeight(centre))`). Pools pick with `WeightMap.getValue(biomeNoise)`.
    /// `height_noise` is `getHeightNoise(seed, x, z)` for the world's seed (UN-eroded, see [`is_steep_at`]); `x`/`z`
    /// are block coordinates; the first error is returned. `getBiomeOverride` ([`biome_override`]) runs after this
    /// and wins.
    pub fn initial_biome<E>(
        &self,
        sample: &ClimateSample,
        x: i32,
        z: i32,
        mut height_noise: impl FnMut(i32, i32) -> Result<f32, E>,
        base_temperature: impl Fn(&T) -> f32,
    ) -> Result<Arc<T>, E> {
        let lowland = match lowland_pool(sample, self.wetlands) {
            LowlandPool::Wet => self.wet,
            LowlandPool::Dry => self.dry,
        };
        let Some(normal) = from_pool(lowland, sample) else {
            return Ok(Arc::clone(self.plains));
        };
        if !uses_mountain_pool(sample.biome_noise, self.mountain_usage) {
            return Ok(normal);
        }
        let centre = height_noise(x, z)?;
        if !is_steep_at(self.levels.max_y, x, z, &mut height_noise)? {
            return Ok(normal);
        }
        let Some(highland) = from_pool(self.highland, sample) else {
            return Ok(normal);
        };
        if Arc::ptr_eq(&highland, &normal) {
            return Ok(normal);
        }
        let height = self.levels.height(self.levels.scaled_height(centre));
        let keeps = keeps_snow_cover(
            base_temperature(&normal),
            base_temperature(&highland),
            height,
            self.levels.sea_level,
        );
        Ok(if keeps { highland } else { normal })
    }
}

/// `BiomeSampler.fromPool`: `None` where Java returns null (no pool for the climate type, an empty pool, or
/// `getValue` past the last band or NaN).
fn from_pool<T>(pools: &BiomePools<T>, sample: &ClimateSample) -> Option<Arc<T>> {
    let pool = pools.get(sample.climate_type)?;
    if pool.is_empty() {
        return None;
    }
    pool.get_value(sample.biome_noise).cloned()
}

/// `BiomeSampler.cellRoll`: `MathUtil.rand(Float.floatToIntBits(biomeNoise), salt)`, a 0..1 value constant
/// over one biome cell. `floatToIntBits` folds every NaN to `0x7fc00000`.
pub fn cell_roll(biome_noise: f32, salt: i32) -> f32 {
    let bits = if biome_noise.is_nan() {
        0x7fc0_0000_u32
    } else {
        biome_noise.to_bits()
    } as i32;
    random(hash(bits, salt))
}

#[cfg(test)]
mod tests {
    use super::super::{terraforged_engine::Terrain, terraforged_terrain_blend_util::WeightMap};
    use super::*;

    /// `BiomeOverrideOracle.java` (`.local/terraforged/oracles/biome_override/`): TerraForged 1.18.2-0.3.1's own
    /// `getBiomeOverride`/`ClimateSample` bytecode. Letters are `BiomeOverride` variants in declaration order
    /// (`a` = `Input`); `C` lines: per climate type, every `CONTINENT` value over a river (river noise 0), then
    /// over hills; `T` lines: at continent 0.75, every terrain by every `RIVER` value; `P`: the whole product
    /// (climate × terrain × continent × river) as count and FNV-1a; `K`: the folded beach bound's bits.
    const ORACLE: &str = "\
N 0 3f800000 0 0 3f800000 none 0 0 0 0 5
R true 0 3f800000 0 0 3f800000 none 3f333333 3f4ccccd 3f666666 bdcccccd 9
C 0 ddddddddddhhhllllnnnnnnnnn ddddddddddhhhllllaaaaaaaaa
C 1 ddddddddddhhhllllnnnnnnnnn ddddddddddhhhllllaaaaaaaaa
C 2 ddddddddddhhhllllnnnnnnnnn ddddddddddhhhllllaaaaaaaaa
C 3 eeeeeeeeeeiiillllnnnnnnnnn eeeeeeeeeeiiillllaaaaaaaaa
C 4 eeeeeeeeeeiiillllnnnnnnnnn eeeeeeeeeeiiillllaaaaaaaaa
C 5 eeeeeeeeeeiiillllnnnnnnnnn eeeeeeeeeeiiillllaaaaaaaaa
C 6 bbbbbbbbbbfffkkkknnnnnnnnn bbbbbbbbbbfffkkkkaaaaaaaaa
C 7 eeeeeeeeeeiiillllnnnnnnnnn eeeeeeeeeeiiillllaaaaaaaaa
C 8 bbbbbbbbbbfffllllnnnnnnnnn bbbbbbbbbbfffllllaaaaaaaaa
C 9 ccccccccccgggjjjjmmmmmmmmm ccccccccccgggjjjjaaaaaaaaa
C 10 eeeeeeeeeeiiillllnnnnnnnnn eeeeeeeeeeiiillllaaaaaaaaa
T 5 aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa nnaaaaaaaa nnaaaaaaaa aaaaaaaaaa aaaaaaaaaa \
aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa nnaaaaaaaa nnaaaaaaaa aaaaaaaaaa
T 9 aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa mmaaaaaaaa mmaaaaaaaa aaaaaaaaaa aaaaaaaaaa \
aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa mmaaaaaaaa mmaaaaaaaa aaaaaaaaaa
P 54340 c2061fdda420f167
K 3f0147ae 19 river-hills flats-hills
";

    const CONTINENT: [f32; 26] = [
        f32::NEG_INFINITY,
        -f32::MAX,
        -1.0,
        -f32::from_bits(1),
        -0.0,
        0.0,
        f32::from_bits(1),
        0.1,
        f32::from_bits(0x3e7f_ffff),
        0.25,
        f32::from_bits(0x3e80_0001),
        f32::from_bits(0x3eff_ffff),
        0.5,
        f32::from_bits(0x3f00_0001),
        0.5 + 0.0049,
        f32::from_bits(0x3f01_47ad),
        0.505,
        f32::from_bits(0x3f01_47af),
        0.506,
        0.55,
        0.6,
        1.0,
        f32::MAX,
        f32::INFINITY,
        f32::NAN,
        f32::from_bits(0xffc0_0001),
    ];

    const RIVER: [f32; 10] = [
        0.0,
        -0.0,
        f32::from_bits(1),
        -f32::from_bits(1),
        1.0,
        0.5,
        -1.0,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ];

    fn letter(sample: &ClimateSample) -> char {
        (b'a' + biome_override(sample) as u8) as char
    }

    fn sample(
        terrains: &TerrainRegistry,
        climate: BiomeType,
        continent: f32,
        terrain: &Terrain,
        river: f32,
    ) -> ClimateSample {
        let mut sample = ClimateSample::new(terrains);
        sample.climate_type = climate;
        sample.noise.continent_noise = continent;
        sample.noise.terrain_type = terrain.clone();
        sample.noise.river_noise = river;
        sample
    }

    fn fields(sample: &ClimateSample) -> String {
        let noise = &sample.noise;
        format!(
            "{} {:x} {:x} {:x} {:x} {} {:x} {:x} {:x} {:x} {}",
            noise.continent_center,
            noise.continent_noise.to_bits(),
            noise.base_noise.to_bits(),
            noise.height_noise.to_bits(),
            noise.river_noise.to_bits(),
            noise.terrain_type.name(),
            sample.biome_noise.to_bits(),
            sample.biome_edge_noise.to_bits(),
            sample.moisture.to_bits(),
            sample.temperature.to_bits(),
            sample.climate_type as u8
        )
    }

    fn oracle_output() -> String {
        let mut terrains = TerrainRegistry::new();
        let builtin = |name: &str, terrains: &TerrainRegistry| terrains.get_by_name(name).unwrap();
        let (river, lake, hills) = (
            builtin("river", &terrains),
            builtin("lake", &terrains),
            builtin("hills", &terrains),
        );
        let (mountains, flats) = (builtin("mountains", &terrains), builtin("flats", &terrains));
        terrains.register_composite(&river, &hills);
        terrains.register_composite(&lake, &mountains);
        terrains.register_composite(&flats, &hills);
        let all = terrains.terrains().to_vec();
        let mut out = String::new();

        let mut climate = ClimateSample::new(&terrains);
        out += &format!("N {}\n", fields(&climate));
        climate.noise.continent_center = -5;
        climate.noise.continent_noise = 0.3;
        climate.noise.base_noise = 0.4;
        climate.noise.height_noise = 0.6;
        climate.noise.river_noise = 0.0;
        climate.noise.terrain_type = river.clone();
        climate.biome_noise = 0.7;
        climate.biome_edge_noise = 0.8;
        climate.moisture = 0.9;
        climate.temperature = -0.1;
        climate.climate_type = BiomeType::Tundra;
        let same = std::ptr::eq(
            climate.reset(&terrains) as *const ClimateSample,
            &raw const climate,
        );
        out += &format!("R {same} {}\n", fields(&climate));
        assert!(climate.noise.terrain_type.is_same(&terrains.get_by_id(0)));

        for kind in BiomeType::VALUES {
            let line = |terrain: &Terrain| -> String {
                CONTINENT
                    .iter()
                    .map(|&c| letter(&sample(&terrains, kind, c, terrain, 0.0)))
                    .collect()
            };
            out += &format!("C {} {} {}\n", kind as u8, line(&river), line(&hills));
        }
        for kind in [BiomeType::Grassland, BiomeType::Tundra] {
            let line: Vec<String> = all
                .iter()
                .map(|terrain| {
                    RIVER
                        .iter()
                        .map(|&r| letter(&sample(&terrains, kind, 0.75, terrain, r)))
                        .collect()
                })
                .collect();
            out += &format!("T {} {}\n", kind as u8, line.join(" "));
        }
        let (mut hash, mut count) = (0xcbf2_9ce4_8422_2325_u64, 0);
        for kind in BiomeType::VALUES {
            for terrain in &all {
                for continent in CONTINENT {
                    for river_noise in RIVER {
                        let mut s = sample(&terrains, kind, continent, terrain, river_noise);
                        s.biome_noise = river_noise;
                        s.temperature = continent;
                        hash = (hash ^ biome_override(&s) as u64).wrapping_mul(0x0100_0000_01b3);
                        count += 1;
                    }
                }
            }
        }
        out += &format!("P {count} {hash:x}\n");
        out += &format!(
            "K {:x} {} {} {}\n",
            (BEACH + BEACH_SIZE).to_bits(),
            all.len(),
            all[16].name(),
            all[18].name()
        );
        out
    }

    #[test]
    fn biome_overrides_match_terraforged_jar() {
        assert_eq!(oracle_output(), ORACLE);
    }

    #[test]
    fn beach_band_reaches_the_coast() {
        let terrains = TerrainRegistry::new();
        let hills = terrains.get_by_name("hills").unwrap();
        let at = |continent| {
            biome_override(&sample(&terrains, BiomeType::Grassland, continent, &hills, 1.0))
        };
        assert_eq!(at(BEACH + BEACH_SIZE), BiomeOverride::Beach);
        assert_eq!(at(BEACH + 0.006), BiomeOverride::Beach);
        assert_eq!(at(COAST + f32::EPSILON), BiomeOverride::Input);
    }

    #[test]
    fn override_keys_name_minecraft_biomes() {
        assert_eq!(BiomeOverride::Input.key(), None);
        assert_eq!(
            BiomeOverride::DeepLukewarmOcean.key(),
            Some("minecraft:deep_lukewarm_ocean")
        );
        assert_eq!(
            BiomeOverride::StonyShore.key(),
            Some("minecraft:stony_shore")
        );
        assert_eq!(
            BiomeOverride::FrozenRiver.key(),
            Some("minecraft:frozen_river")
        );
    }

    /// `BiomeSamplerDecisionOracle.java` (`.local/terraforged/oracles/biome_sampler_decisions/`, `run.sh`):
    /// Reimagined a24d450's own `BiomeSampler` compiled from source, interpreted and with `-Xcomp`. `R`: roll bits
    /// (salts 4153, 7309); `Q`: the slope gate's sample points around `(i32::MAX, i32::MIN)`; the other lines are a
    /// case count and FNV-1a over each case's result, replayed below from the same LCG.
    const DECISIONS: &str = "\
R 00000000 3c8361c0 3f67e0cc
R 3e800000 3e94b594 3f5e55dc
R 3f333334 3ec09a0e 3e8c1296
R 3f7fffff 3f020074 3f56d48c
R 7fc01234 3ecfd000 3f67fd24
ROLL 32104 b4c35a89f728d491
WET 811200 7d4dd0a89c0a0c25
Q 2147483635,-2147483648 -2147483637,-2147483648 2147483647,2147483636 2147483647,-2147483636
STEEP 17640 862f3f69854a40d4
SNOW 7920 3118b3e53a0fa005
MOUNTAIN 6591 897ed70fc41e4b14
INITIAL 25600 61b4ea2aa079a049
";

    struct Lcg(u64);

    impl Lcg {
        fn int(&mut self) -> i32 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 32) as i32
        }

        fn unit(&mut self) -> f32 {
            ((self.int() as u32) >> 8) as f32 / 16_777_216.0
        }
    }

    struct Fold {
        hash: u64,
        count: u32,
        out: String,
    }

    impl Fold {
        fn add(&mut self, value: u32) {
            for byte in value.to_le_bytes() {
                self.hash = (self.hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
            }
            self.count += 1;
        }

        fn section(&mut self, name: &str) {
            self.out += &format!("{name} {} {:016x}\n", self.count, self.hash);
            self.hash = 0xcbf2_9ce4_8422_2325;
            self.count = 0;
        }
    }

    fn special() -> [f32; 26] {
        [
            0.0,
            -0.0,
            f32::from_bits(1),
            -f32::from_bits(1),
            0.25,
            0.5,
            0.7,
            0.7_f32.next_down(),
            0.7_f32.next_up(),
            0.75,
            0.75_f32.next_down(),
            0.75_f32.next_up(),
            1.0_f32.next_down(),
            1.0,
            1.0_f32.next_up(),
            1.5,
            -1.0,
            0.3,
            0.2,
            f32::MAX,
            -f32::MAX,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NAN,
            f32::from_bits(0x7fc0_1234),
            f32::from_bits(0xffc0_0001),
        ]
    }

    fn decisions_output() -> String {
        let mut rng = Lcg(0x5eed_1630_0000_0001);
        let mut fold = Fold {
            hash: 0xcbf2_9ce4_8422_2325,
            count: 0,
            out: String::new(),
        };
        let special = special();
        let terrains = TerrainRegistry::new();
        let mut sample = ClimateSample::new(&terrains);

        let mut noises = special.to_vec();
        noises.extend((0..4000).map(|_| f32::from_bits(rng.int() as u32)));
        noises.extend((0..4000).map(|_| rng.unit()));
        for i in 0..5 {
            let noise = noises[if i == 4 { 24 } else { i * 4 }];
            fold.out += &format!(
                "R {:08x} {:08x} {:08x}\n",
                noise.to_bits(),
                cell_roll(noise, WETLAND_SALT).to_bits(),
                cell_roll(noise, MOUNTAIN_SALT).to_bits()
            );
        }
        for &noise in &noises {
            for salt in [4153, 7309, 0, -1] {
                fold.add(cell_roll(noise, salt).to_bits());
            }
        }
        fold.section("ROLL");

        let reach = [0.75, 0.7, 0.0, 1.0, f32::NAN];
        let chances = [1.0, 0.3, 0.5, 0.0, -1.0, 1.5, 1.0_f32.next_down(), f32::NAN];
        let cells = [0.0, 0.25, 0.5, 1.0, f32::NAN, 0.123];
        for river in special {
            for continent in special {
                for chance in chances {
                    for river_size in reach {
                        for coast_size in reach {
                            for cell in cells {
                                sample.noise.river_noise = river;
                                sample.noise.continent_noise = continent;
                                sample.biome_noise = cell;
                                let wetlands = WetlandSettings {
                                    river_size,
                                    coast_size,
                                    chance,
                                };
                                fold.add(u32::from(
                                    lowland_pool(&sample, wetlands) == LowlandPool::Wet,
                                ));
                            }
                        }
                    }
                }
            }
        }
        fold.section("WET");

        let mut points = Vec::new();
        let _ = is_steep_at(480, i32::MAX, i32::MIN, |x, z| {
            points.push(format!("{x},{z}"));
            Ok::<_, ()>(0.0)
        });
        fold.out += &format!("Q {}\n", points.join(" "));
        let coords = [0, 5, -7, i32::MAX, i32::MIN, i32::MAX - 11, i32::MIN + 11];
        let max_ys = [480, 384, 1024, 256, 0, -480, 1, i32::MAX, i32::MIN];
        for cx in coords {
            for cz in coords {
                for max_y in max_ys {
                    for i in 0..40 {
                        let base = rng.unit();
                        let limit = if max_y == 0 {
                            0.01
                        } else {
                            0.42_f32 * 24.0 / max_y as f32
                        };
                        let mut step = base + limit;
                        let mut k = (rng.int() & 7) - 3;
                        while k != 0 {
                            step = if k > 0 {
                                step.next_up()
                            } else {
                                step.next_down()
                            };
                            k += if k > 0 { -1 } else { 1 };
                        }
                        let pick = i % 8;
                        let mut west = if pick < 4 {
                            base
                        } else {
                            special[((rng.int() as u32) >> 1) as usize % special.len()]
                        };
                        let mut east = if pick < 2 { step } else { rng.unit() };
                        let north = if pick == 2 || pick == 3 {
                            base
                        } else {
                            rng.unit()
                        };
                        let south = if pick == 2 || pick == 3 {
                            step
                        } else {
                            rng.unit()
                        };
                        if pick == 1 {
                            std::mem::swap(&mut west, &mut east);
                        }
                        let _seed = rng.int();
                        let steep = is_steep_at(max_y, cx, cz, |x, z| {
                            let half = SLOPE_STEP / 2;
                            Ok::<_, ()>(if (x, z) == (cx.wrapping_sub(half), cz) {
                                west
                            } else if (x, z) == (cx.wrapping_add(half), cz) {
                                east
                            } else if (x, z) == (cx, cz.wrapping_sub(half)) {
                                north
                            } else if (x, z) == (cx, cz.wrapping_add(half)) {
                                south
                            } else {
                                panic!("unexpected sample point {x},{z}")
                            })
                        });
                        fold.add(u32::from(steep.unwrap()));
                    }
                }
            }
        }
        fold.section("STEEP");

        let temps = [
            0.2,
            0.6,
            0.8,
            0.1,
            0.15,
            0.15_f32.next_up(),
            0.15_f32.next_down(),
            -0.5,
            2.0,
            0.0,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            0.95,
            0.25,
        ];
        let heights = [
            79,
            80,
            120,
            121,
            122,
            148,
            200,
            320,
            960,
            -64,
            0,
            i32::MAX,
            i32::MIN,
            i32::MIN + 100,
            1_000_000,
            16_777_300,
        ];
        let sea_levels = [62, 63, 0, -64, 200, i32::MAX, i32::MAX - 17, i32::MIN];
        for sea in sea_levels {
            for t in temps {
                for h in heights {
                    fold.add(u32::from(freezes(t, h, sea)));
                }
                for _ in 0..50 {
                    let h = rng.int() % 2000;
                    fold.add(u32::from(freezes(t, h, sea)));
                }
            }
        }
        fold.section("SNOW");

        // Java runs getInitialBiome on steep ground with a distinct highland biome of the same temperature, so
        // the result is the highland biome exactly when the usage gate passes.
        let usages = [
            1.0,
            0.0,
            0.5,
            0.2,
            0.1,
            -0.5,
            1.5,
            1.0_f32.next_down(),
            f32::from_bits(1),
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            0.75,
        ];
        let mut cells = vec![
            0.0,
            -0.0,
            0.25,
            0.5,
            0.75,
            1.0_f32.next_down(),
            f32::from_bits(1),
        ];
        cells.extend((0..500).map(|_| rng.unit() * 0.999));
        for usage in usages {
            for &cell in &cells {
                let steep =
                    is_steep_at(480, 0, 0, |x, _| Ok::<_, ()>(if x > 0 { 1.0 } else { 0.0 }));
                fold.add(u32::from(uses_mountain_pool(cell, usage) && steep.unwrap()));
            }
        }
        fold.section("MOUNTAIN");
        initial_output(&mut rng, &mut fold, &special, &mut sample);
        fold.out
    }

    struct TestBiome {
        temperature: f32,
    }

    /// The oracle's `initial` section: 64 worlds of pools from shared templates, 200 `getInitialBiome` samples each;
    /// folds the biome (index, -1 plains, -2 failed) and the number of height-noise reads.
    fn initial_output(
        rng: &mut Lcg,
        fold: &mut Fold,
        special: &[f32; 26],
        sample: &mut ClimateSample,
    ) {
        let temps = [
            0.8,
            0.6,
            0.2,
            0.1,
            0.15,
            0.15_f32.next_down(),
            0.95,
            f32::NAN,
        ];
        let biomes: Vec<_> = temps
            .iter()
            .map(|&temperature| Arc::new(TestBiome { temperature }))
            .collect();
        let plains = Arc::new(TestBiome { temperature: 0.8 });
        let members: [&[usize]; 11] = [
            &[],
            &[],
            &[0],
            &[1, 2],
            &[2, 3, 4],
            &[5, 6, 7],
            &[2],
            &[0, 1],
            &[3],
            &[1, 6],
            &[6, 2, 0],
        ];
        let weights: [&[f32]; 11] = [
            &[],
            &[1.0],
            &[1.0],
            &[1.0, 1.0],
            &[0.5, 2.0, 1.0],
            &[1.0, 0.0, 3.0],
            &[1.0],
            &[1.0],
            &[f32::NAN],
            &[2.0, 1.0],
            &[1.0, 1.0, 1.0],
        ];
        let mut templates = vec![None];
        for (values, weights) in members.iter().zip(weights) {
            let values = values.iter().map(|&i| Arc::clone(&biomes[i])).collect();
            templates.push(Some(Arc::new(WeightMap::new(values, weights))));
        }
        let chances = [1.0, 0.5, 0.0, f32::NAN];
        let usages = [1.0, 0.0, 0.5, f32::NAN, 0.25, 1.5];
        let max_ys = [480, 384, 1024, 256];
        let sea_levels = [62, 63, -64, 200];
        let coords = [0, -7, i32::MAX, i32::MIN];
        let pick = |rng: &mut Lcg, len: usize| ((rng.int() as u32) >> 1) as usize % len;
        for world in 0..64 {
            let mut pools: [BiomePools<TestBiome>; 3] = Default::default();
            for pool in &mut pools {
                for kind in BiomeType::VALUES {
                    let t = if world < 2 {
                        world
                    } else {
                        pick(rng, templates.len())
                    };
                    if let Some(template) = &templates[t] {
                        pool.insert(kind, Arc::clone(template));
                    }
                }
            }
            for _ in 0..200 {
                sample.climate_type = BiomeType::VALUES[pick(rng, 11)];
                sample.biome_noise = if rng.int() & 3 == 0 {
                    special[pick(rng, special.len())]
                } else {
                    rng.unit()
                };
                sample.noise.river_noise = rng.unit();
                sample.noise.continent_noise = rng.unit();
                let chance = chances[(rng.int() & 3) as usize];
                let mountain_usage = usages[pick(rng, usages.len())];
                let mut levels = TerrainLevels::unconfigured();
                levels.max_y = max_ys[(rng.int() & 3) as usize];
                levels.sea_level = sea_levels[(rng.int() & 3) as usize];
                let kind = rng.int() & 7;
                let x = if kind < 4 {
                    coords[kind as usize]
                } else {
                    rng.int()
                };
                let z = if kind < 4 {
                    coords[(rng.int() & 3) as usize]
                } else {
                    rng.int()
                };
                let west = rng.unit();
                let (mut east, mut north, mut south) = (west, west, west);
                match rng.int() & 3 {
                    0 => {}
                    1 => east = west + 0.5,
                    2 => {
                        east = west + 0.42_f32 * 24.0 / levels.max_y as f32;
                        let mut k = (rng.int() & 7) - 3;
                        while k != 0 {
                            east = if k > 0 {
                                east.next_up()
                            } else {
                                east.next_down()
                            };
                            k += if k > 0 { -1 } else { 1 };
                        }
                    }
                    _ => (east, north, south) = (rng.unit(), rng.unit(), rng.unit()),
                }
                let centre = if rng.int() & 1 == 0 {
                    rng.unit() * 0.5
                } else {
                    special[pick(rng, special.len())]
                };
                let throw_at = (rng.int() & 7) - 1;
                let _seed = rng.int();
                let selector = InitialBiomeSelector {
                    wet: &pools[0],
                    dry: &pools[1],
                    highland: &pools[2],
                    plains: &plains,
                    wetlands: WetlandSettings {
                        river_size: 0.75,
                        coast_size: 0.7,
                        chance,
                    },
                    mountain_usage,
                    levels,
                };
                let mut calls = 0;
                let half = SLOPE_STEP / 2;
                let got = selector.initial_biome(
                    sample,
                    x,
                    z,
                    |px, pz| {
                        calls += 1;
                        if calls - 1 == throw_at {
                            return Err(());
                        }
                        Ok(match (px, pz) {
                            _ if (px, pz) == (x.wrapping_sub(half), z) => west,
                            _ if (px, pz) == (x.wrapping_add(half), z) => east,
                            _ if (px, pz) == (x, z.wrapping_sub(half)) => north,
                            _ if (px, pz) == (x, z.wrapping_add(half)) => south,
                            _ if (px, pz) == (x, z) => centre,
                            _ => panic!("unexpected sample point {px},{pz}"),
                        })
                    },
                    |biome| biome.temperature,
                );
                let result = match got {
                    Err(()) => -2,
                    Ok(biome) if Arc::ptr_eq(&biome, &plains) => -1,
                    Ok(biome) => biomes
                        .iter()
                        .position(|b| Arc::ptr_eq(b, &biome))
                        .expect("a pooled biome") as i32,
                };
                fold.add(result as u32);
                fold.add(calls as u32);
            }
        }
        fold.section("INITIAL");
    }

    #[test]
    fn decisions_match_reimagined_java() {
        assert_eq!(decisions_output(), DECISIONS);
    }

    #[test]
    fn initial_biome_keeps_reimagined_order() {
        let biome = |temperature| Arc::new(TestBiome { temperature });
        let (plains, wet_biome, dry_biome, high_biome) =
            (biome(0.8), biome(0.6), biome(0.6), biome(0.2));
        let one =
            |biome: &Arc<TestBiome>| Arc::new(WeightMap::new(vec![Arc::clone(biome)], &[1.0]));
        let (mut wet, mut dry, mut highland) = Default::default();
        BiomePools::insert(&mut wet, BiomeType::Grassland, one(&wet_biome));
        BiomePools::insert(&mut dry, BiomeType::Grassland, one(&dry_biome));
        BiomePools::insert(&mut highland, BiomeType::Grassland, one(&high_biome));
        let mut selector = InitialBiomeSelector {
            wet: &wet,
            dry: &dry,
            highland: &highland,
            plains: &plains,
            wetlands: WetlandSettings {
                river_size: 0.75,
                coast_size: 0.7,
                chance: 1.0,
            },
            mountain_usage: 1.0,
            levels: TerrainLevels::unconfigured(),
        };
        let terrains = TerrainRegistry::new();
        let mut sample = ClimateSample::new(&terrains);
        sample.biome_noise = 0.5;
        sample.noise.river_noise = 0.5;
        sample.noise.continent_noise = 0.8;
        // Steep east-west at every point; the centre reads `centre`; returns the biome and the read count.
        let run = |selector: &InitialBiomeSelector<'_, TestBiome>,
                   sample: &ClimateSample,
                   centre: f32| {
            let mut reads = 0;
            let got = selector
                .initial_biome(
                    sample,
                    0,
                    0,
                    |x, _| {
                        reads += 1;
                        Ok::<_, ()>(if x == 0 {
                            centre
                        } else if x > 0 {
                            1.0
                        } else {
                            0.0
                        })
                    },
                    |biome| biome.temperature,
                )
                .unwrap();
            (got, reads)
        };
        let is = |(got, reads): (Arc<TestBiome>, i32), want: &Arc<TestBiome>, n: i32| {
            Arc::ptr_eq(&got, want) && reads == n
        };
        // y 79 (centre 79.5 / 480): neither freezes, the highland biome is taken.
        assert!(is(run(&selector, &sample, 79.5 / 480.0), &high_biome, 5));
        // y 122: only the highland biome would freeze, so the swap is refused.
        assert!(is(run(&selector, &sample, 122.5 / 480.0), &wet_biome, 5));
        // A zero usage never reads the height noise.
        selector.mountain_usage = 0.0;
        assert!(is(run(&selector, &sample, 0.0), &wet_biome, 0));
        selector.mountain_usage = 1.0;
        // Away from water: the dry pool; the highland pool's own biome is a no-op (identity).
        sample.noise.river_noise = 0.9;
        selector.highland = &dry;
        assert!(is(run(&selector, &sample, 0.0), &dry_biome, 5));
        // No pool for the climate (or a noise past the last band): plains at once, before any height read.
        sample.climate_type = BiomeType::Alpine;
        assert!(is(run(&selector, &sample, 0.0), &plains, 0));
        sample.climate_type = BiomeType::Grassland;
        sample.biome_noise = 1.5;
        assert!(is(run(&selector, &sample, 0.0), &plains, 0));
        assert_eq!(PLAINS, "minecraft:plains");
    }

    #[test]
    fn decision_gates_keep_reimagined_edge_cases() {
        let terrains = TerrainRegistry::new();
        let mut sample = ClimateSample::new(&terrains);
        sample.biome_noise = 0.5;
        sample.noise.river_noise = 0.5;
        sample.noise.continent_noise = 0.8;
        let settings = WetlandSettings {
            river_size: 0.75,
            coast_size: 0.7,
            chance: 0.3,
        };
        assert_eq!(lowland_pool(&sample, settings), LowlandPool::Wet);
        let thinner = WetlandSettings {
            chance: 0.2,
            ..settings
        };
        assert_eq!(lowland_pool(&sample, thinner), LowlandPool::Dry);
        sample.noise.river_noise = 0.8;
        assert_eq!(lowland_pool(&sample, settings), LowlandPool::Dry);
        assert!(uses_mountain_pool(0.5, 0.2));
        assert!(!uses_mountain_pool(0.5, 0.1));
        assert!(!uses_mountain_pool(0.5, 0.0));
        assert!(uses_mountain_pool(0.5, 1.0));
        // Java's `usage <= 0` and `usage < 1` both fail for NaN: the highland pool is always used.
        assert!(uses_mountain_pool(0.5, f32::NAN));
        // `Math.max` keeps NaN, so a NaN height sample is never steep (f32::max would drop it).
        assert!(!is_steep(480, f32::NAN, 0.0, 0.0, 1.0));
        assert!(is_steep(480, 0.0, 0.0, 0.0, 1.0));
        // Slope gate errors come back from the first failing sample.
        assert_eq!(
            is_steep_at(480, 0, 0, |x, _| if x < 0 { Err(x) } else { Ok(0.0) }),
            Err(-12)
        );
        assert!(!freezes(0.2, 79, 62));
        assert!(freezes(0.2, 122, 62));
        assert!(keeps_snow_cover(0.6, 0.2, 79, 62));
        assert!(!keeps_snow_cover(0.6, 0.2, 122, 62));
        // `height - threshold` wraps like Java's int.
        assert!(!freezes(0.2, i32::MAX, i32::MIN));
    }
}
