/*
 * MIT License
 *
 * Copyright (c) 2021 TerraForged
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

use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Seed {
    root: i32,
    value: i32,
}

impl Seed {
    pub const fn new(value: i32) -> Self {
        Self { root: value, value }
    }

    pub const fn from_long(value: i64) -> Self {
        Self::new(value as i32)
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> i32 {
        let value = self.value;
        self.value = self.value.wrapping_add(1);
        value
    }

    pub const fn get(self) -> i32 {
        self.value
    }

    pub const fn root(self) -> i32 {
        self.root
    }

    pub const fn split(self) -> Self {
        Self::new(self.root)
    }

    pub const fn offset(self, offset: i32) -> Self {
        Self::new(self.root.wrapping_add(offset))
    }
}

pub trait TerrainProperties {
    fn erosion_modifier(&self) -> f32 {
        1.0
    }

    fn is_flat(&self) -> bool {
        false
    }

    fn is_river(&self) -> bool {
        false
    }

    fn is_shallow_ocean(&self) -> bool {
        false
    }

    fn is_deep_ocean(&self) -> bool {
        false
    }

    fn is_coast(&self) -> bool {
        false
    }

    fn is_submerged(&self) -> bool {
        self.is_deep_ocean() || self.is_shallow_ocean() || self.is_river() || self.is_lake()
    }

    fn is_overground(&self) -> bool {
        false
    }

    fn overrides_river(&self) -> bool {
        self.is_deep_ocean() || self.is_shallow_ocean() || self.is_coast()
    }

    fn overrides_coast(&self) -> bool {
        self.is_volcano()
    }

    fn is_lake(&self) -> bool {
        false
    }

    fn is_wetland(&self) -> bool {
        false
    }

    fn is_mountain(&self) -> bool {
        false
    }

    fn is_volcano(&self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum TerrainCategory {
    None,
    DeepOcean,
    ShallowOcean,
    Coast,
    Beach,
    River,
    Lake,
    Wetland,
    Flatland,
    Lowland,
    Highland,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum BiomeType {
    TropicalRainforest,
    Savanna,
    Desert,
    TemperateRainforest,
    TemperateForest,
    Grassland,
    ColdSteppe,
    Steppe,
    Taiga,
    Tundra,
    Alpine,
}

impl BiomeType {
    pub const RESOLUTION: i32 = 256;
    pub const MAX: i32 = 255;
    pub const VALUES: [Self; 11] = [
        Self::TropicalRainforest,
        Self::Savanna,
        Self::Desert,
        Self::TemperateRainforest,
        Self::TemperateForest,
        Self::Grassland,
        Self::ColdSteppe,
        Self::Steppe,
        Self::Taiga,
        Self::Tundra,
        Self::Alpine,
    ];

    pub const fn id(self) -> i32 {
        self as i32
    }

    pub fn get(id: i32) -> Self {
        Self::VALUES[id as usize]
    }

    pub const fn is_extreme(self) -> bool {
        matches!(self, Self::Tundra | Self::Desert)
    }
}

impl TerrainCategory {
    pub const VALUES: [Self; 11] = [
        Self::None,
        Self::DeepOcean,
        Self::ShallowOcean,
        Self::Coast,
        Self::Beach,
        Self::River,
        Self::Lake,
        Self::Wetland,
        Self::Flatland,
        Self::Lowland,
        Self::Highland,
    ];

    pub const fn get_dominant(self, other: Self) -> Self {
        if self as u8 > other as u8 {
            self
        } else {
            other
        }
    }
}

impl TerrainProperties for TerrainCategory {
    fn is_flat(&self) -> bool {
        matches!(self, Self::Flatland)
    }

    fn is_river(&self) -> bool {
        matches!(self, Self::River)
    }

    fn is_shallow_ocean(&self) -> bool {
        matches!(self, Self::ShallowOcean)
    }

    fn is_deep_ocean(&self) -> bool {
        matches!(self, Self::DeepOcean)
    }

    fn is_coast(&self) -> bool {
        matches!(self, Self::Coast | Self::Beach)
    }

    fn is_submerged(&self) -> bool {
        matches!(
            self,
            Self::DeepOcean | Self::ShallowOcean | Self::River | Self::Lake
        )
    }

    fn is_overground(&self) -> bool {
        matches!(
            self,
            Self::Coast
                | Self::Beach
                | Self::Wetland
                | Self::Flatland
                | Self::Lowland
                | Self::Highland
        )
    }

    fn overrides_river(&self) -> bool {
        matches!(
            self,
            Self::DeepOcean | Self::ShallowOcean | Self::Coast | Self::Beach
        )
    }

    fn is_lake(&self) -> bool {
        matches!(self, Self::Lake)
    }

    fn is_wetland(&self) -> bool {
        matches!(self, Self::Wetland)
    }
}

#[derive(Clone)]
enum TerrainDelegate {
    Category(TerrainCategory),
    Terrain(Terrain),
}

impl TerrainDelegate {
    fn properties(&self) -> &dyn TerrainProperties {
        match self {
            Self::Category(category) => category,
            Self::Terrain(terrain) => terrain,
        }
    }
}

struct TerrainData {
    id: i32,
    name: Arc<str>,
    category: TerrainCategory,
    delegate: TerrainDelegate,
    erosion: Option<f32>,
    flat: Option<bool>,
    mountain: Option<bool>,
    river_override: Option<bool>,
    coast_override: Option<bool>,
    volcano: Option<bool>,
}

#[derive(Clone)]
pub struct Terrain(Arc<TerrainData>);

impl Terrain {
    pub fn new(id: i32, name: impl Into<Arc<str>>, category: TerrainCategory) -> Self {
        Self::build(id, name, category, TerrainDelegate::Category(category))
    }

    pub fn with_parent(id: i32, name: impl Into<Arc<str>>, parent: Self) -> Self {
        Self::build(
            id,
            name,
            parent.category(),
            TerrainDelegate::Terrain(parent),
        )
    }

    fn build(
        id: i32,
        name: impl Into<Arc<str>>,
        category: TerrainCategory,
        delegate: TerrainDelegate,
    ) -> Self {
        Self(Arc::new(TerrainData {
            id,
            name: name.into(),
            category,
            delegate,
            erosion: None,
            flat: None,
            mountain: None,
            river_override: None,
            coast_override: None,
            volcano: None,
        }))
    }

    pub fn configured_erosion(
        id: i32,
        name: impl Into<Arc<str>>,
        category: TerrainCategory,
        erosion: f32,
    ) -> Self {
        Self::configured(
            id,
            name,
            category,
            erosion,
            category.is_mountain(),
            category.overrides_river(),
        )
    }

    pub fn configured_river_override(
        id: i32,
        name: impl Into<Arc<str>>,
        category: TerrainCategory,
        overrides_river: bool,
    ) -> Self {
        Self::configured(
            id,
            name,
            category,
            category.erosion_modifier(),
            category.is_mountain(),
            overrides_river,
        )
    }

    pub fn configured_mountain(
        id: i32,
        name: impl Into<Arc<str>>,
        category: TerrainCategory,
        is_mountain: bool,
        overrides_river: bool,
    ) -> Self {
        Self::configured(
            id,
            name,
            category,
            category.erosion_modifier(),
            is_mountain,
            overrides_river,
        )
    }

    pub fn configured(
        id: i32,
        name: impl Into<Arc<str>>,
        category: TerrainCategory,
        erosion: f32,
        is_mountain: bool,
        overrides_river: bool,
    ) -> Self {
        let mut terrain = Self::new(id, name, category);
        let data = Arc::get_mut(&mut terrain.0).unwrap();
        data.erosion = Some(erosion);
        data.mountain = Some(is_mountain);
        data.river_override = Some(overrides_river);
        terrain
    }

    pub fn volcano(id: i32, name: impl Into<Arc<str>>, category: TerrainCategory) -> Self {
        let mut terrain = Self::configured_mountain(id, name, category, true, true);
        let data = Arc::get_mut(&mut terrain.0).unwrap();
        data.coast_override = Some(true);
        data.volcano = Some(true);
        terrain
    }

    pub fn composite(id: i32, first: Self, second: Self) -> Self {
        let name: Arc<str> = format!("{}-{}", first.name(), second.name()).into();
        let category = first.category();
        let flat = first.is_flat() && second.is_flat();
        let erosion = java_min(first.erosion_modifier(), second.erosion_modifier());
        let mut terrain = Self::with_parent(id, name, first);
        let data = Arc::get_mut(&mut terrain.0).unwrap();
        data.category = category;
        data.flat = Some(flat);
        data.erosion = Some(erosion);
        terrain
    }

    pub fn id(&self) -> i32 {
        self.0.id
    }

    pub fn name(&self) -> &str {
        &self.0.name
    }

    pub fn category(&self) -> TerrainCategory {
        self.0.category
    }

    pub fn render_hue(&self) -> f32 {
        let hash = self.name().encode_utf16().fold(0_i32, |hash, unit| {
            hash.wrapping_mul(31).wrapping_add(i32::from(unit))
        });
        hash.wrapping_mul(hash)
            .wrapping_mul(hash)
            .wrapping_mul(60_493) as f32
            / 2_147_483_648.0_f32
    }

    pub fn with_id(&self, id: i32) -> Self {
        let delegate = match &self.0.delegate {
            TerrainDelegate::Terrain(terrain) => TerrainDelegate::Terrain(terrain.clone()),
            TerrainDelegate::Category(_) => TerrainDelegate::Terrain(self.clone()),
        };
        Self::build(id, self.0.name.clone(), self.category(), delegate)
    }

    pub fn is_same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for Terrain {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Terrain")
            .field("id", &self.id())
            .field("name", &self.name())
            .field("category", &self.category())
            .finish()
    }
}

impl TerrainProperties for Terrain {
    fn erosion_modifier(&self) -> f32 {
        self.0
            .erosion
            .unwrap_or_else(|| self.0.delegate.properties().erosion_modifier())
    }

    fn is_flat(&self) -> bool {
        self.0
            .flat
            .unwrap_or_else(|| self.0.delegate.properties().is_flat())
    }

    fn is_river(&self) -> bool {
        self.0.delegate.properties().is_river()
    }

    fn is_shallow_ocean(&self) -> bool {
        self.0.delegate.properties().is_shallow_ocean()
    }

    fn is_deep_ocean(&self) -> bool {
        self.0.delegate.properties().is_deep_ocean()
    }

    fn is_coast(&self) -> bool {
        self.0.delegate.properties().is_coast()
    }

    fn is_submerged(&self) -> bool {
        self.0.delegate.properties().is_submerged()
    }

    fn is_overground(&self) -> bool {
        self.0.delegate.properties().is_overground()
    }

    fn overrides_river(&self) -> bool {
        self.0
            .river_override
            .unwrap_or_else(|| self.0.delegate.properties().overrides_river())
    }

    fn overrides_coast(&self) -> bool {
        self.0
            .coast_override
            .unwrap_or_else(|| self.0.delegate.properties().overrides_coast())
    }

    fn is_lake(&self) -> bool {
        self.0.delegate.properties().is_lake()
    }

    fn is_wetland(&self) -> bool {
        self.0.delegate.properties().is_wetland()
    }

    fn is_mountain(&self) -> bool {
        self.0
            .mountain
            .unwrap_or_else(|| self.0.delegate.properties().is_mountain())
    }

    fn is_volcano(&self) -> bool {
        self.0
            .volcano
            .unwrap_or_else(|| self.0.delegate.properties().is_volcano())
    }
}

fn java_min(first: f32, second: f32) -> f32 {
    if first.is_nan() {
        first
    } else if second.is_nan() {
        second
    } else if first == second {
        f32::from_bits(first.to_bits() | second.to_bits())
    } else if first < second {
        first
    } else {
        second
    }
}

pub struct TerrainRegistry {
    terrains: Vec<Terrain>,
}

impl TerrainRegistry {
    pub fn new() -> Self {
        let terrains = vec![
            Terrain::new(0, "none", TerrainCategory::None),
            Terrain::new(1, "deep_ocean", TerrainCategory::DeepOcean),
            Terrain::new(2, "ocean", TerrainCategory::ShallowOcean),
            Terrain::new(3, "coast", TerrainCategory::Coast),
            Terrain::new(4, "beach", TerrainCategory::Beach),
            Terrain::new(5, "river", TerrainCategory::River),
            Terrain::new(6, "lake", TerrainCategory::Lake),
            Terrain::configured_river_override(7, "wetland", TerrainCategory::Wetland, true),
            Terrain::new(8, "flats", TerrainCategory::Flatland),
            Terrain::configured_erosion(9, "badlands", TerrainCategory::Flatland, 0.3),
            Terrain::new(10, "plateau", TerrainCategory::Lowland),
            Terrain::new(11, "hills", TerrainCategory::Lowland),
            Terrain::configured_mountain(12, "mountains", TerrainCategory::Highland, true, true),
            Terrain::configured_mountain(
                13,
                "mountain_chain",
                TerrainCategory::Highland,
                true,
                true,
            ),
            Terrain::volcano(14, "volcano", TerrainCategory::Highland),
            Terrain::volcano(15, "volcano_pipe", TerrainCategory::Highland),
        ];
        Self { terrains }
    }

    pub fn terrains(&self) -> &[Terrain] {
        &self.terrains
    }

    pub fn find(&self, filter: impl Fn(&Terrain) -> bool) -> Option<Terrain> {
        self.terrains
            .iter()
            .find(|terrain| filter(terrain))
            .cloned()
    }

    pub fn get_by_name(&self, name: &str) -> Option<Terrain> {
        self.terrains
            .iter()
            .find(|terrain| terrain.name().eq_ignore_ascii_case(name))
            .cloned()
    }

    pub fn get_by_id(&self, id: i32) -> Terrain {
        usize::try_from(id)
            .ok()
            .and_then(|id| self.terrains.get(id))
            .unwrap_or(&self.terrains[0])
            .clone()
    }

    pub fn register(&mut self, instance: Terrain) -> Terrain {
        if let Some(current) = self.get_by_name(instance.name()) {
            return current;
        }
        let terrain = instance.with_id(self.terrains.len() as i32);
        self.terrains.push(terrain.clone());
        terrain
    }

    pub fn register_composite(&mut self, first: &Terrain, second: &Terrain) -> Terrain {
        if first.is_same(second) {
            return first.clone();
        }
        let min = if first.id() < second.id() {
            first.clone()
        } else {
            second.clone()
        };
        let max = if first.id() > second.id() {
            first.clone()
        } else {
            second.clone()
        };
        let name = format!("{}-{}", min.name(), max.name());
        if let Some(current) = self.get_by_name(&name) {
            return current;
        }
        let terrain = Terrain::composite(self.terrains.len() as i32, min, max);
        self.terrains.push(terrain.clone());
        terrain
    }

    pub fn get_or_create(&mut self, name: &str, parent: Option<Terrain>) -> Terrain {
        let Some(parent) = parent else {
            return self.get_by_id(0);
        };
        if parent.is_same(&self.terrains[0]) {
            return self.get_by_id(0);
        }
        if let Some(current) = self.get_by_name(name) {
            return current;
        }
        self.register(Terrain::with_parent(0, Arc::<str>::from(name), parent))
    }
}

impl Default for TerrainRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct Cell {
    pub value: f32,
    pub erosion: f32,
    pub sediment: f32,
    pub gradient: f32,
    pub moisture: f32,
    pub temperature: f32,
    pub continent_id: f32,
    pub continent_edge: f32,
    pub terrain_region_id: f32,
    pub terrain_region_edge: f32,
    pub terrain_region_center: i64,
    pub biome_region_id: f32,
    pub biome_region_edge: f32,
    pub macro_biome_id: f32,
    pub river_mask: f32,
    pub continent_x: i32,
    pub continent_z: i32,
    pub erosion_mask: bool,
    pub terrain: Terrain,
    pub biome: BiomeType,
    absent: bool,
}

impl Cell {
    pub fn empty() -> Self {
        Self {
            absent: true,
            ..Self::default()
        }
    }

    pub fn copy_from(&mut self, other: &Self) {
        self.value = other.value;
        self.continent_x = other.continent_x;
        self.continent_z = other.continent_z;
        self.continent_id = other.continent_id;
        self.continent_edge = other.continent_edge;
        self.terrain_region_id = other.terrain_region_id;
        self.terrain_region_edge = other.terrain_region_edge;
        self.biome_region_id = other.biome_region_id;
        self.biome_region_edge = other.biome_region_edge;
        self.river_mask = other.river_mask;
        self.erosion_mask = other.erosion_mask;
        self.moisture = other.moisture;
        self.temperature = other.temperature;
        self.macro_biome_id = other.macro_biome_id;
        self.gradient = other.gradient;
        self.erosion = other.erosion;
        self.sediment = other.sediment;
        self.biome = other.biome;
        self.terrain = other.terrain.clone();
    }

    pub fn reset(&mut self) -> &mut Self {
        self.copy_from(&Self::default());
        self
    }

    pub const fn is_absent(&self) -> bool {
        self.absent
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            value: 0.0,
            erosion: 0.0,
            sediment: 0.0,
            gradient: 0.0,
            moisture: 0.5,
            temperature: 0.5,
            continent_id: 0.0,
            continent_edge: 0.0,
            terrain_region_id: 0.0,
            terrain_region_edge: 0.0,
            terrain_region_center: 0,
            biome_region_id: 0.0,
            biome_region_edge: 1.0,
            macro_biome_id: 0.0,
            river_mask: 1.0,
            continent_x: 0,
            continent_z: 0,
            erosion_mask: false,
            terrain: TerrainRegistry::new().get_by_id(0),
            biome: BiomeType::Grassland,
            absent: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpawnType {
    ContinentCenter,
    WorldOrigin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ContinentType {
    Multi = 0,
    Single = 1,
    MultiImproved = 2,
    Fancy = 3,
}

impl ContinentType {
    pub const fn index(self) -> i32 {
        self as i32
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContinentShape {
    Euclidean,
    Manhattan,
    Natural,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContinentSettings {
    pub continent_type: ContinentType,
    pub continent_shape: ContinentShape,
    pub continent_scale: i32,
    pub continent_jitter: f32,
    pub continent_skipping: f32,
    pub continent_size_variance: f32,
    pub continent_noise_octaves: i32,
    pub continent_noise_gain: f32,
    pub continent_noise_lacunarity: f32,
}

impl ContinentSettings {
    pub const DEFAULT_SCALE: i32 = 3_000;
}

impl Default for ContinentSettings {
    fn default() -> Self {
        Self {
            continent_type: ContinentType::MultiImproved,
            continent_shape: ContinentShape::Euclidean,
            continent_scale: Self::DEFAULT_SCALE,
            continent_jitter: 0.7,
            continent_skipping: 0.25,
            continent_size_variance: 0.25,
            continent_noise_octaves: 5,
            continent_noise_gain: 0.26,
            continent_noise_lacunarity: 4.33,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorldProperties {
    pub spawn_type: SpawnType,
    pub world_height: i32,
    pub sea_level: i32,
}

impl Default for WorldProperties {
    fn default() -> Self {
        Self {
            spawn_type: SpawnType::ContinentCenter,
            world_height: 256,
            sea_level: 63,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldControlPoints {
    pub deep_ocean: f32,
    pub shallow_ocean: f32,
    pub beach: f32,
    pub coast: f32,
    pub inland: f32,
}

impl Default for WorldControlPoints {
    fn default() -> Self {
        Self {
            deep_ocean: 0.1,
            shallow_ocean: 0.25,
            beach: 0.327,
            coast: 0.448,
            inland: 0.502,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WorldSettings {
    pub seed: i64,
    pub continent: ContinentSettings,
    pub control_points: WorldControlPoints,
    pub properties: WorldProperties,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainFormSettings {
    pub weight: f32,
    pub base_scale: f32,
    pub vertical_scale: f32,
    pub horizontal_scale: f32,
}

impl TerrainFormSettings {
    pub const fn new(weight: f32, vertical_scale: f32, horizontal_scale: f32) -> Self {
        Self {
            weight,
            base_scale: 1.0,
            vertical_scale,
            horizontal_scale,
        }
    }
}

impl Default for TerrainFormSettings {
    fn default() -> Self {
        Self::new(1.0, 1.0, 1.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeneralTerrainSettings {
    pub terrain_seed_offset: i32,
    pub terrain_region_size: i32,
    pub global_vertical_scale: f32,
    pub global_horizontal_scale: f32,
    pub fancy_mountains: bool,
}

impl Default for GeneralTerrainSettings {
    fn default() -> Self {
        Self {
            terrain_seed_offset: 0,
            terrain_region_size: 1_200,
            global_vertical_scale: 0.98,
            global_horizontal_scale: 1.0,
            fancy_mountains: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainSettings {
    pub general: GeneralTerrainSettings,
    pub steppe: TerrainFormSettings,
    pub plains: TerrainFormSettings,
    pub hills: TerrainFormSettings,
    pub dales: TerrainFormSettings,
    pub plateau: TerrainFormSettings,
    pub badlands: TerrainFormSettings,
    pub torridonian: TerrainFormSettings,
    pub mountains: TerrainFormSettings,
    pub volcano: TerrainFormSettings,
}

impl Default for TerrainSettings {
    fn default() -> Self {
        Self {
            general: GeneralTerrainSettings::default(),
            steppe: TerrainFormSettings::new(1.0, 1.0, 1.0),
            plains: TerrainFormSettings::new(2.0, 1.0, 1.0),
            hills: TerrainFormSettings::new(2.0, 1.0, 1.0),
            dales: TerrainFormSettings::new(1.5, 1.0, 1.0),
            plateau: TerrainFormSettings::new(1.5, 1.0, 1.0),
            badlands: TerrainFormSettings::new(1.0, 1.0, 1.0),
            torridonian: TerrainFormSettings::new(2.0, 1.0, 1.0),
            mountains: TerrainFormSettings::new(2.5, 1.0, 1.0),
            volcano: TerrainFormSettings::new(5.0, 1.0, 1.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlPoints {
    pub deep_ocean: f32,
    pub shallow_ocean: f32,
    pub beach: f32,
    pub coast: f32,
    pub coast_marker: f32,
    pub inland: f32,
}

impl ControlPoints {
    pub fn new(mut points: WorldControlPoints) -> Self {
        if !Self::validate(&points) {
            points = WorldControlPoints::default();
        }
        Self {
            inland: points.inland,
            coast: points.coast,
            beach: points.beach,
            shallow_ocean: points.shallow_ocean,
            deep_ocean: points.deep_ocean,
            coast_marker: points.coast + (points.inland - points.coast) / 2.0,
        }
    }

    pub fn validate(points: &WorldControlPoints) -> bool {
        points.inland <= 1.0
            && points.inland > points.coast
            && points.coast > points.beach
            && points.beach > points.shallow_ocean
            && points.shallow_ocean > points.deep_ocean
            && points.deep_ocean >= 0.0
    }
}

impl Default for ControlPoints {
    fn default() -> Self {
        Self::new(WorldControlPoints::default())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Levels {
    pub world_height: i32,
    pub unit: f32,
    pub water_y: i32,
    ground_y: i32,
    pub ground_level: i32,
    pub water_level: i32,
    pub ground: f32,
    pub water: f32,
    elevation_range: f32,
}

impl Levels {
    pub fn from_properties(properties: WorldProperties) -> Self {
        Self::new(properties.world_height, properties.sea_level)
    }

    pub fn new(height: i32, sea_level: i32) -> Self {
        let world_height = height.max(1);
        let unit = 1.0 / world_height as f32;
        let water_level = sea_level;
        let ground_level = water_level.wrapping_add(1);
        let water_y = water_level.wrapping_sub(1).min(world_height);
        let ground_y = ground_level.wrapping_sub(1).min(world_height);
        let ground = ground_y as f32 / world_height as f32;
        let water = water_y as f32 / world_height as f32;
        Self {
            world_height,
            unit,
            water_y,
            ground_y,
            ground_level,
            water_level,
            ground,
            water,
            elevation_range: 1.0 - water,
        }
    }

    pub fn scale_value(self, value: f32) -> i32 {
        if value >= 1.0 {
            self.world_height - 1
        } else {
            (value * self.world_height as f32) as i32
        }
    }

    pub fn elevation_value(self, value: f32) -> f32 {
        if value <= self.water {
            0.0
        } else {
            (value - self.water) / self.elevation_range
        }
    }

    pub fn elevation_y(self, y: i32) -> f32 {
        if y <= self.water_y {
            0.0
        } else {
            self.scale_level(y - self.water_y) / self.elevation_range
        }
    }

    pub fn scale_level(self, level: i32) -> f32 {
        level as f32 / self.world_height as f32
    }

    pub fn water(self, amount: i32) -> f32 {
        self.water_y.wrapping_add(amount) as f32 / self.world_height as f32
    }

    pub fn ground(self, amount: i32) -> f32 {
        self.ground_y.wrapping_add(amount) as f32 / self.world_height as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn property_bits(terrain: &impl TerrainProperties) -> u16 {
        u16::from(terrain.is_flat())
            | u16::from(terrain.is_river()) << 1
            | u16::from(terrain.is_shallow_ocean()) << 2
            | u16::from(terrain.is_deep_ocean()) << 3
            | u16::from(terrain.is_coast()) << 4
            | u16::from(terrain.is_submerged()) << 5
            | u16::from(terrain.is_overground()) << 6
            | u16::from(terrain.overrides_river()) << 7
            | u16::from(terrain.overrides_coast()) << 8
            | u16::from(terrain.is_lake()) << 9
            | u16::from(terrain.is_wetland()) << 10
            | u16::from(terrain.is_mountain()) << 11
            | u16::from(terrain.is_volcano()) << 12
    }

    #[test]
    fn cell_model_matches_engine_0_3_0() {
        assert_eq!(
            BiomeType::VALUES.map(BiomeType::id),
            [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
        );
        assert_eq!(BiomeType::get(5), BiomeType::Grassland);
        assert!(BiomeType::Desert.is_extreme());
        assert!(BiomeType::Tundra.is_extreme());
        assert!(!BiomeType::Alpine.is_extreme());

        let defaults = Cell::default();
        assert_eq!(defaults.value.to_bits(), 0);
        assert_eq!(defaults.erosion.to_bits(), 0);
        assert_eq!(defaults.sediment.to_bits(), 0);
        assert_eq!(defaults.gradient.to_bits(), 0);
        assert_eq!(defaults.moisture.to_bits(), 0x3f000000);
        assert_eq!(defaults.temperature.to_bits(), 0x3f000000);
        assert_eq!(defaults.biome_region_edge.to_bits(), 0x3f800000);
        assert_eq!(defaults.river_mask.to_bits(), 0x3f800000);
        assert_eq!(defaults.terrain.id(), 0);
        assert_eq!(defaults.biome, BiomeType::Grassland);
        assert!(!defaults.is_absent());

        let mut source = Cell::default();
        source.value = 0.75;
        source.erosion = 0.25;
        source.sediment = 0.125;
        source.gradient = 0.875;
        source.moisture = 0.2;
        source.temperature = 0.8;
        source.continent_id = 0.1;
        source.continent_edge = 0.9;
        source.terrain_region_id = 0.3;
        source.terrain_region_edge = 0.7;
        source.terrain_region_center = 123_456_789;
        source.biome_region_id = 0.4;
        source.biome_region_edge = 0.6;
        source.macro_biome_id = 0.55;
        source.river_mask = 0.45;
        source.continent_x = -123;
        source.continent_z = 456;
        source.erosion_mask = true;
        source.terrain = TerrainRegistry::new().get_by_id(12);
        source.biome = BiomeType::Alpine;

        let mut copied = Cell::default();
        copied.terrain_region_center = -99;
        copied.copy_from(&source);
        assert_eq!(copied.value.to_bits(), source.value.to_bits());
        assert_eq!(copied.erosion.to_bits(), source.erosion.to_bits());
        assert_eq!(copied.sediment.to_bits(), source.sediment.to_bits());
        assert_eq!(copied.gradient.to_bits(), source.gradient.to_bits());
        assert_eq!(copied.moisture.to_bits(), source.moisture.to_bits());
        assert_eq!(copied.temperature.to_bits(), source.temperature.to_bits());
        assert_eq!(copied.continent_x, source.continent_x);
        assert_eq!(copied.continent_z, source.continent_z);
        assert_eq!(copied.terrain_region_center, -99);
        assert!(copied.erosion_mask);
        assert_eq!(copied.terrain.id(), 12);
        assert_eq!(copied.biome, BiomeType::Alpine);

        copied.reset();
        assert_eq!(copied.value.to_bits(), 0);
        assert_eq!(copied.terrain_region_center, -99);
        assert_eq!(copied.terrain.id(), 0);
        assert_eq!(copied.biome, BiomeType::Grassland);

        let mut empty = Cell::empty();
        assert!(empty.is_absent());
        empty.reset();
        assert!(empty.is_absent());
    }

    #[test]
    fn seed_matches_engine_0_3_0() {
        let cases = [
            (0, 0, 0, 1, 2, 0, i32::MAX),
            (42, 42, 42, 43, 44, 42, -2_147_483_607),
            (-1, -1, -1, 0, 1, -1, 2_147_483_646),
            (
                i32::MAX as i64,
                i32::MAX,
                i32::MAX,
                i32::MIN,
                i32::MIN + 1,
                i32::MAX,
                -2,
            ),
            (
                2_147_483_648,
                i32::MIN,
                i32::MIN,
                i32::MIN + 1,
                i32::MIN + 2,
                i32::MIN,
                -1,
            ),
            (i64::MIN, 0, 0, 1, 2, 0, i32::MAX),
            (i64::MAX, -1, -1, 0, 1, -1, 2_147_483_646),
        ];
        for (input, root, first, second, current, split, offset) in cases {
            let mut seed = Seed::from_long(input);
            assert_eq!(seed.root(), root);
            assert_eq!(seed.next(), first);
            assert_eq!(seed.next(), second);
            assert_eq!(seed.get(), current);
            assert_eq!(seed.split().get(), split);
            assert_eq!(seed.offset(i32::MAX).get(), offset);
        }
    }

    #[test]
    fn terrain_categories_match_engine_0_3_0() {
        let expected = [
            0x000, 0x0a8, 0x0a4, 0x0d0, 0x0d0, 0x022, 0x220, 0x440, 0x041, 0x040, 0x040,
        ];
        for (category, expected) in TerrainCategory::VALUES.into_iter().zip(expected) {
            assert_eq!(category.erosion_modifier().to_bits(), 0x3f800000);
            assert_eq!(property_bits(&category), expected, "{category:?}");
        }
    }

    #[test]
    fn category_dominance_matches_engine_0_3_0() {
        for first in TerrainCategory::VALUES {
            for second in TerrainCategory::VALUES {
                let expected = if first as u8 > second as u8 {
                    first
                } else {
                    second
                };
                assert_eq!(first.get_dominant(second), expected);
            }
        }
    }

    #[test]
    fn terrain_descriptors_match_engine_0_3_0() {
        let river = Terrain::new(7, "river_custom", TerrainCategory::River);
        let child = Terrain::with_parent(8, "child", river.clone());
        let unicode = Terrain::new(9, "Volcán🔥", TerrainCategory::Highland);
        let erosion = Terrain::configured_erosion(10, "erosion", TerrainCategory::Flatland, 0.3);
        let wet = Terrain::configured_river_override(11, "wet", TerrainCategory::Wetland, true);
        let mountain =
            Terrain::configured_mountain(12, "mountain", TerrainCategory::Highland, true, true);
        let full = Terrain::configured(13, "full", TerrainCategory::Coast, 0.625, true, false);
        let composite_ab = Terrain::composite(14, erosion.clone(), mountain.clone());
        let composite_ba = Terrain::composite(15, mountain, erosion);
        let with_id = child.with_id(99);
        let cases = [
            (
                river,
                7,
                "river_custom",
                TerrainCategory::River,
                0xbf7959fa,
                0x3f800000,
                0x022,
            ),
            (
                child,
                8,
                "child",
                TerrainCategory::River,
                0xbf58360a,
                0x3f800000,
                0x022,
            ),
            (
                unicode,
                9,
                "Volcán🔥",
                TerrainCategory::Highland,
                0xbf6f06bb,
                0x3f800000,
                0x040,
            ),
            (
                composite_ab,
                14,
                "erosion-mountain",
                TerrainCategory::Flatland,
                0xbf6edf09,
                0x3e99999a,
                0x040,
            ),
            (
                composite_ba,
                15,
                "mountain-erosion",
                TerrainCategory::Highland,
                0x3d66463b,
                0x3e99999a,
                0x8c0,
            ),
            (
                full,
                13,
                "full",
                TerrainCategory::Coast,
                0xbf354c07,
                0x3f200000,
                0x850,
            ),
            (
                wet,
                11,
                "wet",
                TerrainCategory::Wetland,
                0x3f394597,
                0x3f800000,
                0x4c0,
            ),
            (
                with_id,
                99,
                "child",
                TerrainCategory::River,
                0xbf58360a,
                0x3f800000,
                0x022,
            ),
        ];
        for (terrain, id, name, category, hue, erosion, properties) in cases {
            assert_eq!(terrain.id(), id);
            assert_eq!(terrain.name(), name);
            assert_eq!(terrain.category(), category);
            assert_eq!(terrain.render_hue().to_bits(), hue);
            assert_eq!(terrain.erosion_modifier().to_bits(), erosion);
            assert_eq!(property_bits(&terrain), properties, "{terrain:?}");
        }
    }

    #[test]
    fn volcano_overrides_match_engine_0_3_0() {
        let volcano = Terrain::volcano(14, "volcano", TerrainCategory::Highland);
        assert_eq!(property_bits(&volcano), 0x19c0);
    }

    #[test]
    fn default_terrain_registry_matches_engine_0_3_0() {
        let registry = TerrainRegistry::new();
        let expected = [
            ("none", TerrainCategory::None, 0x3f800000, 0x000),
            ("deep_ocean", TerrainCategory::DeepOcean, 0x3f800000, 0x0a8),
            ("ocean", TerrainCategory::ShallowOcean, 0x3f800000, 0x0a4),
            ("coast", TerrainCategory::Coast, 0x3f800000, 0x0d0),
            ("beach", TerrainCategory::Beach, 0x3f800000, 0x0d0),
            ("river", TerrainCategory::River, 0x3f800000, 0x022),
            ("lake", TerrainCategory::Lake, 0x3f800000, 0x220),
            ("wetland", TerrainCategory::Wetland, 0x3f800000, 0x4c0),
            ("flats", TerrainCategory::Flatland, 0x3f800000, 0x041),
            ("badlands", TerrainCategory::Flatland, 0x3e99999a, 0x041),
            ("plateau", TerrainCategory::Lowland, 0x3f800000, 0x040),
            ("hills", TerrainCategory::Lowland, 0x3f800000, 0x040),
            ("mountains", TerrainCategory::Highland, 0x3f800000, 0x8c0),
            (
                "mountain_chain",
                TerrainCategory::Highland,
                0x3f800000,
                0x8c0,
            ),
            ("volcano", TerrainCategory::Highland, 0x3f800000, 0x19c0),
            (
                "volcano_pipe",
                TerrainCategory::Highland,
                0x3f800000,
                0x19c0,
            ),
        ];
        assert_eq!(registry.terrains().len(), expected.len());
        for (id, (terrain, expected)) in registry.terrains().iter().zip(expected).enumerate() {
            let (name, category, erosion, properties) = expected;
            assert_eq!(terrain.id(), id as i32);
            assert_eq!(terrain.name(), name);
            assert_eq!(terrain.category(), category);
            assert_eq!(terrain.erosion_modifier().to_bits(), erosion);
            assert_eq!(property_bits(terrain), properties, "{terrain:?}");
        }
        assert!(registry.get_by_id(-1).is_same(&registry.get_by_id(0)));
        assert!(registry.get_by_id(999).is_same(&registry.get_by_id(0)));
        assert_eq!(registry.get_by_name("OcEaN").unwrap().id(), 2);
        assert_eq!(
            registry
                .find(|terrain| terrain.name() == "hills")
                .unwrap()
                .id(),
            11
        );
    }

    #[test]
    fn terrain_registration_matches_engine_0_3_0() {
        let mut registry = TerrainRegistry::new();
        let hills = registry.get_by_id(11);
        let custom = registry.register(Terrain::with_parent(999, "Custom", hills));
        assert_eq!(custom.id(), 16);
        assert_eq!(custom.category(), TerrainCategory::Lowland);

        let duplicate = registry.register(Terrain::new(999, "cUsToM", TerrainCategory::River));
        assert!(custom.is_same(&duplicate));

        let badlands = registry.get_by_id(9);
        let composite = registry.register_composite(&custom, &badlands);
        assert_eq!(composite.id(), 17);
        assert_eq!(composite.name(), "badlands-Custom");
        assert_eq!(composite.category(), TerrainCategory::Flatland);
        assert_eq!(composite.erosion_modifier().to_bits(), 0x3e99999a);
        assert_eq!(property_bits(&composite), 0x040);
        assert!(composite.is_same(&registry.register_composite(&badlands, &custom)));
        assert!(custom.is_same(&registry.register_composite(&custom, &custom)));

        assert!(
            registry
                .get_or_create("null_child", None)
                .is_same(&registry.get_by_id(0))
        );
        assert!(
            registry
                .get_or_create("none_child", Some(registry.get_by_id(0)))
                .is_same(&registry.get_by_id(0))
        );
        assert!(
            registry
                .get_or_create("CUSTOM", Some(registry.get_by_id(5)))
                .is_same(&custom)
        );
        let child = registry.get_or_create("child", Some(custom));
        assert_eq!(child.id(), 18);
        assert_eq!(child.category(), TerrainCategory::Lowland);
        assert_eq!(registry.terrains().len(), 19);
    }

    #[test]
    fn control_points_match_engine_0_3_0() {
        let defaults = ControlPoints::default();
        assert_eq!(
            bits_of_control_points(defaults),
            [
                0x3dcccccd, 0x3e800000, 0x3ea76c8b, 0x3ee56042, 0x3ef33333, 0x3f008312,
            ]
        );
        let valid = WorldControlPoints {
            deep_ocean: 0.0,
            shallow_ocean: 0.2,
            beach: 0.3,
            coast: 0.6,
            inland: 1.0,
        };
        assert!(ControlPoints::validate(&valid));
        assert_eq!(
            bits_of_control_points(ControlPoints::new(valid)),
            [
                0x00000000, 0x3e4ccccd, 0x3e99999a, 0x3f19999a, 0x3f4ccccd, 0x3f800000,
            ]
        );
        let mut invalid = WorldControlPoints::default();
        invalid.coast = invalid.inland;
        assert!(!ControlPoints::validate(&invalid));
        assert_eq!(ControlPoints::new(invalid), defaults);
        invalid = WorldControlPoints::default();
        invalid.beach = f32::NAN;
        assert!(!ControlPoints::validate(&invalid));
        assert_eq!(ControlPoints::new(invalid), defaults);
    }

    fn bits_of_control_points(points: ControlPoints) -> [u32; 6] {
        [
            points.deep_ocean.to_bits(),
            points.shallow_ocean.to_bits(),
            points.beach.to_bits(),
            points.coast.to_bits(),
            points.coast_marker.to_bits(),
            points.inland.to_bits(),
        ]
    }

    #[test]
    fn levels_match_engine_0_3_0() {
        let levels = Levels::new(256, 63);
        assert_eq!(levels.world_height, 256);
        assert_eq!(levels.unit.to_bits(), 0x3b800000);
        assert_eq!(levels.water_y, 62);
        assert_eq!(levels.ground_level, 64);
        assert_eq!(levels.ground.to_bits(), 0x3e7c0000);
        assert_eq!(levels.water.to_bits(), 0x3e780000);
        let values = [
            -1.0,
            0.0,
            0.25,
            levels.water,
            0.75,
            1.0,
            f32::NAN,
            f32::INFINITY,
        ];
        assert_eq!(
            values.map(|value| levels.scale_value(value)),
            [-256, 0, 64, 62, 192, 255, 0, 255]
        );
        assert_eq!(
            values.map(|value| levels.elevation_value(value).to_bits()),
            [
                0x00000000, 0x00000000, 0x3c28e83f, 0x00000000, 0x3f2b8be0, 0x3f800000, 0x7fc00000,
                0x7f800000,
            ]
        );
        assert_eq!(
            [-10, 62, 63, 256, 300].map(|y| levels.elevation_y(y).to_bits()),
            [0x00000000, 0x00000000, 0x3ba8e83f, 0x3f800000, 0x3f9d07eb]
        );
        assert_eq!(levels.scale_level(-5).to_bits(), 0xbca00000);
        assert_eq!(levels.water(5).to_bits(), 0x3e860000);
        assert_eq!(levels.ground(5).to_bits(), 0x3e880000);
    }

    #[test]
    fn level_edge_cases_match_engine_0_3_0() {
        let zero = Levels::new(0, 63);
        assert_eq!(zero.world_height, 1);
        assert_eq!(zero.water_y, 1);
        assert_eq!(zero.water.to_bits(), 0x3f800000);
        assert_eq!(zero.scale_value(1.0), 0);
        assert_eq!(zero.elevation_y(2).to_bits(), 0x7f800000);

        let negative = Levels::new(-10, -5);
        assert_eq!(negative.world_height, 1);
        assert_eq!(negative.water_y, -6);
        assert_eq!(negative.ground.to_bits(), 0xc0a00000);
        assert_eq!(negative.water.to_bits(), 0xc0c00000);
        assert_eq!(negative.elevation_value(-1.0).to_bits(), 0x3f36db6e);

        let high_sea = Levels::new(100, 200);
        assert_eq!(high_sea.water_y, 100);
        assert_eq!(high_sea.water.to_bits(), 0x3f800000);
        assert_eq!(high_sea.elevation_y(101).to_bits(), 0x7f800000);
    }

    #[test]
    fn world_settings_match_engine_0_3_0() {
        let settings = WorldSettings::default();
        assert_eq!(settings.seed, 0);
        assert_eq!(
            settings.continent.continent_type,
            ContinentType::MultiImproved
        );
        assert_eq!(
            settings.continent.continent_shape,
            ContinentShape::Euclidean
        );
        assert_eq!(settings.continent.continent_scale, 3_000);
        assert_eq!(settings.continent.continent_jitter.to_bits(), 0x3f333333);
        assert_eq!(settings.continent.continent_skipping.to_bits(), 0x3e800000);
        assert_eq!(
            settings.continent.continent_size_variance.to_bits(),
            0x3e800000
        );
        assert_eq!(settings.continent.continent_noise_octaves, 5);
        assert_eq!(
            settings.continent.continent_noise_gain.to_bits(),
            0x3e851eb8
        );
        assert_eq!(
            settings.continent.continent_noise_lacunarity.to_bits(),
            0x408a8f5c
        );
        assert_eq!(settings.properties, WorldProperties::default());
        assert_eq!(settings.control_points, WorldControlPoints::default());
        assert_eq!(ContinentType::Multi.index(), 0);
        assert_eq!(ContinentType::Single.index(), 1);
        assert_eq!(ContinentType::MultiImproved.index(), 2);
        assert_eq!(ContinentType::Fancy.index(), 3);
    }

    #[test]
    fn terrain_settings_match_engine_0_3_0() {
        let settings = TerrainSettings::default();
        assert_eq!(settings.general.terrain_seed_offset, 0);
        assert_eq!(settings.general.terrain_region_size, 1_200);
        assert_eq!(settings.general.global_vertical_scale.to_bits(), 0x3f7ae148);
        assert_eq!(
            settings.general.global_horizontal_scale.to_bits(),
            0x3f800000
        );
        assert!(settings.general.fancy_mountains);
        let forms = [
            settings.steppe,
            settings.plains,
            settings.hills,
            settings.dales,
            settings.plateau,
            settings.badlands,
            settings.torridonian,
            settings.mountains,
            settings.volcano,
        ];
        assert_eq!(
            forms.map(|form| form.weight.to_bits()),
            [
                0x3f800000, 0x40000000, 0x40000000, 0x3fc00000, 0x3fc00000, 0x3f800000, 0x40000000,
                0x40200000, 0x40a00000,
            ]
        );
        for form in forms {
            assert_eq!(form.base_scale.to_bits(), 0x3f800000);
            assert_eq!(form.vertical_scale.to_bits(), 0x3f800000);
            assert_eq!(form.horizontal_scale.to_bits(), 0x3f800000);
        }
    }
}
