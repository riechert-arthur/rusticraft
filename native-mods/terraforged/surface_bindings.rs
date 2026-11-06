use super::{
    super::terrain::{TerrainBlockStates, TerrainChunk, TerrainError},
    terraforged_surface,
    terraforged_terrain_data::TerrainData,
};
use crate::registry::{BlockRegistry, BlockStateId};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// The block tags whose states the surface passes erode by default: TerraForged Reimagined a24d450's
/// `Surface.isErodible` (MIT, (c) 2020 TerraForged, (c) 2026 Romoslayer), `#minecraft:substrate_overworld` or
/// `#minecraft:snow`. This deliberately departs from TerraForged 6dd607eb (and the 1.18.2-0.3.1 jar), whose
/// `isErodible` reads `#minecraft:dirt` or `#minecraft:snow`: 26.x shrank `#minecraft:dirt` to dirt, coarse dirt and
/// rooted dirt and moved grass, podzol, mycelium, moss and mud into `#minecraft:substrate_overworld`, so the literal
/// tag would leave grassy slopes uneroded. In 26.3, `substrate_overworld` is dirt, coarse dirt, rooted dirt, mud,
/// muddy mangrove roots, moss block, pale moss block, grass block, podzol and mycelium.
pub const ERODIBLE_TAGS: [&str; 2] = ["minecraft:substrate_overworld", "minecraft:snow"];

/// 6dd607eb's literal `isErodible` tags (`#minecraft:dirt` or `#minecraft:snow`), for parity checks against its Java;
/// with 26.3's tags this is only dirt, coarse dirt, rooted dirt, snow, snow block and powder snow.
pub const JAVA_ERODIBLE_TAGS: [&str; 2] = ["minecraft:dirt", "minecraft:snow"];

/// The tag `applyPost` tests for snow to erode (`state.is(BlockTags.SNOW)`), in every source tree.
pub const SNOW_TAG: &str = "minecraft:snow";

/// A registry state or block tag the surface passes need but the world's data doesn't have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceBindingError {
    /// No default state for this block.
    MissingBlock(&'static str),
    /// The block's default state has no state with this property value (`setValue` would throw).
    MissingState(&'static str, &'static str, &'static str),
    /// No block tag of this name.
    MissingTag(String),
}

impl std::fmt::Display for SurfaceBindingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingBlock(block) => write!(f, "TerraForged surface: no block {block}"),
            Self::MissingState(block, property, value) => {
                write!(
                    f,
                    "TerraForged surface: no state {block}[{property}={value}]"
                )
            }
            Self::MissingTag(tag) => write!(f, "TerraForged surface: no block tag #{tag}"),
        }
    }
}

impl std::error::Error for SurfaceBindingError {}

/// The registry states and tag sets the surface passes (`surface.rs`) take from the caller, resolved once per world
/// from the 26.3 block registry and its bound block tags (`FeatureBlockBindings::bind_tags`'s second map, keyed by
/// tag name with nested tags flattened). Air, stone and the snow layer's `layers` values are not here: the passes
/// read them from the chunk's own registry.
pub struct SurfaceBindings {
    /// Membership of the erodible tags' union, indexed by state id.
    erodible: Box<[bool]>,
    snow: Arc<BTreeSet<BlockStateId>>,
    source_water: BlockStateId,
    level_two_water: BlockStateId,
}

impl SurfaceBindings {
    /// The bindings with the default erodible tags, [`ERODIBLE_TAGS`] (TerraForged Reimagined's choice).
    pub fn new(
        registry: &BlockRegistry,
        tags: &BTreeMap<String, Arc<BTreeSet<BlockStateId>>>,
    ) -> Result<Self, SurfaceBindingError> {
        Self::with_erodible_tags(registry, tags, &ERODIBLE_TAGS)
    }

    /// The bindings with `erodible_tags` as `isErodible`: a state erodes when any of these tags holds it (pass
    /// [`JAVA_ERODIBLE_TAGS`] for 6dd607eb's literal tags). Every tag, `#minecraft:snow` and the water states
    /// `smoothWater` uses must exist, or this is an `Err` (Java would throw when the pass first reads them).
    pub fn with_erodible_tags(
        registry: &BlockRegistry,
        tags: &BTreeMap<String, Arc<BTreeSet<BlockStateId>>>,
        erodible_tags: &[&str],
    ) -> Result<Self, SurfaceBindingError> {
        let tag = |name: &str| {
            tags.get(name)
                .ok_or_else(|| SurfaceBindingError::MissingTag(name.to_owned()))
        };
        let mut erodible = vec![false; registry.len()].into_boxed_slice();
        for name in erodible_tags {
            for state in tag(name)?.iter() {
                if let Some(slot) = erodible.get_mut(state.0 as usize) {
                    *slot = true;
                }
            }
        }
        const WATER: &str = "minecraft:water";
        let water = registry
            .default_state(WATER)
            .ok_or(SurfaceBindingError::MissingBlock(WATER))?;
        Ok(Self {
            erodible,
            snow: tag(SNOW_TAG)?.clone(),
            // `state.is(Blocks.WATER) && state.getValue(LiquidBlock.LEVEL) == 0`: level is water's only property.
            source_water: with_property(registry, water, WATER, "level", "0")?,
            // `Blocks.WATER.defaultBlockState().setValue(LiquidBlock.LEVEL, 2)`.
            level_two_water: with_property(registry, water, WATER, "level", "2")?,
        })
    }

    /// Whether `state` is in one of the erodible tags (Java's `isErodible`).
    pub fn is_erodible(&self, state: BlockStateId) -> bool {
        self.erodible
            .get(state.0 as usize)
            .copied()
            .unwrap_or(false)
    }

    /// Whether `state` is in `#minecraft:snow`.
    pub fn is_snow(&self, state: BlockStateId) -> bool {
        self.snow.contains(&state)
    }

    /// `minecraft:water[level=0]` and `minecraft:water[level=2]`, as [`terraforged_surface::smooth_water`] takes them.
    pub fn water(&self) -> (BlockStateId, BlockStateId) {
        (self.source_water, self.level_two_water)
    }

    /// [`terraforged_surface::apply`] with [`Self::is_erodible`].
    pub fn apply<B: TerrainBlockStates>(
        &self,
        terrain: &TerrainData,
        chunk: &mut TerrainChunk<'_, B>,
        gen_depth: i32,
        sea_level: i32,
    ) -> Result<(), TerrainError> {
        terraforged_surface::apply(terrain, chunk, gen_depth, sea_level, |state| {
            self.is_erodible(state)
        })
    }

    /// [`terraforged_surface::smooth_water`] with the bound water states.
    pub fn smooth_water<B: TerrainBlockStates>(
        &self,
        terrain: &TerrainData,
        chunk: &mut TerrainChunk<'_, B>,
        region_block: impl FnMut(i32, i32, i32) -> Result<BlockStateId, TerrainError>,
    ) -> Result<(), TerrainError> {
        terraforged_surface::smooth_water(
            terrain,
            chunk,
            self.source_water,
            self.level_two_water,
            region_block,
        )
    }

    /// [`terraforged_surface::apply_post`] with [`Self::is_snow`] and [`Self::is_erodible`].
    pub fn apply_post<B: TerrainBlockStates>(
        &self,
        terrain: &TerrainData,
        chunk: &mut TerrainChunk<'_, B>,
        gen_depth: i32,
    ) -> Result<(), TerrainError> {
        terraforged_surface::apply_post(
            terrain,
            chunk,
            gen_depth,
            |state| self.is_snow(state),
            |state| self.is_erodible(state),
        )
    }
}

/// `state.setValue(property, value)`: the registry's state with every other property kept.
fn with_property(
    registry: &BlockRegistry,
    state: BlockStateId,
    block: &'static str,
    property: &'static str,
    value: &'static str,
) -> Result<BlockStateId, SurfaceBindingError> {
    let missing = SurfaceBindingError::MissingState(block, property, value);
    let mut descriptor = registry.state(state).ok_or(missing.clone())?.clone();
    match descriptor.properties.get_mut(property) {
        Some(slot) => *slot = value.to_owned(),
        None => return Err(missing),
    }
    registry.id(&descriptor).ok_or(missing)
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged_engine::TerrainRegistry,
        terraforged_terrain_data::{NoiseData, NoiseSample},
        terraforged_terrain_levels::TerrainLevels,
    };
    use super::*;
    use crate::{DATA_VERSION, heightmap::HeightmapState, nbt::Tag};
    use horizon_core::position::ChunkPos;

    fn state(name: &str, properties: &[(&str, &str)]) -> Tag {
        Tag::compound([
            ("id", Tag::String(name.into())),
            (
                "properties",
                Tag::compound(
                    properties
                        .iter()
                        .map(|(key, value)| (*key, Tag::String((*value).into()))),
                ),
            ),
        ])
    }

    /// Heightmap facts of [`registry`]'s states: `air` is the air id (void air follows it), the ids from 4 below it
    /// are water.
    #[derive(Clone, Copy)]
    struct State {
        id: u32,
        air: u32,
    }

    impl HeightmapState for State {
        fn is_air(self) -> bool {
            self.id == self.air || self.id == self.air + 1
        }

        fn is_air_block(self) -> bool {
            self.id == self.air
        }

        fn has_fluid(self) -> bool {
            (4..self.air).contains(&self.id)
        }

        fn blocks_motion_in_heightmap(self) -> bool {
            !self.is_air() && !self.has_fluid()
        }

        fn blocks_motion_in_heightmap_no_leaves(self) -> bool {
            self.blocks_motion_in_heightmap()
        }
    }

    struct Properties {
        air: u32,
    }

    impl TerrainBlockStates for Properties {
        type State = State;

        fn heightmap_state(&self, id: BlockStateId) -> State {
            State {
                id: id.0,
                air: self.air,
            }
        }
    }

    /// Water's default is level 0, as in 26.3, but stored after other states so ids can't line up by accident.
    fn registry(levels: &[&str]) -> BlockRegistry {
        let mut states = vec![
            state("minecraft:dirt", &[]),
            state("minecraft:grass_block", &[("snowy", "false")]),
            state("minecraft:snow_block", &[]),
            state("minecraft:stone", &[]),
        ];
        states.extend(
            levels
                .iter()
                .map(|level| state("minecraft:water", &[("level", level)])),
        );
        states.push(state("minecraft:air", &[]));
        states.push(state("minecraft:void_air", &[]));
        BlockRegistry::from_nbt(&Tag::compound([
            ("DataVersion", Tag::Int(DATA_VERSION)),
            ("states", Tag::List(states)),
            (
                "defaults",
                Tag::compound([
                    ("minecraft:dirt", Tag::Int(0)),
                    ("minecraft:grass_block", Tag::Int(1)),
                    ("minecraft:snow_block", Tag::Int(2)),
                    ("minecraft:stone", Tag::Int(3)),
                    ("minecraft:water", Tag::Int(4)),
                    ("minecraft:air", Tag::Int(4 + levels.len() as i32)),
                    ("minecraft:void_air", Tag::Int(5 + levels.len() as i32)),
                ]),
            ),
        ]))
        .unwrap()
    }

    /// 26.3's shapes of the three tags over the fixture: dirt only, dirt + grass, snow block.
    fn tags() -> BTreeMap<String, Arc<BTreeSet<BlockStateId>>> {
        let set = |ids: &[u32]| Arc::new(ids.iter().copied().map(BlockStateId).collect());
        BTreeMap::from([
            ("minecraft:dirt".to_owned(), set(&[0])),
            ("minecraft:substrate_overworld".to_owned(), set(&[0, 1])),
            ("minecraft:snow".to_owned(), set(&[2])),
        ])
    }

    #[test]
    fn default_erodible_tags_follow_reimagined_and_java_tags_stay_available() {
        let registry = registry(&["0", "1", "2"]);
        let reimagined = SurfaceBindings::new(&registry, &tags()).unwrap();
        let java =
            SurfaceBindings::with_erodible_tags(&registry, &tags(), &JAVA_ERODIBLE_TAGS).unwrap();
        let erodible = |b: &SurfaceBindings| {
            (0..8)
                .map(|i| b.is_erodible(BlockStateId(i)))
                .collect::<Vec<_>>()
        };
        // Grass erodes only with substrate_overworld; snow erodes with both; stone, water and unknown ids never.
        assert_eq!(
            erodible(&reimagined),
            [true, true, true, false, false, false, false, false]
        );
        assert_eq!(
            erodible(&java),
            [true, false, true, false, false, false, false, false]
        );
        for bindings in [&reimagined, &java] {
            assert!(bindings.is_snow(BlockStateId(2)));
            assert!(!bindings.is_snow(BlockStateId(0)));
            assert_eq!(bindings.water(), (BlockStateId(4), BlockStateId(6)));
        }
        let none = SurfaceBindings::with_erodible_tags(&registry, &tags(), &[]).unwrap();
        assert!(!none.is_erodible(BlockStateId(2)));
    }

    #[test]
    fn missing_states_and_tags_are_errors() {
        assert_eq!(
            SurfaceBindings::new(&registry(&["0", "1"]), &tags()).err(),
            Some(SurfaceBindingError::MissingState(
                "minecraft:water",
                "level",
                "2"
            ))
        );
        let no_water = BlockRegistry::from_nbt(&Tag::compound([
            ("DataVersion", Tag::Int(DATA_VERSION)),
            ("states", Tag::List(vec![state("minecraft:stone", &[])])),
            (
                "defaults",
                Tag::compound([("minecraft:stone", Tag::Int(0))]),
            ),
        ]))
        .unwrap();
        assert_eq!(
            SurfaceBindings::new(&no_water, &tags()).err(),
            Some(SurfaceBindingError::MissingBlock("minecraft:water"))
        );
        let registry = registry(&["0", "2"]);
        for missing in ["minecraft:substrate_overworld", "minecraft:snow"] {
            let mut tags = tags();
            tags.remove(missing);
            assert_eq!(
                SurfaceBindings::new(&registry, &tags).err(),
                Some(SurfaceBindingError::MissingTag(missing.to_owned()))
            );
        }
        let mut tags = tags();
        tags.remove("minecraft:dirt");
        assert!(SurfaceBindings::new(&registry, &tags).is_ok());
        assert_eq!(
            SurfaceBindings::with_erodible_tags(&registry, &tags, &JAVA_ERODIBLE_TAGS).err(),
            Some(SurfaceBindingError::MissingTag("minecraft:dirt".to_owned()))
        );
    }

    /// A steep (ramped) chunk of grass over stone: the default tags erode the grass to stone through
    /// `surface::apply`, the literal 6dd607eb tags leave it, as `#minecraft:dirt` no longer holds grass in 26.3.
    #[test]
    fn bound_erosion_strips_grass_only_with_the_default_tags() {
        let registry = registry(&["0", "2"]);
        let air = 6;
        let terrains = TerrainRegistry::new();
        let mut noise = NoiseData::new(&terrains);
        for z in NoiseData::MIN..NoiseData::MAX {
            for x in NoiseData::MIN..NoiseData::MAX {
                let mut sample = NoiseSample::new(&terrains);
                sample.height_noise = 0.1 + 0.04 * x as f32;
                noise.set_noise(x, z, &sample);
            }
        }
        let mut terrain = TerrainData::new(TerrainLevels::default());
        terrain.accept(&noise);
        let run = |bindings: &SurfaceBindings| {
            let mut chunk = TerrainChunk::new(
                ChunkPos::new(-3, 2),
                -64,
                192,
                &registry,
                Properties { air },
            )
            .unwrap();
            for z in 0..16 {
                for x in 0..16 {
                    for y in -64..64 {
                        let id = if y < 60 { 3 } else { 1 };
                        chunk.set_block_raw(x, y, z, BlockStateId(id)).unwrap();
                    }
                }
            }
            bindings.apply(&terrain, &mut chunk, 255, 40).unwrap();
            (0..16)
                .flat_map(|z| (0..16).map(move |x| (x, z)))
                .map(|(x, z)| {
                    (60..64)
                        .filter(|&y| chunk.block(x, y, z) == BlockStateId(3))
                        .count()
                })
                .sum::<usize>()
        };
        for z in 0..16 {
            for x in 0..16 {
                assert!(terrain.normalized_gradient(x, z, 55.0) >= 0.6);
            }
        }
        let java =
            SurfaceBindings::with_erodible_tags(&registry, &tags(), &JAVA_ERODIBLE_TAGS).unwrap();
        assert_eq!(
            run(&SurfaceBindings::new(&registry, &tags()).unwrap()),
            16 * 16 * 4
        );
        assert_eq!(run(&java), 0);
    }

    /// The bindings over Horizon's real 26.3 registry and block tags (`GeneratorData`, bound as the feature bindings
    /// bind them); skipped without the prepared resources.
    #[test]
    fn real_26_3_tags_bind_reimagined_and_java_erodible_sets() {
        let root = std::env::var_os("HORIZON_TEST_RESOURCES")
            .or_else(|| std::env::var_os("HORIZON_RESOURCES"))
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../.local/horizon-client/26.3")
            });
        if !root.join("worldgen/world_presets.json").exists() {
            eprintln!("skipping: no worldgen bundle at {}", root.display());
            return;
        }
        let registry: &'static BlockRegistry = Box::leak(Box::new(
            crate::level::data::load_registry(&root).expect("registry"),
        ));
        let data = super::super::GeneratorData::load(&root.join("worldgen"), registry)
            .expect("worldgen data");
        let (_, tags) = super::super::super::block_feature::FeatureBlockBindings::bind_tags(
            registry,
            &data.block_tags,
        )
        .unwrap();
        let reimagined = SurfaceBindings::new(registry, &tags).unwrap();
        let java =
            SurfaceBindings::with_erodible_tags(registry, &tags, &JAVA_ERODIBLE_TAGS).unwrap();
        let default = |name| registry.default_state(name).unwrap();
        for (name, erodes, erodes_in_java) in [
            ("minecraft:dirt", true, true),
            ("minecraft:coarse_dirt", true, true),
            ("minecraft:rooted_dirt", true, true),
            ("minecraft:grass_block", true, false),
            ("minecraft:podzol", true, false),
            ("minecraft:mycelium", true, false),
            ("minecraft:moss_block", true, false),
            ("minecraft:pale_moss_block", true, false),
            ("minecraft:mud", true, false),
            ("minecraft:muddy_mangrove_roots", true, false),
            ("minecraft:snow", true, true),
            ("minecraft:snow_block", true, true),
            ("minecraft:powder_snow", true, true),
            ("minecraft:stone", false, false),
            ("minecraft:sand", false, false),
            ("minecraft:gravel", false, false),
            ("minecraft:farmland", false, false),
            ("minecraft:dirt_path", false, false),
        ] {
            let state = default(name);
            assert_eq!(reimagined.is_erodible(state), erodes, "{name}");
            assert_eq!(java.is_erodible(state), erodes_in_java, "{name}");
        }
        // Every state of a tagged block, not only its default (snowy grass, eight snow layers).
        let states = |name: &'static str| {
            (0..registry.len() as u32)
                .map(BlockStateId)
                .filter(move |&id| registry.state(id).unwrap().name == name)
        };
        assert_eq!(
            states("minecraft:grass_block")
                .filter(|&s| reimagined.is_erodible(s))
                .count(),
            2
        );
        assert_eq!(
            states("minecraft:snow")
                .filter(|&s| reimagined.is_snow(s))
                .count(),
            8
        );
        let (source, level_two) = reimagined.water();
        assert_eq!(source, default("minecraft:water"));
        let level = |id| registry.state(id).unwrap().properties.get("level").cloned();
        assert_eq!(
            (level(source), level(level_two)),
            (Some("0".into()), Some("2".into()))
        );
        assert_eq!(registry.state(level_two).unwrap().name, "minecraft:water");
    }
}
