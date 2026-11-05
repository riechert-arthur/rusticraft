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

// Adapted from TerraForged Reimagined (https://github.com/Romoslayer/TerraForged-Reimagined, a24d450):
// `src/main/java/com/terraforged/mod/worldgen/util/ChunkUtil.java` (`fillChunk`, `fillSection`, `getFiller`,
// `getLowestSection`, `getHighestSection`, `primeHeightmaps`), as `Generator.fillFromNoise` calls them. The fill
// logic is the same in TerraForged 6dd607eb and the 1.18.2-0.3.1 jar; Reimagined's is the 26.3 form
// (`getMinY`, `getMaxY() + 1`). MIT License, Copyright (c) 2020 TerraForged, Copyright (c) 2026 Romoslayer; see
// `LICENSE-TERRAFORGED-REIMAGINED`.

use super::{
    super::terrain::{TerrainBlockStates, TerrainChunk, TerrainError},
    terraforged_terrain_data::TerrainData,
    terraforged_terrain_levels::TerrainLevels,
};
use crate::{heightmap::HeightmapType, registry::BlockStateId};

/// `ChunkUtil.fillChunk(seaLevel, chunk, terrainData, FILLER, resource)` then
/// `ChunkUtil.primeHeightmaps(seaLevel, chunk, terrainData, FILLER)`, as `Generator.fillFromNoise` runs them.
///
/// Inside the chunk's build range every block is what Java writes: whole stone sections below the lowest
/// section (`getMin`'s, rounded down to 16), then, in the sections up to the highest one (`max(getMaxBase,
/// getMax)`'s, rounded down to 16), stone up to the column's height and water above it up to the column's water
/// level ([`TerrainLevels::water_level`]: the base height in a still river or lake, else `sea_level`). Water
/// above the highest section's top is not placed (Java's section loop stops there), and the heightmaps follow
/// Java's `primeHeightmaps`: OCEAN_FLOOR_WG from the height, WORLD_SURFACE_WG from `max(sea_level, height)`,
/// ignoring river water levels.
///
/// The build range clips the blocks (Java throws an `ArrayIndexOutOfBoundsException` instead, reading a
/// section outside the chunk): the result is the window of the fill Java makes in a chunk tall enough to hold
/// the terrain. The heightmaps take Java's heights unchanged.
pub fn fill_chunk<B: TerrainBlockStates>(
    sea_level: i32,
    chunk: &mut TerrainChunk<'_, B>,
    terrain: &TerrainData,
    stone: BlockStateId,
    water: BlockStateId,
) -> Result<(), TerrainError> {
    fill_columns(
        sea_level,
        chunk,
        lowest_section(terrain.min()),
        highest_section(terrain.max_base().max(terrain.max())),
        |x, z| {
            (
                terrain.height_at(x, z),
                TerrainLevels::water_level(x, z, sea_level, terrain),
            )
        },
        stone,
        water,
    )
}

/// `getLowestSection`: `(y >> 4) << 4`.
const fn lowest_section(min: i32) -> i32 {
    (min >> 4) << 4
}

/// `getHighestSection`: `(y >> 4) << 4`.
const fn highest_section(max: i32) -> i32 {
    (max >> 4) << 4
}

/// The fill and the heightmaps from each column's `(height, water level)` and the chunk's section bounds
/// (`lowest`/`highest`: section starts, multiples of 16). Coordinates are local (0..16).
fn fill_columns<B: TerrainBlockStates>(
    sea_level: i32,
    chunk: &mut TerrainChunk<'_, B>,
    lowest: i32,
    highest: i32,
    column: impl Fn(i32, i32) -> (i32, i32),
    stone: BlockStateId,
    water: BlockStateId,
) -> Result<(), TerrainError> {
    // i64 so that heights at the int extremes neither wrap nor overflow.
    let min_y = i64::from(chunk.min_y());
    let top = min_y + i64::from(chunk.height()) - 1;
    let lowest = i64::from(lowest);
    let section_top = i64::from(highest) + 15;
    for z in 0..16 {
        for x in 0..16 {
            let (solid_y, water_y) = column(x, z);
            let solid_y = i64::from(solid_y);
            // Stone: every block below `lowest` (whole sections), then up to the height inside the sections.
            let stone_top = (lowest - 1).max(solid_y.min(section_top)).min(top);
            if stone_top >= min_y {
                chunk.fill_solid_run(
                    x,
                    stone_top as i32,
                    z,
                    (stone_top - min_y + 1) as i32,
                    stone,
                )?;
            }
            // Water: above the height, from `lowest`, up to the water level inside the sections.
            let water_bottom = (solid_y + 1).max(lowest).max(min_y);
            let water_top = i64::from(water_y).min(section_top).min(top);
            if water_top >= water_bottom {
                chunk.fill_solid_run(
                    x,
                    water_top as i32,
                    z,
                    (water_top - water_bottom + 1) as i32,
                    water,
                )?;
            }
        }
    }
    chunk.ensure_heightmap(HeightmapType::OceanFloorWg);
    chunk.ensure_heightmap(HeightmapType::WorldSurfaceWg);
    for z in 0..16 {
        for x in 0..16 {
            let (floor, _) = column(x, z);
            let surface = sea_level.max(floor);
            // `getFiller(surface, floor)`.
            let surface_state = if surface <= floor { stone } else { water };
            chunk.update_heightmap(HeightmapType::OceanFloorWg, x, floor, z, stone)?;
            chunk.update_heightmap(HeightmapType::WorldSurfaceWg, x, surface, z, surface_state)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged_engine::TerrainRegistry,
        terraforged_terrain_data::{NoiseData, NoiseSample},
    };
    use super::*;
    use crate::{DATA_VERSION, heightmap::HeightmapState, nbt::Tag, registry::BlockRegistry};
    use horizon_core::position::ChunkPos;

    const AIR: BlockStateId = BlockStateId(0);
    const STONE: BlockStateId = BlockStateId(2);
    const WATER: BlockStateId = BlockStateId(3);

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
            self.0 == 3
        }

        fn blocks_motion_in_heightmap(self) -> bool {
            self.0 == 2
        }

        fn blocks_motion_in_heightmap_no_leaves(self) -> bool {
            self.0 == 2
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
                Tag::List(vec![
                    Tag::compound([("id", Tag::String("minecraft:air".into()))]),
                    Tag::compound([("id", Tag::String("minecraft:void_air".into()))]),
                    Tag::compound([("id", Tag::String("minecraft:stone".into()))]),
                    Tag::compound([("id", Tag::String("minecraft:water".into()))]),
                ]),
            ),
            (
                "defaults",
                Tag::compound([
                    ("minecraft:air", Tag::Int(0)),
                    ("minecraft:void_air", Tag::Int(1)),
                    ("minecraft:stone", Tag::Int(2)),
                    ("minecraft:water", Tag::Int(3)),
                ]),
            ),
        ]))
        .unwrap()
    }

    fn chunk(registry: &BlockRegistry, min_y: i32, height: i32) -> TerrainChunk<'_, Properties> {
        TerrainChunk::new(ChunkPos::new(0, 0), min_y, height, registry, Properties).unwrap()
    }

    fn terrain(registry: &TerrainRegistry, height_noise: f32) -> TerrainData {
        let hills = registry.get_by_name("hills").unwrap();
        let river = registry.get_by_name("river").unwrap();
        let mut noise = NoiseData::new(registry);
        for z in NoiseData::MIN..NoiseData::MAX {
            for x in NoiseData::MIN..NoiseData::MAX {
                let at_river = (x, z) == (0, 0);
                let mut sample = NoiseSample::new(registry);
                sample.height_noise = height_noise;
                sample.base_noise = if at_river { 0.5 } else { 0.25 };
                sample.river_noise = if at_river { 0.0 } else { 1.0 };
                sample.terrain_type = if at_river {
                    river.clone()
                } else {
                    hills.clone()
                };
                noise.set_noise(x, z, &sample);
            }
        }
        let mut terrain = TerrainData::new(TerrainLevels::default());
        terrain.accept(&noise);
        terrain
    }

    fn first_available(
        chunk: &TerrainChunk<'_, Properties>,
        kind: HeightmapType,
        x: i32,
        z: i32,
    ) -> i32 {
        chunk
            .heightmap(kind)
            .unwrap()
            .first_available(x, z)
            .unwrap()
    }

    #[test]
    fn fill_chunk_matches_terraforged_stone_water_and_heightmaps() {
        let blocks = registry();
        let terrains = TerrainRegistry::new();
        let terrain = terrain(&terrains, 0.2);
        let mut chunk = chunk(&blocks, -64, 544);
        fill_chunk(62, &mut chunk, &terrain, STONE, WATER).unwrap();

        assert_eq!(TerrainLevels::water_level(0, 0, 62, &terrain), 125);
        assert_eq!(TerrainLevels::water_level(1, 0, 62, &terrain), 62);
        assert_eq!(chunk.block(0, -64, 0), STONE);
        assert_eq!(chunk.block(0, 96, 0), STONE);
        assert_eq!(chunk.block(0, 97, 0), WATER);
        assert_eq!(chunk.block(0, 125, 0), WATER);
        assert_eq!(chunk.block(0, 126, 0), AIR);
        assert_eq!(chunk.block(1, 96, 0), STONE);
        assert_eq!(chunk.block(1, 97, 0), AIR);

        // primeHeightmaps ignores the river's water level: the surface map is max(sea level, height) + 1.
        for kind in [HeightmapType::OceanFloorWg, HeightmapType::WorldSurfaceWg] {
            assert_eq!(first_available(&chunk, kind, 0, 0), 97);
            assert_eq!(first_available(&chunk, kind, 1, 0), 97);
        }
    }

    // ChunkFillOracle.java's case generator and digest (`.local/terraforged/oracles/chunk_fill/`).
    struct Lcg(i64);

    impl Lcg {
        fn rnd(&mut self, lo: i32, hi: i32) -> i32 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            lo + (((self.0 as u64) >> 33) as i32).rem_euclid(hi - lo + 1)
        }
    }

    /// minY, height, sea, solid lo/hi, base lo/hi, river one in N (0 = none), NaN (min = max = -1).
    const CASES: [[i32; 9]; 16] = [
        [-64, 384, 62, 40, 120, 30, 70, 4, 0],
        [-64, 384, 63, 10, 50, 0, 60, 3, 0],
        [-64, 384, 70, 0, 40, 0, 40, 0, 0],
        [-64, 384, 62, 250, 480, 60, 300, 5, 0],
        [0, 256, 62, -100, 40, -50, 30, 4, 0],
        [-64, 1024, 62, 0, 480, 30, 400, 4, 0],
        [-2032, 4064, 62, -2000, 2000, -1500, 1900, 6, 0],
        [-64, 384, 62, -60, 100, -60, 80, 4, 1],
        [-64, 384, -100, -40, 30, -60, 20, 3, 0],
        [64, 16, 62, 50, 90, 55, 85, 2, 0],
        [-64, 384, 400, 0, 100, 0, 100, 5, 0],
        [-64, 384, 62, 319, 319, 319, 319, 0, 0],
        [-64, 384, 62, 320, 320, 0, 0, 0, 0],
        [-64, 384, 62, -65, -65, -65, -65, 0, 0],
        [-64, 384, 62, -64, -64, -64, -64, 2, 0],
        [-64, 384, 62, 300, 360, 0, 10, 0, 1],
    ];

    /// `ChunkFillOracle` output: the digest of Java's fill in a -4096..4096 chunk, read through this case's
    /// build range (heightmaps clamped to it). Cases 3, 4, 9, 12 and 13 throw in Java's own chunk (the
    /// terrain leaves the build range); every other case's own chunk gives the same digest.
    const JAVA: [u64; 16] = [
        0x7700_d65c_5141_1374,
        0xd782_5b25_c7d4_19e7,
        0x42a7_99a0_18b9_afdb,
        0x64a5_4588_42a4_fa65,
        0x9ad1_a6c1_d4d1_939f,
        0x3c65_8f65_be45_694d,
        0xd0ee_7dbb_93a5_3c74,
        0xcd53_8a3b_235d_a72f,
        0x1f48_da25_6f44_596b,
        0xd0c6_8fb1_9c1b_fa11,
        0xcfb5_5321_ebb7_08fb,
        0xc72b_dfcc_f49d_1325,
        0xc72b_dfcc_f49d_1325,
        0x1f12_388d_516d_3325,
        0x1561_a053_8b50_f17f,
        0xd850_c8c5_b444_68b5,
    ];

    fn fnv(hash: u64, byte: u8) -> u64 {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    }

    #[test]
    fn fill_matches_java_chunk_util() {
        let blocks = registry();
        for (index, case) in CASES.iter().enumerate() {
            if [3, 4, 9, 12, 13].contains(&index) {
                continue; // Java throws in the case's own chunk
            }
            let [min_y, height, sea, lo, hi, base_lo, base_hi, river, nan] = *case;
            let mut lcg = Lcg(0x9E37_79B9_7F4A_7C15_u64 as i64 ^ index as i64);
            let (mut solid, mut water) = ([0; 256], [0; 256]);
            let (mut min, mut max, mut max_base) = (i32::MAX, i32::MIN, i32::MIN);
            for i in 0..256 {
                solid[i] = lcg.rnd(lo, hi);
                let base = lcg.rnd(base_lo, base_hi);
                let is_river = river != 0 && lcg.rnd(0, river - 1) == 0;
                water[i] = if is_river { base } else { sea };
                min = min.min(solid[i]);
                max = max.max(solid[i]);
                max_base = max_base.max(base);
            }
            if nan != 0 {
                (min, max) = (-1, -1);
            }
            let mut chunk = chunk(&blocks, min_y, height);
            fill_columns(
                sea,
                &mut chunk,
                lowest_section(min),
                highest_section(max_base.max(max)),
                |x, z| (solid[(z * 16 + x) as usize], water[(z * 16 + x) as usize]),
                STONE,
                WATER,
            )
            .unwrap();
            let mut hash = 0xcbf2_9ce4_8422_2325;
            for y in min_y..min_y + height {
                for z in 0..16 {
                    for x in 0..16 {
                        hash = fnv(
                            hash,
                            match chunk.block(x, y, z) {
                                AIR => 0,
                                STONE => 1,
                                WATER => 2,
                                other => panic!("unexpected state {other:?}"),
                            },
                        );
                    }
                }
            }
            for kind in [HeightmapType::OceanFloorWg, HeightmapType::WorldSurfaceWg] {
                for z in 0..16 {
                    for x in 0..16 {
                        // Clamped to the build range, as the oracle's digest.
                        let y = first_available(&chunk, kind, x, z).clamp(min_y, min_y + height);
                        for byte in y.to_le_bytes() {
                            hash = fnv(hash, byte);
                        }
                    }
                }
            }
            assert_eq!(hash, JAVA[index], "case {index}");
        }
    }
}
