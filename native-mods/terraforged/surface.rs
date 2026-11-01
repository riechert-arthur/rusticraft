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
    terraforged_engine::TerrainProperties,
    terraforged_terrain_data::TerrainData,
};
use crate::{heightmap::HeightmapType, registry::BlockStateId};

pub fn smooth_water<B: TerrainBlockStates>(
    terrain: &TerrainData,
    chunk: &mut TerrainChunk<'_, B>,
    source_water: BlockStateId,
    level_two_water: BlockStateId,
    mut is_air: impl FnMut(BlockStateId) -> bool,
    mut region_block: impl FnMut(i32, i32, i32) -> Result<BlockStateId, TerrainError>,
) -> Result<(), TerrainError> {
    let kind = HeightmapType::MotionBlockingNoLeaves;
    if chunk.heightmap(kind).is_none() {
        chunk.prime_heightmaps(&[kind])?;
    }
    let position = chunk.position();
    let min_x = position.x.wrapping_shl(4);
    let min_z = position.z.wrapping_shl(4);
    for dz in 0..16 {
        for dx in 0..16 {
            let land = terrain
                .terrain()
                .get(dx, dz)
                .expect("generated terrain type");
            if !(land.is_river() || land.is_lake()) || terrain.river().get(dx, dz) != 0.0 {
                continue;
            }
            let y = chunk
                .heightmap(kind)
                .expect("water heightmap primed")
                .highest_taken(dx, dz)?;
            if chunk.block(dx, y, dz) != source_water {
                continue;
            }
            let x = min_x.wrapping_add(dx);
            let z = min_z.wrapping_add(dz);
            let mut smooth = false;
            'search: for offset_z in -6i32..=6 {
                for offset_x in -6i32..=6 {
                    let distance = offset_x * offset_x + offset_z * offset_z;
                    if distance == 0 || distance > 36 {
                        continue;
                    }
                    let sample_x = x.wrapping_add(offset_x);
                    let sample_z = z.wrapping_add(offset_z);
                    let state = if sample_x >> 4 == position.x && sample_z >> 4 == position.z {
                        chunk.block(sample_x & 15, y, sample_z & 15)
                    } else {
                        region_block(sample_x, y, sample_z)?
                    };
                    if is_air(state) {
                        smooth = true;
                        break 'search;
                    }
                }
            }
            if smooth {
                chunk.set_block_worldgen(dx, y, dz, level_two_water)?;
            }
        }
    }
    Ok(())
}

pub fn apply<B: TerrainBlockStates>(
    terrain: &TerrainData,
    chunk: &mut TerrainChunk<'_, B>,
    gen_depth: i32,
    sea_level: i32,
    mut is_erodible: impl FnMut(BlockStateId) -> bool,
) -> Result<(), TerrainError> {
    let norm = 55.0 * (gen_depth as f32 / 255.0);
    if chunk.heightmap(HeightmapType::OceanFloorWg).is_none() {
        chunk.prime_heightmaps(&[HeightmapType::OceanFloorWg])?;
    }
    for z in 0..16 {
        for x in 0..16 {
            let mut y = chunk
                .heightmap(HeightmapType::OceanFloorWg)
                .expect("surface heightmap primed")
                .highest_taken(x, z)?;
            let gradient = terrain.normalized_gradient(x, z, norm);
            if y < sea_level || gradient < 0.6 {
                continue;
            }
            let Some((bottom, solid)) = find_solid(chunk, x, y, z, &mut is_erodible) else {
                continue;
            };
            while y > bottom {
                chunk.set_block_worldgen(x, y, z, solid)?;
                for kind in [HeightmapType::OceanFloorWg, HeightmapType::WorldSurfaceWg] {
                    if chunk.heightmap(kind).is_some() {
                        chunk.update_heightmap(kind, x, y, z, solid)?;
                    }
                }
                y -= 1;
            }
        }
    }
    Ok(())
}

fn find_solid<B: TerrainBlockStates>(
    chunk: &TerrainChunk<'_, B>,
    x: i32,
    y: i32,
    z: i32,
    is_erodible: &mut impl FnMut(BlockStateId) -> bool,
) -> Option<(i32, BlockStateId)> {
    if !is_erodible(chunk.block(x, y, z)) {
        return None;
    }
    let bottom = 0.max(y.wrapping_sub(20));
    let mut scan_y = y.wrapping_sub(1);
    while scan_y > bottom {
        let state = chunk.block(x, scan_y, z);
        if !is_erodible(state) {
            return Some((scan_y, state));
        }
        scan_y -= 1;
    }
    None
}
