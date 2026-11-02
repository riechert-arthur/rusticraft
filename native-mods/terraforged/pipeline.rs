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
    BiomeChunk, BiomeId, BiomeRegion, BiomeSearch, Biomes, DecorateStats, GeneratorData,
    GeneratorError, JigsawPieces, JigsawQuery, LodColumn, OverworldGenerator, Pipeline,
    PreLightChunkStage, ProtoChunk, SeededPipeline, SurfaceInputs, SurfaceQueries, SurfaceSample,
    TerrainChunk, UniformSurfaceBiomes, WorkerState, build_surface, overrides, resource, stage,
    terraforged_generator_facade::TerraForgedGenerator,
    terraforged_generator_preset::{build as build_noise, terrain_levels},
    terraforged_live_biomes::LiveBiomeBindings,
    terraforged_surface_bindings::SurfaceBindings,
    terraforged_world_preset::{GENERATOR_TYPE, PRESET},
};
use crate::heightmap::HeightmapType;
use crate::registry::BlockStateId;
use horizon_core::biome::obfuscate_seed;
use horizon_core::density::{DensitySampler, SamplerContext};
use horizon_core::noise::DensityVolume;
use horizon_core::position::ChunkPos;
use horizon_core::random::XoroshiroRandom;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

type BlockSets = Arc<BTreeMap<String, Arc<BTreeSet<BlockStateId>>>>;

pub(super) struct TerraForgedPipeline {
    vanilla: Pipeline<XoroshiroRandom>,
    generator: TerraForgedGenerator,
    biomes: LiveBiomeBindings,
    /// Block states by block and by tag, for TerraForged's surface passes.
    blocks: BlockSets,
    block_tags: BlockSets,
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
    if preset != PRESET || !is_terraforged(&data, dimension) {
        return OverworldGenerator::build(data, seed, preset, dimension, with_structures);
    }
    let _ = with_structures;
    Ok(OverworldGenerator {
        pipeline: Arc::new(TerraForgedPipeline::new(&data, seed)?),
        noise_clamped: None,
    })
}

impl TerraForgedPipeline {
    /// `GeneratorPreset.build`: TerraForged's noise and biome source over the vanilla
    /// overworld's surface rule, fluids and features (`VanillaGen`). TerraForged's own
    /// structure placement is not wired yet, so the vanilla half runs without structures.
    fn new(data: &Arc<GeneratorData>, seed: i64) -> Result<Self, GeneratorError> {
        let preset: Value = serde_json::from_str(
            data.world_presets
                .get(PRESET)
                .ok_or_else(|| resource(format!("missing world preset {PRESET}")))?,
        )?;
        let levels =
            terrain_levels(&preset["dimensions"]["minecraft:overworld"]["generator"]["levels"])
                .map_err(resource)?;
        let generator = build_noise(levels).map_err(resource)?;
        generator.capture_seed(seed);
        let biomes = LiveBiomeBindings::from_generator_data(data, &[])
            .map_err(|error| resource(error.to_string()))?;
        let (blocks, block_tags) = super::super::block_feature::FeatureBlockBindings::bind_tags(
            data.registry,
            &data.block_tags,
        )?;
        Ok(Self {
            vanilla: vanilla_overworld(data, seed)?,
            generator,
            biomes,
            blocks,
            block_tags,
        })
    }

    fn biome_chunk(&self, pos: ChunkPos) -> Result<BiomeChunk<BiomeId>, GeneratorError> {
        let settings = &self.vanilla.settings;
        let mut chunk =
            BiomeChunk::new(pos, settings.level_min_y, settings.level_height, BiomeId(0))
                .map_err(stage("biomes"))?;
        self.biomes
            .fill_chunk(&self.generator, &mut chunk, pos)
            .map_err(stage("biomes"))?;
        Ok(chunk)
    }

    fn surface_bindings(&self) -> Result<SurfaceBindings<'static>, GeneratorError> {
        let snow_layers = self
            .blocks
            .get("minecraft:snow")
            .cloned()
            .unwrap_or_default();
        SurfaceBindings::new(self.vanilla.data.registry, &self.block_tags, snow_layers)
            .map_err(stage("surface"))
    }
}

/// The `minecraft:normal` overworld's noise pipeline, as `OverworldGenerator::build` makes
/// it, without structures.
fn vanilla_overworld(
    data: &Arc<GeneratorData>,
    seed: i64,
) -> Result<Pipeline<XoroshiroRandom>, GeneratorError> {
    let preset: Value = serde_json::from_str(
        data.world_presets
            .get("minecraft:normal")
            .ok_or_else(|| resource("missing world preset minecraft:normal"))?,
    )?;
    let stem = match data.dimensions.get("minecraft:overworld") {
        Some(json) => serde_json::from_str(json)?,
        None => preset["dimensions"]["minecraft:overworld"].clone(),
    };
    let generator = stem["generator"].clone();
    let level = overrides::LevelHeight::of_type(&stem["type"], &data.dimension_types)
        .map(|height| (height.min_y, height.height));
    let settings: Value = serde_json::from_str(
        data.settings
            .get("minecraft:overworld")
            .ok_or_else(|| resource("missing noise settings minecraft:overworld"))?,
    )?;
    Pipeline::new(
        Arc::clone(data),
        seed,
        XoroshiroRandom::new(seed),
        &generator,
        &settings,
        false,
        level,
    )
}

/// `NoiseChunkUtil.initSurfaceCache`: the vanilla surface rules' preliminary surface is
/// TerraForged's height at each quart column of the chunk, and the chunk's lowest such
/// height around it.
#[derive(Debug)]
struct PreliminarySurface {
    heights: BTreeMap<i64, i32>,
    fallback: i32,
}

impl DensitySampler for PreliminarySurface {
    fn sample_value(
        &self,
        _context: &mut SamplerContext,
        position: [i32; 3],
    ) -> Result<f32, horizon_core::density::SampleError> {
        let key =
            super::terraforged_surface_height_cache::column_key(position[0] & !3, position[2] & !3);
        Ok(*self.heights.get(&key).unwrap_or(&self.fallback) as f32)
    }
}

fn is_terraforged(data: &GeneratorData, dimension: &str) -> bool {
    let Some(json) = data.world_presets.get(PRESET) else {
        return false;
    };
    serde_json::from_str::<Value>(json).is_ok_and(|preset| {
        preset["dimensions"][dimension]["generator"]["type"].as_str() == Some(GENERATOR_TYPE)
    })
}

impl SeededPipeline for TerraForgedPipeline {
    fn generate(
        &self,
        worker: &mut WorkerState,
        pos: ChunkPos,
    ) -> Result<ProtoChunk, GeneratorError> {
        let vanilla = &self.vanilla;
        let data = &vanilla.data;
        let settings = &vanilla.settings;
        let (min, height) = (settings.level_min_y, settings.level_height);
        // createBiomes: the 3x3 neighbourhood the surface rules may look into.
        let mut neighbors = Vec::with_capacity(9);
        let mut possible = BTreeSet::new();
        for i in 0..9 {
            let around =
                ChunkPos::new(pos.x.wrapping_add(i % 3 - 1), pos.z.wrapping_add(i / 3 - 1));
            let chunk = self.biome_chunk(around)?;
            let mut section = 0;
            while let Some(biomes) = chunk.section(section) {
                possible.extend(biomes.iter().map(|id| data.biome_name(*id).to_owned()));
                section += 1;
            }
            neighbors.push(chunk);
        }
        let center = neighbors[4].clone();
        let region =
            BiomeRegion::new(neighbors, obfuscate_seed(vanilla.seed)).map_err(stage("biomes"))?;

        // fillFromNoise.
        let mut chunk = TerrainChunk::new(pos, min, height, data.registry, data.states.clone())
            .map_err(stage("noise"))?;
        chunk.set_generation_stage(PreLightChunkStage::Biomes);
        let block = |name: &str| {
            data.registry
                .default_state(name)
                .ok_or_else(|| resource(format!("missing block {name}")))
        };
        let terrain = self
            .generator
            .fill_from_noise(
                &mut chunk,
                block("minecraft:stone")?,
                block("minecraft:water")?,
            )
            .map_err(stage("noise"))?;
        chunk
            .prime_heightmaps(&[HeightmapType::OceanFloorWg, HeightmapType::WorldSurfaceWg])
            .map_err(stage("heightmaps"))?;

        // buildSurface: the vanilla rules over TerraForged's heights, then Surface.apply.
        let mut heights = BTreeMap::new();
        self.generator.initialize_surface_cache(pos, &mut heights);
        let fallback = heights
            .values()
            .copied()
            .min()
            .unwrap_or(settings.sea_level);
        let preliminary: Arc<dyn DensitySampler> =
            Arc::new(PreliminarySurface { heights, fallback });
        let volume = DensityVolume::new(
            [16, settings.noise_height, 16],
            [
                pos.x.wrapping_shl(4),
                settings.noise_min_y,
                pos.z.wrapping_shl(4),
            ],
            [1; 3],
        )
        .map_err(stage("surface"))?;
        let mut context = std::mem::take(&mut worker.terrain_context);
        let surface = build_surface(
            &mut chunk,
            vanilla.rule.clone(),
            SurfaceInputs::<XoroshiroRandom, _> {
                generation: vanilla.generation(),
                noise_volume: volume,
                root: vanilla.root,
                surface: SurfaceQueries {
                    pipeline: vanilla,
                    temperatures: &mut worker.temperatures,
                    biome: Biomes::Region(&region),
                    uniform_biomes: Some(UniformSurfaceBiomes::new(pos, min, height)),
                    possible: Some(possible),
                },
                preliminary_surface: preliminary,
                density_context: &mut context,
                write_min_y: settings.noise_min_y,
                write_max_y: settings.noise_min_y + settings.noise_height - 1,
            },
            &vanilla.extensions,
        );
        context.reset();
        worker.terrain_context = context;
        surface.map_err(stage("surface"))?;
        self.surface_bindings()?
            .apply(
                &terrain.read(),
                &mut chunk,
                self.generator.levels().max_y,
                self.generator.sea_level(),
            )
            .map_err(stage("surface"))?;

        chunk
            .prime_heightmaps(&[
                HeightmapType::MotionBlocking,
                HeightmapType::MotionBlockingNoLeaves,
                HeightmapType::OceanFloor,
                HeightmapType::WorldSurface,
            ])
            .map_err(stage("heightmaps"))?;
        chunk.set_generation_stage(PreLightChunkStage::Terrain);
        Ok(ProtoChunk {
            terrain: chunk,
            biomes: center,
            data: data.clone(),
            decorated: false,
            block_entities: Vec::new(),
            structure_starts: Vec::new(),
            own_starts: Default::default(),
            entities: Vec::new(),
        })
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
        let _ = &self.generator;
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
    fn noise_biomes(&self, _worker: &mut WorkerState, quarts: &[[i32; 3]]) -> Vec<Option<String>> {
        let data = self.vanilla.data();
        quarts
            .iter()
            .map(|quart| {
                let biome = self.biomes.sample(
                    &self.generator,
                    quart[0].wrapping_shl(2),
                    quart[2].wrapping_shl(2),
                );
                Some(data.biome_name(biome).to_owned())
            })
            .collect()
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

    fn data() -> Option<Arc<GeneratorData>> {
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
        Some(GeneratorData::load(&root.join("worldgen"), registry).unwrap())
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
}
