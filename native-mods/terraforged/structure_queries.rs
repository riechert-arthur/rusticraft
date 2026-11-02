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

// Structure placement over TerraForged: `Generator.getBaseHeight` and `getBaseColumn`,
// and `Source.getNoiseBiome` (the surface biome, whatever the height), for the vanilla
// structure placement the original keeps (`ChunkGenerator.createStructures`).

use super::{
    super::{
        anchor::WorldGenerationContext,
        biome::BiomeId,
        structure::{
            jigsaw::JigsawContext,
            start::{NoiseBlock, NoiseColumn, StartContext},
        },
    },
    WorkerState,
    terraforged_generator_facade::{ColumnBlock, TerraHeightmap},
    terraforged_pipeline::TerraForgedPipeline,
};
use crate::heightmap::HeightmapType;
use horizon_core::position::BlockPos;
use std::collections::BTreeSet;

pub(super) struct TfQueries<'a> {
    pub(super) pipeline: &'a TerraForgedPipeline,
    pub(super) worker: &'a mut WorkerState,
    pub(super) valid: BTreeSet<BiomeId>,
}

impl TfQueries<'_> {
    fn biome(&mut self, x: i32, z: i32) -> BiomeId {
        let quart = [x >> 2, 0, z >> 2];
        if let Some(&id) = self.worker.structure_biomes.get(&quart) {
            return id;
        }
        let id = self.pipeline.biome_at(x & !3, z & !3);
        if self.worker.structure_biomes.len() >= 1 << 16 {
            self.worker.structure_biomes.clear();
        }
        self.worker.structure_biomes.insert(quart, id);
        id
    }

    /// `getBaseColumn`: stone to the terrain height and water to the base height, held
    /// from the terrain height up (the original passes the height as the column's minimum).
    fn column(&self, x: i32, z: i32) -> NoiseColumn {
        let column = self.pipeline.generator.base_column(x, z);
        NoiseColumn {
            min: column.min_y,
            blocks: column
                .states
                .iter()
                .map(|block| match block {
                    ColumnBlock::Stone => NoiseBlock::Solid,
                    ColumnBlock::Water => NoiseBlock::Water,
                })
                .collect(),
        }
    }
}

fn heightmap(kind: HeightmapType) -> TerraHeightmap {
    match kind {
        HeightmapType::WorldSurface => TerraHeightmap::WorldSurface,
        HeightmapType::WorldSurfaceWg => TerraHeightmap::WorldSurfaceWorldgen,
        HeightmapType::MotionBlocking => TerraHeightmap::MotionBlocking,
        HeightmapType::MotionBlockingNoLeaves => TerraHeightmap::MotionBlockingNoLeaves,
        HeightmapType::OceanFloor => TerraHeightmap::OceanFloor,
        HeightmapType::OceanFloorWg => TerraHeightmap::OceanFloorWorldgen,
    }
}

impl JigsawContext for TfQueries<'_> {
    fn min_y(&self) -> i32 {
        self.pipeline.vanilla.settings.level_min_y
    }
    fn max_y(&self) -> i32 {
        let settings = &self.pipeline.vanilla.settings;
        settings.level_min_y + settings.level_height - 1
    }
    fn base_height(&mut self, x: i32, z: i32, kind: HeightmapType) -> i32 {
        if let Some(height) = self.worker.heights.get(&(x, z, kind as u8)) {
            return *height;
        }
        let height = self.pipeline.generator.base_height(x, z, heightmap(kind));
        if self.worker.heights.len() > 65536 {
            self.worker.heights.clear();
        }
        self.worker.heights.insert((x, z, kind as u8), height);
        height
    }
    fn column_has_valid_biome(&mut self, x: i32, z: i32, min_y: i32, max_y: i32) -> bool {
        min_y <= max_y && {
            let biome = self.biome(x, z);
            self.valid.contains(&biome)
        }
    }
}

impl StartContext for TfQueries<'_> {
    fn seed(&self) -> i64 {
        self.pipeline.vanilla.seed
    }
    fn sea_level(&self) -> i32 {
        self.pipeline.generator.sea_level()
    }
    fn base_column(&mut self, x: i32, z: i32, kind: HeightmapType) -> Vec<bool> {
        self.base_columns(x, z, &[kind]).pop().unwrap_or_default()
    }
    fn base_columns(&mut self, x: i32, z: i32, kinds: &[HeightmapType]) -> Vec<Vec<bool>> {
        let settings = &self.pipeline.vanilla.settings;
        let (min, height) = (settings.noise_min_y, settings.noise_height);
        let column = self.column(x, z);
        let data = &self.pipeline.vanilla.data;
        let state = |block| {
            let name = match block {
                NoiseBlock::Water => "minecraft:water",
                _ => "minecraft:stone",
            };
            data.registry
                .default_state(name)
                .map(|id| data.states.get(id))
        };
        let (stone, water) = (state(NoiseBlock::Solid), state(NoiseBlock::Water));
        kinds
            .iter()
            .map(|kind| {
                (min..min + height)
                    .map(|y| match column.get(y) {
                        NoiseBlock::Solid => stone.is_some_and(|state| kind.is_opaque(state)),
                        NoiseBlock::Water => water.is_some_and(|state| kind.is_opaque(state)),
                        _ => false,
                    })
                    .collect()
            })
            .collect()
    }
    fn noise_column(&mut self, x: i32, z: i32) -> NoiseColumn {
        self.column(x, z)
    }
    fn cold_enough_to_snow(&mut self, biome: BiomeId, pos: [i32; 3]) -> bool {
        let sea = self.pipeline.vanilla.settings.sea_level;
        self.worker.temperatures[biome.0 as usize]
            .cold_enough_to_snow(BlockPos::new(pos[0], pos[1], pos[2]), sea)
    }
    fn generation(&self) -> WorldGenerationContext {
        self.pipeline.vanilla.generation()
    }
    fn noise_biome(&mut self, quart: [i32; 3]) -> BiomeId {
        self.biome(quart[0].wrapping_shl(2), quart[2].wrapping_shl(2))
    }
    fn noise_biomes(&mut self, quarts: &[[i32; 3]]) -> Vec<BiomeId> {
        quarts
            .iter()
            .map(|&quart| self.noise_biome(quart))
            .collect()
    }
    fn set_valid_biomes(&mut self, biomes: Option<&BTreeSet<BiomeId>>) {
        self.valid = match biomes {
            Some(biomes) => biomes.clone(),
            None => (0..self.pipeline.vanilla.data.biome_names().len() as u32)
                .map(BiomeId)
                .collect(),
        };
    }
}
