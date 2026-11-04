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
use super::terraforged_engine::{
    Cell, ControlPoints, Levels, Terrain, TerrainProperties, TerrainRegistry, WorldControlPoints,
};

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

/// Engine's `Steepness` filter: sets each cell's `gradient` from its height differences to the 15
/// cells at `(dx, dz) * radius` for `dx, dz` in `-1..=2` (Engine's neighbourhood is asymmetric),
/// with neighbours below sea level raised to it. Neighbours `Tile.getCellRaw` answers with
/// `Cell.empty()` (outside the backing array) or that are absent are skipped. Engine's unused
/// `maxBeachLevel` field is not kept.
#[derive(Clone, Copy, Debug)]
pub struct Steepness {
    radius: i32,
    scaler: f32,
    water_level: f32,
}

impl Steepness {
    /// `new Steepness(radius, scaler, levels)`; `WorldFilters` uses radius 1 and scaler 10.
    pub fn new(radius: i32, scaler: f32, levels: Levels) -> Self {
        Self {
            radius,
            scaler,
            water_level: levels.water,
        }
    }

    /// `apply(map, seedX, seedZ, iterations)`: one pass over every cell, row by row (Engine's
    /// `Filter.iterate`); the seeds and the iteration count are unused, as in Engine.
    pub fn apply(&self, map: &mut impl Filterable, _seed_x: i32, _seed_z: i32, _iterations: i32) {
        let total = map.total_size();
        let radius = self.radius as f32;
        for z in 0..total {
            for x in 0..total {
                let cell_value = map
                    .cell_raw(x, z)
                    .expect("filter coordinates are in bounds")
                    .value;
                let mut total_height_difference = 0.0_f32;
                for dz in -1_i32..=2 {
                    for dx in -1_i32..=2 {
                        if dx == 0 && dz == 0 {
                            continue;
                        }
                        // Java `int` arithmetic: `cx + dx * radius`, then `z * total + x`.
                        let neighbour_x = (x as i32).wrapping_add(dx.wrapping_mul(self.radius));
                        let neighbour_z = (z as i32).wrapping_add(dz.wrapping_mul(self.radius));
                        let Some(neighbour) = java_cell_raw(map, neighbour_x, neighbour_z) else {
                            continue;
                        };
                        if neighbour.is_absent() {
                            continue;
                        }
                        let height = java_max(neighbour.value, self.water_level);
                        total_height_difference += (cell_value - height).abs() / radius;
                    }
                }
                map.cell_raw_mut(x, z)
                    .expect("filter coordinates are in bounds")
                    .gradient = java_min(1.0, total_height_difference * self.scaler);
            }
        }
    }
}

/// `Tile.getCellRaw(x, z)` with Java `int` coordinates: the index `z * total + x` wraps like Java's,
/// and an index outside the backing array is `Cell.empty()` (`None`).
fn java_cell_raw(map: &impl Filterable, x: i32, z: i32) -> Option<&Cell> {
    let index = z.wrapping_mul(map.total_size() as i32).wrapping_add(x);
    usize::try_from(index)
        .ok()
        .and_then(|index| map.backing().get(index))
}

/// Engine's `BeachDetect` filter: turns a coast cell (`isCoast`: coast, beach, or a terrain
/// delegating to one) whose continent edge is below the beach control point into the world's
/// beach terrain when the squared height gradient across the cells 8 blocks to the east and west
/// and to the north and south is below 0.275. A neighbour that is absent or outside the map
/// (`Tile.getCellRaw`'s `Cell.empty()`) is replaced by the cell itself and shortens the distance
/// from 17 by 8. Engine's unused `grad2` field is not kept.
#[derive(Clone, Debug)]
pub struct BeachDetect {
    transition: ControlPoints,
    beach: Terrain,
}

impl BeachDetect {
    const RADIUS: i32 = 8;
    const DIAMETER: i32 = 17;

    /// `new BeachDetect(context)`: `points` is `context.settings.world.controlPoints` (validated by
    /// `ControlPoints`, which falls back to the defaults); `terrains` is the world's registry, whose
    /// built-in `beach` the filter assigns.
    pub fn new(points: WorldControlPoints, terrains: &TerrainRegistry) -> Self {
        Self {
            transition: ControlPoints::new(points),
            beach: terrains.get_by_id(4),
        }
    }

    /// `apply(map, seedX, seedZ, iterations)`: one pass over every cell, row by row (Engine's
    /// `Filter.iterate`); the seeds and the iteration count are unused, as in Engine.
    pub fn apply(&self, map: &mut impl Filterable, _seed_x: i32, _seed_z: i32, _iterations: i32) {
        let total = map.total_size();
        for z in 0..total {
            for x in 0..total {
                let cell = map
                    .cell_raw(x, z)
                    .expect("filter coordinates are in bounds");
                // `!(a && b)`, not `edge >= beach`: a NaN edge fails Engine's `<` test.
                if !(cell.terrain.is_coast() && cell.continent_edge < self.transition.beach) {
                    continue;
                }
                // Java `int` arithmetic: `dz - 8` and so on, then `z * total + x`.
                let (cx, cz) = (x as i32, z as i32);
                let north = java_cell_raw(map, cx, cz.wrapping_sub(Self::RADIUS));
                let south = java_cell_raw(map, cx, cz.wrapping_add(Self::RADIUS));
                let east = java_cell_raw(map, cx.wrapping_add(Self::RADIUS), cz);
                let west = java_cell_raw(map, cx.wrapping_sub(Self::RADIUS), cz);
                let gradient_x = Self::gradient(east, west, cell);
                let gradient_z = Self::gradient(north, south, cell);
                if gradient_x * gradient_x + gradient_z * gradient_z < 0.275 {
                    map.cell_raw_mut(x, z)
                        .expect("filter coordinates are in bounds")
                        .terrain = self.beach.clone();
                }
            }
        }
    }

    /// `grad(a, b, def)`.
    fn gradient(first: Option<&Cell>, second: Option<&Cell>, default: &Cell) -> f32 {
        let mut distance = Self::DIAMETER;
        let first = match first {
            Some(cell) if !cell.is_absent() => cell,
            _ => {
                distance -= Self::RADIUS;
                default
            }
        };
        let second = match second {
            Some(cell) if !cell.is_absent() => cell,
            _ => {
                distance -= Self::RADIUS;
                default
            }
        };
        (first.value - second.value) / distance as f32
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

    /// Replays `SteepnessOracle.java` (`.local/terraforged/oracles/steepness/`), which runs Engine's
    /// `Steepness.apply` on the smoothing oracle's 14x14 maps: `WorldFilters`' radius 1 and scaler 10,
    /// wider radii, radius 0 (division by zero), negative radii, radii that reach past the map (7, 20)
    /// or wrap Java's `int` arithmetic (`i32::MAX`, `i32::MIN`), a negative scaler, swapped levels,
    /// and maps with NaN, -0.0 and 0.0 heights (one with water level 0). Each line folds every cell's
    /// gradient bits (NaNs canonical, `fold * 31 + bits`, from 17) and lists every 23rd cell's bits
    /// from cell 5.
    #[test]
    fn steepness_matches_engine_oracle() {
        let registry = TerrainRegistry::new();
        let total = 14;
        let cases = [
            (1, 10.0, 256, 63, false),
            (2, 0.05, 256, 63, false),
            (3, 0.5, 384, 127, false),
            (0, 10.0, 256, 63, false),
            (-1, 10.0, 256, 63, false),
            (-2, 2.5, 256, 63, false),
            (7, 0.1, 256, 63, false),
            (20, 10.0, 256, 63, false),
            (i32::MAX, 10.0, 256, 63, false),
            (i32::MIN, 10.0, 256, 63, false),
            (1, -3.0, 256, 63, false),
            (1, 0.2, 63, 255, false),
            (1, 0.25, 256, 0, true),
            (2, 1.0, 256, 63, true),
        ];
        let mut out = String::new();
        for (case, (radius, scaler, height, sea_level, specials)) in cases.into_iter().enumerate() {
            let mut cells = oracle_cells(&registry, total, case as i32);
            if specials {
                for (index, cell) in cells.iter_mut().enumerate() {
                    if index % 61 == 4 {
                        cell.value = f32::NAN;
                    }
                    if index % 13 == 6 {
                        cell.value = -0.0;
                    }
                    if index % 17 == 2 {
                        cell.value = 0.0;
                    }
                }
            }
            let mut map = TestMap { total, cells };
            Steepness::new(radius, scaler, Levels::new(height, sea_level))
                .apply(&mut map, 123, -456, 99);
            let mut fold = 17_i64;
            let mut samples = String::new();
            for (index, cell) in map.cells.iter().enumerate() {
                let bits = if cell.gradient.is_nan() {
                    0x7fc0_0000
                } else {
                    cell.gradient.to_bits()
                };
                fold = fold.wrapping_mul(31).wrapping_add(i64::from(bits as i32));
                if index % 23 == 5 {
                    samples += &format!(" {bits:08x}");
                }
            }
            out += &format!("{case} {fold:016x}{samples}\n");
        }
        assert_eq!(
            out,
            "0 dd8f5852f6207791 3f800000 3f800000 3f800000 3f800000 3f800000 3f800000 3f800000 3f800000 3f800000\n\
             1 2bdb47d06ebd99a0 3d5ee97a 3e19c0eb 3e02e6da 3de711f0 3dfe6668 3e8b4b5e 3e004799 3dbf28b7 3e16ddec\n\
             2 148098a72e7358f3 3f2c73c8 3f800000 3f800000 3f46bcae 3f800000 3f800000 3f800000 3ed9fcff 3e376087\n\
             3 1401a87416207791 7fc00000 7fc00000 3f800000 7fc00000 7fc00000 7fc00000 00000000 7fc00000 7fc00000\n\
             4 ce02f2cc83d7ab23 c257b688 c22275e3 c2884c28 c2464561 c2aa0cad c2c74354 c243f312 c2806b95 c248347e\n\
             5 af24849e597f53e0 c095e687 c09307af c0aa7ba4 c13bf924 c19f863e c13edd82 c0a13668 c162bee9 c0c42be6\n\
             6 6d08bffe7aae90a8 3d218847 3d1a15bf 3dc4aad1 3d40c34c 3d0fb18b 3dd2de02 3d6f11a2 3d761075 3cd3f410\n\
             7 4143b0a1239d946b 3f5bf764 3f2b1eb8 3f0dc01a 3f800000 3eb83fe4 3f1d7176 3f800000 3f76e978 3ebe1e50\n\
             8 f97440a2a3db3598 31be5a1c 32a63666 32a5ebc7 3263cc8a 32919f1b 3265be34 32596a7b 3229c7ae 31d80625\n\
             9 e9452892e248b625 00000000 00000000 b13a2c06 00000000 b2464d91 00000000 00000000 00000000 b251c4dd\n\
             10 e2a736b305a93819 c18e8824 c1a4ba6d c1a41fa9 c1896950 c1f1f8f6 c1ca3659 c1cff4ea c1c3354e c1142c98\n\
             11 068e0d0e17e14776 3f57be72 3f800000 3f800000 3f4f6112 3f800000 3f5bdaf3 3f800000 3f3b0e7d 3f2b2766\n\
             12 2b14e288d61df853 7fc00000 3f800000 7fc00000 3f800000 7fc00000 3f800000 3f800000 3f800000 3ed315b5\n\
             13 d16a5f5730b00644 7fc00000 7fc00000 3f800000 3f800000 3f800000 3f800000 3f800000 3f800000 7fc00000\n"
        );
    }

    /// `WorldFilters`' radius 1 and scaler 10 on a 6x6 map of heights just above sea level, with no
    /// absent cells: edge, corner and interior cells against Engine's gradient bits.
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

    /// Replays `BeachDetectOracle.java` (`.local/terraforged/oracles/beach/`), which runs Engine's
    /// `BeachDetect.apply` on hashed maps of coast, beach and other terrains with absent cells,
    /// NaN, infinite and signed-zero heights, NaN continent edges, maps smaller than the radius and
    /// valid and invalid beach control points. Per case: a fold of every cell's terrain id, the
    /// number of beach cells, the number of cells turned into beach and the first eight of them.
    #[test]
    fn beach_detection_matches_engine_oracle() {
        let registry = TerrainRegistry::new();
        // COAST, COAST, COAST, BEACH, NONE, SHALLOW_OCEAN, FLATS, RIVER
        let terrains = [3, 3, 3, 4, 0, 2, 8, 5].map(|id| registry.get_by_id(id));
        let beach = registry.get_by_id(4);
        let cases = [
            (20, 1.0_f32, false, 0.327_f32),
            (20, 4.0, false, 0.327),
            (20, 12.0, false, 0.327),
            (20, 30.0, true, 0.327),
            (17, 9.0, false, 0.4),
            (9, 9.0, false, 0.327),
            (8, 6.0, true, 0.327),
            (24, 9.0, true, 0.3),
            (16, 9.0, false, 0.9),
            (33, 20.0, true, 0.44),
        ];
        let mut out = String::new();
        for (case, (total, scale, specials, beach_point)) in cases.into_iter().enumerate() {
            let cells = (0..total * total)
                .map(|index| {
                    let mut h = (index as i32)
                        .wrapping_mul(7919)
                        .wrapping_add((case as i32).wrapping_mul(104_729));
                    h ^= ((h as u32) >> 13) as i32;
                    h = h.wrapping_mul(0x5bd1_e995);
                    h ^= ((h as u32) >> 15) as i32;
                    let mut cell = if h & 31 == 3 {
                        Cell::empty(&registry)
                    } else {
                        Cell::new(&registry)
                    };
                    let bits = h as u32;
                    cell.value = ((bits >> 8) & 0xffff) as f32 / 65_536.0 * scale - scale * 0.5;
                    cell.terrain = terrains[((bits >> 24) & 7) as usize].clone();
                    cell.continent_edge = ((bits >> 4) & 0x3ff) as f32 / 1_600.0;
                    if specials && index % 37 == 4 {
                        cell.value = f32::NAN;
                    }
                    if specials && index % 29 == 7 {
                        cell.continent_edge = f32::NAN;
                    }
                    if specials && index % 13 == 6 {
                        cell.value = -0.0;
                    }
                    if specials && index % 11 == 2 {
                        cell.value = f32::INFINITY;
                    }
                    cell
                })
                .collect::<Vec<_>>();
            let before = cells
                .iter()
                .map(|cell| cell.terrain.is_same(&beach))
                .collect::<Vec<_>>();
            let mut map = TestMap { total, cells };
            let points = WorldControlPoints {
                beach: beach_point,
                ..WorldControlPoints::default()
            };
            BeachDetect::new(points, &registry).apply(&mut map, 123, -456, 99);
            let mut fold = 17_i64;
            let mut beaches = 0;
            let mut turned = Vec::new();
            for (index, cell) in map.cells.iter().enumerate() {
                fold = fold
                    .wrapping_mul(31)
                    .wrapping_add(i64::from(cell.terrain.id()));
                if cell.terrain.is_same(&beach) {
                    beaches += 1;
                    if !before[index] {
                        turned.push(index);
                    }
                }
            }
            let first = turned.iter().take(8).map(|index| format!(" {index}"));
            out += &format!(
                "{case} {fold:016x} {beaches} {}{}\n",
                turned.len(),
                first.collect::<String>()
            );
        }
        assert_eq!(
            out,
            "0 a16232084b282657 129 80 0 14 15 17 24 33 48 50\n\
             1 523cf84637a15ed8 135 87 2 4 5 8 16 19 21 23\n\
             2 698133c6de0a2998 100 53 9 12 15 25 32 34 35 37\n\
             3 24a27b1ea817910c 51 5 28 284 309 320 367\n\
             4 c2298533b0ec2087 83 42 11 13 21 40 41 45 47 54\n\
             5 e3a2b4be1ab85643 22 11 7 20 27 29 44 50 51 53\n\
             6 c0b62d4973d97357 12 4 28 29 31 34\n\
             7 67a94bc7beee2b79 111 44 14 37 77 84 110 119 127 161\n\
             8 0fb25686d56c8179 67 38 0 12 13 15 32 49 53 56\n\
             9 1ba7e55f2c89924d 213 58 20 34 51 67 88 114 136 140\n"
        );
    }

    /// The filter assigns the registry's own beach (`is_same`), so later identity checks see it.
    #[test]
    fn beach_detection_assigns_the_world_beach() {
        let registry = TerrainRegistry::new();
        let total = 3;
        let mut cells = (0..total * total)
            .map(|_| Cell::new(&registry))
            .collect::<Vec<_>>();
        cells[4].terrain = registry.get_by_id(3);
        let mut map = TestMap { total, cells };
        BeachDetect::new(WorldControlPoints::default(), &registry).apply(&mut map, 0, 0, 0);
        assert!(map.cells[4].terrain.is_same(&registry.get_by_id(4)));
        assert!(
            !map.cells[4]
                .terrain
                .is_same(&TerrainRegistry::new().get_by_id(4))
        );
        assert!(map.cells[0].terrain.is_same(&registry.get_by_id(0)));
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
