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
    super::terrain::{TerrainBlockStates, TerrainChunk, TerrainError},
    terraforged_terrain_data::TerrainData,
    terraforged_terrain_levels::TerrainLevels,
};
use crate::{heightmap::HeightmapType, registry::BlockStateId};

pub fn fill_chunk<B: TerrainBlockStates>(
    sea_level: i32,
    chunk: &mut TerrainChunk<'_, B>,
    terrain: &TerrainData,
    stone: BlockStateId,
    water: BlockStateId,
) -> Result<(), TerrainError> {
    let min_y = chunk.min_y();
    let max_y = min_y.wrapping_add(chunk.height());
    chunk.ensure_heightmap(HeightmapType::OceanFloorWg);
    chunk.ensure_heightmap(HeightmapType::WorldSurfaceWg);
    for z in 0..16 {
        for x in 0..16 {
            let solid_y = terrain.height_at(x, z);
            let water_y = TerrainLevels::water_level(sea_level, x, z, terrain);
            let solid_top = solid_y.min(max_y.wrapping_sub(1));
            if solid_top >= min_y {
                chunk.fill_solid_run(
                    x,
                    solid_top,
                    z,
                    solid_top.wrapping_sub(min_y).wrapping_add(1),
                    stone,
                )?;
            }
            let water_bottom = solid_y.wrapping_add(1).max(min_y);
            let water_top = water_y.min(max_y.wrapping_sub(1));
            if water_top >= water_bottom {
                chunk.fill_solid_run(
                    x,
                    water_top,
                    z,
                    water_top.wrapping_sub(water_bottom).wrapping_add(1),
                    water,
                )?;
            }
            chunk.update_heightmap(HeightmapType::OceanFloorWg, x, solid_y, z, stone)?;
            let surface_y = sea_level.max(solid_y);
            let surface_state = if surface_y <= solid_y { stone } else { water };
            chunk.update_heightmap(
                HeightmapType::WorldSurfaceWg,
                x,
                surface_y,
                z,
                surface_state,
            )?;
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

    fn terrain() -> TerrainData {
        let registry = TerrainRegistry::new();
        let hills = registry.get_by_name("hills").unwrap();
        let river = registry.get_by_name("river").unwrap();
        let mut noise = NoiseData::new();
        for z in NoiseData::MIN..NoiseData::MAX {
            for x in NoiseData::MIN..NoiseData::MAX {
                noise.set_noise(
                    x,
                    z,
                    &NoiseSample {
                        height_noise: 0.2,
                        base_noise: if (x, z) == (0, 0) { 0.5 } else { 0.25 },
                        river_noise: if (x, z) == (0, 0) { 0.0 } else { 1.0 },
                        terrain_type: if (x, z) == (0, 0) {
                            river.clone()
                        } else {
                            hills.clone()
                        },
                        ..NoiseSample::default()
                    },
                );
            }
        }
        let mut terrain = TerrainData::new(TerrainLevels::default());
        terrain.accept(&noise);
        terrain
    }

    #[test]
    fn fill_chunk_matches_terraforged_stone_water_and_heightmaps() {
        let registry = registry();
        let terrain = terrain();
        let mut chunk =
            TerrainChunk::new(ChunkPos::new(0, 0), -64, 544, &registry, Properties).unwrap();
        fill_chunk(62, &mut chunk, &terrain, BlockStateId(2), BlockStateId(3)).unwrap();

        assert_eq!(TerrainLevels::water_level(62, 0, 0, &terrain), 125);
        assert_eq!(TerrainLevels::water_level(62, 1, 0, &terrain), 62);
        assert_eq!(chunk.block(0, 96, 0), BlockStateId(2));
        assert_eq!(chunk.block(0, 97, 0), BlockStateId(3));
        assert_eq!(chunk.block(0, 125, 0), BlockStateId(3));
        assert_eq!(chunk.block(0, 126, 0), BlockStateId(0));
        assert_eq!(chunk.block(1, 96, 0), BlockStateId(2));
        assert_eq!(chunk.block(1, 97, 0), BlockStateId(0));

        let ocean = chunk.heightmap(HeightmapType::OceanFloorWg).unwrap();
        let surface = chunk.heightmap(HeightmapType::WorldSurfaceWg).unwrap();
        assert_eq!(ocean.first_available(0, 0).unwrap(), 97);
        assert_eq!(surface.first_available(0, 0).unwrap(), 97);
        assert_eq!(ocean.first_available(1, 0).unwrap(), 97);
        assert_eq!(surface.first_available(1, 0).unwrap(), 97);
    }
}
