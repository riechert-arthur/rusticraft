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
            matches!(self.0, 3 | 7 | 8)
        }

        fn blocks_motion_in_heightmap(self) -> bool {
            self.0 >= 2 && !self.has_fluid()
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
                Tag::List(vec![
                    Tag::compound([("id", Tag::String("minecraft:air".into()))]),
                    Tag::compound([("id", Tag::String("minecraft:void_air".into()))]),
                    Tag::compound([("id", Tag::String("minecraft:stone".into()))]),
                    Tag::compound([("id", Tag::String("minecraft:water".into()))]),
                    Tag::compound([("id", Tag::String("minecraft:dirt".into()))]),
                    Tag::compound([("id", Tag::String("minecraft:snow_block".into()))]),
                    Tag::compound([("id", Tag::String("minecraft:gravel".into()))]),
                    Tag::compound([
                        ("id", Tag::String("minecraft:water".into())),
                        (
                            "properties",
                            Tag::compound([("level", Tag::String("2".into()))]),
                        ),
                    ]),
                    Tag::compound([
                        ("id", Tag::String("minecraft:water".into())),
                        (
                            "properties",
                            Tag::compound([("level", Tag::String("1".into()))]),
                        ),
                    ]),
                ]),
            ),
            (
                "defaults",
                Tag::compound([
                    ("minecraft:air", Tag::Int(0)),
                    ("minecraft:void_air", Tag::Int(1)),
                    ("minecraft:stone", Tag::Int(2)),
                    ("minecraft:water", Tag::Int(3)),
                    ("minecraft:dirt", Tag::Int(4)),
                    ("minecraft:snow_block", Tag::Int(5)),
                    ("minecraft:gravel", Tag::Int(6)),
                ]),
            ),
        ]))
        .unwrap()
    }

    fn terrain() -> TerrainData {
        terrain_with_slope(0.0)
    }

    fn terrain_with_slope(slope: f32) -> TerrainData {
        terrain_with_river(slope, "river", 0.0)
    }

    fn terrain_with_river(slope: f32, kind: &str, river_noise: f32) -> TerrainData {
        let registry = TerrainRegistry::new();
        let hills = registry.get_by_name("hills").unwrap();
        let river = registry.get_by_name(kind).unwrap();
        let mut noise = NoiseData::new();
        for z in NoiseData::MIN..NoiseData::MAX {
            for x in NoiseData::MIN..NoiseData::MAX {
                noise.set_noise(
                    x,
                    z,
                    &NoiseSample {
                        height_noise: 0.2 + x as f32 * slope,
                        base_noise: if (x, z) == (0, 0) { 0.5 } else { 0.25 },
                        river_noise: if (x, z) == (0, 0) { river_noise } else { 1.0 },
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
    fn water_smoothing_matches_original_java_region_reads() {
        use super::super::terraforged_surface;

        let cases = [
            ("river", 0.0, 0, -6, 3, 7, 1, 0x202cf52a763010c9),
            ("river", 0.0, 6, 0, 3, 7, 56, 0x6b81b6b91ed91ac5),
            ("river", 0.0, 6, 1, 3, 3, 78, 0x6c4929d8df816aa1),
            ("river", 0.0, 99, 99, 3, 3, 78, 0x6c4929d8df816aa1),
            ("lake", 0.0, 0, -6, 3, 7, 1, 0x202cf52a763010c9),
            ("hills", 0.0, 0, -6, 3, 3, 0, 0xcbf29ce484222325),
            ("river", 0.001, 0, -6, 3, 3, 0, 0xcbf29ce484222325),
            ("river", -0.0, 0, -6, 3, 7, 1, 0x202cf52a763010c9),
            ("river", f32::NAN, 0, -6, 3, 3, 0, 0xcbf29ce484222325),
            ("river", 0.0, 0, -6, 8, 8, 0, 0xcbf29ce484222325),
        ];
        for (kind, river, air_x, air_z, initial, expected, reads, expected_hash) in cases {
            let registry = registry();
            let terrain = terrain_with_river(0.0, kind, river);
            let mut chunk =
                TerrainChunk::new(ChunkPos::new(-1, 2), -64, 192, &registry, Properties).unwrap();
            for z in 0..16 {
                for x in 0..16 {
                    let state = if (x, z) == (0, 0) {
                        initial
                    } else if (x, z) == (air_x, air_z) {
                        0
                    } else {
                        2
                    };
                    chunk.set_block_raw(x, 60, z, BlockStateId(state)).unwrap();
                }
            }
            let mut calls = 0;
            let mut hash = 0xcbf29ce484222325u64;
            terraforged_surface::smooth_water(
                &terrain,
                &mut chunk,
                BlockStateId(3),
                BlockStateId(7),
                |state| State(state.0).is_air(),
                |x, y, z| {
                    calls += 1;
                    for value in [x, y, z] {
                        hash = (hash ^ u64::from(value as u32)).wrapping_mul(0x100000001b3);
                    }
                    Ok(BlockStateId(if (x, z) == (-16 + air_x, 32 + air_z) {
                        0
                    } else {
                        2
                    }))
                },
            )
            .unwrap();
            assert_eq!(
                (chunk.block(0, 60, 0).0, calls, hash),
                (expected, reads, expected_hash)
            );
            assert_eq!(
                chunk
                    .heightmap(HeightmapType::MotionBlockingNoLeaves)
                    .unwrap()
                    .highest_taken(0, 0)
                    .unwrap(),
                60
            );
        }
    }

    #[test]
    fn water_smoothing_propagates_neighbor_failure_without_replacing_source() {
        use super::super::terraforged_surface;

        let registry = registry();
        let mut chunk =
            TerrainChunk::new(ChunkPos::new(-1, 2), -64, 192, &registry, Properties).unwrap();
        chunk.set_block_raw(0, 60, 0, BlockStateId(3)).unwrap();
        let result = terraforged_surface::smooth_water(
            &terrain(),
            &mut chunk,
            BlockStateId(3),
            BlockStateId(7),
            |state| State(state.0).is_air(),
            |_, _, _| Err(TerrainError::InvalidVolume),
        );
        assert!(matches!(result, Err(TerrainError::InvalidVolume)));
        assert_eq!(chunk.block(0, 60, 0), BlockStateId(3));
    }

    #[test]
    fn surface_erosion_matches_original_java_column_hashes() {
        use super::super::super::terrain::PreLightChunkStage;
        use super::super::terraforged_surface;

        let cases = [
            (80, 77, 6, 4, 62, true, 0x0fb8dce8e2487b97),
            (80, 77, 0, 4, 62, true, 0x7a0f35d114bb7957),
            (80, 77, 3, 5, 62, true, 0xddf2ffc6026800c3),
            (80, 61, 6, 4, 62, true, 0x68f69ba86fa10657),
            (80, 60, 6, 4, 62, true, 0xc741cb65d4d3a2e3),
            (1, 0, 6, 4, 0, true, 0x787bb1ac25aaa70f),
            (-10, -12, 6, 4, -64, true, 0x79f6f6c52385b53b),
            (61, 58, 6, 4, 62, true, 0x4e66875d7fb9463f),
            (62, 59, 6, 4, 62, true, 0x45b4c2eeb855c96f),
            (80, 80, 6, 4, 62, true, 0x67211f99f7082963),
            (80, 77, 6, 4, 62, false, 0x16a500356406365d),
        ];
        for stage in [PreLightChunkStage::Empty, PreLightChunkStage::Terrain] {
            for (top, bottom, base, cover, sea, steep, expected) in cases {
                let registry = registry();
                let terrain = terrain_with_slope(if steep { 0.01 } else { 0.005 });
                let mut chunk =
                    TerrainChunk::new(ChunkPos::new(-1, 2), -64, 192, &registry, Properties)
                        .unwrap();
                chunk.set_generation_stage(stage);
                for y in -64..=top {
                    let state = if y < bottom {
                        2
                    } else if y == bottom {
                        base
                    } else {
                        cover
                    };
                    chunk.set_block_raw(0, y, 0, BlockStateId(state)).unwrap();
                }
                chunk
                    .prime_heightmaps(&[HeightmapType::WorldSurfaceWg])
                    .unwrap();
                let original_height = terrain.height_at(0, 0);
                terraforged_surface::apply(&terrain, &mut chunk, 255, sea, |state| {
                    matches!(state.0, 4 | 5)
                })
                .unwrap();
                let hash = (-64..128).fold(0xcbf29ce484222325u64, |hash, y| {
                    (hash ^ u64::from(chunk.block(0, y, 0).0)).wrapping_mul(0x100000001b3)
                });
                assert_eq!(
                    hash, expected,
                    "top={top} bottom={bottom} base={base} stage={stage:?}"
                );
                let ocean = (-64..128)
                    .rev()
                    .find(|&y| State(chunk.block(0, y, 0).0).blocks_motion_in_heightmap())
                    .unwrap_or(-65);
                assert_eq!(
                    chunk
                        .heightmap(HeightmapType::OceanFloorWg)
                        .unwrap()
                        .highest_taken(0, 0)
                        .unwrap(),
                    ocean
                );
                let surface = (-64..128)
                    .rev()
                    .find(|&y| !State(chunk.block(0, y, 0).0).is_air())
                    .unwrap_or(-65);
                assert_eq!(
                    chunk
                        .heightmap(HeightmapType::WorldSurfaceWg)
                        .unwrap()
                        .highest_taken(0, 0)
                        .unwrap(),
                    surface
                );
                assert_eq!(terrain.height_at(0, 0), original_height);
                assert_eq!(chunk.generation_stage(), stage);
            }
        }
    }

    #[test]
    fn structure_adapter_updates_negative_chunk_columns_without_changing_extrema() {
        use super::super::super::terrain::PreLightChunkStage;
        use super::super::terraforged_structure_terrain::{Bounds, Piece, StructureTerrain};

        for stage in [PreLightChunkStage::Empty, PreLightChunkStage::Terrain] {
            let registry = registry();
            let mut terrain = terrain();
            let extrema = (terrain.min(), terrain.max(), terrain.max_base());
            let mut chunk =
                TerrainChunk::new(ChunkPos::new(-1, -1), -64, 544, &registry, Properties).unwrap();
            chunk.set_generation_stage(stage);
            for z in 0..16 {
                for x in 0..16 {
                    chunk
                        .fill_solid_run(x, 60, z, 125, BlockStateId(2))
                        .unwrap();
                }
            }
            let operation = StructureTerrain::from_rigid_pieces(vec![Piece {
                bounds: Bounds {
                    min_x: -16,
                    min_y: 70,
                    min_z: -16,
                    max_x: -10,
                    max_y: 75,
                    max_z: -10,
                },
                ground_delta: 0,
            }]);
            operation
                .modify_chunk(&mut chunk, &mut terrain, BlockStateId(2), BlockStateId(0))
                .unwrap();
            assert_eq!(chunk.block(3, 69, 3), BlockStateId(2));
            assert_eq!(chunk.block(3, 70, 3), BlockStateId(0));
            assert_eq!(terrain.height_at(-13, -13), 69);
            for kind in [HeightmapType::OceanFloorWg, HeightmapType::WorldSurfaceWg] {
                assert_eq!(
                    chunk.heightmap(kind).unwrap().highest_taken(3, 3).unwrap(),
                    69
                );
            }
            assert_eq!((terrain.min(), terrain.max(), terrain.max_base()), extrema);
            assert_eq!(chunk.generation_stage(), stage);
        }
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
