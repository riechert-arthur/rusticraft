/*
 * MIT License
 *
 * Copyright (c) 2021 TerraForged
 * Copyright (c) 2020 TerraForged
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

// `NoiseCaveGenerator.carve`, `NoiseCaveCarver` and `CarverChunk`: TerraForged's noise
// caves, carved into a chunk after its surface, with their biomes written into the
// carved quarts.

use super::{
    super::{
        biome_chunk::BiomeChunk,
        terrain::{TerrainBlockStates, TerrainChunk, TerrainError},
    },
    terraforged::Module,
    terraforged_cave_biome_sampler::CaveBiomeSampler,
    terraforged_cave_noise::{CaveType, NoiseCave, cave_breach_noise, unique_cave_noise},
    terraforged_terrain_data::TerrainData,
};
use crate::heightmap::{HeightmapState, HeightmapType};
use crate::registry::BlockStateId;
use std::collections::HashMap;

pub struct NoiseCaveGenerator<T> {
    caves: Vec<NoiseCave>,
    unique: Module,
    breach: Module,
    biomes: CaveBiomeSampler<T>,
    /// `Source.seed`: the world seed as an int, added to each cave's own seed.
    source_seed: i32,
}

/// `CarverChunk`: the last quart column's cave biome (shared by every cave config, as
/// the original's single cache is) and each config's biomes, in first-seen order.
pub struct CarverChunk<T> {
    cached: Option<(i32, i32, Option<T>)>,
    biomes: Vec<Option<Vec<T>>>,
}

impl<T: Clone + PartialEq> CarverChunk<T> {
    fn new(caves: usize) -> Self {
        Self {
            cached: None,
            biomes: (0..caves).map(|_| None).collect(),
        }
    }

    /// `getBiomes(config)`: the biomes cave `index` passed through, or none.
    pub fn biomes(&self, index: usize) -> Option<&[T]> {
        self.biomes.get(index)?.as_deref()
    }
}

impl<T: Clone + PartialEq> NoiseCaveGenerator<T> {
    pub fn new(caves: Vec<NoiseCave>, biomes: CaveBiomeSampler<T>, world_seed: i64) -> Self {
        Self {
            caves,
            unique: unique_cave_noise(),
            breach: cave_breach_noise(),
            biomes,
            source_seed: world_seed as i32,
        }
    }

    pub fn caves(&self) -> &[NoiseCave] {
        &self.caves
    }

    /// `carve` (or, with `carve` false, `getPostCarveChunk`'s replay that only records
    /// the biomes). Writes go to `chunk` and `biomes`; `min_y` is `Generator.getMinY`.
    #[allow(clippy::too_many_arguments)]
    pub fn carve<B: TerrainBlockStates>(
        &self,
        seed: i32,
        chunk: &mut TerrainChunk<'_, B>,
        biomes: &mut BiomeChunk<T>,
        terrain: &TerrainData,
        min_y: i32,
        sea_level: i32,
        air: BlockStateId,
        carve: bool,
    ) -> Result<CarverChunk<T>, TerrainError> {
        let mut carver = CarverChunk::new(self.caves.len());
        let mut writes = HashMap::new();
        let position = chunk.position();
        let (start_x, start_z) = (position.x.wrapping_shl(4), position.z.wrapping_shl(4));
        for (index, config) in self.caves.iter().enumerate() {
            let modifier = match config.cave_type() {
                CaveType::Global => None,
                CaveType::Unique => Some(&self.unique),
            };
            for i in 0..256 {
                let (dx, dz) = (i & 15, i >> 4);
                let (x, z) = (start_x.wrapping_add(dx), start_z.wrapping_add(dz));
                let surface = self.surface(seed, x, z, dx, dz, chunk, terrain, sea_level)?;
                let y = sampled(config.height(seed, x, z))?;
                let value = match modifier {
                    Some(noise) => sampled(noise.try_value_seed_first(seed, x as f32, z as f32))?,
                    None => 1.0,
                };
                let cavern = sampled(config.cavern_size(seed, x, z, value))?;
                if cavern == 0 {
                    continue;
                }
                let floor = sampled(config.floor_depth(seed, x, z, cavern))?;
                let top = y.wrapping_add(cavern).clamp(min_y, surface.max(min_y));
                let bottom = y.wrapping_sub(floor).clamp(min_y, surface.max(min_y));
                if top - bottom < 2 {
                    continue;
                }
                let biome = self.biome(&mut carver, index, x, z, config);
                if carve {
                    carve_column(chunk, &mut writes, biome, dx, dz, bottom, top, surface, air)?;
                }
            }
        }
        if !writes.is_empty() {
            let old = biomes.clone();
            let (min, max) = (chunk.min_y(), chunk.min_y() + chunk.height() - 1);
            biomes
                .fill(min, max, |quart| {
                    Ok::<_, std::convert::Infallible>(match writes.get(&quart) {
                        Some(biome) => biome.clone(),
                        None => old.noise_biome(quart).clone(),
                    })
                })
                .map_err(|_| TerrainError::InvalidState)?;
        }
        Ok(carver)
    }

    /// `NoiseCaveCarver.getSurface`.
    #[allow(clippy::too_many_arguments)]
    fn surface<B: TerrainBlockStates>(
        &self,
        seed: i32,
        x: i32,
        z: i32,
        dx: i32,
        dz: i32,
        chunk: &TerrainChunk<'_, B>,
        terrain: &TerrainData,
        sea_level: i32,
    ) -> Result<i32, TerrainError> {
        // CarverChunk.getCarvingMask: rivers keep their banks closed.
        let noise = sampled(self.breach.try_value_seed_first(seed, x as f32, z as f32))?;
        let mask = 1.0 - noise * terrain.river().get(x, z);
        // ChunkAccess.getHeight is the first free block minus one.
        let first_free = chunk
            .heightmap(HeightmapType::OceanFloorWg)
            .ok_or(TerrainError::InvalidState)?
            .first_available(dx, dz)?;
        let mut surface = first_free.wrapping_sub(2);
        if surface > sea_level || surface < sea_level - 16 {
            surface = surface.wrapping_add(9);
        }
        Ok(surface.wrapping_sub((16.0 * mask).floor() as i32))
    }

    /// `CarverChunk.getBiome`: one cave-biome sample per quart column, whichever config
    /// asks; a fresh sample joins the asking config's list.
    fn biome(
        &self,
        carver: &mut CarverChunk<T>,
        index: usize,
        x: i32,
        z: i32,
        config: &NoiseCave,
    ) -> Option<T> {
        let (biome_x, biome_z) = (x >> 2, z >> 2);
        if let Some((cached_x, cached_z, biome)) = &carver.cached
            && (*cached_x, *cached_z) == (biome_x, biome_z)
        {
            return biome.clone();
        }
        let biome = self
            .biomes
            .get_underground_biome(
                self.source_seed.wrapping_add(config.seed()),
                x,
                z,
                config.cave_type(),
            )
            .map(|biome| T::clone(biome));
        carver.cached = Some((biome_x, biome_z, biome.clone()));
        let list = carver.biomes[index].get_or_insert_with(Vec::new);
        if let Some(biome) = &biome
            && !list.contains(biome)
        {
            list.push(biome.clone());
        }
        biome
    }
}

/// `NoiseCaveCarver.carve` for one column: air through `bottom..=top` except fluids, and
/// the cave biome in carved quarts more than 16 blocks below the surface. Without a cave
/// biome (the pool is empty) the quarts keep theirs.
#[allow(clippy::too_many_arguments)]
fn carve_column<B: TerrainBlockStates, T: Clone>(
    chunk: &mut TerrainChunk<'_, B>,
    writes: &mut HashMap<[i32; 3], T>,
    biome: Option<T>,
    dx: i32,
    dz: i32,
    bottom: i32,
    top: i32,
    surface: i32,
    air: BlockStateId,
) -> Result<(), TerrainError> {
    let position = chunk.position();
    let max_biome_y = surface.wrapping_sub(16) >> 2;
    for y in bottom..=top {
        if chunk.block_properties(chunk.block(dx, y, dz)).has_fluid() {
            continue;
        }
        chunk.set_block_worldgen(dx, y, dz, air)?;
        if (y >> 2) >= max_biome_y {
            continue;
        }
        if let Some(biome) = &biome {
            let quart = [
                position.x.wrapping_shl(2).wrapping_add(dx >> 2),
                y >> 2,
                position.z.wrapping_shl(2).wrapping_add(dz >> 2),
            ];
            writes.insert(quart, biome.clone());
        }
    }
    Ok(())
}

fn sampled<V>(value: Result<V, &'static str>) -> Result<V, TerrainError> {
    value.map_err(|_| TerrainError::InvalidState)
}
