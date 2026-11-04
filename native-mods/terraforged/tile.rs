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

/// Engine 0.3.0's tile `Size` (`com.terraforged.engine.tile.Size`): `size` cells per edge plus
/// `border` on each side (`total`), stored in a `total * total` array. `mask` assumes `size` is a
/// power of two, as `chunks`/`blocks` make it. All of it is Java `int` math: sums and products
/// wrap, shifts take their count modulo 32 and `>>` is arithmetic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Size {
    pub size: i32,
    pub total: i32,
    pub border: i32,
    pub array_size: i32,
    pub lower_border: i32,
    pub upper_border: i32,
    mask: i32,
}

impl Size {
    /// Engine's `new Size(size, border)`.
    pub const fn new(size: i32, border: i32) -> Self {
        let total = size.wrapping_add(border.wrapping_mul(2));
        Self {
            size,
            total,
            border,
            array_size: total.wrapping_mul(total),
            lower_border: border,
            upper_border: border.wrapping_add(size),
            mask: size.wrapping_sub(1),
        }
    }

    /// `value & (size - 1)`: the position inside a tile.
    pub const fn mask(self, value: i32) -> i32 {
        value & self.mask
    }

    /// `z * total + x`, the array index of a cell (border included); not bounds-checked.
    pub const fn index_of(self, x: i32, z: i32) -> i32 {
        z.wrapping_mul(self.total).wrapping_add(x)
    }

    /// `value << 4`.
    pub const fn chunk_to_block(value: i32) -> i32 {
        value.wrapping_shl(4)
    }

    /// `value >> 4`, rounding toward negative infinity.
    pub const fn block_to_chunk(value: i32) -> i32 {
        value >> 4
    }

    /// `(max_x - min_x) * (max_z - min_z)`.
    pub const fn count(min_x: i32, min_z: i32, max_x: i32, max_z: i32) -> i32 {
        max_x
            .wrapping_sub(min_x)
            .wrapping_mul(max_z.wrapping_sub(min_z))
    }

    /// A size of `1 << factor` chunks with `border_chunks` chunks of border.
    pub const fn chunks(factor: i32, border_chunks: i32) -> Self {
        Self::new(1_i32.wrapping_shl(factor as u32), border_chunks)
    }

    /// The same tile in blocks: `(1 << factor) << 4` blocks with `border_chunks << 4` blocks of border.
    pub const fn blocks(factor: i32, border_chunks: i32) -> Self {
        let chunks = 1_i32.wrapping_shl(factor as u32);
        Self::new(chunks.wrapping_shl(4), border_chunks.wrapping_shl(4))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_matches_engine_0_3_0() {
        let size = Size::new(64, 16);
        assert_eq!(
            (
                size.size,
                size.total,
                size.border,
                size.array_size,
                size.lower_border,
                size.upper_border,
            ),
            (64, 96, 16, 9_216, 16, 80)
        );
        assert_eq!([size.mask(-1), size.mask(64), size.mask(127)], [63, 0, 63]);
        assert_eq!(size.index_of(5, 7), 677);
        assert_eq!(Size::chunk_to_block(-17), -272);
        assert_eq!(Size::block_to_chunk(-17), -2);
        assert_eq!(Size::count(-3, 5, 7, 13), 80);
        assert_eq!(Size::chunks(3, 2), Size::new(8, 2));
        assert_eq!(Size::blocks(3, 2), Size::new(128, 32));
    }
    const VALUES: [i32; 27] = [
        0,
        1,
        -1,
        2,
        -2,
        15,
        16,
        17,
        -15,
        -16,
        -17,
        31,
        63,
        64,
        127,
        -272,
        1000,
        -1000,
        0x07FF_FFFF,
        0x0800_0000,
        -0x0800_0000,
        -0x0800_0001,
        0x4000_0000,
        i32::MAX,
        i32::MIN,
        i32::MAX - 1,
        i32::MIN + 1,
    ];

    fn fold(fold: i64, value: i32) -> i64 {
        fold.wrapping_mul(31).wrapping_add(i64::from(value))
    }

    fn fold_fields(mut f: i64, size: Size) -> i64 {
        for value in [
            size.size,
            size.total,
            size.border,
            size.array_size,
            size.lower_border,
            size.upper_border,
            size.mask(-1),
        ] {
            f = fold(f, value);
        }
        f
    }

    /// Replays `SizeOracle.java` (`.local/terraforged/oracles/size/`) on Engine 0.3.0's `Size`.
    /// `n` lines: for each size (0, negative, powers of two, `i32::MIN`/`MAX`...), a fold over eight
    /// borders (negative and overflowing ones included) of every field and of `mask` and `indexOf` over
    /// `VALUES` (every pair). `c`: `chunkToBlock`/`blockToChunk` of each value. `k`: a fold of `count`
    /// over every four values. `f` lines: `chunks`/`blocks` for factors that Java's `<<` masks to five
    /// bits (negative, 31, 32, 64, `i32::MIN`/`MAX`), with seven border counts each.
    #[test]
    fn size_matches_engine_oracle() {
        let mut out = String::new();
        let borders = [16, 0, 1, -1, -16, 0x4000_0000, i32::MAX, i32::MIN];
        for size in [64, 1, 0, -1, 16, 512, 0x4000_0000, i32::MAX, i32::MIN, -64] {
            let mut f = 17;
            for border in borders {
                let s = Size::new(size, border);
                f = fold_fields(f, s);
                for a in VALUES {
                    f = fold(f, s.mask(a));
                    for b in VALUES {
                        f = fold(f, s.index_of(a, b));
                    }
                }
            }
            out += &format!("n {size} {f:x}\n");
        }
        out += "c";
        for v in VALUES {
            out += &format!(" {}:{}", Size::chunk_to_block(v), Size::block_to_chunk(v));
        }
        let mut f = 17;
        for a in VALUES {
            for b in VALUES {
                for c in VALUES {
                    for d in VALUES {
                        f = fold(f, Size::count(a, b, c, d));
                    }
                }
            }
        }
        out += &format!("\nk {f:x}\n");
        for factor in [
            -33,
            -32,
            -31,
            -1,
            0,
            1,
            2,
            3,
            4,
            26,
            27,
            28,
            30,
            31,
            32,
            33,
            63,
            64,
            i32::MAX,
            i32::MIN,
        ] {
            let (mut c, mut b) = (17, 17);
            for border in [0, 1, 2, -1, 0x0800_0000, i32::MAX, i32::MIN] {
                c = fold_fields(c, Size::chunks(factor, border));
                b = fold_fields(b, Size::blocks(factor, border));
            }
            let s = Size::blocks(factor, 2);
            out += &format!("f {factor} {} {} {c:x} {b:x}\n", s.size, s.total);
        }
        assert_eq!(
            out,
            "n 64 1f9a174438cfacd8\n\
             n 1 24d3f09de5153adc\n\
             n 0 d60f3a4d8492cbd8\n\
             n -1 553bc285fe93ead4\n\
             n 16 6bd4ecc9ef6c3418\n\
             n 512 f0d931fd1befd3d8\n\
             n 1073741824 9c923efc8492cbd8\n\
             n 2147483647 9a5aed22fe93ead4\n\
             n -2147483648 b50666338492cbd8\n\
             n -64 70352a0964c5ead8\n\
             c 0:0 16:0 -16:-1 32:0 -32:-1 240:0 256:1 272:1 -240:-1 -256:-1 -272:-2 496:1 1008:3 1024:4 2032:7 -4352:-17 16000:62 -16000:-63 2147483632:8388607 -2147483648:8388608 -2147483648:-8388608 2147483632:-8388609 0:67108864 -16:134217727 0:-134217728 -32:134217727 16:-134217728\n\
             k c8c83b7d4b32800f\n\
             f -33 0 64 6d2e3e641d4afca7 d4fbe486a8eaa67e\n\
             f -32 16 80 5402ebd2dfd3889a f468072fbb4bd97e\n\
             f -31 32 96 fbce2a0d72fc4a8b 30b1f1a96de30a7e\n\
             f -1 0 64 6d2e3e641d4afca7 d4fbe486a8eaa67e\n\
             f 0 16 80 5402ebd2dfd3889a f468072fbb4bd97e\n\
             f 1 32 96 fbce2a0d72fc4a8b 30b1f1a96de30a7e\n\
             f 2 64 128 4b295dbb0b2e7067 ffdf1e0eb3b3667e\n\
             f 3 128 192 845deeb203154407 f89ed4a0c1dc067e\n\
             f 4 256 320 779ec3f810ed0ae7 53b3b8e2e84ce67e\n\
             f 26 1073741824 1073741888 c43d6d23ed4afca7 d33d8986a8eaa67e\n\
             f 27 -2147483648 -2147483584 f0264ee7bd4afca7 8b67754aa8eaa67e\n\
             f 28 0 64 bfc7d2905d4afca7 d4fbe486a8eaa67e\n\
             f 30 0 64 969630601d4afca7 d4fbe486a8eaa67e\n\
             f 31 0 64 6d2e3e641d4afca7 d4fbe486a8eaa67e\n\
             f 32 16 80 5402ebd2dfd3889a f468072fbb4bd97e\n\
             f 33 32 96 fbce2a0d72fc4a8b 30b1f1a96de30a7e\n\
             f 63 0 64 6d2e3e641d4afca7 d4fbe486a8eaa67e\n\
             f 64 16 80 5402ebd2dfd3889a f468072fbb4bd97e\n\
             f 2147483647 0 64 6d2e3e641d4afca7 d4fbe486a8eaa67e\n\
             f -2147483648 16 80 5402ebd2dfd3889a f468072fbb4bd97e\n"
        );
    }
}
