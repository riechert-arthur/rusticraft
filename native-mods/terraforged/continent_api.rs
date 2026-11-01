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

use super::{
    terraforged_advanced_continent::AdvancedContinentGenerator,
    terraforged_climate::LandValue,
    terraforged_continent::SimpleContinentGenerator,
    terraforged_engine::{ContinentType, Seed, WorldSettings},
    terraforged_fancy::FancyContinentGenerator,
    terraforged_populator::Populator,
    terraforged_river::Rivermap,
};

pub trait Continent: Populator + LandValue + Send + Sync {
    fn kind(&self) -> ContinentType;
    fn edge_value(&self, x: f32, z: f32) -> f32;
    fn nearest_center(&self, x: f32, z: f32) -> i64;
    fn rivermap(&self, x: i32, z: i32) -> Arc<Rivermap>;
}

impl LandValue for SimpleContinentGenerator {
    fn land_value(&self, x: f32, z: f32) -> f32 {
        SimpleContinentGenerator::land_value(self, x, z)
    }
}

impl Continent for SimpleContinentGenerator {
    fn kind(&self) -> ContinentType {
        ContinentType::Multi
    }

    fn edge_value(&self, x: f32, z: f32) -> f32 {
        SimpleContinentGenerator::edge_value(self, x, z)
    }

    fn nearest_center(&self, x: f32, z: f32) -> i64 {
        SimpleContinentGenerator::nearest_center(self, x, z)
    }

    fn rivermap(&self, x: i32, z: i32) -> Arc<Rivermap> {
        SimpleContinentGenerator::rivermap(self, x, z)
    }
}

impl LandValue for AdvancedContinentGenerator {
    fn land_value(&self, x: f32, z: f32) -> f32 {
        AdvancedContinentGenerator::land_value(self, x, z)
    }
}

impl Continent for AdvancedContinentGenerator {
    fn kind(&self) -> ContinentType {
        ContinentType::MultiImproved
    }

    fn edge_value(&self, x: f32, z: f32) -> f32 {
        AdvancedContinentGenerator::edge_value(self, x, z)
    }

    fn nearest_center(&self, x: f32, z: f32) -> i64 {
        AdvancedContinentGenerator::nearest_center(self, x, z)
    }

    fn rivermap(&self, x: i32, z: i32) -> Arc<Rivermap> {
        AdvancedContinentGenerator::rivermap(self, x, z)
    }
}

impl LandValue for FancyContinentGenerator {
    fn land_value(&self, x: f32, z: f32) -> f32 {
        FancyContinentGenerator::land_value(self, x, z)
    }
}

impl Continent for FancyContinentGenerator {
    fn kind(&self) -> ContinentType {
        ContinentType::Fancy
    }

    fn edge_value(&self, x: f32, z: f32) -> f32 {
        FancyContinentGenerator::edge_value(self, x, z)
    }

    fn nearest_center(&self, x: f32, z: f32) -> i64 {
        FancyContinentGenerator::nearest_center(self, x, z)
    }

    fn rivermap(&self, x: i32, z: i32) -> Arc<Rivermap> {
        FancyContinentGenerator::rivermap(self, x, z)
    }
}

pub fn create_continent(
    kind: ContinentType,
    seed: &mut Seed,
    settings: WorldSettings,
) -> Arc<dyn Continent> {
    match kind {
        ContinentType::Multi => Arc::new(SimpleContinentGenerator::multi(seed, settings)),
        ContinentType::Single => Arc::new(SingleContinent(SimpleContinentGenerator::single(
            seed, settings,
        ))),
        ContinentType::MultiImproved => Arc::new(AdvancedContinentGenerator::new(seed, settings)),
        ContinentType::Fancy => Arc::new(FancyContinentGenerator::new(seed, settings)),
    }
}

struct SingleContinent(SimpleContinentGenerator);

impl Populator for SingleContinent {
    fn apply(&self, cell: &mut super::terraforged_engine::Cell, x: f32, z: f32) {
        self.0.apply(cell, x, z);
    }
}

impl LandValue for SingleContinent {
    fn land_value(&self, x: f32, z: f32) -> f32 {
        self.0.land_value(x, z)
    }
}

impl Continent for SingleContinent {
    fn kind(&self) -> ContinentType {
        ContinentType::Single
    }

    fn edge_value(&self, x: f32, z: f32) -> f32 {
        self.0.edge_value(x, z)
    }

    fn nearest_center(&self, x: f32, z: f32) -> i64 {
        self.0.nearest_center(x, z)
    }

    fn rivermap(&self, x: i32, z: i32) -> Arc<Rivermap> {
        self.0.rivermap(x, z)
    }
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_engine::{Cell, TerrainRegistry};
    use super::*;

    #[test]
    fn continent_factory_dispatches_all_engine_modes() {
        for kind in ContinentType::VALUES {
            let mut seed = Seed::new(4_321);
            let mut settings = WorldSettings::default();
            settings.continent.continent_type = kind;
            let continent = create_continent(kind, &mut seed, settings);
            assert_eq!(continent.kind(), kind);

            let mut cell = Cell::default();
            cell.terrain = TerrainRegistry::new().get_by_id(8);
            continent.apply(&mut cell, 12.25, -98.75);
            assert_eq!(
                continent.land_value(12.25, -98.75).to_bits(),
                continent.edge_value(12.25, -98.75).to_bits()
            );
            assert!(!cell.continent_edge.is_nan());
        }
    }
}
