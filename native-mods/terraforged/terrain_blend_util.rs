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
    terraforged::map,
    terraforged_util::{pack, pack_floats},
};
use std::sync::Arc;

/// `MathUtil.EPSILON`: `WeightMap` scales its weight sum by it, so a noise of exactly 1 still picks the last value.
pub const EPSILON: f32 = 0.99999;
/// `NoiseUtil.X_PRIME`, which `MathUtil.rand(int)` mixes in.
pub const X_PRIME: i32 = 1619;
/// `NoiseUtil.Y_PRIME`, which `MathUtil.rand(int)` mixes in.
pub const Y_PRIME: i32 = 31_337;

/// `MathUtil.rand(int)`: a hash to 0..1, in Java `int` math (`n * n * n * 60493` wraps), then
/// `NoiseUtil.map(value, -1, 1, 2)`.
pub fn random(mut value: i32) -> f32 {
    value ^= X_PRIME;
    value ^= Y_PRIME;
    let value = value
        .wrapping_mul(value)
        .wrapping_mul(value)
        .wrapping_mul(60_493) as f32
        / 2_147_483_648.0_f32;
    map(value, -1.0, 1.0, 2.0)
}

/// `WeightMap.Weighted`: a value's own weight, read by [`WeightMap::of`].
pub trait Weighted {
    fn weight(&self) -> f32;
}

/// TerraForged's `WeightMap` (1.18.2-0.3.1, `util.map`): values picked by a 0..1 noise over bands as wide as their
/// weights. Values are compared by identity (`Arc::ptr_eq`, Java's `==`).
pub struct WeightMap<T> {
    values: Vec<Arc<T>>,
    weights: Vec<f32>,
    sum_weight: f32,
    zero_weight: f32,
}

impl<T> WeightMap<T> {
    /// `new WeightMap(values, weights)`: one cumulative weight per value (a value past the end of `weights`
    /// weighs 1), but the sum and the first band come from `weights` itself, whatever its length. The sum starts
    /// at +0 and adds in order, as `MathUtil.sum` does.
    pub fn new(values: Vec<Arc<T>>, source_weights: &[f32]) -> Self {
        let mut weight = 0.0_f32;
        let weights = (0..values.len())
            .map(|index| {
                weight += source_weights.get(index).copied().unwrap_or(1.0);
                weight
            })
            .collect();
        let sum = source_weights
            .iter()
            .fold(0.0_f32, |sum, weight| sum + weight);
        Self {
            values,
            weights,
            sum_weight: sum * EPSILON,
            zero_weight: source_weights.first().copied().unwrap_or(0.0),
        }
    }

    /// `WeightMap.of(values)`: each value's [`Weighted::weight`].
    pub fn of(values: Vec<Arc<T>>) -> Self
    where
        T: Weighted,
    {
        let weights: Vec<_> = values.iter().map(|value| value.weight()).collect();
        Self::new(values, &weights)
    }

    /// `isEmpty`.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// `getValues`.
    pub fn values(&self) -> &[Arc<T>] {
        &self.values
    }

    /// `getValue(noise)`: the first value whose band holds `noise * sum`; `None` where Java returns null (past the
    /// last band, or NaN).
    pub fn get_value(&self, mut noise: f32) -> Option<&Arc<T>> {
        noise *= self.sum_weight;
        if noise < self.zero_weight {
            return self.values.first();
        }
        (1..self.weights.len())
            .find(|&index| noise < self.weights[index])
            .map(|index| &self.values[index])
    }

    /// `find(predicate)`: the first matching value.
    pub fn find(&self, predicate: impl Fn(&T) -> bool) -> Option<&Arc<T>> {
        self.values.iter().find(|value| predicate(value))
    }

    /// `getBand(value)`: `PosUtil.packf(lower / sum, upper / sum)` of the value's band, found by identity; 0 (Java's
    /// `0L`) when the value is not in the map.
    pub fn band(&self, value: &Arc<T>) -> i64 {
        let mut lower = 0.0_f32;
        for (candidate, &upper) in self.values.iter().zip(&self.weights) {
            if Arc::ptr_eq(candidate, value) {
                return pack_floats(lower / self.sum_weight, upper / self.sum_weight);
            }
            lower = upper;
        }
        0
    }
}

/// TerraForged's `Object2FloatCache` (1.18.2-0.3.1): samples keyed by identity in `capacity` preallocated slots.
/// Kept exactly: `put` always writes slot `size()` and points the key at it, so putting a key again moves it to
/// the next free slot, which the next new key then shares (and overwrites). `put` with every slot taken panics
/// where Java throws `ArrayIndexOutOfBoundsException`, even for a key already present. Keys are held (as Java's
/// `IdentityHashMap` holds them) until `clear`.
pub struct Object2FloatCache<T> {
    slots: Vec<f32>,
    keys: Vec<(Arc<T>, usize)>,
}

impl<T> Object2FloatCache<T> {
    /// `new Object2FloatCache(size)`.
    pub fn new(capacity: usize) -> Self {
        Self {
            slots: vec![0.0; capacity],
            keys: Vec::with_capacity(capacity),
        }
    }

    /// `clear`: forgets the keys; slots keep their values.
    pub fn clear(&mut self) {
        self.keys.clear();
    }

    /// `put(key, sample)`.
    pub fn put(&mut self, key: &Arc<T>, sample: f32) {
        let index = self.keys.len();
        self.slots[index] = sample;
        match self
            .keys
            .iter_mut()
            .find(|(held, _)| Arc::ptr_eq(held, key))
        {
            Some(entry) => entry.1 = index,
            None => self.keys.push((Arc::clone(key), index)),
        }
    }

    /// `get(key)`: the key's slot, or NaN (`Float.NaN`) for a key not put since the last `clear`.
    pub fn get(&self, key: &Arc<T>) -> f32 {
        self.keys
            .iter()
            .find(|(held, _)| Arc::ptr_eq(held, key))
            .map_or(f32::NAN, |&(_, index)| self.slots[index])
    }
}

/// TerraForged's `SpiralIterator` (1.18.2-0.3.1): square rings from `radius` to `max_radius` around `(x, z)`,
/// each walked from its corner `(-r, -r)` along +x, +z, -x, then -z, as `PosUtil.pack`ed positions. All `int`
/// math wraps as Java's. As an [`Iterator`], `next` returns `None` once `has_next` is false (Java's `next` never
/// checks; callers loop on `hasNext`). Java's quirks are kept: a start radius above `max_radius` yields the same
/// position forever, and stepping into ring 0 (from a negative start radius) panics where Java divides by zero.
pub struct SpiralIterator {
    x: i32,
    z: i32,
    min_radius: i32,
    max_radius: i32,
    index: i32,
    radius: i32,
    length: i32,
    max_index: i32,
}

impl SpiralIterator {
    /// `new SpiralIterator(x, z, radius, maxRadius)`.
    pub fn new(x: i32, z: i32, radius: i32, max_radius: i32) -> Self {
        let mut iterator = Self {
            x,
            z,
            min_radius: radius,
            max_radius,
            index: -1,
            radius,
            length: 0,
            max_index: 0,
        };
        iterator.set_radius(radius);
        iterator
    }

    /// `reset`: back to the first ring.
    pub fn reset(&mut self) {
        self.index = -1;
        self.set_radius(self.min_radius);
    }

    /// `hasNext`.
    pub fn has_next(&self) -> bool {
        self.index.wrapping_add(1) < self.max_index || self.radius < self.max_radius
    }

    /// `finder(function)`: a [`PositionFinder`] calling `function` on this iterator.
    pub fn finder<F>(self, function: F) -> PositionFinder<F>
    where
        F: FnMut(&mut SpiralIterator) -> i64,
    {
        PositionFinder {
            iterator: self,
            function,
        }
    }

    fn next_index(&mut self) {
        if self.radius > self.max_radius {
            return;
        }
        self.index = self.index.wrapping_add(1);
        if self.index < self.max_index {
            return;
        }
        self.set_radius(self.radius.wrapping_add(1));
        self.index = 0;
    }

    fn set_radius(&mut self, radius: i32) {
        let diameter = 1_i32.wrapping_add(radius.wrapping_mul(2));
        self.radius = radius;
        self.length = diameter.wrapping_sub(1);
        self.max_index = diameter.wrapping_sub(1).wrapping_mul(4);
    }
}

impl Iterator for SpiralIterator {
    type Item = i64;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.has_next() {
            return None;
        }
        self.next_index();
        // Java's `idiv`/`irem`: a zero length panics as Java throws; `wrapping_*` covers `MIN / -1`.
        let edge = self.index.wrapping_div(self.length);
        let step = self.index.wrapping_rem(self.length);
        let radius = self.radius;
        let (mut delta_x, mut delta_z) = (radius.wrapping_neg(), radius.wrapping_neg());
        match edge {
            0 => delta_x = radius.wrapping_neg().wrapping_add(step),
            1 => {
                delta_x = radius;
                delta_z = radius.wrapping_neg().wrapping_add(step);
            }
            2 => {
                delta_x = radius.wrapping_sub(step);
                delta_z = radius;
            }
            3 => delta_z = radius.wrapping_sub(step),
            _ => {}
        }
        Some(pack(
            self.x.wrapping_add(delta_x),
            self.z.wrapping_add(delta_z),
        ))
    }
}

/// `SpiralIterator.PositionFinder`: while the spiral has positions, each `next` returns `function(spiral)`;
/// `None` where Java's `next` returns `0L`.
pub struct PositionFinder<F> {
    iterator: SpiralIterator,
    function: F,
}

impl<F> PositionFinder<F> {
    /// `hasNext`: the spiral's.
    pub fn has_next(&self) -> bool {
        self.iterator.has_next()
    }
}

impl<F: FnMut(&mut SpiralIterator) -> i64> Iterator for PositionFinder<F> {
    type Item = i64;

    fn next(&mut self) -> Option<Self::Item> {
        self.iterator
            .has_next()
            .then(|| (self.function)(&mut self.iterator))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fmt::Write,
        panic::{AssertUnwindSafe, catch_unwind},
    };

    /// `Float.floatToIntBits`: every NaN prints as 7fc00000.
    fn h(v: f32) -> String {
        format!("{:08x}", if v.is_nan() { 0x7fc0_0000 } else { v.to_bits() })
    }

    fn fnv(text: &str) -> u64 {
        text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
    }

    const S: f32 = f32::from_bits(1);

    const VALUES: [f32; 26] = [
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
        S,
        -S,
        3e9,
        -3e9,
        f32::MAX,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
        0.1,
        0.3,
        0.75,
        0.2,
        0.9999,
        1.0001,
    ];

    /// The oracle's weight arrays and value counts.
    fn cases() -> Vec<(Vec<f32>, usize)> {
        vec![
            (vec![2.0, 3.0, 5.0], 3),
            (vec![1.0], 1),
            (vec![], 0),
            (vec![], 3),
            (vec![0.5, 0.25], 4),
            (vec![1.0, 2.0, 3.0, 4.0], 2),
            (vec![-0.0, -0.0], 2),
            (vec![f32::NAN, 1.0], 2),
            (vec![f32::INFINITY, 1.0], 2),
            (vec![1.0, f32::NEG_INFINITY, 2.0], 3),
            (vec![S, S * 3.0], 2),
            (vec![f32::MAX, f32::MAX], 2),
            (vec![-1.0, 2.0, 0.1], 3),
            (vec![1e-3, 3e9, 1.0], 3),
            (vec![0.0, 0.0, 1.0], 3),
        ]
    }

    fn index(values: &[Arc<u8>], value: Option<&Arc<u8>>) -> i32 {
        value.map_or(-1, |value| {
            values
                .iter()
                .position(|candidate| Arc::ptr_eq(candidate, value))
                .map_or(-2, |index| index as i32)
        })
    }

    fn spiral(out: &mut String, x: i32, z: i32, r: i32, max: i32) {
        let mut iterator = SpiralIterator::new(x, z, r, max);
        write!(out, "S {x} {z} {r} {max} |").unwrap();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut points = String::new();
            for _ in 0..100 {
                let Some(point) = iterator.next() else { break };
                write!(points, " {point}").unwrap();
            }
            points
        }));
        match result {
            Ok(points) => writeln!(out, "{points} | {}", iterator.has_next()).unwrap(),
            Err(_) => {
                // Java printed the points (and the space before the throwing `next`) first.
                let mut replay = SpiralIterator::new(x, z, r, max);
                let mut points = String::new();
                while let Ok(Some(point)) = catch_unwind(AssertUnwindSafe(|| replay.next())) {
                    write!(points, " {point}").unwrap();
                }
                writeln!(out, "{points}  throw").unwrap();
            }
        }
    }

    fn oracle_output() -> String {
        let mut out = String::from("R");
        let specials = [
            0,
            1,
            -1,
            i32::MIN,
            i32::MAX,
            1619,
            31337,
            1619 ^ 31337,
            60493,
            123_456_789,
        ];
        for value in specials {
            write!(out, " {}", h(random(value))).unwrap();
        }
        for i in 0..200_i32 {
            write!(
                out,
                " {}",
                h(random(i.wrapping_mul(0x9E37_79B1_u32 as i32)))
            )
            .unwrap();
        }
        out.push('\n');

        for (case, (weights, count)) in cases().into_iter().enumerate() {
            let values: Vec<_> = (0..count).map(|_| Arc::new(0_u8)).collect();
            let map = WeightMap::new(values.clone(), &weights);
            let cumulative: Vec<_> = map.weights.iter().map(|&w| h(w)).collect();
            write!(
                out,
                "M {case} {} {} {} [{}] |",
                map.is_empty(),
                h(map.sum_weight),
                h(map.zero_weight),
                cumulative.join(",")
            )
            .unwrap();
            for noise in VALUES {
                write!(out, " {}", index(&values, map.get_value(noise))).unwrap();
            }
            out.push_str(" |");
            for value in &values {
                write!(out, " {:x}", map.band(value) as u64).unwrap();
            }
            write!(out, " {:x}", map.band(&Arc::new(0)) as u64).unwrap();
            let last = values.last().cloned();
            let found = map.find(|value| {
                last.as_ref()
                    .is_some_and(|last| std::ptr::eq(value, &**last))
            });
            writeln!(out, " | {}", index(&values, found)).unwrap();
        }

        let (a, b, c, d) = (
            Arc::new(0_u8),
            Arc::new(0_u8),
            Arc::new(0_u8),
            Arc::new(0_u8),
        );
        let mut cache = Object2FloatCache::new(3);
        let mut k = String::from("C");
        let get = |k: &mut String, cache: &Object2FloatCache<u8>, key: &Arc<u8>| {
            write!(k, " {}", h(cache.get(key))).unwrap();
        };
        let put = |k: &mut String, cache: &mut Object2FloatCache<u8>, key: &Arc<u8>, v: f32| {
            let thrown = catch_unwind(AssertUnwindSafe(|| cache.put(key, v))).is_err();
            k.push_str(if thrown { " throw" } else { " ok" });
        };
        cache.put(&a, 1.0);
        get(&mut k, &cache, &a);
        get(&mut k, &cache, &b);
        cache.put(&b, 2.0);
        cache.put(&a, 3.0);
        get(&mut k, &cache, &a);
        get(&mut k, &cache, &b);
        cache.put(&c, 4.0);
        get(&mut k, &cache, &a);
        get(&mut k, &cache, &b);
        get(&mut k, &cache, &c);
        cache.clear();
        get(&mut k, &cache, &a);
        get(&mut k, &cache, &c);
        cache.put(&d, 5.0);
        cache.put(&a, f32::NAN);
        cache.put(&a, -0.0);
        get(&mut k, &cache, &d);
        get(&mut k, &cache, &a);
        cache.put(&b, 7.0);
        get(&mut k, &cache, &a);
        get(&mut k, &cache, &b);
        put(&mut k, &mut cache, &c, 8.0);
        get(&mut k, &cache, &c);
        cache.clear();
        cache.put(&a, 1.0);
        cache.put(&b, 2.0);
        cache.put(&c, 3.0);
        put(&mut k, &mut cache, &a, 9.0);
        get(&mut k, &cache, &a);
        put(&mut k, &mut Object2FloatCache::new(0), &a, 1.0);
        out.push_str(&k);
        out.push('\n');

        for (x, z, r, max) in [
            (3, -2, 1, 2),
            (0, 0, 0, 2),
            (0, 0, 0, 0),
            (0, 0, 0, -1),
            (5, 5, 2, 2),
            (5, 5, 3, 1),
            (-7, 9, 1, 1),
            (i32::MAX, i32::MIN, 1, 2),
            (0, 0, -1, 1),
            (0, 0, -2, 0),
            (0, 0, 1 << 30, 1 << 30),
            (0, 0, (1 << 30) - 1, 1 << 30),
            (0, 0, i32::MIN, 0),
            (100, -100, 2, 4),
        ] {
            spiral(&mut out, x, z, r, max);
        }

        let mut iterator = SpiralIterator::new(3, -2, 1, 2);
        out.push('T');
        for _ in 0..5 {
            write!(out, " {}", iterator.next().unwrap()).unwrap();
        }
        iterator.reset();
        out.push_str(" |");
        for _ in 0..3 {
            write!(out, " {}", iterator.next().unwrap()).unwrap();
        }
        out.push('\n');

        let mut finder = SpiralIterator::new(-1, 4, 1, 2).finder(|spiral| {
            let point = spiral.next().unwrap();
            point
                .wrapping_mul(31)
                .wrapping_add(i64::from(spiral.has_next()))
        });
        out.push('F');
        for _ in 0..30 {
            assert_eq!(finder.has_next(), finder.iterator.has_next());
            match finder.next() {
                Some(value) => write!(out, " {value}").unwrap(),
                None => out.push_str(" end"),
            }
        }
        out.push('\n');
        out
    }

    /// Replays `TerrainBlendUtilOracle.java` (`.local/terraforged/oracles/terrain_blend_util/`), which runs the
    /// 1.18.2-0.3.1 jar's `MathUtil.rand`, `WeightMap`, `Object2FloatCache` and `SpiralIterator`.
    #[test]
    fn blend_utilities_match_terraforged_jar() {
        let out = oracle_output();
        let lines: Vec<_> = out.lines().collect();
        assert_eq!(lines.len(), 33);
        assert!(lines[0].starts_with("R 3da79170 3f1b3a56 3ec98b54 3da79170 3f64c5aa 3f63c722 "));
        assert_eq!(
            lines[1],
            "M 0 false 411fff97 40000000 [40000000,40a00000,41200000] | 0 0 2 0 1 0 1 2 -1 0 -1 0 0 0 -1 0 \
             -1 -1 0 -1 0 1 2 0 2 -1 | 3e4ccd53 3e4ccd533f000054 3f0000543f800054 0 | 2"
        );
        assert_eq!(
            lines[4],
            "M 3 false 00000000 00000000 [3f800000,40000000,40400000] | 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 \
             -1 -1 -1 1 1 1 1 1 1 | ffc000007f800000 7f8000007f800000 7f8000007f800000 0 | 2"
        );
        assert_eq!(
            lines[7],
            "M 6 false 00000000 80000000 [00000000,00000000] | -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 \
             -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 | ffc00000ffc00000 ffc00000ffc00000 0 | 1"
        );
        assert_eq!(
            lines[16],
            "C 3f800000 7fc00000 40400000 40000000 40800000 40000000 40800000 7fc00000 7fc00000 40a00000 \
             80000000 40e00000 40e00000 throw 7fc00000 throw 3f800000 throw"
        );
        assert_eq!(lines[25], "S 0 0 -1 1 |  throw");
        assert_eq!(lines[26], "S 0 0 -2 0 | 4294967297  throw");
        assert_eq!(
            lines[28],
            "S 0 0 1073741823 1073741824 | -4611686015206162432 | false"
        );
        assert_eq!(
            lines[31],
            "T 12884901885 17179869181 21474836477 21474836478 21474836479 | 12884901885 17179869181 21474836477"
        );
        assert_eq!(fnv(&out), FNV);
    }

    const FNV: u64 = 0xb91d_9037_91ff_1af7;
}
