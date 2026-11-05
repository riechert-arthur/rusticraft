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
    terraforged_engine::{BiomeType, TerrainProperties, TerrainRegistry},
    terraforged_terrain_data::NoiseSample,
};

/// `ContinentPoints.DEEP_OCEAN`: continent noise of the deep-ocean control point.
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
pub const BEACH_SIZE: f32 = 0.005;

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

#[cfg(test)]
mod tests {
    use super::super::terraforged_engine::Terrain;
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
}
