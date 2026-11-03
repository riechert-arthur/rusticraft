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

use super::super::generator_structures::{Starts, StructureState};
use super::{
    BiomeChunk, BiomeId, BiomeRegion, BiomeSearch, Biomes, DecorateStats, GeneratorData,
    GeneratorError, JigsawPieces, JigsawQuery, LodColumn, MaterialConditions,
    MaterialDensitySources, OverworldGenerator, Pipeline, PreLightChunkStage, ProtoChunk,
    SeededPipeline, SurfaceInputs, SurfaceQueries, SurfaceSample, TerrainChunk,
    UniformSurfaceBiomes, WorkerState, build_surface, overrides, resource, stage,
    terraforged_cave_assets::current_caves,
    terraforged_cave_biome_sampler::CaveBiomeSampler,
    terraforged_cave_carver::NoiseCaveGenerator,
    terraforged_generator_facade::{TerraForgedGenerator, TerraHeightmap},
    terraforged_generator_preset::{build as build_noise, terrain_levels},
    terraforged_live_biomes::LiveBiomeBindings,
    terraforged_settings::{self, Settings},
    terraforged_structure_queries::TfQueries,
    terraforged_structure_terrain::StructureTerrain,
    terraforged_surface_bindings::SurfaceBindings,
    terraforged_vegetation::{VegetationConfig, current_vegetation},
    terraforged_world_preset::{GENERATOR_TYPE, PRESET},
};
use crate::heightmap::HeightmapType;
use crate::registry::BlockStateId;
use horizon_core::biome::obfuscate_seed;
use horizon_core::density::{DensitySampler, SamplerContext};
use horizon_core::noise::DensityVolume;
use horizon_core::position::{BlockPos, ChunkPos};
use horizon_core::random::XoroshiroRandom;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

type BlockSets = Arc<BTreeMap<String, Arc<BTreeSet<BlockStateId>>>>;

pub(super) struct TerraForgedPipeline {
    pub(super) vanilla: Pipeline<XoroshiroRandom>,
    pub(super) generator: TerraForgedGenerator,
    biomes: LiveBiomeBindings,
    caves: NoiseCaveGenerator<BiomeId>,
    vegetation: Vec<VegetationConfig>,
    structures: Option<StructureState>,
    /// Block states by block and by tag, for TerraForged's surface passes.
    blocks: BlockSets,
    block_tags: BlockSets,
    /// Horizon's settings of the world (`terraforged_settings`), when it was built.
    settings: Settings,
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
    Ok(OverworldGenerator {
        pipeline: Arc::new(TerraForgedPipeline::new(&data, seed, with_structures)?),
        noise_clamped: None,
    })
}

impl TerraForgedPipeline {
    /// `GeneratorPreset.build`: TerraForged's noise and biome source over the vanilla
    /// overworld's surface rule, fluids and features (`VanillaGen`). TerraForged's own
    /// structure placement is not wired yet, so the vanilla half runs without structures.
    fn new(
        data: &Arc<GeneratorData>,
        seed: i64,
        with_structures: bool,
    ) -> Result<Self, GeneratorError> {
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
        // CaveBiomeSampler: GLOBAL caves use terraforged:cave; UNIQUE caves draw from
        // every registered biome (the Nether's and the End's too).
        let global = data
            .biome_id("terraforged:cave")
            .map(Arc::new)
            .into_iter()
            .collect();
        let special = if terraforged_settings::current().corrected_biomes {
            // `corrected_biomes`: the overworld's cave biomes, not the Nether's or the End's.
            super::terraforged_biome_registry::CAVE_BIOMES
                .iter()
                .filter(|name| **name != "terraforged:cave")
                .filter_map(|name| data.biome_id(name))
                .map(Arc::new)
                .collect()
        } else {
            (0..data.biome_names().len())
                .map(|id| Arc::new(BiomeId(id as u32)))
                .collect()
        };
        let caves = NoiseCaveGenerator::new(
            current_caves().map_err(resource)?,
            CaveBiomeSampler::new(800, global, special),
            seed,
        );
        // ChunkGenerator's structure placement over Source.possibleBiomes (the overworld
        // biome list).
        let structures = if with_structures {
            let possible = biomes.map.overworld.iter().map(|id| **id).collect();
            let generator_json = &preset["dimensions"]["minecraft:overworld"]["generator"];
            StructureState::new(data, &possible, data.rings_for(seed, generator_json))?
        } else {
            None
        };
        Ok(Self {
            vanilla: vanilla_overworld(data, seed)?,
            generator,
            biomes,
            caves,
            vegetation: current_vegetation().map_err(resource)?,
            structures,
            blocks,
            block_tags,
            settings: terraforged_settings::current(),
        })
    }

    /// `Source.getNoiseBiome` at a block column.
    pub(super) fn biome_at(&self, x: i32, z: i32) -> BiomeId {
        self.biomes.sample(&self.generator, x, z)
    }

    fn queries<'a>(&'a self, worker: &'a mut WorkerState) -> TfQueries<'a> {
        TfQueries {
            pipeline: self,
            worker,
            valid: BTreeSet::new(),
        }
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

    /// Distant columns from TerraForged's base columns (stone to the terrain height,
    /// water to the base level, as `getBaseColumn` builds them), their top blocks painted by
    /// the vanilla surface rule over TerraForged's biome.
    fn far_columns(&self, worker: &mut WorkerState, columns: &[(i32, i32)]) -> Vec<LodColumn> {
        let vanilla = &self.vanilla;
        let data = &vanilla.data;
        let settings = &vanilla.settings;
        let (Some(stone), Some(water)) = (
            data.registry.default_state("minecraft:stone"),
            data.registry.default_state("minecraft:water"),
        ) else {
            return columns.iter().map(|_| (None, Vec::new())).collect();
        };
        let min_y = settings.level_min_y;
        let top_y = min_y + settings.level_height;
        let mut geometry = Vec::with_capacity(columns.len());
        let mut chunks: BTreeMap<(i32, i32), BiomeId> = BTreeMap::new();
        for &(x, z) in columns {
            let solid = (self
                .generator
                .base_height(x, z, TerraHeightmap::OceanFloorWorldgen)
                - 1)
            .clamp(min_y - 1, top_y - 1);
            let surface = (self
                .generator
                .base_height(x, z, TerraHeightmap::WorldSurfaceWorldgen)
                - 1)
            .clamp(solid, top_y - 1);
            let biome = self.biome_at(x & !3, z & !3);
            chunks.entry((x >> 4, z >> 4)).or_insert(biome);
            let mut spans = Vec::new();
            if surface > solid {
                spans.push((solid + 1, surface + 1, water));
            }
            if solid >= min_y {
                spans.push((min_y, solid + 1, stone));
            }
            let water_top = (surface > solid).then_some((surface + 1, water));
            let top = (solid >= min_y).then_some((solid + 1, stone));
            geometry.push((top, water_top, solid + 1 - min_y, spans, biome));
        }
        let region = chunks
            .iter()
            .map(|(&(cx, cz), &biome)| {
                BiomeChunk::new(ChunkPos::new(cx, cz), min_y, settings.level_height, biome)
            })
            .collect::<Result<Vec<_>, _>>()
            .ok()
            .and_then(|chunks| BiomeRegion::new(chunks, obfuscate_seed(vanilla.seed)).ok());
        let Some(region) = region else {
            return geometry
                .into_iter()
                .map(|(.., spans, _)| (None, spans))
                .collect();
        };
        let Ok(volume) = DensityVolume::new([1; 3], [columns[0].0, 0, columns[0].1], [1; 3]) else {
            return geometry
                .into_iter()
                .map(|(.., spans, _)| (None, spans))
                .collect();
        };
        let queries = SurfaceQueries {
            pipeline: vanilla,
            temperatures: &mut worker.temperatures,
            biome: Biomes::Region(&region),
            uniform_biomes: None,
            possible: None,
        };
        let preliminary: Arc<dyn DensitySampler> = Arc::new(PreliminarySurface {
            heights: BTreeMap::new(),
            fallback: settings.sea_level,
        });
        let material =
            MaterialDensitySources::new(queries, volume, preliminary, SamplerContext::cached())
                .ok()
                .map(|sources| {
                    MaterialConditions::<XoroshiroRandom, _>::new(
                        vanilla.generation(),
                        vanilla.root,
                        sources,
                    )
                });
        let Some(mut material) = material else {
            return geometry
                .into_iter()
                .map(|(.., spans, _)| (None, spans))
                .collect();
        };
        let Ok(rule) = material.add_rule(vanilla.rule.clone()) else {
            return geometry
                .into_iter()
                .map(|(.., spans, _)| (None, spans))
                .collect();
        };
        let mut result = Vec::with_capacity(geometry.len());
        for (&(wx, wz), (top, water, below, mut spans, biome)) in columns.iter().zip(geometry) {
            let Some((height, mut state)) = top else {
                result.push((None, spans));
                continue;
            };
            material.update_xz(wx, wz, 0, 0);
            for depth in 1..=below.min(8) {
                let y = height - depth;
                material.update_y(
                    depth,
                    below - depth + 1,
                    water.map_or(i32::MIN, |(h, _)| h),
                    y,
                );
                let Some(index) = spans.iter().position(|&(min, max, _)| min <= y && y < max)
                else {
                    continue;
                };
                let (min, max, old) = spans[index];
                let painted = material
                    .try_apply(rule, BlockPos::new(wx, y, wz))
                    .ok()
                    .flatten()
                    .unwrap_or(old);
                if depth == 1 {
                    state = painted;
                }
                if old == painted {
                    continue;
                }
                let mut replacement = Vec::with_capacity(3);
                if y + 1 < max {
                    replacement.push((y + 1, max, old));
                }
                replacement.push((y, y + 1, painted));
                if min < y {
                    replacement.push((min, y, old));
                }
                spans.splice(index..=index, replacement);
            }
            result.push((
                Some(SurfaceSample {
                    height,
                    state,
                    water,
                    biome,
                }),
                spans,
            ));
        }
        result
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
        let mut center = neighbors[4].clone();
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
        // createStructures / createReferences, then the structure terrain the fill adapts.
        let (references, own_starts) = match &self.structures {
            Some(structures) => {
                let mut queries = self.queries(worker);
                (
                    structures.references(&mut queries, pos),
                    structures.starts_at(&mut queries, pos),
                )
            }
            None => (Vec::new(), Starts::default()),
        };
        let adaptation = match &self.structures {
            Some(structures) => {
                StructureTerrain::from_starts(&structures.registry, &references, pos)
            }
            None => StructureTerrain::from_rigid_pieces(Vec::new()),
        };
        let terrain = self
            .generator
            .fill_from_noise_with_structures(
                &mut chunk,
                block("minecraft:stone")?,
                block("minecraft:water")?,
                block("minecraft:air")?,
                &adaptation,
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
                // SurfaceSystem.buildSurface walks each column down from its own top: the
                // whole level, not only the vanilla noise range.
                write_min_y: min,
                write_max_y: min + height - 1,
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

        // applyCarvers: TerraForged's noise caves; no vanilla carvers run.
        let levels = self.generator.levels();
        self.caves
            .carve(
                vanilla.seed as i32,
                &mut chunk,
                &mut center,
                &terrain.read(),
                levels.min_y,
                levels.sea_level,
                block("minecraft:air")?,
                true,
            )
            .map_err(stage("carvers"))?;

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
            structure_starts: references,
            own_starts,
            entities: Vec::new(),
        })
    }
    fn decorate(
        &self,
        worker: &mut WorkerState,
        region: &mut [ProtoChunk; 9],
    ) -> Result<DecorateStats, GeneratorError> {
        let vanilla = &self.vanilla;
        let decoration = vanilla
            .decoration
            .as_ref()
            .ok_or_else(|| resource("decoration resources were not loaded"))?;
        let surface = self.surface_bindings()?;
        let mut random = vanilla.region_random_at(region[4].position());
        super::super::generator_decoration::terraforged_decorator::decorate(
            decoration,
            &vanilla.data,
            vanilla.seed,
            vanilla.generation(),
            worker,
            region,
            &mut random,
            self.structures.as_ref(),
            &super::super::generator_decoration::terraforged_decorator::TerraForgedDecoration {
                generator: &self.generator,
                vegetation: &self.vegetation,
                caves: &self.caves,
                surface: &surface,
                settings: self.settings,
            },
        )
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
        let (sample, column) = self.far_columns(worker, &[(x, z)]).pop()?;
        if let Some(spans) = spans {
            *spans = column;
        }
        sample
    }
    fn lod_grid(
        &self,
        worker: &mut WorkerState,
        x: i32,
        z: i32,
        width: i32,
        side: usize,
        _approximate: bool,
    ) -> Result<Vec<LodColumn>, GeneratorError> {
        if width <= 0 || side == 0 || side > 64 {
            return Err(resource("far sample dimensions"));
        }
        let columns: Vec<(i32, i32)> = (0..side * side)
            .map(|index| {
                (
                    x + (index % side) as i32 * width,
                    z + (index / side) as i32 * width,
                )
            })
            .collect();
        Ok(self.far_columns(worker, &columns))
    }
    fn structure_starts(
        &self,
        worker: &mut WorkerState,
        pos: ChunkPos,
    ) -> Vec<Arc<super::super::structure::start::StructureStart>> {
        match &self.structures {
            Some(structures) => structures
                .starts_at(&mut self.queries(worker), pos)
                .to_vec(),
            None => Vec::new(),
        }
    }
    fn structure_start(
        &self,
        worker: &mut WorkerState,
        structure: &str,
        chunk: ChunkPos,
    ) -> Option<Arc<super::super::structure::start::StructureStart>> {
        self.structures
            .as_ref()?
            .structure_start(&mut self.queries(worker), structure, chunk)
            .map(Arc::new)
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
        let structures = self
            .structures
            .as_ref()
            .ok_or("structures were not loaded")?;
        structures.use_saved_start(&mut self.queries(worker), chunk, structure, pieces)
    }
    fn forget_structure_starts(&self) {
        if let Some(structures) = &self.structures {
            structures.forget_starts();
        }
    }
    fn ring_positions(&self, worker: &mut WorkerState, set: &str) -> Option<Vec<ChunkPos>> {
        self.structures
            .as_ref()?
            .ring_positions(&mut self.queries(worker), set)
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
        self.structures
            .as_ref()?
            .locate(&mut self.queries(worker), names, from, radius, skip)
    }
    fn debug_density_line(&self, _feet: [i32; 3]) -> Option<String> {
        None
    }
    fn find_closest_biome(
        &self,
        _worker: &mut WorkerState,
        search: &BiomeSearch<'_>,
    ) -> Option<(String, [i32; 3])> {
        // BiomeSource.findClosestBiome3d over Source.possibleBiomes. TerraForged's noise
        // biome is the same at every height, so a column answers at its first y.
        let data = self.vanilla.data();
        let candidates: BTreeSet<BiomeId> = self
            .biomes
            .map
            .overworld
            .iter()
            .map(|id| **id)
            .filter(|id| (search.allowed)(data.biome_name(*id)))
            .collect();
        // Biomes only caves hold (terraforged:cave, the unique caves' ones): found where
        // a cave carves them, not in the noise biome.
        let underground: BTreeSet<BiomeId> = (0..data.biome_names().len())
            .map(|id| BiomeId(id as u32))
            .filter(|id| (search.allowed)(data.biome_name(*id)) && self.caves.may_hold(id))
            .collect();
        if candidates.is_empty() && underground.is_empty() {
            return None;
        }
        let seed = self.generator.seed();
        let min_y = search.min_y;
        let y = *super::out_from_origin(
            search.origin[1],
            search.min_y + 1,
            search.max_y + 1,
            search.vertical,
        )
        .first()?;
        let radius = search.radius.div_euclid(search.horizontal);
        for [sx, sz] in super::spiral_around(radius) {
            let x = search.origin[0].wrapping_add(sx.wrapping_mul(search.horizontal));
            let z = search.origin[2].wrapping_add(sz.wrapping_mul(search.horizontal));
            if !candidates.is_empty() {
                let biome = self.biome_at(x & !3, z & !3);
                if candidates.contains(&biome) {
                    return Some((data.biome_name(biome).to_owned(), [x, y, z]));
                }
            }
            if !underground.is_empty() {
                let first_free = self
                    .generator
                    .base_height(x, z, TerraHeightmap::OceanFloorWorldgen)
                    .wrapping_add(1);
                if let Some((biome, cave_y)) = self.caves.underground_biome_at(
                    seed,
                    x,
                    z,
                    first_free,
                    self.generator.sea_level(),
                    min_y,
                ) && underground.contains(&biome)
                {
                    return Some((data.biome_name(biome).to_owned(), [x, cave_y, z]));
                }
            }
        }
        None
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
        // JigsawPlacement.generateJigsaw: addPieces with no heightmap
        // projection, MaxDistance(128), no aliases, no padding and
        // waterlogging applied. The pieces draw from the GenerationContext's
        // random, seeded from the level seed and the position's chunk, not
        // from the level random.
        let structures = self.structures.as_ref()?;
        let mut random =
            horizon_core::random::WorldgenRandom::new(horizon_core::random::LegacyRandom::new(0));
        random.set_large_feature_seed(
            self.vanilla.seed,
            query.position[0] >> 4,
            query.position[2] >> 4,
        );
        let aliases = BTreeMap::new();
        let config = super::super::structure::jigsaw::JigsawConfig {
            start_pool: query.pool,
            start_jigsaw: Some(query.target),
            max_depth: query.max_depth,
            use_expansion_hack: false,
            project_to: None,
            max_distance: [128, 128],
            aliases: &aliases,
            padding: [0, 0],
            waterlogging: true,
        };
        let (_, pieces) = super::super::structure::jigsaw::assemble(
            &config,
            &mut self.queries(worker),
            &structures.registry.pools,
            &structures.resources,
            &structures.bindings,
            &mut random,
            query.position,
        )?;
        Some(JigsawPieces {
            pieces,
            resources: structures.resources.clone(),
        })
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
        // Noise caves open air well below the surface somewhere nearby.
        let mut cave_air = 0;
        for (cx, cz) in [(0, 0), (3, -2), (-5, 4), (8, 8)] {
            let chunk = worker.generate_proto(ChunkPos::new(cx, cz)).unwrap();
            for x in 0..16 {
                for z in 0..16 {
                    let top = chunk.surface_height(x, z);
                    cave_air += (-60..top - 12)
                        .filter(|y| chunk.block_name(x, *y, z) == "minecraft:air")
                        .count();
                }
            }
        }
        assert!(cave_air > 0);
        // Far samples follow TerraForged's terrain and paint its surface.
        let near = worker.generate_proto(ChunkPos::new(3, -2)).unwrap();
        let mut grass = 0;
        for (x, z) in [(48, -32), (52, -28), (56, -24), (60, -20)] {
            let far = worker.surface_sample(x, z).unwrap();
            let real = near.surface_height(x & 15, z & 15);
            assert!(
                (far.height - real).abs() <= 12,
                "{x} {z}: far {} real {real}",
                far.height
            );
            grass += usize::from(
                near.block_name(x & 15, far.height - 1, z & 15) == "minecraft:grass_block",
            );
        }
        let _ = grass;
        // Villages place on TerraForged's terrain and biomes.
        let villages = [
            "minecraft:village_plains",
            "minecraft:village_desert",
            "minecraft:village_savanna",
            "minecraft:village_snowy",
            "minecraft:village_taiga",
        ]
        .map(String::from);
        let (_, at) = worker.locate(&villages, [0, 64, 0], 100).unwrap();
        let start = worker
            .generate_proto(ChunkPos::new(at[0] >> 4, at[2] >> 4))
            .unwrap();
        assert!(!start.own_starts().is_empty());
        // TerraForged's decoration plants trees in a wooded biome near the origin.
        let wooded = [
            "minecraft:forest",
            "minecraft:birch_forest",
            "minecraft:flower_forest",
            "minecraft:old_growth_birch_forest",
            "minecraft:dark_forest",
            "minecraft:taiga",
            "minecraft:old_growth_spruce_taiga",
            "minecraft:jungle",
        ];
        let forest = (-12..12)
            .flat_map(|cx| (-12..12).map(move |cz| ChunkPos::new(cx * 4, cz * 4)))
            .find(|&at| {
                let chunk = worker.generate_proto(at).unwrap();
                let top = chunk.surface_height(8, 8);
                wooded.contains(&chunk.biome_at(8, top, 8))
            })
            .expect("a wooded chunk near the origin");
        let mut region: [ProtoChunk; 9] = std::array::from_fn(|i| {
            worker
                .generate_proto(ChunkPos::new(
                    forest.x + i as i32 % 3 - 1,
                    forest.z + i as i32 / 3 - 1,
                ))
                .unwrap()
        });
        let stats = worker.decorate(&mut region).unwrap();
        assert!(stats.placed > 0);
        let logs = (0..16)
            .flat_map(|x| (0..16).map(move |z| (x, z)))
            .flat_map(|(x, z)| (40..200).map(move |y| (x, y, z)))
            .filter(|&(x, y, z)| region[4].block_name(x, y, z).ends_with("_log"))
            .count();
        assert!(logs > 0, "no tree logs in the decorated chunk");
    }

    /// Tree blocks per chunk of the middle 3x3 of a fresh 5x5, decorated in `order`.
    fn decorated_trees(
        data: &Arc<GeneratorData>,
        order: &[(i32, i32)],
        threads: usize,
    ) -> std::collections::BTreeMap<(i32, i32), usize> {
        let overworld = OverworldGenerator::for_world(
            Arc::clone(data),
            777,
            PRESET,
            "minecraft:overworld",
            true,
        )
        .unwrap();
        let mut worker = overworld.worker();
        let mut protos: std::collections::BTreeMap<
            (i32, i32),
            crate::worldgen::generator::ProtoChunk,
        > = std::collections::BTreeMap::new();
        let all: Vec<(i32, i32)> = (-2..=2)
            .flat_map(|x| (-2..=2).map(move |z| (x, z)))
            .collect();
        if threads <= 1 {
            for &(x, z) in &all {
                protos.insert((x, z), worker.generate_proto(ChunkPos::new(x, z)).unwrap());
            }
        } else {
            let made = std::sync::Mutex::new(Vec::new());
            std::thread::scope(|scope| {
                for t in 0..threads {
                    let (overworld, all, made) = (&overworld, &all, &made);
                    scope.spawn(move || {
                        let mut worker = overworld.worker();
                        for &(x, z) in all.iter().skip(t).step_by(threads) {
                            let chunk = worker.generate_proto(ChunkPos::new(x, z)).unwrap();
                            made.lock().unwrap().push(((x, z), chunk));
                        }
                    });
                }
            });
            protos.extend(made.into_inner().unwrap());
        }
        if threads == 0 {
            worker = overworld.worker();
        }
        for &(cx, cz) in order {
            let keys: Vec<(i32, i32)> = (0..9).map(|i| (cx + i % 3 - 1, cz + i / 3 - 1)).collect();
            let region: Vec<_> = keys.iter().map(|k| protos.remove(k).unwrap()).collect();
            let mut region: [_; 9] = region.try_into().ok().unwrap();
            worker.decorate(&mut region).unwrap();
            for (key, chunk) in keys.into_iter().zip(region) {
                protos.insert(key, chunk);
            }
        }
        let mut trees = std::collections::BTreeMap::new();
        for &(cx, cz) in order {
            let chunk = &protos[&(cx, cz)];
            let mut n = 0;
            for x in 0..16 {
                for z in 0..16 {
                    for y in 40..400 {
                        let name = chunk.block_name(x, y, z);
                        n += usize::from(name.ends_with("_log") || name.ends_with("_leaves"));
                    }
                }
            }
            trees.insert((cx, cz), n);
        }
        trees
    }

    #[test]
    #[ignore = "decorates 9 chunks three times (about a minute)"]
    fn deterministic_decoration_ignores_threads_and_workers() {
        let Some(data) = data() else { return };
        let forward: Vec<(i32, i32)> = (-1..=1)
            .flat_map(|z| (-1..=1).map(move |x| (x, z)))
            .collect();
        super::terraforged_settings::set_for_thread(Some(super::terraforged_settings::Settings {
            deterministic: true,
            ..super::terraforged_settings::FAITHFUL
        }));
        let single = decorated_trees(&data, &forward, 1);
        let threaded = decorated_trees(&data, &forward, 6);
        let cold = decorated_trees(&data, &forward, 0);
        super::terraforged_settings::set_for_thread(None);
        assert_eq!(single, threaded);
        assert_eq!(single, cold);
        // The settled river noise keeps TerraForged's forests.
        assert!(single.values().sum::<usize>() > 2000, "{single:?}");
    }

    #[test]
    fn deterministic_terrain_data_ignores_threads() {
        let preset: Value =
            serde_json::from_str(super::super::terraforged_world_preset::preset_json()).unwrap();
        let levels =
            terrain_levels(&preset["dimensions"]["minecraft:overworld"]["generator"]["levels"])
                .unwrap();
        let summary = |g: &TerraForgedGenerator, x: i32, z: i32| {
            let handle = g.chunk_data(x, z);
            let d = handle.read();
            let mut v = Vec::new();
            for lx in 0..16 {
                for lz in 0..16 {
                    v.push((
                        d.height_at(lx, lz),
                        d.river().get(lx, lz).to_bits(),
                        d.normalized_gradient(lx, lz, 1.0).to_bits(),
                        d.base_height_at(lx, lz),
                    ));
                }
            }
            v
        };
        let all: Vec<(i32, i32)> = (-2..=2)
            .flat_map(|x| (-2..=2).map(move |z| (x, z)))
            .collect();
        super::terraforged_settings::set_for_thread(Some(super::terraforged_settings::Settings {
            deterministic: true,
            ..super::terraforged_settings::FAITHFUL
        }));
        let single = build_noise(levels).unwrap();
        single.capture_seed(777);
        let a: Vec<_> = all.iter().map(|&(x, z)| summary(&single, x, z)).collect();
        let threaded = build_noise(levels).unwrap();
        threaded.capture_seed(777);
        let made = std::sync::Mutex::new(std::collections::BTreeMap::new());
        std::thread::scope(|scope| {
            for t in 0..6 {
                let (g, all, made) = (&threaded, &all, &made);
                scope.spawn(move || {
                    for &(x, z) in all.iter().skip(t).step_by(6) {
                        let s = summary(g, x, z);
                        made.lock().unwrap().insert((x, z), s);
                    }
                });
            }
        });
        let made = made.into_inner().unwrap();
        let mut bad = 0;
        for (i, k) in all.iter().enumerate() {
            let diff = a[i].iter().zip(&made[k]).filter(|(p, q)| p != q).count();
            if diff > 0 {
                bad += 1;
                eprintln!(
                    "{k:?}: {diff} columns differ, e.g. {:?} vs {:?}",
                    a[i].iter().zip(&made[k]).find(|(p, q)| p != q).unwrap().0,
                    a[i].iter().zip(&made[k]).find(|(p, q)| p != q).unwrap().1
                );
            }
        }
        super::terraforged_settings::set_for_thread(None);
        assert_eq!(bad, 0, "terrain data differs between threads");
    }

    /// Seed 98765's chunk (-9, -128) holds a unique cave the original fills with
    /// `the_end`; corrected biomes give it an overworld cave biome.
    #[test]
    fn corrected_unique_caves_stay_in_the_overworld() {
        let Some(data) = data() else { return };
        let biomes = |corrected| {
            super::terraforged_settings::set_for_thread(Some(
                super::terraforged_settings::Settings {
                    corrected_biomes: corrected,
                    ..super::terraforged_settings::FAITHFUL
                },
            ));
            let overworld = OverworldGenerator::for_world(
                Arc::clone(&data),
                98765,
                PRESET,
                "minecraft:overworld",
                true,
            )
            .unwrap();
            super::terraforged_settings::set_for_thread(None);
            let chunk = overworld
                .worker()
                .generate_proto(ChunkPos::new(-9, -128))
                .unwrap();
            let mut names = std::collections::BTreeSet::new();
            for y in (-64..64).step_by(4) {
                for x in (0..16).step_by(4) {
                    for z in (0..16).step_by(4) {
                        names.insert(chunk.biome_at(x, y, z).to_owned());
                    }
                }
            }
            names
        };
        let faithful = biomes(false);
        assert!(faithful.contains("minecraft:the_end"), "{faithful:?}");
        let corrected = biomes(true);
        assert!(!corrected.contains("minecraft:the_end"), "{corrected:?}");
        assert!(
            corrected.iter().any(|name| [
                "minecraft:lush_caves",
                "minecraft:dripstone_caves",
                "minecraft:deep_dark"
            ]
            .contains(&name.as_str())),
            "{corrected:?}"
        );
    }

    /// `/locate biome` finds a biome only caves hold (seed 98765's unique caves hold
    /// `the_end`; `terraforged:cave` comes with the mod's pack, not this bundle), and
    /// the chunk generated there holds it near the answer.
    #[test]
    fn locate_finds_cave_biomes() {
        let Some(data) = data() else { return };
        let overworld = OverworldGenerator::for_world(
            Arc::clone(&data),
            98765,
            PRESET,
            "minecraft:overworld",
            true,
        )
        .unwrap();
        let mut worker = overworld.worker();
        let allowed = |name: &str| name == "minecraft:the_end";
        let (name, [x, y, z]) = worker
            .find_closest_biome(&BiomeSearch {
                allowed: &allowed,
                origin: [0, 64, 0],
                radius: 6400,
                horizontal: 32,
                vertical: 64,
                min_y: -64,
                max_y: 959,
            })
            .expect("a cave biome nearby");
        assert_eq!(name, "minecraft:the_end");
        let chunk = worker
            .generate_proto(ChunkPos::new(x >> 4, z >> 4))
            .unwrap();
        let near =
            (-8..=8).any(|dy| chunk.biome_at(x & 15, y + dy * 4, z & 15) == "minecraft:the_end");
        assert!(near, "no cave biome near {x} {y} {z}");
    }
}
