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

// Engine 0.3.0's position helpers: `util.pos.PosUtil`, `util.pos.PosIterator` and
// `world.continent.MutableVeci`.

/// Engine's `MutableVeci`: a mutable 2D integer vector.
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

/// `PosUtil.packMix(int, float)`: `left` in the high half, `right`'s raw bits (`floatToRawIntBits`, NaN
/// payloads kept) in the low half.
pub fn pack_mix_int_float(left: i32, right: f32) -> i64 {
    ((i64::from(left) & 0xffff_ffff) << 32) | i64::from(right.to_bits())
}

/// `PosUtil.packMix(float, int)`: `left`'s raw bits in the high half, `right` in the low half.
pub fn pack_mix_float_int(left: f32, right: i32) -> i64 {
    (i64::from(left.to_bits()) << 32) | (i64::from(right) & 0xffff_ffff)
}

/// `PosUtil.pack(int, int)`: `left` in the high half, `right` in the low half.
pub fn pack(left: i32, right: i32) -> i64 {
    (i64::from(left) << 32) | (i64::from(right) & 0xffff_ffff)
}

/// `PosUtil.pack(float, float)`: both halves truncated like Java's `(int)` (saturating, NaN to 0).
pub fn pack_float(left: f32, right: f32) -> i64 {
    pack(left as i32, right as i32)
}

/// `PosUtil.unpackLeft`: the high half.
pub fn unpack_left(packed: i64) -> i32 {
    (packed as u64 >> 32) as i32
}

/// `PosUtil.unpackRight`: the low half.
pub fn unpack_right(packed: i64) -> i32 {
    packed as i32
}

/// `PosUtil.packf`: both floats' raw bits (`floatToRawIntBits`, NaN payloads kept).
pub fn pack_floats(left: f32, right: f32) -> i64 {
    (i64::from(left.to_bits()) << 32) | i64::from(right.to_bits())
}

/// `PosUtil.unpackLeftf`: the high half's bits as a float.
pub fn unpack_left_float(packed: i64) -> f32 {
    f32::from_bits((packed as u64 >> 32) as u32)
}

/// `PosUtil.unpackRightf`: the low half's bits as a float.
pub fn unpack_right_float(packed: i64) -> f32 {
    f32::from_bits(packed as u32)
}

/// `PosUtil.iterate`: visits `width * depth` (Java `int` product) positions row by row along x.
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

/// `PosUtil.contains(int...)`: half-open `[x1, x2) × [z1, z2)`.
pub const fn contains_int(x: i32, z: i32, x1: i32, z1: i32, x2: i32, z2: i32) -> bool {
    x >= x1 && x < x2 && z >= z1 && z < z2
}

/// `PosUtil.contains(float...)`: half-open, so NaN is never contained.
pub const fn contains_float(x: f32, z: f32, x1: f32, z1: f32, x2: f32, z2: f32) -> bool {
    x >= x1 && x < x2 && z >= z1 && z < z2
}

/// Engine's `PosIterator`: walks x, then z, then y. Moving to the next y layer does not advance
/// `index`, and `size` is the Java `width * height * length - 1` (wrapping), as in Engine.
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

    /// `PosIterator.next`.
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

    /// Engine's six-argument `range2D` overload: inclusive bounds on all three axes.
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

    /// FNV-style fold the Java oracle (`.local/terraforged/oracles/posutil/`) uses: `h = h * 1_000_003 ^ v`
    /// with Java `long` wrap, `v` widened from `int` with sign extension.
    fn mix(h: &mut i64, v: i64) {
        *h = h.wrapping_mul(1_000_003) ^ v;
    }

    const INTS: [i32; 8] = [
        0,
        -1,
        1,
        i32::MIN,
        i32::MAX,
        -123_456_789,
        i32::MAX - 1,
        i32::MIN + 1,
    ];
    // Zeros, ±1, ±inf, quiet and signalling NaNs with payloads, subnormals and the `(int)` saturation edges.
    const FLOAT_BITS: [u32; 22] = [
        0x0000_0000,
        0x8000_0000,
        0x3f80_0000,
        0xc14c_0000,
        0x3e80_0000,
        0x7f80_0000,
        0xff80_0000,
        0x7fc0_0000,
        0xffc0_0000,
        0x7fc0_1234,
        0xffc0_0001,
        0x7f80_0001,
        0xff81_2345,
        0x7fbf_ffff,
        0x0000_0001,
        0x807f_ffff,
        0x4f00_0000,
        0xcf00_0000,
        0xcf00_0001,
        0x4eff_ffff,
        0x3f7f_ffff,
        0xbf7f_ffff,
    ];

    #[test]
    fn packing_matches_engine_0_3_0_over_edge_values() {
        let mut h = 17;
        for l in INTS {
            for r in INTS {
                let p = pack(l, r);
                mix(&mut h, p);
                mix(&mut h, unpack_left(p).into());
                mix(&mut h, unpack_right(p).into());
            }
        }
        assert_eq!(h as u64, 0x82d1dfd0de18201d, "pack(int, int)");

        let mut h = 17;
        for l in FLOAT_BITS {
            for r in FLOAT_BITS {
                mix(&mut h, pack_float(f32::from_bits(l), f32::from_bits(r)));
            }
        }
        assert_eq!(h as u64, 0x1f0d8ffc2ab26f0d, "pack(float, float)");

        let mut h = 17;
        for l in FLOAT_BITS {
            for r in FLOAT_BITS {
                let p = pack_floats(f32::from_bits(l), f32::from_bits(r));
                mix(&mut h, p);
                mix(&mut h, (unpack_left_float(p).to_bits() as i32).into());
                mix(&mut h, (unpack_right_float(p).to_bits() as i32).into());
            }
        }
        assert_eq!(h as u64, 0xeb4ae913ad7cfd81, "packf");

        let mut h = 17;
        for i in INTS {
            for f in FLOAT_BITS {
                let f = f32::from_bits(f);
                mix(&mut h, pack_mix_int_float(i, f));
                mix(&mut h, pack_mix_float_int(f, i));
            }
        }
        assert_eq!(h as u64, 0x92f89286cee4b235, "packMix");

        let mut h = 17;
        for p in [
            0_u64,
            u64::MAX,
            1 << 63,
            (1 << 63) - 1,
            0x8000_0000,
            0xffff_ffff,
            0x7fff_ffff_8000_0000,
            0xffff_ffff_0000_0000,
            0x0000_0001_ffff_ffff,
            0x7fc0_0001_7f80_0001,
            0xff81_2345_ffc0_0001,
        ] {
            let p = p as i64;
            mix(&mut h, unpack_left(p).into());
            mix(&mut h, unpack_right(p).into());
            mix(&mut h, (unpack_left_float(p).to_bits() as i32).into());
            mix(&mut h, (unpack_right_float(p).to_bits() as i32).into());
        }
        assert_eq!(h as u64, 0xa60c0309d8230ff1, "unpack");

        // Raw bits: NaN payloads and signs survive (Java's floatToRawIntBits, not floatToIntBits).
        assert_eq!(
            pack_floats(-0.0, f32::from_bits(0x7f80_0001)) as u64,
            0x8000_0000_7f80_0001
        );
        assert_eq!(unpack_right_float(0xff81_2345).to_bits(), 0xff81_2345);
        assert_eq!(
            pack_float(f32::NAN, f32::NEG_INFINITY) as u64,
            0x0000_0000_8000_0000
        );
        assert_eq!(
            pack_float(f32::INFINITY, f32::from_bits(0xcf00_0001)) as u64,
            0x7fff_ffff_8000_0000
        );
        assert_eq!(
            pack_mix_int_float(i32::MIN, f32::from_bits(0xffc0_0001)) as u64,
            0x8000_0000_ffc0_0001
        );
        assert_eq!(
            pack_mix_float_int(f32::from_bits(0xff81_2345), -1) as u64,
            0xff81_2345_ffff_ffff
        );
    }

    #[test]
    fn containment_matches_engine_0_3_0_over_edge_values() {
        let ints = [i32::MIN, -1, 0, 1, 2, i32::MAX];
        let mut h = 17;
        for x in ints {
            for z in ints {
                for x1 in ints {
                    for x2 in ints {
                        mix(&mut h, contains_int(x, z, x1, x1, x2, x2).into());
                        mix(&mut h, contains_int(x, z, x1, z, x2, x2).into());
                    }
                }
            }
        }
        assert_eq!(h as u64, 0x5b0b5e1123378d2e);

        let floats = [
            0.0,
            -0.0,
            1.0,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NAN,
            -1.0,
        ];
        let mut h = 17;
        for x in floats {
            for z in floats {
                for x1 in floats {
                    for x2 in floats {
                        mix(&mut h, contains_float(x, z, x1, x1, x2, x2).into());
                        mix(&mut h, contains_float(x, z, x1, z, x2, x2).into());
                    }
                }
            }
        }
        assert_eq!(h as u64, 0xfce7407daf8d31e4);
    }

    #[test]
    fn rectangular_visitation_matches_engine_0_3_0_over_edge_values() {
        for (start_x, start_z, width, depth, count, expected) in [
            (10, -3, 2, 2, 4, 0x8e9ee03dbf3efe65_u64),
            (0, 0, 0, 5, 0, 0x11),
            (0, 0, 5, 0, 0, 0x11),
            (-5, 7, -2, -3, 6, 0x034316be2c259c8c),
            (3, 4, -1, -1, 1, 0x00000f7624dcaf54),
            (i32::MAX - 1, i32::MIN + 1, 3, 2, 6, 0x4fee14b6cff11a32),
            (1, 1, 1, 7, 7, 0x20d10c359df3dd26),
            (-1, -1, 7, 1, 7, 0x7f462ae27c59212c),
            (0, 0, -4, 2, 0, 0x11),
        ] {
            let mut state = (17_i64, 0);
            iterate(
                start_x,
                start_z,
                width,
                depth,
                &mut state,
                |x, z, (h, n)| {
                    mix(h, x.into());
                    mix(h, z.into());
                    *n += 1;
                },
            );
            assert_eq!(
                (state.0 as u64, state.1),
                (expected, count),
                "{start_x} {start_z} {width} {depth}"
            );
        }
    }

    #[test]
    fn position_iterators_match_engine_0_3_0_over_edge_values() {
        const MAX: i32 = i32::MAX;
        const MIN: i32 = i32::MIN;
        // (iterator, size, steps, final (index, x, y, z), fold of every state), from the Java oracle.
        let cases: [(PosIterator, i32, i32, (i32, i32, i32, i32), u64); 25] = [
            (
                PosIterator::new(1, 2, 3, 2, 2, 2),
                7,
                9,
                (7, 2, 3, 4),
                0x9034430bc835c105,
            ),
            (
                PosIterator::new(0, 0, 0, 3, 0, 3),
                -1,
                9,
                (8, 2, 0, 2),
                0x15e6249581d9843a,
            ),
            (
                PosIterator::new(0, 0, 0, 0, 0, 0),
                -1,
                0,
                (-1, -1, 0, 0),
                0xbbd01592b79cc350,
            ),
            (
                PosIterator::new(-4, -2, 9, -2, 3, 4),
                -25,
                11,
                (8, -4, 0, 12),
                0x512ef4ad8bd804c3,
            ),
            (
                PosIterator::new(5, 1, 5, 3, -2, 2),
                -13,
                6,
                (5, 7, 1, 6),
                0x5ca450c4935ac72a,
            ),
            (
                PosIterator::new(MAX - 1, MAX - 1, MAX - 1, 3, 3, 3),
                26,
                0,
                (-1, MAX - 2, MAX - 1, MAX - 1),
                0x0014db6442e61b89,
            ),
            (
                PosIterator::new(MIN, MIN, MIN, 2, 2, 2),
                7,
                9,
                (7, MIN + 1, MIN + 1, MIN + 1),
                0x805b9d7c09548ab1,
            ),
            (
                PosIterator::new(MAX, 0, 0, 1, 1, 1),
                0,
                0,
                (-1, MAX - 1, 0, 0),
                0x69921efb1a6273c7,
            ),
            (
                PosIterator::new(MIN, 0, 0, 1, 1, 1),
                0,
                1,
                (0, MIN, 0, 0),
                0xcc57afba7d79bed5,
            ),
            (
                PosIterator::new(0, 0, MAX - 1, 2, 1, 5),
                9,
                2,
                (1, 1, 0, MAX - 1),
                0x6d73da0e07bc580c,
            ),
            (
                PosIterator::new(0, MAX - 1, 0, 2, 5, 2),
                19,
                4,
                (3, 1, MAX - 1, 1),
                0x154462e0b6e39c43,
            ),
            (
                PosIterator::new(0, 0, 0, 65536, 65536, 2),
                -1,
                100_000,
                (100_000, 34464, 0, 1),
                0x2ac22f0410045e5f,
            ),
            (
                PosIterator::radius_2d(3, -4, 2),
                -1,
                25,
                (24, 5, 0, -2),
                0xb5a7d8bc9b7e2433,
            ),
            (
                PosIterator::radius_2d(0, 0, 0),
                -1,
                1,
                (0, 0, 0, 0),
                0x428469c9ba9b6d5e,
            ),
            (
                PosIterator::radius_2d(0, 0, -2),
                -1,
                0,
                (-1, 1, 0, 2),
                0x4a8fb88ddc9b89a8,
            ),
            (
                PosIterator::radius_2d(MIN, MAX, 1),
                -1,
                0,
                (-1, MAX - 1, 0, MAX - 1),
                0xc242bd1c0141c58c,
            ),
            (
                PosIterator::radius_3d(1, 2, 3, 1),
                26,
                29,
                (26, 2, 3, 4),
                0xbfe07ed9f5d654f0,
            ),
            (
                PosIterator::radius_3d(0, 0, 0, -1),
                -2,
                0,
                (-1, 0, 1, 1),
                0x45ad6231b95f8af9,
            ),
            (
                PosIterator::area(-1, 5, 3, 2),
                -1,
                6,
                (5, 1, 0, 6),
                0x502dd15abed52ee6,
            ),
            (
                PosIterator::volume_3d(2, -3, 4, 2, 3, 1),
                5,
                8,
                (5, 3, -1, 4),
                0x0ca9a7e024f596f7,
            ),
            (
                PosIterator::range_2d(-2, -1, 1, 2),
                -1,
                9,
                (8, 0, 0, 1),
                0x670dd35d4cbbdc35,
            ),
            (
                PosIterator::range_2d(5, 5, 2, 2),
                -1,
                0,
                (-1, 4, 0, 5),
                0xa4fc00dc88fba78c,
            ),
            (
                PosIterator::range_3d(-1, 0, -1, 1, 1, 0),
                11,
                13,
                (11, 1, 1, 0),
                0x1909ee28e29577ba,
            ),
            (
                PosIterator::range_3d(MIN, 0, 0, MAX, 0, 0),
                -1,
                0,
                (-1, MAX, 0, 0),
                0x12915c37b79cc350,
            ),
            (
                PosIterator::range_3d(3, 3, 3, 2, 2, 2),
                -1,
                0,
                (-1, 2, 3, 3),
                0x0239e99655449fb4,
            ),
        ];
        for (case, (mut iterator, size, steps, end, expected)) in cases.into_iter().enumerate() {
            let mut h = 17;
            let fold = |h: &mut i64, it: &PosIterator| {
                for v in [it.index(), it.x(), it.y(), it.z()] {
                    mix(h, v.into());
                }
            };
            mix(&mut h, iterator.size().into());
            fold(&mut h, &iterator);
            let mut taken = 0;
            // The oracle stops after 100000 steps (the 65536² case runs for ~2^33).
            while iterator.next_position() && taken < 100_000 {
                taken += 1;
                fold(&mut h, &iterator);
            }
            fold(&mut h, &iterator);
            let state = (iterator.index(), iterator.x(), iterator.y(), iterator.z());
            assert_eq!(
                (iterator.size(), taken, state, h as u64),
                (size, steps, end, expected),
                "case {case}"
            );
        }
    }
}
