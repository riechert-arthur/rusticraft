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

use super::terraforged::{java_max, java_min, lerp, map, round};
use super::terraforged_engine::{Cell, Levels, TerrainProperties};

/// Engine's `FilterSettings.Smoothing` (defaults: one iteration, radius 1.8, rate 0.9). Engine's
/// `@Range` limits (iterations and radius 0 to 5, rate 0 to 1) are only enforced by its settings UI,
/// so values outside them are accepted here as in Engine.
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

/// Engine's `Filterable`: a square map of `total_size() * total_size()` cells stored row by row
/// (`Size.indexOf(x, z) = z * total + x`). `cell_raw` is `getCellRaw`, which Engine's `Tile` answers
/// with `Cell.empty()` outside the backing array; here that is `None`.
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

/// Engine's `Modifier.range(min, max).invert()`, with `Modifier.modify`'s strength adjustments.
#[derive(Clone, Copy, Debug)]
struct RangeModifier {
    min: f32,
    max: f32,
    range: f32,
}

impl RangeModifier {
    fn new(min: f32, max: f32) -> Self {
        Self {
            min,
            max,
            range: max - min,
        }
    }

    /// `invert().getValueModifier(value)`: one minus the range modifier.
    fn value(self, value: f32) -> f32 {
        let modifier = if value > self.max {
            1.0
        } else if value < self.min {
            0.0
        } else {
            (value - self.min) / self.range
        };
        1.0 - modifier
    }

    /// `Modifier.modify(cell, value)`: weakened near terrain-region edges of terrains whose erosion
    /// modifier is not 1, and near rivers.
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

/// Engine's `Smoothing` filter: pulls each cell's height towards the weighted mean of the present
/// cells within `smoothing_radius`, in place (later cells see earlier cells' new values), skipping
/// the outer `radius` cells of the map and cells under the erosion mask.
#[derive(Clone, Copy, Debug)]
pub struct Smoothing {
    radius: i32,
    radius_squared: f32,
    strength: f32,
    modifier: RangeModifier,
}

impl Smoothing {
    pub fn new(settings: SmoothingSettings, levels: Levels) -> Self {
        Self {
            radius: round(settings.smoothing_radius + 0.5),
            radius_squared: settings.smoothing_radius * settings.smoothing_radius,
            strength: settings.smoothing_rate,
            modifier: RangeModifier::new(levels.ground(1), levels.ground(120)),
        }
    }

    /// `apply(map, seedX, seedZ, iterations)`: `iterations` passes (none when it is not positive);
    /// the seeds are unused, as in Engine.
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
        // A negative radius (a smoothing radius below -1) makes Engine's neighbourhood loops empty,
        // so no cell has any weight and nothing changes.
        let Ok(radius) = usize::try_from(self.radius) else {
            return;
        };
        let Some(max) = map.total_size().checked_sub(radius) else {
            return;
        };
        let reach = self.radius as isize;
        for z in radius..max {
            for x in radius..max {
                let cell = map
                    .cell_raw(x, z)
                    .expect("filter coordinates are in bounds");
                if cell.erosion_mask {
                    continue;
                }
                let mut total = 0.0_f32;
                let mut weights = 0.0_f32;
                for dz in -reach..=reach {
                    for dx in -reach..=reach {
                        let distance_squared = (dx * dx + dz * dz) as f32;
                        if distance_squared > self.radius_squared {
                            continue;
                        }
                        let Some(neighbour) =
                            map.cell_raw(x.wrapping_add_signed(dx), z.wrapping_add_signed(dz))
                        else {
                            continue;
                        };
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

#[derive(Clone, Copy, Debug)]
pub struct Steepness {
    radius: isize,
    scaler: f32,
    water_level: f32,
    _max_beach_level: f32,
}

impl Steepness {
    pub fn new(radius: i32, scaler: f32, levels: Levels) -> Self {
        Self {
            radius: radius as isize,
            scaler,
            water_level: levels.water,
            _max_beach_level: levels.water(6),
        }
    }

    pub fn apply(&self, map: &mut impl Filterable, _seed_x: i32, _seed_z: i32, _iterations: i32) {
        let total = map.total_size();
        for z in 0..total {
            for x in 0..total {
                let cell_value = map
                    .cell_raw(x, z)
                    .expect("filter coordinates are in bounds")
                    .value;
                let mut total_height_difference = 0.0_f32;
                for dz in -1_isize..=2 {
                    for dx in -1_isize..=2 {
                        if dx == 0 && dz == 0 {
                            continue;
                        }
                        let Some(neighbour) = map.cell_raw(
                            x.wrapping_add_signed(dx * self.radius),
                            z.wrapping_add_signed(dz * self.radius),
                        ) else {
                            continue;
                        };
                        if neighbour.is_absent() {
                            continue;
                        }
                        let height = java_max(neighbour.value, self.water_level);
                        total_height_difference += (cell_value - height).abs() / self.radius as f32;
                    }
                }
                map.cell_raw_mut(x, z)
                    .expect("filter coordinates are in bounds")
                    .gradient = java_min(1.0, total_height_difference * self.scaler);
            }
        }
    }
}

/// Engine's `WorldErosion<T>`: a lazily built value shared between threads, rebuilt by `factory`
/// whenever `validator` rejects it for the requested context. Engine's `StampedLock` (optimistic
/// read, then a read lock) is a `RwLock` here; the value is shared as an `Arc`.
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

    /// Engine's `get(ctx)`: returns the cached value when the validator accepts it for `context`;
    /// otherwise takes the write lock, checks again (another thread may have rebuilt it) and builds
    /// a new value with the factory, which replaces the cached one.
    pub fn get(&self, context: i32) -> Arc<T> {
        // Engine's readValue: the read lock only covers taking the value; it is validated outside.
        let current = self
            .value
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        if let Some(value) = current.filter(|value| (self.validator)(value, context)) {
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
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_engine::TerrainRegistry;
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

    /// The cell map `SmoothingOracle.java` (`.local/terraforged/oracles/smoothing/`) builds for case
    /// `salt`: hashed values, river masks on both sides of 0.1 and 0.002, region edges, erosion
    /// masks, absent cells, and the none/badlands/hills/mountains terrains (badlands' erosion
    /// modifier is 0.3, the others' 1).
    fn oracle_cells(registry: &TerrainRegistry, total: usize, salt: i32) -> Vec<Cell> {
        let terrains = ["none", "badlands", "hills", "mountains"]
            .map(|name| registry.get_by_name(name).expect("built-in terrain"));
        (0..(total * total) as i32)
            .map(|index| {
                let mut h = index
                    .wrapping_mul(7919)
                    .wrapping_add(salt.wrapping_mul(104_729));
                h ^= ((h as u32) >> 13) as i32;
                h = h.wrapping_mul(0x5bd1_e995);
                h ^= ((h as u32) >> 15) as i32;
                let mut cell = if h & 31 == 3 {
                    Cell::empty(registry)
                } else {
                    Cell::new(registry)
                };
                cell.value = (((h as u32) >> 8) & 0xFFFF) as f32 / 40_000.0 - 0.2;
                cell.river_mask = match ((h as u32) >> 4) & 7 {
                    0 => 0.001,
                    1 => 0.05,
                    2 => 0.0999,
                    3 => 0.1,
                    _ => 1.0,
                };
                cell.terrain_region_edge = (((h as u32) >> 24) & 63) as f32 / 256.0;
                cell.erosion_mask = h & 63 == 17;
                cell.terrain = terrains[(((h as u32) >> 1) & 3) as usize].clone();
                cell
            })
            .collect()
    }

    /// Replays `SmoothingOracle.java`, which runs Engine's `Smoothing.apply` on the same 14x14 maps:
    /// default settings for one and three passes, wider and narrower radii (0.4 weighs only the
    /// cell itself, 0 gives NaN weights, -0.7 rounds to radius 0, -2.5 to a negative radius, 7 leaves
    /// no interior), other rates and levels, and zero or negative iterations. Each line folds every
    /// cell's value bits (`fold * 31 + bits`, from 17) and lists every 23rd cell's bits from cell 5.
    #[test]
    fn smoothing_matches_engine_oracle() {
        let registry = TerrainRegistry::new();
        let total = 14;
        let cases = [
            (1.8, 0.9, 1, 256, 63),
            (1.8, 0.9, 3, 256, 63),
            (3.2, 0.5, 2, 384, 127),
            (0.4, 1.0, 1, 256, 63),
            (5.0, 0.75, 1, 256, 63),
            (0.0, 0.9, 1, 256, 63),
            (-0.7, 0.9, 1, 256, 63),
            (-2.5, 0.9, 1, 256, 63),
            (7.0, 0.9, 1, 256, 63),
            (1.8, 0.9, 0, 256, 63),
            (1.8, 0.9, -3, 256, 63),
        ];
        let mut out = String::new();
        for (case, (radius, rate, iterations, height, sea_level)) in cases.into_iter().enumerate() {
            let settings = SmoothingSettings {
                iterations,
                smoothing_radius: radius,
                smoothing_rate: rate,
            };
            let mut map = TestMap {
                total,
                cells: oracle_cells(&registry, total, case as i32),
            };
            Smoothing::new(settings, Levels::new(height, sea_level))
                .apply(&mut map, 123, -456, iterations);
            let mut fold = 17_i64;
            let mut samples = String::new();
            for (index, cell) in map.cells.iter().enumerate() {
                let bits = cell.value.to_bits();
                fold = fold.wrapping_mul(31).wrapping_add(i64::from(bits as i32));
                if index % 23 == 5 {
                    samples += &format!(" {bits:08x}");
                }
            }
            out += &format!("{case} {fold:016x}{samples}\n");
        }
        assert_eq!(
            out,
            "0 55c4c81f1c2fe0d3 be3d70a4 3f93fb15 3fa9a9fc 3fb2fec5 3f179097 3f4f573f 3f721b09 3e16bb99 3f47b4a2\n\
             1 d9d2f379b0262870 3ec0c156 3f65652d 3f49e4f8 3f44a71e 3f34d35b 3f0be331 3f282e1d 3e993dda be244674\n\
             2 1b58a6970691d7eb 3f89ac71 3f9fedfa 3ea7212e 3f2dfd8b 3fa27bb3 3ee513c3 3d9288ce 3f6e2339 3ecde9e2\n\
             3 0ed0ffc6695dc5f4 3e7edfa5 3f01844d bdc624de 3faca305 3fa6786c 3e9f34d8 3fa3a511 3e8caf4e 3f8cfaac\n\
             4 7a690cc803d7c0e3 bd77b4a4 3e626e97 3eca339c 3e8b39c0 3dfb15b6 3c2ee630 3f39460b 3f67c9ef 3f593c37\n\
             5 78a2e5a0d654cab2 3fa31687 3fa1c6a8 3f118c7e 3f9d2bd3 be2b8bad 3cbf4880 3e3f6fd1 bdd6f007 3fa31b71\n\
             6 65f09fe0648f8a36 3f5cd845 3f17c01a bdceb1c5 3f3d3f7d 3f371759 3b7f9700 3fae0f90 3fa7eb85 3ecb6e30\n\
             7 c6bb5f58b1771378 3fb684b5 3ee0f5c2 3f0d3c36 3f953b64 3f9400d1 3e6a9931 3f9fce07 3e204ea5 3dbf2e4a\n\
             8 ec8ecb93e7989f9e 3f3d54ca be3b8bad 3f78779b 3ea53f7e 3e14ea4b 3f4d38ef 3f843d70 3f46ca58 3f243c9f\n\
             9 9669758e74e9a8c9 3fa185f0 3f7507c9 3e22e48f 3eb47e28 bde538f0 bda5bc02 3f527a0f 3fa0d35a be0793de\n\
             10 e596f2904148f08e 3eac9ba6 3d56bb94 3f90a4a8 3f48538f 3fa786c2 3e932ca6 3fb6acda bd59999c 3f262b6b\n"
        );
    }

    #[test]
    fn steepness_matches_engine_0_3_0() {
        let registry = TerrainRegistry::new();
        let total = 6;
        let cells = (0..total * total)
            .map(|index| {
                let mut cell = Cell::new(&registry);
                cell.value = 0.25 + ((index * 29 + 7) % 97) as f32 / 32_768.0;
                cell
            })
            .collect();
        let mut map = TestMap { total, cells };

        Steepness::new(1, 10.0, Levels::new(256, 63)).apply(&mut map, 123, -456, 99);

        let expected = [
            (0, 0x3e0c0000),
            (1, 0x3dd7a000),
            (7, 0x3e3a9000),
            (14, 0x3e165000),
            (21, 0x3e0a2000),
            (28, 0x3da96000),
            (35, 0x3d1c4000),
        ];
        for (index, bits) in expected {
            assert_eq!(map.cells[index].gradient.to_bits(), bits, "cell {index}");
        }
    }

    #[test]
    fn world_erosion_reuses_until_the_size_changes() {
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

    #[test]
    fn world_erosion_builds_once_across_threads() {
        let builds = AtomicUsize::new(0);
        let cache = WorldErosion::new(
            |size| {
                builds.fetch_add(1, Ordering::Relaxed);
                std::thread::sleep(std::time::Duration::from_millis(5));
                size
            },
            |value, size| *value == size,
        );
        let values: Vec<Arc<i32>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8).map(|_| scope.spawn(|| cache.get(4_096))).collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert_eq!(builds.load(Ordering::Relaxed), 1);
        assert!(values.iter().all(|value| Arc::ptr_eq(value, &values[0])));
    }

    /// Replays `WorldErosionOracle.java` (`.local/terraforged/oracles/world_erosion/`), which calls Engine's
    /// `WorldErosion.get` with a validator that accepts any cached size at least as large as the context:
    /// each line is the context, the returned size, its build number and the factory and validator calls.
    #[test]
    fn world_erosion_calls_match_engine_oracle() {
        let calls = std::sync::Mutex::new(Vec::new());
        let builds = AtomicUsize::new(0);
        let cache = WorldErosion::new(
            |size: i32| {
                calls.lock().unwrap().push(format!("build {size}"));
                (size, builds.fetch_add(1, Ordering::Relaxed))
            },
            |value: &(i32, usize), context| {
                calls
                    .lock()
                    .unwrap()
                    .push(format!("validate {} {context}", value.0));
                value.0 >= context
            },
        );
        let mut out = String::new();
        for context in [
            4096,
            4096,
            1024,
            8192,
            8192,
            4096,
            -1,
            i32::MIN,
            i32::MAX,
            0,
        ] {
            let value = cache.get(context);
            let calls = std::mem::take(&mut *calls.lock().unwrap());
            out += &format!(
                "{context} -> {} #{} [{}]\n",
                value.0,
                value.1,
                calls.join(", ")
            );
        }
        assert_eq!(
            out,
            "4096 -> 4096 #0 [build 4096]\n\
             4096 -> 4096 #0 [validate 4096 4096]\n\
             1024 -> 4096 #0 [validate 4096 1024]\n\
             8192 -> 8192 #1 [validate 4096 8192, validate 4096 8192, build 8192]\n\
             8192 -> 8192 #1 [validate 8192 8192]\n\
             4096 -> 8192 #1 [validate 8192 4096]\n\
             -1 -> 8192 #1 [validate 8192 -1]\n\
             -2147483648 -> 8192 #1 [validate 8192 -2147483648]\n\
             2147483647 -> 2147483647 #2 [validate 8192 2147483647, validate 8192 2147483647, build 2147483647]\n\
             0 -> 2147483647 #2 [validate 2147483647 0]\n"
        );
    }
}
