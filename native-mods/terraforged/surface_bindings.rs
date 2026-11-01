use super::{
    super::terrain::{TerrainBlockStates, TerrainChunk, TerrainError},
    terraforged_surface::{self, SnowBlocks},
    terraforged_terrain_data::TerrainData,
};
use crate::registry::{BlockRegistry, BlockStateId};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub struct SurfaceBindings<'r> {
    registry: &'r BlockRegistry,
    dirt: Arc<BTreeSet<BlockStateId>>,
    snow: Arc<BTreeSet<BlockStateId>>,
    snow_layers: Arc<BTreeSet<BlockStateId>>,
    air: [BlockStateId; 3],
    stone: BlockStateId,
    source_water: BlockStateId,
    flowing_water: BlockStateId,
}

impl<'r> SurfaceBindings<'r> {
    pub fn new(
        registry: &'r BlockRegistry,
        tags: &BTreeMap<String, Arc<BTreeSet<BlockStateId>>>,
        snow_layers: Arc<BTreeSet<BlockStateId>>,
    ) -> Result<Self, TerrainError> {
        let default = |name| {
            registry
                .default_state(name)
                .ok_or(TerrainError::InvalidState)
        };
        let water = default("minecraft:water")?;
        Ok(Self {
            registry,
            dirt: tags.get("minecraft:dirt").cloned().unwrap_or_default(),
            snow: tags.get("minecraft:snow").cloned().unwrap_or_default(),
            snow_layers,
            air: [
                default("minecraft:air")?,
                default("minecraft:cave_air")?,
                default("minecraft:void_air")?,
            ],
            stone: default("minecraft:stone")?,
            source_water: with_property(registry, water, "level", "0")?,
            flowing_water: with_property(registry, water, "level", "2")?,
        })
    }

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

    pub fn apply_post<B: TerrainBlockStates>(
        &self,
        terrain: &TerrainData,
        chunk: &mut TerrainChunk<'_, B>,
        gen_depth: i32,
    ) -> Result<(), TerrainError> {
        terraforged_surface::apply_post(terrain, chunk, gen_depth, self)
    }

    pub fn smooth_water<B: TerrainBlockStates>(
        &self,
        terrain: &TerrainData,
        chunk: &mut TerrainChunk<'_, B>,
        region: impl FnMut(i32, i32, i32) -> Result<BlockStateId, TerrainError>,
    ) -> Result<(), TerrainError> {
        terraforged_surface::smooth_water(
            terrain,
            chunk,
            self.source_water,
            self.flowing_water,
            |state| self.is_air(state),
            region,
        )
    }
}

impl SnowBlocks for SurfaceBindings<'_> {
    fn is_air(&self, state: BlockStateId) -> bool {
        self.air.contains(&state)
    }
    fn is_snow_layer(&self, state: BlockStateId) -> bool {
        self.snow_layers.contains(&state)
    }
    fn is_snow(&self, state: BlockStateId) -> bool {
        self.snow.contains(&state)
    }
    fn is_erodible(&self, state: BlockStateId) -> bool {
        self.dirt.contains(&state) || self.is_snow(state)
    }
    fn with_layers(&self, state: BlockStateId, layers: i32) -> Result<BlockStateId, TerrainError> {
        if !self.is_snow_layer(state) || !(1..=8).contains(&layers) {
            return Err(TerrainError::InvalidState);
        }
        with_property(self.registry, state, "layers", &layers.to_string())
    }
    fn air(&self) -> BlockStateId {
        self.air[0]
    }
    fn stone(&self) -> BlockStateId {
        self.stone
    }
}

fn with_property(
    registry: &BlockRegistry,
    state: BlockStateId,
    property: &str,
    value: &str,
) -> Result<BlockStateId, TerrainError> {
    let mut descriptor = registry
        .state(state)
        .ok_or(TerrainError::InvalidState)?
        .clone();
    if !descriptor.properties.contains_key(property) {
        return Err(TerrainError::InvalidState);
    }
    descriptor.properties.insert(property.into(), value.into());
    registry.id(&descriptor).ok_or(TerrainError::InvalidState)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DATA_VERSION, nbt::Tag};

    fn registry(flow_level: &str) -> BlockRegistry {
        let state = |name: &str, properties: &[(&str, &str)]| {
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
        };
        BlockRegistry::from_nbt(&Tag::compound([
            ("DataVersion", Tag::Int(DATA_VERSION)),
            (
                "states",
                Tag::List(vec![
                    state("minecraft:water", &[("level", flow_level)]),
                    state("test:snow", &[("layers", "1"), ("tint", "blue")]),
                    state("minecraft:cave_air", &[]),
                    state("minecraft:stone", &[]),
                    state("test:snow", &[("layers", "4"), ("tint", "blue")]),
                    state("test:snow", &[("layers", "4"), ("tint", "red")]),
                    state("minecraft:air", &[]),
                    state("minecraft:void_air", &[]),
                    state("minecraft:water", &[("level", "0")]),
                    state("minecraft:dirt", &[]),
                    state("test:tagged_snow", &[]),
                    state("test:snow", &[("layers", "1"), ("tint", "red")]),
                ]),
            ),
            (
                "defaults",
                Tag::compound([
                    ("minecraft:water", Tag::Int(8)),
                    ("test:snow", Tag::Int(1)),
                    ("minecraft:cave_air", Tag::Int(2)),
                    ("minecraft:stone", Tag::Int(3)),
                    ("minecraft:air", Tag::Int(6)),
                    ("minecraft:void_air", Tag::Int(7)),
                    ("minecraft:dirt", Tag::Int(9)),
                    ("test:tagged_snow", Tag::Int(10)),
                ]),
            ),
        ]))
        .unwrap()
    }

    #[test]
    fn registry_binding_preserves_properties_and_separates_classes_from_tags() {
        let registry = registry("2");
        let tags = BTreeMap::from([
            (
                "minecraft:dirt".into(),
                Arc::new(BTreeSet::from([BlockStateId(9)])),
            ),
            (
                "minecraft:snow".into(),
                Arc::new(BTreeSet::from([BlockStateId(10)])),
            ),
        ]);
        let bindings = SurfaceBindings::new(
            &registry,
            &tags,
            Arc::new([1, 4, 5, 11].map(BlockStateId).into_iter().collect()),
        )
        .unwrap();
        assert_eq!(
            (bindings.source_water, bindings.flowing_water),
            (BlockStateId(8), BlockStateId(0))
        );
        for state in [2, 6, 7] {
            assert!(bindings.is_air(BlockStateId(state)));
        }
        assert!(!bindings.is_air(BlockStateId(0)));
        assert!(bindings.is_snow_layer(BlockStateId(1)));
        assert!(!bindings.is_snow(BlockStateId(1)));
        assert!(bindings.is_snow(BlockStateId(10)));
        assert!(!bindings.is_snow_layer(BlockStateId(10)));
        assert!(bindings.is_erodible(BlockStateId(9)));
        assert!(bindings.is_erodible(BlockStateId(10)));
        assert_eq!(
            bindings.with_layers(BlockStateId(1), 4).unwrap(),
            BlockStateId(4)
        );
        assert_eq!(
            bindings.with_layers(BlockStateId(11), 4).unwrap(),
            BlockStateId(5)
        );
        for (state, layers) in [(1, 0), (1, 9), (1, 8), (3, 4)] {
            assert!(matches!(
                bindings.with_layers(BlockStateId(state), layers),
                Err(TerrainError::InvalidState)
            ));
        }
        assert_eq!(
            (bindings.air(), bindings.stone()),
            (BlockStateId(6), BlockStateId(3))
        );
    }

    #[test]
    fn missing_water_level_is_not_silently_replaced_by_default() {
        let registry = registry("1");
        assert!(matches!(
            SurfaceBindings::new(&registry, &BTreeMap::new(), Arc::default()),
            Err(TerrainError::InvalidState)
        ));
    }
}
