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
fn java_equals_ignore_case(first: &str, second: &str) -> bool {
    if first.len() == second.len() && first.eq_ignore_ascii_case(second) {
        return true;
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
        // Java 26's String.equalsIgnoreCase; an offline check also compared every pair of code
        // points that share a case mapping (6,134 pairs) and every code point's simple mappings.
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
}
