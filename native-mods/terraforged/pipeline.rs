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

// The pipeline a `terraforged:normal` world's overworld generates with, in place of
// `NoiseBasedChunkGenerator`'s. Stages TerraForged does not replace yet run on the
// vanilla `minecraft:normal` pipeline it wraps.

use super::{
    BiomeSearch, DecorateStats, GeneratorData, GeneratorError, JigsawPieces, JigsawQuery,
    LodColumn, OverworldGenerator, ProtoChunk, SeededPipeline, SurfaceSample, WorkerState,
    terraforged_world_preset::{GENERATOR_TYPE, PRESET},
};
use crate::registry::BlockStateId;
use horizon_core::position::ChunkPos;
use serde_json::Value;
use std::sync::Arc;

pub(super) struct TerraForgedPipeline {
    vanilla: Arc<dyn SeededPipeline>,
}

/// `OverworldGenerator::for_world` and `for_dimension`, overwritten: a TerraForged
/// overworld for the TerraForged preset, `OverworldGenerator::build` otherwise.
pub(super) fn build(
    data: Arc<GeneratorData>,
    seed: i64,
    preset: &str,
    dimension: &str,
    with_structures: bool,
) -> Result<OverworldGenerator, GeneratorError> {
    if preset != PRESET || !is_terraforged(&data, preset, dimension) {
        return OverworldGenerator::build(data, seed, preset, dimension, with_structures);
    }
    let vanilla = OverworldGenerator::build(
        data,
        seed,
        "minecraft:normal",
        "minecraft:overworld",
        with_structures,
    )?;
    Ok(OverworldGenerator {
        pipeline: Arc::new(TerraForgedPipeline {
            vanilla: vanilla.pipeline,
        }),
        noise_clamped: None,
    })
}

/// Whether `dimension`'s level stem uses TerraForged's generator. A data pack's stem
/// replaces the preset's, as `OverworldGenerator::build` and 26.3's
/// `WorldDimensions.bake` pick it, so a pack's own overworld keeps its generator.
fn is_terraforged(data: &GeneratorData, preset: &str, dimension: &str) -> bool {
    let stem = match data.dimensions.get(dimension) {
        Some(json) => serde_json::from_str::<Value>(json).ok(),
        None => data
            .world_presets
            .get(preset)
            .and_then(|json| serde_json::from_str::<Value>(json).ok())
            .map(|preset| preset["dimensions"][dimension].clone()),
    };
    stem.is_some_and(|stem| stem["generator"]["type"].as_str() == Some(GENERATOR_TYPE))
}

impl SeededPipeline for TerraForgedPipeline {
    fn generate(
        &self,
        worker: &mut WorkerState,
        pos: ChunkPos,
    ) -> Result<ProtoChunk, GeneratorError> {
        self.vanilla.generate(worker, pos)
    }
    fn decorate(
        &self,
        worker: &mut WorkerState,
        region: &mut [ProtoChunk; 9],
    ) -> Result<DecorateStats, GeneratorError> {
        self.vanilla.decorate(worker, region)
    }
    fn data(&self) -> &Arc<GeneratorData> {
        self.vanilla.data()
    }
    fn end_island_origins(&self, _chunk: ChunkPos) -> Vec<[i32; 3]> {
        Vec::new()
    }
    fn sea_level(&self) -> i32 {
        self.vanilla.sea_level()
    }
    fn level(&self) -> (i32, i32) {
        self.vanilla.level()
    }
    fn surface_sample(
        &self,
        worker: &mut WorkerState,
        x: i32,
        z: i32,
        spans: Option<&mut Vec<(i32, i32, BlockStateId)>>,
    ) -> Option<SurfaceSample> {
        self.vanilla.surface_sample(worker, x, z, spans)
    }
    fn lod_grid(
        &self,
        worker: &mut WorkerState,
        x: i32,
        z: i32,
        width: i32,
        side: usize,
        approximate: bool,
    ) -> Result<Vec<LodColumn>, GeneratorError> {
        self.vanilla
            .lod_grid(worker, x, z, width, side, approximate)
    }
    fn structure_starts(
        &self,
        worker: &mut WorkerState,
        pos: ChunkPos,
    ) -> Vec<Arc<super::super::structure::start::StructureStart>> {
        self.vanilla.structure_starts(worker, pos)
    }
    fn structure_start(
        &self,
        worker: &mut WorkerState,
        structure: &str,
        chunk: ChunkPos,
    ) -> Option<Arc<super::super::structure::start::StructureStart>> {
        self.vanilla.structure_start(worker, structure, chunk)
    }
    fn unsupported_features(&self) -> Vec<(String, String)> {
        self.vanilla.unsupported_features()
    }
    fn use_saved_start(
        &self,
        worker: &mut WorkerState,
        chunk: ChunkPos,
        structure: &str,
        pieces: &[Value],
    ) -> Result<(), String> {
        self.vanilla
            .use_saved_start(worker, chunk, structure, pieces)
    }
    fn forget_structure_starts(&self) {
        self.vanilla.forget_structure_starts()
    }
    fn ring_positions(&self, worker: &mut WorkerState, set: &str) -> Option<Vec<ChunkPos>> {
        self.vanilla.ring_positions(worker, set)
    }
    fn spawn_position(&self) -> Result<[i32; 2], GeneratorError> {
        // ChunkGenerator.getOrigin: a generator without a spawn target spawns at 0 0.
        Ok([0, 0])
    }
    fn region_random(
        &self,
        center: ChunkPos,
    ) -> Box<dyn super::super::structure::legacy::PieceRandom> {
        self.vanilla.region_random(center)
    }
    fn locate(
        &self,
        worker: &mut WorkerState,
        names: &[String],
        from: [i32; 3],
        radius: i32,
        skip: &dyn Fn(&str, ChunkPos) -> bool,
    ) -> Option<(String, [i32; 3])> {
        self.vanilla.locate(worker, names, from, radius, skip)
    }
    fn debug_density_line(&self, _feet: [i32; 3]) -> Option<String> {
        None
    }
    fn find_closest_biome(
        &self,
        worker: &mut WorkerState,
        search: &BiomeSearch<'_>,
    ) -> Option<(String, [i32; 3])> {
        self.vanilla.find_closest_biome(worker, search)
    }
    fn noise_biomes(&self, worker: &mut WorkerState, quarts: &[[i32; 3]]) -> Vec<Option<String>> {
        self.vanilla.noise_biomes(worker, quarts)
    }
    fn jigsaw_pieces(
        &self,
        worker: &mut WorkerState,
        query: &JigsawQuery<'_>,
    ) -> Option<JigsawPieces> {
        self.vanilla.jigsaw_pieces(worker, query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn data_with(ov: &crate::worldgen::overrides::WorldgenOverrides) -> Option<Arc<GeneratorData>> {
        let root = std::env::var_os("HORIZON_TEST_RESOURCES")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/horizon-client/26.3")
            });
        if !root.join("worldgen/world_presets.json").exists() {
            eprintln!("skipping: no worldgen bundle at {}", root.display());
            return None;
        }
        let registry = Box::leak(Box::new(crate::level::data::load_registry(&root).unwrap()));
        Some(GeneratorData::load_with(&root.join("worldgen"), registry, ov).unwrap())
    }

    fn data() -> Option<Arc<GeneratorData>> {
        data_with(&Default::default())
    }

    #[test]
    fn the_preset_builds_a_terraforged_overworld_only() {
        let Some(data) = data() else { return };
        assert!(data.world_presets.contains_key(PRESET));
        let overworld = OverworldGenerator::for_world(
            Arc::clone(&data),
            7,
            PRESET,
            "minecraft:overworld",
            true,
        )
        .unwrap();
        assert_eq!(overworld.spawn_position().unwrap(), [0, 0]);
        let nether = OverworldGenerator::for_world(
            Arc::clone(&data),
            7,
            PRESET,
            "minecraft:the_nether",
            true,
        )
        .unwrap();
        assert_ne!(nether.level(), overworld.level());
        let mut worker = overworld.worker();
        let chunk = worker.generate_proto(ChunkPos::new(0, 0)).unwrap();
        assert_eq!(chunk.position(), ChunkPos::new(0, 0));
    }

    #[test]
    fn a_data_pack_overworld_keeps_its_own_generator() {
        let mut ov = crate::worldgen::overrides::WorldgenOverrides::default();
        let vanilla = r#"{"type":"minecraft:overworld","generator":{"type":"minecraft:noise","biome_source":{"type":"minecraft:multi_noise","preset":"minecraft:overworld"},"settings":"minecraft:overworld"}}"#;
        ov.set(
            crate::worldgen::overrides::DIMENSIONS,
            "minecraft:overworld",
            vanilla.to_owned(),
        );
        let Some(data) = data_with(&ov) else { return };
        assert!(!is_terraforged(&data, PRESET, "minecraft:overworld"));
        let overworld =
            OverworldGenerator::for_dimension(Arc::clone(&data), 7, PRESET, "minecraft:overworld")
                .unwrap();
        let normal = OverworldGenerator::for_dimension(
            Arc::clone(&data),
            7,
            "minecraft:normal",
            "minecraft:overworld",
        )
        .unwrap();
        // The vanilla spawn search, not TerraForged's `ChunkGenerator.getOrigin` 0 0.
        assert_ne!(normal.spawn_position().unwrap(), [0, 0]);
        assert_eq!(
            overworld.spawn_position().unwrap(),
            normal.spawn_position().unwrap()
        );
    }

    #[test]
    fn every_stage_still_generates_the_normal_overworld() {
        let Some(data) = data() else { return };
        let seed = -4_172_144_997_902_289_642;
        let terraforged = OverworldGenerator::for_dimension(
            Arc::clone(&data),
            seed,
            PRESET,
            "minecraft:overworld",
        )
        .unwrap();
        let normal = OverworldGenerator::for_dimension(
            Arc::clone(&data),
            seed,
            "minecraft:normal",
            "minecraft:overworld",
        )
        .unwrap();
        assert_eq!(terraforged.level(), normal.level());
        assert_eq!(terraforged.sea_level(), normal.sea_level());
        let pos = ChunkPos::new(3, -2);
        let a = terraforged.worker().generate_proto(pos).unwrap();
        let b = normal.worker().generate_proto(pos).unwrap();
        let (min_y, height) = normal.level();
        for y in min_y..min_y + height {
            for z in 0..16 {
                for x in 0..16 {
                    assert_eq!(a.block(x, y, z), b.block(x, y, z), "block {x} {y} {z}");
                    if (x | y | z) & 3 == 0 {
                        assert_eq!(a.biome_id_at(x, y, z), b.biome_id_at(x, y, z));
                    }
                }
            }
        }
    }
}
