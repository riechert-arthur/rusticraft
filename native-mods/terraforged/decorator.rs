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

// `FeatureDecorator`, `VanillaDecorator`, `PositionSampler` and `NoiseCaveDecorator`:
// TerraForged decorates a chunk with the one biome at its lowest corner, seeds every
// feature from a legacy random, places vanilla features before and after the vegetal
// step, and its own trees and grass in between. Spliced into the decoration module for
// its world and feature table.

use super::*;
use crate::worldgen::generator::{
    terraforged_cave_carver::NoiseCaveGenerator,
    terraforged_generator_facade::TerraForgedGenerator,
    terraforged_surface_bindings::SurfaceBindings,
};
use horizon_core::random::LegacyRandom;

const LOCAL_MODIFICATIONS: usize = 2;
/// `VegetationFeatures.STAGE`: `VEGETAL_DECORATION`.
const VEGETAL: usize = 9;
/// `TOP_LAYER_MODIFICATION`.
const MAX_STAGE: usize = 10;

/// What TerraForged adds to the vanilla decoration resources.
pub(in crate::worldgen) struct TerraForgedDecoration<'a> {
    pub generator: &'a TerraForgedGenerator,
    pub caves: &'a NoiseCaveGenerator<BiomeId>,
    pub surface: &'a SurfaceBindings<'a>,
}

type Random = WorldgenRandom<LegacyRandom>;

/// `Generator.applyBiomeDecoration` for the centre of a 3x3 region.
#[allow(clippy::too_many_arguments)]
pub(in crate::worldgen) fn decorate(
    decoration: &Decoration,
    data: &Arc<GeneratorData>,
    seed: i64,
    generation: WorldGenerationContext,
    worker: &mut WorkerState,
    region: &mut [ProtoChunk; 9],
    level_random: &mut dyn super::super::structure::legacy::PieceRandom,
    structures: Option<&super::super::generator_structures::StructureState>,
    tf: &TerraForgedDecoration<'_>,
) -> Result<DecorateStats, GeneratorError> {
    let center = region[4].position();
    if region[4].is_decorated() {
        return Ok(DecorateStats::default());
    }
    let int_seed = seed as i32;
    let terrain_handle = tf.generator.chunk_data(center.x, center.z);
    let terrain = terrain_handle.read();
    // getPostCarveChunk: the caves' biome lists, replayed without writing.
    let carver = {
        let chunk = &mut region[4];
        let mut biomes = chunk.biomes().clone();
        let levels = tf.generator.levels();
        let air = data
            .registry()
            .default_state("minecraft:air")
            .ok_or_else(|| resource("air"))?;
        tf.caves
            .carve(
                int_seed,
                &mut chunk.terrain,
                &mut biomes,
                &terrain,
                levels.min_y,
                levels.sea_level,
                air,
                false,
            )
            .map_err(|e| resource(e.to_string()))?
    };

    let mut chunks = Vec::with_capacity(9);
    let mut biomes = Vec::with_capacity(9);
    let (min_y, height) = (region[4].terrain.min_y(), region[4].terrain.height());
    for chunk in region.iter_mut() {
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
        build: (min_y, min_y + height),
        shape_ticks: std::cell::RefCell::new(crate::level::Ticks::new(0)),
    };
    let mut decorator = Decorator {
        decoration,
        data,
        stats: DecorateStats::default(),
        placer: FeaturePlacer::new(),
        steps: HashMap::new(),
        structures,
        references,
        chunk_box: BoundingBox::new(
            [center.x * 16, min_y + 1, center.z * 16],
            [center.x * 16 + 15, min_y + height - 1, center.z * 16 + 15],
        ),
    };

    // FeatureDecorator.decorate.
    let origin = [center.x.wrapping_shl(4), min_y, center.z.wrapping_shl(4)];
    let biome = world
        .biome_identity(origin)
        .map_err(|e| resource(e.to_string()))?;
    let mut random = Random::new(LegacyRandom::new(seed));
    let decoration_seed = random.set_decoration_seed(seed, origin[0], origin[2]);
    let result = (|| -> Result<(), GeneratorError> {
        decorator.vanilla(
            &mut world,
            &mut random,
            decoration_seed,
            0,
            VEGETAL - 1,
            origin,
            biome,
        )?;
        // The vegetal step (PositionSampler) is TerraForged's own; see the next change.
        decorator.vanilla(
            &mut world,
            &mut random,
            decoration_seed,
            VEGETAL + 1,
            MAX_STAGE,
            origin,
            biome,
        )?;
        // NoiseCaveGenerator.decorate.
        for (index, config) in tf.caves.caves().iter().enumerate() {
            let Some(cave_biomes) = carver.biomes(index) else {
                continue;
            };
            let start_y = config
                .height(int_seed, origin[0], origin[2])
                .map_err(resource)?;
            let pos = [origin[0], start_y, origin[2]];
            let mut random = Random::new(LegacyRandom::new(seed));
            for &cave_biome in cave_biomes {
                let base = random.set_decoration_seed(seed, pos[0], pos[2]);
                let steps = decorator.steps(cave_biome);
                for (stage, features) in steps.iter().enumerate().skip(LOCAL_MODIFICATIONS) {
                    for (index, name) in features.iter().enumerate() {
                        random.set_feature_seed(base, index as i32, stage as i32);
                        decorator.place(&mut world, &mut random, name, false, pos);
                    }
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

    // Surface.smoothWater and Surface.applyPost on the centre chunk.
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
        .map_err(|e| resource(e.to_string()))?;
    tf.surface
        .apply_post(&terrain, &mut chunk.terrain, tf.generator.levels().max_y)
        .map_err(|e| resource(e.to_string()))?;
    drop(terrain);
    tf.generator.drop_chunk(center.x, center.z);

    let stats = std::mem::take(&mut decorator.stats);
    region[4]
        .terrain
        .set_generation_stage(PreLightChunkStage::Features);
    region[4].set_decorated();
    Ok(stats)
}

struct Decorator<'a> {
    decoration: &'a Decoration,
    data: &'a Arc<GeneratorData>,
    stats: DecorateStats,
    placer: FeaturePlacer,
    /// Each biome's feature ids per step, from its definition.
    steps: HashMap<BiomeId, Arc<Vec<Vec<String>>>>,
    structures: Option<&'a super::super::generator_structures::StructureState>,
    references: Vec<Arc<super::super::structure::start::StructureStart>>,
    chunk_box: BoundingBox,
}

impl Decorator<'_> {
    fn steps(&mut self, biome: BiomeId) -> Arc<Vec<Vec<String>>> {
        let data = self.data;
        Arc::clone(self.steps.entry(biome).or_insert_with(|| {
            let json: Value = serde_json::from_str(data.biome_json(biome)).unwrap_or_default();
            Arc::new(
                json["features"]
                    .as_array()
                    .map(|steps| {
                        steps
                            .iter()
                            .map(|step| {
                                step.as_array()
                                    .map(|ids| {
                                        ids.iter()
                                            .filter_map(|id| id.as_str().map(str::to_owned))
                                            .collect()
                                    })
                                    .unwrap_or_default()
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            )
        }))
    }

    /// `PlacedFeature.placeWithBiomeCheck`; `unwrap` drops the tree's count, square and
    /// biome modifiers (`VegetationFeatures.unwrap`).
    fn place(
        &mut self,
        world: &mut World<'_>,
        random: &mut Random,
        name: &str,
        unwrap: bool,
        origin: [i32; 3],
    ) -> bool {
        let Some(index) = self.decoration.names.iter().position(|known| known == name) else {
            return false;
        };
        let Ok(feature) = &self.decoration.features[index] else {
            self.stats.unsupported += 1;
            self.stats.skipped.insert(name.to_owned());
            return false;
        };
        let mut placement = feature.placement.clone();
        if unwrap {
            use super::super::placement::PlacementModifier as P;
            placement.retain(|modifier| {
                !matches!(
                    modifier,
                    P::Biome
                        | P::Count(_)
                        | P::CountOnEveryLayer(_)
                        | P::NoiseBasedCount { .. }
                        | P::NoiseThresholdCount { .. }
                        | P::InSquare
                )
            });
        }
        self.stats.placed += 1;
        let blocks = self.decoration.blocks;
        let placed = self.placer.place_with_biome_check(
            &placement,
            world,
            random,
            origin,
            FeatureIdentity(index as u32),
            |world, random, p| feature.feature.place(world, random, p, blocks),
        );
        if placed.is_err() {
            self.stats.failed += 1;
            self.stats.skipped.insert(name.to_owned());
        }
        placed.unwrap_or(false)
    }

    /// `VanillaDecorator.decorate` over `from..=to`.
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
        let steps = self.steps(biome);
        let tree_blocks = self
            .decoration
            .bindings
            .tree_blocks()
            .map_err(|e| resource(e.to_string()))?;
        for stage in from..=to {
            let Some(features) = steps.get(stage) else {
                continue;
            };
            let in_step = self
                .structures
                .and_then(|structures| structures.by_step.get(stage).cloned())
                .unwrap_or_default();
            for (index, &structure) in in_step.iter().enumerate() {
                random.set_feature_seed(seed, index as i32, stage as i32);
                let structures = self.structures.expect("structures by step");
                let blocks = self.decoration.blocks;
                for start in self
                    .references
                    .iter()
                    .filter(|start| start.structure == structure)
                {
                    let mut place_feature =
                        |world: &mut World<'_>, random: &mut Random, name: &str, pos: [i32; 3]| {
                            match self.decoration.named_feature(name) {
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
                        self.stats.failed += 1;
                    }
                }
            }
            for (index, name) in features.iter().enumerate() {
                random.set_feature_seed(seed, (in_step.len() + index) as i32, stage as i32);
                self.place(world, random, name, false, origin);
            }
        }
        Ok(())
    }
}
