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
    terraforged_biome_viability::ViabilityContext,
    terraforged_cave_carver::NoiseCaveGenerator,
    terraforged_continent_cell::{hash_1d, random as rand},
    terraforged_generator_facade::TerraForgedGenerator,
    terraforged_surface_bindings::SurfaceBindings,
    terraforged_terrain_data::TerrainData,
    terraforged_vegetation::{VegetationConfig, VegetationFeatures, config_for},
};
use horizon_core::random::LegacyRandom;

const LOCAL_MODIFICATIONS: usize = 2;
/// `VegetationFeatures.STAGE`: `VEGETAL_DECORATION`.
const VEGETAL: usize = 9;
/// `TOP_LAYER_MODIFICATION`.
const MAX_STAGE: usize = 10;
const BORDER: f32 = 6.0;
const X_PRIME: i32 = 1_619;
// `deterministic`'s feature-seed slots: apart, and past the original's offsets.
const CORNER_TREES: i32 = 0x1000;
const CORNER_GRASS: i32 = 0x1100;
const GRASS: i32 = 0x2000;
const OTHER: i32 = 0x3000;
const SAMPLED_TREES: i32 = 0x10000;
const Y_PRIME: i32 = 31_337;

/// What TerraForged adds to the vanilla decoration resources.
pub(in crate::worldgen) struct TerraForgedDecoration<'a> {
    pub generator: &'a TerraForgedGenerator,
    pub vegetation: &'a [VegetationConfig],
    pub caves: &'a NoiseCaveGenerator<BiomeId>,
    pub surface: &'a SurfaceBindings<'a>,
    pub settings: super::super::generator::terraforged_settings::Settings,
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
        deterministic: tf.settings.deterministic,
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
        let offset = decorator.trees_and_grass(
            &mut world,
            &mut random,
            decoration_seed,
            center,
            &terrain,
            tf,
        )?;
        decorator.other(
            &mut world,
            &mut random,
            decoration_seed,
            offset,
            origin,
            biome,
            tf,
        )?;
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
    /// `deterministic`: fixed feature-seed slots and the ground under leaves.
    deterministic: bool,
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

    fn vegetation(
        &mut self,
        biome: BiomeId,
        tf: &TerraForgedDecoration<'_>,
    ) -> (Option<usize>, VegetationFeatures) {
        let name = self.data.biome_name(biome).to_owned();
        let config = tf
            .vegetation
            .iter()
            .position(|config| config_for(std::slice::from_ref(config), &name).is_some());
        let steps = self.steps(biome);
        let vegetal = steps
            .get(VEGETAL)
            .map(|ids| ids.iter().map(String::as_str).collect::<Vec<_>>())
            .unwrap_or_default();
        let features = VegetationFeatures::create(vegetal, config.is_some());
        (config, features)
    }

    /// The feature-seed index of a vegetation placement: the original's running count
    /// of placed features, or, with `deterministic`, a fixed slot for the
    /// placement, so a blocked tree no longer reseeds the rest of its chunk.
    fn seed_index(&self, offset: usize, slot: i32) -> i32 {
        if self.deterministic {
            slot
        } else {
            offset as i32
        }
    }

    /// `chunk.getHeight(OCEAN_FLOOR_WG, x, z)`: the top block, one under the first free
    /// one. With `deterministic`, below the trees, vines and mushrooms
    /// decoration put there, so a neighbour's leaves don't move the ground.
    fn top(&self, world: &mut World<'_>, x: i32, z: i32) -> Result<i32, GeneratorError> {
        let mut y = world
            .height(HeightmapType::OceanFloorWg, x, z)
            .map(|first_free| first_free - 1)
            .map_err(|e| resource(e.to_string()))?;
        if self.deterministic {
            let registry = self.data.registry();
            for _ in 0..64 {
                let state =
                    super::super::placement::PlacementContext::block_state(world, [x, y, z])
                        .map_err(|e| resource(e.to_string()))?;
                let decoration = registry.state(state).is_some_and(|block| {
                    let name = block.name.as_str();
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
                });
                if !decoration {
                    break;
                }
                y -= 1;
            }
        }
        Ok(y)
    }

    /// `PositionSampler.placeTreesAndGrass`; returns the feature-seed offset it reached.
    fn trees_and_grass(
        &mut self,
        world: &mut World<'_>,
        random: &mut Random,
        seed: i64,
        chunk: ChunkPos,
        terrain: &TerrainData,
        tf: &TerraForgedDecoration<'_>,
    ) -> Result<usize, GeneratorError> {
        let (start_x, start_z) = (chunk.x.wrapping_shl(4), chunk.z.wrapping_shl(4));
        let sea_level = tf.generator.sea_level();
        let context = TerrainViability(terrain);
        // populate: each column's fitness under its biome's config, and the biomes met.
        let mut viability = [0.0_f32; 256];
        let mut biome_list: Vec<BiomeId> = Vec::new();
        for dz in 0..16 {
            for dx in 0..16 {
                let (x, z) = (start_x + dx, start_z + dz);
                let y = self.top(world, x, z)?;
                let biome = world
                    .biome_identity([x, y, z])
                    .map_err(|e| resource(e.to_string()))?;
                let (config, _) = self.vegetation(biome, tf);
                viability[(dz * 16 + dx) as usize] = match config {
                    Some(config) => tf.vegetation[config].viability.fitness(x, z, &context),
                    None => super::super::generator::terraforged_biome_viability::Viability::None
                        .fitness(x, z, &context),
                };
                if !biome_list.contains(&biome) {
                    biome_list.push(biome);
                }
            }
        }
        let rivers = self
            .data
            .biome_tags
            .get("minecraft:is_river")
            .cloned()
            .unwrap_or_default();
        let mut offset = 0usize;
        for (i, &biome) in biome_list.iter().enumerate() {
            // "Remove annoying river trees!"
            if rivers
                .iter()
                .any(|name| name == self.data.biome_name(biome))
            {
                continue;
            }
            let (config, features) = self.vegetation(biome, tf);
            match config {
                None => {
                    offset = self.place_at_corner(
                        world,
                        random,
                        seed,
                        offset + i,
                        start_x,
                        start_z,
                        sea_level,
                        &features,
                    )?;
                }
                Some(index) => {
                    let config = &tf.vegetation[index];
                    offset = self.sample(
                        world,
                        random,
                        seed,
                        offset + i,
                        chunk,
                        config,
                        biome,
                        &viability,
                        sea_level,
                        &features,
                    )?;
                    offset = self.grass(
                        world,
                        random,
                        seed,
                        offset + i,
                        start_x,
                        start_z,
                        terrain,
                        &features,
                    );
                }
            }
        }
        Ok(offset)
    }

    /// `placeAt(seed, offset, x, z, context)`: a config-less biome's trees and grass at the
    /// chunk's corner, with their own placement.
    #[allow(clippy::too_many_arguments)]
    fn place_at_corner(
        &mut self,
        world: &mut World<'_>,
        random: &mut Random,
        seed: i64,
        mut offset: usize,
        x: i32,
        z: i32,
        sea_level: i32,
        features: &VegetationFeatures,
    ) -> Result<usize, GeneratorError> {
        let y = self.top(world, x, z)?;
        if y <= sea_level {
            return Ok(offset);
        }
        for (slot, (name, unwrap)) in features.trees.iter().enumerate() {
            let index = self.seed_index(offset, CORNER_TREES + slot as i32);
            random.set_feature_seed(seed, index, VEGETAL as i32);
            if self.place(world, random, name, *unwrap, [x, y, z]) {
                offset += 1;
            }
        }
        for (slot, name) in features.grass.iter().enumerate() {
            let index = self.seed_index(offset, CORNER_GRASS + slot as i32);
            random.set_feature_seed(seed, index, VEGETAL as i32);
            if self.place(world, random, name, false, [x, y, z]) {
                offset += 1;
            }
        }
        Ok(offset)
    }

    /// `PositionSampler.sample` with the jittered `placeAt`: trees on a squashed,
    /// jittered grid, where the column's fitness beats the cell's roll.
    #[allow(clippy::too_many_arguments)]
    fn sample(
        &mut self,
        world: &mut World<'_>,
        random: &mut Random,
        seed: i64,
        mut offset: usize,
        chunk: ChunkPos,
        config: &VegetationConfig,
        biome: BiomeId,
        viability: &[f32; 256],
        sea_level: i32,
        features: &VegetationFeatures,
    ) -> Result<usize, GeneratorError> {
        let (x, z) = (chunk.x.wrapping_shl(4), chunk.z.wrapping_shl(4));
        let freq_x = config.frequency;
        let freq_z = config.frequency * (2.0 / 3.0_f32.sqrt());
        let min_x = ((x as f32 - BORDER) * freq_x).floor() as i32;
        let min_z = ((z as f32 - BORDER) * freq_z).floor() as i32;
        let max_x = ((x as f32 + 15.0 + BORDER) * freq_x).floor() as i32;
        let max_z = ((z as f32 + 15.0 + BORDER) * freq_z).floor() as i32;
        let cell_seed = seed as i32;
        for pz in min_z..=max_z {
            let ox = (pz & 1) as f32 * 0.5;
            for px in min_x..=max_x {
                let hash = crate::worldgen::generator::terraforged::hash_2d(cell_seed, px, pz);
                let dx = rand(hash_1d(hash, X_PRIME));
                let dz = rand(hash_1d(hash, Y_PRIME));
                let sx = px as f32 + ox + dx * config.jitter * 0.65;
                let sz = pz as f32 + dz * config.jitter;
                let pos_x = (sx / freq_x).floor() as i32;
                let pos_z = (sz / freq_z).floor() as i32;
                if pos_x >> 4 != chunk.x || pos_z >> 4 != chunk.z {
                    continue;
                }
                let y = self.top(world, pos_x, pos_z)?;
                if y <= sea_level {
                    continue;
                }
                let at = world
                    .biome_identity([pos_x, y, pos_z])
                    .map_err(|e| resource(e.to_string()))?;
                if at != biome {
                    continue;
                }
                let fitness = viability[((pos_z & 15) * 16 + (pos_x & 15)) as usize];
                let noise = (1.0 - config.density) * rand(hash);
                if fitness < noise {
                    continue;
                }
                let column = ((pos_z & 15) * 16 + (pos_x & 15)) * 16;
                for (slot, (name, unwrap)) in features.trees.iter().enumerate() {
                    let index = self.seed_index(offset, SAMPLED_TREES + column + slot as i32);
                    random.set_feature_seed(seed, index, VEGETAL as i32);
                    if self.place(world, random, name, *unwrap, [pos_x, y, pos_z]) {
                        offset += 1;
                    }
                }
            }
        }
        Ok(offset)
    }

    /// `placeGrassAt`: at least two passes of the grass features from the chunk's corner
    /// at y 0 (the context's maximum fitness is never raised from 0).
    #[allow(clippy::too_many_arguments)]
    fn grass(
        &mut self,
        world: &mut World<'_>,
        random: &mut Random,
        seed: i64,
        mut offset: usize,
        x: i32,
        z: i32,
        terrain: &TerrainData,
        features: &VegetationFeatures,
    ) -> usize {
        let mut passes = 2;
        passes += (2.0 * (1.0 - 0.0_f32)).floor() as i32;
        passes += (4.0 * terrain.river().get(8, 8)).floor() as i32;
        passes -= (5.0 * terrain.height_at(8, 8) as f32).floor() as i32;
        let passes = passes.max(2);
        for i in 0..passes {
            for (slot, name) in features.grass.iter().enumerate() {
                let index = if self.deterministic {
                    GRASS + i * 64 + slot as i32
                } else {
                    (offset as i32).wrapping_add(i)
                };
                random.set_feature_seed(seed, index, VEGETAL as i32);
                if self.place(world, random, name, false, [x, 0, z]) {
                    offset += 1;
                }
            }
        }
        offset
    }

    /// `PositionSampler.placeOther`: the rest of the origin biome's vegetal features.
    #[allow(clippy::too_many_arguments)]
    fn other(
        &mut self,
        world: &mut World<'_>,
        random: &mut Random,
        seed: i64,
        mut offset: usize,
        origin: [i32; 3],
        biome: BiomeId,
        tf: &TerraForgedDecoration<'_>,
    ) -> Result<(), GeneratorError> {
        let (_, features) = self.vegetation(biome, tf);
        for (slot, name) in features.other.iter().enumerate() {
            let index = self.seed_index(offset, OTHER + slot as i32);
            random.set_feature_seed(seed, index, VEGETAL as i32);
            if self.place(world, random, name, false, origin) {
                offset += 1;
            }
        }
        Ok(())
    }
}

/// `ViabilityContext`: the chunk's terrain data, seed 0 and no biome edges (the original
/// never sets them).
struct TerrainViability<'a>(&'a TerrainData);

impl ViabilityContext for TerrainViability<'_> {
    fn seed(&self) -> i32 {
        0
    }
    fn edge(&self) -> bool {
        false
    }
    fn max_y(&self) -> i32 {
        self.0.levels().max_y
    }
    fn base_height(&self, x: i32, z: i32) -> i32 {
        self.0.base_height_at(x, z)
    }
    fn height(&self, x: i32, z: i32) -> i32 {
        self.0.height_at(x, z)
    }
    fn normalized_gradient(&self, x: i32, z: i32, norm: f32) -> f32 {
        self.0.normalized_gradient(x, z, norm)
    }
    fn river(&self, x: i32, z: i32) -> f32 {
        self.0.river().get(x, z)
    }
    fn climate_shape(&self, _seed: i32, _x: i32, _z: i32) -> f32 {
        0.0
    }
}
