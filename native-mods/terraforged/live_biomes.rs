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
    super::{
        biome::BiomeId,
        biome_chunk::{BiomeChunk, BiomeFillError},
    },
    GeneratorData,
    terraforged_biome_registry::{ClimateWeight, RegistryBiomeMap},
    terraforged_biome_sampler::BiomeOverride,
    terraforged_generator_facade::TerraForgedGenerator,
};
use horizon_core::position::ChunkPos;
use std::convert::Infallible;
use std::{fmt, sync::Arc};

const FALLBACK: &str = "minecraft:plains";
const OVERRIDES: [&str; 13] = [
    "minecraft:deep_cold_ocean",
    "minecraft:deep_frozen_ocean",
    "minecraft:deep_lukewarm_ocean",
    "minecraft:deep_ocean",
    "minecraft:cold_ocean",
    "minecraft:frozen_ocean",
    "minecraft:warm_ocean",
    "minecraft:ocean",
    "minecraft:snowy_beach",
    "minecraft:stony_shore",
    "minecraft:beach",
    "minecraft:frozen_river",
    "minecraft:river",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MissingBiome(pub Arc<str>);

impl fmt::Display for MissingBiome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "missing TerraForged biome {}", self.0)
    }
}

impl std::error::Error for MissingBiome {}

pub struct LiveBiomeBindings {
    pub map: RegistryBiomeMap<BiomeId>,
    fallback: Arc<BiomeId>,
    overrides: [Arc<BiomeId>; 13],
}

impl LiveBiomeBindings {
    pub fn from_generator_data(
        data: &GeneratorData,
        configured: &[ClimateWeight],
    ) -> Result<Self, MissingBiome> {
        let map = RegistryBiomeMap::from_generator_data(data, configured);
        Self::resolve(map, |name| data.biome_id(name))
    }

    fn resolve(
        map: RegistryBiomeMap<BiomeId>,
        mut resolve: impl FnMut(&str) -> Option<BiomeId>,
    ) -> Result<Self, MissingBiome> {
        let mut required = |name: &'static str| {
            resolve(name)
                .map(Arc::new)
                .ok_or_else(|| MissingBiome(Arc::from(name)))
        };
        let fallback = required(FALLBACK)?;
        let overrides = [
            required(OVERRIDES[0])?,
            required(OVERRIDES[1])?,
            required(OVERRIDES[2])?,
            required(OVERRIDES[3])?,
            required(OVERRIDES[4])?,
            required(OVERRIDES[5])?,
            required(OVERRIDES[6])?,
            required(OVERRIDES[7])?,
            required(OVERRIDES[8])?,
            required(OVERRIDES[9])?,
            required(OVERRIDES[10])?,
            required(OVERRIDES[11])?,
            required(OVERRIDES[12])?,
        ];
        Ok(Self {
            map,
            fallback,
            overrides,
        })
    }

    pub fn fallback(&self) -> Arc<BiomeId> {
        Arc::clone(&self.fallback)
    }

    pub fn override_biome(&self, replacement: BiomeOverride) -> Arc<BiomeId> {
        let index = match replacement {
            BiomeOverride::Input => return Arc::clone(&self.fallback),
            BiomeOverride::DeepColdOcean => 0,
            BiomeOverride::DeepFrozenOcean => 1,
            BiomeOverride::DeepLukewarmOcean => 2,
            BiomeOverride::DeepOcean => 3,
            BiomeOverride::ColdOcean => 4,
            BiomeOverride::FrozenOcean => 5,
            BiomeOverride::WarmOcean => 6,
            BiomeOverride::Ocean => 7,
            BiomeOverride::SnowyBeach => 8,
            BiomeOverride::StonyShore => 9,
            BiomeOverride::Beach => 10,
            BiomeOverride::FrozenRiver => 11,
            BiomeOverride::River => 12,
        };
        Arc::clone(&self.overrides[index])
    }

    pub fn fill_chunk(
        &self,
        generator: &TerraForgedGenerator,
        chunk: &mut BiomeChunk<BiomeId>,
        position: ChunkPos,
    ) -> Result<(), BiomeFillError<Infallible>> {
        generator.fill_biomes(
            chunk,
            position,
            &self.map.wet,
            self.fallback(),
            &|replacement| self.override_biome(replacement),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_biome_registry::RegistryBiome;
    use super::*;

    fn bindings(missing: Option<&str>) -> Result<LiveBiomeBindings, MissingBiome> {
        let entries = [RegistryBiome::new(
            BiomeId(0),
            FALLBACK,
            0.8,
            0.4,
            true,
            [Arc::from("minecraft:is_overworld")],
        )];
        let map = RegistryBiomeMap::build(&entries, &[]);
        LiveBiomeBindings::resolve(map, |name| {
            if missing == Some(name) {
                return None;
            }
            if name == FALLBACK {
                Some(BiomeId(0))
            } else {
                OVERRIDES
                    .iter()
                    .position(|candidate| *candidate == name)
                    .map(|index| BiomeId(index as u32 + 1))
            }
        })
    }

    #[test]
    fn resolves_every_positional_override_in_java_order() {
        let bindings = bindings(None).unwrap();
        let replacements = [
            BiomeOverride::DeepColdOcean,
            BiomeOverride::DeepFrozenOcean,
            BiomeOverride::DeepLukewarmOcean,
            BiomeOverride::DeepOcean,
            BiomeOverride::ColdOcean,
            BiomeOverride::FrozenOcean,
            BiomeOverride::WarmOcean,
            BiomeOverride::Ocean,
            BiomeOverride::SnowyBeach,
            BiomeOverride::StonyShore,
            BiomeOverride::Beach,
            BiomeOverride::FrozenRiver,
            BiomeOverride::River,
        ];
        for (index, replacement) in replacements.into_iter().enumerate() {
            assert_eq!(
                *bindings.override_biome(replacement),
                BiomeId(index as u32 + 1)
            );
        }
        assert_eq!(*bindings.fallback(), BiomeId(0));
    }

    #[test]
    fn reports_the_first_missing_required_biome() {
        let error = match bindings(Some("minecraft:warm_ocean")) {
            Ok(_) => panic!("missing biome was accepted"),
            Err(error) => error,
        };
        assert_eq!(error, MissingBiome(Arc::from("minecraft:warm_ocean")));
    }
}
