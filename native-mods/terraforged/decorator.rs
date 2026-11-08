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
// `Surface.smoothWater` and `Surface.applyPost`; the vegetal step is `PositionSampler` (each column's fitness under its
// biome's vegetation config, trees on a squashed, jittered grid where the fitness beats the cell's roll, grass passes
// and the origin biome's other vegetal features), over `BiomeVegetationManager`'s per-biome configs and
// `VegetationFeatures`. Spliced into the decoration module for its world, features and feature placement.

use super::super::biome_source::CheckerboardBiomeSource;
use super::super::placement::PlacementModifier;
use super::*;
use crate::worldgen::generator::{
    terraforged::{floor, hash, hash_2d},
    terraforged_biome_viability::TerrainViabilityContext,
    terraforged_cave_carver::{CarverChunk, NoiseCaveGenerator},
    terraforged_generator_facade::TerraForgedGenerator,
    terraforged_surface_bindings::SurfaceBindings,
    terraforged_terrain_blend_util::{X_PRIME, Y_PRIME, random as rand},
    terraforged_vegetation::{
        VegetationConfig, VegetationFeatures, bind_tags, config_for, current_vegetation,
    },
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
/// `PositionSampler.BORDER`: how far past the chunk the tree grid reaches.
const BORDER: f32 = 6.0;

/// The `deterministic` setting's feature-seed slots ([`slot_index`]): a placement's kind in bits 28..31, past every
/// running count the original reaches.
const CORNER: i32 = 1;
const SAMPLED: i32 = 2;
const GRASS: i32 = 3;
const OTHER: i32 = 4;

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
    /// The bundled vegetation configs, their tags bound to the registry (`TerraForged.VEGETATIONS.entries`).
    configs: Vec<VegetationConfig>,
    /// `BiomeVegetationManager.getVegetation`, by `BiomeId` (Java computes it lazily per biome; it never changes).
    vegetation: Vec<BiomeVegetation>,
    /// By block state: what decoration grows above the ground (trees, huge mushrooms, vines, bee nests), which the
    /// `deterministic` setting looks under for a column's ground ([`RegionPlanting`]'s `top`).
    canopy: Vec<bool>,
}

/// `BiomeVegetation`: a biome's vegetation config and its vegetal features split by `VegetationFeatures.create`.
struct BiomeVegetation {
    /// The config's index in `TerraForgedFeatures::configs`; `None` is Java's `VegetationConfig.NONE`.
    config: Option<usize>,
    /// `biome.is(BiomeTags.IS_RIVER)`.
    river: bool,
    trees: Vec<VegetationTree>,
    grass: Vec<FeatureIdentity>,
    other: Vec<FeatureIdentity>,
}

/// A tree feature, with `VegetationFeatures.unwrap`'s placement when the biome has a config.
struct VegetationTree {
    id: FeatureIdentity,
    /// The feature's placement without its biome, count and square modifiers (`TREE_EXCLUSIONS`); `None` keeps
    /// the feature's own (no config, or a feature that cannot run).
    placement: Option<Vec<PlacementModifier>>,
}

impl BiomeVegetation {
    /// `compute(biome)`: `getConfig` and `VegetationFeatures.create` over the biome's `VEGETAL_DECORATION` list.
    fn new(
        decoration: &Decoration,
        configs: &[VegetationConfig],
        name: &str,
        vegetal: &[FeatureIdentity],
        river: bool,
    ) -> Self {
        let config = config_for(configs, name)
            .map(|found| configs.iter().position(|c| std::ptr::eq(c, found)).unwrap());
        let ids: HashMap<&str, FeatureIdentity> = vegetal
            .iter()
            .map(|&id| (decoration.names[id.0 as usize].as_str(), id))
            .collect();
        let split = VegetationFeatures::create(
            vegetal
                .iter()
                .map(|id| decoration.names[id.0 as usize].as_str()),
            config.is_some(),
        );
        let trees = split
            .trees
            .iter()
            .map(|(name, unwrap)| {
                let id = ids[name.as_str()];
                let placement = match &decoration.features[id.0 as usize] {
                    Ok(feature) if *unwrap => Some(
                        feature
                            .placement
                            .iter()
                            .filter(|modifier| {
                                !matches!(
                                    modifier,
                                    PlacementModifier::Biome
                                        | PlacementModifier::Count(_)
                                        | PlacementModifier::CountOnEveryLayer(_)
                                        | PlacementModifier::NoiseBasedCount { .. }
                                        | PlacementModifier::NoiseThresholdCount { .. }
                                        | PlacementModifier::InSquare
                                )
                            })
                            .cloned()
                            .collect(),
                    ),
                    _ => None,
                };
                VegetationTree { id, placement }
            })
            .collect();
        let resolve = |names: &[String]| names.iter().map(|name| ids[name.as_str()]).collect();
        Self {
            config,
            river,
            trees,
            grass: resolve(&split.grass),
            other: resolve(&split.other),
        }
    }
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
        // BiomeVegetationManager: the configs in registry order, each tag bound as 26.3's TagLoader loads it.
        let mut configs = current_vegetation().map_err(resource)?;
        bind_tags(&mut configs, |biome| data.biome_id(biome).is_some());
        let rivers = data.biome_tags.get("minecraft:is_river");
        let vegetation = (0..data.biome_names().len())
            .map(|id| {
                let name = data.biome_name(BiomeId(id as u32));
                let vegetal = steps[id].get(VEGETAL).map_or(&[][..], Vec::as_slice);
                let river = rivers.is_some_and(|tag| tag.iter().any(|member| member == name));
                BiomeVegetation::new(&decoration, &configs, name, vegetal, river)
            })
            .collect();
        let registry = data.registry();
        let canopy = (0..registry.len() as u32)
            .map(|id| {
                registry
                    .state(BlockStateId(id))
                    .is_some_and(|state| is_canopy(&state.name))
            })
            .collect();
        Ok(Some(Self {
            decoration,
            steps,
            structures_per_step,
            configs,
            vegetation,
            canopy,
        }))
    }

    /// The same features with Horizon's `realistic_vegetation` (`terraforged_realistic_vegetation::overlay`) over
    /// every vegetation config's fitness when `on`.
    pub(in crate::worldgen) fn with_realistic_vegetation(mut self, on: bool) -> Self {
        if on {
            for config in &mut self.configs {
                let viability = std::mem::replace(
                    &mut config.viability,
                    crate::worldgen::generator::terraforged_biome_viability::Viability::None,
                );
                config.viability =
                    crate::worldgen::generator::terraforged_realistic_vegetation::overlay(
                        viability,
                    );
            }
        }
        self
    }

    /// The bound vegetation configs, in registry order.
    #[cfg(test)]
    pub(in crate::worldgen) fn configs(&self) -> &[VegetationConfig] {
        &self.configs
    }

    /// Whether every feature of `biome`'s lists is one `BiomeFilter` lets `biome` place.
    #[cfg(test)]
    pub(in crate::worldgen) fn knows_every_feature_of(&self, biome: BiomeId) -> bool {
        let steps = self.biome_steps(biome);
        let allowed = &self.decoration.biome_features[biome.0 as usize];
        steps.iter().any(|ids| !ids.is_empty())
            && steps.iter().flatten().all(|id| allowed.contains(id))
    }

    /// Biome `biome`'s vegetation: its config's tag, whether it is a river, its tree, grass and other feature names,
    /// and whether every tree's placement dropped the biome, count and square modifiers (or kept its own).
    #[cfg(test)]
    pub(in crate::worldgen) fn vegetation_of(
        &self,
        biome: BiomeId,
    ) -> (Option<&str>, bool, [Vec<&str>; 3], bool) {
        let vegetation = &self.vegetation[biome.0 as usize];
        let name = |id: &FeatureIdentity| self.decoration.names[id.0 as usize].as_str();
        let unwrapped = vegetation.trees.iter().all(|tree| {
            let own = self.decoration.features[tree.id.0 as usize].as_ref().ok();
            match (&tree.placement, own) {
                (Some(placement), Some(own)) => {
                    placement.len() < own.placement.len()
                        && placement.iter().all(|modifier| {
                            !matches!(
                                modifier,
                                PlacementModifier::Biome
                                    | PlacementModifier::Count(_)
                                    | PlacementModifier::InSquare
                            )
                        })
                }
                (None, own) => vegetation.config.is_none() || own.is_none(),
                (Some(_), None) => false,
            }
        });
        (
            vegetation
                .config
                .map(|config| self.configs[config].tag.as_str()),
            vegetation.river,
            [
                vegetation.trees.iter().map(|tree| name(&tree.id)).collect(),
                vegetation.grass.iter().map(name).collect(),
                vegetation.other.iter().map(name).collect(),
            ],
            unwrapped,
        )
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
    /// The world's `deterministic` setting (`terraforged_settings`): the vegetation's fixed feature-seed slots and
    /// ground under the trees.
    pub deterministic: bool,
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
        // decorateVegetation: PositionSampler.placeVegetation.
        PositionSampler {
            planting: RegionPlanting {
                decorator: &mut decorator,
                world: &mut world,
                context: TerrainViabilityContext(&terrain),
                deterministic: tf.deterministic,
            },
            random: &mut random,
            seed: decoration_seed,
            chunk: center,
            vegetation: &features.vegetation,
            configs: &features.configs,
            sea_level: tf.generator.sea_level(),
            centre: (terrain.river().get(8, 8), terrain.height_at(8, 8)),
            deterministic: tf.deterministic,
        }
        .place_vegetation(origin, biome)?;
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
        self.place_with(world, random, id, None, origin)
    }

    /// [`Self::place`] with `placement` in place of the feature's own modifiers (`VegetationFeatures.unwrap`'s new
    /// `PlacedFeature`) when it is `Some`.
    fn place_with(
        &mut self,
        world: &mut World<'_>,
        random: &mut Random,
        id: FeatureIdentity,
        placement: Option<&[PlacementModifier]>,
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
            placement.unwrap_or(&feature.placement),
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

/// What `PositionSampler` reads from and does to the region, so tests can drive the sampler over stubs.
trait Planting {
    /// `chunk.getHeight(OCEAN_FLOOR_WG, x, z)`.
    fn top(&mut self, x: i32, z: i32) -> Result<i32, GeneratorError>;
    /// `region.getBiome(pos)`.
    fn biome(&mut self, pos: [i32; 3]) -> Result<BiomeId, GeneratorError>;
    /// `configs[config].viability().getFitness(x, z, viabilityContext)`.
    fn fitness(&mut self, config: usize, x: i32, z: i32) -> Result<f32, GeneratorError>;
    /// `feature.placeWithBiomeCheck(region, generator, random, pos)`, with `placement` in place of the feature's own
    /// when it is `Some` (an unwrapped tree).
    fn place(
        &mut self,
        random: &mut Random,
        feature: FeatureIdentity,
        placement: Option<&[PlacementModifier]>,
        pos: [i32; 3],
    ) -> bool;
}

/// The decoration's region as [`Planting`]: the centre chunk's heights and the region's biomes through the
/// [`World`], fitness over the chunk's terrain data, placement through the [`Decorator`].
struct RegionPlanting<'d, 'w, 'a> {
    decorator: &'d mut Decorator<'a>,
    world: &'d mut World<'w>,
    context: TerrainViabilityContext<'d>,
    /// The `deterministic` setting: a column's top is the ground under any trees.
    deterministic: bool,
}

impl Planting for RegionPlanting<'_, '_, '_> {
    /// `ChunkAccess.getHeight`: one under the region's first free block (`WorldGenRegion.getHeight` adds 1). With
    /// `deterministic`, the first block under that which no tree, huge mushroom, vine or bee nest placed (at most 64
    /// down), so a neighbour's leaves over the column, there or not depending on which chunk decorated first, don't
    /// move the ground.
    fn top(&mut self, x: i32, z: i32) -> Result<i32, GeneratorError> {
        let mut y = self
            .world
            .height(HeightmapType::OceanFloorWg, x, z)
            .map(|first_free| first_free.wrapping_sub(1))
            .map_err(query_error)?;
        if self.deterministic {
            let canopy = &self.decorator.features.canopy;
            for _ in 0..64 {
                let state = super::super::placement::PlacementContext::block_state(
                    &mut *self.world,
                    [x, y, z],
                )
                .map_err(query_error)?;
                if !canopy.get(state.0 as usize).copied().unwrap_or(false) {
                    break;
                }
                y = y.wrapping_sub(1);
            }
        }
        Ok(y)
    }

    fn biome(&mut self, pos: [i32; 3]) -> Result<BiomeId, GeneratorError> {
        self.world.biome_identity(pos).map_err(query_error)
    }

    fn fitness(&mut self, config: usize, x: i32, z: i32) -> Result<f32, GeneratorError> {
        self.decorator.features.configs[config]
            .viability
            .try_fitness(x, z, &self.context)
            .map_err(|e| GeneratorError::Stage("features", e.to_owned()))
    }

    fn place(
        &mut self,
        random: &mut Random,
        feature: FeatureIdentity,
        placement: Option<&[PlacementModifier]>,
        pos: [i32; 3],
    ) -> bool {
        self.decorator
            .place_with(self.world, random, feature, placement, pos)
    }
}

/// `PositionSampler.placeVegetation` for one chunk: its `SamplerContext`, less the per-biome fields (passed along).
struct PositionSampler<'v, P> {
    planting: P,
    random: &'v mut Random,
    /// The decoration seed (`setDecorationSeed`'s).
    seed: i64,
    chunk: ChunkPos,
    /// `getVegetation`, by `BiomeId`.
    vegetation: &'v [BiomeVegetation],
    configs: &'v [VegetationConfig],
    sea_level: i32,
    /// The terrain data's `getRiver().get(8, 8)` and `getHeight(8, 8)`, for the grass passes.
    centre: (f32, i32),
    /// The `deterministic` setting: each placement is seeded from a fixed slot ([`slot_index`]) in place of the
    /// original's running count of placed features, so a tree a neighbour blocks no longer reseeds the rest of the
    /// chunk.
    deterministic: bool,
}

impl<P: Planting> PositionSampler<'_, P> {
    /// `setFeatureSeed(seed, offset, VEGETAL)`, or with `deterministic` the slot `slot()`.
    fn seed_feature(&mut self, offset: i32, slot: impl FnOnce() -> i32) {
        let index = if self.deterministic { slot() } else { offset };
        self.random
            .set_feature_seed(self.seed, index, VEGETAL as i32);
    }

    /// `placeVegetation`: `placeTreesAndGrass`, then `placeOther` from the offset it reached.
    fn place_vegetation(&mut self, origin: [i32; 3], biome: BiomeId) -> Result<(), GeneratorError> {
        let offset = self.trees_and_grass()?;
        self.other(offset, origin, biome);
        Ok(())
    }

    /// `placeTreesAndGrass`: `populate` (every column's fitness under its biome's config, `Viability.NONE`'s 1
    /// without one, and the biomes met, in first-met order), then per biome but rivers a config-less biome's trees
    /// and grass at the chunk's corner, or the jittered tree grid and the grass passes. Returns the feature-seed
    /// offset reached, Java's `int` (it wraps).
    fn trees_and_grass(&mut self) -> Result<i32, GeneratorError> {
        let (start_x, start_z) = (self.chunk.x.wrapping_shl(4), self.chunk.z.wrapping_shl(4));
        let mut viability = [0.0_f32; 256];
        let mut biomes: Vec<BiomeId> = Vec::new();
        for dz in 0..16 {
            for dx in 0..16 {
                let (x, z) = (start_x.wrapping_add(dx), start_z.wrapping_add(dz));
                let y = self.planting.top(x, z)?;
                let biome = self.planting.biome([x, y, z])?;
                viability[(dz * 16 + dx) as usize] = match self.vegetation[biome.0 as usize].config
                {
                    Some(config) => self.planting.fitness(config, x, z)?,
                    None => 1.0,
                };
                if !biomes.contains(&biome) {
                    biomes.push(biome);
                }
            }
        }
        let mut offset = 0_i32;
        for (i, &biome) in biomes.iter().enumerate() {
            let i = i as i32;
            let vegetation = &self.vegetation[biome.0 as usize];
            // "Remove annoying river trees!"
            if vegetation.river {
                continue;
            }
            offset = match vegetation.config {
                None => {
                    self.place_at_corner(offset.wrapping_add(i), i, [start_x, start_z], vegetation)?
                }
                Some(config) => {
                    let offset = self.sample(
                        offset.wrapping_add(i),
                        i,
                        config,
                        biome,
                        vegetation,
                        &viability,
                    )?;
                    self.grass(offset.wrapping_add(i), i, [start_x, start_z], vegetation)
                }
            };
        }
        Ok(offset)
    }

    /// `placeAt(seed, offset, x, z, context)`: a config-less biome's trees, then its grass, with their own
    /// placements, at the chunk's corner (unless it is at or under the sea level).
    fn place_at_corner(
        &mut self,
        mut offset: i32,
        biome_index: i32,
        [x, z]: [i32; 2],
        vegetation: &BiomeVegetation,
    ) -> Result<i32, GeneratorError> {
        let y = self.planting.top(x, z)?;
        if y <= self.sea_level {
            return Ok(offset);
        }
        let trees = vegetation.trees.iter().map(|tree| tree.id);
        for (feature, id) in trees.chain(vegetation.grass.iter().copied()).enumerate() {
            self.seed_feature(offset, || slot_index(CORNER, biome_index, 0, feature));
            if self.planting.place(self.random, id, None, [x, y, z]) {
                offset = offset.wrapping_add(1);
            }
        }
        Ok(offset)
    }

    /// `sample(seed, offset, x, z, frequency, jitter, context, PositionSampler::placeAt)` with the jittered
    /// `placeAt`: a grid of `frequency` cells (rows squashed by `SQUASH_FACTOR`, odd rows shifted half a cell), each
    /// point jittered by its cell's hash; a point in the chunk, above the sea level, in this biome and whose column's
    /// fitness is not under `(1 - density) * rand(hash)` places every tree there.
    #[allow(clippy::too_many_arguments)]
    fn sample(
        &mut self,
        mut offset: i32,
        biome_index: i32,
        config: usize,
        biome: BiomeId,
        vegetation: &BiomeVegetation,
        viability: &[f32; 256],
    ) -> Result<i32, GeneratorError> {
        let chunk = self.chunk;
        let config = &self.configs[config];
        let (freq_x, freq_z) = (config.frequency, config.frequency * squash_factor());
        let (min_x, min_z, max_x, max_z) = grid_bounds(
            chunk.x.wrapping_shl(4),
            chunk.z.wrapping_shl(4),
            config.frequency,
        );
        let cell_seed = self.seed as i32;
        for pz in min_z..=max_z {
            for px in min_x..=max_x {
                let (hash, x, z) = grid_point(cell_seed, px, pz, freq_x, freq_z, config.jitter);
                // isFeatureChunk
                if x >> 4 != chunk.x || z >> 4 != chunk.z {
                    continue;
                }
                let y = self.planting.top(x, z)?;
                if y <= self.sea_level {
                    continue;
                }
                let pos = [x, y, z];
                if self.planting.biome(pos)? != biome {
                    continue;
                }
                let fitness = viability[((z & 15) * 16 + (x & 15)) as usize];
                let noise = (1.0 - config.density) * rand(hash);
                if fitness < noise {
                    continue;
                }
                let column = (z & 15) * 16 + (x & 15);
                for (feature, tree) in vegetation.trees.iter().enumerate() {
                    self.seed_feature(offset, || slot_index(SAMPLED, biome_index, column, feature));
                    if self
                        .planting
                        .place(self.random, tree.id, tree.placement.as_deref(), pos)
                    {
                        offset = offset.wrapping_add(1);
                    }
                }
            }
        }
        Ok(offset)
    }

    /// `placeGrassAt`: [`grass_passes`] passes of the grass features from the chunk's corner at y 0, each pass's
    /// features seeded from `offset + pass` (with the successes so far).
    fn grass(
        &mut self,
        mut offset: i32,
        biome_index: i32,
        [x, z]: [i32; 2],
        vegetation: &BiomeVegetation,
    ) -> i32 {
        let passes = grass_passes(self.centre.0, self.centre.1);
        for i in 0..passes {
            for (feature, &id) in vegetation.grass.iter().enumerate() {
                self.seed_feature(offset.wrapping_add(i), || {
                    slot_index(GRASS, biome_index, i, feature)
                });
                if self.planting.place(self.random, id, None, [x, 0, z]) {
                    offset = offset.wrapping_add(1);
                }
            }
        }
        offset
    }

    /// `placeOther`: the origin biome's other vegetal features at the origin (`VegetationFeatures.create` never
    /// returns `NONE`, so Java's early return never happens).
    fn other(&mut self, mut offset: i32, origin: [i32; 3], biome: BiomeId) {
        for (feature, &id) in self.vegetation[biome.0 as usize].other.iter().enumerate() {
            self.seed_feature(offset, || slot_index(OTHER, 0, 0, feature));
            if self.planting.place(self.random, id, None, origin) {
                offset = offset.wrapping_add(1);
            }
        }
    }
}

/// `PositionSampler.SQUASH_FACTOR`: `2F / NoiseUtil.sqrt(3)` (`(float) Math.sqrt`, which `f32::sqrt` matches: both
/// round the exact root once).
fn squash_factor() -> f32 {
    2.0 / 3.0_f32.sqrt()
}

/// Whether the block `name` is one the decoration grows above the ground: logs, wood and leaves (not stripped: no
/// tree places those), huge mushroom blocks and stems, vines and bee nests.
fn is_canopy(name: &str) -> bool {
    [
        "_leaves",
        "_log",
        "_wood",
        "vine",
        "mushroom_block",
        "_stem",
        "bee_nest",
    ]
    .iter()
    .any(|part| name.contains(part))
        && !name.starts_with("minecraft:stripped")
}

/// The `deterministic` setting's feature-seed index of a vegetation placement: its `kind` (bits 28..31), the biome's
/// place in the chunk's first-met order (bits 20..27), its column or pass (bits 8..19) and the feature's place in its
/// list (bits 0..7). Fields wrap at their width (no chunk meets 256 biomes; no list holds 256 features).
fn slot_index(kind: i32, biome: i32, place: i32, feature: usize) -> i32 {
    kind << 28 | (biome & 0xFF) << 20 | (place & 0xFFF) << 8 | (feature as i32 & 0xFF)
}

/// `sample(seed, offset, x, z, freq, ..)`'s cell bounds for the chunk whose lowest block is `(x, z)`: `x - BORDER`
/// widens the `int` to float, `x + 15 + BORDER` adds 15 in `int` (wrapping) first.
fn grid_bounds(x: i32, z: i32, frequency: f32) -> (i32, i32, i32, i32) {
    let (freq_x, freq_z) = (frequency, frequency * squash_factor());
    (
        floor((x as f32 - BORDER) * freq_x),
        floor((z as f32 - BORDER) * freq_z),
        floor((x.wrapping_add(15) as f32 + BORDER) * freq_x),
        floor((z.wrapping_add(15) as f32 + BORDER) * freq_z),
    )
}

/// The inner `sample` loop's cell `(px, pz)`: `MathUtil.hash(cellSeed, px, pz)` and the jittered point
/// `floor(sx / freqX), floor(sz / freqZ)` (`randX`/`randZ` = `rand(hash(hash, X_PRIME / Y_PRIME))`).
fn grid_point(
    cell_seed: i32,
    px: i32,
    pz: i32,
    freq_x: f32,
    freq_z: f32,
    jitter: f32,
) -> (i32, i32, i32) {
    let ox = (pz & 1) as f32 * 0.5;
    let cell = hash_2d(cell_seed, px, pz);
    let dx = rand(hash(cell, X_PRIME));
    let dz = rand(hash(cell, Y_PRIME));
    let sx = px as f32 + ox + dx * jitter * 0.65;
    let sz = pz as f32 + dz * jitter;
    (cell, floor(sx / freq_x), floor(sz / freq_z))
}

/// `placeGrassAt`'s pass count: `2 + floor(2 * (1 - maxViability)) + floor(4 * river) - floor(5 * height)`, at
/// least 2, with `maxViability` 0 and `5 * height` in Java `int` math, widened to float for `NoiseUtil.floor`.
fn grass_passes(river: f32, height: i32) -> i32 {
    let mut passes = 2_i32;
    passes = passes.wrapping_add(floor(2.0 * (1.0 - 0.0)));
    passes = passes.wrapping_add(floor(4.0 * river));
    passes = passes.wrapping_sub(floor(5_i32.wrapping_mul(height) as f32));
    passes.max(2)
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

    /// `PositionSamplerOracle.java` (`.local/terraforged/oracles/position_sampler/`, `run.sh`): 6dd607eb's own
    /// `PositionSampler`, `SamplerContext` and `BiomeList`, compiled over stub worlds, on 26.3's `WorldgenRandom`. `P`
    /// lines: per seed and chunk, the placements of `placeVegetation` over four biomes (no config; config A; a river
    /// with config A; config B, one of [`GRID_CONFIGS`]) and four terrain centres, and a fold of each placement's
    /// feature, unwrapping, position and two draws. `G` lines: the cells (hash and point) of `sample`'s grid for each
    /// config.
    const JAVA_VEGETATION: &str = "\
P 0 0 0 1014 -7824645947807395381\n\
P 0 3 -2 1046 -3869755918850437142\n\
P 0 -7 4 750 1710284815390465178\n\
P 0 1875000 -1875000 1246 -2569214469645208999\n\
P 0 134217727 -134217728 558 9001433799512573109\n\
P 0 -1 -1 982 -3935649146532020042\n\
P 7 0 0 1318 4518252628030312556\n\
P 7 3 -2 1278 5937033624461309741\n\
P 7 -7 4 866 7828589533538317727\n\
P 7 1875000 -1875000 1146 6418870736470320395\n\
P 7 134217727 -134217728 558 -4012886488193598316\n\
P 7 -1 -1 1054 1615616775035044733\n\
P -4172144997902289642 0 0 1174 -1782629333388028419\n\
P -4172144997902289642 3 -2 998 7740323252981822052\n\
P -4172144997902289642 -7 4 866 4734710214459629574\n\
P -4172144997902289642 1875000 -1875000 1234 3051150899061939389\n\
P -4172144997902289642 134217727 -134217728 558 6479053801473790690\n\
P -4172144997902289642 -1 -1 914 -1442960748111000480\n\
P -9223372036854775808 0 0 1014 -7824645947807395381\n\
P -9223372036854775808 3 -2 1046 -3869755918850437142\n\
P -9223372036854775808 -7 4 750 1710284815390465178\n\
P -9223372036854775808 1875000 -1875000 1246 -2569214469645208999\n\
P -9223372036854775808 134217727 -134217728 558 9001433799512573109\n\
P -9223372036854775808 -1 -1 982 -3935649146532020042\n\
P 9223372036854775807 0 0 1146 -4745633643961142130\n\
P 9223372036854775807 3 -2 1178 -1903102337649952436\n\
P 9223372036854775807 -7 4 998 -2758851159177341288\n\
P 9223372036854775807 1875000 -1875000 1310 -4663784590082957892\n\
P 9223372036854775807 134217727 -134217728 558 -412412491611821423\n\
P 9223372036854775807 -1 -1 930 6964717499533469478\n\
P 123456789012345 0 0 982 -6100181328469900797\n\
P 123456789012345 3 -2 1006 -1543596993396307456\n\
P 123456789012345 -7 4 922 5895283090055492243\n\
P 123456789012345 1875000 -1875000 1378 7528485731507164264\n\
P 123456789012345 134217727 -134217728 558 8763757097191631043\n\
P 123456789012345 -1 -1 938 4919914697760528874\n\
G 0 0 0 3300 847937686874229593\n\
G 0 3 -2 3162 -2835965235355760722\n\
G 0 -7 4 3259 -8438176476420211023\n\
G 0 1875000 -1875000 3316 155653574403403975\n\
G 0 134217727 -134217728 9 -8039755307983892340\n\
G 0 -1 -1 3292 -8712869352202709176\n\
G 7 0 0 3300 4204939772180822938\n\
G 7 3 -2 3162 8288703493095732005\n\
G 7 -7 4 3259 7978997956930818544\n\
G 7 1875000 -1875000 3316 -4921097991689895810\n\
G 7 134217727 -134217728 9 1900122104232702106\n\
G 7 -1 -1 3292 -7778877044908744430\n\
G -4172144997902289642 0 0 3300 5056805913034277411\n\
G -4172144997902289642 3 -2 3162 7473341095950493239\n\
G -4172144997902289642 -7 4 3259 -5367574249768170629\n\
G -4172144997902289642 1875000 -1875000 3316 -5505200081247028975\n\
G -4172144997902289642 134217727 -134217728 9 -3455049682260732096\n\
G -4172144997902289642 -1 -1 3292 4878292620917612799\n\
G -9223372036854775808 0 0 3300 847937686874229593\n\
G -9223372036854775808 3 -2 3162 -2835965235355760722\n\
G -9223372036854775808 -7 4 3259 -8438176476420211023\n\
G -9223372036854775808 1875000 -1875000 3316 155653574403403975\n\
G -9223372036854775808 134217727 -134217728 9 -8039755307983892340\n\
G -9223372036854775808 -1 -1 3292 -8712869352202709176\n\
G 9223372036854775807 0 0 3300 3422705114750334516\n\
G 9223372036854775807 3 -2 3162 -1854709904146424179\n\
G 9223372036854775807 -7 4 3259 5564000000370240274\n\
G 9223372036854775807 1875000 -1875000 3316 -4083763934215314854\n\
G 9223372036854775807 134217727 -134217728 9 1122241109656609825\n\
G 9223372036854775807 -1 -1 3292 -4801647018637383876\n\
G 123456789012345 0 0 3300 -2628198402422011777\n\
G 123456789012345 3 -2 3162 -7369022319576744684\n\
G 123456789012345 -7 4 3259 256032975198899179\n\
G 123456789012345 1875000 -1875000 3316 -2586999637569324817\n\
G 123456789012345 134217727 -134217728 9 2482080171667295833\n\
G 123456789012345 -1 -1 3292 846005903824255357\n\
";

    /// Frequency, jitter and density: the eight bundled configs' (one shared by two), then edges.
    #[rustfmt::skip]
    const GRID_CONFIGS: [[f32; 3]; 11] = [
        [0.2, 0.8, 0.6], [0.22, 0.8, 0.7], [0.2, 0.75, 0.5], [0.35, 0.75, 0.7], [0.15, 0.75, 0.35],
        [0.15, 0.8, 0.45], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0], [-0.2, 0.8, 0.6], [1.5, 2.0, -1.0], [0.05, 0.0, 0.0],
    ];
    /// The terrain centres (`getRiver().get(8, 8)`, `getHeight(8, 8)`) for the grass passes.
    const CENTRES: [(f32, i32); 4] = [(0.0, 70), (0.9, 0), (0.75, -1), (1.0, -3)];

    /// At chunk `i32::MAX >> 4` a frequency of 1 or more saturates both grid bounds to `i32::MAX`, where Java's
    /// `px <= maxX; px++` never ends (the port's loop stops): the oracle skips those cases.
    fn hangs(cx: i32, frequency: f32) -> bool {
        cx == i32::MAX >> 4 && frequency >= 1.0
    }

    /// The oracle's stub world: tops 40..103 around sea level 62, biomes by 8×8 patches and height, hashed fitness,
    /// and features that fold what they are given and succeed on `nextInt(4) == 0`.
    struct StubPlanting {
        h: i64,
        placed: usize,
    }

    impl Planting for StubPlanting {
        fn top(&mut self, x: i32, z: i32) -> Result<i32, GeneratorError> {
            Ok(40 + (hash_2d(11, x, z) & 63))
        }

        fn biome(&mut self, [x, y, z]: [i32; 3]) -> Result<BiomeId, GeneratorError> {
            Ok(BiomeId((hash_2d(12 ^ y, x >> 3, z >> 3) & 3) as u32))
        }

        fn fitness(&mut self, config: usize, x: i32, z: i32) -> Result<f32, GeneratorError> {
            Ok(rand(hash_2d(20 + config as i32, x, z)))
        }

        fn place(
            &mut self,
            random: &mut Random,
            feature: FeatureIdentity,
            placement: Option<&[PlacementModifier]>,
            [x, y, z]: [i32; 3],
        ) -> bool {
            let h = &mut self.h;
            for v in [
                feature.0 as i64,
                placement.is_some() as i64,
                x as i64,
                y as i64,
                z as i64,
            ] {
                fold(h, v);
            }
            let d = random.next_int_bound(4).unwrap();
            fold(h, d as i64);
            fold(h, random.next_long());
            self.placed += 1;
            d == 0
        }
    }

    fn stub_vegetation() -> Vec<BiomeVegetation> {
        let trees = |ids: &[u32], unwrapped: bool| {
            ids.iter()
                .map(|&id| VegetationTree {
                    id: FeatureIdentity(id),
                    placement: unwrapped.then(Vec::new),
                })
                .collect()
        };
        let ids = |ids: &[u32]| ids.iter().map(|&id| FeatureIdentity(id)).collect();
        vec![
            BiomeVegetation {
                config: None,
                river: false,
                trees: trees(&[100], false),
                grass: ids(&[101, 102]),
                other: ids(&[103]),
            },
            BiomeVegetation {
                config: Some(0),
                river: false,
                trees: trees(&[110, 111], true),
                grass: ids(&[112]),
                other: ids(&[113, 114]),
            },
            BiomeVegetation {
                config: Some(0),
                river: true,
                trees: trees(&[120], true),
                grass: ids(&[121]),
                other: ids(&[122]),
            },
            BiomeVegetation {
                config: Some(1),
                river: false,
                trees: trees(&[130], true),
                grass: ids(&[]),
                other: ids(&[]),
            },
        ]
    }

    fn stub_config([frequency, jitter, density]: [f32; 3]) -> VegetationConfig {
        VegetationConfig {
            frequency,
            jitter,
            density,
            tag: String::new(),
            biomes: Default::default(),
            viability: crate::worldgen::generator::terraforged_biome_viability::Viability::None,
        }
    }

    /// The vegetal step (populate, the corner, the jittered grid with its density roll, the grass passes and the
    /// other features, rivers skipped) seeds and places like 6dd607eb's `PositionSampler` on 26.3's random, and its
    /// grid's cells match `sample`'s, bit for bit.
    #[test]
    fn vegetation_matches_6dd607eb_position_sampler_on_26_3_random() {
        let vegetation = stub_vegetation();
        let mut lines = JAVA_VEGETATION.lines();
        for seed in SEEDS {
            for [cx, cz] in CHUNKS {
                let mut planting = StubPlanting { h: 0, placed: 0 };
                for config in GRID_CONFIGS {
                    if hangs(cx, config[0]) {
                        continue;
                    }
                    let configs = [stub_config([0.2, 0.8, 0.6]), stub_config(config)];
                    for centre in CENTRES {
                        let origin = [cx << 4, -64, cz << 4];
                        let biome = planting.biome(origin).unwrap();
                        let mut random = Random::new(LegacyRandom::new(seed));
                        let mut sampler = PositionSampler {
                            planting,
                            random: &mut random,
                            seed,
                            chunk: ChunkPos::new(cx, cz),
                            vegetation: &vegetation,
                            configs: &configs,
                            sea_level: 62,
                            centre,
                            deterministic: false,
                        };
                        sampler.place_vegetation(origin, biome).unwrap();
                        planting = sampler.planting;
                    }
                }
                let line = format!("P {seed} {cx} {cz} {} {}", planting.placed, planting.h);
                assert_eq!(Some(line.as_str()), lines.next());
            }
        }
        for seed in SEEDS {
            for [cx, cz] in CHUNKS {
                let (mut h, mut cells) = (0_i64, 0);
                for [frequency, jitter, _] in GRID_CONFIGS {
                    if hangs(cx, frequency) {
                        continue;
                    }
                    let (min_x, min_z, max_x, max_z) = grid_bounds(cx << 4, cz << 4, frequency);
                    for pz in min_z..=max_z {
                        for px in min_x..=max_x {
                            let (hash, x, z) = grid_point(
                                seed as i32,
                                px,
                                pz,
                                frequency,
                                frequency * squash_factor(),
                                jitter,
                            );
                            for v in [hash, x, z] {
                                fold(&mut h, v as i64);
                            }
                            cells += 1;
                        }
                    }
                }
                let line = format!("G {seed} {cx} {cz} {cells} {h}");
                assert_eq!(Some(line.as_str()), lines.next());
            }
        }
        assert_eq!(lines.next(), None);
    }

    /// A stub world that records every placement's feature, position and first draw, and lets a placement succeed
    /// by `succeeds` (not by the random, so the draws recorded are the seeds').
    struct Recording {
        succeeds: fn(usize) -> bool,
        attempts: Vec<(u32, [i32; 3], i64)>,
    }

    impl Planting for Recording {
        fn top(&mut self, x: i32, z: i32) -> Result<i32, GeneratorError> {
            Ok(40 + (hash_2d(11, x, z) & 63))
        }

        fn biome(&mut self, [x, y, z]: [i32; 3]) -> Result<BiomeId, GeneratorError> {
            Ok(BiomeId((hash_2d(12 ^ y, x >> 3, z >> 3) & 3) as u32))
        }

        fn fitness(&mut self, config: usize, x: i32, z: i32) -> Result<f32, GeneratorError> {
            Ok(rand(hash_2d(20 + config as i32, x, z)))
        }

        fn place(
            &mut self,
            random: &mut Random,
            feature: FeatureIdentity,
            _: Option<&[PlacementModifier]>,
            pos: [i32; 3],
        ) -> bool {
            self.attempts.push((feature.0, pos, random.next_long()));
            (self.succeeds)(self.attempts.len())
        }
    }

    /// With `deterministic`, every vegetation placement gets the same seed whichever placements before it succeed
    /// (a neighbour's tree in the way, or not); the original's running count reseeds what follows a success, and its
    /// seeds collide. The slots stay apart from each other.
    #[test]
    fn deterministic_seeds_ignore_earlier_placements() {
        let vegetation = stub_vegetation();
        let configs = [stub_config([0.2, 0.8, 0.6]), stub_config([0.1, 0.9, 0.9])];
        let run = |deterministic: bool, succeeds: fn(usize) -> bool| {
            let mut all = Vec::new();
            for [cx, cz] in [[0, 0], [3, -2], [-7, 5]] {
                let planting = Recording {
                    succeeds,
                    attempts: Vec::new(),
                };
                let mut random = Random::new(LegacyRandom::new(5));
                let origin = [cx << 4, -64, cz << 4];
                let mut sampler = PositionSampler {
                    planting,
                    random: &mut random,
                    seed: 99,
                    chunk: ChunkPos::new(cx, cz),
                    vegetation: &vegetation,
                    configs: &configs,
                    sea_level: 62,
                    centre: (0.75, 0),
                    deterministic,
                };
                let biome = sampler.planting.biome(origin).unwrap();
                sampler.place_vegetation(origin, biome).unwrap();
                all.push(sampler.planting.attempts);
            }
            all
        };
        let (none, every_third, all) = (|_| false, |n| n % 3 == 0, |_| true);
        let fixed = run(true, none);
        assert!(fixed.iter().all(|chunk| !chunk.is_empty()), "{fixed:?}");
        assert!(fixed.iter().map(Vec::len).sum::<usize>() > 20, "{fixed:?}");
        assert_eq!(run(true, every_third), fixed);
        assert_eq!(run(true, all), fixed);
        for chunk in &fixed {
            let mut draws: Vec<i64> = chunk.iter().map(|&(_, _, draw)| draw).collect();
            draws.sort_unstable();
            draws.dedup();
            assert_eq!(draws.len(), chunk.len(), "two placements share a seed");
        }
        // The faithful port's seeds follow the successes.
        assert_ne!(run(false, every_third), run(false, none));
        assert_eq!(run(false, none)[0].len(), fixed[0].len());
    }

    #[test]
    fn slot_indices_keep_their_fields_apart() {
        assert_eq!(slot_index(CORNER, 0, 0, 0), 1 << 28);
        assert_eq!(slot_index(OTHER, 0, 0, 3), (4 << 28) | 3);
        assert_eq!(
            slot_index(SAMPLED, 2, 255, 7),
            (2 << 28) | (2 << 20) | (255 << 8) | 7
        );
        // Each field wraps at its width instead of reaching the next.
        assert_eq!(slot_index(GRASS, 256, 4096, 256), 3 << 28);
    }

    /// `placeGrassAt`'s pass count: 2 + 2 (`maxViability` stays 0) + `floor(4 * river)` - `5 * height`, at least 2.
    #[test]
    fn grass_passes_follow_place_grass_at() {
        assert_eq!(grass_passes(0.0, 0), 4);
        assert_eq!(grass_passes(0.75, 0), 7);
        // NoiseUtil.floor takes 1 off a negative whole number too: floor(-15F) is -16.
        assert_eq!(grass_passes(1.0, -3), 24);
        assert_eq!(grass_passes(0.5, 70), 2);
        assert_eq!(grass_passes(f32::NAN, 0), 3);
        // 5 * height wraps in Java int math.
        assert_eq!(grass_passes(0.0, i32::MAX / 5 + 1), 2);
        assert_eq!(grass_passes(0.0, i32::MIN / 5 - 1), 2);
    }
}
