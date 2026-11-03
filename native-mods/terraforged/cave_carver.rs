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
    terraforged::{Module, floor},
    terraforged_cave_biome_sampler::CaveBiomeSampler,
    terraforged_cave_noise::{CaveType, NoiseCave, cave_breach_noise, unique_cave_noise},
    terraforged_terrain_data::TerrainData,
    terraforged_terrain_levels::java_clamp,
};
use crate::heightmap::{HeightmapState, HeightmapType};
use crate::registry::BlockStateId;

pub struct NoiseCaveGenerator<T> {
    caves: Vec<NoiseCave>,
    unique: Module,
    breach: Module,
    biomes: CaveBiomeSampler<T>,
    /// `Source.seed`: the world seed as an int, added to each cave's own seed.
    source_seed: i32,
}

/// `CarverChunk`: the last quart column's cave biome (shared by every cave config, as
/// the original's single cache is) and each config's biomes, in first-seen order. A
/// null biome (an empty pool) is never cached, as Java's `cached == null` test resamples
/// it, and is left out of the lists (Java records it once, and its `NoiseCaveDecorator` then fails on it; Java
/// always registers `terraforged:cave`, which a TerraForged world's pack provides).
pub struct CarverChunk<T> {
    cached: Option<(i32, i32, T)>,
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
        // The cave biome of each of the chunk's quarts the caves reached (16 per quart row).
        let mut writes: Vec<Option<T>> = vec![None; 16 * (chunk.height() >> 2).max(0) as usize];
        // ChunkAccess.getHeight primes a missing OCEAN_FLOOR_WG on the first read, before
        // any write; ProtoChunk.setBlockState then updates the persisted status's maps.
        if chunk.heightmap(HeightmapType::OceanFloorWg).is_none() {
            chunk.prime_heightmaps(&[HeightmapType::OceanFloorWg])?;
        }
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
                // MathUtil.clamp: below `min` first, so a surface under min_y wins.
                let top = java_clamp(y.wrapping_add(cavern), min_y, surface);
                let bottom = java_clamp(y.wrapping_sub(floor), min_y, surface);
                if top - bottom < 2 {
                    continue;
                }
                let biome = self.biome(&mut carver, index, x, z, config);
                if carve {
                    carve_column(chunk, &mut writes, biome, dx, dz, bottom, top, surface, air)?;
                }
            }
        }
        if writes.iter().any(Option::is_some) {
            // The quart origin as BiomeChunk.fill computes it (QuartPos.fromBlock of the minimum block).
            let (base_x, base_z) = (
                position.x.wrapping_shl(4) >> 2,
                position.z.wrapping_shl(4) >> 2,
            );
            let (min, max) = (chunk.min_y(), chunk.min_y() + chunk.height() - 1);
            let old = biomes.clone();
            biomes
                .fill(min, max, |quart| {
                    let slot = quart_slot(
                        quart[0].wrapping_sub(base_x),
                        quart[1] - (min >> 2),
                        quart[2].wrapping_sub(base_z),
                    );
                    Ok::<_, std::convert::Infallible>(match &writes[slot] {
                        Some(biome) => biome.clone(),
                        None => old.noise_biome(quart).clone(),
                    })
                })
                .map_err(|_| TerrainError::InvalidState)?;
        }
        Ok(carver)
    }

    /// Whether a cave may hold `biome`.
    pub fn may_hold(&self, biome: &T) -> bool
    where
        T: PartialEq,
    {
        self.biomes.may_hold(biome)
    }

    /// The cave biome `carve` would write in the column at `x, z` and a height it
    /// writes it at, for `/locate biome`: `first_free` is the column's first free
    /// block over the terrain; the river breach is taken as fully open (the river
    /// noise inland). The last cave to carve there wins, as its writes do.
    pub fn underground_biome_at(
        &self,
        seed: i32,
        x: i32,
        z: i32,
        first_free: i32,
        sea_level: i32,
        min_y: i32,
    ) -> Option<(T, i32)>
    where
        T: Clone,
    {
        let mut surface = first_free.wrapping_sub(2);
        if surface > sea_level || surface < sea_level - 16 {
            surface = surface.wrapping_add(9);
        }
        let surface = surface.wrapping_sub(16);
        let deepest_quart = surface.wrapping_sub(16) >> 2;
        let mut found = None;
        for config in &self.caves {
            let value = match config.cave_type() {
                CaveType::Global => 1.0,
                CaveType::Unique => self
                    .unique
                    .try_value_seed_first(seed, x as f32, z as f32)
                    .ok()?,
            };
            let y = config.height(seed, x, z).ok()?;
            let cavern = config.cavern_size(seed, x, z, value).ok()?;
            if cavern == 0 {
                continue;
            }
            let floor = config.floor_depth(seed, x, z, cavern).ok()?;
            let top = y.wrapping_add(cavern).clamp(min_y, surface.max(min_y));
            let bottom = y.wrapping_sub(floor).clamp(min_y, surface.max(min_y));
            if top - bottom < 2 || (bottom >> 2) >= deepest_quart {
                continue;
            }
            if let Some(biome) = self.biomes.get_underground_biome(
                self.source_seed.wrapping_add(config.seed()),
                x,
                z,
                config.cave_type(),
            ) {
                found = Some((T::clone(biome), bottom.wrapping_add(1)));
            }
        }
        found
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
        // NoiseUtil.floor: -1 below zero even on whole numbers, -1 for NaN.
        Ok(surface.wrapping_sub(floor(16.0 * mask)))
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
            return Some(biome.clone());
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
        carver.cached = biome.clone().map(|biome| (biome_x, biome_z, biome));
        let list = carver.biomes[index].get_or_insert_with(Vec::new);
        if let Some(biome) = &biome
            && !list.contains(biome)
        {
            list.push(biome.clone());
        }
        biome
    }
}

/// The `writes` slot of the quart at chunk-local quart `x`, `y` (from the chunk's bottom) and `z`.
fn quart_slot(x: i32, y: i32, z: i32) -> usize {
    (y * 16 + z * 4 + x) as usize
}

/// `NoiseCaveCarver.carve` for one column: air through `bottom..=top` except fluids, and
/// the cave biome in carved quarts more than 16 blocks below the surface. Without a cave
/// biome (the pool is empty; Java writes null) the quarts keep theirs. Blocks outside the
/// build range are skipped, as ProtoChunk skips them; so are their quarts (Java's section
/// lookup would throw below the range, where only a generator `min_y` under the chunk's
/// reaches).
#[allow(clippy::too_many_arguments)]
fn carve_column<B: TerrainBlockStates, T: Clone>(
    chunk: &mut TerrainChunk<'_, B>,
    writes: &mut [Option<T>],
    biome: Option<T>,
    dx: i32,
    dz: i32,
    bottom: i32,
    top: i32,
    surface: i32,
    air: BlockStateId,
) -> Result<(), TerrainError> {
    let max_biome_y = surface.wrapping_sub(16) >> 2;
    let (min_y, max_y) = (chunk.min_y(), chunk.min_y() + chunk.height());
    for y in bottom..=top {
        if chunk.block_properties(chunk.block(dx, y, dz)).has_fluid() {
            continue;
        }
        chunk.set_block_worldgen(dx, y, dz, air)?;
        if (y >> 2) >= max_biome_y {
            continue;
        }
        if let Some(biome) = &biome
            && (min_y..max_y).contains(&y)
        {
            writes[quart_slot(dx >> 2, (y - min_y) >> 2, dz >> 2)] = Some(biome.clone());
        }
    }
    Ok(())
}

fn sampled<V>(value: Result<V, &'static str>) -> Result<V, TerrainError> {
    value.map_err(|_| TerrainError::InvalidState)
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged_cave_assets::{
            cave_array, current_cave_documents, current_caves, decode_noise_cave,
        },
        terraforged_engine::TerrainRegistry,
        terraforged_terrain_data::{NoiseData, NoiseSample},
        terraforged_terrain_levels::TerrainLevels,
    };
    use super::*;
    use crate::{
        DATA_VERSION, nbt::Tag, registry::BlockRegistry, worldgen::terrain::PreLightChunkStage,
    };
    use horizon_core::position::ChunkPos;
    use std::sync::Arc;

    /// The oracle's palette: air, void air, stone, water, dirt, lava.
    const NAMES: [&str; 6] = [
        "minecraft:air",
        "minecraft:void_air",
        "minecraft:stone",
        "minecraft:water",
        "minecraft:dirt",
        "minecraft:lava",
    ];

    #[derive(Clone, Copy)]
    struct State(u32);

    impl HeightmapState for State {
        fn is_air(self) -> bool {
            self.0 < 2
        }

        fn is_air_block(self) -> bool {
            self.0 == 0
        }

        fn has_fluid(self) -> bool {
            self.0 == 3 || self.0 == 5
        }

        fn blocks_motion_in_heightmap(self) -> bool {
            self.0 == 2 || self.0 == 4
        }

        fn blocks_motion_in_heightmap_no_leaves(self) -> bool {
            self.blocks_motion_in_heightmap()
        }
    }

    struct Properties;

    impl TerrainBlockStates for Properties {
        type State = State;

        fn heightmap_state(&self, id: BlockStateId) -> Self::State {
            State(id.0)
        }
    }

    fn registry() -> BlockRegistry {
        BlockRegistry::from_nbt(&Tag::compound([
            ("DataVersion", Tag::Int(DATA_VERSION)),
            (
                "states",
                Tag::List(
                    NAMES
                        .iter()
                        .map(|name| Tag::compound([("id", Tag::String((*name).into()))]))
                        .collect(),
                ),
            ),
            (
                "defaults",
                Tag::compound(
                    NAMES
                        .iter()
                        .enumerate()
                        .map(|(i, name)| (*name, Tag::Int(i as i32)))
                        .collect::<Vec<_>>(),
                ),
            ),
        ]))
        .unwrap()
    }

    /// The oracle's LCG.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self, bound: i32) -> i32 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 33) % bound as u64) as i32
        }
    }

    fn fold(hash: u64, value: u32) -> u64 {
        (hash ^ u64::from(value)).wrapping_mul(0x100000001b3)
    }

    /// The oracle's biomes (indices into this list), in 26.3 registry order (path, then namespace).
    const BIOMES: [&str; 5] = [
        "terraforged:cave",
        "minecraft:deep_dark",
        "minecraft:desert",
        "minecraft:lush_caves",
        "minecraft:plains",
    ];
    const SEEDS: [i64; 7] = [
        0,
        1,
        1337,
        -4_172_144_997_902_289_642,
        i64::MIN,
        2_147_483_648,
        123_456_789_012_345,
    ];
    const RIVERS: [f32; 6] = [0.0, 0.0, 0.25, 0.5, 1.0, -1.0];

    /// The oracle's `uniqueChunk`: the first chunk of a fixed walk whose centre has unique-cave noise above 0.3.
    fn unique_chunk(unique: &Module, seed: i32) -> ChunkPos {
        for k in 0i32.. {
            let cx = k.wrapping_mul(7919).rem_euclid(4001) - 2000;
            let cz = k.wrapping_mul(104_729).rem_euclid(4001) - 2000;
            let x = (cx * 16 + 8) as f32;
            let z = (cz * 16 + 8) as f32;
            if unique.try_value_seed_first(seed, x, z).unwrap() > 0.3 {
                return ChunkPos::new(cx, cz);
            }
        }
        unreachable!()
    }

    fn lists(carver: &CarverChunk<u8>, caves: usize) -> String {
        (0..caves)
            .map(|i| match carver.biomes(i) {
                None => "-".to_owned(),
                Some(list) => list.iter().map(u8::to_string).collect::<Vec<_>>().join(","),
            })
            .collect::<Vec<_>>()
            .join("/")
    }

    /// `CaveCarverOracle` (`.local/terraforged/oracles/cave_carver/run.sh`): 6dd607eb's `NoiseCaveGenerator.carve`,
    /// `NoiseCaveCarver` and `CarverChunk` over real 26.3 ProtoChunks, heightmaps and biome containers, with the
    /// bundled caves decoded by 6dd607eb's `NoiseCave.CODEC` (some scenarios in reverse registry order). Per scenario: blocks, biomes and heightmaps after
    /// carving, the blocks and quarts changed, each cave config's recorded biomes, and those of a replay
    /// (`getPostCarveChunk`, carve = false) over the carved chunk.
    const JAVA_CARVER: &str = r"p 0 minecraft:air false false false false
p 1 minecraft:void_air false false false false
p 2 minecraft:stone false true true true
p 3 minecraft:water true false true true
p 4 minecraft:dirt false true true true
p 5 minecraft:lava true false true true
0 0x0678a151f7ef0092 0x4f6bdb2873e75125 0x80c6b4f79052cdee 6454 0 104 -/-/-/-//-// -/-/-/-//-//
1 0xd8859a096cf30441 0x268c3fb0971ebe96 0x2cdb13b3d284b39c 9996 127 0 1/4/-/-/-/-/-/0 1/4/-/-/-/-/-/0
2 0x6401bc8729126e65 0xd8cd60721defde5f 0x75edba30609c8a79 114 2 0 -/-/-/-/0/-/-/- -/-/-/-/0/-/-/-
3 0x00460d7346de4501 0x1a24e1f95f661725 0x667837248eba8250 0 0 0 -/-/-/-/-/-/-/- -/-/-/-/-/-/-/-
4 0x94887fcfd586ea99 0x410c62410fdbc585 0x1b0028c38441800f 5535 120 0 -/-/-/0/-/0/-/0 -/-/-/0/-/0/-/0
5 0x11af1a7f46ac476e 0xdce1fd5ca9cb7720 0x668b550b866b1799 7113 112 0 -/-/-/0/-/-/2/3 -/-/-/0/-/-/2/3
6 0x8311b14df9315c57 0x468608cb146cd125 0x46deb2bd8c8ee1d1 1895 0 57 -/-/-/-//-/-/- -/-/-/-//-/-/-
7 0xe0580017c4287cfe 0x2ba640d680e8244f 0xe961032c4b0110bb 1135 34 0 -/-/-/0/-/-/-/0 -/-/-/0/-/-/-/0
8 0x02f6b36f0dbbc653 0xb09a14bc5b14d5a3 0x80024055cc02bb63 6462 133 0 -/-/-/-/0/0/0/0 -/-/-/-/0/0/0/0
9 0xde089e0f9c036662 0x830600591f133134 0xa8b12bb8d3ff27fa 9020 80 0 -/-/-/-/-/-/4/2,1 -/-/-/-/-/-/4/2,1
10 0xc49efd5891e37844 0x236f8285d380084d 0xb572420683a010c8 179 0 0 -/-/-/-/0/-/-/- -/-/-/-/0/-/-/-
11 0x7197884e2ca93fbf 0xa650313e22d39ec6 0x2fceee37e93f2f53 0 0 0 -/-/-/-/-/-/-/- -/-/-/-/-/-/-/-
12 0xa676ec5939af0de2 0x4f6bdb2873e75125 0x5ff933965a4c2d53 2001 0 66 -/-/-/-/-/-// -/-/-/-/-/-//
13 0x31bc4345b85fe75a 0x72825576122ab2ca 0x21b8fb246961070b 1252 2 0 -/4/-/-/-/-/-/- -/4/-/-/-/-/-/-
14 0x71b34ae2e03cff4b 0xdc93cbe2965b6b2c 0xa61420fccb63df43 3044 73 0 -/-/-/-/0/-/0/- -/-/-/-/0/-/0/-
15 0x7443aa097495c4c5 0x51364e708fdc1d25 0x5140531e95fed191 22 0 2 -/-/-/-/-//-/- -/-/-/-/-//-/-
16 0xc1587e32f68afdbe 0x450ac375a992f153 0x862cc18cd1feafc9 3460 70 0 -/-/0/0/-/0/0/- -/-/0/0/-/0/0/-
17 0x5f11182e5568bd39 0x1d139e325591aa67 0x080040f6413d6989 14161 210 0 -/0/0/-/0/-/4/1 -/0/0/-/0/-/4/1
18 0x162f8874fddebcac 0x468608cb146cd125 0xb201c5c02b8ecb07 34 0 0 -/-/-/-/-/-/-/ -/-/-/-/-/-/-/
19 0xeeebd272ffcef48b 0xe5998b234100fa17 0x13076a26db1e067b 1568 18 0 -/-/0/0/-/-/-/- -/-/0/0/-/-/-/-
20 0xb038e711d7e1f801 0x24745d040f8f785f 0x3ed34b9d8a62bba9 3374 25 0 -/-/-/-/0/-/-/- -/-/-/-/0/-/-/-
21 0x919569bdeaa81a92 0x6334f9ba0e408f4b 0xf74e55f030241ea7 4417 56 16 //-/-/-/-/-/3,4 //-/-/-/-/-/3,4
22 0x1f3a7c9aa7a6587f 0x6a56d3776bee9ba5 0x66bdad11d3a74e85 24 4 0 -/-/-/-/-/0/-/- -/-/-/-/-/0/-/-
23 0xd4450e8fb570bedc 0x6814039eb2a33bf0 0x347eb7733231c0d7 532 14 0 -/-/-/-/0/-/-/- -/-/-/-/0/-/-/-
24 0xc0f46764a71ec353 0x4f6bdb2873e75125 0x40767396d6ed3cd6 0 0 0 -/-/-/-/-/-/-/- -/-/-/-/-/-/-/-
25 0x6ff45b51e552c999 0x8f2fc7136c6d41db 0x3ea25dd30784857b 3487 37 0 3/-/-/-/-/-/-/- 3/-/-/-/-/-/-/-
26 0x47fe9cb759e88f0c 0x30f1f36721dfb462 0xbb4195fb87b96ce1 0 0 0 -/-/-/-/-/-/-/- -/-/-/-/-/-/-/-
27 0xe4947841aec9bbe3 0x51364e708fdc1d25 0xea0d6f8833175183 0 0 0 -/-/-/-/-/-/-/- -/-/-/-/-/-/-/-
28 0x8b3ed025b1ce82c6 0x37f6b7e85ef330a9 0xa8aa37323629e4b3 3656 72 0 -/-/-/-/0/-/-/- -/-/-/-/0/-/-/-
29 0xa70944ca7d2b73e4 0x9cc4e94eab7fc402 0xd9caba4467dddc6d 16950 233 0 -/-/0/-/-/-/4/1 -/-/0/-/-/-/4/1
30 0xdff2773284f9874e 0x52c21db0292e9725 0x0c9e5decf5f17602 2458 0 77 -/-/-/-/-///- -/-/-/-/-///-
31 0x9c3f721c0ad84cba 0x8648f121a5140d40 0xac13781c5132253e 753 2 0 -/-/-/-/0/-/-/- -/-/-/-/0/-/-/-
32 0x292779ae81c37607 0x6be5c9652edf6140 0x7054cb46dae6e81d 4264 53 0 -/-/-/-/-/0/0/- -/-/-/-/-/0/0/-
33 0xc37fab1e8d3c5496 0x6334f9ba0e408f4b 0x9ed042e85c0f6a51 4610 56 44 //-/-/-/-/-/3,4 //-/-/-/-/-/3,4
34 0xe0583a2e22dba50c 0x822f461dbb744559 0x11761686b02b1e33 112 0 0 -/-/-/-/-/0/-/- -/-/-/-/-/0/-/-
35 0xe1b69f7a3634374e 0x58ef85919f44e84b 0x4ca98b9924ca8942 840 5 0 -/-/-/-/0/0/-/- -/-/-/-/0/0/-/-
36 0x53877ad900ae5be4 0x945bbea8ccd59725 0x17500faae4f7626e 76 0 7 -/-/-/-//-/-/- -/-/-/-//-/-/-
37 0xb05519e455d48194 0x1e6b5af6cb2325f5 0x6da50498ff28e053 8609 158 0 0/4,3/-/0/0/-/-/0 0/4,3/-/0/0/-/-/0
38 0x9d6f2fc7c1f56007 0x69fd44d0c7f2c1ba 0x205b394bf2bdcb19 1232 40 0 -/-/-/-/-/0/0/- -/-/-/-/-/0/0/-
39 0x67aa6307a5eb64ae 0x3fef1d3d72c01125 0x7fb6f4e3f4893250 2927 0 86 -/-/-/-/-/-//- -/-/-/-/-/-//-
40 0x6a7689944e3408ab 0xce6f8200a50cce19 0x3ec0004224efc0a7 0 0 0 -/-/-/-/-/-/-/- -/-/-/-/-/-/-/-
41 0xfff1a00989bd56e8 0x2f392f1f6ce582dd 0xf466b29174f2d7c5 7535 117 0 -/-/0/-/-/-/4,3/0 -/-/0/-/-/-/4,3/0
42 0x976f912bb34c331d 0xfc4166a823c05d25 0x2432ccf1800d75f9 0 0 0 -/-/-/-/-/-/-/- -/-/-/-/-/-/-/-
43 0x4699507355c9555d 0x180816f2a33f4653 0x91c311132a70dab3 3044 62 0 -/-/-/-/0/-/-/- -/-/-/-/0/-/-/-
44 0x319793fe4500e2b9 0x442bffc4c0583851 0xffbd4a00e6384c2e 160 0 0 -/-/-/-/-/0/-/- -/-/-/-/-/0/-/-
45 0xfe19d767ccbbb7c0 0xaa2d89284d8d9125 0xccf0cfa3e481e6b0 0 0 0 -/-/-/-/-/-/-/- -/-/-/-/-/-/-/-
46 0xabe6f89d2902a557 0xa895ab447bf6bb40 0x99c9fbe418643f6d 1129 33 0 -/-/-/-/0/-/-/- -/-/-/-/0/-/-/-
47 0xa2f0aa9e4fbe71b8 0x147f9859b762d2d3 0xc1a44f101b0eecdb 2353 52 0 -/-/-/0/0/0/0/- -/-/-/0/0/0/0/-";

    #[test]
    fn carving_matches_6dd607eb_java_on_26_3_chunks() {
        let mut lines = JAVA_CARVER.lines();
        for (i, line) in lines.by_ref().take(NAMES.len()).enumerate() {
            let fields: Vec<_> = line.split(' ').collect();
            assert_eq!(fields[..3], ["p", i.to_string().as_str(), NAMES[i]]);
            let state = State(i as u32);
            let flag = |k: usize| fields[k] == "true";
            assert_eq!(flag(3), state.has_fluid(), "{line}");
            assert_eq!(flag(4), state.blocks_motion_in_heightmap(), "{line}");
            assert_eq!(
                flag(5),
                state.blocks_motion_in_heightmap() || state.has_fluid()
            );
            assert_eq!(flag(6), !state.is_air());
        }
        let blocks = registry();
        let terrains = TerrainRegistry::new();
        let caves = current_caves().unwrap();
        // The registry in reverse (as a data pack could order its caves): GLOBAL caves then come before UNIQUE ones,
        // so the shared cache can hold an empty pool's null biome when a UNIQUE cave asks.
        let reversed = cave_array(
            &current_cave_documents()
                .into_iter()
                .rev()
                .map(|(_, json)| decode_noise_cave(json).unwrap())
                .collect::<Vec<_>>(),
        );
        let unique = unique_cave_noise();
        let all: Vec<u8> = (0..BIOMES.len() as u8).collect();
        let ranges = [(-64, 384), (-64, 384), (-64, 384), (0, 256), (-32, 128)];
        let pockets = [3, 5, 0];
        let (mut scenarios, mut carving, mut total, mut nulls) = (0, 0, 0, 0);
        for (s, line) in lines.enumerate() {
            let mut rng = Lcg((s as u64)
                .wrapping_mul(0x9E3779B97F4A7C15)
                .wrapping_add(209));
            let (min_y, height) = ranges[rng.next(ranges.len() as i32) as usize];
            let max_y = min_y + height;
            let gen_min_y = if rng.next(4) == 0 {
                min_y + rng.next(40)
            } else {
                min_y
            };
            let sea = if rng.next(3) == 0 {
                62
            } else {
                min_y + rng.next(height)
            };
            let stage = if rng.next(4) == 0 {
                PreLightChunkStage::Terrain
            } else {
                PreLightChunkStage::Biomes
            };
            let prime = rng.next(4) != 0;
            let seed = SEEDS[rng.next(SEEDS.len() as i32) as usize];
            let cave = s % 3 != 0;
            let (a, b) = (rng.next(9), rng.next(9));
            let mut pos = if s % 10 == 9 {
                ChunkPos::new(1_000_000 + a, -1_875_000 + b)
            } else {
                ChunkPos::new(a - 5, b - 5)
            };
            if s % 4 == 1 {
                pos = unique_chunk(&unique, seed as i32);
            }
            let mut noise = NoiseData::new(&terrains);
            for z in 0..16 {
                for x in 0..16 {
                    let river = RIVERS[rng.next(RIVERS.len() as i32) as usize];
                    let mut sample = NoiseSample::new(&terrains);
                    sample.river_noise = if river < 0.0 {
                        rng.next(1001) as f32 / 1000.0
                    } else {
                        river
                    };
                    noise.set_noise(x, z, &sample);
                }
            }
            let mut terrain = TerrainData::new(TerrainLevels::default());
            terrain.accept(&noise);
            let mut chunk = TerrainChunk::new(pos, min_y, height, &blocks, Properties).unwrap();
            chunk.set_generation_stage(stage);
            let base = min_y + 24 + rng.next((height - 48).max(1));
            let water = rng.next(2) == 0;
            for z in 0..16 {
                for x in 0..16 {
                    let top = base + rng.next(13) - 6;
                    for y in min_y..max_y {
                        let id = if y <= top - 3 {
                            if rng.next(40) == 0 {
                                pockets[rng.next(3) as usize]
                            } else {
                                2
                            }
                        } else if y <= top {
                            4
                        } else if water && y <= sea {
                            3
                        } else {
                            0
                        };
                        if id != 0 {
                            chunk.set_block_raw(x, y, z, BlockStateId(id)).unwrap();
                        }
                    }
                }
            }
            let registered: &[u8] = if cave { &all } else { &all[1..] };
            let (base_x, base_z) = (pos.x.wrapping_shl(4) >> 2, pos.z.wrapping_shl(4) >> 2);
            let mut biomes = BiomeChunk::new(pos, min_y, height, 4u8).unwrap();
            biomes
                .fill(min_y, max_y - 1, |quart| {
                    let (qx, qz) = (quart[0] - base_x, quart[2] - base_z);
                    let qy = quart[1] - (min_y >> 2);
                    let index =
                        (qx * 31 + qy * 17 + qz * 7 + s as i32).rem_euclid(registered.len() as i32);
                    Ok::<_, std::convert::Infallible>(registered[index as usize])
                })
                .unwrap();
            if prime {
                chunk
                    .prime_heightmaps(&[HeightmapType::WorldSurfaceWg, HeightmapType::OceanFloorWg])
                    .unwrap();
            }
            let mut before = Vec::new();
            for z in 0..16 {
                for x in 0..16 {
                    before.extend((min_y..max_y).map(|y| chunk.block(x, y, z)));
                }
            }
            let quart = |si: i32, qy: i32, qz: i32, qx: i32| {
                [base_x + qx, (min_y >> 2) + si * 4 + qy, base_z + qz]
            };
            let mut biomes_before = Vec::new();
            for si in 0..height / 16 {
                for qy in 0..4 {
                    for qz in 0..4 {
                        for qx in 0..4 {
                            biomes_before.push(*biomes.noise_biome(quart(si, qy, qz, qx)));
                        }
                    }
                }
            }
            let pools = || {
                let global = if cave {
                    vec![Arc::new(0u8)]
                } else {
                    Vec::new()
                };
                CaveBiomeSampler::new(
                    800,
                    global,
                    registered.iter().copied().map(Arc::new).collect(),
                )
            };
            let order = if s % 4 == 1 && s % 3 != 1 {
                &reversed
            } else {
                &caves
            };
            let generator = NoiseCaveGenerator::new(order.clone(), pools(), seed);
            let carver = generator
                .carve(
                    seed as i32,
                    &mut chunk,
                    &mut biomes,
                    &terrain,
                    gen_min_y,
                    sea,
                    BlockStateId(0),
                    true,
                )
                .unwrap();
            let replay = NoiseCaveGenerator::new(order.clone(), pools(), seed)
                .carve(
                    seed as i32,
                    &mut chunk,
                    &mut biomes,
                    &terrain,
                    gen_min_y,
                    sea,
                    BlockStateId(0),
                    false,
                )
                .unwrap();

            let (mut block_hash, mut changed, mut index) = (0xcbf29ce484222325u64, 0, 0);
            for z in 0..16 {
                for x in 0..16 {
                    for y in min_y..max_y {
                        let state = chunk.block(x, y, z);
                        changed += i32::from(state != before[index]);
                        index += 1;
                        block_hash = fold(block_hash, state.0);
                    }
                }
            }
            let (mut biome_hash, mut quarts, mut index) = (0xcbf29ce484222325u64, 0, 0);
            for si in 0..height / 16 {
                for qy in 0..4 {
                    for qz in 0..4 {
                        for qx in 0..4 {
                            let biome = *biomes.noise_biome(quart(si, qy, qz, qx));
                            quarts += i32::from(biome != biomes_before[index]);
                            index += 1;
                            biome_hash = fold(biome_hash, u32::from(biome));
                        }
                    }
                }
            }
            let mut maps = 0xcbf29ce484222325u64;
            for kind in HeightmapType::ALL {
                let Some(map) = chunk.heightmap(kind) else {
                    maps = fold(maps, u32::MAX);
                    continue;
                };
                for z in 0..16 {
                    for x in 0..16 {
                        maps = fold(maps, map.first_available(x, z).unwrap() as u32);
                    }
                }
            }
            // Java writes null biomes where the GLOBAL pool is empty (no terraforged:cave registered); the port
            // keeps those quarts' biomes, which the oracle hashes. Only such worlds have them.
            let nulled: i32 = line.split(' ').nth(6).unwrap().parse().unwrap();
            assert_eq!(nulled > 0, !cave && nulled > 0, "scenario {s}");
            nulls += nulled;
            let rust = format!(
                "{s} 0x{block_hash:016x} 0x{biome_hash:016x} 0x{maps:016x} {changed} {quarts} {nulled} {} {}",
                lists(&carver, caves.len()),
                lists(&replay, caves.len()),
            );
            assert_eq!(
                rust, line,
                "scenario {s} ({stage:?}, {min_y}..{max_y}, {pos:?})"
            );
            assert_eq!(chunk.generation_stage(), stage);
            scenarios += 1;
            carving += i32::from(changed > 0);
            total += changed;
        }
        assert_eq!((scenarios, carving, total, nulls), (48, 40, 145_987, 459));
    }

    /// `CarverChunk.getBiome` resamples whenever `cached == null`: a GLOBAL cave whose pool is empty (no
    /// terraforged:cave) leaves nothing behind, so a UNIQUE cave asking at the same quart column gets its own biome,
    /// while a real biome is reused by the next cave at that column (one cache for every config, as in Java).
    #[test]
    fn a_null_cave_biome_is_not_cached() {
        let caves = current_caves().unwrap();
        let global = caves
            .iter()
            .position(|c| c.cave_type() == CaveType::Global)
            .unwrap();
        let unique = caves
            .iter()
            .position(|c| c.cave_type() == CaveType::Unique)
            .unwrap();
        let sampler =
            |global: Vec<Arc<u8>>| CaveBiomeSampler::new(800, global, vec![Arc::new(7u8)]);
        let generator = NoiseCaveGenerator::new(caves.clone(), sampler(Vec::new()), 1337);
        let mut carver = CarverChunk::new(caves.len());
        assert_eq!(
            generator.biome(&mut carver, global, 5, 9, &caves[global]),
            None
        );
        assert_eq!(
            generator.biome(&mut carver, unique, 6, 10, &caves[unique]),
            Some(7)
        );
        assert_eq!(carver.biomes(global), Some(&[][..]));
        assert_eq!(carver.biomes(unique), Some(&[7][..]));

        let generator = NoiseCaveGenerator::new(caves.clone(), sampler(vec![Arc::new(3u8)]), 1337);
        let mut carver = CarverChunk::new(caves.len());
        assert_eq!(
            generator.biome(&mut carver, global, 5, 9, &caves[global]),
            Some(3)
        );
        assert_eq!(
            generator.biome(&mut carver, unique, 6, 10, &caves[unique]),
            Some(3)
        );
        assert_eq!(carver.biomes(unique), None);
        assert_eq!(
            generator.biome(&mut carver, unique, 8, 10, &caves[unique]),
            Some(7)
        );
        assert_eq!(carver.biomes(unique), Some(&[7][..]));
    }
}
