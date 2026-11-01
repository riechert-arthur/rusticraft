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

use std::sync::{Arc, RwLock};

use super::terraforged_engine::{Cell, Levels, TerrainProperties};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SmoothingSettings {
    pub iterations: i32,
    pub smoothing_radius: f32,
    pub smoothing_rate: f32,
}

impl Default for SmoothingSettings {
    fn default() -> Self {
        Self {
            iterations: 1,
            smoothing_radius: 1.8,
            smoothing_rate: 0.9,
        }
    }
}

pub trait Filterable {
    fn block_x(&self) -> i32;

    fn block_z(&self) -> i32;

    fn total_size(&self) -> usize;

    fn backing(&self) -> &[Cell];

    fn backing_mut(&mut self) -> &mut [Cell];

    fn cell_raw(&self, x: usize, z: usize) -> Option<&Cell> {
        let total = self.total_size();
        self.backing().get(z.wrapping_mul(total).wrapping_add(x))
    }

    fn cell_raw_mut(&mut self, x: usize, z: usize) -> Option<&mut Cell> {
        let total = self.total_size();
        self.backing_mut()
            .get_mut(z.wrapping_mul(total).wrapping_add(x))
    }
}

#[derive(Clone, Copy, Debug)]
struct RangeModifier {
    min: f32,
    max: f32,
    range: f32,
    inverted: bool,
}

impl RangeModifier {
    fn inverted(min: f32, max: f32) -> Self {
        Self {
            min,
            max,
            range: max - min,
            inverted: true,
        }
    }

    fn value(self, value: f32) -> f32 {
        let modifier = if value > self.max {
            1.0
        } else if value < self.min {
            0.0
        } else {
            (value - self.min) / self.range
        };
        if self.inverted {
            1.0 - modifier
        } else {
            modifier
        }
    }

    fn modify(self, cell: &Cell, value: f32) -> f32 {
        let mut strength = 1.0;
        let erosion = cell.terrain.erosion_modifier();
        if erosion != 1.0 {
            let alpha = map(cell.terrain_region_edge, 0.0, 0.15, 0.15);
            strength = lerp(1.0, erosion, alpha);
        }
        if cell.river_mask < 0.1 {
            strength *= map(cell.river_mask, 0.002, 0.1, 0.098);
        }
        self.value(cell.value) * strength * value
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Smoothing {
    radius: usize,
    radius_squared: f32,
    strength: f32,
    modifier: RangeModifier,
}

impl Smoothing {
    pub fn new(settings: SmoothingSettings, levels: Levels) -> Self {
        Self {
            radius: java_round(settings.smoothing_radius + 0.5).max(0) as usize,
            radius_squared: settings.smoothing_radius * settings.smoothing_radius,
            strength: settings.smoothing_rate,
            modifier: RangeModifier::inverted(levels.ground(1), levels.ground(120)),
        }
    }

    pub fn apply(
        &self,
        map: &mut impl Filterable,
        _seed_x: i32,
        _seed_z: i32,
        mut iterations: i32,
    ) {
        while iterations > 0 {
            self.apply_once(map);
            iterations -= 1;
        }
    }

    fn apply_once(&self, map: &mut impl Filterable) {
        let Some(max) = map.total_size().checked_sub(self.radius) else {
            return;
        };
        for z in self.radius..max {
            for x in self.radius..max {
                let cell = map
                    .cell_raw(x, z)
                    .expect("filter coordinates are in bounds");
                if cell.erosion_mask {
                    continue;
                }
                let mut total = 0.0_f32;
                let mut weights = 0.0_f32;
                for dz in -(self.radius as isize)..=self.radius as isize {
                    for dx in -(self.radius as isize)..=self.radius as isize {
                        let distance_squared = (dx * dx + dz * dz) as f32;
                        if distance_squared > self.radius_squared {
                            continue;
                        }
                        let neighbour = map
                            .cell_raw(x.wrapping_add_signed(dx), z.wrapping_add_signed(dz))
                            .expect("smoothing radius stays inside the filter map");
                        if neighbour.is_absent() {
                            continue;
                        }
                        let weight = 1.0 - distance_squared / self.radius_squared;
                        total += neighbour.value * weight;
                        weights += weight;
                    }
                }
                if weights > 0.0 {
                    let difference = cell.value - total / weights;
                    let adjustment = self.modifier.modify(cell, difference * self.strength);
                    map.cell_raw_mut(x, z)
                        .expect("filter coordinates are in bounds")
                        .value -= adjustment;
                }
            }
        }
    }
}

fn java_round(value: f32) -> i32 {
    (value + 0.5).floor() as i32
}

fn map(value: f32, min: f32, max: f32, range: f32) -> f32 {
    let difference = value.clamp(min, max) - min;
    if difference > range {
        1.0
    } else {
        difference / range
    }
}

fn lerp(first: f32, second: f32, alpha: f32) -> f32 {
    first + alpha * (second - first)
}

pub struct WorldErosion<T, F, V> {
    value: RwLock<Option<Arc<T>>>,
    factory: F,
    validator: V,
}

impl<T, F, V> WorldErosion<T, F, V>
where
    F: Fn(i32) -> T,
    V: Fn(&T, i32) -> bool,
{
    pub fn new(factory: F, validator: V) -> Self {
        Self {
            value: RwLock::new(None),
            factory,
            validator,
        }
    }

    pub fn get(&self, context: i32) -> Arc<T> {
        if let Some(value) = self.read_valid(context) {
            return value;
        }
        let mut slot = self
            .value
            .write()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(value) = slot
            .as_ref()
            .filter(|value| (self.validator)(value, context))
        {
            return Arc::clone(value);
        }
        let value = Arc::new((self.factory)(context));
        *slot = Some(Arc::clone(&value));
        value
    }

    fn read_valid(&self, context: i32) -> Option<Arc<T>> {
        self.value
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .filter(|value| (self.validator)(value, context))
            .map(Arc::clone)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TestMap {
        total: usize,
        cells: Vec<Cell>,
    }

    impl Filterable for TestMap {
        fn block_x(&self) -> i32 {
            -32
        }

        fn block_z(&self) -> i32 {
            48
        }

        fn total_size(&self) -> usize {
            self.total
        }

        fn backing(&self) -> &[Cell] {
            &self.cells
        }

        fn backing_mut(&mut self) -> &mut [Cell] {
            &mut self.cells
        }
    }

    #[test]
    fn smoothing_matches_engine_0_3_0() {
        let total = 7;
        let mut cells = (0..total * total)
            .map(|index| {
                let mut cell = Cell::default();
                cell.value = ((index * 37 + 11) % 101) as f32 / 100.0;
                cell.river_mask = if index % 9 == 0 { 0.05 } else { 1.0 };
                cell.terrain_region_edge = (index % 5) as f32 * 0.04;
                cell.erosion_mask = index == 24;
                cell
            })
            .collect::<Vec<_>>();
        cells[17] = Cell::empty();
        let mut map = TestMap { total, cells };

        Smoothing::new(SmoothingSettings::default(), Levels::new(256, 63))
            .apply(&mut map, 123, -456, 2);

        let expected = [
            (16, 0x3f7ae148),
            (18, 0x3f35299f),
            (23, 0x3f105097),
            (24, 0x3f68f5c3),
            (25, 0x3f0ba1bc),
            (30, 0x3eec95ee),
            (32, 0x3f570a3d),
        ];
        for (index, bits) in expected {
            assert_eq!(map.cells[index].value.to_bits(), bits, "cell {index}");
        }
    }

    #[test]
    fn world_erosion_matches_engine_0_3_0() {
        let builds = AtomicUsize::new(0);
        let cache = WorldErosion::new(
            |size| {
                builds.fetch_add(1, Ordering::Relaxed);
                size
            },
            |value, size| *value == size,
        );
        let first = cache.get(4_096);
        let reused = cache.get(4_096);
        assert!(Arc::ptr_eq(&first, &reused));
        assert_eq!(builds.load(Ordering::Relaxed), 1);

        let resized = cache.get(8_192);
        assert_eq!(*resized, 8_192);
        assert!(!Arc::ptr_eq(&first, &resized));
        assert_eq!(builds.load(Ordering::Relaxed), 2);
    }
}
