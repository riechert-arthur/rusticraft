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

use super::{
    terraforged_climate::ClimateSettings,
    terraforged_engine::{Levels, RiverSettings, Seed, TerrainSettings, WorldSettings},
    terraforged_filters::FilterSettings,
};

#[derive(Clone, Copy, Default)]
pub struct GeneratorSettings {
    pub world: WorldSettings,
    pub climate: ClimateSettings,
    pub terrain: TerrainSettings,
    pub rivers: RiverSettings,
    pub filters: FilterSettings,
}

pub struct GeneratorContext {
    pub seed: Seed,
    pub levels: Levels,
    pub settings: GeneratorSettings,
}

impl GeneratorContext {
    pub fn new(settings: GeneratorSettings) -> Self {
        Self {
            seed: Seed::from_long(settings.world.seed),
            levels: Levels::from_properties(settings.world.properties),
            settings,
        }
    }

    pub fn copy(&self) -> Self {
        self.split(0)
    }

    pub fn split(&self, offset: i32) -> Self {
        Self {
            seed: self.seed.offset(offset),
            levels: self.levels,
            settings: self.settings,
        }
    }

    pub const fn seed(&self) -> Seed {
        self.seed.split()
    }

    pub const fn seed_offset(&self, offset: i32) -> Seed {
        self.seed.offset(offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generator_context_matches_engine_0_3_0() {
        let mut settings = GeneratorSettings::default();
        settings.world.seed = 1_234;
        settings.world.properties.world_height = 320;
        settings.world.properties.sea_level = 80;
        settings.rivers.river_count = 3;
        let mut context = GeneratorContext::new(settings);

        assert_eq!(context.seed.get(), 1_234);
        assert_eq!(context.seed.next(), 1_234);
        assert_eq!(context.seed.get(), 1_235);
        assert_eq!(context.levels, Levels::new(320, 80));

        let copy = context.copy();
        let split = context.split(9);
        assert_eq!(copy.seed.get(), 1_234);
        assert_eq!(split.seed.get(), 1_243);
        assert_eq!(context.seed().get(), 1_234);
        assert_eq!(context.seed_offset(5).get(), 1_239);
        assert_eq!(copy.settings.rivers.river_count, 3);
    }
}
