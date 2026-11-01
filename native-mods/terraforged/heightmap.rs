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
    terraforged_engine::{Cell, Levels, TerrainRegistry},
    terraforged_populator::VolcanoPopulator,
    terraforged_river::Rivermap,
};

pub struct HeightmapRivers {
    levels: Levels,
    terrains: TerrainRegistry,
}

impl HeightmapRivers {
    pub fn new(levels: Levels, terrains: TerrainRegistry) -> Self {
        Self { levels, terrains }
    }

    pub fn apply(&self, cell: &mut Cell, x: f32, z: f32, rivermap: &Rivermap) {
        rivermap.apply(cell, x, z);
        VolcanoPopulator::modify_volcano_type(cell, self.levels, &self.terrains);
    }
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_river::GenWarp;
    use super::*;

    #[test]
    fn heightmap_rivers_match_engine_0_3_0() {
        let terrains = TerrainRegistry::new();
        let stage = HeightmapRivers::new(Levels::new(256, 63), TerrainRegistry::new());
        let map = Rivermap::new(17, -9, Vec::new(), GenWarp::direct());

        for (value, river_mask, terrain) in [
            (0.1, 0.5, 14),
            (0.1, 1.0, 14),
            (0.5, 0.5, 14),
            (0.5, 1.0, 15),
        ] {
            let mut cell = Cell::default();
            cell.terrain = terrains.get_by_id(15);
            cell.value = value;
            cell.river_mask = river_mask;
            stage.apply(&mut cell, 12.25, -98.75, &map);
            assert_eq!(cell.terrain.id(), terrain);
        }
    }
}
