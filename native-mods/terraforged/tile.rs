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

    pub const fn mask(self, value: i32) -> i32 {
        value & self.mask
    }

    pub const fn index_of(self, x: i32, z: i32) -> i32 {
        z.wrapping_mul(self.total).wrapping_add(x)
    }

    pub const fn chunk_to_block(value: i32) -> i32 {
        value.wrapping_shl(4)
    }

    pub const fn block_to_chunk(value: i32) -> i32 {
        value >> 4
    }

    pub const fn count(min_x: i32, min_z: i32, max_x: i32, max_z: i32) -> i32 {
        max_x
            .wrapping_sub(min_x)
            .wrapping_mul(max_z.wrapping_sub(min_z))
    }

    pub const fn chunks(factor: i32, border_chunks: i32) -> Self {
        Self::new(1_i32.wrapping_shl(factor as u32), border_chunks)
    }

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
}
