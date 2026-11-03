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

#[cfg(test)]
mod tests {
    use super::*;

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
            let actual = u16::from(category.is_flat())
                | u16::from(category.is_river()) << 1
                | u16::from(category.is_shallow_ocean()) << 2
                | u16::from(category.is_deep_ocean()) << 3
                | u16::from(category.is_coast()) << 4
                | u16::from(category.is_submerged()) << 5
                | u16::from(category.is_overground()) << 6
                | u16::from(category.overrides_river()) << 7
                | u16::from(category.overrides_coast()) << 8
                | u16::from(category.is_lake()) << 9
                | u16::from(category.is_wetland()) << 10
                | u16::from(category.is_mountain()) << 11
                | u16::from(category.is_volcano()) << 12;
            assert_eq!(category.erosion_modifier().to_bits(), 0x3f800000);
            assert_eq!(actual, expected, "{category:?}");
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
}
