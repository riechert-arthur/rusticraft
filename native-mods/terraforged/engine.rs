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
}
