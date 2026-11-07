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
    terraforged_generator_facade::{TerraForgedGenerator, TerraHeightmap, register_live},
    terraforged_generator_preset::{build as build_noise, terrain_levels},
    terraforged_live_biomes::LiveBiomeBindings,
    terraforged_settings::{self, Settings},
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
use horizon_core::position::{BlockPos, ChunkPos};
use horizon_core::random::XoroshiroRandom;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

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
    let pipeline = TerraForgedPipeline::new(&data, seed, &stem, with_structures)?;
    register_live(seed, dimension, &pipeline.generator);
    hold(&pipeline);
    Ok(OverworldGenerator {
        pipeline: Arc::new(pipeline),
        noise_clamped: None,
    })
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
    /// `Generator.CODEC` over `stem` (its `levels`, then `GeneratorPreset.build`) with the level seed captured as
    /// `createState` does, the registry biome bindings and the surface passes' block bindings. The build range is
    /// the stem's dimension type's; TerraForged's levels are not mapped onto it: the fill clips them to it.
    fn new(
        data: &Arc<GeneratorData>,
        seed: i64,
        stem: &Value,
        with_structures: bool,
    ) -> Result<Self, GeneratorError> {
        let settings = terraforged_settings::current();
        let levels = terrain_levels(stem["generator"].get("levels")).map_err(resource)?;
        let generator = Arc::new(build_noise(levels).map_err(resource)?);
        generator.capture_seed(seed);
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
            features: TerraForgedFeatures::new(data)?,
            structures,
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
    /// lossy cell caches with the chunks, as `/locate biome` does. Its top eight blocks are
    /// painted by the vanilla surface rule as `buildSurface` paints a chunk: TerraForged's generation context (sea
    /// level 62), its preliminary height at the column, and the column's TerraForged biome (the one the chunk stores
    /// at the surface block's zoomed quart). Like vanilla's far sampling it skips the surface rule's slope reads and
    /// column extensions, and `Surface.apply`, the carvers and the decoration. A failed height or biome sample is an
    /// `Err` (the chunk would fail to generate there too).
    fn far_columns(
        &self,
        worker: &mut WorkerState,
        columns: &[(i32, i32)],
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
        for &(x, z) in columns {
            let (height, surface) = self
                .generator
                .far_heights(x, z)
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
        let (sample, column) = self.far_columns(worker, &[(x, z)]).ok()?.pop()?;
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
        // `approximate` (Reduced far detail) has nothing to trade here: each column is one height sample and one
        // biome sample already. Columns in the order vanilla's `lod_grid` returns them (x first, then z).
        let columns: Vec<(i32, i32)> = (0..side * side)
            .map(|index| {
                (
                    x.wrapping_add((index % side) as i32 * width),
                    z.wrapping_add((index / side) as i32 * width),
                )
            })
            .collect();
        self.far_columns(worker, &columns)
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

    /// A chunk's terrain data (heights, rivers, gradients, base heights) is the same whichever thread computes it and
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
}
