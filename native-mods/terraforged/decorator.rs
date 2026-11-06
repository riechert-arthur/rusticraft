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

// TerraForged 6dd607eb's `BiomeGenerator.decorate`
// (https://github.com/TerraForged/TerraForged/tree/6dd607ebfd41a3b7274e090b1f2cf565d53d1d44, MIT License, Copyright (c)
// 2021 TerraForged): `FeatureDecorator` and `VanillaDecorator` (the chunk is decorated with the one biome at its
// lowest corner, every feature seeded from a legacy random, vanilla features and structure pieces before and after
// the vegetal step), `NoiseCaveGenerator.decorate` and `NoiseCaveDecorator` (each noise cave's biomes' features), then
// `Surface.smoothWater` and `Surface.applyPost`. The vegetal step (`PositionSampler`) is the next layer. Spliced into
// the decoration module for its world, features and feature placement.

use super::super::biome_source::CheckerboardBiomeSource;
use super::*;
use crate::worldgen::generator::{
    terraforged_cave_carver::{CarverChunk, NoiseCaveGenerator},
    terraforged_generator_facade::TerraForgedGenerator,
    terraforged_surface_bindings::SurfaceBindings,
};
use horizon_core::random::LegacyRandom;

/// `GenerationStep.Decoration.LOCAL_MODIFICATIONS`: the first step `NoiseCaveDecorator` runs.
const LOCAL_MODIFICATIONS: usize = 2;
/// `VegetationFeatures.STAGE`: `VEGETAL_DECORATION`.
pub(in crate::worldgen) const VEGETAL: usize = 9;
/// `FeatureDecorator.MAX_DECORATION_STAGE`: `TOP_LAYER_MODIFICATION`.
const MAX_STAGE: usize = 10;
/// The key of the every-biome decoration in the generator data's decoration cache (whose other keys are biome
/// source JSON, which this is not).
const EVERY_BIOME: &str = "terraforged:every_biome";

type Random = WorldgenRandom<LegacyRandom>;

/// The features TerraForged decorates with, built once per world. Every registered biome's features are known
/// (`biome.getGenerationSettings()` and `BiomeFilter`'s `hasFeature`, not a biome source's feature sort): the noise
/// caves give quarts any registered biome, the nether's and the end's included (6dd607eb's UNDERGROUND filter is
/// commented out), and `NoiseCaveDecorator` places their features there.
pub(in crate::worldgen) struct TerraForgedFeatures {
    decoration: Arc<Decoration>,
    /// Each biome's `getGenerationSettings().features()`, step by step, by `BiomeId`.
    steps: Vec<Vec<Vec<FeatureIdentity>>>,
    /// How many structures each decoration step registers: `VanillaDecorator`'s feature-seed offset
    /// (`structures.size()`), the same whether or not the world places structures.
    structures_per_step: [usize; STEPS.len()],
}

impl TerraForgedFeatures {
    /// `None` when the generator data has no feature resources (as the vanilla decoration).
    pub(in crate::worldgen) fn new(
        data: &Arc<GeneratorData>,
    ) -> Result<Option<Self>, GeneratorError> {
        let Some(resources) = &data.feature_resources else {
            return Ok(None);
        };
        let names: Vec<String> = data
            .biome_names()
            .iter()
            .map(|name| name.to_string())
            .collect();
        // A checkerboard source only to name every biome: its features' order (the vanilla sort) is not used.
        let source = ResolvedBiomeSource::Checkerboard(
            CheckerboardBiomeSource::new(names, 0).map_err(|e| resource(format!("{e:?}")))?,
        );
        let Some(decoration) = data.decoration_for(EVERY_BIOME, &source)? else {
            return Ok(None);
        };
        // A step may name a feature tag, as `Decoration::new` reads it.
        let mut feature_tags = super::super::tags::TagDefinitions::default();
        for (name, json) in &resources.feature_tags {
            feature_tags.apply_json(name, "vanilla", json)?;
        }
        let tags = feature_tags
            .build(|name, _| resources.placed.contains_key(name).then(|| name.to_owned()))
            .map_err(|e| resource(format!("feature tags: {e:?}")))?
            .tags;
        let identity = |name: &str| -> Result<FeatureIdentity, GeneratorError> {
            let name = super::super::tags::identifier(name)?;
            decoration
                .names
                .iter()
                .position(|known| *known == name)
                .map(|index| FeatureIdentity(index as u32))
                .ok_or_else(|| resource(format!("unknown placed feature {name}")))
        };
        let mut steps = Vec::with_capacity(data.biome_names().len());
        for id in 0..data.biome_names().len() as u32 {
            let json: Value = serde_json::from_str(data.biome_json(BiomeId(id)))?;
            let mut biome = Vec::new();
            for step in json["features"].as_array().into_iter().flatten() {
                let entries = match step {
                    Value::String(tag) if tag.starts_with('#') => tags
                        .get(&super::super::tags::identifier(&tag[1..])?)
                        .cloned()
                        .ok_or_else(|| resource(format!("unknown feature tag {tag}")))?,
                    Value::String(name) => vec![name.clone()],
                    Value::Array(values) => values
                        .iter()
                        .map(|v| {
                            v.as_str()
                                .map(str::to_owned)
                                .ok_or_else(|| resource("inline placed feature in biome"))
                        })
                        .collect::<Result<_, _>>()?,
                    _ => return Err(resource("biome feature step")),
                };
                biome.push(
                    entries
                        .iter()
                        .map(|entry| identity(entry))
                        .collect::<Result<_, _>>()?,
                );
            }
            steps.push(biome);
        }
        // `buildStructureMap`: every registered structure under its step (a missing or unknown step reads as
        // `surface_structures`, as the structure registry decodes it).
        let mut structures_per_step = [0; STEPS.len()];
        if let Some(raw) = &data.structure_resources {
            for json in raw.structures.values() {
                let structure: Value = serde_json::from_str(json)?;
                let step = STEPS
                    .iter()
                    .position(|s| Some(*s) == structure["step"].as_str())
                    .unwrap_or(4);
                structures_per_step[step] += 1;
            }
        }
        Ok(Some(Self {
            decoration,
            steps,
            structures_per_step,
        }))
    }

    /// Whether every feature of `biome`'s lists is one `BiomeFilter` lets `biome` place.
    #[cfg(test)]
    pub(in crate::worldgen) fn knows_every_feature_of(&self, biome: BiomeId) -> bool {
        let steps = self.biome_steps(biome);
        let allowed = &self.decoration.biome_features[biome.0 as usize];
        steps.iter().any(|ids| !ids.is_empty())
            && steps.iter().flatten().all(|id| allowed.contains(id))
    }

    #[cfg(test)]
    pub(in crate::worldgen) fn structures_per_step(&self) -> &[usize] {
        &self.structures_per_step
    }

    /// Biome `biome`'s feature lists, step by step (`getGenerationSettings().features()`).
    fn biome_steps(&self, biome: BiomeId) -> &[Vec<FeatureIdentity>] {
        self.steps.get(biome.0 as usize).map_or(&[], Vec::as_slice)
    }
}

/// What TerraForged's decoration reads beside the vanilla decoration resources.
pub(in crate::worldgen) struct TerraForgedDecoration<'a> {
    pub features: &'a TerraForgedFeatures,
    pub generator: &'a TerraForgedGenerator,
    pub caves: &'a NoiseCaveGenerator<BiomeId>,
    pub surface: &'a SurfaceBindings,
    /// The air the noise caves carve, for a `getPostCarveChunk` replay.
    pub air: BlockStateId,
    /// `NoiseCaveGenerator.cache`: the carved chunks' `CarverChunk`s. The decorated chunk's is taken out once the
    /// region is checked; a chunk without one (carved by an earlier run) replays its caves without writing.
    pub carvers: &'a std::sync::Mutex<HashMap<ChunkPos, CarverChunk<BiomeId>>>,
}

/// `Generator.applyBiomeDecoration`'s `BiomeGenerator.decorate` for the centre of a 3x3 region: `FeatureDecorator`
/// (before and after the vegetal step), `NoiseCaveGenerator.decorate`, `Surface.smoothWater` and `Surface.applyPost`.
/// The caller drops the chunk's terrain-cache entry afterwards. `generation` is 26.3's
/// `WorldGenerationContext(generator, level)` over TerraForged's levels.
#[allow(clippy::too_many_arguments)]
pub(in crate::worldgen) fn decorate(
    data: &Arc<GeneratorData>,
    seed: i64,
    generation: WorldGenerationContext,
    worker: &mut WorkerState,
    region: &mut [ProtoChunk; 9],
    level_random: &mut dyn super::super::structure::legacy::PieceRandom,
    structures: Option<&super::super::generator_structures::StructureState>,
    tf: TerraForgedDecoration<'_>,
) -> Result<DecorateStats, GeneratorError> {
    let center = region[4].position();
    for (i, chunk) in region.iter().enumerate() {
        let expected = ChunkPos::new(
            center.x.wrapping_add(i as i32 % 3 - 1),
            center.z.wrapping_add(i as i32 / 3 - 1),
        );
        if chunk.position() != expected {
            return Err(resource("decoration region is not a 3×3 neighborhood"));
        }
        if (chunk.terrain.generation_stage() as u8) < PreLightChunkStage::Terrain as u8 {
            return Err(resource("decoration requires terrain-stage chunks"));
        }
    }
    if region[4].is_decorated() {
        return Ok(DecorateStats::default());
    }
    let features = tf.features;
    let decoration = &*features.decoration;
    // `Seeds.get(region.getSeed())`: the level seed as an int, for the terrain cache and the noise caves.
    let int_seed = seed as i32;
    let terrain_handle = tf.generator.chunk_data(int_seed, center.x, center.z);
    let terrain = terrain_handle.read();
    // getPostCarveChunk: `cache.remove(chunk.getPos())`.
    let cached = tf
        .carvers
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&center);
    let carver = match cached {
        Some(carver) => carver,
        // getPostCarveChunk: the caves again over the carved chunk, recording their biomes without writing.
        None => {
            let chunk = &mut region[4];
            let mut biomes = chunk.biomes().clone();
            tf.caves
                .carve(
                    int_seed,
                    &mut chunk.terrain,
                    &mut biomes,
                    &terrain,
                    tf.generator.min_y(),
                    tf.generator.sea_level(),
                    tf.air,
                    false,
                )
                .map_err(|e| GeneratorError::Stage("features", e.to_string()))?
        }
    };

    let mut chunks = Vec::with_capacity(9);
    let mut biomes = Vec::with_capacity(9);
    let (min_y, height) = (region[4].terrain.min_y(), region[4].terrain.height());
    for chunk in region.iter_mut() {
        // A one-section stand-in while the region holds the terrain, as the vanilla decoration does.
        let empty = super::super::terrain::TerrainChunk::new(
            chunk.position(),
            min_y,
            16,
            data.registry(),
            data.block_states().clone(),
        )
        .map_err(|e| resource(e.to_string()))?;
        chunks.push(std::mem::replace(&mut chunk.terrain, empty));
        biomes.push(chunk.biomes().clone());
    }
    let terrain_region = TerrainRegion::new(chunks, generation)
        .and_then(|region| region.with_block_bindings(&decoration.bindings))
        .map(|region| region.with_world_seed(seed))
        .map_err(|e| resource(e.to_string()))?;
    let references = region[4].structure_starts.clone();
    let mut block_entities = Vec::new();
    for chunk in region.iter_mut() {
        block_entities.append(&mut chunk.block_entities);
    }
    let mut world = World {
        block_entities,
        entities: Vec::new(),
        level_random,
        structure_mobs: worker.structure_mobs.clone(),
        starts: references.clone(),
        biome_names: data.biome_names(),
        blocks: data
            .feature_resources
            .as_ref()
            .and_then(|r| r.blocks.as_ref()),
        region: terrain_region,
        biomes: BiomeRegion::new(biomes, obfuscate_seed(seed))
            .map_err(|e| resource(e.to_string()))?,
        zone: GenerationWriteZone::new(center, 1),
        center,
        uniform_biomes: vec![UNKNOWN_BIOME; 9 * (height / 16).max(0) as usize],
        biome_quarts: region[4].biomes().quart_range(),
        decoration,
        temperatures: &mut worker.temperatures,
        behaviours: data.behaviours.as_deref(),
        // WorldGenRegion.isOutsideBuildHeight: the level's range, not the generator's.
        build: (min_y, min_y + height),
        shape_ticks: std::cell::RefCell::new(crate::level::Ticks::new(0)),
    };
    let mut decorator = Decorator {
        features,
        decoration,
        stats: DecorateStats::default(),
        placer: FeaturePlacer::new(),
        structures,
        structures_only: worker.structures_only,
        references,
        // VanillaDecorator.getWritableArea: the chunk's minimum build height + 1 to its maximum - 1.
        chunk_box: {
            let (x, z) = (center.x.wrapping_shl(4), center.z.wrapping_shl(4));
            BoundingBox::new(
                [x, min_y + 1, z],
                [x.wrapping_add(15), min_y + height - 1, z.wrapping_add(15)],
            )
        },
        structure_errors: 0,
    };

    let result = (|| -> Result<(), GeneratorError> {
        // FeatureDecorator.decorate: the biome at the chunk's lowest corner (`SectionPos.of(chunkPos,
        // level.getMinSection()).origin()`, through the biome zoom as `level.getBiome` reads it) and one legacy
        // random for every step.
        let origin = origin(center, min_y);
        let biome = world.biome_identity(origin).map_err(query_error)?;
        let mut random = Random::new(LegacyRandom::new(seed));
        let decoration_seed = random.set_decoration_seed(seed, origin[0], origin[2]);
        decorator.vanilla(
            &mut world,
            &mut random,
            decoration_seed,
            0,
            VEGETAL - 1,
            origin,
            biome,
        )?;
        // decorateVegetation (PositionSampler) is the next layer.
        decorator.vanilla(
            &mut world,
            &mut random,
            decoration_seed,
            VEGETAL + 1,
            MAX_STAGE,
            origin,
            biome,
        )?;
        // NoiseCaveGenerator.decorate: NoiseCaveDecorator for each cave, in array order.
        for (index, config) in tf.caves.caves().iter().enumerate() {
            let Some(cave_biomes) = carver.biomes(index) else {
                continue;
            };
            let start_y = config
                .height(int_seed, origin[0], origin[2])
                .map_err(|e| GeneratorError::Stage("features", e.to_owned()))?;
            let pos = [origin[0], start_y, origin[2]];
            let mut random = Random::new(LegacyRandom::new(seed));
            for &cave_biome in cave_biomes {
                let base = random.set_decoration_seed(seed, pos[0], pos[2]);
                for (stage, index, id) in cave_order(features.biome_steps(cave_biome)) {
                    random.set_feature_seed(base, index as i32, stage as i32);
                    decorator.place(&mut world, &mut random, id, pos);
                }
            }
        }
        Ok(())
    })();

    for (pos, nbt) in std::mem::take(&mut world.block_entities) {
        let index = ((pos[2] >> 4).wrapping_sub(center.z) + 1) * 3
            + (pos[0] >> 4).wrapping_sub(center.x)
            + 1;
        if let Some(chunk) = usize::try_from(index).ok().and_then(|i| region.get_mut(i)) {
            chunk.block_entities.push((pos, nbt));
        }
    }
    // WorldGenRegion.addFreshEntity: into the chunk holding the position.
    for nbt in std::mem::take(&mut world.entities) {
        let Some(at) = block_of(&nbt) else {
            continue;
        };
        let index =
            ((at[2] >> 4).wrapping_sub(center.z) + 1) * 3 + (at[0] >> 4).wrapping_sub(center.x) + 1;
        if let Some(chunk) = usize::try_from(index).ok().and_then(|i| region.get_mut(i)) {
            chunk.entities.push(nbt);
        }
    }
    for chunk in world.region.into_chunks() {
        let p = chunk.position();
        let index = (p.z.wrapping_sub(center.z) + 1) * 3 + p.x.wrapping_sub(center.x) + 1;
        region[index as usize].terrain = chunk;
    }
    result?;

    // Surface.smoothWater (over the region, after every feature) and Surface.applyPost on the centre chunk.
    let (before, rest) = region.split_at_mut(4);
    let (middle, after) = rest.split_at_mut(1);
    let neighbours: Vec<&ProtoChunk> = before.iter().chain(after.iter()).collect();
    let chunk = &mut middle[0];
    tf.surface
        .smooth_water(&terrain, &mut chunk.terrain, |x, y, z| {
            let at = ChunkPos::new(x >> 4, z >> 4);
            neighbours
                .iter()
                .find(|other| other.position() == at)
                .map(|other| other.terrain.block(x & 15, y, z & 15))
                .ok_or(super::super::terrain::TerrainError::InvalidState)
        })
        .map_err(|e| GeneratorError::Stage("features", e.to_string()))?;
    tf.surface
        .apply_post(&terrain, &mut chunk.terrain, tf.generator.gen_depth())
        .map_err(|e| GeneratorError::Stage("features", e.to_string()))?;

    let mut stats = std::mem::take(&mut decorator.stats);
    stats.failed += decorator.structure_errors;
    region[4]
        .terrain
        .set_generation_stage(PreLightChunkStage::Features);
    region[4].set_decorated();
    Ok(stats)
}

/// `FeatureDecorator.getOrigin`: `SectionPos.of(chunkPos, level.getMinSection()).origin()`.
fn origin(chunk: ChunkPos, min_y: i32) -> [i32; 3] {
    [
        chunk.x.wrapping_shl(4),
        (min_y >> 4).wrapping_shl(4),
        chunk.z.wrapping_shl(4),
    ]
}

fn query_error(e: PlacementQueryError) -> GeneratorError {
    GeneratorError::Stage("features", e.to_string())
}

struct Decorator<'a> {
    features: &'a TerraForgedFeatures,
    decoration: &'a Decoration,
    stats: DecorateStats,
    /// One placer for the whole chunk, as the vanilla decoration shares one.
    placer: FeaturePlacer,
    structures: Option<&'a super::super::generator_structures::StructureState>,
    structures_only: bool,
    references: Vec<Arc<super::super::structure::start::StructureStart>>,
    chunk_box: BoundingBox,
    structure_errors: usize,
}

impl Decorator<'_> {
    /// `PlacedFeature.placeWithBiomeCheck` at `origin`. A feature that cannot run is counted and skipped, as the
    /// vanilla decoration does.
    fn place(
        &mut self,
        world: &mut World<'_>,
        random: &mut Random,
        id: FeatureIdentity,
        origin: [i32; 3],
    ) -> bool {
        if self.structures_only {
            return false;
        }
        let decoration = self.decoration;
        let Ok(feature) = &decoration.features[id.0 as usize] else {
            self.stats.unsupported += 1;
            self.stats
                .skipped
                .insert(decoration.names[id.0 as usize].clone());
            return false;
        };
        self.stats.placed += 1;
        let blocks = decoration.blocks;
        let placed = self.placer.place_with_biome_check(
            &feature.placement,
            world,
            random,
            origin,
            id,
            |world, random, p| feature.feature.place(world, random, p, blocks),
        );
        if placed.is_err() {
            self.stats.failed += 1;
            self.stats
                .skipped
                .insert(decoration.names[id.0 as usize].clone());
        }
        placed.unwrap_or(false)
    }

    /// `VanillaDecorator.decorate` over steps `from..=to` ([`vanilla_order`]): each step's structure pieces
    /// (`placeStructures`, in registry order), then its features (`placeFeatures`).
    #[allow(clippy::too_many_arguments)]
    fn vanilla(
        &mut self,
        world: &mut World<'_>,
        random: &mut Random,
        seed: i64,
        from: usize,
        to: usize,
        origin: [i32; 3],
        biome: BiomeId,
    ) -> Result<(), GeneratorError> {
        let (features, decoration) = (self.features, self.decoration);
        let tree_blocks = decoration
            .bindings
            .tree_blocks()
            .map_err(|e| resource(e.to_string()))?;
        let blocks = decoration.blocks;
        let order = vanilla_order(
            features.biome_steps(biome),
            &features.structures_per_step,
            from,
            to,
        );
        for slot in order {
            match slot {
                Slot::Structure { stage, index } => {
                    random.set_feature_seed(seed, index as i32, stage as i32);
                    let Some(structures) = self.structures else {
                        continue;
                    };
                    let Some(&structure) = structures.by_step.get(stage).and_then(|s| s.get(index))
                    else {
                        continue;
                    };
                    // structureManager.startsForStructure(sectionPos, structure): the chunk's references to it.
                    for start in self
                        .references
                        .iter()
                        .filter(|start| start.structure == structure)
                    {
                        let mut place_feature =
                            |world: &mut World<'_>,
                             random: &mut Random,
                             name: &str,
                             pos: [i32; 3]| {
                                match decoration.named_feature(name) {
                                    Some(feature) => feature
                                        .place(world, random, pos, blocks)
                                        .map_err(|e| PlacementQueryError::Failed(e.to_string())),
                                    None => Ok(false),
                                }
                            };
                        if place_in_chunk(
                            start,
                            world,
                            &structures.resources,
                            &tree_blocks,
                            random,
                            self.chunk_box,
                            &mut place_feature,
                        )
                        .is_err()
                        {
                            self.structure_errors += 1;
                        }
                    }
                }
                Slot::Feature { stage, index, id } => {
                    random.set_feature_seed(seed, index as i32, stage as i32);
                    self.place(world, random, id, origin);
                }
            }
        }
        Ok(())
    }
}

/// One `setFeatureSeed(seed, index, stage)` of `VanillaDecorator.decorate`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot<T> {
    /// `placeStructures`: the step's structure `index` (registry order), whose starts in the chunk place next.
    Structure { stage: usize, index: usize },
    /// `placeFeatures`: feature `id`, seeded with `index` = the step's structure count + its place in the step.
    Feature { stage: usize, index: usize, id: T },
}

/// `VanillaDecorator.decorate`'s seeding order over steps `from..=to` of a biome's feature lists `steps`: a step
/// the biome does not have (`getStageFeatures` is null past its last) is skipped, structures and all; otherwise every
/// structure registered for it (`structures_per_step`), then its features after them.
fn vanilla_order<'a, T: Copy>(
    steps: &'a [Vec<T>],
    structures_per_step: &'a [usize],
    from: usize,
    to: usize,
) -> impl Iterator<Item = Slot<T>> + 'a {
    (from..=to)
        .filter_map(move |stage| steps.get(stage).map(|ids| (stage, ids)))
        .flat_map(move |(stage, ids)| {
            let structures = structures_per_step.get(stage).copied().unwrap_or(0);
            (0..structures)
                .map(move |index| Slot::Structure { stage, index })
                .chain(ids.iter().enumerate().map(move |(i, &id)| Slot::Feature {
                    stage,
                    index: structures + i,
                    id,
                }))
        })
}

/// `NoiseCaveDecorator.decorate(pos, ..)`'s seeding order over a cave biome's feature lists: every step from
/// `LOCAL_MODIFICATIONS` on, each feature seeded with its place in the step (no structure offset).
fn cave_order<T: Copy>(steps: &[Vec<T>]) -> impl Iterator<Item = (usize, usize, T)> + '_ {
    steps
        .iter()
        .enumerate()
        .skip(LOCAL_MODIFICATIONS)
        .flat_map(|(stage, ids)| ids.iter().enumerate().map(move |(i, &id)| (stage, i, id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_core::random::RandomSource;

    /// `DecoratorSeedOracle.java` (`.local/terraforged/oracles/decorator/`, `run.sh`): 6dd607eb's
    /// `FeatureDecorator`, `VanillaDecorator` and `NoiseCaveDecorator` seeding loops, verbatim, on 26.3's own
    /// `WorldgenRandom`, `LegacyRandomSource` and `SectionPos`. Per seed, chunk and minimum Y: the decoration seed
    /// and a fold of every seeding's kind, step, index and first three draws, over four biomes' step tables and three
    /// structure tables.
    const JAVA_SEEDS: &str = "\
0 0 0 -64 0 -6999181085366639327\n\
0 0 0 0 0 -6999181085366639327\n\
0 0 0 -2032 0 -6999181085366639327\n\
0 0 0 17 0 -6999181085366639327\n\
0 3 -2 -64 7181098201969140432 -5691397055950002498\n\
0 3 -2 0 7181098201969140432 -5691397055950002498\n\
0 3 -2 -2032 7181098201969140432 -5691397055950002498\n\
0 3 -2 17 7181098201969140432 -5691397055950002498\n\
0 -7 4 -64 -8744877247954377008 -6771854199184000047\n\
0 -7 4 0 -8744877247954377008 -6771854199184000047\n\
0 -7 4 -2032 -8744877247954377008 -6771854199184000047\n\
0 -7 4 17 -8744877247954377008 -6771854199184000047\n\
0 1875000 -1875000 -64 -2388966735442662656 3529128781972003180\n\
0 1875000 -1875000 0 -2388966735442662656 3529128781972003180\n\
0 1875000 -1875000 -2032 -2388966735442662656 3529128781972003180\n\
0 1875000 -1875000 17 -2388966735442662656 3529128781972003180\n\
0 134217727 -134217728 -64 7693461254008663152 -5823456815906483461\n\
0 134217727 -134217728 0 7693461254008663152 -5823456815906483461\n\
0 134217727 -134217728 -2032 7693461254008663152 -5823456815906483461\n\
0 134217727 -134217728 17 7693461254008663152 -5823456815906483461\n\
0 -1 -1 -64 8410474954089554048 -6338832587954304377\n\
0 -1 -1 0 8410474954089554048 -6338832587954304377\n\
0 -1 -1 -2032 8410474954089554048 -6338832587954304377\n\
0 -1 -1 17 8410474954089554048 -6338832587954304377\n\
7 0 0 -64 7 -4451521485483274286\n\
7 0 0 0 7 -4451521485483274286\n\
7 0 0 -2032 7 -4451521485483274286\n\
7 0 0 17 7 -4451521485483274286\n\
7 3 -2 -64 1847005117509295511 -3923858850416993055\n\
7 3 -2 0 1847005117509295511 -3923858850416993055\n\
7 3 -2 -2032 1847005117509295511 -3923858850416993055\n\
7 3 -2 17 1847005117509295511 -3923858850416993055\n\
7 -7 4 -64 2002628184085627735 4018292098347989636\n\
7 -7 4 0 2002628184085627735 4018292098347989636\n\
7 -7 4 -2032 2002628184085627735 4018292098347989636\n\
7 -7 4 17 2002628184085627735 4018292098347989636\n\
7 1875000 -1875000 -64 -2267635819427196921 2743304821172541\n\
7 1875000 -1875000 0 -2267635819427196921 2743304821172541\n\
7 1875000 -1875000 -2032 -2267635819427196921 2743304821172541\n\
7 1875000 -1875000 17 -2267635819427196921 2743304821172541\n\
7 134217727 -134217728 -64 -6949446863283241353 -641865201754251432\n\
7 134217727 -134217728 0 -6949446863283241353 -641865201754251432\n\
7 134217727 -134217728 -2032 -6949446863283241353 -641865201754251432\n\
7 134217727 -134217728 17 -6949446863283241353 -641865201754251432\n\
7 -1 -1 -64 5941726569660418791 -1587596695855380369\n\
7 -1 -1 0 5941726569660418791 -1587596695855380369\n\
7 -1 -1 -2032 5941726569660418791 -1587596695855380369\n\
7 -1 -1 17 5941726569660418791 -1587596695855380369\n\
-4172144997902289642 0 0 -64 -4172144997902289642 -995491752600925569\n\
-4172144997902289642 0 0 0 -4172144997902289642 -995491752600925569\n\
-4172144997902289642 0 0 -2032 -4172144997902289642 -995491752600925569\n\
-4172144997902289642 0 0 17 -4172144997902289642 -995491752600925569\n\
-4172144997902289642 3 -2 -64 2010949356533822150 -2635192447035919084\n\
-4172144997902289642 3 -2 0 2010949356533822150 -2635192447035919084\n\
-4172144997902289642 3 -2 -2032 2010949356533822150 -2635192447035919084\n\
-4172144997902289642 3 -2 17 2010949356533822150 -2635192447035919084\n\
-4172144997902289642 -7 4 -64 -1981642273168320122 935223274117126922\n\
-4172144997902289642 -7 4 0 -1981642273168320122 935223274117126922\n\
-4172144997902289642 -7 4 -2032 -1981642273168320122 935223274117126922\n\
-4172144997902289642 -7 4 17 -1981642273168320122 935223274117126922\n\
-4172144997902289642 1875000 -1875000 -64 2846806866529289494 3649674101976208642\n\
-4172144997902289642 1875000 -1875000 0 2846806866529289494 3649674101976208642\n\
-4172144997902289642 1875000 -1875000 -2032 2846806866529289494 3649674101976208642\n\
-4172144997902289642 1875000 -1875000 17 2846806866529289494 3649674101976208642\n\
-4172144997902289642 134217727 -134217728 -64 826392813538052390 3472305415552655263\n\
-4172144997902289642 134217727 -134217728 0 826392813538052390 3472305415552655263\n\
-4172144997902289642 134217727 -134217728 -2032 826392813538052390 3472305415552655263\n\
-4172144997902289642 134217727 -134217728 17 826392813538052390 3472305415552655263\n\
-4172144997902289642 -1 -1 -64 -2546775879774179466 191716485735449760\n\
-4172144997902289642 -1 -1 0 -2546775879774179466 191716485735449760\n\
-4172144997902289642 -1 -1 -2032 -2546775879774179466 191716485735449760\n\
-4172144997902289642 -1 -1 17 -2546775879774179466 191716485735449760\n\
-9223372036854775808 0 0 -64 -9223372036854775808 -6999181085366639327\n\
-9223372036854775808 0 0 0 -9223372036854775808 -6999181085366639327\n\
-9223372036854775808 0 0 -2032 -9223372036854775808 -6999181085366639327\n\
-9223372036854775808 0 0 17 -9223372036854775808 -6999181085366639327\n\
-9223372036854775808 3 -2 -64 -2042273834885635376 -5691397055950002498\n\
-9223372036854775808 3 -2 0 -2042273834885635376 -5691397055950002498\n\
-9223372036854775808 3 -2 -2032 -2042273834885635376 -5691397055950002498\n\
-9223372036854775808 3 -2 17 -2042273834885635376 -5691397055950002498\n\
-9223372036854775808 -7 4 -64 478494788900398800 -6771854199184000047\n\
-9223372036854775808 -7 4 0 478494788900398800 -6771854199184000047\n\
-9223372036854775808 -7 4 -2032 478494788900398800 -6771854199184000047\n\
-9223372036854775808 -7 4 17 478494788900398800 -6771854199184000047\n\
-9223372036854775808 1875000 -1875000 -64 6834405301412113152 3529128781972003180\n\
-9223372036854775808 1875000 -1875000 0 6834405301412113152 3529128781972003180\n\
-9223372036854775808 1875000 -1875000 -2032 6834405301412113152 3529128781972003180\n\
-9223372036854775808 1875000 -1875000 17 6834405301412113152 3529128781972003180\n\
-9223372036854775808 134217727 -134217728 -64 -1529910782846112656 -5823456815906483461\n\
-9223372036854775808 134217727 -134217728 0 -1529910782846112656 -5823456815906483461\n\
-9223372036854775808 134217727 -134217728 -2032 -1529910782846112656 -5823456815906483461\n\
-9223372036854775808 134217727 -134217728 17 -1529910782846112656 -5823456815906483461\n\
-9223372036854775808 -1 -1 -64 -812897082765221760 -6338832587954304377\n\
-9223372036854775808 -1 -1 0 -812897082765221760 -6338832587954304377\n\
-9223372036854775808 -1 -1 -2032 -812897082765221760 -6338832587954304377\n\
-9223372036854775808 -1 -1 17 -812897082765221760 -6338832587954304377\n\
9223372036854775807 0 0 -64 9223372036854775807 363299305401191065\n\
9223372036854775807 0 0 0 9223372036854775807 363299305401191065\n\
9223372036854775807 0 0 -2032 9223372036854775807 363299305401191065\n\
9223372036854775807 0 0 17 9223372036854775807 363299305401191065\n\
9223372036854775807 3 -2 -64 -306349037405507761 7043810572986831187\n\
9223372036854775807 3 -2 0 -306349037405507761 7043810572986831187\n\
9223372036854775807 3 -2 -2032 -306349037405507761 7043810572986831187\n\
9223372036854775807 3 -2 17 -306349037405507761 7043810572986831187\n\
9223372036854775807 -7 4 -64 -3019794537391370865 -293089019000930368\n\
9223372036854775807 -7 4 0 -3019794537391370865 -293089019000930368\n\
9223372036854775807 -7 4 -2032 -3019794537391370865 -293089019000930368\n\
9223372036854775807 -7 4 17 -3019794537391370865 -293089019000930368\n\
9223372036854775807 1875000 -1875000 -64 -4041204259572284929 6167776352514523628\n\
9223372036854775807 1875000 -1875000 0 -4041204259572284929 6167776352514523628\n\
9223372036854775807 1875000 -1875000 -2032 -4041204259572284929 6167776352514523628\n\
9223372036854775807 1875000 -1875000 17 -4041204259572284929 6167776352514523628\n\
9223372036854775807 134217727 -134217728 -64 6591362910607518767 4094934350776054496\n\
9223372036854775807 134217727 -134217728 0 6591362910607518767 4094934350776054496\n\
9223372036854775807 134217727 -134217728 -2032 6591362910607518767 4094934350776054496\n\
9223372036854775807 134217727 -134217728 17 6591362910607518767 4094934350776054496\n\
9223372036854775807 -1 -1 -64 -11034012353944033 -8872291609949929790\n\
9223372036854775807 -1 -1 0 -11034012353944033 -8872291609949929790\n\
9223372036854775807 -1 -1 -2032 -11034012353944033 -8872291609949929790\n\
9223372036854775807 -1 -1 17 -11034012353944033 -8872291609949929790\n\
123456789012345 0 0 -64 123456789012345 -4913837437295472336\n\
123456789012345 0 0 0 123456789012345 -4913837437295472336\n\
123456789012345 0 0 -2032 123456789012345 -4913837437295472336\n\
123456789012345 0 0 17 123456789012345 -4913837437295472336\n\
123456789012345 3 -2 -64 8639132024461909481 2475195192679476726\n\
123456789012345 3 -2 0 8639132024461909481 2475195192679476726\n\
123456789012345 3 -2 -2032 8639132024461909481 2475195192679476726\n\
123456789012345 3 -2 17 8639132024461909481 2475195192679476726\n\
123456789012345 -7 4 -64 6585772716220803369 -2984604022899797839\n\
123456789012345 -7 4 0 6585772716220803369 -2984604022899797839\n\
123456789012345 -7 4 -2032 6585772716220803369 -2984604022899797839\n\
123456789012345 -7 4 17 6585772716220803369 -2984604022899797839\n\
123456789012345 1875000 -1875000 -64 -9170378878264862855 5563255641983586291\n\
123456789012345 1875000 -1875000 0 -9170378878264862855 5563255641983586291\n\
123456789012345 1875000 -1875000 -2032 -9170378878264862855 5563255641983586291\n\
123456789012345 1875000 -1875000 17 -9170378878264862855 5563255641983586291\n\
123456789012345 134217727 -134217728 -64 4574064472269186057 -6507918896678888492\n\
123456789012345 134217727 -134217728 0 4574064472269186057 -6507918896678888492\n\
123456789012345 134217727 -134217728 -2032 4574064472269186057 -6507918896678888492\n\
123456789012345 134217727 -134217728 17 4574064472269186057 -6507918896678888492\n\
123456789012345 -1 -1 -64 8638912910808533657 -4999559691898626556\n\
123456789012345 -1 -1 0 8638912910808533657 -4999559691898626556\n\
123456789012345 -1 -1 -2032 8638912910808533657 -4999559691898626556\n\
123456789012345 -1 -1 17 8638912910808533657 -4999559691898626556\n\
";

    const SEEDS: [i64; 6] = [
        0,
        7,
        -4_172_144_997_902_289_642,
        i64::MIN,
        i64::MAX,
        123_456_789_012_345,
    ];
    const CHUNKS: [[i32; 2]; 6] = [
        [0, 0],
        [3, -2],
        [-7, 4],
        [1_875_000, -1_875_000],
        [i32::MAX >> 4, i32::MIN >> 4],
        [-1, -1],
    ];
    const MIN_YS: [i32; 4] = [-64, 0, -2032, 17];
    const STRUCTURES: [[usize; STEPS.len()]; 3] = [
        [0, 0, 0, 3, 25, 1, 0, 0, 0, 0, 0],
        [0; STEPS.len()],
        [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
    ];

    /// A biome's step table with `counts[stage]` features per step (the ids are their places).
    fn steps(counts: &[usize]) -> Vec<Vec<usize>> {
        counts.iter().map(|&n| (0..n).collect()).collect()
    }

    fn fold(h: &mut i64, v: i64) {
        *h = h.wrapping_mul(31).wrapping_add(v);
    }

    fn draws(h: &mut i64, random: &mut Random) {
        fold(h, random.next_int_bound(16).unwrap() as i64);
        fold(h, random.next_long());
        fold(h, random.next_float().to_bits() as i32 as i64);
    }

    /// The decorator's own seeding order (`vanilla_order` before and after the vegetal step, then `cave_order` for
    /// two cave biomes) matches the original loops on 26.3's random, in Java's order.
    #[test]
    fn decoration_seeds_match_6dd607eb_on_26_3_random() {
        let biomes = [
            steps(&[2, 0, 1, 3, 1, 0, 4, 2, 1, 5, 1]),
            steps(&[1, 2]),
            steps(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 2]),
            steps(&[]),
        ];
        let mut lines = JAVA_SEEDS.lines();
        for seed in SEEDS {
            for [cx, cz] in CHUNKS {
                for min_y in MIN_YS {
                    let mut h = 0i64;
                    let mut decoration_seed = 0;
                    for b in 0..biomes.len() {
                        for structures in &STRUCTURES {
                            let origin = origin(ChunkPos::new(cx, cz), min_y);
                            let mut random = Random::new(LegacyRandom::new(seed));
                            decoration_seed =
                                random.set_decoration_seed(seed, origin[0], origin[2]);
                            let order =
                                vanilla_order(&biomes[b], structures, 0, VEGETAL - 1).chain(
                                    vanilla_order(&biomes[b], structures, VEGETAL + 1, MAX_STAGE),
                                );
                            for slot in order {
                                let (kind, stage, index) = match slot {
                                    Slot::Structure { stage, index } => (1, stage, index),
                                    Slot::Feature { stage, index, id } => {
                                        assert_eq!(index, structures[stage] + id);
                                        (2, stage, index)
                                    }
                                };
                                random.set_feature_seed(
                                    decoration_seed,
                                    index as i32,
                                    stage as i32,
                                );
                                for v in [kind, stage as i64, index as i64] {
                                    fold(&mut h, v);
                                }
                                draws(&mut h, &mut random);
                            }
                            let mut cave_random = Random::new(LegacyRandom::new(seed));
                            for cave in [&biomes[b], &biomes[(b + 1) % biomes.len()]] {
                                let base =
                                    cave_random.set_decoration_seed(seed, origin[0], origin[2]);
                                for (stage, index, _) in cave_order(cave) {
                                    cave_random.set_feature_seed(base, index as i32, stage as i32);
                                    for v in [3, stage as i64, index as i64] {
                                        fold(&mut h, v);
                                    }
                                    draws(&mut h, &mut cave_random);
                                }
                            }
                        }
                    }
                    let line = format!("{seed} {cx} {cz} {min_y} {decoration_seed} {h}");
                    assert_eq!(Some(line.as_str()), lines.next());
                }
            }
        }
        assert_eq!(lines.next(), None);
    }
}
