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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MutableVeci {
    pub x: i32,
    pub z: i32,
}

impl MutableVeci {
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }
}

pub fn pack_mix_int_float(left: i32, right: f32) -> i64 {
    ((i64::from(left) & 0xffff_ffff) << 32) | i64::from(right.to_bits())
}

pub fn pack_mix_float_int(left: f32, right: i32) -> i64 {
    (i64::from(left.to_bits()) << 32) | (i64::from(right) & 0xffff_ffff)
}

pub fn pack(left: i32, right: i32) -> i64 {
    (i64::from(left) << 32) | (i64::from(right) & 0xffff_ffff)
}

pub fn pack_float(left: f32, right: f32) -> i64 {
    pack(left as i32, right as i32)
}

pub fn unpack_left(packed: i64) -> i32 {
    (packed as u64 >> 32) as i32
}

pub fn unpack_right(packed: i64) -> i32 {
    packed as i32
}

pub fn pack_floats(left: f32, right: f32) -> i64 {
    (i64::from(left.to_bits()) << 32) | i64::from(right.to_bits())
}

pub fn unpack_left_float(packed: i64) -> f32 {
    f32::from_bits((packed as u64 >> 32) as u32)
}

pub fn unpack_right_float(packed: i64) -> f32 {
    f32::from_bits(packed as u32)
}

pub fn iterate<T>(
    start_x: i32,
    start_z: i32,
    width: i32,
    depth: i32,
    context: &mut T,
    mut visitor: impl FnMut(i32, i32, &mut T),
) {
    let size = width.wrapping_mul(depth);
    for index in 0..size {
        let delta_z = index / width;
        let delta_x = index - delta_z * width;
        visitor(
            start_x.wrapping_add(delta_x),
            start_z.wrapping_add(delta_z),
            context,
        );
    }
}

pub const fn contains_int(x: i32, z: i32, x1: i32, z1: i32, x2: i32, z2: i32) -> bool {
    x >= x1 && x < x2 && z >= z1 && z < z2
}

pub const fn contains_float(x: f32, z: f32, x1: f32, z1: f32, x2: f32, z2: f32) -> bool {
    x >= x1 && x < x2 && z >= z1 && z < z2
}

pub struct PosIterator {
    min_x: i32,
    min_z: i32,
    max_x: i32,
    max_y: i32,
    max_z: i32,
    size: i32,
    x: i32,
    y: i32,
    z: i32,
    index: i32,
}

impl PosIterator {
    pub fn new(x: i32, y: i32, z: i32, width: i32, height: i32, length: i32) -> Self {
        Self {
            x: x.wrapping_sub(1),
            y,
            z,
            min_x: x,
            min_z: z,
            max_x: x.wrapping_add(width),
            max_y: y.wrapping_add(height),
            max_z: z.wrapping_add(length),
            size: width
                .wrapping_mul(height)
                .wrapping_mul(length)
                .wrapping_sub(1),
            index: -1,
        }
    }

    pub fn next_position(&mut self) -> bool {
        if self.x.wrapping_add(1) < self.max_x {
            self.x = self.x.wrapping_add(1);
            self.index = self.index.wrapping_add(1);
            return true;
        }
        if self.z.wrapping_add(1) < self.max_z {
            self.x = self.min_x;
            self.z = self.z.wrapping_add(1);
            self.index = self.index.wrapping_add(1);
            return true;
        }
        if self.y.wrapping_add(1) < self.max_y {
            self.x = self.min_x.wrapping_sub(1);
            self.z = self.min_z;
            self.y = self.y.wrapping_add(1);
            return true;
        }
        false
    }

    pub const fn size(&self) -> i32 {
        self.size
    }

    pub const fn index(&self) -> i32 {
        self.index
    }

    pub const fn x(&self) -> i32 {
        self.x
    }

    pub const fn y(&self) -> i32 {
        self.y
    }

    pub const fn z(&self) -> i32 {
        self.z
    }

    pub fn radius_2d(x: i32, z: i32, radius: i32) -> Self {
        let size = radius.wrapping_mul(2).wrapping_add(1);
        Self::new(
            x.wrapping_sub(radius),
            0,
            z.wrapping_sub(radius),
            size,
            0,
            size,
        )
    }

    pub fn radius_3d(x: i32, y: i32, z: i32, radius: i32) -> Self {
        let size = radius.wrapping_mul(2).wrapping_add(1);
        Self::new(
            x.wrapping_sub(radius),
            y.wrapping_sub(radius),
            z.wrapping_sub(radius),
            size,
            size,
            size,
        )
    }

    pub fn area(x: i32, z: i32, width: i32, length: i32) -> Self {
        Self::new(x, 0, z, width, 0, length)
    }

    pub fn volume_3d(x: i32, y: i32, z: i32, width: i32, height: i32, length: i32) -> Self {
        Self::new(x, y, z, width, height, length)
    }

    pub fn range_2d(min_x: i32, min_z: i32, max_x: i32, max_z: i32) -> Self {
        Self::new(
            min_x,
            0,
            min_z,
            max_x.wrapping_sub(min_x),
            0,
            max_z.wrapping_sub(min_z),
        )
    }

    pub fn range_3d(
        min_x: i32,
        min_y: i32,
        min_z: i32,
        max_x: i32,
        max_y: i32,
        max_z: i32,
    ) -> Self {
        Self::new(
            min_x,
            min_y,
            min_z,
            1_i32.wrapping_add(max_x).wrapping_sub(min_x),
            1_i32.wrapping_add(max_y).wrapping_sub(min_y),
            1_i32.wrapping_add(max_z).wrapping_sub(min_z),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_packing_matches_engine_0_3_0() {
        let packed = pack(-123_456_789, 987_654_321);
        assert_eq!(packed as u64, 0xf8a432eb3ade68b1);
        assert_eq!(unpack_left(packed), -123_456_789);
        assert_eq!(unpack_right(packed), 987_654_321);
        assert_eq!(pack_float(-12.75, 98.5) as u64, 0xfffffff400000062);

        let packed = pack_floats(-12.75, f32::from_bits(0x7fc01234));
        assert_eq!(packed as u64, 0xc14c00007fc01234);
        assert_eq!(unpack_left_float(packed).to_bits(), 0xc14c0000);
        assert_eq!(unpack_right_float(packed).to_bits(), 0x7fc01234);
        assert_eq!(pack_mix_int_float(-7, 0.25) as u64, 0xfffffff93e800000);
        assert_eq!(pack_mix_float_int(-0.5, -9) as u64, 0xbf000000fffffff7);
    }

    #[test]
    fn position_iteration_matches_engine_0_3_0() {
        let mut positions = Vec::new();
        let mut iterator = PosIterator::new(1, 2, 3, 2, 2, 2);
        assert_eq!(iterator.size(), 7);
        while iterator.next_position() {
            positions.push((iterator.index(), iterator.x(), iterator.y(), iterator.z()));
        }
        assert_eq!(
            positions,
            [
                (0, 1, 2, 3),
                (1, 2, 2, 3),
                (2, 1, 2, 4),
                (3, 2, 2, 4),
                (3, 0, 3, 3),
                (4, 1, 3, 3),
                (5, 2, 3, 3),
                (6, 1, 3, 4),
                (7, 2, 3, 4),
            ]
        );

        let mut area = PosIterator::area(-1, 5, 3, 2);
        let mut points = Vec::new();
        while area.next_position() {
            points.push((area.x(), area.z()));
        }
        assert_eq!(points, [(-1, 5), (0, 5), (1, 5), (-1, 6), (0, 6), (1, 6)]);

        let mut visited = Vec::new();
        iterate(10, -3, 2, 2, &mut visited, |x, z, output| {
            output.push((x, z))
        });
        assert_eq!(visited, [(10, -3), (11, -3), (10, -2), (11, -2)]);
        assert!(contains_int(1, 2, 0, 0, 2, 3));
        assert!(!contains_int(2, 2, 0, 0, 2, 3));
    }
}
