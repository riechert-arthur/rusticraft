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

use super::terraforged_util::{pack, pack_floats};
use std::{collections::HashMap, sync::Arc};

pub const EPSILON: f32 = 0.99999;
pub const X_PRIME: i32 = 1619;
pub const Y_PRIME: i32 = 31_337;

pub fn random(mut value: i32) -> f32 {
    value ^= X_PRIME;
    value ^= Y_PRIME;
    let value = value
        .wrapping_mul(value)
        .wrapping_mul(value)
        .wrapping_mul(60_493) as f32
        / 2_147_483_648.0_f32;
    (value.clamp(-1.0, 1.0) + 1.0) / 2.0
}

pub trait Weighted {
    fn weight(&self) -> f32;
}

pub struct WeightMap<T> {
    values: Vec<Arc<T>>,
    weights: Vec<f32>,
    sum_weight: f32,
    zero_weight: f32,
}

impl<T> WeightMap<T> {
    pub fn new(values: Vec<Arc<T>>, source_weights: &[f32]) -> Self {
        let mut weight = 0.0;
        let weights = (0..values.len())
            .map(|index| {
                weight += source_weights.get(index).copied().unwrap_or(1.0);
                weight
            })
            .collect();
        Self {
            values,
            weights,
            sum_weight: source_weights.iter().copied().sum::<f32>() * EPSILON,
            zero_weight: source_weights.first().copied().unwrap_or(0.0),
        }
    }

    pub fn of(values: Vec<Arc<T>>) -> Self
    where
        T: Weighted,
    {
        let weights: Vec<_> = values.iter().map(|value| value.weight()).collect();
        Self::new(values, &weights)
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn values(&self) -> &[Arc<T>] {
        &self.values
    }

    pub fn get_value(&self, mut noise: f32) -> Option<&Arc<T>> {
        noise *= self.sum_weight;
        if noise < self.zero_weight {
            return self.values.first();
        }
        for index in 1..self.weights.len() {
            if noise < self.weights[index] {
                return self.values.get(index);
            }
        }
        None
    }

    pub fn find(&self, predicate: impl Fn(&T) -> bool) -> Option<&Arc<T>> {
        self.values.iter().find(|value| predicate(value))
    }

    pub fn band(&self, value: &Arc<T>) -> Option<i64> {
        let mut lower = 0.0;
        for (index, candidate) in self.values.iter().enumerate() {
            let upper = self.weights[index];
            if Arc::ptr_eq(candidate, value) {
                return Some(pack_floats(
                    lower / self.sum_weight,
                    upper / self.sum_weight,
                ));
            }
            lower = upper;
        }
        None
    }
}

pub struct Object2FloatCache<T> {
    capacity: usize,
    values: HashMap<usize, f32>,
    marker: std::marker::PhantomData<T>,
}

impl<T> Object2FloatCache<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            values: HashMap::with_capacity(capacity),
            marker: std::marker::PhantomData,
        }
    }

    pub fn clear(&mut self) {
        self.values.clear();
    }

    pub fn put(&mut self, value: &Arc<T>, sample: f32) {
        assert!(self.values.len() < self.capacity);
        self.values.insert(Arc::as_ptr(value) as usize, sample);
    }

    pub fn get(&self, value: &Arc<T>) -> Option<f32> {
        self.values.get(&(Arc::as_ptr(value) as usize)).copied()
    }
}

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

    pub fn reset(&mut self) {
        self.index = -1;
        self.set_radius(self.min_radius);
    }

    pub fn has_next(&self) -> bool {
        self.index + 1 < self.max_index || self.radius < self.max_radius
    }

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
        self.index += 1;
        if self.index < self.max_index {
            return;
        }
        self.set_radius(self.radius + 1);
        self.index = 0;
    }

    fn set_radius(&mut self, radius: i32) {
        let diameter = 1 + radius * 2;
        self.radius = radius;
        self.length = diameter - 1;
        self.max_index = (diameter - 1) * 4;
    }
}

impl Iterator for SpiralIterator {
    type Item = i64;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.has_next() {
            return None;
        }
        self.next_index();
        let edge = self.index / self.length;
        let step = self.index % self.length;
        let (mut delta_x, mut delta_z) = (-self.radius, -self.radius);
        match edge {
            0 => delta_x = -self.radius + step,
            1 => {
                delta_x = self.radius;
                delta_z = -self.radius + step;
            }
            2 => {
                delta_x = self.radius - step;
                delta_z = self.radius;
            }
            3 => delta_z = self.radius - step,
            _ => {}
        }
        Some(pack(
            self.x.wrapping_add(delta_x),
            self.z.wrapping_add(delta_z),
        ))
    }
}

pub struct PositionFinder<F> {
    iterator: SpiralIterator,
    function: F,
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
    use super::super::terraforged_util::{unpack_left, unpack_left_float, unpack_right};
    use super::*;

    #[derive(Debug)]
    struct Value(f32);

    impl Weighted for Value {
        fn weight(&self) -> f32 {
            self.0
        }
    }

    #[test]
    fn blend_utilities_match_terraforged_source() {
        assert_eq!(random(0).to_bits(), 0x3da79170);
        assert_eq!(random(1).to_bits(), 0x3f1b3a56);
        assert_eq!(random(-1).to_bits(), 0x3ec98b54);
        assert_eq!(random(123_456_789).to_bits(), 0x3ef11bce);

        let values = vec![
            Arc::new(Value(2.0)),
            Arc::new(Value(3.0)),
            Arc::new(Value(5.0)),
        ];
        let map = WeightMap::of(values.clone());
        assert_eq!(map.sum_weight.to_bits(), 0x411fff97);
        assert!(Arc::ptr_eq(map.get_value(0.1).unwrap(), &values[0]));
        assert!(Arc::ptr_eq(map.get_value(0.3).unwrap(), &values[1]));
        assert!(Arc::ptr_eq(map.get_value(0.75).unwrap(), &values[2]));
        let band = map.band(&values[1]).unwrap();
        assert_eq!(unpack_left_float(band).to_bits(), 0x3e4ccd53);
    }

    #[test]
    fn spiral_iterator_matches_terraforged_source() {
        let points: Vec<_> = SpiralIterator::new(3, -2, 1, 2)
            .map(|point| (unpack_left(point), unpack_right(point)))
            .collect();
        assert_eq!(points.len(), 24);
        assert_eq!(
            &points[..8],
            &[
                (2, -3),
                (3, -3),
                (4, -3),
                (4, -2),
                (4, -1),
                (3, -1),
                (2, -1),
                (2, -2)
            ]
        );
        assert_eq!(points[23], (1, -3));
    }
}
