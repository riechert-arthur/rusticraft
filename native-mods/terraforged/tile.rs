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
    terraforged_engine::{Cell, TerrainRegistry},
    terraforged_filters::Filterable,
    terraforged_util::{pack, unpack_left, unpack_right},
};

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

/// Engine 0.3.0's `Tile`: the cells of one region of `1 << size` chunks (plus `border_chunks` chunks
/// of border on each side), at chunk `region << size` and block `chunk << 4`. Its block backing is
/// `block_size().array_size` cells, all allocated up front with the world's registry (Engine's
/// pooled arrays fill their cells lazily, `computeCell`, and may be longer than the tile needs when
/// a larger pooled array is reused; neither is observable once a tile is generated). The chunk
/// views, generation, disposal and resource pools are later layers. A negative array size (only
/// from absurd sizes or borders) gives an empty backing where Java throws.
#[derive(Clone)]
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
    /// `new Tile(regionX, regionZ, size, borderChunks, resources, listener)`; `terrains` is the
    /// world's registry, which every cell starts from (`Cell::new`).
    pub fn new(
        region_x: i32,
        region_z: i32,
        size: i32,
        border_chunks: i32,
        terrains: &TerrainRegistry,
    ) -> Self {
        // Java's `<<` takes its count modulo 32, as `wrapping_shl` does.
        let chunk_x = region_x.wrapping_shl(size as u32);
        let chunk_z = region_z.wrapping_shl(size as u32);
        let block_size = Size::blocks(size, border_chunks);
        let chunk_size = Size::chunks(size, border_chunks);
        let cells = usize::try_from(block_size.array_size).unwrap_or(0);
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
            blocks: (0..cells).map(|_| Cell::new(terrains)).collect(),
        }
    }

    /// `getGenerationSize()`: the size factor the tile was built with.
    pub const fn generation_size(&self) -> i32 {
        self.size
    }

    /// `getRegionId()`: `PosUtil.pack(regionX, regionZ)`.
    pub fn region_id(&self) -> i64 {
        Self::region_id_for(self.region_x, self.region_z)
    }

    pub const fn region_x(&self) -> i32 {
        self.region_x
    }

    pub const fn region_z(&self) -> i32 {
        self.region_z
    }

    pub const fn chunk_x(&self) -> i32 {
        self.chunk_x
    }

    pub const fn chunk_z(&self) -> i32 {
        self.chunk_z
    }

    /// `getBlockX()`: the block position of the tile's first non-border cell.
    pub const fn block_x(&self) -> i32 {
        self.block_x
    }

    pub const fn block_z(&self) -> i32 {
        self.block_z
    }

    /// `getOffsetChunks()`: the border, in chunks.
    pub const fn offset_chunks(&self) -> i32 {
        self.border
    }

    /// `getChunkCount()`: the length of Engine's chunk array, `chunk_size().array_size`.
    pub const fn chunk_count(&self) -> i32 {
        self.chunk_size.array_size
    }

    /// `getBlockCount()`: the length of the block backing.
    pub fn block_count(&self) -> i32 {
        self.blocks.len() as i32
    }

    pub const fn chunk_size(&self) -> Size {
        self.chunk_size
    }

    pub const fn block_size(&self) -> Size {
        self.block_size
    }

    /// `filterable()`: the tile's cells as a `Filterable`, for `WorldFilters::apply`.
    pub fn filterable(&mut self) -> FilterRegion<'_> {
        FilterRegion { tile: self }
    }

    /// `border + mask(block)` on both axes: the index `getCell` reads, always inside the backing.
    fn masked_index(&self, block_x: i32, block_z: i32) -> usize {
        let size = self.block_size;
        let x = size.border.wrapping_add(size.mask(block_x));
        let z = size.border.wrapping_add(size.mask(block_z));
        size.index_of(x, z) as usize
    }

    /// `getCell(blockX, blockZ)`: the non-border cell at a world block position, wrapped into the
    /// tile with `mask`. Panics where Engine throws (a negative border or a zero block size can
    /// put the masked index outside the backing).
    pub fn cell(&self, block_x: i32, block_z: i32) -> &Cell {
        &self.blocks[self.masked_index(block_x, block_z)]
    }

    pub fn cell_mut(&mut self, block_x: i32, block_z: i32) -> &mut Cell {
        let index = self.masked_index(block_x, block_z);
        &mut self.blocks[index]
    }

    /// `getRawCell(x, z)`: the cell at `indexOf(x, z)` (border included, Java `int` math). Engine
    /// throws `ArrayIndexOutOfBoundsException` for an index outside the backing; this returns `None`.
    pub fn raw_cell(&self, x: i32, z: i32) -> Option<&Cell> {
        usize::try_from(self.block_size.index_of(x, z))
            .ok()
            .and_then(|index| self.blocks.get(index))
    }

    /// `Tile.getRegionId(regionX, regionZ)`.
    pub fn region_id_for(region_x: i32, region_z: i32) -> i64 {
        pack(region_x, region_z)
    }

    /// `Tile.getRegionX(id)`.
    pub fn region_x_from(id: i64) -> i32 {
        unpack_left(id)
    }

    /// `Tile.getRegionZ(id)`.
    pub fn region_z_from(id: i64) -> i32 {
        unpack_right(id)
    }
}

/// Engine's `Tile.FilterRegion`: the tile's block backing, block position and block size as the
/// filters see them. The filters' `getCellRaw` (`java_cell_raw` in filters.rs) indexes it in Java
/// `int` math and treats an index outside `[0, arraySize)` as `Cell.empty()`, as this class does.
pub struct FilterRegion<'tile> {
    tile: &'tile mut Tile,
}

// `total_size`/`border_size` are Java `int`s stored as `usize`: the filters cast them back to `i32`
// for Java math, so a negative border (or the negative total of an absurd size factor) round-trips.
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

#[cfg(test)]
mod tests {
    use super::super::terraforged_filters::java_cell_raw;
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

    #[test]
    fn tile_filter_region_matches_engine_0_3_0() {
        let terrains = TerrainRegistry::new();
        let mut tile = Tile::new(-2, 3, 2, 1, &terrains);
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
        assert!(
            tile.blocks
                .iter()
                .all(|cell| cell.terrain.is_same(&terrains.get_by_id(0)))
        );

        for (index, cell) in tile.blocks.iter_mut().enumerate() {
            cell.value = index as f32;
        }
        assert_eq!(tile.cell(0, 0).value, 1_552.0);
        assert_eq!(tile.cell(-1, -1).value, 7_663.0);
        tile.cell_mut(64, 64).value = 3.0;
        assert_eq!(tile.blocks[1_552].value, 3.0);
        assert_eq!(tile.raw_cell(5, 7).unwrap().value, 677.0);
        assert!(tile.raw_cell(-1, 0).is_none());

        let mut filter = tile.filterable();
        assert_eq!((filter.block_x(), filter.block_z()), (-128, 192));
        assert_eq!((filter.total_size(), filter.border_size()), (96, 16));
        assert_eq!(java_cell_raw(&filter, 5, 7).unwrap().value, 677.0);
        assert!(java_cell_raw(&filter, -1, 0).is_none());
        filter.backing_mut()[677].value = -1.0;
        assert_eq!(tile.blocks[677].value, -1.0);
    }

    /// Replays `TileOracle.java` (`.local/terraforged/oracles/tile/`) on Engine 0.3.0's `Tile` and
    /// its `FilterRegion`, for tiles with negative, `i32::MIN`/`MAX` and wrapping region positions,
    /// shift factors Java masks to five bits (33, -31, 31: a zero block size), no border and
    /// negative borders. Each `t` line has the tile's getters and its filter region's position and
    /// size; the next line folds, over every pair of `VALUES` (negative, border, edge and
    /// overflowing coordinates), `getCell`, `getRawCell` (-2 where Java throws) and the filter
    /// region's `getCellRaw` (-1 for `Cell.empty()`), with each cell's value set to its index.
    #[test]
    fn tile_matches_engine_oracle() {
        const VALUES: [i32; 33] = [
            0,
            1,
            -1,
            2,
            5,
            7,
            15,
            16,
            17,
            -16,
            -17,
            31,
            32,
            63,
            64,
            79,
            80,
            95,
            96,
            97,
            127,
            191,
            192,
            -96,
            677,
            1023,
            1024,
            9215,
            9216,
            0x4000_0000,
            i32::MAX,
            i32::MIN,
            i32::MIN + 1,
        ];
        let code =
            |cell: Option<&Cell>, missing: i32| cell.map_or(missing, |cell| cell.value as i32);
        let terrains = TerrainRegistry::new();
        let mut out = String::new();
        for (region_x, region_z, size, border) in [
            (-2, 3, 2, 1),
            (0, 0, 3, 2),
            (5, -7, 1, 0),
            (i32::MIN, i32::MAX, 2, 1),
            (1000, -1000, 33, 1),
            (3, -3, -31, 1),
            (1, 1, 0, 0),
            (-1, -1, 4, 1),
            (7, -9, 31, 1),
            (2, 2, 1, -1),
            (-3, 4, 3, -1),
            (0x1234_5678, -0x0765_4321, 2, 1),
        ] {
            let mut tile = Tile::new(region_x, region_z, size, border, &terrains);
            for (index, cell) in tile.blocks.iter_mut().enumerate() {
                cell.value = index as f32;
            }
            let id = tile.region_id();
            out += &format!(
                "t {region_x} {region_z} {size} {border}: {} {} {} {} {} {} {} {} {} {id:x} {} {} {}",
                tile.generation_size(),
                tile.region_x(),
                tile.region_z(),
                tile.chunk_x(),
                tile.chunk_z(),
                tile.block_x(),
                tile.block_z(),
                tile.offset_chunks(),
                tile.chunk_count(),
                Tile::region_x_from(id),
                Tile::region_z_from(id),
                tile.block_count(),
            );
            let (mut cells, mut raws, mut filtered) = (17_i64, 17_i64, 17_i64);
            for a in VALUES {
                for b in VALUES {
                    // `getCell` throws where its masked index leaves the backing (negative borders,
                    // a zero block size); `cell` panics there, so the test reads the index itself.
                    let index = tile.masked_index(a, b);
                    cells = fold(cells, code(tile.blocks.get(index), -2));
                    raws = fold(raws, code(tile.raw_cell(a, b), -2));
                }
            }
            let filter = tile.filterable();
            for a in VALUES {
                for b in VALUES {
                    filtered = fold(filtered, code(java_cell_raw(&filter, a, b), -1));
                }
            }
            out += &format!(
                " {} {} {} {}\nc {cells:x} {raws:x} {filtered:x}\n",
                filter.block_x(),
                filter.block_z(),
                filter.total_size() as i32,
                filter.border_size() as i32,
            );
        }
        assert_eq!(
            out,
            "t -2 3 2 1: 2 -2 3 -8 12 -128 192 1 36 fffffffe00000003 -2 3 9216 -128 192 96 16\n\
             c 4676d075e0c79d72 b8ad21aa9d7c1add 2daac5ee4e89e63a\n\
             t 0 0 3 2: 3 0 0 0 0 0 0 2 144 0 0 0 36864 0 0 192 32\n\
             c 510c8edd09604322 d0aa76a15123f3a0 6301d39e8beafa1d\n\
             t 5 -7 1 0: 1 5 -7 10 -14 160 -224 0 4 5fffffff9 5 -7 1024 160 -224 32 0\n\
             c cbd45f5a68cd9ea2 f1726c9ac36e6c7e 20b1e9c66e7eaf3c\n\
             t -2147483648 2147483647 2 1: 2 -2147483648 2147483647 0 -4 0 -64 1 36 800000007fffffff -2147483648 2147483647 9216 0 -64 96 16\n\
             c 4676d075e0c79d72 b8ad21aa9d7c1add 2daac5ee4e89e63a\n\
             t 1000 -1000 33 1: 33 1000 -1000 2000 -2000 32000 -32000 1 16 3e8fffffc18 1000 -1000 4096 32000 -32000 64 16\n\
             c 3eb1d77f7c592912 75d86ccb1f1fa8ff b6796627a9c02f3d\n\
             t 3 -3 -31 1: -31 3 -3 6 -6 96 -96 1 16 3fffffffd 3 -3 4096 96 -96 64 16\n\
             c 3eb1d77f7c592912 75d86ccb1f1fa8ff b6796627a9c02f3d\n\
             t 1 1 0 0: 0 1 1 1 1 16 16 0 1 100000001 1 1 256 16 16 16 0\n\
             c aa0f449a2540aa2 efe2f61fa6f19be4 1aee21c40bb74705\n\
             t -1 -1 4 1: 4 -1 -1 -16 -16 -256 -256 1 324 ffffffffffffffff -1 -1 82944 -256 -256 288 16\n\
             c 70e0414be31d7032 ef395657a637f88 bccfd1f1bb1a87c8\n\
             t 7 -9 31 1: 31 7 -9 -2147483648 -2147483648 0 0 1 4 7fffffff7 7 -9 1024 0 0 32 16\n\
             c b47b2f48dbcb99c f1726c9ac36e6c7e 20b1e9c66e7eaf3c\n\
             t 2 2 1 -1: 1 2 2 4 4 64 64 -1 0 200000002 2 2 0 64 64 0 -16\n\
             c ee6f3c4f6bd9120d ee6f3c4f6bd9120d 983a098804c0ce0e\n\
             t -3 4 3 -1: 3 -3 4 -24 32 -384 512 -1 36 fffffffd00000004 -3 4 9216 -384 512 96 -16\n\
             c 92264f80fb20476d b8ad21aa9d7c1add 2daac5ee4e89e63a\n\
             t 305419896 -124076833 2 1: 2 305419896 -124076833 1221679584 -496307332 -1927963136 649017280 1 36 12345678f89abcdf 305419896 -124076833 9216 -1927963136 649017280 96 16\n\
             c 4676d075e0c79d72 b8ad21aa9d7c1add 2daac5ee4e89e63a\n"
        );
    }
}
