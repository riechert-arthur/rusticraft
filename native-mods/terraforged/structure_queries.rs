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

// Structure placement over TerraForged: `Generator.getBaseHeight` and `getBaseColumn`, and `Source.getNoiseBiome`
// (the surface biome, whatever the height), for the vanilla structure placement the original keeps
// (`ChunkGenerator.createStructures`). The heights are TerraForged Reimagined's eroded ones
// (https://github.com/Romoslayer/TerraForged-Reimagined, a24d450, `Generator.getBaseHeight`; MIT License,
// Copyright (c) 2020 TerraForged, Copyright (c) 2026 Romoslayer; see `LICENSE-TERRAFORGED-REIMAGINED`), so
// structures stand on the ground the chunk is filled to; see `TerraForgedGenerator::base_height`.

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

/// The biome of a column whose pool has no biome (Java's `Source.getNoiseBiome` returns null there): it is in no
/// structure's biome set and in no ring search's, so nothing is placed on it. Java would throw out of
/// `Structure.isValidBiome`; such a column's own `createBiomes` fails the same way (`BiomeFillError::Resolve`).
pub(super) const NO_BIOME: BiomeId = BiomeId(u32::MAX);

/// `ChunkGenerator`'s structure queries over TerraForged's generator, for one worker.
pub(super) struct TfQueries<'a> {
    pub(super) pipeline: &'a TerraForgedPipeline,
    pub(super) worker: &'a mut WorkerState,
    pub(super) valid: BTreeSet<BiomeId>,
}

impl TfQueries<'_> {
    /// `Source.getNoiseBiome` of the quarts at `quarts` (`[x, _, z]`; TerraForged ignores `y`), cached per quart
    /// column for the worker. Misses are sampled with one surface sampler.
    fn biomes(&mut self, quarts: &[[i32; 3]]) -> Vec<BiomeId> {
        let mut sampler = None;
        quarts
            .iter()
            .map(|quart| {
                let key = [quart[0], 0, quart[2]];
                if let Some(&id) = self.worker.structure_biomes.get(&key) {
                    return id;
                }
                let sampler = sampler.get_or_insert_with(|| self.pipeline.surface_sampler());
                let id = self
                    .pipeline
                    .biome_at(sampler, key[0].wrapping_shl(2), key[2].wrapping_shl(2))
                    .unwrap_or(NO_BIOME);
                if self.worker.structure_biomes.len() >= 1 << 16 {
                    self.worker.structure_biomes.clear();
                }
                self.worker.structure_biomes.insert(key, id);
                id
            })
            .collect()
    }

    /// `getBaseColumn` in the level's build range: stone from the level's bottom to the (eroded) terrain height, water
    /// to the surface, air above, clipped as the fill clips, so it is the filled ground (`base_height` is one block
    /// above it). TerraForged starts its column at the height instead, which makes column-placed structures float;
    /// see `TerraForgedGenerator::base_column` (Arthur's decision). Panics where the noise has no seed-first form (no
    /// bundled noise is a custom module; Java has no such failure).
    fn column(&self, x: i32, z: i32) -> NoiseColumn {
        let settings = &self.pipeline.vanilla.settings;
        let column = self
            .pipeline
            .generator
            .base_column(x, z, settings.level_min_y, settings.level_height)
            .expect("TerraForged's getBaseColumn: the noise has no seed-first form");
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

/// A TerraForged base height (the first free block above the column) as the clipped fill's heightmaps read it in a
/// level of `height` blocks from `min_y`: at most the top (`min_y + height`, a column cut flat at `top - 1`), at least
/// the bottom (a column with no stone in the level).
fn clip_to_level(base_height: i32, min_y: i32, height: i32) -> i32 {
    let top = min_y.saturating_add(height);
    base_height.clamp(min_y, top.max(min_y))
}

impl JigsawContext for TfQueries<'_> {
    fn min_y(&self) -> i32 {
        self.pipeline.vanilla.settings.level_min_y
    }
    fn max_y(&self) -> i32 {
        let settings = &self.pipeline.vanilla.settings;
        settings.level_min_y + settings.level_height - 1
    }
    /// `getBaseHeight` (Reimagined's eroded height), clipped to the level's build range as the terrain fill clips
    /// the terrain: a column above the top is filled to a flat top whose heightmaps read the top
    /// (`level_min_y + level_height`), one below the bottom gets no stone and reads the bottom, so a structure is
    /// placed where the filled chunk has its ground. Panics as [`Self::column`] does.
    fn base_height(&mut self, x: i32, z: i32, kind: HeightmapType) -> i32 {
        if let Some(height) = self.worker.heights.get(&(x, z, kind as u8)) {
            return *height;
        }
        let settings = &self.pipeline.vanilla.settings;
        let height = clip_to_level(
            self.pipeline
                .generator
                .base_height(x, z, heightmap(kind))
                .expect("TerraForged's getBaseHeight: the noise has no seed-first form"),
            settings.level_min_y,
            settings.level_height,
        );
        if self.worker.heights.len() > 65536 {
            self.worker.heights.clear();
        }
        self.worker.heights.insert((x, z, kind as u8), height);
        height
    }
    fn column_has_valid_biome(&mut self, x: i32, z: i32, min_y: i32, max_y: i32) -> bool {
        min_y <= max_y && {
            let biome = self.biomes(&[[x >> 2, 0, z >> 2]])[0];
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
    /// `Biome.coldEnoughToSnow(pos, chunkGenerator.getSeaLevel())` (26.3's `RuinedPortalStructure.isCold`): TerraForged's
    /// sea level, not the vanilla half's. [`NO_BIOME`] is never cold.
    fn cold_enough_to_snow(&mut self, biome: BiomeId, pos: [i32; 3]) -> bool {
        let sea = self.pipeline.generator.sea_level();
        self.worker
            .temperatures
            .get_mut(biome.0 as usize)
            .is_some_and(|t| t.cold_enough_to_snow(BlockPos::new(pos[0], pos[1], pos[2]), sea))
    }
    /// `new WorldGenerationContext(chunkGenerator, heightAccessor)`, as 26.3's structures build it for their height
    /// providers: TerraForged's minimum height, generation depth and sea level.
    fn generation(&self) -> WorldGenerationContext {
        self.pipeline.generation()
    }
    fn noise_biome(&mut self, quart: [i32; 3]) -> BiomeId {
        self.biomes(&[quart])[0]
    }
    fn noise_biomes(&mut self, quarts: &[[i32; 3]]) -> Vec<BiomeId> {
        self.biomes(quarts)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Heights above the build top read the top, heights below the bottom read the bottom, others are kept.
    #[test]
    fn base_heights_clip_to_the_level_as_the_fill_does() {
        for (height, clipped) in [
            (405, 320),
            (321, 320),
            (320, 320),
            (319, 319),
            (85, 85),
            (-64, -64),
            (-65, -64),
            (i32::MIN, -64),
            (i32::MAX, 320),
        ] {
            assert_eq!(clip_to_level(height, -64, 384), clipped, "{height}");
        }
        assert_eq!(clip_to_level(500, 0, 256), 256);
        assert_eq!(clip_to_level(500, i32::MAX - 4, 16), i32::MAX - 4);
        assert_eq!(clip_to_level(i32::MAX, i32::MAX - 4, 16), i32::MAX);
    }
}
