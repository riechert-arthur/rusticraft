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
// `NoiseBasedChunkGenerator`'s: TerraForged 6dd607eb's `Generator`
// (https://github.com/TerraForged/TerraForged/tree/6dd607ebfd41a3b7274e090b1f2cf565d53d1d44, MIT License,
// Copyright (c) 2021 TerraForged): `createBiomes`, `fillFromNoise`, `buildSurface` (`SurfaceDecorator.decorate`
// over `NoiseChunkUtil.getNoiseChunk`, then `decoratePost`'s `Surface.apply`), `applyCarvers`
// (`NoiseCaveGenerator.carve`), the `drop` at the end of `applyBiomeDecoration`, `getSeaLevel` and
// `Source.getNoiseBiome`. Stages TerraForged does not replace yet run on
// the vanilla overworld pipeline it wraps (`VanillaGen`).

use super::super::generator_structures::{Starts, StructureState};
use super::{
    BiomeChunk, BiomeId, BiomeRegion, BiomeSearch, Biomes, DecorateStats, GeneratorData,
    GeneratorError, JigsawPieces, JigsawQuery, LodColumn, NoiseSettings, OverworldGenerator,
    Pipeline, PreLightChunkStage, ProtoChunk, SeededPipeline, SurfaceInputs, SurfaceQueries,
    SurfaceSample, TerrainChunk, UniformSurfaceBiomes, WorkerState, WorldGenerationContext,
    build_surface, overrides, resource, stage,
    terraforged_biome_registry::identifier_order,
    terraforged_cave_assets::current_caves,
    terraforged_cave_biome_sampler::CaveBiomeSampler,
    terraforged_cave_carver::NoiseCaveGenerator,
    terraforged_current_surface_biomes::CurrentSurfaceBiomeSampler,
    terraforged_generator_facade::TerraForgedGenerator,
    terraforged_generator_preset::{build as build_noise, terrain_levels},
    terraforged_live_biomes::LiveBiomeBindings,
    terraforged_structure_queries::TfQueries,
    terraforged_structure_terrain::StructureTerrain,
    terraforged_surface_bindings::SurfaceBindings,
    terraforged_surface_height_cache::column_key,
    terraforged_world_preset::{GENERATOR_TYPE, PRESET},
};
use crate::heightmap::HeightmapType;
use crate::registry::BlockStateId;
use horizon_core::biome::obfuscate_seed;
use horizon_core::density::{DensitySampler, SampleError, SamplerContext};
use horizon_core::noise::DensityVolume;
use horizon_core::position::ChunkPos;
use horizon_core::random::XoroshiroRandom;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub(super) struct TerraForgedPipeline {
    /// `VanillaGen`: the `minecraft:overworld` noise settings' surface rule, fluids, features and carvers, with
    /// the vanilla overworld's biome source. It runs without structures: TerraForged's own placement
    /// ([`StructureState`] over TerraForged's heights and biomes) replaces them.
    pub(super) vanilla: Pipeline<XoroshiroRandom>,
    pub(super) generator: TerraForgedGenerator,
    biomes: LiveBiomeBindings,
    surface: SurfaceBindings,
    /// `ChunkUtil.FILLER` and the water `fillChunk` places.
    stone: BlockStateId,
    water: BlockStateId,
    /// The air `NoiseCaveCarver.carve` places.
    air: BlockStateId,
    caves: NoiseCaveGenerator<BiomeId>,
    /// `ChunkGenerator`'s structure placement, when the world generates structures.
    structures: Option<StructureState>,
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
    let stem = if preset == PRESET {
        terraforged_stem(&data, preset, dimension)
    } else {
        None
    };
    let Some(stem) = stem else {
        return OverworldGenerator::build(data, seed, preset, dimension, with_structures);
    };
    Ok(OverworldGenerator {
        pipeline: Arc::new(TerraForgedPipeline::new(&data, seed, &stem, with_structures)?),
        noise_clamped: None,
    })
}

/// `GeneratorData::prepare_decoration` (the world screens' preload) for TerraForged's preset: its overworld stem has
/// no `biome_source`, and a TerraForged overworld decorates as its vanilla half does, with the `minecraft:normal`
/// overworld's biome source ([`vanilla_overworld`]), so that decoration is the one prepared. Other presets and
/// dimensions, and a data pack's own overworld, go to Horizon's body.
pub(super) fn prepare_decoration(
    data: &Arc<GeneratorData>,
    preset: &str,
    dimension: &str,
) -> std::ops::ControlFlow<Result<(), GeneratorError>> {
    if preset == PRESET && terraforged_stem(data, preset, dimension).is_some() {
        std::ops::ControlFlow::Break(
            data.prepare_decoration("minecraft:normal", "minecraft:overworld"),
        )
    } else {
        std::ops::ControlFlow::Continue(())
    }
}

/// `dimension`'s level stem, when it uses TerraForged's generator. A data pack's stem
/// replaces the preset's, as `OverworldGenerator::build` and 26.3's
/// `WorldDimensions.bake` pick it, so a pack's own overworld keeps its generator, and a
/// pack's TerraForged stem brings its own `levels`.
fn terraforged_stem(data: &GeneratorData, preset: &str, dimension: &str) -> Option<Value> {
    let stem = match data.dimensions.get(dimension) {
        Some(json) => serde_json::from_str::<Value>(json).ok(),
        None => data
            .world_presets
            .get(preset)
            .and_then(|json| serde_json::from_str::<Value>(json).ok())
            .map(|preset| preset["dimensions"][dimension].clone()),
    };
    stem.filter(|stem| stem["generator"]["type"].as_str() == Some(GENERATOR_TYPE))
}

impl TerraForgedPipeline {
    /// `Generator.CODEC` over `stem` (its `levels`, then `GeneratorPreset.build`) with the level seed captured as
    /// `createState` does, the registry biome bindings and the surface passes' block bindings. The build range is
    /// the stem's dimension type's; TerraForged's levels are not mapped onto it: the fill clips them to it.
    fn new(
        data: &Arc<GeneratorData>,
        seed: i64,
        stem: &Value,
        with_structures: bool,
    ) -> Result<Self, GeneratorError> {
        let levels = terrain_levels(stem["generator"].get("levels")).map_err(resource)?;
        let generator = build_noise(levels).map_err(resource)?;
        generator.capture_seed(seed);
        let biomes = LiveBiomeBindings::from_generator_data(data, &[])
            .map_err(|error| resource(error.to_string()))?;
        let (_, block_tags) = super::super::block_feature::FeatureBlockBindings::bind_tags(
            data.registry,
            &data.block_tags,
        )?;
        let surface = SurfaceBindings::new(data.registry, &block_tags)
            .map_err(|error| resource(error.to_string()))?;
        let block = |name: &str| {
            data.registry
                .default_state(name)
                .ok_or_else(|| resource(format!("missing block {name}")))
        };
        let level = overrides::LevelHeight::of_type(&stem["type"], &data.dimension_types)
            .map(|height| (height.min_y, height.height));
        // CaveBiomeSampler(800, biomeMapManager): GLOBAL caves use terraforged:cave (not registered in Horizon yet, so
        // they keep their biomes); UNIQUE caves draw from every registered biome (6dd607eb's UNDERGROUND filter is
        // commented out), in 26.3's registry order: path, then namespace.
        let global = data
            .biome_id("terraforged:cave")
            .map(Arc::new)
            .into_iter()
            .collect();
        let mut special: Vec<_> = (0..data.biome_names().len() as u32).map(BiomeId).collect();
        special.sort_by(|a, b| identifier_order(data.biome_name(*a), data.biome_name(*b)));
        let special = special.into_iter().map(Arc::new).collect();
        let caves = NoiseCaveGenerator::new(
            current_caves().map_err(resource)?,
            CaveBiomeSampler::new(800, global, special),
            seed,
        );
        // ChunkGenerator's structure placement over Source.possibleBiomes (the overworld
        // biome list).
        let structures = if with_structures {
            let possible = biomes.map.overworld.iter().map(|id| **id).collect();
            StructureState::new(data, &possible, data.rings_for(seed, &stem["generator"]))?
        } else {
            None
        };
        Ok(Self {
            vanilla: vanilla_overworld(data, seed, level)?,
            generator,
            biomes,
            surface,
            stone: block("minecraft:stone")?,
            water: block("minecraft:water")?,
            air: block("minecraft:air")?,
            caves,
            structures,
        })
    }

    /// 26.3's `new WorldGenerationContext(generator, level)`: the higher of the two minimum heights, the lower of
    /// the level's height and `getGenDepth()` (480 by default), and the generator's sea level.
    fn generation(&self) -> WorldGenerationContext {
        let settings = &self.vanilla.settings;
        WorldGenerationContext::new(
            settings.level_min_y.max(self.generator.min_y()),
            settings.level_height.min(self.generator.gen_depth()),
            self.generator.sea_level(),
        )
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

    /// `pos`'s biomes, as `createBiomes` fills them. Only the chunk being generated hints the terrain cache
    /// (`createBiomes` runs once per chunk in Java); its neighbours' biomes, which the surface rules read, are
    /// sampled again without the hint, so a neighbour whose terrain was already dropped is not generated again.
    fn biome_chunk(
        &self,
        surface: &CurrentSurfaceBiomeSampler<'_, BiomeId>,
        pos: ChunkPos,
        hint: bool,
    ) -> Result<BiomeChunk<BiomeId>, GeneratorError> {
        let settings = &self.vanilla.settings;
        let mut chunk =
            BiomeChunk::new(pos, settings.level_min_y, settings.level_height, BiomeId(0))
                .map_err(stage("biomes"))?;
        if hint {
            self.biomes
                .create_biomes(&self.generator, &mut chunk, pos, surface)
                .map_err(stage("biomes"))?;
        } else {
            self.generator
                .sample_chunk_biomes(&mut chunk, pos, surface, |replacement| {
                    self.biomes.override_biome(replacement)
                })
                .map_err(stage("biomes"))?;
        }
        Ok(chunk)
    }
}

/// `VanillaGen`'s pipeline: the `minecraft:normal` preset's overworld generator (its biome source) over the
/// `minecraft:overworld` noise settings (a data pack's over the vanilla ones, as `getHolderOrThrow` reads them),
/// in `level`, the TerraForged stem's build range, clamped to it as `OverworldGenerator::build` does
/// (`NoiseSettings.clampToHeightAccessor`), without structures. Always Xoroshiro: TerraForged's generator is no
/// `NoiseBasedChunkGenerator`, so 26.3's `ChunkMap` builds its `RandomState` (whose surface system runs the rule) with
/// the legacy random source off, stone and sea level 63, whatever the noise settings say. (Here the surface
/// system's default block and sea level come from the overworld settings: the same unless a data pack changes them.)
fn vanilla_overworld(
    data: &Arc<GeneratorData>,
    seed: i64,
    level: Option<(i32, i32)>,
) -> Result<Pipeline<XoroshiroRandom>, GeneratorError> {
    let preset: Value = serde_json::from_str(
        data.world_presets
            .get("minecraft:normal")
            .ok_or_else(|| resource("missing world preset minecraft:normal"))?,
    )?;
    let generator = preset["dimensions"]["minecraft:overworld"]["generator"].clone();
    let mut settings: Value = serde_json::from_str(
        data.settings
            .get("minecraft:overworld")
            .ok_or_else(|| resource("missing noise settings minecraft:overworld"))?,
    )?;
    if let Some((level_min_y, level_height)) = level
        && let (Some(min_y), Some(height)) = (
            settings["noise"]["min_y"].as_i64(),
            settings["noise"]["height"].as_i64(),
        )
    {
        let clamped = NoiseSettings::new(min_y as i32, height as i32)
            .clamp_to_height(level_min_y, level_height);
        settings["noise"]["min_y"] = clamped.min_y.into();
        settings["noise"]["height"] = clamped.height.into();
    }
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

/// `NoiseChunkUtil.initSurfaceCache`'s cache as the vanilla surface rules read it (`NoiseChunk`'s preliminary
/// surface level): TerraForged's height at each quart column of the chunk, and the chunk's lowest one over the
/// 3x3 chunks around it. Heights are clipped to the chunk's build range, as the terrain fill clips the terrain, so
/// a plateau cut off at the top still gets its surface. A column outside the cache (Java would ask the vanilla
/// noise router; the surface stage never reaches one) gets the chunk's lowest height.
#[derive(Debug)]
struct PreliminarySurface {
    heights: BTreeMap<i64, i32>,
    fallback: i32,
}

impl PreliminarySurface {
    fn new(mut heights: BTreeMap<i64, i32>, min_y: i32, height: i32, sea_level: i32) -> Self {
        let top = min_y.saturating_add(height).saturating_sub(1);
        for value in heights.values_mut() {
            *value = (*value).clamp(min_y, top.max(min_y));
        }
        let fallback = heights.values().copied().min().unwrap_or(sea_level);
        Self { heights, fallback }
    }
}

impl DensitySampler for PreliminarySurface {
    fn sample_value(
        &self,
        _context: &mut SamplerContext,
        position: [i32; 3],
    ) -> Result<f32, SampleError> {
        let key = column_key(position[0] & !3, position[2] & !3);
        Ok(*self.heights.get(&key).unwrap_or(&self.fallback) as f32)
    }
}

impl SeededPipeline for TerraForgedPipeline {
    /// TerraForged's stages up to `applyCarvers`: `createBiomes`, `fillFromNoise` (`fillChunk` and
    /// `primeHeightmaps`), `buildSurface` (the vanilla surface rules over TerraForged's preliminary heights, then
    /// `Surface.apply`) and `applyCarvers` (the noise caves), in 26.3's order: its single `buildTerrain` runs fill,
    /// surface and carvers on a BIOMES chunk, so every write updates the WG heightmaps. Structures are not run yet.
    fn generate(
        &self,
        worker: &mut WorkerState,
        pos: ChunkPos,
    ) -> Result<ProtoChunk, GeneratorError> {
        let vanilla = &self.vanilla;
        let data = &vanilla.data;
        let (min, height) = (vanilla.settings.level_min_y, vanilla.settings.level_height);
        // createBiomes, and the neighbours' biomes the surface rules look into. The sampler holds the pools and the
        // climate noise's few modules: a few microseconds to build, so one per chunk. Sampling the 8 neighbours
        // again (Java reads the biomes their own createBiomes stored) costs about 2% of the chunk.
        let sampler = self.biomes.surface_sampler(&self.generator);
        let mut neighbors = Vec::with_capacity(9);
        let mut seen = BTreeSet::new();
        for i in 0..9 {
            let around =
                ChunkPos::new(pos.x.wrapping_add(i % 3 - 1), pos.z.wrapping_add(i / 3 - 1));
            let chunk = self.biome_chunk(&sampler, around, i == 4)?;
            let mut section = 0;
            while let Some(biomes) = chunk.section(section) {
                seen.extend(biomes.iter().copied());
                section += 1;
            }
            neighbors.push(chunk);
        }
        let possible = seen
            .into_iter()
            .map(|id| data.biome_name(id).to_owned())
            .collect();
        let mut center = neighbors[4].clone();
        let region =
            BiomeRegion::new(neighbors, obfuscate_seed(vanilla.seed)).map_err(stage("biomes"))?;

        // fillFromNoise: the fill primes OCEAN_FLOOR_WG and WORLD_SURFACE_WG as `primeHeightmaps` does.
        let mut chunk = TerrainChunk::new(pos, min, height, data.registry, data.states.clone())
            .map_err(stage("noise"))?;
        chunk.set_generation_stage(PreLightChunkStage::Biomes);
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
                self.stone,
                self.water,
                self.air,
                &adaptation,
            )
            .map_err(stage("noise"))?;

        // buildSurface: SurfaceDecorator.decorate, then decoratePost.
        let generation = self.generation();
        let mut heights = BTreeMap::new();
        self.generator.initialize_surface_cache(pos, &mut heights);
        let preliminary: Arc<dyn DensitySampler> = Arc::new(PreliminarySurface::new(
            heights,
            min,
            height,
            generation.sea_level,
        ));
        let volume = DensityVolume::new(
            [16, vanilla.settings.noise_height, 16],
            [
                pos.x.wrapping_shl(4),
                vanilla.settings.noise_min_y,
                pos.z.wrapping_shl(4),
            ],
            [1; 3],
        )
        .map_err(stage("surface"))?;
        let write_max_y = generation
            .min_y
            .saturating_add(generation.height)
            .saturating_sub(1)
            .min(min.saturating_add(height).saturating_sub(1));
        let mut context = std::mem::take(&mut worker.terrain_context);
        let surface = build_surface(
            &mut chunk,
            vanilla.rule.clone(),
            SurfaceInputs::<XoroshiroRandom, _> {
                generation,
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
                write_min_y: generation.min_y,
                write_max_y,
            },
            &vanilla.extensions,
        );
        // Release chunk-specific bindings and caches even after a sampling error.
        context.reset();
        worker.terrain_context = context;
        surface.map_err(stage("surface"))?;
        self.surface
            .apply(
                &terrain.read(),
                &mut chunk,
                self.generator.gen_depth(),
                self.generator.sea_level(),
            )
            .map_err(stage("surface"))?;

        // applyCarvers: TerraForged's noise caves with `(int)` of the level seed; no vanilla carvers run (6dd607eb's
        // `applyCarvers` replaces them). The CarverChunk (the biomes each cave passed through, which
        // `NoiseCaveDecorator` reads) is not kept: TerraForged's decoration is not wired yet.
        self.caves
            .carve(
                self.generator.level_seed() as i32,
                &mut chunk,
                &mut center,
                &terrain.read(),
                self.generator.min_y(),
                self.generator.sea_level(),
                self.air,
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
    /// The vanilla decoration, then `applyBiomeDecoration`'s `terrainCache.drop(Seeds.get(region.getSeed()), pos)`
    /// for the decorated chunk, so the cache holds only chunks still being generated (Java drops it here too).
    fn decorate(
        &self,
        worker: &mut WorkerState,
        region: &mut [ProtoChunk; 9],
    ) -> Result<DecorateStats, GeneratorError> {
        let stats = SeededPipeline::decorate(&self.vanilla, worker, region)?;
        let center = region[4].position();
        self.generator
            .drop_chunk(self.generator.seed(), center.x, center.z);
        Ok(stats)
    }
    fn data(&self) -> &Arc<GeneratorData> {
        self.vanilla.data()
    }
    fn end_island_origins(&self, _chunk: ChunkPos) -> Vec<[i32; 3]> {
        Vec::new()
    }
    /// `getSeaLevel()`: `levels.seaLevel` (62 by default).
    fn sea_level(&self) -> i32 {
        self.generator.sea_level()
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
        worker: &mut WorkerState,
        search: &BiomeSearch<'_>,
    ) -> Option<(String, [i32; 3])> {
        self.vanilla.find_closest_biome(worker, search)
    }
    /// `Source.getNoiseBiome(x, y, z)` per quart: `sampleBiome(seed, x << 2, z << 2)`, whatever `y` (no cave
    /// layer), with the captured seed and without a terrain-cache hint. `None` where the column's pool has no
    /// biome (Java's `sampleBiome` returns null there).
    fn noise_biomes(&self, _worker: &mut WorkerState, quarts: &[[i32; 3]]) -> Vec<Option<String>> {
        let data = self.vanilla.data();
        let sampler = self.biomes.surface_sampler(&self.generator);
        let seed = self.generator.seed();
        quarts
            .iter()
            .map(|quart| {
                sampler
                    .sample_biome(
                        seed,
                        quart[0].wrapping_shl(2),
                        quart[2].wrapping_shl(2),
                        |replacement| self.biomes.override_biome(replacement),
                    )
                    .ok()
                    .map(|biome| data.biome_name(*biome).to_owned())
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
    }

    #[test]
    fn the_preload_prepares_the_decoration_a_terraforged_overworld_uses() {
        let Some(data) = data() else { return };
        // Horizon's own body finds no biome source in the preset's overworld.
        assert!(
            prepare_decoration(&data, PRESET, "minecraft:overworld").is_break(),
            "the TerraForged overworld is handled"
        );
        data.prepare_decoration(PRESET, "minecraft:overworld")
            .unwrap();
        let normal: Value = serde_json::from_str(&data.world_presets["minecraft:normal"]).unwrap();
        let source =
            normal["dimensions"]["minecraft:overworld"]["generator"]["biome_source"].to_string();
        let prepared = |key: &str| {
            let cache = data.decorations.lock().unwrap();
            cache
                .iter()
                .any(|(json, slot)| json == key && slot.get().is_some_and(Result::is_ok))
        };
        assert!(
            prepared(&source),
            "the vanilla overworld's decoration, which the pipeline reuses"
        );
        // The preset's nether is vanilla's: Horizon's body prepares it.
        assert!(prepare_decoration(&data, PRESET, "minecraft:the_nether").is_continue());
        data.prepare_decoration(PRESET, "minecraft:the_nether")
            .unwrap();
        assert!(prepare_decoration(&data, "minecraft:normal", "minecraft:overworld").is_continue());
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
        assert!(terraforged_stem(&data, PRESET, "minecraft:overworld").is_none());
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
    fn a_data_pack_terraforged_overworld_brings_its_levels() {
        // A pack's TerraForged stem wins over the preset's, `levels` and all (here a sea level of 40).
        let mut ov = crate::worldgen::overrides::WorldgenOverrides::default();
        let stem = r#"{"type":"minecraft:overworld","generator":{"type":"terraforged:generator","levels":{"min_y":-64,"max_y":480,"base_height":128,"sea_level":40,"sea_floor":22}}}"#;
        ov.set(
            crate::worldgen::overrides::DIMENSIONS,
            "minecraft:overworld",
            stem.to_owned(),
        );
        let Some(data) = data_with(&ov) else { return };
        let overworld =
            OverworldGenerator::for_dimension(Arc::clone(&data), 7, PRESET, "minecraft:overworld")
                .unwrap();
        assert_eq!(overworld.sea_level(), 40);
    }

    #[test]
    fn the_terraforged_overworld_keeps_the_normal_build_range() {
        // Only the build range is still the vanilla overworld's (TerraForged's levels are clipped to it); the
        // blocks and biomes are TerraForged's now (see `terraforged_chunks_are_terraforged_and_repeatable`).
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
        assert_eq!((terraforged.sea_level(), normal.sea_level()), (62, 63));
    }

    #[test]
    fn terraforged_chunks_are_terraforged_and_repeatable() {
        let Some(data) = data() else { return };
        let seed = -4_172_144_997_902_289_642;
        let overworld = || {
            OverworldGenerator::for_dimension(
                Arc::clone(&data),
                seed,
                PRESET,
                "minecraft:overworld",
            )
            .unwrap()
        };
        let (first, second) = (overworld(), overworld());
        let pos = ChunkPos::new(3, -2);
        let a = first.worker().generate_proto(pos).unwrap();
        let again = first.worker().generate_proto(pos).unwrap();
        let b = second.worker().generate_proto(pos).unwrap();
        let normal = OverworldGenerator::for_dimension(
            Arc::clone(&data),
            seed,
            "minecraft:normal",
            "minecraft:overworld",
        )
        .unwrap()
        .worker()
        .generate_proto(pos)
        .unwrap();

        // The biomes are `createBiomes`'s (TerraForged's surface biome per quart column, at every height), except
        // where a noise cave wrote its biome: such a quart was carved, so it holds air.
        let quarts: Vec<[i32; 3]> = (0..16)
            .map(|i| [pos.x * 4 + i % 4, 0, pos.z * 4 + i / 4])
            .collect();
        let expected = second.worker().noise_biomes(&quarts);
        let (min_y, height) = first.level();
        let mut differs = false;
        for (i, biome) in expected.iter().enumerate() {
            let (x, z) = ((i as i32 % 4) * 4, (i as i32 / 4) * 4);
            for y in (min_y..min_y + height).step_by(4) {
                let id = a.biome_id_at(x, y, z);
                assert_eq!(id, again.biome_id_at(x, y, z), "rerun biome {x} {y} {z}");
                assert_eq!(id, b.biome_id_at(x, y, z), "second world biome {x} {y} {z}");
                if Some(data.biome_name(id)) != biome.as_deref() {
                    assert!(carved_quart(&a, x, y, z), "biome {x} {y} {z}");
                }
            }
        }
        for y in min_y..min_y + height {
            for z in 0..16 {
                for x in 0..16 {
                    let block = a.block(x, y, z);
                    assert_eq!(block, again.block(x, y, z), "rerun {x} {y} {z}");
                    assert_eq!(block, b.block(x, y, z), "second world {x} {y} {z}");
                    differs |= block != normal.block(x, y, z);
                }
            }
        }
        assert!(differs, "the chunk is still the vanilla overworld's");
    }

    /// Whether the quart at block `x y z` holds air (the noise caves carve with plain air).
    fn carved_quart(chunk: &ProtoChunk, x: i32, y: i32, z: i32) -> bool {
        (0..64).any(|i| chunk.block_name(x + i % 4, y + i / 16, z + i / 4 % 4) == "minecraft:air")
    }

    #[test]
    fn noise_caves_carve_the_same_chunk_the_same_way() {
        // The first chunks where unique caves form (the walk of the cave oracle, `cave_carver.rs`), in two worlds.
        let Some(data) = data() else { return };
        let seed = 7;
        let unique = super::super::terraforged_cave_noise::unique_cave_noise();
        let chunks: Vec<ChunkPos> = (0i32..)
            .map(|k| {
                let cx = k.wrapping_mul(7919).rem_euclid(4001) - 2000;
                let cz = k.wrapping_mul(104_729).rem_euclid(4001) - 2000;
                ChunkPos::new(cx, cz)
            })
            .filter(|pos| {
                let (x, z) = ((pos.x * 16 + 8) as f32, (pos.z * 16 + 8) as f32);
                unique.try_value_seed_first(seed as i32, x, z).unwrap() > 0.3
            })
            .take(4)
            .collect();
        let overworld = || {
            OverworldGenerator::for_dimension(
                Arc::clone(&data),
                seed,
                PRESET,
                "minecraft:overworld",
            )
            .unwrap()
        };
        let (first, second) = (overworld(), overworld());
        let (min_y, height) = first.level();
        let (mut cave_air, mut cave_quarts) = (0, 0);
        for pos in chunks {
            let a = first.worker().generate_proto(pos).unwrap();
            let again = first.worker().generate_proto(pos).unwrap();
            let b = second.worker().generate_proto(pos).unwrap();
            for z in 0..16 {
                for x in 0..16 {
                    let top = a.surface_height(x, z);
                    for y in min_y..min_y + height {
                        let block = a.block(x, y, z);
                        assert_eq!(block, again.block(x, y, z), "rerun {pos:?} {x} {y} {z}");
                        assert_eq!(block, b.block(x, y, z), "second world {pos:?} {x} {y} {z}");
                        cave_air +=
                            i32::from(y < top - 12 && a.block_name(x, y, z) == "minecraft:air");
                    }
                }
            }
            let quarts: Vec<[i32; 3]> = (0..16)
                .map(|i| [pos.x * 4 + i % 4, 0, pos.z * 4 + i / 4])
                .collect();
            let surface = first.worker().noise_biomes(&quarts);
            for (i, biome) in surface.iter().enumerate() {
                let (x, z) = ((i as i32 % 4) * 4, (i as i32 / 4) * 4);
                for y in (min_y..min_y + height).step_by(4) {
                    let id = a.biome_id_at(x, y, z);
                    assert_eq!(
                        id,
                        again.biome_id_at(x, y, z),
                        "rerun biome {pos:?} {x} {y} {z}"
                    );
                    assert_eq!(
                        id,
                        b.biome_id_at(x, y, z),
                        "second world biome {pos:?} {x} {y} {z}"
                    );
                    if Some(data.biome_name(id)) != biome.as_deref() {
                        assert!(carved_quart(&a, x, y, z), "biome {pos:?} {x} {y} {z}");
                        cave_quarts += 1;
                    }
                }
            }
        }
        // Without terraforged:cave only the unique caves write biomes (any registered biome).
        assert!(cave_air > 0 && cave_quarts > 0, "{cave_air} {cave_quarts}");
    }

    #[test]
    fn only_undecorated_chunks_stay_in_the_terrain_cache() {
        // `createBiomes` hints only its own chunk (the surface stage's neighbours are sampled without a hint), and
        // decorating a chunk drops its entry, as `applyBiomeDecoration` does.
        let Some(data) = data() else { return };
        let stem = terraforged_stem(&data, PRESET, "minecraft:overworld").unwrap();
        let pipeline = Arc::new(TerraForgedPipeline::new(&data, 11, &stem, false).unwrap());
        let overworld = OverworldGenerator {
            pipeline: Arc::clone(&pipeline) as Arc<dyn SeededPipeline>,
            noise_clamped: None,
        };
        let mut worker = overworld.worker();
        let center = ChunkPos::new(-7, 4);
        let first = worker.generate_proto(center).unwrap();
        assert_eq!(pipeline.generator.cached_chunks(), 1);
        let mut slots: Vec<Option<ProtoChunk>> = (0..9).map(|_| None).collect();
        slots[4] = Some(first);
        for (i, slot) in slots.iter_mut().enumerate() {
            if i != 4 {
                let (dx, dz) = (i as i32 % 3 - 1, i as i32 / 3 - 1);
                *slot = Some(
                    worker
                        .generate_proto(ChunkPos::new(center.x + dx, center.z + dz))
                        .unwrap(),
                );
            }
        }
        let mut region: [ProtoChunk; 9] = std::array::from_fn(|i| slots[i].take().unwrap());
        assert_eq!(pipeline.generator.cached_chunks(), 9);
        worker.decorate(&mut region).unwrap();
        assert_eq!(pipeline.generator.cached_chunks(), 8);
        // A neighbour generated again does not bring the decorated chunk back.
        worker
            .generate_proto(ChunkPos::new(center.x + 1, center.z))
            .unwrap();
        assert_eq!(pipeline.generator.cached_chunks(), 8);
    }
}
