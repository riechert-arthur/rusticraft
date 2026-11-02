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
    terraforged_biome_map::BiomePools,
    terraforged_engine::{BiomeType, TerrainProperties, TerrainRegistry},
    terraforged_terrain_data::NoiseSample,
};
use std::sync::Arc;

/// `ContinentPoints.DEEP_OCEAN`: continent noise of the deep-ocean control point. `DEEP_OCEAN` and `INLAND` are
/// unused here; the continent control points (#113) re-export all five.
pub const DEEP_OCEAN: f32 = 0.1;
/// `ContinentPoints.SHALLOW_OCEAN`: at or below it, `getBiomeOverride` picks a deep ocean.
pub const SHALLOW_OCEAN: f32 = 0.25;
/// `ContinentPoints.BEACH`: at or below it (above `SHALLOW_OCEAN`), a shallow ocean.
pub const BEACH: f32 = 0.5;
/// `ContinentPoints.COAST`. Not a beach bound in 6dd607eb (Reimagined's `beachSize = COAST - BEACH` is not followed).
pub const COAST: f32 = 0.55;
/// `ContinentPoints.INLAND`.
pub const INLAND: f32 = 0.6;
/// `BiomeSampler.beachSize` of the 1.18.2-0.3.1 jar and 6dd607eb (`protected final float beachSize = 0.005f`):
/// [`biome_override`]'s beach band is `BEACH < noise <= BEACH + BEACH_SIZE` (javac folds the sum to the float
/// `0.505f`, as this f32 constant sum does). Reimagined a24d450 widens it to `COAST - BEACH`; that is not followed.
pub const BEACH_SIZE: f32 = 0.005;
/// `Biomes.PLAINS`: what [`initial_biome`] gives when the climate type has no pool or an empty one.
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

/// The jar's (and 6dd607eb's) `BiomeSampler.getBiomeOverride(input, sample)`, used by the jar and the current
/// surface sampler alike: checks, in the jar's order, deep ocean
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

/// TerraForged 6dd607eb's `BiomeSampler.getInitialBiome(noise, climateType)`: the climate type's pool
/// (`biomeMapManager.getBiomeMap()`) picks with `WeightMap.getValue(sample.biome_noise)`; a missing or empty pool
/// gives `plains` (`getBiomes().getHolderOrThrow(Biomes.PLAINS)`, see [`PLAINS`]). One pool, no other input: the
/// wet/dry pools, highland swap, slope gate and snow check of TerraForged Reimagined's `getInitialBiome` are not
/// followed. `None` where Java's `getValue` returns null (a noise at or past the pool's last band, or NaN; a pool
/// whose weights are all zero returns null for every noise): Java hands that null on as the sampled biome, and
/// [`biome_override`] may still replace it.
pub fn initial_biome<T>(
    pools: &BiomePools<T>,
    sample: &ClimateSample,
    plains: &Arc<T>,
) -> Option<Arc<T>> {
    match pools.get(sample.climate_type) {
        Some(pool) if !pool.is_empty() => pool.get_value(sample.biome_noise).cloned(),
        _ => Some(Arc::clone(plains)),
    }
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

    /// `SourceBiomeOverrideOracle.java` (`.local/terraforged/oracles/biome_override/6dd607eb/`, `run.sh`): TerraForged
    /// 6dd607eb's own `getBiomeOverride`, `ClimateSample`, `NoiseSample` and `ContinentPoints` compiled from source
    /// against Engine 0.3.0 (`beachSize = 0.005f`), interpreted and with `-Xcomp`. Lines as in [`ORACLE`] over
    /// [`SOURCE_CONTINENT`] and Engine's 16 built-in terrains (no composites); `K`: the beach size's bits, the folded
    /// bound's (0.505f), `COAST`'s and the terrain names. The floats next to `COAST` and `BEACH + 0.05` are not beach.
    const SOURCE_ORACLE: &str = "\
C 0 ddddddddddhhhllllnnnnnnnnnnnnn ddddddddddhhhllllaaaaaaaaaaaaa
C 1 ddddddddddhhhllllnnnnnnnnnnnnn ddddddddddhhhllllaaaaaaaaaaaaa
C 2 ddddddddddhhhllllnnnnnnnnnnnnn ddddddddddhhhllllaaaaaaaaaaaaa
C 3 eeeeeeeeeeiiillllnnnnnnnnnnnnn eeeeeeeeeeiiillllaaaaaaaaaaaaa
C 4 eeeeeeeeeeiiillllnnnnnnnnnnnnn eeeeeeeeeeiiillllaaaaaaaaaaaaa
C 5 eeeeeeeeeeiiillllnnnnnnnnnnnnn eeeeeeeeeeiiillllaaaaaaaaaaaaa
C 6 bbbbbbbbbbfffkkkknnnnnnnnnnnnn bbbbbbbbbbfffkkkkaaaaaaaaaaaaa
C 7 eeeeeeeeeeiiillllnnnnnnnnnnnnn eeeeeeeeeeiiillllaaaaaaaaaaaaa
C 8 bbbbbbbbbbfffllllnnnnnnnnnnnnn bbbbbbbbbbfffllllaaaaaaaaaaaaa
C 9 ccccccccccgggjjjjmmmmmmmmmmmmm ccccccccccgggjjjjaaaaaaaaaaaaa
C 10 eeeeeeeeeeiiillllnnnnnnnnnnnnn eeeeeeeeeeiiillllaaaaaaaaaaaaa
T 5 aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa nnaaaaaaaa nnaaaaaaaa aaaaaaaaaa aaaaaaaaaa \
aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa
T 9 aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa mmaaaaaaaa mmaaaaaaaa aaaaaaaaaa aaaaaaaaaa \
aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa aaaaaaaaaa
P 52800 815d2546ac70da45
K 3ba3d70a 3f0147ae 3f0ccccd none deep_ocean ocean coast beach river lake wetland flats badlands plateau \
hills mountains mountain_chain volcano volcano_pipe
";

    /// [`CONTINENT`] plus the floats next to `COAST` and either side of `BEACH + 0.05` (where Reimagined's wider
    /// band would still give a beach).
    const SOURCE_CONTINENT: [f32; 30] = {
        let mut values = [0.0; 30];
        let mut i = 0;
        while i < CONTINENT.len() {
            values[i] = CONTINENT[i];
            i += 1;
        }
        values[26] = f32::from_bits(0x3f0c_cccc);
        values[27] = f32::from_bits(0x3f0c_ccce);
        values[28] = 0.5 + 0.0499;
        values[29] = 0.5 + 0.0501;
        values
    };

    fn source_oracle_output() -> String {
        let terrains = TerrainRegistry::new();
        let all = terrains.terrains().to_vec();
        let name = |n: &str| terrains.get_by_name(n).unwrap();
        let (river, hills) = (name("river"), name("hills"));
        let mut out = String::new();
        for kind in BiomeType::VALUES {
            let line = |terrain: &Terrain| -> String {
                SOURCE_CONTINENT
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
                for continent in SOURCE_CONTINENT {
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
        let names: Vec<String> = all.iter().map(|t| t.name().to_string()).collect();
        out += &format!(
            "K {:x} {:x} {:x} {}\n",
            BEACH_SIZE.to_bits(),
            (BEACH + BEACH_SIZE).to_bits(),
            COAST.to_bits(),
            names.join(" ")
        );
        out
    }

    #[test]
    fn biome_overrides_match_6dd607eb_source() {
        assert_eq!(source_oracle_output(), SOURCE_ORACLE);
        // The band stops at 0.505f: the next float up and everything to COAST keep the input.
        let terrains = TerrainRegistry::new();
        let hills = terrains.get_by_name("hills").unwrap();
        for continent in SOURCE_CONTINENT {
            let s = sample(&terrains, BiomeType::Grassland, continent, &hills, 1.0);
            let beach = continent > BEACH && continent <= 0.505;
            assert_eq!(
                biome_override(&s) == BiomeOverride::Beach,
                beach,
                "{continent:?}"
            );
        }
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

    /// 6dd607eb's `getInitialBiome`: one pool by climate type; no pool or an empty pool gives plains, and a noise
    /// Java's `WeightMap.getValue` answers with null (NaN, past the last band, all-zero weights) gives `None`.
    #[test]
    fn initial_biome_takes_the_climate_pool_or_plains() {
        let (a, b, plains) = (Arc::new("a"), Arc::new("b"), Arc::new("plains"));
        let mut pools = BiomePools::default();
        pools.insert(
            BiomeType::Grassland,
            Arc::new(WeightMap::new(
                vec![Arc::clone(&a), Arc::clone(&b)],
                &[1.0, 3.0],
            )),
        );
        pools.insert(BiomeType::Desert, Arc::new(WeightMap::new(Vec::new(), &[])));
        pools.insert(
            BiomeType::Tundra,
            Arc::new(WeightMap::new(vec![Arc::clone(&a)], &[0.0])),
        );
        let terrains = TerrainRegistry::new();
        let mut sample = ClimateSample::new(&terrains);
        let pick = |sample: &ClimateSample| initial_biome(&pools, sample, &plains);
        sample.biome_noise = 0.2;
        assert!(Arc::ptr_eq(&pick(&sample).unwrap(), &a));
        sample.biome_noise = 0.3;
        assert!(Arc::ptr_eq(&pick(&sample).unwrap(), &b));
        sample.biome_noise = 1.0;
        assert!(Arc::ptr_eq(&pick(&sample).unwrap(), &b));
        sample.biome_noise = 1.5;
        assert!(pick(&sample).is_none());
        sample.biome_noise = f32::NAN;
        assert!(pick(&sample).is_none());
        sample.biome_noise = 0.2;
        sample.climate_type = BiomeType::Desert;
        assert!(Arc::ptr_eq(&pick(&sample).unwrap(), &plains));
        sample.climate_type = BiomeType::Taiga;
        assert!(Arc::ptr_eq(&pick(&sample).unwrap(), &plains));
        sample.climate_type = BiomeType::Tundra;
        assert!(pick(&sample).is_none());
    }
}
