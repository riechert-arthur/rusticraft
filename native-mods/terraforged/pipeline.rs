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
// (`NoiseCaveGenerator.carve`), `applyBiomeDecoration` (`BiomeGenerator.decorate`, in `decorator.rs`, then the
// terrain-cache `drop`), `getSeaLevel` and
// `Source.getNoiseBiome` and `possibleBiomes` (which 26.3's `findClosestBiome3d`, `/locate biome`, searches), and
// `ChunkGenerator`'s structure placement over `getBaseHeight`/`getBaseColumn`
// (`structure_queries.rs`). Stages TerraForged does not replace yet run on the vanilla overworld pipeline it wraps
// (`VanillaGen`).

use super::super::generator_decoration::terraforged_decorator::TerraForgedFeatures;
use super::super::generator_structures::{Starts, StructureState};
use super::{
    BiomeChunk, BiomeId, BiomeRegion, BiomeSearch, Biomes, DecorateStats, GeneratorData,
    GeneratorError, JigsawPieces, JigsawQuery, LodColumn, MaterialConditions,
    MaterialDensitySources, NoiseSettings, OverworldGenerator, Pipeline, PreLightChunkStage,
    ProtoChunk, SeededPipeline, SurfaceInputs, SurfaceQueries, SurfaceSample, TerrainChunk,
    UniformSurfaceBiomes, WorkerState, WorldGenerationContext, build_surface, overrides, resource,
    stage,
    terraforged_biome_registry::{identifier_order, registry_entries},
    terraforged_cave_assets::current_caves,
    terraforged_cave_biome_sampler::CaveBiomeSampler,
    terraforged_cave_carver::{CarverChunk, NoiseCaveGenerator},
    terraforged_current_surface_biomes::CurrentSurfaceBiomeSampler,
    terraforged_generator_facade::{
        TerraForgedGenerator, TerraHeightmap, register_live, shareable_generator,
    },
    terraforged_generator_preset::{build as build_noise, scale_heights, terrain_levels},
    terraforged_live_biomes::LiveBiomeBindings,
    terraforged_settings::{self, Settings},
    terraforged_structure_queries::TfQueries,
    terraforged_structure_terrain::StructureTerrain,
    terraforged_surface_bindings::SurfaceBindings,
    terraforged_surface_height_cache::column_key,
    terraforged_terrain_levels::TerrainLevels,
    terraforged_world_preset::{GENERATOR_TYPE, PRESET},
};
use crate::heightmap::HeightmapType;
use crate::registry::BlockStateId;
use horizon_core::biome::obfuscate_seed;
use horizon_core::density::{DensitySampler, SampleError, SamplerContext};
use horizon_core::noise::DensityVolume;
use horizon_core::position::{BlockPos, ChunkPos};
use horizon_core::random::XoroshiroRandom;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// How many blocks from the top down distant terrain paints with the surface rule: the eight vanilla's far tiles
/// paint, or two with Reduced far detail (the player's choice to draw far terrain coarser).
const FULL_PAINT: i32 = 8;
const REDUCED_PAINT: i32 = 2;

/// The widest far cells that read eroded heights with `eroded_far_terrain`: the cells of Horizon's finest far tiles
/// (`lod.rs`'s `DETAIL_LEVEL`, cells of up to 4 blocks), which it keeps within its far-decoration radius (about 1024
/// blocks around the player at Full far detail, 512 at Reduced) and which the chunks replace as the player walks.
/// Wider cells lie farther out, where erosion's few blocks don't show and eroding would cost a region per 128 blocks.
const ERODED_FAR_WIDTH: i32 = 4;

/// The name of Horizon's far-decoration threads (`vanillagen/lod.rs`), which run at idle priority.
const FAR_DECORATION_THREAD: &str = "Far decoration";

pub(super) struct TerraForgedPipeline {
    /// `VanillaGen`: the `minecraft:overworld` noise settings' surface rule, fluids, features and carvers, with
    /// the vanilla overworld's biome source. It runs without structures: TerraForged's own placement
    /// ([`StructureState`] over TerraForged's heights and biomes) replaces them.
    pub(super) vanilla: Pipeline<XoroshiroRandom>,
    /// Also in `terraforged_generator_facade::live_generator`'s registry (for `/locateterrain`).
    pub(super) generator: Arc<TerraForgedGenerator>,
    biomes: LiveBiomeBindings,
    surface: SurfaceBindings,
    /// `ChunkUtil.FILLER` and the water `fillChunk` places.
    stone: BlockStateId,
    water: BlockStateId,
    /// The air `NoiseCaveCarver.carve` places.
    air: BlockStateId,
    caves: NoiseCaveGenerator<BiomeId>,
    /// `NoiseCaveGenerator.cache`: each carved chunk's `CarverChunk` (its caves' biome lists), until the chunk is
    /// decorated. Like the terrain cache, an entry lives from the chunk's terrain stage to its decoration.
    carvers: Arc<Carvers>,
    /// Every biome's features, for TerraForged's decoration (`None` without feature resources).
    features: Option<TerraForgedFeatures>,
    /// `ChunkGenerator`'s structure placement, when the world generates structures.
    structures: Option<StructureState>,
    /// The blocks of `erosion_deposits` when the world has it on (`terraforged_deposits`).
    deposits: Option<super::terraforged_deposits::DepositBlocks>,
    /// The ground cover's blocks when the world has `realistic_vegetation` on (`terraforged_realistic_vegetation`).
    ground_cover: Option<super::terraforged_realistic_vegetation::CoverBlocks>,
    /// The bands' blocks when the world has `rock_strata` on (`terraforged_rock_strata`).
    strata: Option<super::terraforged_rock_strata::StrataBlocks>,
    /// Horizon's settings of the world (`terraforged_settings`) when the pipeline was built.
    settings: Settings,
}

type Carvers = std::sync::Mutex<std::collections::HashMap<ChunkPos, CarverChunk<BiomeId>>>;

/// What each live pipeline keeps per chunk between its terrain stage and its decoration (the level seed as an
/// int, the terrain cache's generator and the carver records), for [`release_finished`]. Entries die with their
/// pipeline: they hold no reference that keeps it alive.
type Held = Vec<(
    i32,
    std::sync::Weak<TerraForgedGenerator>,
    std::sync::Weak<Carvers>,
)>;
static HELD: std::sync::Mutex<Held> = std::sync::Mutex::new(Vec::new());

/// Records what `pipeline` keeps per chunk, for [`release_finished`].
fn hold(pipeline: &TerraForgedPipeline) {
    let mut held = HELD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    held.retain(|(_, generator, carvers)| {
        generator.strong_count() > 0 && carvers.strong_count() > 0
    });
    held.push((
        pipeline.vanilla.seed as i32,
        Arc::downgrade(&pipeline.generator),
        Arc::downgrade(&pipeline.carvers),
    ));
}

/// Horizon's edge decoration (`Streaming::regenerate`, `/tf regen`) generates finished chunks again to decorate
/// their regenerated neighbour against, and drops those copies without decorating them: what their terrain stage
/// left in the terrain cache and the carver records, which only a chunk's own decoration removes, goes here
/// instead. A finished chunk has no generation in progress that needs them; another pipeline of the world (the
/// far-terrain ones) that held such a chunk would only compute it again.
pub fn release_finished(chunks: &[(i32, i32)]) {
    if chunks.is_empty() {
        return;
    }
    let mut held = HELD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    held.retain(|(_, generator, carvers)| {
        generator.strong_count() > 0 && carvers.strong_count() > 0
    });
    for (seed, generator, carvers) in held.iter() {
        let (Some(generator), Some(carvers)) = (generator.upgrade(), carvers.upgrade()) else {
            continue;
        };
        let mut carvers = carvers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for &(x, z) in chunks {
            carvers.remove(&ChunkPos::new(x, z));
            generator.drop_chunk(*seed, x, z);
        }
    }
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
    let pipeline = live_pipeline(&data, seed, &stem, dimension, with_structures)?;
    hold(&pipeline);
    Ok(OverworldGenerator {
        pipeline: Arc::new(pipeline),
        noise_clamped: None,
    })
}

/// A TerraForged pipeline of `stem`, registered as live. Horizon's far-decoration threads each build a pipeline of the
/// level's world (`vanillagen/lod.rs`, which can't share the level's: its structure pieces' one-shot flags). A pipeline
/// of a world whose generator is already alive shares that root's noise and keeps its own caches
/// (`TerraForgedGenerator::sharing`), so the world has one TerraForged noise (graphs, climate, continent and river
/// caches) however many pipelines it runs; the first builds the noise and is the root `live_generator` answers with.
fn live_pipeline(
    data: &Arc<GeneratorData>,
    seed: i64,
    stem: &Value,
    dimension: &str,
    with_structures: bool,
) -> Result<TerraForgedPipeline, GeneratorError> {
    let settings = terraforged_settings::current();
    let levels = TerraForgedPipeline::levels(stem, settings)?;
    let root = shareable_generator(seed, dimension, levels, settings);
    let generator = match &root {
        Some(root) => Arc::new(root.sharing()),
        None => TerraForgedPipeline::generator(levels, seed)?,
    };
    let pipeline = TerraForgedPipeline::with_generator(
        data,
        seed,
        stem,
        with_structures,
        settings,
        generator,
    )?;
    // The world's far erosion thread (`eroded_far_terrain`) is started by the first far-decoration pipeline, on its
    // thread: a new thread inherits its creator's scheduling class on Linux (and its quality of service on macOS), so
    // it runs at idle priority as they do. Without far decoration nothing builds far regions and far tiles keep the
    // un-eroded heights.
    if pipeline.settings.eroded_far_terrain
        && std::thread::current().name() == Some(FAR_DECORATION_THREAD)
    {
        pipeline.generator.start_far_erosion();
    }
    register_live(
        seed,
        dimension,
        settings,
        root.is_none(),
        &pipeline.generator,
    );
    Ok(pipeline)
}

/// `GeneratorData::prepare_decoration` (the world screens' preload) for TerraForged's preset: its overworld stem has
/// no `biome_source`, so the decorations a TerraForged overworld builds are the ones prepared: its vanilla half's
/// (the `minecraft:normal` overworld's biome source, [`vanilla_overworld`]) and TerraForged's own over every biome
/// ([`TerraForgedFeatures`]). Other presets and dimensions, and a data pack's own overworld, go to Horizon's body.
pub(super) fn prepare_decoration(
    data: &Arc<GeneratorData>,
    preset: &str,
    dimension: &str,
) -> std::ops::ControlFlow<Result<(), GeneratorError>> {
    if preset == PRESET && terraforged_stem(data, preset, dimension).is_some() {
        std::ops::ControlFlow::Break(
            data.prepare_decoration("minecraft:normal", "minecraft:overworld")
                .and_then(|()| TerraForgedFeatures::new(data).map(drop)),
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
    /// [`Self::with_generator`] over a new generator of its own.
    #[cfg(test)]
    fn new(
        data: &Arc<GeneratorData>,
        seed: i64,
        stem: &Value,
        with_structures: bool,
    ) -> Result<Self, GeneratorError> {
        let settings = terraforged_settings::current();
        let levels = Self::levels(stem, settings)?;
        Self::with_generator(
            data,
            seed,
            stem,
            with_structures,
            settings,
            Self::generator(levels, seed)?,
        )
    }

    /// The stem's `levels` (strict `TerrainLevels.CODEC`), scaled by the world's `height_scale`.
    fn levels(stem: &Value, settings: Settings) -> Result<TerrainLevels, GeneratorError> {
        Ok(scale_heights(
            terrain_levels(stem["generator"].get("levels")).map_err(resource)?,
            settings.height_scale,
        ))
    }

    /// `GeneratorPreset.build(levels)` with `createState`'s seed capture: a new generator over its own noise.
    fn generator(
        levels: TerrainLevels,
        seed: i64,
    ) -> Result<Arc<TerraForgedGenerator>, GeneratorError> {
        let generator = Arc::new(build_noise(levels).map_err(resource)?);
        generator.capture_seed(seed);
        Ok(generator)
    }

    /// `Generator.CODEC` over `stem` (its `levels`, then `GeneratorPreset.build`) with the level seed captured as
    /// `createState` does, the registry biome bindings and the surface passes' block bindings. The build range is
    /// the stem's dimension type's; TerraForged's levels are not mapped onto it: the fill clips them to it.
    /// `generator` is `GeneratorPreset.build`'s: a new one ([`Self::generator`]) or one sharing a live root's noise,
    /// built from the same stem and `settings` (the world's, read once by the caller so the generator's levels and
    /// the rest of the pipeline see the same ones).
    fn with_generator(
        data: &Arc<GeneratorData>,
        seed: i64,
        stem: &Value,
        with_structures: bool,
        settings: Settings,
        generator: Arc<TerraForgedGenerator>,
    ) -> Result<Self, GeneratorError> {
        let biomes =
            LiveBiomeBindings::from_generator_data_with(data, &[], settings.corrected_biomes)
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
        // CaveBiomeSampler(800, biomeMapManager): GLOBAL caves use terraforged:cave; UNIQUE caves draw from every
        // registered biome (6dd607eb's UNDERGROUND filter is commented out; the Nether's and the End's too), in
        // 26.3's registry order: path, then namespace. `corrected_biomes` keeps only the cave biomes.
        let global: Vec<_> = data
            .biome_id("terraforged:cave")
            .map(Arc::new)
            .into_iter()
            .collect();
        let mut special: Vec<_> = if settings.corrected_biomes {
            // TerraForged Reimagined a24d450's CaveBiomeSampler: the registry's `isNonSurface` biomes, or the
            // GLOBAL pool when there are none.
            let caves: Vec<_> = registry_entries(data)
                .iter()
                .filter(|entry| entry.is_non_surface())
                .map(|entry| *entry.value)
                .collect();
            if caves.is_empty() {
                global.iter().map(|id: &Arc<BiomeId>| **id).collect()
            } else {
                caves
            }
        } else {
            (0..data.biome_names().len() as u32).map(BiomeId).collect()
        };
        special.sort_by(|a, b| identifier_order(data.biome_name(*a), data.biome_name(*b)));
        let special = special.into_iter().map(Arc::new).collect();
        let caves = NoiseCaveGenerator::new(
            current_caves().map_err(resource)?,
            CaveBiomeSampler::new(800, global, special),
            seed,
        );
        // ChunkGenerator's structure placement over Source.possibleBiomes (the overworld biome list). The concentric
        // rings are cached per level seed and generator JSON: the stem's own, so they never mix with vanilla's.
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
            carvers: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            // Horizon's `realistic_vegetation` weighs every vegetation config's fitness by aspect and water.
            features: TerraForgedFeatures::new(data)?
                .map(|features| features.with_realistic_vegetation(settings.realistic_vegetation)),
            structures,
            // `erosion_deposits`' blocks (Horizon's setting); None without it.
            deposits: settings
                .erosion_deposits
                .then(|| super::terraforged_deposits::DepositBlocks::new(data.registry))
                .flatten(),
            ground_cover: settings
                .realistic_vegetation
                .then(|| super::terraforged_realistic_vegetation::CoverBlocks::new(data.registry))
                .flatten(),
            strata: settings
                .rock_strata
                .then(|| super::terraforged_rock_strata::StrataBlocks::new(data.registry))
                .flatten(),
            settings,
        })
    }

    /// 26.3's `new WorldGenerationContext(generator, level)`: the higher of the two minimum heights, the lower of
    /// the level's height and `getGenDepth()` (480 by default), and the generator's sea level.
    pub(super) fn generation(&self) -> WorldGenerationContext {
        let settings = &self.vanilla.settings;
        WorldGenerationContext::new(
            settings.level_min_y.max(self.generator.min_y()),
            settings.level_height.min(self.generator.gen_depth()),
            self.generator.sea_level(),
        )
    }

    /// The surface sampler [`Self::biome_at`] reads.
    pub(super) fn surface_sampler(&self) -> CurrentSurfaceBiomeSampler<'_, BiomeId> {
        self.biomes.surface_sampler(&self.generator)
    }

    /// The cave biome of `wanted` the noise caves leave in the column at block `x, z`, and a block of it, for
    /// `/locate biome` (see [`NoiseCaveGenerator::written_cave_biome`]). The cave biomes' noise comes first, then the
    /// caves' noise; only a column where a cave would carve a wanted biome reads its height: the un-eroded one first
    /// (no erosion region), then, if that passes, the eroded height the chunk is filled to, which builds the column's
    /// erosion region (a fraction of a second; the region the answer's chunks need) and answers. `None` where a noise
    /// fails, as everywhere in the search.
    fn cave_biome_at(&self, x: i32, z: i32, wanted: &BTreeSet<BiomeId>) -> Option<(BiomeId, i32)> {
        if wanted.is_empty() {
            return None;
        }
        let caves = self
            .caves
            .column_caves(self.generator.level_seed() as i32, x, z, |biome| {
                wanted.contains(biome)
            })
            .ok()??;
        if !caves
            .iter()
            .any(|cave| cave.biome().is_some_and(|biome| wanted.contains(biome)))
        {
            return None;
        }
        let (sea_level, min_y) = (self.generator.sea_level(), self.generator.min_y());
        let written = |first_free: i32| {
            NoiseCaveGenerator::written_cave_biome(&caves, first_free, sea_level, min_y, |biome| {
                wanted.contains(biome)
            })
        };
        // `far_heights`' terrain height is the top stone block; OCEAN_FLOOR_WG's first free block is above it.
        let (height, _) = self.generator.far_heights(x, z).ok()?;
        written(height.wrapping_add(1))?;
        written(
            self.generator
                .base_height(x, z, TerraHeightmap::OceanFloorWorldgen)
                .ok()?,
        )
    }

    /// `Source.getNoiseBiome` at block column `x z` (a quart's corner) with the captured seed and no terrain-cache
    /// hint, as [`SeededPipeline::noise_biomes`] samples it. `None` where the column's pool has no biome.
    pub(super) fn biome_at(
        &self,
        sampler: &CurrentSurfaceBiomeSampler<'_, BiomeId>,
        x: i32,
        z: i32,
    ) -> Option<BiomeId> {
        sampler
            .sample_biome(self.generator.seed(), x, z, |replacement| {
                self.biomes.override_biome(replacement)
            })
            .ok()
            .map(|biome| *biome)
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

    /// Horizon's far terrain (`SeededPipeline::lod_grid`; vanilla has no counterpart): each column is TerraForged's
    /// ground in `getBaseColumn`'s shape (stone from the level bottom to the terrain height, water above it to the
    /// surface, clipped to the level as the fill clips it), from the UN-ERODED height
    /// ([`TerraForgedGenerator::far_heights`]): the eroded one needs the column's erosion region, about 0.25 s to
    /// build per 256x256 blocks (a 16x16 tile of 64-block cells spans 64 regions: about 20 s), and the 16-region
    /// cache would then evict the regions the chunks being generated read. Erosion moves the ground by about 2 blocks
    /// on average (6 at the 90th percentile, at most about 10), so distant ground can sit that much off the real
    /// chunks'. Far sampling creates no terrain-cache entry and builds no region; it only shares the continent's
    /// lossy cell caches with the chunks, as `/locate biome` does. Its top `paint_depth` blocks ([`FULL_PAINT`], or
    /// [`REDUCED_PAINT`] with Reduced far detail) are painted by the vanilla surface rule as `buildSurface` paints a chunk: TerraForged's generation context (sea
    /// level 62), its preliminary height at the column, and the column's TerraForged biome (the one the chunk stores
    /// at the surface block's zoomed quart). Like vanilla's far sampling it skips the surface rule's slope reads and
    /// column extensions, and `Surface.apply`, the carvers and the decoration. A failed height or biome sample is an
    /// `Err` (the chunk would fail to generate there too).
    ///
    /// With `erode` (the `eroded_far_terrain` setting, in the finest tiles) a column reads the ERODED height instead
    /// where its erosion region is built already, so it stands exactly on the chunk's ground
    /// ([`TerraForgedGenerator::far_eroded_heights`]); an unbuilt region is queued for the world's far erosion thread,
    /// which builds it into a cache of its own, never the fill's, and the column keeps the un-eroded height.
    fn far_columns(
        &self,
        worker: &mut WorkerState,
        columns: &[(i32, i32)],
        paint_depth: i32,
        erode: bool,
    ) -> Result<Vec<LodColumn>, GeneratorError> {
        let vanilla = &self.vanilla;
        let (min_y, level_height) = (vanilla.settings.level_min_y, vanilla.settings.level_height);
        let top = min_y.saturating_add(level_height).saturating_sub(1);
        let zoom = horizon_core::biome::BiomeZoom::new(obfuscate_seed(vanilla.seed));
        let sampler = self.surface_sampler();
        let mut geometry = Vec::with_capacity(columns.len());
        // The preliminary heights per 4x4 cell, and each column's biome at every quart its zoomed lookups can reach
        // (`(y - 2) >> 2` and the next quart on each axis): the first column to claim a quart keeps it.
        let mut heights = BTreeMap::new();
        let mut quarts: BTreeMap<(i32, i32), BiomeId> = BTreeMap::new();
        let mut eroded = erode.then(|| self.generator.far_eroded_heights());
        for &(x, z) in columns {
            let (height, surface) = match &mut eroded {
                Some(eroded) => eroded.heights(x, z).map(|(heights, _)| heights),
                None => self.generator.far_heights(x, z),
            }
            .map_err(stage("far terrain"))?;
            heights.entry(column_key(x & !3, z & !3)).or_insert(height);
            // The stone and water tops, clipped as the fill clips them (cut flat at the top block).
            let stone = height.min(top);
            let water = surface.min(top);
            let shown = if stone >= min_y {
                stone
            } else {
                water.max(min_y)
            };
            let quart = zoom.quart_at_block(BlockPos::new(x, shown, z));
            let biome = self
                .biome_at(&sampler, quart[0].wrapping_shl(2), quart[2].wrapping_shl(2))
                .ok_or_else(|| resource("far biome: the column's pool has no biome"))?;
            let (qx, qz) = (x.wrapping_sub(2) >> 2, z.wrapping_sub(2) >> 2);
            for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                quarts
                    .entry((qx.wrapping_add(dx), qz.wrapping_add(dz)))
                    .or_insert(biome);
            }
            let mut spans = Vec::new();
            let water_bottom = stone.saturating_add(1).max(min_y);
            if water >= water_bottom {
                spans.push((water_bottom, water + 1, self.water));
            }
            if stone >= min_y {
                spans.push((min_y, stone + 1, self.stone));
            }
            let water_top = (water >= water_bottom).then_some((water + 1, self.water));
            let solid = (stone >= min_y).then_some((stone + 1, self.stone));
            geometry.push((solid, water_top, stone + 1 - min_y, spans, biome));
        }
        // One 16-block section per chunk is enough: the region clamps every lookup's y into it.
        let mut chunks: BTreeMap<(i32, i32), BiomeChunk<BiomeId>> = BTreeMap::new();
        for (&(qx, qz), &biome) in &quarts {
            let (cx, cz) = (qx >> 2, qz >> 2);
            if let std::collections::btree_map::Entry::Vacant(entry) = chunks.entry((cx, cz)) {
                let mut chunk = BiomeChunk::new(ChunkPos::new(cx, cz), min_y, 16, biome)
                    .map_err(stage("far biome"))?;
                chunk
                    .fill(min_y, min_y + 15, |quart| {
                        Ok::<_, std::convert::Infallible>(
                            quarts.get(&(quart[0], quart[2])).copied().unwrap_or(biome),
                        )
                    })
                    .map_err(stage("far biome"))?;
                entry.insert(chunk);
            }
        }
        let region = BiomeRegion::new(chunks.into_values().collect(), obfuscate_seed(vanilla.seed))
            .map_err(stage("far biome"))?;
        let generation = self.generation();
        let preliminary: Arc<dyn DensitySampler> = Arc::new(PreliminarySurface::new(
            heights,
            min_y,
            level_height,
            generation.sea_level,
        ));
        let volume = DensityVolume::new([1; 3], [columns[0].0, 0, columns[0].1], [1; 3])
            .map_err(stage("far material"))?;
        let queries = SurfaceQueries {
            pipeline: vanilla,
            temperatures: &mut worker.temperatures,
            biome: Biomes::Region(&region),
            uniform_biomes: None,
            possible: None,
        };
        let sources =
            MaterialDensitySources::new(queries, volume, preliminary, SamplerContext::cached())
                .map_err(stage("far material"))?;
        let mut material =
            MaterialConditions::<XoroshiroRandom, _>::new(generation, vanilla.root, sources);
        let rule = material
            .add_rule(vanilla.rule.clone())
            .map_err(stage("far material"))?;
        let mut result = Vec::with_capacity(geometry.len());
        for (&(wx, wz), (solid, water, below, mut spans, biome)) in columns.iter().zip(geometry) {
            let Some((height, mut state)) = solid else {
                result.push((None, spans));
                continue;
            };
            material.update_xz(wx, wz, 0, 0);
            for depth in 1..=below.min(paint_depth) {
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
                    .map_err(stage("far material"))?
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
        Ok(result)
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
    /// surface and carvers on a BIOMES chunk, so every write updates the WG heightmaps. Before the fill, 26.3's
    /// STRUCTURE_STARTS and STRUCTURE_REFERENCES: [`StructureState::references`] builds (or reads the cached) starts of
    /// every chunk within 8 of this one, this chunk's own included, then keeps those whose box reaches this chunk;
    /// that list is what the vanilla beardifier would read, and `fillFromNoise`'s `buildStructureTerrain` adapts
    /// the terrain to its rigid jigsaw pieces ([`StructureTerrain::from_starts`]). Java's
    /// `createStructures`/`createReferences` also hint the terrain cache; the biome stage already hints this chunk.
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
        // Horizon's `rock_strata`: Reimagined's `StrataDecorator` (blended), at the end of its `fillFromNoise`.
        if let Some(blocks) = &self.strata {
            super::terraforged_rock_strata::apply(self.generator.seed(), &mut chunk, blocks)
                .map_err(stage("strata"))?;
        }

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
                // SurfaceSystem.buildSurface walks each column down from its own top: the
                // whole level, not only the vanilla noise range.
                write_min_y: min,
                write_max_y: min + height - 1,
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
        // Horizon's `erosion_deposits`: erosion's sediment on the natural surface (no height changes).
        if let Some(blocks) = &self.deposits {
            super::terraforged_deposits::apply(
                self.generator.seed(),
                &terrain.read(),
                &mut chunk,
                blocks,
            )
            .map_err(stage("deposits"))?;
        }
        // Horizon's `realistic_vegetation`: podzol, moss and coarse dirt by aspect and water (no height changes).
        if let Some(blocks) = &self.ground_cover {
            super::terraforged_realistic_vegetation::ground_cover(
                self.generator.seed(),
                &terrain.read(),
                &mut chunk,
                blocks,
            )
            .map_err(stage("ground cover"))?;
        }

        // applyCarvers: TerraForged's noise caves with `(int)` of the level seed; no vanilla carvers run (6dd607eb's
        // `applyCarvers` replaces them). The CarverChunk (the biomes each cave passed through) is kept for the
        // chunk's decoration (`NoiseCaveDecorator`), as `getPreCarveChunk` caches it.
        let carver = self
            .caves
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
        self.carvers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(pos, carver);

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
    /// `applyBiomeDecoration`: `BiomeGenerator.decorate` (`decorator.rs`: TerraForged's feature decoration, the noise
    /// caves' biome features, water smoothing and the snow pass, all while the chunk's terrain-cache entry is still
    /// held), then `terrainCache.drop(Seeds.get(region.getSeed()), pos)`, so the cache holds only chunks still being
    /// generated. The decoration context is TerraForged's (sea level 62), as `placeWithBiomeCheck` builds it from the
    /// generator.
    fn decorate(
        &self,
        worker: &mut WorkerState,
        region: &mut [ProtoChunk; 9],
    ) -> Result<DecorateStats, GeneratorError> {
        let features = self
            .features
            .as_ref()
            .ok_or_else(|| resource("decoration resources were not loaded"))?;
        let center = region[4].position();
        let mut random = self.vanilla.region_random_at(center);
        let stats = super::super::generator_decoration::terraforged_decorator::decorate(
            &self.vanilla.data,
            self.vanilla.seed,
            self.generation(),
            worker,
            region,
            &mut random,
            self.structures.as_ref(),
            super::super::generator_decoration::terraforged_decorator::TerraForgedDecoration {
                features,
                generator: &self.generator,
                caves: &self.caves,
                surface: &self.surface,
                air: self.air,
                carvers: &self.carvers,
                deterministic: self.settings.deterministic,
                altitude_climate: super::terraforged_altitude_climate::AltitudeClimate::of(
                    self.settings,
                ),
            },
        )?;
        // terrainCache.drop(Seeds.get(region.getSeed()), pos).
        self.generator
            .drop_chunk(self.vanilla.seed as i32, center.x, center.z);
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
        let (sample, column) = self
            .far_columns(worker, &[(x, z)], FULL_PAINT, false)
            .ok()?
            .pop()?;
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
        approximate: bool,
    ) -> Result<Vec<LodColumn>, GeneratorError> {
        if width <= 0 || side == 0 || side > 64 {
            return Err(resource("far sample dimensions"));
        }
        // Columns in the order vanilla's `lod_grid` returns them (x first, then z). Each is one height sample and one
        // biome sample; `approximate` (Reduced far detail) paints fewer blocks of it, which only changes the blocks
        // under the top (seen on the sides of steep far columns).
        let columns: Vec<(i32, i32)> = (0..side * side)
            .map(|index| {
                (
                    x.wrapping_add((index % side) as i32 * width),
                    z.wrapping_add((index / side) as i32 * width),
                )
            })
            .collect();
        let paint = if approximate {
            REDUCED_PAINT
        } else {
            FULL_PAINT
        };
        let erode = self.settings.eroded_far_terrain && width <= ERODED_FAR_WIDTH;
        self.far_columns(worker, &columns, paint, erode)
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
        _worker: &mut WorkerState,
        search: &BiomeSearch<'_>,
    ) -> Option<(String, [i32; 3])> {
        // 26.3's BiomeSource.findClosestBiome3d (TerraForged's Source does not override it) over
        // Source.possibleBiomes (BiomeMapManager.getOverworldBiomes). Source.getNoiseBiome ignores `y`, so a
        // column's first sample height answers for all of them (Java samples the other heights in vain). A column
        // whose pool has no biome (Java's null) matches nothing here; Java's `candidateBiomes.contains(null)` throws
        // NullPointerException out of the command instead.
        let data = self.vanilla.data();
        let candidates: BTreeSet<BiomeId> = self
            .biomes
            .map
            .overworld
            .iter()
            .map(|id| **id)
            .filter(|id| (search.allowed)(data.biome_name(*id)))
            .collect();
        // Beyond TerraForged (its search never finds them): the biomes only caves hold (terraforged:cave, and the
        // biomes unique caves draw that no climate pool holds: with `corrected_biomes` the cave biomes, which stay in
        // `possibleBiomes`) are looked for where the noise caves carve them, column by column after the noise biome.
        // A biome a climate pool holds is searched as a noise biome only, as before.
        let underground: BTreeSet<BiomeId> = (0..data.biome_names().len())
            .map(|id| BiomeId(id as u32))
            .filter(|id| {
                !self.biomes.map.pools.holds(id)
                    && (search.allowed)(data.biome_name(*id))
                    && self.caves.may_hold(id)
            })
            .collect();
        if candidates.is_empty() && underground.is_empty() {
            return None;
        }
        let y = *super::out_from_origin(
            search.origin[1],
            search.min_y + 1,
            search.max_y + 1,
            search.vertical,
        )
        .first()?;
        // One sampler for the whole search; every sample reads the continent, river and climate noise only (no
        // terrain-cache entry, no erosion region), as `noise_biomes` does.
        let sampler = self.surface_sampler();
        let radius = search.radius.div_euclid(search.horizontal);
        for [sx, sz] in super::spiral_around(radius) {
            let x = search.origin[0].wrapping_add(sx.wrapping_mul(search.horizontal));
            let z = search.origin[2].wrapping_add(sz.wrapping_mul(search.horizontal));
            // QuartPos.fromBlock, then `compute`'s `<< 2`: the quart's corner.
            if !candidates.is_empty() {
                let biome = self.biome_at(&sampler, x & !3, z & !3);
                if let Some(biome) = biome.filter(|biome| candidates.contains(biome)) {
                    return Some((data.biome_name(biome).to_owned(), [x, y, z]));
                }
            }
            if let Some((biome, cave_y)) = self.cave_biome_at(x, z, &underground) {
                return Some((data.biome_name(biome).to_owned(), [x, cave_y, z]));
            }
        }
        None
    }
    /// `Source.getNoiseBiome(x, y, z)` per quart: `sampleBiome(seed, x << 2, z << 2)`, whatever `y` (no cave
    /// layer), with the captured seed and without a terrain-cache hint. `None` where the column's pool has no
    /// biome (Java's `sampleBiome` returns null there).
    fn noise_biomes(&self, _worker: &mut WorkerState, quarts: &[[i32; 3]]) -> Vec<Option<String>> {
        let data = self.vanilla.data();
        let sampler = self.surface_sampler();
        quarts
            .iter()
            .map(|quart| {
                self.biome_at(&sampler, quart[0].wrapping_shl(2), quart[2].wrapping_shl(2))
                    .map(|biome| data.biome_name(biome).to_owned())
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
    use super::super::terraforged_generator_facade::TerraHeightmap;
    use super::super::terraforged_mod_biomes::mod_biomes;
    use super::super::terraforged_terrain_data::Facing;
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
        // TerraForged's decoration plants trees in a wooded biome near the origin.
        let forest = wooded_chunk(&mut worker);
        let mut region = region_around(&mut worker, forest);
        let stats = worker.decorate(&mut region).unwrap();
        assert!(stats.placed > 0);
        assert!(logs(&region[4]) > 0, "no tree logs in the decorated chunk");
    }

    /// The first chunk (on a 4-chunk grid near the origin) whose centre column is in a wooded biome.
    fn wooded_chunk(worker: &mut super::super::GeneratorWorker) -> ChunkPos {
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
        (-12..12)
            .flat_map(|cx| (-12..12).map(move |cz| ChunkPos::new(cx * 4, cz * 4)))
            .find(|&at| {
                let chunk = worker.generate_proto(at).unwrap();
                let top = chunk.surface_height(8, 8);
                wooded.contains(&chunk.biome_at(8, top, 8))
            })
            .expect("a wooded chunk near the origin")
    }

    /// Log blocks in the chunk's whole build range.
    fn logs(chunk: &ProtoChunk) -> usize {
        let (min_y, height) = (chunk.terrain.min_y(), chunk.terrain.height());
        (0..16)
            .flat_map(|x| (0..16).map(move |z| (x, z)))
            .flat_map(|(x, z)| (min_y..min_y + height).map(move |y| (x, y, z)))
            .filter(|&(x, y, z)| chunk.block_name(x, y, z).ends_with("_log"))
            .count()
    }

    /// The vegetal step (PositionSampler's tree grid, grass passes and other features) plants the same wooded region
    /// in the same world twice and in a second world.
    #[test]
    fn terraforged_vegetation_is_repeatable() {
        let Some(data) = data() else { return };
        let (_, first) = terraforged(&data, 7, false);
        let (_, second) = terraforged(&data, 7, false);
        let level = first.level();
        let forest = wooded_chunk(&mut first.worker());
        let decorated = |overworld: &OverworldGenerator| {
            let mut worker = overworld.worker();
            let mut region = region_around(&mut worker, forest);
            worker.decorate(&mut region).unwrap();
            assert!(logs(&region[4]) > 0, "no tree logs in the decorated chunk");
            digest(&region, level)
        };
        let a = decorated(&first);
        assert_eq!(decorated(&first), a, "the same world");
        assert_eq!(decorated(&second), a, "a second world");
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
            "the vanilla overworld's decoration, which the pipeline's vanilla half builds"
        );
        assert!(
            prepared("terraforged:every_biome"),
            "TerraForged's decoration over every biome"
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
        // Without the world's own pack (`datapacks/terraforged`, see `the_taller_overworld_keeps_terrain_above_320`)
        // the build range is the vanilla overworld's (TerraForged's levels are clipped to it); the blocks and biomes
        // are TerraForged's (see `terraforged_chunks_are_terraforged_and_repeatable`).
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

    /// A TerraForged world's own pack (`client_world_type.rs`) gives the overworld TerraForged's dimension type:
    /// the build range is -64..960, so terrain above 26.3's 320 is kept (filled, surfaced, in the heightmaps) and
    /// the structure queries see it: `getBaseHeight` is not clipped at 320 and `getBaseColumn` reaches it.
    #[test]
    fn the_taller_overworld_keeps_terrain_above_320() {
        use crate::worldgen::structure::{jigsaw::JigsawContext, start::StartContext};
        let tall = |ov: &mut crate::worldgen::overrides::WorldgenOverrides| {
            ov.set(
                crate::worldgen::overrides::DIMENSION_TYPES,
                "minecraft:overworld",
                super::super::terraforged_world_preset::overworld_type_json().to_owned(),
            )
        };
        let Some(normal_data) = data() else { return };
        let mut ov = crate::worldgen::overrides::WorldgenOverrides::default();
        tall(&mut ov);
        let Some(data) = data_with(&ov) else { return };
        // TerraForged's default levels seldom rise above 320 (the highest un-eroded height within 32768 blocks of
        // the origin is about 310 to 350 for most seeds); seed 1234 has a peak near x -14848, z -32256.
        let seed = 1234;
        let (pipeline, overworld) = terraforged(&data, seed, false);
        assert_eq!(overworld.level(), (-64, 1024));
        assert_eq!(terraforged(&normal_data, seed, false).1.level(), (-64, 384));
        // The peak's highest column, from the un-eroded height noise (a cheap read) on a 16-block grid.
        let generator = &pipeline.generator;
        let noise = generator.noise_generator();
        let max_y = generator.levels().max_y as f32;
        let height_at =
            |(x, z): (i32, i32)| noise.height_noise(generator.seed(), x, z).unwrap() * max_y;
        let (x, z) = (0..64 * 64)
            .map(|i| (-14848 + (i % 64 - 32) * 16, -32256 + (i / 64 - 32) * 16))
            .max_by(|a, b| height_at(*a).total_cmp(&height_at(*b)))
            .unwrap();
        assert!(height_at((x, z)) > 340.0, "{}", height_at((x, z)));
        let pos = ChunkPos::new(x >> 4, z >> 4);
        let chunk = overworld.worker().generate_proto(pos).unwrap();
        let top = (0..256)
            .map(|i| chunk.surface_height(i % 16, i / 16))
            .max()
            .unwrap();
        assert!(top > 330, "the chunk's highest ground is {top}");
        let (lx, lz) = (0..256)
            .map(|i| (i % 16, i / 16))
            .find(|&(lx, lz)| chunk.surface_height(lx, lz) == top)
            .unwrap();
        // Solid from 320 up to the surface: not cut off at 26.3's top.
        for y in 320..top {
            assert_ne!(
                chunk.block_name(lx, y, lz),
                "minecraft:air",
                "{lx} {y} {lz}"
            );
        }
        // The same chunk in a 384-block overworld is cut flat at y 319.
        let (_, normal) = terraforged(&normal_data, seed, false);
        let clipped = normal.worker().generate_proto(pos).unwrap();
        assert!((0..256).all(|i| clipped.surface_height(i % 16, i / 16) <= 320));
        // The structure queries follow the level: a base height above 320, and the column solid up to it.
        let mut worker = overworld.worker();
        let mut queries = pipeline.queries(&mut worker.state);
        let (bx, bz) = (pos.x * 16 + lx, pos.z * 16 + lz);
        let height = queries.base_height(bx, bz, HeightmapType::OceanFloorWg);
        assert!(height > 320, "base height {height}");
        assert_eq!(queries.min_y(), -64);
        let column = StartContext::base_column(&mut queries, bx, bz, HeightmapType::OceanFloorWg);
        assert_eq!(column.len(), 1024);
        assert!(column[(height - 1 + 64) as usize]);
        assert!(!column[(height + 64) as usize]);
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

    /// With structures on, a village's chunk gets the same starts, references and blocks when it is generated again
    /// in the same world and in a second world (the starts are built per chunk from the level seed and TerraForged's
    /// heights and biomes, whichever worker asks first), and its terrain is adapted to the village's rigid pieces:
    /// some column differs from the same chunk without structures.
    #[test]
    fn structure_chunks_are_repeatable_and_adapted() {
        let Some(data) = data() else { return };
        let seed = 7;
        let overworld = |structures| {
            OverworldGenerator::for_world(
                Arc::clone(&data),
                seed,
                PRESET,
                "minecraft:overworld",
                structures,
            )
            .unwrap()
        };
        let (first, second) = (overworld(true), overworld(true));
        let villages = [
            "minecraft:village_plains",
            "minecraft:village_desert",
            "minecraft:village_savanna",
            "minecraft:village_snowy",
            "minecraft:village_taiga",
        ]
        .map(String::from);
        let (_, at) = first.worker().locate(&villages, [0, 64, 0], 100).unwrap();
        let pos = ChunkPos::new(at[0] >> 4, at[2] >> 4);
        // The second world is asked from another neighbour first, so its starts are built in another order.
        second
            .worker()
            .generate_proto(ChunkPos::new(pos.x + 3, pos.z - 2))
            .unwrap();
        let a = first.worker().generate_proto(pos).unwrap();
        let again = first.worker().generate_proto(pos).unwrap();
        let b = second.worker().generate_proto(pos).unwrap();
        let plain = overworld(false).worker().generate_proto(pos).unwrap();
        assert!(!a.own_starts().is_empty());
        assert!(plain.own_starts().is_empty());
        let starts =
            |chunk: &ProtoChunk| format!("{:?} {:?}", chunk.own_starts(), chunk.structure_starts);
        assert_eq!(starts(&a), starts(&again));
        assert_eq!(starts(&a), starts(&b));
        let (min_y, height) = first.level();
        let mut adapted = false;
        for y in min_y..min_y + height {
            for z in 0..16 {
                for x in 0..16 {
                    let block = a.block(x, y, z);
                    assert_eq!(block, again.block(x, y, z), "rerun {x} {y} {z}");
                    assert_eq!(block, b.block(x, y, z), "second world {x} {y} {z}");
                    adapted |= block != plain.block(x, y, z);
                }
            }
        }
        assert!(adapted, "the village's rigid pieces adapt no column");
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
        // Without terraforged:cave (no TerraForged pack) only the unique caves write biomes (any registered biome).
        assert!(cave_air > 0 && cave_quarts > 0, "{cave_air} {cave_quarts}");
    }

    #[test]
    fn finished_chunks_generated_again_are_released() {
        // `Streaming::regenerate`'s edge decoration generates finished neighbours again and drops them
        // undecorated: their terrain-cache entries and carver records go with `release_finished`.
        let Some(data) = data() else { return };
        let stem = terraforged_stem(&data, PRESET, "minecraft:overworld").unwrap();
        let pipeline = Arc::new(TerraForgedPipeline::new(&data, 12, &stem, false).unwrap());
        super::hold(&pipeline);
        let overworld = OverworldGenerator {
            pipeline: Arc::clone(&pipeline) as Arc<dyn SeededPipeline>,
            noise_clamped: None,
        };
        let mut worker = overworld.worker();
        let (copy, other) = (ChunkPos::new(3, -2), ChunkPos::new(9, 9));
        for pos in [copy, other] {
            worker.generate_proto(pos).unwrap();
        }
        let held = |pipeline: &TerraForgedPipeline| {
            let carvers = pipeline.carvers.lock().unwrap();
            (
                pipeline.generator.cached_chunks(),
                carvers.contains_key(&copy),
                carvers.len(),
            )
        };
        assert_eq!(held(&pipeline), (2, true, 2));
        super::release_finished(&[(copy.x, copy.z)]);
        // Only the released chunk goes; the other is still waiting for its decoration.
        assert_eq!(held(&pipeline), (1, false, 1));
        assert!(pipeline.carvers.lock().unwrap().contains_key(&other));
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

    /// A TerraForged pipeline, shared with the generator that runs it.
    fn terraforged(
        data: &Arc<GeneratorData>,
        seed: i64,
        structures: bool,
    ) -> (Arc<TerraForgedPipeline>, OverworldGenerator) {
        let stem = terraforged_stem(data, PRESET, "minecraft:overworld").unwrap();
        let pipeline = Arc::new(TerraForgedPipeline::new(data, seed, &stem, structures).unwrap());
        let overworld = OverworldGenerator {
            pipeline: Arc::clone(&pipeline) as Arc<dyn SeededPipeline>,
            noise_clamped: None,
        };
        (pipeline, overworld)
    }

    /// The 3x3 chunks around `center`, generated now.
    fn region_around(
        worker: &mut super::super::GeneratorWorker,
        center: ChunkPos,
    ) -> [ProtoChunk; 9] {
        std::array::from_fn(|i| {
            worker
                .generate_proto(ChunkPos::new(
                    center.x + i as i32 % 3 - 1,
                    center.z + i as i32 / 3 - 1,
                ))
                .unwrap()
        })
    }

    /// Every block, biome, block-entity position and entity count of the region's chunks.
    fn digest(region: &[ProtoChunk; 9], (min_y, height): (i32, i32)) -> Vec<u32> {
        let mut out = Vec::new();
        for chunk in region {
            for y in min_y..min_y + height {
                for z in 0..16 {
                    for x in 0..16 {
                        out.push(chunk.block(x, y, z).0 as u32);
                    }
                }
            }
            for y in (min_y..min_y + height).step_by(4) {
                for z in (0..16).step_by(4) {
                    for x in (0..16).step_by(4) {
                        out.push(chunk.biome_id_at(x, y, z).0);
                    }
                }
            }
            let mut positions: Vec<_> = chunk.block_entities.iter().map(|(at, _)| *at).collect();
            positions.sort_unstable();
            out.extend(positions.into_iter().flatten().map(|v| v as u32));
            out.push(chunk.entities.len() as u32);
        }
        out
    }

    /// TerraForged's decoration of a chunk with unique caves gives the same region when its chunks are generated and
    /// decorated again in the same world and in a second world. The chunk's `CarverChunk` is kept from its carving to
    /// its decoration, which takes it out; a chunk carved by an earlier run (no CarverChunk) replays its caves and
    /// still decorates.
    #[test]
    fn terraforged_decoration_is_repeatable() {
        let Some(data) = data() else { return };
        let seed = 7;
        // The first chunk where unique caves form, as in `noise_caves_carve_the_same_chunk_the_same_way`.
        let unique = super::super::terraforged_cave_noise::unique_cave_noise();
        let center = (0i32..)
            .map(|k| {
                let cx = k.wrapping_mul(7919).rem_euclid(4001) - 2000;
                let cz = k.wrapping_mul(104_729).rem_euclid(4001) - 2000;
                ChunkPos::new(cx, cz)
            })
            .find(|pos| {
                let (x, z) = ((pos.x * 16 + 8) as f32, (pos.z * 16 + 8) as f32);
                unique.try_value_seed_first(seed as i32, x, z).unwrap() > 0.3
            })
            .unwrap();
        let (pipeline, first) = terraforged(&data, seed, false);
        let (_, second) = terraforged(&data, seed, false);
        let level = first.level();
        let decorated = |overworld: &OverworldGenerator| {
            let mut worker = overworld.worker();
            let mut region = region_around(&mut worker, center);
            let undecorated = digest(&region, level);
            let stats = worker.decorate(&mut region).unwrap();
            assert!(stats.placed > 0);
            let after = digest(&region, level);
            assert_ne!(after, undecorated, "the decoration changed nothing");
            (after, region)
        };
        let (a, region) = decorated(&first);
        assert!(region[4].is_decorated());
        let carvers = |pipeline: &TerraForgedPipeline| {
            let carvers = pipeline.carvers.lock().unwrap();
            (carvers.contains_key(&center), carvers.len())
        };
        // The centre's CarverChunk was taken; its 8 neighbours' wait for their own decoration.
        assert_eq!(carvers(&pipeline), (false, 8));
        let (again, _) = decorated(&first);
        assert_eq!(again, a, "the same world");
        let (b, _) = decorated(&second);
        assert_eq!(b, a, "a second world");
        // getPostCarveChunk's replay, for a chunk without its CarverChunk.
        let mut worker = first.worker();
        let mut region = region_around(&mut worker, center);
        pipeline.carvers.lock().unwrap().remove(&center);
        worker.decorate(&mut region).unwrap();
        assert!(region[4].is_decorated());
    }

    /// A second live pipeline of a world (a far-decoration thread's) shares the first's noise, not its caches, while
    /// `live_generator` keeps answering with the first; another seed builds its own. The far pipelines' chunks,
    /// generated on one thread or six and decorated, are the level pipeline's, block for block and height for height.
    #[test]
    fn far_pipelines_share_the_level_noise_and_make_its_chunks() {
        // A seed no other test builds a live pipeline of.
        far_pipelines_make_the_level_chunks(267_000_267, terraforged_settings::FAITHFUL);
    }

    /// The same with `realistic_vegetation` (and `eroded_far_terrain`) on: the far-decoration pipelines weigh the
    /// trees and lay the ground cover from the same aspects and distances to water as the level's, whichever thread
    /// generates a chunk and whatever it generated before.
    #[test]
    fn far_pipelines_make_the_level_chunks_with_realistic_vegetation() {
        far_pipelines_make_the_level_chunks(
            297_000_297,
            Settings {
                realistic_vegetation: true,
                eroded_far_terrain: true,
                ..terraforged_settings::FAITHFUL
            },
        );
    }

    /// The same with `altitude_climate` on (strength 2), around the highest point of plains or a colder biome on a
    /// 64-block grid over 3,000 x 3,000 blocks: the far-decoration pipelines lay the same snow line and treeline as
    /// the level's.
    #[test]
    fn far_pipelines_make_the_level_chunks_with_altitude_climate() {
        far_pipelines_make_the_level_chunks(
            298_000_298,
            Settings {
                altitude_climate: true,
                altitude_climate_strength: 2.0,
                ..terraforged_settings::FAITHFUL
            },
        );
    }

    /// The same with `rock_strata` (and `eroded_far_terrain`) on: the far-decoration pipelines band the stone and
    /// erode with the strata's hardness as the level's does.
    #[test]
    fn far_pipelines_make_the_level_chunks_with_rock_strata() {
        far_pipelines_make_the_level_chunks(
            299_000_299,
            Settings {
                rock_strata: true,
                eroded_far_terrain: true,
                ..terraforged_settings::FAITHFUL
            },
        );
    }

    /// The same with `coastal_landforms` (and `eroded_far_terrain`) on, around a cliff coast: the far-decoration
    /// pipelines cut the same cliffs, lay the same flats and turn the same beaches to stony shores as the level's.
    #[test]
    fn far_pipelines_make_the_level_chunks_with_coastal_landforms() {
        far_pipelines_make_the_level_chunks(
            300_000_300,
            Settings {
                coastal_landforms: true,
                eroded_far_terrain: true,
                ..terraforged_settings::FAITHFUL
            },
        );
    }

    /// The same with `glacial_landforms` (and `eroded_far_terrain`) on, in a glaciated valley: the far-decoration
    /// pipelines cut the same troughs as the level's.
    #[test]
    fn far_pipelines_make_the_level_chunks_with_glacial_landforms() {
        far_pipelines_make_the_level_chunks(
            301_000_301,
            Settings {
                glacial_landforms: true,
                eroded_far_terrain: true,
                ..terraforged_settings::FAITHFUL
            },
        );
    }

    /// The same with `glacial_landforms` and `coastal_landforms` (and `eroded_far_terrain`) on, where a glaciated
    /// valley reaches the coast band.
    #[test]
    fn far_pipelines_make_the_level_chunks_with_glacial_and_coastal_landforms() {
        far_pipelines_make_the_level_chunks(
            301_000_302,
            Settings {
                glacial_landforms: true,
                coastal_landforms: true,
                eroded_far_terrain: true,
                ..terraforged_settings::FAITHFUL
            },
        );
    }

    /// The same with `flow_rivers` (and `eroded_far_terrain`) on, across the channel of runtime seed 777 at
    /// (736, -672) (the level seed's high bits keep it out of the other tests' live pipelines): the far-decoration
    /// pipelines carve the same channels, over the drainage map they share through the level's noise, as the level's.
    #[test]
    #[ignore = "real terrain: every query builds drainage tiles, too slow in debug; run with --profile fast --ignored"]
    fn far_pipelines_make_the_level_chunks_with_flow_rivers() {
        far_pipelines_make_the_level_chunks(
            777 + (293 << 32),
            Settings {
                flow_rivers: true,
                eroded_far_terrain: true,
                ..terraforged_settings::FAITHFUL
            },
        );
    }

    /// The same with `basin_erosion` (and `eroded_far_terrain`) on, in one of runtime seed 777's cut valleys (level
    /// seed 777 + (294 << 32), so no other test's live pipeline shares it): the far-decoration pipelines lower the
    /// same valleys, over the drainage map and erosion tiles they share through the level's noise, as the level's.
    #[test]
    #[ignore = "real terrain: every query builds drainage tiles, too slow in debug; run with --profile fast --ignored"]
    fn far_pipelines_make_the_level_chunks_with_basin_erosion() {
        far_pipelines_make_the_level_chunks(
            777 + (294 << 32),
            Settings {
                basin_erosion: true,
                eroded_far_terrain: true,
                ..terraforged_settings::FAITHFUL
            },
        );
    }

    /// The body of the tests above: the live pipelines of `seed` are built with `settings`. The chunks compared are
    /// around (5, -3), with `flow_rivers` around a channel (see there), with `basin_erosion` alone around the first
    /// point of a 64-block grid over seed 777's valleys (x 600..1368, z -900..-132) cut by more than 10 blocks, with `altitude_climate` around the highest point of plains or a colder biome on a 64-block
    /// grid over 3,000 x 3,000 blocks, so that the snow line and the treeline are in them, with `glacial_landforms`
    /// around a glaciated valley (see there), and with `coastal_landforms` alone around the first point of a 128-block
    /// grid over 16,000 x 16,000 blocks in the coast band of a full cliff.
    fn far_pipelines_make_the_level_chunks(seed: i64, settings: Settings) {
        use super::super::terraforged_generator_facade::live_generator;
        let Some(data) = data() else { return };
        const OVERWORLD: &str = "minecraft:overworld";
        let stem = terraforged_stem(&data, PRESET, OVERWORLD).unwrap();
        let live = |seed: i64| {
            terraforged_settings::set_for_thread(Some(settings));
            let pipeline = live_pipeline(&data, seed, &stem, OVERWORLD, true);
            terraforged_settings::set_for_thread(None);
            Arc::new(pipeline.unwrap())
        };
        let level = live(seed);
        assert_eq!(level.settings, settings);
        assert_eq!(level.ground_cover.is_some(), settings.realistic_vegetation);
        assert_eq!(level.strata.is_some(), settings.rock_strata);
        let (far, far_threaded) = (live(seed), live(seed));
        for other in [&far, &far_threaded] {
            assert!(other.generator.shares_noise_with(&level.generator));
            assert!(!Arc::ptr_eq(&other.generator, &level.generator));
        }
        let found = live_generator(seed, OVERWORLD).unwrap();
        assert!(Arc::ptr_eq(&found, &level.generator));
        drop(found);
        let other_seed = live(seed.wrapping_add(1));
        assert!(!other_seed.generator.shares_noise_with(&level.generator));
        drop(other_seed);

        let center = if settings.altitude_climate {
            // In a biome of vanilla temperature 0.8 or less (plains and colder), which snows from about y 175 at
            // strength 2; savannas and deserts (2.0) stay warm up to the overworld's top.
            let sampler = level.surface_sampler();
            let (height, x, z) = (-24..24)
                .flat_map(|i| (-24..24).map(move |j| (i * 64, j * 64)))
                .filter(|&(x, z)| {
                    level.biome_at(&sampler, x, z).is_some_and(|biome| {
                        data.temperatures[biome.0 as usize].base_temperature <= 0.8
                    })
                })
                .map(|(x, z)| (level.generator.far_heights(x, z).unwrap().0, x, z))
                // Under the 384-block overworld's top: no snow lies on land at its last block, y 319.
                .filter(|&(height, _, _)| height < 300)
                .max()
                .unwrap();
            assert!(height > 200, "{height} at {x} {z}");
            ChunkPos::new(x >> 4, z >> 4)
        } else if settings.flow_rivers {
            let seed = level.generator.seed();
            let channels = level
                .generator
                .noise_generator()
                .flow_rivers()
                .unwrap()
                .channels(seed);
            let reach = channels.reach(736, -672).expect("seed 777's channel");
            assert!(reach.distance <= reach.width * 0.5, "{reach:?}");
            ChunkPos::new(736 >> 4, -672 >> 4)
        } else if settings.basin_erosion {
            let seed = level.generator.seed();
            let map = level
                .generator
                .noise_generator()
                .basin_erosion()
                .unwrap()
                .map(seed);
            let mut memo = Default::default();
            let (x, z) = (0..12 * 12)
                .map(|i| (600 + i % 12 * 64, -900 + i / 12 * 64))
                .find(|&(x, z)| map.delta(&mut memo, x, z) < -10.0)
                .expect("a cut valley");
            ChunkPos::new(x >> 4, z >> 4)
        } else if settings.glacial_landforms {
            // A river valley at least half under ice (with `coastal_landforms` in the coast band: a trough that reaches
            // the sea), on a 128-block grid over 24,000 x 24,000 blocks.
            use super::super::terraforged_biome_sampler::{BEACH, COAST};
            let noise = level.generator.noise_generator();
            let seed = level.generator.seed();
            let glaciers =
                super::super::terraforged_glacial_landforms::Glaciers::new(noise, settings);
            let frequency = noise.levels().noise_levels.frequency;
            let (x, z) = (-96..96)
                .flat_map(|i| (-96..96).map(move |j| (i * 128, j * 128)))
                .find(|&(x, z)| {
                    let mut sample = super::super::terraforged_terrain_data::NoiseSample::new(
                        &noise.continent().context().terrain,
                    );
                    noise
                        .sample(seed, x, z, &mut sample, &mut noise.blender_resource())
                        .unwrap();
                    let at = (x as f32 * frequency, z as f32 * frequency);
                    sample.river_noise < 0.5
                        && (!settings.coastal_landforms
                            || (BEACH..COAST).contains(&sample.continent_noise))
                        && glaciers.glaciation(seed, (x, z), at, sample.height_noise) > 0.5
                })
                .expect("a glaciated valley");
            ChunkPos::new(x >> 4, z >> 4)
        } else if settings.coastal_landforms {
            use super::super::terraforged_biome_sampler::{BEACH, COAST};
            let noise = level.generator.noise_generator();
            let seed = level.generator.seed();
            let coasts = super::super::terraforged_coastal_landforms::Coasts::new(
                seed,
                noise.levels().noise_levels,
            );
            let (x, z) = (-64..64)
                .flat_map(|i| (-64..64).map(move |j| (i * 128, j * 128)))
                .find(|&(x, z)| {
                    let mut sample = super::super::terraforged_terrain_data::NoiseSample::new(
                        &noise.continent().context().terrain,
                    );
                    noise
                        .sample(seed, x, z, &mut sample, &mut noise.blender_resource())
                        .unwrap();
                    (BEACH..COAST).contains(&sample.continent_noise)
                        && coasts.landform(x, z, None).cliff > 0.9
                })
                .expect("a cliff coast");
            ChunkPos::new(x >> 4, z >> 4)
        } else {
            ChunkPos::new(5, -3)
        };
        let decorated = |pipeline: &Arc<TerraForgedPipeline>, threads: usize| {
            let overworld = OverworldGenerator {
                pipeline: Arc::clone(pipeline) as Arc<dyn SeededPipeline>,
                noise_clamped: None,
            };
            let made = std::sync::Mutex::new(BTreeMap::new());
            std::thread::scope(|scope| {
                for t in 0..threads {
                    let (overworld, made) = (&overworld, &made);
                    scope.spawn(move || {
                        let mut worker = overworld.worker();
                        for i in (t..9).step_by(threads) {
                            let at = ChunkPos::new(
                                center.x + i as i32 % 3 - 1,
                                center.z + i as i32 / 3 - 1,
                            );
                            made.lock()
                                .unwrap()
                                .insert(i, worker.generate_proto(at).unwrap());
                        }
                    });
                }
            });
            let mut made = made.into_inner().unwrap();
            let mut region: [ProtoChunk; 9] = std::array::from_fn(|i| made.remove(&i).unwrap());
            overworld.worker().decorate(&mut region).unwrap();
            assert!(region[4].is_decorated());
            if settings.altitude_climate {
                let snow = (0..256)
                    .filter(|i| {
                        let (x, z) = (i & 15, i >> 4);
                        let (min_y, height) =
                            (region[4].terrain.min_y(), region[4].terrain.height());
                        (min_y..min_y + height)
                            .any(|y| region[4].block_name(x, y, z) == "minecraft:snow")
                    })
                    .count();
                assert!(snow > 0, "no snow around the summit {center:?}");
            }
            let mut out = digest(&region, overworld.level());
            for chunk in &region {
                for z in 0..16 {
                    for x in 0..16 {
                        out.push(chunk.surface_height(x, z) as u32);
                    }
                }
            }
            out
        };
        let expected = decorated(&level, 1);
        // The level's erosion regions stay its own: a far pipeline starts with none and builds its own.
        assert!(level.generator.cached_regions() > 0);
        assert_eq!(far.generator.cached_regions(), 0);
        assert!(
            decorated(&far, 1) == expected,
            "a far pipeline on one thread"
        );
        assert!(
            decorated(&far_threaded, 6) == expected,
            "a far pipeline on six threads"
        );
        assert!(far.generator.cached_regions() > 0);
    }

    /// Every block of the middle 3x3 chunks of a fresh 5x5 around `center`, generated on `threads` workers and
    /// decorated in `order` (offsets from `center`), on a pipeline built with `settings`.
    fn decorated_in_order(
        data: &Arc<GeneratorData>,
        settings: Settings,
        center: ChunkPos,
        order: &[(i32, i32)],
        threads: usize,
    ) -> BTreeMap<(i32, i32), Vec<u32>> {
        terraforged_settings::set_for_thread(Some(settings));
        let (_, overworld) = terraforged(data, 777, false);
        terraforged_settings::set_for_thread(None);
        let level = overworld.level();
        let all: Vec<(i32, i32)> = (-2..=2)
            .flat_map(|x| (-2..=2).map(move |z| (x, z)))
            .collect();
        let made = std::sync::Mutex::new(BTreeMap::new());
        std::thread::scope(|scope| {
            for t in 0..threads {
                let (overworld, all, made) = (&overworld, &all, &made);
                scope.spawn(move || {
                    let mut worker = overworld.worker();
                    for &(x, z) in all.iter().skip(t).step_by(threads) {
                        let at = ChunkPos::new(center.x + x, center.z + z);
                        let chunk = worker.generate_proto(at).unwrap();
                        made.lock().unwrap().insert((x, z), chunk);
                    }
                });
            }
        });
        let mut protos = made.into_inner().unwrap();
        let mut worker = overworld.worker();
        for &(cx, cz) in order {
            let keys: Vec<(i32, i32)> = (0..9).map(|i| (cx + i % 3 - 1, cz + i / 3 - 1)).collect();
            let region: Vec<_> = keys.iter().map(|k| protos.remove(k).unwrap()).collect();
            let mut region: [ProtoChunk; 9] = region.try_into().ok().unwrap();
            worker.decorate(&mut region).unwrap();
            for (key, chunk) in keys.into_iter().zip(region) {
                protos.insert(key, chunk);
            }
        }
        let (min_y, height) = level;
        order
            .iter()
            .map(|&key| {
                let chunk = &protos[&key];
                let mut blocks = Vec::new();
                for y in min_y..min_y + height {
                    for z in 0..16 {
                        for x in 0..16 {
                            blocks.push(chunk.block(x, y, z).0);
                        }
                    }
                }
                (key, blocks)
            })
            .collect()
    }

    /// Tree blocks (logs and leaves) of `a` and `b` that differ, and tree blocks in `a`.
    fn tree_differences(
        data: &GeneratorData,
        a: &BTreeMap<(i32, i32), Vec<u32>>,
        b: &BTreeMap<(i32, i32), Vec<u32>>,
    ) -> (usize, usize) {
        let registry = data.registry();
        let tree = |id: &u32| {
            registry
                .state(BlockStateId(*id))
                .is_some_and(|s| s.name.ends_with("_log") || s.name.ends_with("_leaves"))
        };
        let (mut differing, mut trees) = (0, 0);
        for (key, blocks) in a {
            for (p, q) in blocks.iter().zip(&b[key]) {
                trees += usize::from(tree(p));
                differing += usize::from(p != q && (tree(p) || tree(q)));
            }
        }
        (differing, trees)
    }

    /// With `deterministic`, the trees of a wooded 3x3 depend much less on the order its chunks decorate in (and on
    /// how many threads generated them): no placement is reseeded by a neighbour's tree. What is left are trees
    /// that grow into each other across a chunk border, or that their placement's own `heightmap` modifier puts on
    /// a neighbour's leaves; vanilla's underground blobs (ores, granite...) overlapping a border depend on the order
    /// too, as in every world.
    #[test]
    #[ignore = "generates and decorates 25 chunks four times (about a minute)"]
    fn deterministic_trees_depend_less_on_the_decoration_order() {
        let Some(data) = data() else { return };
        let forest = {
            let (_, overworld) = terraforged(&data, 777, false);
            wooded_chunk(&mut overworld.worker())
        };
        let forward: Vec<(i32, i32)> = (-1..=1)
            .flat_map(|z| (-1..=1).map(move |x| (x, z)))
            .collect();
        let backward: Vec<(i32, i32)> = forward.iter().rev().copied().collect();
        let differences = |settings: Settings| {
            let a = decorated_in_order(&data, settings, forest, &forward, 1);
            let b = decorated_in_order(&data, settings, forest, &backward, 4);
            tree_differences(&data, &a, &b)
        };
        let on = differences(Settings {
            deterministic: true,
            ..Settings::default()
        });
        let off = differences(Settings::default());
        eprintln!(
            "tree blocks differing between the orders: {on:?} deterministic, {off:?} faithful"
        );
        assert!(on.1 > 1000, "no forest: {on:?}");
        assert!(on.0 * 3 < off.0 * 2, "{on:?} vs {off:?}");
    }

    /// A chunk's terrain data (heights, rivers, gradients, base heights, and Horizon's aspects and distances to water)
    /// is the same whichever thread computes it and
    /// whatever it computed before, by default and with `sparse_forests`: the eroded generator resets every sample and
    /// starts its river noise at the same constant (`RiverStart`, `ErodedNoiseGenerator::generate`), so TerraForged's
    /// river-noise carry-over from a worker's earlier chunks never reaches the world.
    #[test]
    fn terrain_data_ignores_threads() {
        let preset: Value =
            serde_json::from_str(super::super::terraforged_world_preset::preset_json()).unwrap();
        let levels = terrain_levels(Some(
            &preset["dimensions"]["minecraft:overworld"]["generator"]["levels"],
        ))
        .unwrap();
        let summary = |g: &TerraForgedGenerator, x: i32, z: i32| {
            let handle = g.chunk_data(777, x, z);
            let d = handle.read();
            let mut v = Vec::new();
            for lx in 0..16 {
                for lz in 0..16 {
                    v.push((
                        d.height_at(lx, lz),
                        d.river().get(lx, lz).to_bits(),
                        d.normalized_gradient(lx, lz, 1.0).to_bits(),
                        d.base_height_at(lx, lz),
                        // Horizon's `realistic_vegetation` data.
                        d.facing(lx, lz, Facing::North).to_bits(),
                        d.facing(lx, lz, Facing::East).to_bits(),
                        d.water_distance(lx, lz).to_bits(),
                    ));
                }
            }
            v
        };
        let all: Vec<(i32, i32)> = (0..4).flat_map(|x| (0..4).map(move |z| (x, z))).collect();
        for sparse_forests in [false, true] {
            terraforged_settings::set_for_thread(Some(Settings {
                sparse_forests,
                ..Settings::default()
            }));
            let single = build_noise(levels).unwrap();
            let a: Vec<_> = all.iter().map(|&(x, z)| summary(&single, x, z)).collect();
            let threaded = build_noise(levels).unwrap();
            let made = std::sync::Mutex::new(BTreeMap::new());
            std::thread::scope(|scope| {
                for t in 0..6 {
                    let (g, all, made) = (&threaded, &all, &made);
                    scope.spawn(move || {
                        for &(x, z) in all.iter().rev().skip(t).step_by(6) {
                            let s = summary(g, x, z);
                            made.lock().unwrap().insert((x, z), s);
                        }
                    });
                }
            });
            let made = made.into_inner().unwrap();
            for (i, key) in all.iter().enumerate() {
                assert!(a[i] == made[key], "chunk {key:?} differs between threads");
            }
        }
        terraforged_settings::set_for_thread(None);
    }

    /// TerraForged's decoration knows every registered biome's features, the nether's and the end's included (the
    /// unique noise caves give quarts any biome, and `NoiseCaveDecorator` places that biome's features there), and
    /// offsets each step's feature seeds by the structures registered for it, structures on or off.
    #[test]
    fn terraforged_decoration_covers_every_biome() {
        let Some(data) = data() else { return };
        let (with, _) = terraforged(&data, 3, true);
        let (without, _) = terraforged(&data, 3, false);
        let features = with.features.as_ref().unwrap();
        for name in [
            "minecraft:basalt_deltas",
            "minecraft:end_highlands",
            "minecraft:plains",
        ] {
            let biome = data.biome_id(name).unwrap();
            assert!(features.knows_every_feature_of(biome), "{name}");
        }
        let by_step: Vec<usize> = with
            .structures
            .as_ref()
            .unwrap()
            .by_step
            .iter()
            .map(Vec::len)
            .collect();
        assert_eq!(features.structures_per_step(), by_step.as_slice());
        let off = without.features.as_ref().unwrap();
        assert!(without.structures.is_none());
        assert_eq!(off.structures_per_step(), by_step.as_slice());
        assert!(by_step.iter().sum::<usize>() > 0);
    }

    /// The vegetation is bound to the world's biomes once: the first config whose tag holds the biome (a tag naming an
    /// unregistered biome dropped whole, so `trees/patchy` holds nothing without `terraforged:oak_forest`), rivers by
    /// `#minecraft:is_river`, and the vegetal features split into trees (unwrapped when configured), grass and the rest.
    #[test]
    fn terraforged_vegetation_binds_to_the_registry() {
        let Some(data) = data() else { return };
        let stem = terraforged_stem(&data, PRESET, "minecraft:overworld").unwrap();
        let pipeline = TerraForgedPipeline::new(&data, 7, &stem, false).unwrap();
        let features = pipeline.features.as_ref().unwrap();
        let of = |name: &str| features.vegetation_of(data.biome_id(name).unwrap());
        let (tag, river, [trees, grass, other], unwrapped) = of("minecraft:birch_forest");
        assert_eq!(tag, Some("terraforged:trees/temperate"));
        assert!(!river && unwrapped);
        assert!(
            trees.contains(&"minecraft:trees_birch")
                && grass.contains(&"minecraft:patch_grass_forest")
        );
        assert!(other.contains(&"minecraft:flower_default"), "{other:?}");
        assert_eq!(of("minecraft:taiga").0, Some("terraforged:trees/hardy"));
        assert_eq!(of("minecraft:pale_garden").0, None);
        assert!(of("minecraft:pale_garden").2[0].contains(&"minecraft:pale_garden_vegetation"));
        // Without terraforged:oak_forest (no TerraForged pack) trees/patchy is dropped: wooded badlands has no config.
        assert!(data.biome_id("terraforged:oak_forest").is_none());
        assert_eq!(of("minecraft:wooded_badlands").0, None);
        for river in ["minecraft:river", "minecraft:frozen_river"] {
            assert!(of(river).1, "{river}");
        }
        assert!(!of("minecraft:plains").1);
        for id in 0..data.biome_names().len() as u32 {
            let (_, _, _, unwrapped) = features.vegetation_of(BiomeId(id));
            assert!(unwrapped, "{}", data.biome_name(BiomeId(id)));
        }
    }

    /// TerraForged's biomes (6dd607eb `ModBiomes`) built from the real 26.3 parents as `ModBiome.create` builds them:
    /// `oak_forest` is plains unchanged; `cave` is dripstone caves with only large dripstone (LOCAL_MODIFICATIONS) and
    /// pointed dripstone (UNDERGROUND_DECORATION) and no carvers, its climate, effects and spawns kept.
    #[test]
    fn mod_biomes_copy_their_26_3_parents() {
        let Some(data) = data() else { return };
        let (biomes, missing) = mod_biomes(|name| parent_of(&data, name));
        assert!(missing.is_empty(), "{missing:?}");
        let [(cave_id, cave), (oak_id, oak)] = <[_; 2]>::try_from(biomes).unwrap();
        assert_eq!(
            (cave_id.as_str(), oak_id.as_str()),
            ("terraforged:cave", "terraforged:oak_forest")
        );
        assert_eq!(oak, parent_of(&data, "minecraft:plains").unwrap());
        let dripstone = parent_of(&data, "minecraft:dripstone_caves").unwrap();
        let features = cave["features"].as_array().unwrap();
        assert_eq!(
            features.len(),
            dripstone["features"].as_array().unwrap().len()
        );
        for (step, list) in features.iter().enumerate() {
            let expected: &[&str] = match step {
                2 => &["minecraft:large_dripstone"],
                7 => &["minecraft:pointed_dripstone"],
                _ => &[],
            };
            assert_eq!(list, &serde_json::json!(expected), "step {step}");
        }
        assert_eq!(cave["carvers"], serde_json::json!([]));
        for (key, value) in dripstone.as_object().unwrap() {
            if key != "features" && key != "carvers" {
                assert_eq!(&cave[key], value, "{key}");
            }
        }
    }

    /// A vanilla biome's JSON from the generator's data.
    fn parent_of(data: &GeneratorData, name: &str) -> Result<serde_json::Value, String> {
        let id = data.biome_id(name).ok_or_else(|| format!("no {name}"))?;
        serde_json::from_str(data.biome_json(id)).map_err(|e| e.to_string())
    }

    /// A TerraForged world's pack (`write_pack`) as the generator reads it: its two biomes, built from the bundle's
    /// own 26.3 parents.
    fn terraforged_biomes(data: &GeneratorData) -> crate::worldgen::overrides::WorldgenOverrides {
        let mut ov = crate::worldgen::overrides::WorldgenOverrides::default();
        let (biomes, missing) = mod_biomes(|name| parent_of(data, name));
        assert!(missing.is_empty(), "{missing:?}");
        for (id, json) in biomes {
            ov.set("biomes", &id, json.to_string());
        }
        ov
    }

    /// #269: a TerraForged world made before its pack held TerraForged's biomes (its pack: the dimension type only)
    /// builds a generator whose GLOBAL caves hold `terraforged:cave` and whose UNIQUE pool holds both, from the biomes
    /// its table holds (the `finish` hook); a new world's pack is kept as it is, and a vanilla world gets neither.
    #[test]
    fn an_older_worlds_generator_takes_its_tables_biomes() {
        use super::super::terraforged_cave_noise::CaveType;
        use super::super::terraforged_mod_biomes::{MOD_BIOMES, remember_root};
        use super::super::terraforged_world_preset::overworld_type_json;
        use crate::worldgen::overrides::WorldgenOverrides;
        let Some(vanilla) = data() else { return };
        let root = std::env::var_os("HORIZON_TEST_RESOURCES")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/horizon-client/26.3")
            });
        remember_root(&root);
        // A TerraForged world's table: vanilla's biomes and TerraForged's (the `native_biome_files` hook).
        let table: Vec<String> = vanilla
            .biome_names()
            .iter()
            .map(|name| name.to_string())
            .chain(MOD_BIOMES.iter().map(|(id, _)| id.to_string()))
            .collect();
        let known = |id: &str| table.iter().any(|name| name == id);
        let pack = |biomes: Option<WorldgenOverrides>| {
            let mut ov = biomes.unwrap_or_default();
            ov.set(
                "dimension_types",
                "minecraft:overworld",
                overworld_type_json().to_owned(),
            );
            ov
        };
        let mut older = pack(None);
        assert!(older.finish(known).is_empty());
        let data = data_with(&older).unwrap();
        let cave = data.biome_id("terraforged:cave").unwrap();
        let oak = data.biome_id("terraforged:oak_forest").unwrap();
        let stem = terraforged_stem(&data, PRESET, "minecraft:overworld").unwrap();
        let pipeline = TerraForgedPipeline::new(&data, 7, &stem, false).unwrap();
        let sampler = pipeline.caves.biome_sampler();
        let global: Vec<_> = sampler.pool(CaveType::Global).values().to_vec();
        assert_eq!(global.iter().map(|id| **id).collect::<Vec<_>>(), [cave]);
        let unique = sampler.pool(CaveType::Unique).values();
        assert!(unique.iter().any(|id| **id == cave) && unique.iter().any(|id| **id == oak));
        // A new world's pack holds the same biomes: `finish` keeps it as it is.
        let mut fresh = pack(Some(terraforged_biomes(&vanilla)));
        let written = fresh.clone();
        assert!(fresh.finish(known).is_empty());
        assert_eq!(fresh, written);
        let parse = |ov: &WorldgenOverrides| -> Vec<(String, serde_json::Value)> {
            ov.entries("biomes")
                .map(|(id, json)| (id.to_owned(), serde_json::from_str(json).unwrap()))
                .collect()
        };
        assert_eq!(parse(&older), parse(&fresh));
        // A vanilla world's table holds neither: its packs gain no biome.
        let mut plain = pack(None);
        let vanilla_table = |id: &str| vanilla.biome_id(id).is_some();
        assert!(plain.finish(vanilla_table).is_empty());
        assert_eq!(plain.entries("biomes").count(), 0);
    }

    /// With the world's pack the generator has TerraForged's biomes where 6dd607eb places them: GLOBAL noise caves
    /// write `terraforged:cave` (in carved quarts only), neither is a surface biome (no `minecraft:is_overworld` tag,
    /// as in 6dd607eb; UNIQUE caves can still draw either, as they draw any registered biome), and `trees/patchy`
    /// (which names `terraforged:oak_forest`) binds wooded badlands.
    #[test]
    fn terraforged_biomes_generate_where_6dd607eb_places_them() {
        let Some(vanilla) = data() else { return };
        let data = data_with(&terraforged_biomes(&vanilla)).unwrap();
        let cave = data.biome_id("terraforged:cave").unwrap();
        let oak = data.biome_id("terraforged:oak_forest").unwrap();
        let stem = terraforged_stem(&data, PRESET, "minecraft:overworld").unwrap();
        let pipeline = TerraForgedPipeline::new(&data, 7, &stem, false).unwrap();
        let overworld = &pipeline.biomes.map.overworld;
        assert!(!overworld.iter().any(|id| **id == cave || **id == oak));
        assert!(
            overworld
                .iter()
                .any(|id| data.biome_name(**id) == "minecraft:plains")
        );
        let features = pipeline.features.as_ref().unwrap();
        for biome in ["minecraft:wooded_badlands", "terraforged:oak_forest"] {
            let (tag, ..) = features.vegetation_of(data.biome_id(biome).unwrap());
            assert_eq!(tag, Some("terraforged:trees/patchy"), "{biome}");
        }
        let overworld =
            OverworldGenerator::for_dimension(Arc::clone(&data), 7, PRESET, "minecraft:overworld")
                .unwrap();
        let (min_y, height) = overworld.level();
        let mut worker = overworld.worker();
        let mut caves = 0;
        for (cx, cz) in [(0, 0), (3, -2), (-5, 4), (8, 8), (-12, -9), (20, 3)] {
            let chunk = worker.generate_proto(ChunkPos::new(cx, cz)).unwrap();
            for y in (min_y..min_y + height).step_by(4) {
                for i in 0..16 {
                    let (x, z) = ((i % 4) * 4, (i / 4) * 4);
                    if chunk.biome_id_at(x, y, z) == cave {
                        assert!(carved_quart(&chunk, x, y, z), "{cx} {cz} {x} {y} {z}");
                        caves += 1;
                    }
                }
            }
        }
        assert!(caves > 0);
    }

    /// `/locate biome` (`findClosestBiome3d`) follows the spiral over TerraForged's noise biomes: it answers with the
    /// first sampled column whose biome is allowed, at the first sample height. Outside `possibleBiomes` it finds
    /// only what the noise caves carve (here, without `corrected_biomes`, any registered biome). Searching adds no
    /// terrain-cache entry.
    #[test]
    fn locating_a_biome_follows_the_noise_biomes() {
        let Some(data) = data() else { return };
        let stem = terraforged_stem(&data, PRESET, "minecraft:overworld").unwrap();
        let pipeline = Arc::new(TerraForgedPipeline::new(&data, 11, &stem, false).unwrap());
        let overworld = OverworldGenerator {
            pipeline: Arc::clone(&pipeline) as Arc<dyn SeededPipeline>,
            noise_clamped: None,
        };
        let mut worker = overworld.worker();
        // `search`'s radius, steps and level bounds.
        let (origin, radius, horizontal, vertical) = ([-1_203, 75, 2_051], 2_400, 32, 64);
        let (min_y, max_y) = (-64, 319);
        // The spiral's columns and their noise biomes, as the search samples them.
        let columns: Vec<[i32; 2]> = super::super::spiral_around(radius / horizontal)
            .map(|[sx, sz]| [origin[0] + sx * horizontal, origin[2] + sz * horizontal])
            .collect();
        let quarts: Vec<[i32; 3]> = columns.iter().map(|c| [c[0] >> 2, 0, c[1] >> 2]).collect();
        let biomes = worker.noise_biomes(&quarts);
        let first = biomes[0].clone().unwrap();
        let (index, target) = biomes
            .iter()
            .enumerate()
            .find_map(|(i, b)| b.as_ref().filter(|b| **b != first).map(|b| (i, b.clone())))
            .expect("one biome over the whole search");
        let first_index = biomes
            .iter()
            .position(|b| b.as_deref() == Some(target.as_str()))
            .unwrap();
        assert_eq!(first_index, index);
        let y = super::super::out_from_origin(origin[1], min_y + 1, max_y + 1, vertical)[0];
        fn search<'a>(allowed: &'a dyn Fn(&str) -> bool, origin: [i32; 3]) -> BiomeSearch<'a> {
            BiomeSearch {
                allowed,
                origin,
                radius: 2_400,
                horizontal: 32,
                vertical: 64,
                min_y: -64,
                max_y: 319,
            }
        }
        let only_target = |name: &str| name == target;
        let [x, z] = columns[index];
        assert_eq!(
            worker.find_closest_biome(&search(&only_target, origin)),
            Some((target.clone(), [x, y, z]))
        );
        // The origin's own biome is found at the origin.
        let only_first = |name: &str| name == first;
        assert_eq!(
            worker.find_closest_biome(&search(&only_first, origin)),
            Some((first.clone(), [origin[0], y, origin[2]]))
        );
        // A biome TerraForged's overworld never lists is found only where a unique cave carves it (they draw every
        // registered biome here), at a block of the cave; one no pool holds is not found.
        let nether = |name: &str| name == "minecraft:nether_wastes";
        let found = worker.find_closest_biome(&search(&nether, origin));
        assert!(
            found.as_ref().is_some_and(
                |(name, [_, cave_y, _])| name == "minecraft:nether_wastes" && *cave_y != y
            ),
            "{found:?}"
        );
        let missing = |name: &str| name == "minecraft:no_such_biome";
        assert_eq!(worker.find_closest_biome(&search(&missing, origin)), None);
        assert_eq!(pipeline.generator.cached_chunks(), 0);
    }
    /// Horizon's `rain_shadow` moves biomes, and every biome reader agrees on where: chunks' noise biomes,
    /// `/locate biome` and `/tf preview` read the same climate sampler.
    #[test]
    fn rain_shadow_biomes_agree_everywhere() {
        let Some(data) = data() else { return };
        terraforged_settings::set_for_thread(Some(Settings {
            rain_shadow: true,
            ..Settings::default()
        }));
        let (on, on_world) = terraforged(&data, 777, false);
        terraforged_settings::set_for_thread(Some(Settings::default()));
        let (off, _) = terraforged(&data, 777, false);
        terraforged_settings::set_for_thread(None);
        let (on_sampler, off_sampler) = (on.surface_sampler(), off.surface_sampler());
        let moved: Vec<(i32, i32, BiomeId)> = (0..48 * 48)
            .map(|i| (-6144 + (i % 48) * 256 + 4, -6144 + (i / 48) * 256 + 8))
            .filter_map(|(x, z)| {
                let biome = on.biome_at(&on_sampler, x, z)?;
                (Some(biome) != off.biome_at(&off_sampler, x, z)).then_some((x, z, biome))
            })
            .collect();
        eprintln!(
            "rain shadow: {} of {} columns change biome",
            moved.len(),
            48 * 48
        );
        assert!(!moved.is_empty());
        let mut worker = on_world.worker();
        for &(x, z, biome) in moved.iter().take(4) {
            let name = data.biome_name(biome).to_owned();
            // The chunk's noise biome at the column's quart.
            assert_eq!(
                worker.noise_biomes(&[[x >> 2, 0, z >> 2]])[0].as_deref(),
                Some(name.as_str())
            );
            // `/locate biome` from the column finds it there.
            let only = |candidate: &str| candidate == name;
            let found = worker.find_closest_biome(&BiomeSearch {
                allowed: &only,
                origin: [x, 80, z],
                radius: 64,
                horizontal: 32,
                vertical: 64,
                min_y: -64,
                max_y: 319,
            });
            assert!(
                found
                    .as_ref()
                    .is_some_and(|(found, [fx, _, fz])| *found == name && (*fx, *fz) == (x, z)),
                "{x} {z}: {name} vs {found:?}"
            );
            // `/tf preview` colours by the same sample.
            let sampler = super::super::terraforged_current_biome_sampler::CurrentBiomeSampler::new(
                on.generator.noise_generator(),
            );
            let seed = on.generator.seed();
            let sample = sampler.get_sample(seed, x, z);
            assert_eq!(
                super::super::terraforged_preview::world_color(&sampler, seed, x as f32, z as f32),
                super::super::terraforged_preview::biome_color(&sample, 0.2)
            );
        }
    }

    /// Distant tiles show the chunks' ground: the biome the chunk stores at the surface, the un-eroded height (within
    /// erosion's few blocks of the filled ground), the surface rule's top block, and the level's clipping; sampling
    /// them creates no terrain-cache entry and builds no erosion region.
    #[test]
    fn far_terrain_follows_the_chunks() {
        let Some(data) = data() else { return };
        let (pipeline, overworld) = terraforged(&data, 7, false);
        let generator = &pipeline.generator;
        let mut worker = overworld.worker();
        let far = worker.lod_grid(4104, -2040, 16, 16, false).unwrap();
        assert_eq!(far.len(), 256);
        assert!(
            far.iter()
                .all(|(sample, spans)| sample.is_some() && !spans.is_empty())
        );
        assert_eq!(
            (generator.cached_chunks(), generator.cached_regions()),
            (0, 0)
        );
        // One block per column over chunk (3, -2), then the chunk itself.
        let columns = worker.lod_grid(48, -32, 1, 16, false).unwrap();
        assert_eq!(
            (generator.cached_chunks(), generator.cached_regions()),
            (0, 0)
        );
        let chunk = worker.generate_proto(ChunkPos::new(3, -2)).unwrap();
        let zoom = horizon_core::biome::BiomeZoom::new(obfuscate_seed(7));
        let (mut off, mut worst, mut level, mut same, mut painted) = (0, 0, 0, 0, 0);
        for (index, (sample, spans)) in columns.iter().enumerate() {
            let (lx, lz) = ((index % 16) as i32, (index / 16) as i32);
            let (x, z) = (48 + lx, -32 + lz);
            let sample = sample.unwrap();
            let raw = generator.far_heights(x, z).unwrap();
            assert_eq!(sample.height, raw.0 + 1);
            assert_eq!(spans.last().unwrap().0, -64);
            let eroded = generator
                .base_height(x, z, TerraHeightmap::OceanFloorWorldgen)
                .unwrap();
            let difference = (sample.height - eroded).abs();
            off += difference;
            worst = worst.max(difference);
            if (2..14).contains(&lx) && (2..14).contains(&lz) {
                let quart = zoom.quart_at_block(BlockPos::new(x, sample.height - 1, z));
                assert_eq!(sample.biome, *chunk.biomes().noise_biome(quart), "{x} {z}");
            }
            painted += usize::from(sample.state != pipeline.stone);
            if sample.height == eroded && sample.water.is_none() {
                level += 1;
                same += usize::from(chunk.block(lx, sample.height - 1, lz) == sample.state);
            }
        }
        eprintln!("off {off} worst {worst} level {level} same {same} painted {painted}");
        assert!(
            worst <= 12 && off <= 3 * 256,
            "erosion moved the ground by {off} ({worst})"
        );
        assert!(painted > 200, "{painted} columns painted");
        assert!(
            level > 32 && same * 10 >= level * 8,
            "{same} of {level} tops match"
        );
    }

    /// Far terrain is clipped to the level as the fill clips it: flat at y 319 in a 26.3-high overworld, above it in
    /// the taller one.
    #[test]
    fn far_terrain_is_clipped_to_the_level() {
        let Some(normal) = data() else { return };
        let mut ov = crate::worldgen::overrides::WorldgenOverrides::default();
        ov.set(
            crate::worldgen::overrides::DIMENSION_TYPES,
            "minecraft:overworld",
            super::super::terraforged_world_preset::overworld_type_json().to_owned(),
        );
        let Some(tall) = data_with(&ov) else { return };
        // Seed 1234's peak near x -14848, z -32256 (see the taller overworld's test).
        let (pipeline, _) = terraforged(&normal, 1234, false);
        let peak = (0..64 * 64)
            .map(|i| (-14848 + (i % 64 - 32) * 16, -32256 + (i / 64 - 32) * 16))
            .max_by_key(|&(x, z)| pipeline.generator.far_heights(x, z).unwrap().0)
            .unwrap();
        assert!(pipeline.generator.far_heights(peak.0, peak.1).unwrap().0 > 340);
        for (data, expected) in [(&normal, Some(320)), (&tall, None)] {
            let (pipeline, overworld) = terraforged(data, 1234, false);
            let (sample, spans) = overworld
                .worker()
                .lod_grid(peak.0, peak.1, 1, 1, false)
                .unwrap()
                .pop()
                .unwrap();
            let height = sample.unwrap().height;
            match expected {
                Some(top) => assert_eq!(height, top),
                None => assert!(height > 340, "{height}"),
            }
            assert_eq!(spans.first().unwrap().1, height);
            assert_eq!(pipeline.generator.cached_regions(), 0);
        }
    }

    /// The block a far column's spans hold at `y` (spans run top down), if any.
    fn span_block(spans: &[(i32, i32, BlockStateId)], y: i32) -> Option<BlockStateId> {
        spans
            .iter()
            .find(|&&(min, max, _)| min <= y && y < max)
            .map(|span| span.2)
    }

    /// Reduced far detail changes only the blocks more than [`REDUCED_PAINT`] under a far column's top: over
    /// several seeds, tile positions and widths, every height, top block, water level and biome is the full tile's,
    /// so are the top two blocks, and every block below them is the full tile's or unpainted stone.
    #[test]
    fn reduced_far_detail_changes_only_blocks_under_the_top() {
        let Some(data) = data() else { return };
        let (mut columns, mut deep) = (0, 0);
        for seed in [7, 1234, 98765, -4_000_000_000] {
            let (pipeline, overworld) = terraforged(&data, seed, false);
            let mut worker = overworld.worker();
            for (x, z, width) in [(4104, -2040, 16), (-30_000, 12_345, 64), (517, 903, 4)] {
                let full = worker.lod_grid(x, z, width, 16, false).unwrap();
                let reduced = worker.lod_grid(x, z, width, 16, true).unwrap();
                assert_eq!(full.len(), reduced.len());
                for ((full, full_spans), (reduced, reduced_spans)) in full.iter().zip(&reduced) {
                    columns += 1;
                    let key = |sample: &Option<SurfaceSample>| {
                        sample.map(|s| (s.height, s.state, s.water, s.biome))
                    };
                    assert_eq!(key(full), key(reduced), "seed {seed} at {x} {z}");
                    let bottom = full_spans.last().map_or(0, |span| span.0);
                    let top = full_spans.first().map_or(0, |span| span.1);
                    assert_eq!(top, reduced_spans.first().map_or(0, |span| span.1));
                    let height = full.map_or(i32::MIN, |sample| sample.height);
                    for y in bottom..top {
                        let (f, r) = (span_block(full_spans, y), span_block(reduced_spans, y));
                        if y >= height - REDUCED_PAINT || f == r {
                            assert_eq!(f, r, "seed {seed} at {x} {z}, y {y}");
                        } else {
                            assert_eq!(r, Some(pipeline.stone), "seed {seed} at {x} {z}, y {y}");
                            deep += 1;
                        }
                    }
                }
            }
            assert_eq!(pipeline.generator.cached_regions(), 0);
        }
        eprintln!("{columns} columns, {deep} blocks left unpainted");
        assert!(
            deep > 0,
            "the full tiles painted nothing under their top two blocks"
        );
    }

    /// A pipeline of seed 7 with `settings` (`eroded_far_terrain` on).
    fn eroding(
        data: &Arc<GeneratorData>,
        settings: Settings,
    ) -> (Arc<TerraForgedPipeline>, OverworldGenerator) {
        assert!(settings.eroded_far_terrain);
        terraforged_settings::set_for_thread(Some(settings));
        let built = terraforged(data, 7, false);
        terraforged_settings::set_for_thread(None);
        built
    }

    /// The finest far tile (4-block cells) over chunks 3..=4, -2..=-1 (all in erosion region 0, -1): 64 columns.
    fn fine_tile(worker: &mut super::super::GeneratorWorker) -> Vec<LodColumn> {
        worker.lod_grid(50, -30, 4, 8, false).unwrap()
    }

    /// `eroded_far_terrain`: the finest far tiles stand on the chunks' ground once their erosion region is built, by the
    /// chunks (read from the fill's cache without building or evicting) or by the world's far erosion thread (into its
    /// own cache), and keep the un-eroded heights until then; wider cells never erode. At 64 columns a refined tile's
    /// heights are the chunks' OCEAN_FLOOR_WG and WORLD_SURFACE_WG heights, and both ways give the same tile.
    #[test]
    fn eroded_far_tiles_stand_on_the_chunks() {
        let _ = eroded_far_tiles_stand_on_the_chunks_with(Settings {
            eroded_far_terrain: true,
            ..terraforged_settings::FAITHFUL
        });
    }

    /// The same with `rock_strata` on: the far erosion thread erodes its regions with the strata's hardness, as the
    /// chunks' fill does, so the far tiles still stand on the chunks' ground, which the hardness moved.
    #[test]
    fn eroded_far_tiles_stand_on_the_chunks_with_rock_strata() {
        let faithful = eroded_far_tiles_stand_on_the_chunks_with(Settings {
            eroded_far_terrain: true,
            ..terraforged_settings::FAITHFUL
        });
        let strata = eroded_far_tiles_stand_on_the_chunks_with(Settings {
            eroded_far_terrain: true,
            rock_strata: true,
            ..terraforged_settings::FAITHFUL
        });
        let moved = faithful.iter().zip(&strata).filter(|(a, b)| a != b).count();
        eprintln!("the hardness moved {moved} of 64 far columns");
        assert!(
            moved > 0,
            "the hardness moved no column: the test proves nothing here"
        );
    }

    /// The same with `basin_erosion` on: the valleys are cut into the raw map of the far thread's regions as into the
    /// fill's, and into the un-eroded far tiles as into the plain sample, so the far tiles still stand on the chunks'
    /// ground, which the valleys moved.
    #[test]
    #[ignore = "real terrain: every query builds drainage tiles, too slow in debug; run with --profile fast --ignored"]
    fn eroded_far_tiles_stand_on_the_chunks_with_basin_erosion() {
        let faithful = eroded_far_tiles_stand_on_the_chunks_with(Settings {
            eroded_far_terrain: true,
            ..terraforged_settings::FAITHFUL
        });
        let basins = eroded_far_tiles_stand_on_the_chunks_with(Settings {
            eroded_far_terrain: true,
            basin_erosion: true,
            ..terraforged_settings::FAITHFUL
        });
        let moved = faithful.iter().zip(&basins).filter(|(a, b)| a != b).count();
        eprintln!("the valleys moved {moved} of 64 far columns");
        assert!(
            moved > 0,
            "the valleys moved no column: the test proves nothing here"
        );
    }

    /// The body of the tests above; the refined tile's heights.
    fn eroded_far_tiles_stand_on_the_chunks_with(settings: Settings) -> Vec<i32> {
        let Some(data) = data() else {
            return Vec::new();
        };
        let column = |index: usize| (50 + (index % 8) as i32 * 4, -30 + (index / 8) as i32 * 4);
        // The far thread: the first tile is un-eroded and queues the region, the thread builds it.
        let (far, far_overworld) = eroding(&data, settings);
        let mut far_worker = far_overworld.worker();
        let first = fine_tile(&mut far_worker);
        for (index, (sample, _)) in first.iter().enumerate() {
            let (x, z) = column(index);
            let raw = far.generator.far_heights(x, z).unwrap();
            assert_eq!(sample.unwrap().height, raw.0 + 1, "{x} {z}");
        }
        assert_eq!(far.generator.cached_far_regions(), (0, 1));
        far_worker.lod_grid(4104, -2040, 16, 4, false).unwrap();
        far_worker.lod_grid(50, -30, 8, 4, false).unwrap();
        assert_eq!(
            far.generator.cached_far_regions(),
            (0, 1),
            "wide cells queue nothing"
        );
        far.generator.start_far_erosion();
        let start = std::time::Instant::now();
        while !far
            .generator
            .far_eroded_heights()
            .heights(50, -30)
            .unwrap()
            .1
        {
            assert!(
                start.elapsed().as_secs() < 900,
                "the far region was never built"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let refined = fine_tile(&mut far_worker);
        assert_eq!(far.generator.cached_far_regions().0, 1);
        assert_eq!(
            far.generator.cached_regions(),
            0,
            "far terrain built no fill region"
        );
        // The chunks: their own fill builds the region, which far tiles of their pipeline then read.
        let (level, level_overworld) = eroding(&data, settings);
        let mut level_worker = level_overworld.worker();
        let chunks: Vec<ProtoChunk> = [(3, -2), (4, -2), (3, -1), (4, -1)]
            .into_iter()
            .map(|(x, z)| level_worker.generate_proto(ChunkPos::new(x, z)).unwrap())
            .collect();
        assert!(level.generator.cached_regions() >= 1);
        let peeked = fine_tile(&mut level_worker);
        assert_eq!(
            level.generator.cached_far_regions(),
            (0, 0),
            "a built fill region queues nothing"
        );
        // SurfaceSample has no PartialEq: compare its debug form.
        assert_eq!(
            format!("{peeked:?}"),
            format!("{refined:?}"),
            "the far thread's region is the fill's"
        );
        let mut moved = 0;
        for (index, (sample, spans)) in refined.iter().enumerate() {
            let (x, z) = column(index);
            let sample = sample.unwrap();
            let chunk = &chunks[((x >> 4) - 3 + ((z >> 4) + 2) * 2) as usize];
            let height = |kind| {
                chunk
                    .terrain
                    .heightmap(kind)
                    .unwrap()
                    .first_available(x & 15, z & 15)
                    .unwrap()
            };
            assert_eq!(
                sample.height,
                height(HeightmapType::OceanFloorWg),
                "{x} {z}"
            );
            let surface = sample.water.map_or(sample.height, |(top, _)| top);
            assert_eq!(surface, height(HeightmapType::WorldSurfaceWg), "{x} {z}");
            assert_eq!(spans.first().unwrap().1, surface);
            moved += usize::from(sample.height != first[index].0.unwrap().height);
        }
        eprintln!("erosion moved {moved} of 64 far columns");
        assert!(
            moved > 0,
            "erosion moved no column: the test proves nothing here"
        );
        refined
            .iter()
            .map(|(sample, _)| sample.unwrap().height)
            .collect()
    }

    /// Far terrain's cost, by part and per tile (run with `--ignored --nocapture` in an optimized build).
    #[test]
    #[ignore = "benchmark; run by hand with --ignored --nocapture"]
    fn far_terrain_benchmark() {
        let Some(data) = data() else { return };
        let (pipeline, overworld) = terraforged(&data, 98765, false);
        let mut worker = overworld.worker();
        let tiles: Vec<(i32, i32)> = (0..16)
            .map(|tile| (5000 + (tile % 4) * 512, 5000 + (tile / 4) * 512))
            .collect();
        let columns: Vec<(i32, i32)> = (0..16 * 32 * 32)
            .map(|i| (5000 + (i % 128) * 16, 5000 + (i / 128) * 16))
            .collect();
        let start = std::time::Instant::now();
        for &(x, z) in &columns {
            std::hint::black_box(pipeline.generator.far_heights(x, z).unwrap());
        }
        let heights = start.elapsed();
        let start = std::time::Instant::now();
        let sampler = pipeline.surface_sampler();
        for &(x, z) in &columns {
            std::hint::black_box(pipeline.biome_at(&sampler, x & !3, z & !3));
        }
        let biomes = start.elapsed();
        eprintln!(
            "{} columns: far_heights {heights:?}, biome_at {biomes:?}",
            columns.len()
        );
        // Each run samples its own area, so none reads cells another warmed.
        let runs = [(16, false), (16, true), (64, false), (64, true)];
        for (run, (width, approximate)) in runs.into_iter().enumerate() {
            let offset = 200_000 * (run as i32 + 1);
            let start = std::time::Instant::now();
            for &(x, z) in &tiles {
                let tile = worker.lod_grid(x + offset, z, width, 32, approximate);
                std::hint::black_box(tile.unwrap());
            }
            eprintln!(
                "{width}-block cells, {}: {:?} per 32x32 tile",
                if approximate { "reduced" } else { "full" },
                start.elapsed() / tiles.len() as u32
            );
        }
    }

    /// What far-decoration pipelines cost beside the level's: each of `TF_FAR_THREADS` (12) threads builds its pipeline
    /// and decorates a far 3x3, as `vanillagen/lod.rs`'s threads do. `TF_FAR_SHARED=0` builds each with its own noise (before #267),
    /// otherwise they share the level's. Run each mode in its own process (`--ignored --nocapture`, optimized): the
    /// RSS lines are the process's. The level pipeline decorates three 3x3s of its own meanwhile.
    #[test]
    #[ignore = "benchmark; run by hand with --ignored --nocapture"]
    fn far_pipelines_benchmark() {
        let Some(data) = data() else { return };
        let shared = std::env::var("TF_FAR_SHARED").map_or(true, |v| v != "0");
        let threads: i32 = std::env::var("TF_FAR_THREADS").map_or(12, |v| v.parse().unwrap());
        let rss = || {
            let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
            let field = |name: &str| {
                status
                    .lines()
                    .find_map(|line| line.strip_prefix(name))
                    .map_or(0, |kb| {
                        kb.trim()
                            .trim_end_matches(" kB")
                            .parse::<u64>()
                            .unwrap_or(0)
                            / 1024
                    })
            };
            format!("{} MiB (peak {} MiB)", field("VmRSS:"), field("VmHWM:"))
        };
        const SEED: i64 = 777;
        const OVERWORLD: &str = "minecraft:overworld";
        let stem = terraforged_stem(&data, PRESET, OVERWORLD).unwrap();
        let region = |pipeline: TerraForgedPipeline, center: ChunkPos| {
            let overworld = OverworldGenerator {
                pipeline: Arc::new(pipeline),
                noise_clamped: None,
            };
            let mut worker = overworld.worker();
            let mut region = region_around(&mut worker, center);
            worker.decorate(&mut region).unwrap();
            overworld
        };
        let start = std::time::Instant::now();
        let level = live_pipeline(&data, SEED, &stem, OVERWORLD, true).unwrap();
        let level = region(level, ChunkPos::new(0, 0));
        eprintln!(
            "level pipeline and its first 3x3: {:?}, RSS {}",
            start.elapsed(),
            rss()
        );
        let start = std::time::Instant::now();
        let far: Vec<_> = std::thread::scope(|scope| {
            let threads: Vec<_> = (0..threads)
                .map(|t| {
                    let (data, stem) = (&data, &stem);
                    scope.spawn(move || {
                        let pipeline = if shared {
                            live_pipeline(data, SEED, stem, OVERWORLD, true).unwrap()
                        } else {
                            TerraForgedPipeline::new(data, SEED, stem, true).unwrap()
                        };
                        let built = start.elapsed();
                        let overworld = region(pipeline, ChunkPos::new(40 + 6 * t, -40));
                        (built, start.elapsed(), overworld)
                    })
                })
                .collect();
            // The level's own chunks while the far pipelines run (gameplay's generation beside far decoration).
            let mut worker = level.worker();
            let gameplay = std::time::Instant::now();
            for k in 0..3 {
                let mut region = region_around(&mut worker, ChunkPos::new(-30, 3 * k));
                worker.decorate(&mut region).unwrap();
            }
            eprintln!(
                "level: three 3x3s beside the far pipelines in {:?}",
                gameplay.elapsed()
            );
            threads.into_iter().map(|t| t.join().unwrap()).collect()
        });
        let built = far.iter().map(|f| f.0).max().unwrap();
        let first = far.iter().map(|f| f.1).min().unwrap();
        let last = far.iter().map(|f| f.1).max().unwrap();
        eprintln!(
            "{threads} far pipelines ({}): built by {built:?}, first decorated 3x3 at {first:?}, all at {last:?}, RSS {}",
            if shared {
                "sharing the level's noise"
            } else {
                "each with its own noise"
            },
            rss()
        );
    }

    /// What `eroded_far_terrain` costs (`--ignored --nocapture`, optimized): the finest far tiles (16x16 cells of 4
    /// blocks, as Horizon samples them) without the setting, with it before their regions are built (the first pass)
    /// and after; the far thread's time per region; and the level's 3x3s with the far thread idle and while it builds
    /// (here at the test's normal priority, not the idle priority of a far-decoration thread: the worst case).
    #[test]
    #[ignore = "benchmark; run by hand with --ignored --nocapture"]
    fn eroded_far_terrain_benchmark() {
        let Some(data) = data() else { return };
        const TILES: i32 = 48;
        const AT: i32 = 100_000;
        let tile = |t: i32| (20_000 + AT + (t % 8) * 1024, 20_000 + (t / 8) * 1024);
        let pass = |worker: &mut super::super::GeneratorWorker| {
            let start = std::time::Instant::now();
            for t in 0..TILES {
                let (x, z) = tile(t);
                std::hint::black_box(worker.lod_grid(x, z, 4, 16, false).unwrap());
            }
            start.elapsed() / TILES as u32
        };
        let with = |on: bool| {
            terraforged_settings::set_for_thread(Some(Settings {
                eroded_far_terrain: on,
                ..terraforged_settings::FAITHFUL
            }));
            let built = terraforged(&data, 98765, false);
            terraforged_settings::set_for_thread(None);
            built
        };
        // Each pipeline has its own noise, so every pass starts from cold caches over the same tiles.
        let off = pass(&mut with(false).1.worker());
        let (pipeline, overworld) = with(true);
        let mut worker = overworld.worker();
        let first = pass(&mut worker);
        let off_again = pass(&mut with(false).1.worker());
        let queued = pipeline.generator.cached_far_regions().1;
        eprintln!(
            "finest far tiles: {off:?} and {off_again:?} per tile without the setting, {first:?} on its first pass ({queued} regions queued)"
        );
        let mut level = overworld.worker();
        let mut decorate = |x: i32| {
            let start = std::time::Instant::now();
            let mut region = region_around(&mut level, ChunkPos::new(x, 40));
            level.decorate(&mut region).unwrap();
            start.elapsed()
        };
        decorate(-100);
        let idle: Vec<_> = (1..=3).map(|k| decorate(-100 - 10 * k)).collect();
        let start = std::time::Instant::now();
        pipeline.generator.start_far_erosion();
        let busy: Vec<_> = (4..=6).map(|k| decorate(-100 - 10 * k)).collect();
        let left = pipeline.generator.cached_far_regions().1;
        eprintln!(
            "level 3x3s: {idle:?} beside the idle far thread, {busy:?} while it builds ({left} regions still queued after)"
        );
        let (last_x, last_z) = tile(TILES - 1);
        // The last region taken may still be building when the queue is empty.
        while pipeline.generator.cached_far_regions().1 > 0
            || !pipeline
                .generator
                .far_eroded_heights()
                .heights(last_x, last_z)
                .unwrap()
                .1
        {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let built = start.elapsed();
        let refined = pass(&mut worker);
        eprintln!(
            "far thread: {queued} regions in {built:?} ({:?} each, the 3x3s beside it included); refined tiles {refined:?} per tile",
            built / queued.max(1) as u32
        );
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
            corrected
                .iter()
                .any(|name| super::super::terraforged_biome_registry::NON_SURFACE
                    .contains(&name.as_str())),
            "{corrected:?}"
        );
    }

    /// Seed 777's noise biomes over 4096 blocks around spawn, with `corrected_biomes` off and on. On, no column
    /// takes a cave biome, and the only biomes that aren't climate choices are the water biomes TerraForged's
    /// overrides place; `/locate biome` finds no cave biome among the noise biomes (it finds them in caves, see
    /// `locate_finds_cave_biomes_where_the_chunks_hold_them`), and the chunks' biome layer is the noise biomes'.
    #[test]
    fn corrected_biomes_keep_seed_777_s_surface_to_climate_biomes() {
        let Some(data) = data() else { return };
        let entries = registry_entries(&data);
        let entry = |name: &str| entries.iter().find(|e| *e.name == *name).unwrap();
        let overrides = [
            "minecraft:deep_cold_ocean",
            "minecraft:deep_frozen_ocean",
            "minecraft:deep_lukewarm_ocean",
            "minecraft:deep_ocean",
            "minecraft:cold_ocean",
            "minecraft:frozen_ocean",
            "minecraft:warm_ocean",
            "minecraft:ocean",
            "minecraft:snowy_beach",
            "minecraft:stony_shore",
            "minecraft:beach",
            "minecraft:frozen_river",
            "minecraft:river",
        ];
        let quarts: Vec<[i32; 3]> = (-64..64)
            .flat_map(|x| (-64..64).map(move |z| [x * 8, 0, z * 8]))
            .collect();
        let run = |corrected| {
            terraforged_settings::set_for_thread(Some(Settings {
                corrected_biomes: corrected,
                ..terraforged_settings::FAITHFUL
            }));
            let (pipeline, overworld) = terraforged(&data, 777, false);
            terraforged_settings::set_for_thread(None);
            assert_eq!(pipeline.settings.corrected_biomes, corrected);
            let mut worker = overworld.worker();
            let biomes = worker.noise_biomes(&quarts);
            let mut counts = BTreeMap::<String, usize>::new();
            for biome in biomes.iter().flatten() {
                *counts.entry(biome.clone()).or_default() += 1;
            }
            let mut search = |name: &'static str| {
                let allowed = move |candidate: &str| candidate == name;
                worker.find_closest_biome(&BiomeSearch {
                    allowed: &allowed,
                    origin: [0, 64, 0],
                    radius: 2_048,
                    horizontal: 32,
                    vertical: 64,
                    min_y: -64,
                    max_y: 319,
                })
            };
            let deep_dark = search("minecraft:deep_dark");
            let chunk = overworld
                .worker()
                .generate_proto(ChunkPos::new(3, -5))
                .unwrap();
            let layer: Vec<_> = (0..4)
                .flat_map(|x| (0..4).map(move |z| [x, z]))
                .map(|[x, z]| chunk.biome_at(x * 4, 300, z * 4).to_owned())
                .collect();
            let noise: Vec<_> = (0..4)
                .flat_map(|x| (0..4).map(move |z| [12 + x, 0, -20 + z]))
                .collect();
            let noise = overworld.worker().noise_biomes(&noise);
            (counts, deep_dark, layer, noise)
        };
        let (faithful, faithful_dark, _, _) = run(false);
        let (corrected, corrected_dark, layer, noise) = run(true);
        eprintln!("faithful {faithful:?}\ncorrected {corrected:?}");
        assert!(
            faithful
                .keys()
                .any(|name| !entry(name).is_climate_biome() && !overrides.contains(&name.as_str())),
            "{faithful:?}"
        );
        for name in corrected.keys() {
            assert!(
                entry(name).is_climate_biome() || overrides.contains(&name.as_str()),
                "{name} in {corrected:?}"
            );
        }
        assert!(faithful_dark.is_some() || !faithful.contains_key("minecraft:deep_dark"));
        // On, deep_dark is no noise biome: when `/locate biome` finds it, it is in a cave (not at the first height).
        assert!(
            corrected_dark
                .as_ref()
                .is_none_or(|(_, [_, y, _])| *y != 64),
            "{corrected_dark:?}"
        );
        assert_eq!(
            layer,
            noise.into_iter().map(Option::unwrap).collect::<Vec<_>>()
        );
    }

    /// `/locate biome` finds the biomes only caves hold, with `corrected_biomes` off and on, in seed 777's world with
    /// TerraForged's pack: the chunk generated at each answer holds the biome at the answered block, in a carved
    /// quart below the surface. Off, the cave biomes are noise biomes and are found as before (at the search's first
    /// height), while `terraforged:cave` and the biomes unique caves draw from every registered one (`the_end`,
    /// `oak_forest`) are found underground; on, every cave biome is, and `oak_forest`, which no climate and no cave
    /// places then, is not found.
    #[test]
    fn locate_finds_cave_biomes_where_the_chunks_hold_them() {
        let Some(vanilla) = data() else { return };
        let data = data_with(&terraforged_biomes(&vanilla)).unwrap();
        for corrected in [false, true] {
            terraforged_settings::set_for_thread(Some(Settings {
                corrected_biomes: corrected,
                ..terraforged_settings::FAITHFUL
            }));
            let (pipeline, overworld) = terraforged(&data, 777, false);
            terraforged_settings::set_for_thread(None);
            assert_eq!(pipeline.settings.corrected_biomes, corrected);
            let mut worker = overworld.worker();
            let mut search = |name: &'static str| {
                let allowed = move |candidate: &str| candidate == name;
                let start = std::time::Instant::now();
                let found = worker.find_closest_biome(&BiomeSearch {
                    allowed: &allowed,
                    origin: [0, 64, 0],
                    radius: 6400,
                    horizontal: 32,
                    vertical: 64,
                    min_y: -64,
                    max_y: 959,
                });
                eprintln!("{corrected} {name}: {found:?} in {:?}", start.elapsed());
                found
            };
            let mut underground = vec!["terraforged:cave"];
            if corrected {
                underground.extend([
                    "minecraft:lush_caves",
                    "minecraft:dripstone_caves",
                    "minecraft:deep_dark",
                ]);
            } else {
                underground.extend(["minecraft:the_end", "terraforged:oak_forest"]);
                for name in [
                    "minecraft:lush_caves",
                    "minecraft:dripstone_caves",
                    "minecraft:deep_dark",
                ] {
                    if let Some((_, [_, y, _])) = search(name) {
                        assert_eq!(y, 64, "{name} is a noise biome");
                    }
                }
            }
            for name in underground {
                let (found, [x, y, z]) =
                    search(name).unwrap_or_else(|| panic!("{corrected} {name}"));
                assert_eq!(found, name);
                let chunk = overworld
                    .worker()
                    .generate_proto(ChunkPos::new(x >> 4, z >> 4))
                    .unwrap();
                assert_eq!(
                    chunk.biome_at(x & 15, y, z & 15),
                    name,
                    "{corrected} {x} {y} {z}"
                );
                assert_eq!(
                    chunk.block_name(x & 15, y, z & 15),
                    "minecraft:air",
                    "{corrected} {name} {x} {y} {z}"
                );
            }
            if corrected {
                assert_eq!(search("terraforged:oak_forest"), None);
            }
        }
    }

    /// `erosion_deposits` lays gravel, clay and cobblestone over the natural surface where erosion deposited, and
    /// changes nothing else: every surface height is the same, every changed block was natural ground and is now a
    /// deposit block, within three blocks of the top, and no column of the sea or its beaches changes. Off, the
    /// pipeline is the faithful one (its digest is unchanged, see the review notes of #247).
    #[test]
    fn erosion_deposits_change_only_the_natural_surface() {
        let Some(data) = data() else { return };
        let worker = |deposits| {
            terraforged_settings::set_for_thread(Some(Settings {
                erosion_deposits: deposits,
                ..terraforged_settings::FAITHFUL
            }));
            let (pipeline, overworld) = terraforged(&data, 98765, false);
            terraforged_settings::set_for_thread(None);
            assert_eq!(pipeline.deposits.is_some(), deposits);
            overworld
        };
        let (off, on) = (worker(false), worker(true));
        let (mut off, mut on) = (off.worker(), on.worker());
        let natural = [
            "minecraft:grass_block",
            "minecraft:dirt",
            "minecraft:coarse_dirt",
            "minecraft:podzol",
            "minecraft:sand",
            "minecraft:red_sand",
            "minecraft:stone",
            "minecraft:mud",
        ];
        let deposits = [
            "minecraft:gravel",
            "minecraft:clay",
            "minecraft:cobblestone",
        ];
        let mut changed = std::collections::BTreeMap::<String, usize>::new();
        for cx in -4..=4 {
            for cz in -4..=4 {
                let a = off.generate_proto(ChunkPos::new(cx, cz)).unwrap();
                let b = on.generate_proto(ChunkPos::new(cx, cz)).unwrap();
                for x in 0..16 {
                    for z in 0..16 {
                        let top = a.surface_height(x, z);
                        assert_eq!(top, b.surface_height(x, z), "{cx} {cz} {x} {z}");
                        // The top solid block, under any water.
                        let mut floor = top - 1;
                        while floor > -64
                            && ["minecraft:water", "minecraft:air"]
                                .contains(&a.block_name(x, floor, z))
                        {
                            floor -= 1;
                        }
                        // Deposits never reach below the floor's third block; look further.
                        for y in floor - 8..top {
                            let (before, after) = (a.block_name(x, y, z), b.block_name(x, y, z));
                            if before == after {
                                continue;
                            }
                            let biome = b.biome_at(x, top, z);
                            assert!(
                                natural.contains(&before)
                                    && deposits.contains(&after)
                                    && (floor - 2..=floor).contains(&y)
                                    && !biome.contains("ocean")
                                    && !biome.contains("beach"),
                                "{cx} {cz} {x} {y} {z}: {before} -> {after} in {biome}"
                            );
                            *changed.entry(after.to_owned()).or_default() += 1;
                        }
                    }
                }
            }
        }
        eprintln!("deposits {changed:?}");
        assert!(changed.values().sum::<usize>() > 500, "{changed:?}");
        assert!(changed.contains_key("minecraft:clay"), "{changed:?}");
    }

    /// `realistic_vegetation` overlays every vegetation config (off, they are TerraForged's), and on seed 98765's
    /// terrain it moves the fitness the way the light and the water say: up on north-facing slopes and near water,
    /// down on south-facing slopes and far from it. The data behind it is real: plenty of columns face each way and
    /// lie near and far from water.
    #[test]
    fn realistic_vegetation_follows_aspect_and_water() {
        use super::super::terraforged_biome_viability::TerrainViabilityContext;
        let Some(data) = data() else { return };
        let pipeline = |on| {
            terraforged_settings::set_for_thread(Some(Settings {
                realistic_vegetation: on,
                ..terraforged_settings::FAITHFUL
            }));
            let (pipeline, _) = terraforged(&data, 98765, false);
            terraforged_settings::set_for_thread(None);
            pipeline
        };
        let (off, on) = (pipeline(false), pipeline(true));
        let (off_configs, on_configs) = (
            off.features.as_ref().unwrap().configs(),
            on.features.as_ref().unwrap().configs(),
        );
        // [north, south, wet, dry]: columns and summed fitness change.
        let mut groups = [(0_usize, 0.0_f64); 4];
        let seed = off.generator.seed();
        // Chunks spread over 2,000 blocks: spawn lies by water, the land around it mostly doesn't.
        for cx in (-2..=2).map(|i| i * 23) {
            for cz in (-2..=2).map(|i| i * 19) {
                let handle = off.generator.chunk_data(seed, cx, cz);
                let terrain = handle.read();
                let context = TerrainViabilityContext(&terrain);
                for (x, z) in (0..16).flat_map(|x| (0..16).map(move |z| (x, z))) {
                    if terrain.height_at(x, z) <= 63 {
                        continue;
                    }
                    let (bx, bz) = ((cx << 4) + x, (cz << 4) + z);
                    for (a, b) in off_configs.iter().zip(on_configs) {
                        let before = a.viability.try_fitness(bx, bz, &context).unwrap();
                        let after = b.viability.try_fitness(bx, bz, &context).unwrap();
                        let change = f64::from(after - before);
                        let north = terrain.facing(x, z, Facing::North);
                        let water = terrain.water_distance(x, z);
                        for (group, member) in
                            [north > 0.2, north < -0.2, water < 0.25, water > 0.75]
                                .into_iter()
                                .enumerate()
                        {
                            if member {
                                groups[group].0 += 1;
                                groups[group].1 += change;
                            }
                        }
                    }
                }
            }
        }
        let mean = |(count, sum): (usize, f64)| sum / count.max(1) as f64;
        eprintln!("[north, south, wet, dry] (columns x configs, total change): {groups:?}");
        for (count, _) in groups {
            assert!(count > 8 * 100, "{groups:?}");
        }
        assert!(mean(groups[0]) > mean(groups[1]) + 0.1, "{groups:?}");
        assert!(mean(groups[2]) > mean(groups[3]) + 0.1, "{groups:?}");
    }

    /// `realistic_vegetation`'s ground cover turns some grass blocks open to the sky into podzol and moss on shaded
    /// ground and coarse dirt on sunny ground, and changes nothing else (not a height, nor any other block).
    #[test]
    fn realistic_ground_cover_changes_only_grass_by_its_slope() {
        let Some(data) = data() else { return };
        let worker = |on| {
            terraforged_settings::set_for_thread(Some(Settings {
                realistic_vegetation: on,
                ..terraforged_settings::FAITHFUL
            }));
            let (pipeline, overworld) = terraforged(&data, 98765, false);
            terraforged_settings::set_for_thread(None);
            assert_eq!(pipeline.ground_cover.is_some(), on);
            (pipeline, overworld)
        };
        let ((pipeline, off), (_, on)) = (worker(false), worker(true));
        let (mut off, mut on) = (off.worker(), on.worker());
        // Per cover block: columns, and their summed northness.
        let mut changed = BTreeMap::<String, (usize, f32)>::new();
        for cx in (-2..=2).map(|i| i * 23) {
            for cz in (-2..=2).map(|i| i * 19) {
                let a = off.generate_proto(ChunkPos::new(cx, cz)).unwrap();
                let b = on.generate_proto(ChunkPos::new(cx, cz)).unwrap();
                let handle = pipeline
                    .generator
                    .chunk_data(pipeline.generator.seed(), cx, cz);
                let terrain = handle.read();
                for (x, z) in (0..16).flat_map(|x| (0..16).map(move |z| (x, z))) {
                    let top = a.surface_height(x, z);
                    assert_eq!(top, b.surface_height(x, z), "{cx} {cz} {x} {z}");
                    for y in top - 12..top + 4 {
                        let (before, after) = (a.block_name(x, y, z), b.block_name(x, y, z));
                        if before == after {
                            continue;
                        }
                        assert!(
                            before == "minecraft:grass_block"
                                && y == top - 1
                                && [
                                    "minecraft:podzol",
                                    "minecraft:moss_block",
                                    "minecraft:coarse_dirt"
                                ]
                                .contains(&after),
                            "{cx} {cz} {x} {y} {z}: {before} -> {after}"
                        );
                        let entry = changed.entry(after.to_owned()).or_default();
                        entry.0 += 1;
                        entry.1 += terrain.facing(x, z, Facing::North);
                    }
                }
            }
        }
        eprintln!("ground cover (columns, summed northness): {changed:?}");
        let podzol = changed.get("minecraft:podzol").copied().unwrap_or_default();
        let coarse = changed
            .get("minecraft:coarse_dirt")
            .copied()
            .unwrap_or_default();
        assert!(podzol.0 > 50 && coarse.0 > 50, "{changed:?}");
        assert!(
            podzol.1 / (podzol.0 as f32) > 0.2 && coarse.1 / (coarse.0 as f32) < -0.2,
            "{changed:?}"
        );
    }

    /// `rock_strata` on seed 777, 25 chunks spread over 2,000 blocks: the bands (granite, diorite, andesite) fill
    /// the stone from y 9 up to 8 blocks under the floor, where faithful chunks have nearly none; the hardness
    /// changes the eroded heights of some columns, by a few blocks at most. Decorated, the banded region keeps its
    /// ores.
    #[test]
    #[ignore = "slow in debug: run with --profile fast --include-ignored"]
    fn rock_strata_band_the_stone_and_keep_the_ores() {
        let Some(data) = data() else { return };
        let worker = |on| {
            terraforged_settings::set_for_thread(Some(Settings {
                rock_strata: on,
                ..terraforged_settings::FAITHFUL
            }));
            let (pipeline, overworld) = terraforged(&data, 777, false);
            terraforged_settings::set_for_thread(None);
            assert_eq!(pipeline.strata.is_some(), on);
            overworld
        };
        let (off, on) = (worker(false), worker(true));
        let (mut off, mut on) = (off.worker(), on.worker());
        let banded = [
            "minecraft:granite",
            "minecraft:diorite",
            "minecraft:andesite",
        ];
        let (mut bands, mut changed, mut largest) = (0usize, 0usize, 0i32);
        let (mut wrong, mut own) = (Vec::new(), 0usize);
        for cx in (-2..=2).map(|i| i * 25) {
            for cz in (-2..=2).map(|i| i * 25) {
                let a = off.generate_proto(ChunkPos::new(cx, cz)).unwrap();
                let b = on.generate_proto(ChunkPos::new(cx, cz)).unwrap();
                for (x, z) in (0..16).flat_map(|x| (0..16).map(move |z| (x, z))) {
                    let (top_a, top_b) = (a.surface_height(x, z), b.surface_height(x, z));
                    changed += usize::from(top_a != top_b);
                    largest = largest.max((top_a - top_b).abs());
                    for y in a.terrain.min_y()..top_a.max(top_b) {
                        // Faithful chunks have a few of these blocks of their own (surface rules).
                        let before = banded.contains(&a.block_name(x, y, z));
                        own += usize::from(before);
                        if !before && banded.contains(&b.block_name(x, y, z)) {
                            bands += 1;
                            if !(9..=top_b - 8).contains(&y) {
                                wrong.push(format!("on {cx} {cz} {x} {y} {z} under {top_b}"));
                            }
                        }
                    }
                }
            }
        }
        eprintln!(
            "banded blocks {bands} (faithful's own {own}), columns whose height changed {changed}, largest \
             change {largest}"
        );
        wrong.truncate(20);
        assert!(wrong.is_empty(), "{wrong:?}");
        assert!(bands > 25 * 256 * 20 && own * 100 < bands, "{bands} {own}");
        assert!(changed > 100 && largest <= 8, "{changed} {largest}");
        let ores = |worker: &mut super::super::GeneratorWorker| {
            let mut region = region_around(worker, ChunkPos::new(3, -2));
            worker.decorate(&mut region).unwrap();
            let chunk = &region[4];
            let (min_y, height) = (chunk.terrain.min_y(), chunk.terrain.height());
            (min_y..min_y + height)
                .flat_map(|y| (0..256).map(move |i| (i & 15, y, i >> 4)))
                .filter(|&(x, y, z)| chunk.block_name(x, y, z).ends_with("_ore"))
                .count()
        };
        let (ores_off, ores_on) = (ores(&mut off), ores(&mut on));
        eprintln!("ores off {ores_off}, on {ores_on}");
        assert!(
            ores_on * 10 >= ores_off * 8 && ores_off > 0,
            "{ores_off} {ores_on}"
        );
    }

    /// `coastal_landforms` on seed 777, at a cliff coast (-3693, -5947) and a sheltered one (-5757, -2875), 32x32
    /// columns each: the cliff stands up to 30 blocks over the faithful shore, as a steep face, with stony shores on
    /// its top where the faithful shore has beaches; the flat lays more of the sea floor one block under the water;
    /// structures' base heights stay those of the filled chunks, and chunks are the same on 6 threads in rotated
    /// orders.
    #[test]
    #[ignore = "slow in debug: run with --profile fast --include-ignored"]
    fn coastal_landforms_cut_cliffs_and_lay_flats() {
        let Some(data) = data() else { return };
        let make = |on| {
            terraforged_settings::set_for_thread(Some(Settings {
                coastal_landforms: on,
                ..terraforged_settings::FAITHFUL
            }));
            let pipeline = terraforged(&data, 777, false);
            terraforged_settings::set_for_thread(None);
            pipeline
        };
        let ((off, _), (on, overworld)) = (make(false), make(true));
        let height = |p: &TerraForgedPipeline, x, z| {
            p.generator
                .base_height(x, z, TerraHeightmap::OceanFloorWorldgen)
                .unwrap()
        };
        let biome = |p: &TerraForgedPipeline, x, z| {
            let sampler = p.surface_sampler();
            data.biome_name(p.biome_at(&sampler, x, z).unwrap())
                .to_owned()
        };
        let area = |x0: i32, z0: i32| {
            (0..32).flat_map(move |i| (0..32).map(move |j| (x0 - 16 + i, z0 - 16 + j)))
        };
        // The cliff.
        let (mut raised, mut steep_off, mut steep_on) = (0i32, 0i32, 0i32);
        let (mut beaches, mut stony) = (0, 0);
        for (x, z) in area(-3693, -5947) {
            let (a, b) = (height(&off, x, z), height(&on, x, z));
            raised = raised.max(b - a);
            steep_off = steep_off.max(
                (height(&off, x + 1, z) - a)
                    .abs()
                    .max((height(&off, x, z + 1) - a).abs()),
            );
            steep_on = steep_on.max(
                (height(&on, x + 1, z) - b)
                    .abs()
                    .max((height(&on, x, z + 1) - b).abs()),
            );
            if biome(&off, x, z) == "minecraft:beach" {
                beaches += 1;
                stony += usize::from(biome(&on, x, z) == "minecraft:stony_shore");
            }
        }
        eprintln!(
            "COAST cliff raised {raised}, steepest step off {steep_off} on {steep_on}, beaches {beaches} stony {stony}"
        );
        assert!((15..=31).contains(&raised), "{raised}");
        assert!(
            steep_on >= 4 && steep_on > 2 * steep_off.max(1),
            "{steep_off} {steep_on}"
        );
        assert!(
            beaches > 0 && stony * 10 >= beaches * 9,
            "{beaches} {stony}"
        );
        // The flat: sea floor one block under the sea (water up to y 62).
        let awash = |p: &TerraForgedPipeline| {
            area(-5757, -2875)
                .filter(|&(x, z)| height(p, x, z) == 62)
                .count()
        };
        let (flat_off, flat_on) = (awash(&off), awash(&on));
        eprintln!("COAST flat floor at y 61: off {flat_off} on {flat_on}");
        assert!(flat_on > flat_off + 200, "{flat_off} {flat_on}");
        // The chunks: their ground is the base height structures read, and threads don't change them. The ground is
        // the top block that is not air, water or a frozen ocean's iceberg (vanilla's `frozenOceanExtension` stacks
        // packed ice and snow blocks on the sea floor in the surface stage, as on any frozen ocean; base heights, like
        // vanilla's `getBaseHeight`, read the terrain under them). The threads compare the whole top, icebergs too.
        let top_where = |chunk: &ProtoChunk, x: i32, z: i32, skip: &[&str]| {
            (chunk.terrain.min_y()..chunk.terrain.min_y() + chunk.terrain.height())
                .rev()
                .find(|&y| !skip.contains(&chunk.block_name(x, y, z)))
                .unwrap()
        };
        let (open, ground) = (
            ["minecraft:air", "minecraft:water"].as_slice(),
            [
                "minecraft:air",
                "minecraft:water",
                "minecraft:packed_ice",
                "minecraft:snow_block",
            ]
            .as_slice(),
        );
        // An iceberg can also replace the floor's own top blocks, so columns holding one are left out of the
        // base-height check (counted, and few); every other column must match exactly.
        let mut worker = overworld.worker();
        let chunks = [(-231, -372), (-231, -371), (-360, -180), (-359, -180)];
        let mut icebergs = 0;
        let fresh: Vec<Vec<i32>> = chunks
            .iter()
            .map(|&(cx, cz)| {
                let chunk = worker.generate_proto(ChunkPos::new(cx, cz)).unwrap();
                (0..256)
                    .map(|i| {
                        let (x, z) = (cx * 16 + i % 16, cz * 16 + i / 16);
                        let column = |y0: i32, y1: i32| {
                            (y0..y1)
                                .map(|y| format!("{y}:{}", chunk.block_name(x & 15, y, z & 15)))
                                .collect::<Vec<_>>()
                        };
                        let iceberg = (chunk.terrain.min_y()
                            ..chunk.terrain.min_y() + chunk.terrain.height())
                            .any(|y| ground[2..].contains(&chunk.block_name(x & 15, y, z & 15)));
                        let (floor, base) =
                            (top_where(&chunk, x & 15, z & 15, ground), height(&on, x, z));
                        if iceberg {
                            icebergs += 1;
                        } else {
                            assert_eq!(
                                floor + 1,
                                base,
                                "{x} {z}: {:?}",
                                column(floor.min(base) - 3, floor.max(base) + 3)
                            );
                        }
                        top_where(&chunk, x & 15, z & 15, open)
                    })
                    .collect()
            })
            .collect();
        eprintln!("COAST iceberg columns left out: {icebergs} of 1024");
        assert!(icebergs < 256, "{icebergs}");
        std::thread::scope(|scope| {
            for thread in 0..6 {
                let (overworld, fresh, top_where) = (&overworld, &fresh, &top_where);
                scope.spawn(move || {
                    let mut worker = overworld.worker();
                    let mut order: Vec<usize> = (0..chunks.len()).collect();
                    order.rotate_left(thread % chunks.len());
                    for i in order {
                        let (cx, cz) = chunks[i];
                        let chunk = worker.generate_proto(ChunkPos::new(cx, cz)).unwrap();
                        let tops: Vec<i32> = (0..256)
                            .map(|k| top_where(&chunk, k % 16, k / 16, open))
                            .collect();
                        assert_eq!(tops, fresh[i]);
                    }
                });
            }
        });
    }

    /// `glacial_landforms` on seed 777, across a snowy taiga valley at (2627, -2811): a line of 201 columns along z
    /// through it. The trough only lowers the ground, by tens of blocks, and lays a flat floor (columns within 2 blocks
    /// of the lowest) much wider than the river valley's; structures' base heights stay those of the filled chunks
    /// (frozen oceans' icebergs aside, see `coastal_landforms_cut_cliffs_and_lay_flats`), and chunks are the same on 6
    /// threads in rotated orders. The structures' water surface (`WORLD_SURFACE_WG`) is the chunks' too.
    #[test]
    #[ignore = "slow in debug: run with --profile fast --include-ignored"]
    fn glacial_landforms_cut_u_shaped_troughs() {
        let Some(data) = data() else { return };
        let make = |on| {
            terraforged_settings::set_for_thread(Some(Settings {
                glacial_landforms: on,
                ..terraforged_settings::FAITHFUL
            }));
            let pipeline = terraforged(&data, 777, false);
            terraforged_settings::set_for_thread(None);
            pipeline
        };
        let ((off, _), (on, overworld)) = (make(false), make(true));
        let height = |p: &TerraForgedPipeline, x, z| {
            p.generator
                .base_height(x, z, TerraHeightmap::OceanFloorWorldgen)
                .unwrap()
        };
        let (x, z0) = (2627, -2811);
        let line = |p: &TerraForgedPipeline| {
            (-100..=100)
                .map(|k| height(p, x, z0 + k))
                .collect::<Vec<_>>()
        };
        let (a, b) = (line(&off), line(&on));
        let lowered = a.iter().zip(&b).map(|(a, b)| a - b).max().unwrap();
        assert!(a.iter().zip(&b).all(|(a, b)| b <= a), "{a:?} {b:?}");
        // The floor: columns within 2 blocks of the line's lowest (a V has few, a U many).
        let floor = |h: &[i32]| {
            let low = *h.iter().min().unwrap();
            h.iter().filter(|&&y| y <= low + 2).count()
        };
        let (floor_off, floor_on) = (floor(&a), floor(&b));
        eprintln!("GLACIER lowered {lowered}, floor off {floor_off} on {floor_on}");
        eprintln!("GLACIER off {a:?}");
        eprintln!("GLACIER on {b:?}");
        assert!(lowered >= 30, "{lowered}");
        assert!(
            floor_on >= 30 && floor_on >= 2 * floor_off,
            "{floor_off} {floor_on}"
        );
        // The chunks: their ground is the base height structures read, and threads don't change them.
        let top_where = |chunk: &ProtoChunk, x: i32, z: i32, skip: &[&str]| {
            (chunk.terrain.min_y()..chunk.terrain.min_y() + chunk.terrain.height())
                .rev()
                .find(|&y| !skip.contains(&chunk.block_name(x, y, z)))
                .unwrap()
        };
        let (open, ground) = (
            ["minecraft:air", "minecraft:water"].as_slice(),
            [
                "minecraft:air",
                "minecraft:water",
                "minecraft:packed_ice",
                "minecraft:snow_block",
            ]
            .as_slice(),
        );
        let mut worker = overworld.worker();
        let (cx, cz) = (x >> 4, z0 >> 4);
        let chunks = [(cx, cz), (cx, cz - 1), (cx + 1, cz), (cx, cz + 2)];
        let mut icebergs = 0;
        let fresh: Vec<Vec<i32>> = chunks
            .iter()
            .map(|&(cx, cz)| {
                let chunk = worker.generate_proto(ChunkPos::new(cx, cz)).unwrap();
                (0..256)
                    .map(|i| {
                        let (x, z) = (cx * 16 + i % 16, cz * 16 + i / 16);
                        let iceberg = (chunk.terrain.min_y()
                            ..chunk.terrain.min_y() + chunk.terrain.height())
                            .any(|y| ground[2..].contains(&chunk.block_name(x & 15, y, z & 15)));
                        if iceberg {
                            icebergs += 1;
                        } else {
                            let floor = top_where(&chunk, x & 15, z & 15, ground);
                            assert_eq!(floor + 1, height(&on, x, z), "{x} {z}");
                            // The water level too: the trough lowers it in the chunks and for structures alike.
                            let surface = top_where(&chunk, x & 15, z & 15, &["minecraft:air"]);
                            let structures = on
                                .generator
                                .base_height(x, z, TerraHeightmap::WorldSurfaceWorldgen)
                                .unwrap();
                            assert_eq!(surface + 1, structures, "{x} {z}");
                        }
                        top_where(&chunk, x & 15, z & 15, open)
                    })
                    .collect()
            })
            .collect();
        assert!(icebergs < 256, "{icebergs}");
        std::thread::scope(|scope| {
            for thread in 0..6 {
                let (overworld, fresh, top_where) = (&overworld, &fresh, &top_where);
                scope.spawn(move || {
                    let mut worker = overworld.worker();
                    let mut order: Vec<usize> = (0..chunks.len()).collect();
                    order.rotate_left(thread % chunks.len());
                    for i in order {
                        let (cx, cz) = chunks[i];
                        let chunk = worker.generate_proto(ChunkPos::new(cx, cz)).unwrap();
                        let tops: Vec<i32> = (0..256)
                            .map(|k| top_where(&chunk, k % 16, k / 16, open))
                            .collect();
                        assert_eq!(tops, fresh[i]);
                    }
                });
            }
        });
    }

    /// What a decorated chunk holds on top: per column, its top block (first not air from the level's top down), the
    /// y of that block, and whether a log stands anywhere in the column.
    fn tops(chunk: &ProtoChunk) -> Vec<(String, i32, bool)> {
        let (min_y, height) = (chunk.terrain.min_y(), chunk.terrain.height());
        (0..256)
            .map(|i| {
                let (x, z) = (i & 15, i >> 4);
                let y = (min_y..min_y + height)
                    .rev()
                    .find(|&y| chunk.block_name(x, y, z) != "minecraft:air")
                    .unwrap_or(min_y);
                let log =
                    (min_y..min_y + height).any(|y| chunk.block_name(x, y, z).ends_with("_log"));
                (chunk.block_name(x, y, z).to_owned(), y, log)
            })
            .collect()
    }

    /// `altitude_climate` on seed 777 at height scale 1.85 (the tall overworld), on its highest warm land (biomes of
    /// vanilla temperature 0.7 or more, which vanilla's own height-adjusted temperature doesn't snow on; the highest
    /// is sparse jungle at y 443): the summits are snowed over, except a savanna's (2.0, too warm even there), while
    /// the land keeps its heights and biomes; with `realistic_vegetation` as well they are as snowy. On wooded slopes
    /// at y 195 to 225 (forests and plains, through their treeline) the trees thin out. The chunks are spread out:
    /// four points of a 32-block grid at least 600 blocks apart for each.
    #[test]
    fn altitude_climate_snows_on_summits_and_thins_their_trees() {
        let mut ov = crate::worldgen::overrides::WorldgenOverrides::default();
        ov.set(
            crate::worldgen::overrides::DIMENSION_TYPES,
            "minecraft:overworld",
            super::super::terraforged_world_preset::overworld_type_json().to_owned(),
        );
        let Some(data) = data_with(&ov) else { return };
        let build = |climate, vegetation| {
            terraforged_settings::set_for_thread(Some(Settings {
                height_scale: 1.85,
                altitude_climate: climate,
                realistic_vegetation: vegetation,
                ..terraforged_settings::FAITHFUL
            }));
            let (pipeline, overworld) = terraforged(&data, 777, false);
            terraforged_settings::set_for_thread(None);
            (pipeline, overworld)
        };
        let (off_pipeline, off) = build(false, false);
        // Heights and biomes every 32 blocks over 6,400 x 6,400 blocks, in warm biomes (vanilla temperature 0.7 or
        // more: forests, plains, jungles...), highest first.
        let sampler = off_pipeline.surface_sampler();
        let mut grid: Vec<(i32, i32, i32, f32)> = (-100..100)
            .flat_map(|i| (-100..100).map(move |j| (i * 32, j * 32)))
            .filter_map(|(x, z)| {
                let biome = off_pipeline.biome_at(&sampler, x, z)?;
                let temperature = data.temperatures[biome.0 as usize].base_temperature;
                (temperature >= 0.7).then(|| {
                    let height = off_pipeline.generator.far_heights(x, z).unwrap().0;
                    (height, x, z, temperature)
                })
            })
            .collect();
        grid.sort_unstable_by(|a, b| b.0.cmp(&a.0).then((a.1, a.2).cmp(&(b.1, b.2))));
        // Four points of the grid that `keep` keeps, highest first, at least 600 blocks apart.
        let spread = |keep: &dyn Fn(i32, f32) -> bool| {
            let mut points: Vec<(i32, i32, i32)> = Vec::new();
            for &(height, x, z, temperature) in &grid {
                if points.len() < 4
                    && keep(height, temperature)
                    && points
                        .iter()
                        .all(|s| (s.1 - x).abs().max((s.2 - z).abs()) >= 600)
                {
                    points.push((height, x, z));
                }
            }
            points
        };
        let summits = spread(&|_, _| true);
        // Wooded slopes under the snow line, through plains' and forests' treeline (y 190 to 220 at strength 1).
        let slopes =
            spread(&|height, temperature| (195..=225).contains(&height) && temperature <= 0.8);
        let (on, both) = (build(true, false).1, build(true, true).1);
        let registry = data.registry();
        let snowy = |chunk: &ProtoChunk| {
            (0..256)
                .filter(|i| {
                    let (x, z) = (i & 15, i >> 4);
                    let y = chunk.terrain.min_y() + chunk.terrain.height() - 1;
                    let y = (chunk.terrain.min_y()..=y)
                        .rev()
                        .find(|&y| chunk.block_name(x, y, z) == "minecraft:snow")
                        .unwrap_or(y);
                    registry
                        .state(chunk.block(x, y - 1, z))
                        .and_then(|s| s.properties.get("snowy"))
                        .is_some_and(|v| v == "true")
                })
                .count()
        };
        // [off, on, on with realistic_vegetation]: snow tops, logs, snowy blocks under the snow, over every summit.
        let mut totals = [(0, 0, 0); 3];
        // [off, on, on with realistic_vegetation]: log columns on the slopes.
        let mut slope_logs = [0; 3];
        for (slope, &(height, x, z)) in summits
            .iter()
            .map(|s| (false, s))
            .chain(slopes.iter().map(|s| (true, s)))
        {
            let center = ChunkPos::new(x >> 4, z >> 4);
            let mut decorated = Vec::new();
            for (k, overworld) in [&off, &on, &both].into_iter().enumerate() {
                let mut worker = overworld.worker();
                let mut region = region_around(&mut worker, center);
                worker.decorate(&mut region).unwrap();
                let top = tops(&region[4]);
                if slope {
                    slope_logs[k] += top.iter().filter(|t| t.2).count();
                    decorated.push((region, top));
                    continue;
                }
                totals[k].0 += top.iter().filter(|t| t.0 == "minecraft:snow").count();
                totals[k].1 += top.iter().filter(|t| t.2).count();
                totals[k].2 += snowy(&region[4]);
                decorated.push((region, top));
            }
            let biome = decorated[0].0[4].biome_at(8, height, 8).to_owned();
            eprintln!(
                "{} {x} {z} y {height} {biome}: snow tops {:?}, log columns {:?}",
                if slope { "slope" } else { "summit" },
                decorated
                    .iter()
                    .map(|d| d.1.iter().filter(|t| t.0 == "minecraft:snow").count())
                    .collect::<Vec<_>>(),
                decorated
                    .iter()
                    .map(|d| d.1.iter().filter(|t| t.2).count())
                    .collect::<Vec<_>>(),
            );
            // The ground and the biomes are the same: only the decoration's top differs.
            let (a, b) = (&decorated[0].0[4], &decorated[1].0[4]);
            for (lx, lz) in (0..16).flat_map(|x| (0..16).map(move |z| (x, z))) {
                for y in (height - 40..height + 10).step_by(4) {
                    assert_eq!(a.biome_at(lx, y, lz), b.biome_at(lx, y, lz));
                }
            }
            assert_eq!(
                (0..256)
                    .map(|i| a
                        .terrain
                        .heightmap(HeightmapType::OceanFloorWg)
                        .unwrap()
                        .first_available(i & 15, i >> 4)
                        .unwrap())
                    .collect::<Vec<_>>(),
                (0..256)
                    .map(|i| b
                        .terrain
                        .heightmap(HeightmapType::OceanFloorWg)
                        .unwrap()
                        .first_available(i & 15, i >> 4)
                        .unwrap())
                    .collect::<Vec<_>>(),
            );
        }
        eprintln!(
            "summits {summits:?}; [off, on, on + vegetation] (snow tops, log columns, snowy under snow): {totals:?}"
        );
        assert!(summits.len() == 4 && summits[0].0 > 280, "{summits:?}");
        assert!(totals[1].0 > totals[0].0 + 4 * 64, "{totals:?}");
        assert!(totals[2].0 > totals[0].0 + 4 * 64, "{totals:?}");
        assert!(totals[1].1 * 2 <= totals[0].1, "{totals:?}");
        assert!(totals[2].1 * 2 <= totals[0].1, "{totals:?}");
        assert!(totals[1].2 > totals[0].2, "{totals:?}");
        eprintln!("slopes {slopes:?}; log columns [off, on, on + vegetation]: {slope_logs:?}");
        assert!(
            slopes.len() == 4 && slope_logs[0] > 20,
            "{slopes:?} {slope_logs:?}"
        );
        assert!(slope_logs[1] * 2 <= slope_logs[0], "{slope_logs:?}");
        assert!(slope_logs[2] * 2 <= slope_logs[0], "{slope_logs:?}");
    }
}
