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
    terraforged_engine::Cell,
    terraforged_filters::Filterable,
    terraforged_util::{pack, unpack_left, unpack_right},
};

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

pub struct Tile {
    region_x: i32,
    region_z: i32,
    chunk_x: i32,
    chunk_z: i32,
    block_x: i32,
    block_z: i32,
    border: i32,
    size: i32,
    block_size: Size,
    chunk_size: Size,
    blocks: Vec<Cell>,
}

impl Tile {
    pub fn new(region_x: i32, region_z: i32, size: i32, border_chunks: i32) -> Self {
        let chunk_x = region_x.wrapping_shl(size as u32);
        let chunk_z = region_z.wrapping_shl(size as u32);
        let block_size = Size::blocks(size, border_chunks);
        let chunk_size = Size::chunks(size, border_chunks);
        Self {
            region_x,
            region_z,
            chunk_x,
            chunk_z,
            block_x: Size::chunk_to_block(chunk_x),
            block_z: Size::chunk_to_block(chunk_z),
            border: border_chunks,
            size,
            block_size,
            chunk_size,
            blocks: (0..block_size.array_size)
                .map(|_| Cell::default())
                .collect(),
        }
    }

    pub const fn generation_size(&self) -> i32 {
        self.size
    }

    pub fn region_id(&self) -> i64 {
        pack(self.region_x, self.region_z)
    }

    pub const fn region_x(&self) -> i32 {
        self.region_x
    }

    pub const fn region_z(&self) -> i32 {
        self.region_z
    }

    pub const fn block_x(&self) -> i32 {
        self.block_x
    }

    pub const fn block_z(&self) -> i32 {
        self.block_z
    }

    pub const fn chunk_x(&self) -> i32 {
        self.chunk_x
    }

    pub const fn chunk_z(&self) -> i32 {
        self.chunk_z
    }

    pub const fn offset_chunks(&self) -> i32 {
        self.border
    }

    pub const fn chunk_count(&self) -> i32 {
        self.chunk_size.array_size
    }

    pub const fn block_count(&self) -> i32 {
        self.block_size.array_size
    }

    pub const fn chunk_size(&self) -> Size {
        self.chunk_size
    }

    pub const fn block_size(&self) -> Size {
        self.block_size
    }

    pub fn filterable(&mut self) -> FilterRegion<'_> {
        FilterRegion { tile: self }
    }

    pub fn chunk(&mut self, chunk_x: i32, chunk_z: i32) -> GenChunk<'_> {
        let relative_x = self
            .chunk_size
            .border
            .wrapping_add(self.chunk_size.mask(chunk_x));
        let relative_z = self
            .chunk_size
            .border
            .wrapping_add(self.chunk_size.mask(chunk_z));
        self.chunk_writer(relative_x, relative_z)
    }

    pub fn chunk_writer(&mut self, region_chunk_x: i32, region_chunk_z: i32) -> GenChunk<'_> {
        GenChunk::new(self, region_chunk_x, region_chunk_z)
    }

    pub fn cell(&self, block_x: i32, block_z: i32) -> &Cell {
        let relative_x = self
            .block_size
            .border
            .wrapping_add(self.block_size.mask(block_x));
        let relative_z = self
            .block_size
            .border
            .wrapping_add(self.block_size.mask(block_z));
        &self.blocks[self.block_size.index_of(relative_x, relative_z) as usize]
    }

    pub fn cell_mut(&mut self, block_x: i32, block_z: i32) -> &mut Cell {
        let relative_x = self
            .block_size
            .border
            .wrapping_add(self.block_size.mask(block_x));
        let relative_z = self
            .block_size
            .border
            .wrapping_add(self.block_size.mask(block_z));
        let index = self.block_size.index_of(relative_x, relative_z) as usize;
        &mut self.blocks[index]
    }

    pub fn raw_cell(&self, block_x: i32, block_z: i32) -> Option<&Cell> {
        usize::try_from(self.block_size.index_of(block_x, block_z))
            .ok()
            .and_then(|index| self.blocks.get(index))
    }

    pub fn region_id_for(region_x: i32, region_z: i32) -> i64 {
        pack(region_x, region_z)
    }

    pub fn region_x_from(id: i64) -> i32 {
        unpack_left(id)
    }

    pub fn region_z_from(id: i64) -> i32 {
        unpack_right(id)
    }
}

pub struct FilterRegion<'tile> {
    tile: &'tile mut Tile,
}

impl Filterable for FilterRegion<'_> {
    fn block_x(&self) -> i32 {
        self.tile.block_x
    }

    fn block_z(&self) -> i32 {
        self.tile.block_z
    }

    fn total_size(&self) -> usize {
        self.tile.block_size.total as usize
    }

    fn border_size(&self) -> usize {
        self.tile.block_size.border as usize
    }

    fn backing(&self) -> &[Cell] {
        &self.tile.blocks
    }

    fn backing_mut(&mut self) -> &mut [Cell] {
        &mut self.tile.blocks
    }
}

pub struct GenChunk<'tile> {
    tile: &'tile mut Tile,
    chunk_x: i32,
    chunk_z: i32,
    block_x: i32,
    block_z: i32,
    region_block_x: i32,
    region_block_z: i32,
}

impl<'tile> GenChunk<'tile> {
    fn new(tile: &'tile mut Tile, region_chunk_x: i32, region_chunk_z: i32) -> Self {
        let region_block_x = region_chunk_x.wrapping_shl(4);
        let region_block_z = region_chunk_z.wrapping_shl(4);
        let chunk_x = tile
            .chunk_x
            .wrapping_add(region_chunk_x)
            .wrapping_sub(tile.offset_chunks());
        let chunk_z = tile
            .chunk_z
            .wrapping_add(region_chunk_z)
            .wrapping_sub(tile.offset_chunks());
        Self {
            tile,
            chunk_x,
            chunk_z,
            block_x: chunk_x.wrapping_shl(4),
            block_z: chunk_z.wrapping_shl(4),
            region_block_x,
            region_block_z,
        }
    }

    pub const fn chunk_x(&self) -> i32 {
        self.chunk_x
    }

    pub const fn chunk_z(&self) -> i32 {
        self.chunk_z
    }

    pub const fn block_x(&self) -> i32 {
        self.block_x
    }

    pub const fn block_z(&self) -> i32 {
        self.block_z
    }

    pub fn cell(&self, block_x: i32, block_z: i32) -> &Cell {
        let relative_x = self.region_block_x.wrapping_add(block_x & 15);
        let relative_z = self.region_block_z.wrapping_add(block_z & 15);
        &self.tile.blocks[self.tile.block_size.index_of(relative_x, relative_z) as usize]
    }

    pub fn cell_mut(&mut self, block_x: i32, block_z: i32) -> &mut Cell {
        let relative_x = self.region_block_x.wrapping_add(block_x & 15);
        let relative_z = self.region_block_z.wrapping_add(block_z & 15);
        let index = self.tile.block_size.index_of(relative_x, relative_z) as usize;
        &mut self.tile.blocks[index]
    }

    pub fn visit(
        &self,
        mut min_x: i32,
        mut min_z: i32,
        mut max_x: i32,
        mut max_z: i32,
        mut visitor: impl FnMut(&Cell, i32, i32),
    ) {
        let region_min_x = self.block_x;
        let region_min_z = self.block_z;
        if max_x < region_min_x || max_z < region_min_z {
            return;
        }
        let region_max_x = self.block_x.wrapping_add(15);
        let region_max_z = self.block_z.wrapping_add(15);
        if min_x > region_max_x || max_z > region_max_z {
            return;
        }
        min_x = min_x.max(region_min_x);
        min_z = min_z.max(region_min_z);
        max_x = max_x.min(region_max_x);
        max_z = max_z.min(region_max_z);
        for z in min_z..=max_x {
            for x in min_x..=max_z {
                visitor(self.cell(x, z), x, z);
            }
        }
    }

    pub fn iterate(&self, mut visitor: impl FnMut(&Cell, i32, i32)) {
        for delta_z in 0..16 {
            for delta_x in 0..16 {
                visitor(self.cell(delta_x, delta_z), delta_x, delta_z);
            }
        }
    }

    pub fn generate(&mut self, mut visitor: impl FnMut(&mut Cell, i32, i32, i32, i32)) {
        let block_x = self.block_x;
        let block_z = self.block_z;
        for delta_z in 0..16 {
            for delta_x in 0..16 {
                visitor(
                    self.cell_mut(delta_x, delta_z),
                    delta_x,
                    delta_z,
                    block_x.wrapping_add(delta_x),
                    block_z.wrapping_add(delta_z),
                );
            }
        }
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

    #[test]
    fn tile_filter_region_matches_engine_0_3_0() {
        let mut tile = Tile::new(-2, 3, 2, 1);
        assert_eq!(
            (
                tile.generation_size(),
                tile.region_x(),
                tile.region_z(),
                tile.chunk_x(),
                tile.chunk_z(),
                tile.block_x(),
                tile.block_z(),
                tile.offset_chunks(),
                tile.chunk_count(),
                tile.block_count(),
            ),
            (2, -2, 3, -8, 12, -128, 192, 1, 36, 9_216)
        );
        assert_eq!(tile.chunk_size(), Size::new(4, 1));
        assert_eq!(tile.block_size(), Size::new(64, 16));
        assert_eq!(tile.region_id() as u64, 0xffff_fffe_0000_0003);
        assert_eq!(Tile::region_x_from(tile.region_id()), -2);
        assert_eq!(Tile::region_z_from(tile.region_id()), 3);

        for (index, cell) in tile.blocks.iter_mut().enumerate() {
            cell.value = index as f32;
        }
        assert_eq!(tile.cell(0, 0).value, 1_552.0);
        assert_eq!(tile.cell(-1, -1).value, 7_663.0);
        assert_eq!(tile.raw_cell(5, 7).unwrap().value, 677.0);
        assert!(tile.raw_cell(-1, 0).is_none());

        let mut filter = tile.filterable();
        assert_eq!((filter.block_x(), filter.block_z()), (-128, 192));
        assert_eq!((filter.total_size(), filter.border_size()), (96, 16));
        assert_eq!(filter.cell_raw(5, 7).unwrap().value, 677.0);
        assert_eq!(filter.cell_raw(usize::MAX, 1).unwrap().value, 95.0);
        filter.cell_raw_mut(5, 7).unwrap().value = -1.0;
        assert_eq!(filter.backing()[677].value, -1.0);
    }

    #[test]
    fn tile_chunks_match_engine_0_3_0() {
        let mut tile = Tile::new(-2, 3, 2, 1);
        for (index, cell) in tile.blocks.iter_mut().enumerate() {
            cell.value = index as f32;
        }
        let mut chunk = tile.chunk_writer(2, 3);
        assert_eq!(
            (
                chunk.chunk_x(),
                chunk.chunk_z(),
                chunk.block_x(),
                chunk.block_z(),
            ),
            (-7, 14, -112, 224)
        );
        assert_eq!(chunk.cell(0, 0).value, 4_640.0);
        assert_eq!(chunk.cell(17, -1).value, 6_081.0);
        chunk.cell_mut(17, -1).value = -1.0;
        let mut generated = Vec::new();
        chunk.generate(|cell, delta_x, delta_z, block_x, block_z| {
            if delta_x == 0 && delta_z == 0 {
                generated.push((cell.value, block_x, block_z));
            }
        });
        assert_eq!(generated, [(4_640.0, -112, 224)]);
        drop(chunk);

        let chunk = tile.chunk(-7, 14);
        assert_eq!((chunk.chunk_x(), chunk.chunk_z()), (-7, 14));
        assert_eq!(chunk.cell(17, -1).value, -1.0);
        let mut count = 0;
        chunk.iterate(|_, _, _| count += 1);
        assert_eq!(count, 256);
    }
}
