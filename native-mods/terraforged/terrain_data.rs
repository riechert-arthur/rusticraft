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
    terraforged::{clamp, java_max, java_min},
    terraforged_engine::{Terrain, TerrainRegistry},
    terraforged_terrain_levels::TerrainLevels,
};

/// TerraForged's `Index`: `CHUNK` masks x and z into the chunk (`((z & 15) << 4) | (x & 15)`), a bordered
/// chunk offsets them by the border and wraps in Java `int` math. An index outside a map panics where Java
/// throws `ArrayIndexOutOfBoundsException`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Index {
    offset: i32,
    size: i32,
    chunk: bool,
}

impl Index {
    pub const CHUNK: Self = Self {
        offset: 0,
        size: 16,
        chunk: true,
    };

    /// `Index.borderedChunk(border)`: side `16 + border * 2` (Java `int` math).
    pub const fn bordered_chunk(border: i32) -> Self {
        Self {
            offset: border,
            size: 16_i32.wrapping_add(border.wrapping_mul(2)),
            chunk: false,
        }
    }

    pub fn of(self, x: i32, z: i32) -> i32 {
        if self.chunk {
            ((z & 15) << 4) | (x & 15)
        } else {
            let x = x.wrapping_add(self.offset);
            let z = z.wrapping_add(self.offset);
            z.wrapping_mul(self.size).wrapping_add(x)
        }
    }

    /// `of` as a slot of a map's array (a negative index becomes one past every array: it panics there).
    fn slot(self, x: i32, z: i32) -> usize {
        usize::try_from(self.of(x, z)).unwrap_or(usize::MAX)
    }

    /// The maps' array length, `size * size` in Java `int` math; panics where Java throws
    /// `NegativeArraySizeException`.
    pub fn length(self) -> usize {
        usize::try_from(self.size.wrapping_mul(self.size)).expect("negative array size")
    }
}

pub struct FloatMap {
    index: Index,
    data: Vec<f32>,
}

impl FloatMap {
    pub fn new() -> Self {
        Self::with_index(Index::CHUNK)
    }

    pub fn bordered(border: i32) -> Self {
        Self::with_index(Index::bordered_chunk(border))
    }

    fn with_index(index: Index) -> Self {
        Self {
            index,
            data: vec![0.0; index.length()],
        }
    }

    pub const fn index(&self) -> Index {
        self.index
    }

    pub fn get(&self, x: i32, z: i32) -> f32 {
        self.get_index(self.index.slot(x, z))
    }

    pub fn set(&mut self, x: i32, z: i32, value: f32) {
        self.set_index(self.index.slot(x, z), value);
    }

    pub fn get_index(&self, index: usize) -> f32 {
        self.data[index]
    }

    pub fn set_index(&mut self, index: usize, value: f32) {
        self.data[index] = value;
    }
}

impl Default for FloatMap {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ObjectMap<T> {
    index: Index,
    data: Vec<Option<T>>,
}

impl<T> ObjectMap<T> {
    pub fn new() -> Self {
        Self::with_index(Index::CHUNK)
    }

    pub fn bordered(border: i32) -> Self {
        Self::with_index(Index::bordered_chunk(border))
    }

    fn with_index(index: Index) -> Self {
        Self {
            index,
            data: std::iter::repeat_with(|| None)
                .take(index.length())
                .collect(),
        }
    }

    pub const fn index(&self) -> Index {
        self.index
    }

    pub fn get(&self, x: i32, z: i32) -> Option<&T> {
        self.get_index(self.index.slot(x, z))
    }

    pub fn set(&mut self, x: i32, z: i32, value: T) {
        self.set_index(self.index.slot(x, z), value);
    }

    pub fn get_index(&self, index: usize) -> Option<&T> {
        self.data[index].as_ref()
    }

    pub fn set_index(&mut self, index: usize, value: T) {
        self.data[index] = Some(value);
    }

    /// Java's `set` with a possibly null value (`None` clears the slot).
    pub fn set_option(&mut self, x: i32, z: i32, value: Option<T>) {
        let index = self.index.slot(x, z);
        self.data[index] = value;
    }

    pub fn fill(&mut self, mut supplier: impl FnMut() -> T) {
        for value in &mut self.data {
            *value = Some(supplier());
        }
    }
}

impl<T> Default for ObjectMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// TerraForged's `NoiseSample`. Its default terrain is `TerrainType.NONE`: build samples with
/// [`NoiseSample::new`] from the world's registry so that terrain is the registry's own `none`
/// (terrains compare by identity).
#[derive(Clone)]
pub struct NoiseSample {
    pub continent_center: i64,
    pub continent_noise: f32,
    pub base_noise: f32,
    pub height_noise: f32,
    pub river_noise: f32,
    pub terrain_type: Terrain,
}

impl NoiseSample {
    /// `new NoiseSample()`, with `terrains`' `none` as `TerrainType.NONE`.
    pub fn new(terrains: &TerrainRegistry) -> Self {
        Self {
            continent_center: 0,
            continent_noise: 1.0,
            base_noise: 0.0,
            height_noise: 0.0,
            river_noise: 1.0,
            terrain_type: terrains.get_by_id(0),
        }
    }

    /// `reset()`: copies `NoiseSample.DEFAULT` (whose terrain is `terrains`' `none`).
    pub fn reset(&mut self, terrains: &TerrainRegistry) -> &mut Self {
        *self = Self::new(terrains);
        self
    }

    pub fn copy(&mut self, other: &Self) -> &mut Self {
        self.clone_from(other);
        self
    }
}

/// TerraForged's `NoiseData`: one chunk's noise with a one-cell border (x and z from -1 to 16).
/// Cells never set hold Java's defaults: 0.0 noise and no (null) terrain.
pub struct NoiseData {
    sample: NoiseSample,
    height: FloatMap,
    river: FloatMap,
    base: FloatMap,
    terrain: ObjectMap<Terrain>,
}

impl NoiseData {
    pub const BORDER: i32 = 1;
    pub const MIN: i32 = -Self::BORDER;
    pub const MAX: i32 = 16 + Self::BORDER;

    pub fn new(terrains: &TerrainRegistry) -> Self {
        Self {
            sample: NoiseSample::new(terrains),
            height: FloatMap::bordered(Self::BORDER),
            river: FloatMap::bordered(Self::BORDER),
            base: FloatMap::bordered(Self::BORDER),
            terrain: ObjectMap::bordered(Self::BORDER),
        }
    }

    /// `index()`: the bordered index every map shares.
    pub const fn index(&self) -> Index {
        self.height.index()
    }

    /// `getSample`.
    pub const fn sample(&self) -> &NoiseSample {
        &self.sample
    }

    pub fn sample_mut(&mut self) -> &mut NoiseSample {
        &mut self.sample
    }

    pub const fn height(&self) -> &FloatMap {
        &self.height
    }

    pub const fn base(&self) -> &FloatMap {
        &self.base
    }

    pub const fn river(&self) -> &FloatMap {
        &self.river
    }

    pub const fn terrain(&self) -> &ObjectMap<Terrain> {
        &self.terrain
    }

    pub fn set_noise(&mut self, x: i32, z: i32, sample: &NoiseSample) {
        self.set_noise_index(self.height.index().slot(x, z), sample);
    }

    pub fn set_noise_index(&mut self, index: usize, sample: &NoiseSample) {
        self.terrain.set_index(index, sample.terrain_type.clone());
        self.height.set_index(index, sample.height_noise);
        self.base.set_index(index, sample.base_noise);
        self.river.set_index(index, sample.river_noise);
    }

    pub fn is_inside_chunk(x: i32, z: i32) -> bool {
        (-1..=16).contains(&x) && (-1..=16).contains(&z)
    }
}

/// TerraForged's `TerrainData`: one chunk's (16x16, `Index::CHUNK`) heights scaled by `levels`, gradients, river
/// and base levels and terrains, filled by [`TerrainData::accept`] from a [`NoiseData`]. Heights are TerraForged's
/// block Y (`floor(noise * max_y)`, counted from block 0, so with the default `TerrainLevels` up to 480 and not
/// clamped to 26.3's -64..320); mapping them into a 26.3 world is the generator's job, not this type's.
pub struct TerrainData {
    levels: TerrainLevels,
    height: FloatMap,
    gradient: FloatMap,
    river: FloatMap,
    base_height: FloatMap,
    terrain: ObjectMap<Terrain>,
    min: f32,
    max: f32,
    max_base: f32,
}

impl TerrainData {
    pub fn new(levels: TerrainLevels) -> Self {
        Self {
            levels,
            height: FloatMap::new(),
            gradient: FloatMap::new(),
            river: FloatMap::new(),
            base_height: FloatMap::new(),
            terrain: ObjectMap::new(),
            min: f32::MAX,
            max: f32::from_bits(1),
            max_base: f32::from_bits(1),
        }
    }

    pub fn min(&self) -> i32 {
        self.levels.height(self.min)
    }

    pub fn max(&self) -> i32 {
        self.levels.height(self.max)
    }

    pub fn max_base(&self) -> i32 {
        self.levels.height(self.max_base)
    }

    pub fn height_at(&self, x: i32, z: i32) -> i32 {
        self.levels.height(self.height.get(x, z))
    }

    pub fn base_height_at(&self, x: i32, z: i32) -> i32 {
        self.levels.height(self.base_height.get(x, z))
    }

    pub const fn levels(&self) -> TerrainLevels {
        self.levels
    }

    pub const fn height(&self) -> &FloatMap {
        &self.height
    }

    pub const fn base_height(&self) -> &FloatMap {
        &self.base_height
    }

    pub const fn gradient(&self) -> &FloatMap {
        &self.gradient
    }

    pub const fn river(&self) -> &FloatMap {
        &self.river
    }

    pub const fn terrain(&self) -> &ObjectMap<Terrain> {
        &self.terrain
    }

    /// `getGradient(x, z, norm)`: `NoiseUtil.clamp(gradient * norm, 0, 1)` (NaN passes through).
    pub fn normalized_gradient(&self, x: i32, z: i32, norm: f32) -> f32 {
        clamp(self.gradient.get(x, z) * norm, 0.0, 1.0)
    }

    pub fn accept(&mut self, noise_data: &NoiseData) {
        for z in 0..16 {
            for x in 0..16 {
                let height_noise = noise_data.height.get(x, z);
                let base_level_noise = noise_data.base.get(x, z);
                let scaled_height = self.levels.scaled_height(height_noise);
                let scaled_base_level = self.levels.scaled_base_level(base_level_noise);
                let terrain_type = noise_data.terrain.get(x, z).cloned();
                let north = noise_data.height.get(x, z - 1);
                let south = noise_data.height.get(x, z + 1);
                let east = noise_data.height.get(x + 1, z);
                let west = noise_data.height.get(x - 1, z);
                let dx = east - west;
                let dz = south - north;
                // NoiseUtil.sqrt: (float) Math.sqrt(double).
                let grad = f64::from(dx * dx + dz * dz).sqrt() as f32;
                let noise_gradient = clamp(grad, 0.0, 1.0);

                self.gradient.set(x, z, noise_gradient);
                self.terrain.set_option(x, z, terrain_type);
                self.height.set(x, z, scaled_height);
                self.base_height.set(x, z, scaled_base_level);
                self.river.set(x, z, noise_data.river.get(x, z));
                self.max = java_max(self.max, scaled_height);
                self.min = java_min(self.min, scaled_height);
                self.max_base = java_max(self.max_base, scaled_base_level);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write;

    /// `Float.floatToIntBits`: every NaN prints as 7fc00000.
    fn h(v: f32) -> String {
        format!("{:08x}", if v.is_nan() { 0x7fc0_0000 } else { v.to_bits() })
    }

    fn fnv(text: &str) -> u64 {
        text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
    }

    const VALUES: [f32; 20] = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.25,
        -0.25,
        0.5,
        0.999999,
        1.5,
        -480.0,
        62.5,
        1e-3,
        f32::from_bits(1),
        -f32::from_bits(1),
        3e9,
        -3e9,
        f32::MAX,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
    ];
    const RIVERS: [f32; 6] = [0.0, -0.0, 1.0, 0.5, f32::NAN, f32::from_bits(1)];

    /// TerrainDataOracle's xorshift32 (Java `int` shifts).
    struct Draws(u32);

    impl Draws {
        fn next(&mut self) -> u32 {
            let mut r = self.0;
            r ^= r << 13;
            r ^= r >> 17;
            r ^= r << 5;
            self.0 = r;
            r
        }

        fn pick(&mut self, n: u32) -> usize {
            ((self.next() >> 1) % n) as usize
        }

        fn unit(&mut self) -> f32 {
            (self.next() >> 8) as f32 * f32::from_bits(0x3380_0000)
        }
    }

    fn id(terrain: Option<&Terrain>) -> i32 {
        terrain.map_or(-1, Terrain::id)
    }

    fn chunk(kind: u32, terrains: &TerrainRegistry) -> NoiseData {
        let mut draws = Draws(0x9e37_79b9_u32.wrapping_add(kind));
        let mut data = NoiseData::new(terrains);
        for z in NoiseData::MIN..NoiseData::MAX {
            for x in NoiseData::MIN..NoiseData::MAX {
                let mut sample = data.sample_mut().reset(terrains).clone();
                match kind {
                    0 => {
                        sample.height_noise = 0.25 + (x * 7 + z * 3) as f32 / 512.0;
                        sample.base_noise = (x - z) as f32 / 64.0 + 0.5;
                        sample.river_noise = if (x + z) & 3 == 0 {
                            0.0
                        } else {
                            (x * z) as f32 / 300.0
                        };
                        sample.terrain_type = terrains.get_by_id((x * 3 + z * 5) & 15);
                    }
                    1 | 3 => {
                        if kind == 3 && draws.next() & 3 == 0 {
                            continue;
                        }
                        sample.height_noise = draws.unit();
                        sample.base_noise = draws.unit();
                        sample.river_noise = RIVERS[draws.pick(6)];
                        sample.terrain_type = terrains.get_by_id(draws.pick(16) as i32);
                    }
                    _ => {
                        sample.height_noise = VALUES[draws.pick(20)];
                        sample.base_noise = VALUES[draws.pick(20)];
                        sample.river_noise = VALUES[draws.pick(20)];
                        sample.terrain_type = terrains.get_by_id(draws.pick(16) as i32);
                    }
                }
                if kind == 1 && x == 5 && z == 5 {
                    let index = data.index().of(x, z) as usize;
                    data.set_noise_index(index, &sample);
                } else {
                    data.set_noise(x, z, &sample);
                }
            }
        }
        data
    }

    fn extrema(data: &TerrainData) -> String {
        format!(
            "{} {} {} {} {} {}",
            h(data.min),
            h(data.max),
            h(data.max_base),
            data.min(),
            data.max(),
            data.max_base()
        )
    }

    fn water(x: i32, z: i32, sea_level: i32, data: &TerrainData) -> String {
        match data.terrain().get(x, z) {
            None => "N".into(),
            Some(_) => TerrainLevels::water_level(x, z, sea_level, data).to_string(),
        }
    }

    /// Replays `TerrainDataOracle.java` (`.local/terraforged/oracles/terrain_data/`), which runs TerraForged's own
    /// classes from the mod jar: indices over negative and huge coordinates, four bordered noise chunks (smooth,
    /// random, special values with NaN/infinities/signed zeros/subnormals, sparse with unset cells), each read
    /// into `TerrainData` with four `TerrainLevels`; every array element, height, gradient and water level, the
    /// extrema, and world-coordinate reads of chunk (-3, -1), bit for bit.
    #[test]
    fn terrain_data_matches_terraforged_oracle() {
        let terrains = TerrainRegistry::new();
        let mut out = String::new();
        let coords = [
            0,
            1,
            -1,
            15,
            16,
            17,
            -16,
            -17,
            31,
            -37,
            1000,
            -1000,
            i32::MAX,
            i32::MIN,
        ];
        let indices = [
            Index::CHUNK,
            Index::bordered_chunk(0),
            Index::bordered_chunk(1),
            Index::bordered_chunk(3),
            Index::bordered_chunk(i32::MAX),
            Index::bordered_chunk(-9),
        ];
        for (i, index) in indices.iter().enumerate() {
            write!(out, "I {i}").unwrap();
            for z in coords {
                for x in coords {
                    write!(out, " {}", index.of(x, z)).unwrap();
                }
            }
            out.push('\n');
        }
        writeln!(
            out,
            "INSIDE {} {} {} {}",
            NoiseData::is_inside_chunk(-1, 16),
            NoiseData::is_inside_chunk(-2, 0),
            NoiseData::is_inside_chunk(17, 0),
            NoiseData::is_inside_chunk(0, -1)
        )
        .unwrap();
        let levels = [
            TerrainLevels::default(),
            TerrainLevels::unconfigured(),
            TerrainLevels::new(true, 2.5, -32, 300, 100, 80, 20),
            TerrainLevels::new(true, 1.0, -64, 320, 128, 63, 23),
        ];
        let norms = [2.0, 0.5, -1.0, f32::NAN, f32::INFINITY];
        for kind in 0..4 {
            let noise = chunk(kind, &terrains);
            write!(out, "N {kind}").unwrap();
            for z in NoiseData::MIN..NoiseData::MAX {
                for x in NoiseData::MIN..NoiseData::MAX {
                    write!(
                        out,
                        " {},{},{},{}",
                        h(noise.height().get(x, z)),
                        h(noise.base().get(x, z)),
                        h(noise.river().get(x, z)),
                        id(noise.terrain().get(x, z))
                    )
                    .unwrap();
                }
            }
            out.push('\n');
            for (l, levels) in levels.iter().enumerate() {
                let mut data = TerrainData::new(*levels);
                writeln!(out, "E0 {kind} {l} {}", extrema(&data)).unwrap();
                data.accept(&noise);
                writeln!(out, "E {kind} {l} {}", extrema(&data)).unwrap();
                for z in 0..16 {
                    write!(out, "C {kind} {l} {z}").unwrap();
                    for x in 0..16 {
                        write!(
                            out,
                            " {},{},{},{},{},{},{}",
                            h(data.height().get(x, z)),
                            h(data.base_height().get(x, z)),
                            h(data.gradient().get(x, z)),
                            h(data.river().get(x, z)),
                            id(data.terrain().get(x, z)),
                            data.height_at(x, z),
                            data.base_height_at(x, z)
                        )
                        .unwrap();
                        for norm in norms {
                            write!(out, ",{}", h(data.normalized_gradient(x, z, norm))).unwrap();
                        }
                        write!(out, ",{}", water(x, z, 63, &data)).unwrap();
                    }
                    out.push('\n');
                }
                write!(out, "W {kind} {l}").unwrap();
                for z in coords {
                    for x in coords {
                        write!(out, " {},{}", data.height_at(x, z), water(x, z, -5, &data))
                            .unwrap();
                    }
                }
                for lz in (0..16).step_by(5) {
                    for lx in (0..16).step_by(3) {
                        let same = data.height_at(-3 * 16 + lx, -16 + lz) == data.height_at(lx, lz);
                        write!(out, " {same}").unwrap();
                    }
                }
                out.push('\n');
            }
        }
        // Spot lines (the oracle's out.txt), then the whole output.
        for line in [
            "INSIDE true false false true",
            "E0 0 0 7f7fffff 00000001 00000001 2147483647 0 0",
            "E 0 0 42f00000 43825000 431c0000 120 260 156",
            "E 1 2 400c15b8 4395be28 4333eaa2 2 299 179",
            "E 2 3 7fc00000 7fc00000 7fc00000 -1 -1 -1",
            "E 3 3 00000000 439c4703 433c9f2f 0 312 188",
        ] {
            assert!(out.lines().any(|l| l == line), "missing oracle line {line}");
        }
        assert_eq!(out.lines().count(), 315);
        assert_eq!(fnv(&out), 0x7bfd_a5e4_dd00_6d65);
    }

    #[test]
    fn noise_samples_default_to_the_world_registry_none() {
        let terrains = TerrainRegistry::new();
        let none = terrains.get_by_id(0);
        let mut sample = NoiseSample::new(&terrains);
        assert!(sample.terrain_type.is_same(&none));
        sample.height_noise = 0.5;
        sample.terrain_type = terrains.get_by_id(5);
        let other = sample.clone();
        sample.reset(&terrains);
        assert!(sample.terrain_type.is_same(&none));
        assert_eq!((sample.height_noise, sample.river_noise), (0.0, 1.0));
        sample.copy(&other);
        assert!(sample.terrain_type.is_same(&terrains.get_by_id(5)));
        let noise = NoiseData::new(&terrains);
        assert!(noise.terrain().get(0, 0).is_none());
    }

    #[test]
    #[should_panic]
    fn reads_outside_a_bordered_map_panic_like_java() {
        FloatMap::bordered(1).get(-2, -1);
    }
}
