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

/// Engine's `Seed`: a root and a cursor that `next` post-increments, wrapping like Java's `int`.
///
/// Engine shares one mutable `Seed` by reference, so every builder that calls `next()` advances
/// the caller's cursor. This type is `Copy`: pass `&mut Seed` wherever Engine passes a `Seed` it
/// draws from, or the copy's draws are lost to the caller.
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

/// Engine's `Terrain` and its `ConfiguredTerrain`, `CompositeTerrain` and volcano subclasses: an
/// id, a name and a category, with every `ITerrain` flag delegated to the category or to a parent
/// terrain unless the subclass overrides it.
///
/// Engine compares terrains by object identity (`==`, no `equals`); clones of a `Terrain` share
/// one object, and [`Terrain::is_same`] is that identity. There is deliberately no `PartialEq`.
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
        Self(Arc::new(Self::data(id, name, category, delegate)))
    }

    fn data(
        id: i32,
        name: impl Into<Arc<str>>,
        category: TerrainCategory,
        delegate: TerrainDelegate,
    ) -> TerrainData {
        TerrainData {
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
        }
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
        Self(Arc::new(Self::configured_data(
            id,
            name,
            category,
            erosion,
            is_mountain,
            overrides_river,
        )))
    }

    fn configured_data(
        id: i32,
        name: impl Into<Arc<str>>,
        category: TerrainCategory,
        erosion: f32,
        is_mountain: bool,
        overrides_river: bool,
    ) -> TerrainData {
        let mut data = Self::data(id, name, category, TerrainDelegate::Category(category));
        data.erosion = Some(erosion);
        data.mountain = Some(is_mountain);
        data.river_override = Some(overrides_river);
        data
    }

    pub fn volcano(id: i32, name: impl Into<Arc<str>>, category: TerrainCategory) -> Self {
        let erosion = category.erosion_modifier();
        let mut data = Self::configured_data(id, name, category, erosion, true, true);
        data.coast_override = Some(true);
        data.volcano = Some(true);
        Self(Arc::new(data))
    }

    /// Engine's `CompositeTerrain`: named `first-second`, flat only when both are, with the smaller
    /// erosion modifier (`Math.min`). Engine's `getDominant` compares `first`'s category with
    /// itself, so `first` always wins: the composite takes its category and delegates every other
    /// flag to it, whatever `second`'s category is.
    pub fn composite(id: i32, first: Self, second: Self) -> Self {
        let name: Arc<str> = format!("{}-{}", first.name(), second.name()).into();
        let flat = first.is_flat() && second.is_flat();
        let erosion = java_min(first.erosion_modifier(), second.erosion_modifier());
        let mut data = Self::data(id, name, first.category(), TerrainDelegate::Terrain(first));
        data.flat = Some(flat);
        data.erosion = Some(erosion);
        Self(Arc::new(data))
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

    /// `NoiseUtil.valCoord2D(name.hashCode(), 0, 0)`, with Java's UTF-16 string hash.
    pub fn render_hue(&self) -> f32 {
        let hash = self.name().encode_utf16().fold(0_i32, |hash, unit| {
            hash.wrapping_mul(31).wrapping_add(i32::from(unit))
        });
        hash.wrapping_mul(hash)
            .wrapping_mul(hash)
            .wrapping_mul(60_493) as f32
            / 2_147_483_648.0_f32
    }

    /// Engine's `withId`: a plain terrain with the new id, the same name and category, delegating
    /// to this terrain's parent if it has one, else to this terrain. A composite's own flat flag and
    /// erosion are therefore not carried over (its first terrain's are), as in Engine.
    pub fn with_id(&self, id: i32) -> Self {
        let delegate = match &self.0.delegate {
            TerrainDelegate::Terrain(terrain) => TerrainDelegate::Terrain(terrain.clone()),
            TerrainDelegate::Category(_) => TerrainDelegate::Terrain(self.clone()),
        };
        Self::build(id, self.0.name.clone(), self.category(), delegate)
    }

    /// Java's `==` on terrains: true only for the same object (this terrain or a clone of it).
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

/// Engine's `TerrainType` registry: the 16 built-in terrains with ids 0..=15 in Engine's order,
/// then every registered terrain with the next id. Names are unique ignoring case, as Java's
/// `String.equalsIgnoreCase` compares them.
///
/// Engine's registry is a static global, so in one Java process the ids of composite and custom
/// terrains depend on what earlier worlds registered. Here each world owns a registry, so the ids
/// it assigns only depend on that world's own registrations. Built-in terrains are distinct
/// objects in each registry: compare a terrain with the built-ins of the registry it came from
/// (`is_same`), never with another registry's. Registration takes `&mut self`; share a registry
/// that is no longer registered into behind an `Arc`, or one that still is behind a lock, as
/// Engine's `lock` does.
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

    /// `TerrainType.get(String)`: the first terrain whose name equals `name` ignoring case.
    pub fn get_by_name(&self, name: &str) -> Option<Terrain> {
        self.terrains
            .iter()
            .find(|terrain| java_equals_ignore_case(terrain.name(), name))
            .cloned()
    }

    /// `TerrainType.get(int)`: the terrain with this id, or `none` (id 0) for an unknown id.
    pub fn get_by_id(&self, id: i32) -> Terrain {
        usize::try_from(id)
            .ok()
            .and_then(|id| self.terrains.get(id))
            .unwrap_or(&self.terrains[0])
            .clone()
    }

    /// `TerrainType.register`: the registered terrain with `instance`'s name if there is one, else
    /// `instance.with_id(next id)`, registered.
    pub fn register(&mut self, instance: Terrain) -> Terrain {
        if let Some(current) = self.get_by_name(instance.name()) {
            return current;
        }
        let terrain = instance.with_id(self.terrains.len() as i32);
        self.terrains.push(terrain.clone());
        terrain
    }

    /// `TerrainType.registerComposite`: `first` itself if both are the same object, else the
    /// composite of the lower-id and the higher-id terrain, named `lower-higher`, registered once.
    /// As in Engine, two distinct terrains with the same id make a composite of `second` with itself.
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

    /// `TerrainHelper.getOrCreate`: `none` without a parent or when the parent is this registry's
    /// `none`, else the terrain registered under `name`, or a new one delegating to `parent`.
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

/// Java's `String.equalsIgnoreCase`: the same UTF-16 length and, code point by code point, equal,
/// equal after `Character.toUpperCase`, or equal after `Character.toLowerCase` of those.
pub fn java_equals_ignore_case(first: &str, second: &str) -> bool {
    if first.is_ascii() && second.is_ascii() {
        // ASCII letters only case-map to ASCII letters.
        return first.eq_ignore_ascii_case(second);
    }
    if first.encode_utf16().count() != second.encode_utf16().count() {
        return false;
    }
    let mut second_chars = second.chars();
    first.chars().all(|a| {
        second_chars.next().is_some_and(|b| {
            let (upper_a, upper_b) = (java_to_upper(a), java_to_upper(b));
            a == b || upper_a == upper_b || java_to_lower(upper_a) == java_to_lower(upper_b)
        })
    }) && second_chars.next().is_none()
}

/// Java's `Character.toUpperCase(int)`, a simple (one code point) mapping. Rust's `to_uppercase`
/// is the full mapping; they agree wherever that is one code point, except for the Greek letters
/// with a iota subscript, whose simple uppercase is their titlecase. Checked against Java 26's
/// tables for every code point (Rust 1.95).
fn java_to_upper(c: char) -> char {
    let simple = match c {
        '\u{1f80}'..='\u{1f87}' | '\u{1f90}'..='\u{1f97}' | '\u{1fa0}'..='\u{1fa7}' => {
            u32::from(c) + 8
        }
        '\u{1fb3}' | '\u{1fc3}' | '\u{1ff3}' => u32::from(c) + 9,
        _ => return single_case_mapping(c.to_uppercase(), c),
    };
    char::from_u32(simple).unwrap_or(c)
}

/// Java's `Character.toLowerCase(int)`, as [`java_to_upper`]: only `İ` (U+0130) has a longer full
/// lowercase, and its simple one is `i`.
fn java_to_lower(c: char) -> char {
    if c == '\u{130}' {
        return 'i';
    }
    single_case_mapping(c.to_lowercase(), c)
}

fn single_case_mapping(mut mapped: impl Iterator<Item = char>, c: char) -> char {
    match (mapped.next(), mapped.next()) {
        (Some(mapped), None) => mapped,
        _ => c,
    }
}

/// Engine's `BiomeType` identities: the eleven climate types in Engine's order, with their ids
/// (the ordinals) and `isExtreme`. Their colours, temperature/moisture ranges and climate lookups
/// (Engine's type map, loaded from TerraForged's biome-type image) are in `biome_type.rs`.
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

    /// `getId`: the ordinal.
    pub const fn id(self) -> i32 {
        self as i32
    }

    /// `BiomeType.get(int)`. Panics where Engine throws: an id outside 0..=10.
    pub fn get(id: i32) -> Self {
        usize::try_from(id)
            .ok()
            .and_then(|id| Self::VALUES.get(id).copied())
            .unwrap_or_else(|| panic!("BiomeType id {id} out of range"))
    }

    pub const fn is_extreme(self) -> bool {
        matches!(self, Self::Tundra | Self::Desert)
    }
}

/// Engine's `Cell`: the per-column sample that the heightmap, region, climate, river and erosion
/// stages fill in.
///
/// Engine's default terrain is the static `TerrainType.NONE`; here a cell takes `none` from the
/// world's [`TerrainRegistry`] (so `is_same` against that registry's built-ins works), and
/// [`Cell::reset`] takes the same registry. As in Engine, `copy_from` and `reset` (which copies
/// Engine's defaults) leave `terrain_region_center` and the absent flag alone. `Clone` copies
/// every field. Engine's `Cell.empty()` is one shared, mutable object; [`Cell::empty`] returns a
/// new absent cell each time. Engine's per-thread cell pools (`getResource`) are not ported: Rust
/// callers own their cells.
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
    /// `new Cell()`, with `registry`'s `none` terrain.
    pub fn new(registry: &TerrainRegistry) -> Self {
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
            terrain: registry.get_by_id(0),
            biome: BiomeType::Grassland,
            absent: false,
        }
    }

    /// `Cell.empty()`: a default cell whose `is_absent` is true.
    pub fn empty(registry: &TerrainRegistry) -> Self {
        Self {
            absent: true,
            ..Self::new(registry)
        }
    }

    /// `copyFrom`: every field but `terrain_region_center` (and the absent flag, which is the
    /// object's class in Engine).
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

    /// `reset`: `copyFrom` the defaults, with `registry`'s `none` terrain.
    pub fn reset(&mut self, registry: &TerrainRegistry) -> &mut Self {
        self.copy_from(&Self::new(registry));
        self
    }

    pub const fn is_absent(&self) -> bool {
        self.absent
    }
}

/// Engine's `SpawnType`: spawn near the nearest continent's centre or near x = 0, z = 0.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpawnType {
    ContinentCenter,
    WorldOrigin,
}

/// Engine's `ContinentType`: the continent generator; `index` is Engine's `index` field (equal to the ordinal).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ContinentType {
    Multi = 0,
    Single = 1,
    MultiImproved = 2,
    Fancy = 3,
}

impl ContinentType {
    pub const VALUES: [Self; 4] = [Self::Multi, Self::Single, Self::MultiImproved, Self::Fancy];

    pub const fn index(self) -> i32 {
        self as i32
    }
}

/// Engine's `WorldSettings.Continent.continentShape`, a `DistanceFunc` (the noise layer's
/// `DistanceFunction`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContinentShape {
    Euclidean,
    Manhattan,
    Natural,
}

/// Engine's `WorldSettings.Continent`, with Engine's defaults. The serializer's `@Range` limits are not
/// enforced here.
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
    /// Engine's `WorldSettings.DEFAULT_CONTINENT_SCALE`.
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

/// Engine's `WorldSettings.Properties`.
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

/// Engine's `WorldSettings.ControlPoints`: the configured 0..1 thresholds, unchecked.
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

/// Engine's `WorldSettings` with its defaults (`seed` is `transient` in Engine: never serialized).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WorldSettings {
    pub seed: i64,
    pub continent: ContinentSettings,
    pub control_points: WorldControlPoints,
    pub properties: WorldProperties,
}

/// Engine's `RiverSettings`, with Engine's defaults (the serializer's `@Range` limits are not enforced).
/// TerraForged 0.3.1 keeps these defaults but never uses them: its own continent noise carves rivers with its
/// own config instead of Engine's river map.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiverSettings {
    pub seed_offset: i32,
    pub river_count: i32,
    pub main_rivers: River,
    pub branch_rivers: River,
    pub lakes: Lake,
    pub wetlands: Wetland,
}

impl Default for RiverSettings {
    fn default() -> Self {
        Self {
            seed_offset: 0,
            river_count: 8,
            main_rivers: River::new(5, 2, 6, 20, 8, 0.75),
            branch_rivers: River::new(4, 1, 4, 14, 5, 0.975),
            lakes: Lake::default(),
            wetlands: Wetland::default(),
        }
    }
}

/// Engine's `RiverSettings.River`; `Default` is Engine's no-argument constructor (all zero).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct River {
    pub bed_depth: i32,
    pub min_bank_height: i32,
    pub max_bank_height: i32,
    pub bed_width: i32,
    pub bank_width: i32,
    pub fade: f32,
}

impl River {
    /// Engine's `River(depth, minBank, maxBank, outer, inner, fade)`: `outer` is the bank width, `inner` the
    /// bed width.
    pub const fn new(
        depth: i32,
        min_bank: i32,
        max_bank: i32,
        outer: i32,
        inner: i32,
        fade: f32,
    ) -> Self {
        Self {
            bed_depth: depth,
            min_bank_height: min_bank,
            max_bank_height: max_bank,
            bed_width: inner,
            bank_width: outer,
            fade,
        }
    }
}

impl Default for River {
    fn default() -> Self {
        Self::new(0, 0, 0, 0, 0, 0.0)
    }
}

/// Engine's `RiverSettings.Lake`, with Engine's defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lake {
    pub chance: f32,
    pub min_start_distance: f32,
    pub max_start_distance: f32,
    pub depth: i32,
    pub size_min: i32,
    pub size_max: i32,
    pub min_bank_height: i32,
    pub max_bank_height: i32,
}

impl Default for Lake {
    fn default() -> Self {
        Self {
            chance: 0.3,
            min_start_distance: 0.0,
            max_start_distance: 0.03,
            depth: 10,
            size_min: 75,
            size_max: 150,
            min_bank_height: 2,
            max_bank_height: 10,
        }
    }
}

/// Engine's `RiverSettings.Wetland`, with Engine's defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wetland {
    pub chance: f32,
    pub size_min: i32,
    pub size_max: i32,
}

impl Default for Wetland {
    fn default() -> Self {
        Self {
            chance: 0.6,
            size_min: 175,
            size_max: 225,
        }
    }
}

/// Engine's `TerrainSettings.Terrain`: one landform's weight and its base, vertical and
/// horizontal scales. Engine's `apply` (scale, bias, then `TerrainPopulator.clamp`) is not ported.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainFormSettings {
    pub weight: f32,
    pub base_scale: f32,
    pub vertical_scale: f32,
    pub horizontal_scale: f32,
}

impl TerrainFormSettings {
    /// Engine's `Terrain(weight, vertical, horizontal)`; `base_scale` keeps its default of 1.
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

/// Engine's `TerrainSettings.General` with its defaults.
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

/// Engine's `TerrainSettings` with its defaults. TerraForged's `ModTerrains` builds its landforms
/// with a global vertical scale of 1 (and without fancy mountains for the ridge variants) and
/// weights them with its own values, not these weights.
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

/// Engine's heightmap `ControlPoints`: the configured points if [`ControlPoints::validate`]
/// accepts them, else the defaults, plus `coast_marker` halfway from coast to inland.
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

/// Engine's `Levels`: block heights, counted from block 0, and their 0..1 heightmap values.
/// Java's overloads are named apart: `scale(float)` is [`Levels::scale_value`], `scale(int)`
/// [`Levels::scale_level`], `elevation(float)` [`Levels::elevation_value`] and `elevation(int)`
/// [`Levels::elevation_y`].
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
            self.scale_level(y.wrapping_sub(self.water_y)) / self.elevation_range
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

    /// A cell's fields as Engine's `Cell` oracle prints them: float bits, ints, the terrain id and
    /// whether it is the registry's `none`, the biome and the absent flag.
    fn cell_fields(cell: &Cell, registry: &TerrainRegistry) -> String {
        let floats = [
            cell.value,
            cell.erosion,
            cell.sediment,
            cell.gradient,
            cell.moisture,
            cell.temperature,
            cell.continent_id,
            cell.continent_edge,
            cell.terrain_region_id,
            cell.terrain_region_edge,
        ];
        let more = [
            cell.biome_region_id,
            cell.biome_region_edge,
            cell.macro_biome_id,
            cell.river_mask,
        ];
        let hex = |values: &[f32]| {
            values
                .iter()
                .map(|v| format!("{:08x}", v.to_bits()))
                .collect::<Vec<_>>()
                .join(" ")
        };
        format!(
            "{} {} {} {} {} {} {}:{} {:?} {}",
            hex(&floats),
            cell.terrain_region_center,
            hex(&more),
            cell.continent_x,
            cell.continent_z,
            cell.erosion_mask,
            cell.terrain.id(),
            cell.terrain.is_same(&registry.get_by_id(0)),
            cell.biome,
            cell.is_absent()
        )
    }

    #[test]
    fn cell_model_matches_engine_0_3_0() {
        assert_eq!(
            BiomeType::VALUES.map(BiomeType::id),
            [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
        );
        for biome in BiomeType::VALUES {
            assert_eq!(BiomeType::get(biome.id()), biome);
            assert_eq!(
                biome.is_extreme(),
                matches!(biome, BiomeType::Desert | BiomeType::Tundra)
            );
        }
        assert!(std::panic::catch_unwind(|| BiomeType::get(11)).is_err());
        assert!(std::panic::catch_unwind(|| BiomeType::get(-1)).is_err());

        // Engine 0.3.0's Cell, printed by the oracle kept outside Git: a default cell and a cell
        // copied from `source` below, each with a given terrainRegionCenter and absent flag.
        let default = |center: i64, absent: bool| {
            format!(
                "00000000 00000000 00000000 00000000 3f000000 3f000000 00000000 00000000 00000000 \
                 00000000 {center} 00000000 3f800000 00000000 3f800000 0 0 false 0:true Grassland \
                 {absent}"
            )
        };
        let copied = |center: i64, absent: bool| {
            format!(
                "3f400000 3e800000 3e000000 3f600000 3e4ccccd 3f4ccccd 3dcccccd 3f666666 3e99999a \
                 3f333333 {center} 3ecccccd 3f19999a 3f0ccccd 3ee66666 -123 456 true 12:false Alpine \
                 {absent}"
            )
        };

        let registry = TerrainRegistry::new();
        assert_eq!(
            cell_fields(&Cell::new(&registry), &registry),
            default(0, false)
        );

        let mut source = Cell::new(&registry);
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
        source.terrain = registry.get_by_id(12);
        source.biome = BiomeType::Alpine;

        // copyFrom keeps the target's terrainRegionCenter; reset (copyFrom the defaults) too.
        let mut copied_cell = Cell::new(&registry);
        copied_cell.terrain_region_center = -99;
        copied_cell.copy_from(&source);
        assert_eq!(cell_fields(&copied_cell, &registry), copied(-99, false));
        assert!(copied_cell.terrain.is_same(&source.terrain));
        copied_cell.reset(&registry);
        assert_eq!(cell_fields(&copied_cell, &registry), default(-99, false));

        // The empty cell stays absent through copyFrom and reset; copying it does not make another
        // cell absent.
        let mut empty = Cell::empty(&registry);
        assert_eq!(cell_fields(&empty, &registry), default(0, true));
        empty.copy_from(&source);
        assert_eq!(cell_fields(&empty, &registry), copied(0, true));
        empty.reset(&registry);
        assert_eq!(cell_fields(&empty, &registry), default(0, true));
        let mut from_empty = Cell::new(&registry);
        from_empty.copy_from(&Cell::empty(&registry));
        assert_eq!(cell_fields(&from_empty, &registry), default(0, false));
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
    fn category_ordinals_and_dominance_match_engine_0_3_0() {
        // EngineTypesOracle: TerrainCategory.values()[i].ordinal() and, row by row,
        // values()[i].getDominant(values()[j]).ordinal().
        for (ordinal, category) in TerrainCategory::VALUES.into_iter().enumerate() {
            assert_eq!(category as usize, ordinal, "{category:?}");
        }
        for first in TerrainCategory::VALUES {
            for second in TerrainCategory::VALUES {
                let expected = (first as usize).max(second as usize);
                assert_eq!(
                    first.get_dominant(second),
                    TerrainCategory::VALUES[expected]
                );
            }
        }
    }

    #[test]
    fn terrain_property_defaults_match_engine_0_3_0() {
        // ITerrain's defaults derive isSubmerged, overridesRiver and overridesCoast from the
        // other flags; a type overriding only one flag inherits the derived ones.
        struct Only(u8);
        impl TerrainProperties for Only {
            fn is_river(&self) -> bool {
                self.0 == 1
            }
            fn is_shallow_ocean(&self) -> bool {
                self.0 == 2
            }
            fn is_deep_ocean(&self) -> bool {
                self.0 == 3
            }
            fn is_coast(&self) -> bool {
                self.0 == 4
            }
            fn is_lake(&self) -> bool {
                self.0 == 5
            }
            fn is_volcano(&self) -> bool {
                self.0 == 6
            }
        }
        // (flag, isSubmerged, overridesRiver, overridesCoast)
        let expected = [
            (0, false, false, false),
            (1, true, false, false),
            (2, true, true, false),
            (3, true, true, false),
            (4, false, true, false),
            (5, true, false, false),
            (6, false, false, true),
        ];
        for (flag, submerged, river, coast) in expected {
            let terrain = Only(flag);
            assert_eq!(terrain.is_submerged(), submerged, "{flag}");
            assert_eq!(terrain.overrides_river(), river, "{flag}");
            assert_eq!(terrain.overrides_coast(), coast, "{flag}");
            assert!(!terrain.is_overground() && !terrain.is_flat() && !terrain.is_mountain());
            assert_eq!(terrain.erosion_modifier().to_bits(), 0x3f80_0000);
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
    fn terrain_descriptor_edges_match_engine_0_3_0() {
        // TerrainDescriptorsOracle (.local/terraforged/oracles/terrain_descriptors/): withId on
        // configured, volcanic and composite terrains, Math.min's zero and NaN rules in composites,
        // nested composites and Java string hashes of empty and long names.
        use TerrainCategory::{DeepOcean, Flatland, Highland, River};
        let river = Terrain::new(7, "river_custom", River);
        let child = Terrain::with_parent(8, "child", river.clone());
        let erosion = Terrain::configured_erosion(10, "erosion", Flatland, 0.3);
        let mountain = Terrain::configured_mountain(12, "mountain", Highland, true, true);
        let full = Terrain::configured(13, "full", TerrainCategory::Coast, 0.625, true, false);
        let ab = Terrain::composite(14, erosion.clone(), mountain.clone());
        let volcano = Terrain::volcano(16, "volcano", Highland);
        let neg = Terrain::configured_erosion(17, "neg", Flatland, -0.0);
        let pos = Terrain::configured_erosion(18, "pos", Flatland, 0.0);
        let nan = Terrain::configured_erosion(19, "nan", Flatland, f32::NAN);
        let flats = Terrain::new(20, "flats", Flatland);
        let cases = [
            (
                erosion.clone(),
                10,
                "erosion",
                Flatland,
                0x3ef37902,
                0x3e99999a,
                0x041,
            ),
            (
                mountain.clone(),
                12,
                "mountain",
                Highland,
                0x3f5a91ad,
                0x3f800000,
                0x8c0,
            ),
            (
                volcano.clone(),
                16,
                "volcano",
                Highland,
                0x3ee7697b,
                0x3f800000,
                0x19c0,
            ),
            (
                river.with_id(98),
                98,
                "river_custom",
                River,
                0xbf7959fa,
                0x3f800000,
                0x022,
            ),
            (
                full.with_id(97),
                97,
                "full",
                TerrainCategory::Coast,
                0xbf354c07,
                0x3f200000,
                0x850,
            ),
            (
                volcano.with_id(96),
                96,
                "volcano",
                Highland,
                0x3ee7697b,
                0x3f800000,
                0x19c0,
            ),
            // withId keeps a composite's first terrain as its delegate and drops its own flat flag.
            (
                ab.with_id(95),
                95,
                "erosion-mountain",
                Flatland,
                0xbf6edf09,
                0x3e99999a,
                0x041,
            ),
            (
                Terrain::composite(30, pos.clone(), neg.clone()),
                30,
                "pos-neg",
                Flatland,
                0xbe480657,
                0x80000000,
                0x041,
            ),
            (
                Terrain::composite(31, neg, pos),
                31,
                "neg-pos",
                Flatland,
                0x3f00abc6,
                0x80000000,
                0x041,
            ),
            (
                Terrain::composite(32, flats.clone(), nan),
                32,
                "flats-nan",
                Flatland,
                0x3f23d704,
                0x7fc00000,
                0x041,
            ),
            (
                Terrain::composite(33, flats, Terrain::new(34, "flats2", Flatland)),
                33,
                "flats-flats2",
                Flatland,
                0x3e4397f0,
                0x3f800000,
                0x041,
            ),
            (
                Terrain::composite(35, ab.clone(), volcano.clone()),
                35,
                "erosion-mountain-volcano",
                Flatland,
                0x3f601d24,
                0x3e99999a,
                0x040,
            ),
            (
                Terrain::composite(36, Terrain::new(37, "deep", DeepOcean), volcano),
                36,
                "deep-volcano",
                DeepOcean,
                0xbf728b69,
                0x3f800000,
                0x0a8,
            ),
            (
                Terrain::new(21, "", TerrainCategory::None),
                21,
                "",
                TerrainCategory::None,
                0x00000000,
                0x3f800000,
                0x000,
            ),
            (
                Terrain::new(22, "mountain_chain-volcano_pipe-deep_ocean", Highland),
                22,
                "mountain_chain-volcano_pipe-deep_ocean",
                Highland,
                0xbf2c32b2,
                0x3f800000,
                0x040,
            ),
        ];
        for (terrain, id, name, category, hue, erosion, properties) in cases {
            assert_eq!(terrain.id(), id, "{terrain:?}");
            assert_eq!(terrain.name(), name);
            assert_eq!(terrain.category(), category, "{terrain:?}");
            assert_eq!(terrain.render_hue().to_bits(), hue, "{terrain:?}");
            assert_eq!(terrain.erosion_modifier().to_bits(), erosion, "{terrain:?}");
            assert_eq!(property_bits(&terrain), properties, "{terrain:?}");
        }

        // Identity: withId delegates to the parent of a delegated terrain, else to itself.
        let delegate = |terrain: &Terrain| match &terrain.0.delegate {
            TerrainDelegate::Terrain(parent) => parent.clone(),
            TerrainDelegate::Category(_) => panic!("{terrain:?} delegates to its category"),
        };
        assert!(delegate(&child.with_id(1).with_id(2)).is_same(&river));
        assert!(delegate(&river.with_id(3)).is_same(&river));
        assert!(delegate(&ab.with_id(4)).is_same(&erosion));
        assert!(river.clone().is_same(&river));
        assert!(!river.with_id(7).is_same(&river));
        assert!(!Terrain::new(7, "river_custom", River).is_same(&river));

        // Terrains are shared by world-generation threads.
        fn send_sync<T: Send + Sync>() {}
        send_sync::<Terrain>();
    }

    /// One registered terrain as TerrainRegistryOracle prints it: id, name, category, hue bits,
    /// erosion bits and property bits.
    type Row = (i32, &'static str, TerrainCategory, u32, u32, u16);

    fn assert_row(terrain: &Terrain, row: Row) {
        let (id, name, category, hue, erosion, properties) = row;
        assert_eq!(terrain.id(), id, "{terrain:?}");
        assert_eq!(terrain.name(), name);
        assert_eq!(terrain.category(), category, "{terrain:?}");
        assert_eq!(terrain.render_hue().to_bits(), hue, "{terrain:?}");
        assert_eq!(terrain.erosion_modifier().to_bits(), erosion, "{terrain:?}");
        assert_eq!(property_bits(terrain), properties, "{terrain:?}");
    }

    const BUILT_INS: [Row; 16] = {
        use TerrainCategory::*;
        [
            (0, "none", None, 0xbee66da8, 0x3f800000, 0x000),
            (1, "deep_ocean", DeepOcean, 0x3ba86a77, 0x3f800000, 0x0a8),
            (2, "ocean", ShallowOcean, 0xbef0c69a, 0x3f800000, 0x0a4),
            (3, "coast", Coast, 0xbf14bc2f, 0x3f800000, 0x0d0),
            (4, "beach", Beach, 0xbf22122b, 0x3f800000, 0x0d0),
            (5, "river", River, 0x3cffd9f0, 0x3f800000, 0x022),
            (6, "lake", Lake, 0x3f4c300b, 0x3f800000, 0x220),
            (7, "wetland", Wetland, 0xbf605aad, 0x3f800000, 0x4c0),
            (8, "flats", Flatland, 0x3efbff1a, 0x3f800000, 0x041),
            (9, "badlands", Flatland, 0x3f1fa6c2, 0x3e99999a, 0x041),
            (10, "plateau", Lowland, 0x3f09beec, 0x3f800000, 0x040),
            (11, "hills", Lowland, 0xbea446db, 0x3f800000, 0x040),
            (12, "mountains", Highland, 0x3eb6130c, 0x3f800000, 0x8c0),
            (
                13,
                "mountain_chain",
                Highland,
                0xbb95b75d,
                0x3f800000,
                0x8c0,
            ),
            (14, "volcano", Highland, 0x3ee7697b, 0x3f800000, 0x19c0),
            (15, "volcano_pipe", Highland, 0xbe0851fc, 0x3f800000, 0x19c0),
        ]
    };

    #[test]
    fn default_terrain_registry_matches_engine_0_3_0() {
        // TerrainRegistryOracle (.local/terraforged/oracles/terrain_registry/), a fresh JVM.
        let registry = TerrainRegistry::new();
        assert_eq!(registry.terrains().len(), BUILT_INS.len());
        for (terrain, row) in registry.terrains().iter().zip(BUILT_INS) {
            assert_row(terrain, row);
        }
        let none = registry.get_by_id(0);
        for id in [-1, 16, 999, i32::MIN] {
            assert!(registry.get_by_id(id).is_same(&none), "{id}");
        }
        assert!(registry.get_by_id(15).is_same(&registry.terrains()[15]));
        assert_eq!(registry.get_by_name("OcEaN").unwrap().id(), 2);
        assert!(registry.get_by_name("ocean ").is_none());
        assert!(registry.get_by_name("").is_none());
        let hills = registry.find(|terrain| terrain.name() == "hills");
        assert_eq!(hills.unwrap().id(), 11);

        // Each world owns its registry; registries are shared by world-generation threads.
        assert!(!TerrainRegistry::new().get_by_id(0).is_same(&none));
        fn send_sync<T: Send + Sync>() {}
        send_sync::<TerrainRegistry>();
    }

    #[test]
    fn terrain_registration_matches_engine_0_3_0() {
        // TerrainRegistryOracle: register, registerComposite and TerrainHelper.getOrCreate in one
        // registry, in this order.
        use TerrainCategory::{Flatland, Highland, Lowland, None, River};
        let mut registry = TerrainRegistry::new();
        let builtin = |registry: &TerrainRegistry, id: i32| registry.get_by_id(id);
        let hills = builtin(&registry, 11);
        let custom = registry.register(Terrain::with_parent(999, "Custom", hills.clone()));
        assert_row(
            &custom,
            (16, "Custom", Lowland, 0xbf380ad4, 0x3f800000, 0x040),
        );
        assert!(
            matches!(&custom.0.delegate, TerrainDelegate::Terrain(parent) if parent.is_same(&hills))
        );
        let duplicate = registry.register(Terrain::new(999, "cUsToM", River));
        assert!(duplicate.is_same(&custom));
        // register() re-ids with withId, which delegates to the configured terrain itself.
        let badlands2 =
            registry.register(Terrain::configured_erosion(5, "badlands2", Flatland, 0.3));
        assert_row(
            &badlands2,
            (17, "badlands2", Flatland, 0x3e79820d, 0x3e99999a, 0x041),
        );

        let badlands = builtin(&registry, 9);
        let composite = registry.register_composite(&custom, &badlands);
        let composite_row = (
            18,
            "badlands-Custom",
            Flatland,
            0xbf336802,
            0x3e99999a,
            0x040,
        );
        assert_row(&composite, composite_row);
        assert!(
            registry
                .register_composite(&badlands, &custom)
                .is_same(&composite)
        );
        assert!(
            registry
                .register_composite(&custom, &custom)
                .is_same(&custom)
        );
        assert!(
            registry
                .get_by_name("BADLANDS-custom")
                .unwrap()
                .is_same(&composite)
        );
        // Distinct terrains with the same id: Engine's min and max are both the second terrain.
        let twin = Terrain::new(9, "twin", Flatland);
        let twin_twin = registry.register_composite(&badlands, &twin);
        assert_row(
            &twin_twin,
            (19, "twin-twin", Flatland, 0x3f0beb34, 0x3f800000, 0x041),
        );
        let badlands_badlands = registry.register_composite(&twin, &badlands);
        let row = (
            20,
            "badlands-badlands",
            Flatland,
            0xbf10c48b,
            0x3e99999a,
            0x041,
        );
        assert_row(&badlands_badlands, row);
        let volcano = builtin(&registry, 14);
        let nested = registry.register_composite(&composite, &volcano);
        let row = (
            21,
            "volcano-badlands-Custom",
            Highland,
            0xbf01e800,
            0x3e99999a,
            0x19c0,
        );
        assert_row(&nested, row);

        let none = builtin(&registry, 0);
        assert!(
            registry
                .get_or_create("null_child", Option::None)
                .is_same(&none)
        );
        assert!(
            registry
                .get_or_create("none_child", Some(none.clone()))
                .is_same(&none)
        );
        let river = builtin(&registry, 5);
        assert!(
            registry
                .get_or_create("CUSTOM", Some(river))
                .is_same(&custom)
        );
        let child = registry.get_or_create("child", Some(custom.clone()));
        assert_row(
            &child,
            (22, "child", Lowland, 0xbf58360a, 0x3f800000, 0x040),
        );
        assert!(
            matches!(&child.0.delegate, TerrainDelegate::Terrain(parent) if parent.is_same(&custom))
        );
        // A copy of `none` is not `none` (Engine compares with ==).
        let none_copy = registry.get_or_create("none_copy", Some(none.with_id(0)));
        assert_row(
            &none_copy,
            (23, "none_copy", None, 0xbf2f1dea, 0x3f800000, 0x000),
        );

        // Names compare like Java's equalsIgnoreCase: Unicode simple case mappings, one code
        // point at a time, so `ß` never matches `SS`.
        let names = [
            "Straße", "Kelvin", "ſtone", "izmir", "irmak", "ᾳ", "𐐀bc", "σας", "ǅ", "µ",
        ];
        let hues = [
            0xbde1adc4, 0x3e455dad, 0xbf03a805, 0xbf23fbcc, 0xbef81c13, 0xbe06d705, 0x3e2de400,
            0xbf39f238, 0x3f1aae18, 0xbf76b202,
        ];
        for (index, (name, hue)) in names.into_iter().zip(hues).enumerate() {
            let terrain = registry.register(Terrain::new(0, name, Lowland));
            assert_row(
                &terrain,
                (24 + index as i32, name, Lowland, hue, 0x3f800000, 0x040),
            );
        }
        let lookups = [
            ("STRASSE", -1),
            ("STRAßE", 24),
            ("\u{212a}elvin", 25),
            ("STONE", 26),
            ("İZMIR", 27),
            ("IRMAK", 28),
            ("ırmak", 28),
            ("ᾼ", 29),
            ("ΑΙ", -1),
            ("𐐨BC", 30),
            ("ΣΑΣ", 31),
            ("Ǆ", 32),
            ("Μ", 33),
            ("μ", 33),
        ];
        for (query, id) in lookups {
            let found = registry
                .get_by_name(query)
                .map_or(-1, |terrain| terrain.id());
            assert_eq!(found, id, "{query}");
        }

        // The final registry, in id order.
        let rows = [
            composite_row,
            (19, "twin-twin", Flatland, 0x3f0beb34, 0x3f800000, 0x041),
            (
                20,
                "badlands-badlands",
                Flatland,
                0xbf10c48b,
                0x3e99999a,
                0x041,
            ),
        ];
        assert_eq!(registry.terrains().len(), 34);
        for (terrain, row) in registry.terrains().iter().zip(BUILT_INS) {
            assert_row(terrain, row);
        }
        for (terrain, row) in registry.terrains()[18..].iter().zip(rows) {
            assert_row(terrain, row);
        }
        for (id, terrain) in registry.terrains().iter().enumerate() {
            assert_eq!(terrain.id(), id as i32);
            assert!(registry.get_by_id(id as i32).is_same(terrain));
        }
    }

    #[test]
    fn java_equals_ignore_case_matches_java_26() {
        // Java 26's String.equalsIgnoreCase. The oracle folder's CasePairs/CaseDump check (with
        // jcase.rs) also compares every code point's simple case mappings and every pair of code
        // points that share one (6,134 pairs).
        let cases = [
            ("OcEaN", "ocean", true),
            ("\u{212a}", "k", true),
            ("ſ", "S", true),
            ("İ", "i", true),
            ("ı", "I", true),
            ("ı", "i", true),
            ("ᾳ", "ᾼ", true),
            ("ᾀ", "ᾈ", true),
            ("ᾳ", "ΑΙ", false),
            ("ß", "SS", false),
            ("ﬀ", "FF", false),
            ("ς", "Σ", true),
            ("ǅ", "ǆ", true),
            ("µ", "Μ", true),
            ("ÿ", "Ÿ", true),
            ("𐐀bc", "𐐨BC", true),
            ("a𐐀", "𐐨a", false),
            ("e\u{301}", "É", false),
            ("é", "É", true),
            ("", "", true),
            ("ab", "a", false),
        ];
        for (first, second, equal) in cases {
            assert_eq!(
                java_equals_ignore_case(first, second),
                equal,
                "{first} {second}"
            );
            assert_eq!(
                java_equals_ignore_case(second, first),
                equal,
                "{second} {first}"
            );
        }
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
        // -0.0 passes `deepOcean >= 0` and is kept.
        let negative_zero = WorldControlPoints {
            deep_ocean: -0.0,
            ..valid
        };
        assert_eq!(
            bits_of_control_points(ControlPoints::new(negative_zero)),
            [
                0x80000000, 0x3e4ccccd, 0x3e99999a, 0x3f19999a, 0x3f4ccccd, 0x3f800000,
            ]
        );
        // The points TerraForged's continent noise sets.
        let continent = WorldControlPoints {
            deep_ocean: 0.05,
            shallow_ocean: 0.3,
            beach: 0.45,
            coast: 0.75,
            inland: 0.8,
        };
        assert_eq!(
            bits_of_control_points(ControlPoints::new(continent)),
            [
                0x3d4ccccd, 0x3e99999a, 0x3ee66666, 0x3f400000, 0x3f466666, 0x3f4ccccd,
            ]
        );
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

    struct LevelsCase {
        input: (i32, i32),
        /// `worldHeight`, `unit`, `waterY`, `groundLevel`, `ground`, `water`.
        fields: (i32, u32, i32, i32, u32, u32),
        scale_value: [i32; 12],
        elevation_value: [u32; 12],
        elevation_y: [u32; 8],
        scale_level: [u32; 8],
        water: [u32; 8],
        ground: [u32; 8],
    }

    #[test]
    fn levels_match_java_oracle() {
        // Printed by `new Levels(height, seaLevel)` of Engine 0.3.0 (`.local/terraforged/oracles/levels/`),
        // including TerraForged's default 480/62, 320 and 384 block heights, `int` overflow of the
        // sea level and of `y - waterY`, and heights past 2^24.
        #[rustfmt::skip]
        let cases = [
            LevelsCase {
                input: (256, 63),
                fields: (256, 0x3b800000, 62, 64, 0x3e7c0000, 0x3e780000),
                scale_value: [-256, 0, 64, 62, 192, 255, 0, 255, i32::MIN, 255, 0, 255],
                elevation_value: [0x00000000, 0x00000000, 0x3c28e83f, 0x00000000, 0x3f2b8be0, 0x3f800000, 0x7fc00000, 0x7f800000, 0x00000000, 0x3f7ffffd, 0x00000000, 0x71853e8b],
                elevation_y: [0x00000000, 0x00000000, 0x3ba8e83f, 0x3f800000, 0x3f9d07eb, 0x4b28e83f, 0x00000000, 0x00000000],
                scale_level: [0xbd200000, 0x3e780000, 0x3e7c0000, 0x3f800000, 0x3f960000, 0x4b000000, 0xcb000000, 0x00000000],
                water: [0x3e500000, 0x3ef80000, 0x3efa0000, 0x3f9f0000, 0x3fb50000, 0xcb000000, 0xcb000000, 0x3e780000],
                ground: [0x3e540000, 0x3efa0000, 0x3efc0000, 0x3f9f8000, 0x3fb58000, 0xcb000000, 0xcb000000, 0x3e7c0000],
            },
            LevelsCase {
                input: (0, 63),
                fields: (1, 0x3f800000, 1, 64, 0x3f800000, 0x3f800000),
                scale_value: [-1, 0, 0, 0, 0, 0, 0, 0, i32::MIN, 0, 0, 0],
                elevation_value: [0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x7fc00000, 0x7f800000, 0x00000000, 0x00000000, 0x00000000, 0x7f800000],
                elevation_y: [0x00000000, 0x7f800000, 0x7f800000, 0x7f800000, 0x7f800000, 0x7f800000, 0x00000000, 0x00000000],
                scale_level: [0xc1200000, 0x42780000, 0x427c0000, 0x43800000, 0x43960000, 0x4f000000, 0xcf000000, 0x00000000],
                water: [0xc1100000, 0x427c0000, 0x42800000, 0x43808000, 0x43968000, 0xcf000000, 0xcf000000, 0x3f800000],
                ground: [0xc1100000, 0x427c0000, 0x42800000, 0x43808000, 0x43968000, 0xcf000000, 0xcf000000, 0x3f800000],
            },
            LevelsCase {
                input: (-10, -5),
                fields: (1, 0x3f800000, -6, -4, 0xc0a00000, 0xc0c00000),
                scale_value: [-1, 0, 0, -6, 0, 0, 0, 0, i32::MIN, 0, 0, 0],
                elevation_value: [0x3f36db6e, 0x3f5b6db7, 0x3f649249, 0x00000000, 0x3f76db6e, 0x3f800000, 0x7fc00000, 0x7f800000, 0x00000000, 0x3f800000, 0x3f5b6db7, 0x6fe6cc55],
                elevation_y: [0x00000000, 0x411b6db7, 0x411db6db, 0x4215b6db, 0x422edb6e, 0xcd924925, 0x00000000, 0x3f5b6db7],
                scale_level: [0xc1200000, 0x42780000, 0x427c0000, 0x43800000, 0x43960000, 0x4f000000, 0xcf000000, 0x00000000],
                water: [0xc1800000, 0x42600000, 0x42640000, 0x437a0000, 0x43930000, 0x4f000000, 0x4f000000, 0xc0c00000],
                ground: [0xc1700000, 0x42640000, 0x42680000, 0x437b0000, 0x43938000, 0x4f000000, 0x4f000000, 0xc0a00000],
            },
            LevelsCase {
                input: (100, 200),
                fields: (100, 0x3c23d70a, 100, 201, 0x3f800000, 0x3f800000),
                scale_value: [-100, 0, 25, 99, 75, 99, 0, 99, i32::MIN, 99, 0, 99],
                elevation_value: [0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x7fc00000, 0x7f800000, 0x00000000, 0x00000000, 0x00000000, 0x7f800000],
                elevation_y: [0x00000000, 0x00000000, 0x00000000, 0x7f800000, 0x7f800000, 0x7f800000, 0x00000000, 0x00000000],
                scale_level: [0xbdcccccd, 0x3f1eb852, 0x3f2147ae, 0x4023d70a, 0x40400000, 0x4ba3d70a, 0xcba3d70a, 0x00000000],
                water: [0x3f666666, 0x3fcf5c29, 0x3fd0a3d7, 0x4063d70a, 0x40800000, 0xcba3d70a, 0xcba3d70a, 0x3f800000],
                ground: [0x3f666666, 0x3fcf5c29, 0x3fd0a3d7, 0x4063d70a, 0x40800000, 0xcba3d70a, 0xcba3d70a, 0x3f800000],
            },
            LevelsCase {
                input: (320, 63),
                fields: (320, 0x3b4ccccd, 62, 64, 0x3e49999a, 0x3e466666),
                scale_value: [-320, 0, 80, 62, 240, 319, 0, 319, i32::MIN, 319, 0, 319],
                elevation_value: [0x00000000, 0x00000000, 0x3d8ee23d, 0x00000000, 0x3f309ec2, 0x3f800000, 0x7fc00000, 0x7f800000, 0x00000000, 0x3f7ffffe, 0x00000000, 0x717a7a88],
                elevation_y: [0x00000000, 0x00000000, 0x3b7e03f9, 0x3f407f02, 0x3f6c27b1, 0x4afe03f9, 0x00000000, 0x00000000],
                scale_level: [0xbd000000, 0x3e466666, 0x3e49999a, 0x3f4ccccd, 0x3f700000, 0x4acccccd, 0xcacccccd, 0x00000000],
                water: [0x3e266666, 0x3ec66666, 0x3ec80000, 0x3f7e6666, 0x3f90cccd, 0xcacccccd, 0xcacccccd, 0x3e466666],
                ground: [0x3e29999a, 0x3ec80000, 0x3ec9999a, 0x3f7f3333, 0x3f913333, 0xcacccccd, 0xcacccccd, 0x3e49999a],
            },
            LevelsCase {
                input: (384, 63),
                fields: (384, 0x3b2aaaab, 62, 64, 0x3e280000, 0x3e255555),
                scale_value: [-384, 0, 96, 62, 288, 383, 0, 383, i32::MIN, 383, 0, 383],
                elevation_value: [0x00000000, 0x00000000, 0x3dd83f9b, 0x00000000, 0x3f33ad51, 0x3f800000, 0x7fc00000, 0x7f800000, 0x00000000, 0x3f7ffffe, 0x00000000, 0x7170d536],
                elevation_y: [0x00000000, 0x00000000, 0x3b4b8728, 0x3f1a3c6b, 0x3f3d37a7, 0x4acb8728, 0x00000000, 0x00000000],
                scale_level: [0xbcd55555, 0x3e255555, 0x3e280000, 0x3f2aaaab, 0x3f480000, 0x4aaaaaab, 0xcaaaaaab, 0x00000000],
                water: [0x3e0aaaab, 0x3ea55555, 0x3ea6aaab, 0x3f540000, 0x3f715555, 0xcaaaaaab, 0xcaaaaaab, 0x3e255555],
                ground: [0x3e0d5555, 0x3ea6aaab, 0x3ea80000, 0x3f54aaab, 0x3f720000, 0xcaaaaaab, 0xcaaaaaab, 0x3e280000],
            },
            LevelsCase {
                input: (480, 62),
                fields: (480, 0x3b088889, 61, 63, 0x3e044444, 0x3e022222),
                scale_value: [-480, 0, 120, 61, 360, 479, 0, 479, i32::MIN, 479, 0, 479],
                elevation_value: [0x00000000, 0x00000000, 0x3e1030e1, 0x00000000, 0x3f36aebe, 0x3f800000, 0x7fc00000, 0x7f800000, 0x00000000, 0x3f7ffffe, 0x00000000, 0x71675957],
                elevation_y: [0x00000000, 0x3b1c6917, 0x3b9c6917, 0x3eee4818, 0x3f12061c, 0x4a9c6917, 0x00000000, 0x00000000],
                scale_level: [0xbcaaaaab, 0x3e044444, 0x3e066666, 0x3f088889, 0x3f200000, 0x4a888889, 0xca888889, 0x00000000],
                water: [0x3dd9999a, 0x3e833333, 0x3e844444, 0x3f291111, 0x3f408889, 0xca888889, 0xca888889, 0x3e022222],
                ground: [0x3dddddde, 0x3e844444, 0x3e855555, 0x3f29999a, 0x3f411111, 0xca888889, 0xca888889, 0x3e044444],
            },
            LevelsCase {
                input: (256, i32::MAX),
                fields: (256, 0x3b800000, 256, i32::MIN, 0x3f800000, 0x3f800000),
                scale_value: [-256, 0, 64, 255, 192, 255, 0, 255, i32::MIN, 255, 0, 255],
                elevation_value: [0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x7fc00000, 0x7f800000, 0x00000000, 0x00000000, 0x00000000, 0x7f800000],
                elevation_y: [0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x7f800000, 0x7f800000, 0x00000000, 0x00000000],
                scale_level: [0xbd200000, 0x3e780000, 0x3e7c0000, 0x3f800000, 0x3f960000, 0x4b000000, 0xcb000000, 0x00000000],
                water: [0x3f760000, 0x3f9f0000, 0x3f9f8000, 0x40000000, 0x400b0000, 0xcafffffe, 0xcafffffe, 0x3f800000],
                ground: [0x3f760000, 0x3f9f0000, 0x3f9f8000, 0x40000000, 0x400b0000, 0xcafffffe, 0xcafffffe, 0x3f800000],
            },
            LevelsCase {
                input: (256, i32::MIN),
                fields: (256, 0x3b800000, 256, -2147483647, 0xcb000000, 0x3f800000),
                scale_value: [-256, 0, 64, 255, 192, 255, 0, 255, i32::MIN, 255, 0, 255],
                elevation_value: [0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x7fc00000, 0x7f800000, 0x00000000, 0x00000000, 0x00000000, 0x7f800000],
                elevation_y: [0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x7f800000, 0x7f800000, 0x00000000, 0x00000000],
                scale_level: [0xbd200000, 0x3e780000, 0x3e7c0000, 0x3f800000, 0x3f960000, 0x4b000000, 0xcb000000, 0x00000000],
                water: [0x3f760000, 0x3f9f0000, 0x3f9f8000, 0x40000000, 0x400b0000, 0xcafffffe, 0xcafffffe, 0x3f800000],
                ground: [0x4b000000, 0xcb000000, 0xcb000000, 0xcafffffe, 0xcafffffe, 0xbb800000, 0x00000000, 0xcb000000],
            },
            LevelsCase {
                input: (i32::MAX, 63),
                fields: (i32::MAX, 0x30000000, 62, 64, 0x32fc0000, 0x32f80000),
                scale_value: [i32::MIN, 0, 536870912, 62, 1610612736, 2147483646, 0, 2147483646, i32::MIN, 2147483392, 0, 2147483646],
                elevation_value: [0x00000000, 0x00000000, 0x3e7ffffe, 0x00000000, 0x3f400000, 0x3f800000, 0x7fc00000, 0x7f800000, 0x00000000, 0x3f7ffffe, 0x00000000, 0x7149f2ca],
                elevation_y: [0x00000000, 0x00000000, 0x30000000, 0x33c20000, 0x33ee0000, 0x3f800000, 0x00000000, 0x00000000],
                scale_level: [0xb1a00000, 0x32f80000, 0x32fc0000, 0x34000000, 0x34160000, 0x3f800000, 0xbf800000, 0x00000000],
                water: [0x32d00000, 0x33780000, 0x337a0000, 0x341f0000, 0x34350000, 0xbf800000, 0xbf800000, 0x32f80000],
                ground: [0x32d40000, 0x337a0000, 0x337c0000, 0x341f8000, 0x34358000, 0xbf800000, 0xbf800000, 0x32fc0000],
            },
            LevelsCase {
                input: (16777217, 63),
                fields: (16777217, 0x33800000, 62, 64, 0x367c0000, 0x36780000),
                scale_value: [-16777216, 0, 4194304, 62, 12582912, 16777216, 0, 16777216, i32::MIN, 16777214, 0, 16777216],
                elevation_value: [0x00000000, 0x00000000, 0x3e7fff46, 0x00000000, 0x3f3ffff0, 0x3f800000, 0x7fc00000, 0x7f800000, 0x00000000, 0x3f7ffffe, 0x00000000, 0x7149f2fb],
                elevation_y: [0x00000000, 0x00000000, 0x3380001f, 0x3742002f, 0x376e003a, 0x4300001f, 0x00000000, 0x00000000],
                scale_level: [0xb5200000, 0x36780000, 0x367c0000, 0x37800000, 0x37960000, 0x43000000, 0xc3000000, 0x00000000],
                water: [0x36500000, 0x36f80000, 0x36fa0000, 0x379f0000, 0x37b50000, 0xc3000000, 0xc3000000, 0x36780000],
                ground: [0x36540000, 0x36fa0000, 0x36fc0000, 0x379f8000, 0x37b58000, 0xc3000000, 0xc3000000, 0x367c0000],
            },
            LevelsCase {
                input: (256, -2000000000),
                fields: (256, 0x3b800000, -2000000001, -1999999999, 0xcaee6b28, 0xcaee6b28),
                scale_value: [-256, 0, 64, -2000000000, 192, 255, 0, 255, i32::MIN, 255, 0, 255],
                elevation_value: [0x3f7ffffc, 0x3f7ffffe, 0x3f7ffffe, 0x00000000, 0x3f800000, 0x3f800000, 0x7fc00000, 0x7f800000, 0x00000000, 0x3f800000, 0x3f7ffffe, 0x65d8d725],
                elevation_y: [0x3f7ffffe, 0x3f7ffffe, 0x3f7ffffe, 0x3f800000, 0x3f800000, 0xbd9705f3, 0x00000000, 0x3f7ffffe],
                scale_level: [0xbd200000, 0x3e780000, 0x3e7c0000, 0x3f800000, 0x3f960000, 0x4b000000, 0xcb000000, 0x00000000],
                water: [0xcaee6b28, 0xcaee6b28, 0xcaee6b28, 0xcaee6b26, 0xcaee6b26, 0x490ca6c0, 0x490ca6c0, 0xcaee6b28],
                ground: [0xcaee6b28, 0xcaee6b28, 0xcaee6b28, 0xcaee6b26, 0xcaee6b26, 0x490ca6c0, 0x490ca6c0, 0xcaee6b28],
            },
        ];
        for case in cases {
            let levels = Levels::new(case.input.0, case.input.1);
            assert_eq!(
                (
                    levels.world_height,
                    levels.unit.to_bits(),
                    levels.water_y,
                    levels.ground_level,
                    levels.ground.to_bits(),
                    levels.water.to_bits(),
                ),
                case.fields,
                "{:?}",
                case.input
            );
            assert_eq!(levels.water_level, case.input.1);
            let values = [
                -1.0,
                0.0,
                0.25,
                levels.water,
                0.75,
                1.0,
                f32::NAN,
                f32::INFINITY,
                f32::NEG_INFINITY,
                0.999_999_9,
                -0.0,
                1e30,
            ];
            let ys = [-10, 62, 63, 256, 300, i32::MAX, i32::MIN, 0];
            assert_eq!(values.map(|v| levels.scale_value(v)), case.scale_value);
            assert_eq!(
                values.map(|v| levels.elevation_value(v).to_bits()),
                case.elevation_value
            );
            assert_eq!(
                ys.map(|y| levels.elevation_y(y).to_bits()),
                case.elevation_y
            );
            assert_eq!(
                ys.map(|y| levels.scale_level(y).to_bits()),
                case.scale_level
            );
            assert_eq!(ys.map(|y| levels.water(y).to_bits()), case.water);
            assert_eq!(ys.map(|y| levels.ground(y).to_bits()), case.ground);
        }
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
        assert_eq!(ContinentSettings::DEFAULT_SCALE, 3_000);
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

        let bits = |form: TerrainFormSettings| {
            [
                form.weight.to_bits(),
                form.base_scale.to_bits(),
                form.vertical_scale.to_bits(),
                form.horizontal_scale.to_bits(),
            ]
        };
        assert_eq!(bits(TerrainFormSettings::default()), [0x3f800000; 4]);
        assert_eq!(
            bits(TerrainFormSettings::new(3.5, 0.25, 7.0)),
            [0x40600000, 0x3f800000, 0x3e800000, 0x40e00000]
        );
    }
}
