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
    terraforged_heightmap::HeightmapCache,
    terraforged_river::Rivermap,
    terraforged_util::{pack, unpack_left, unpack_right},
};
use std::sync::Arc;

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
/// a larger pooled array is reused; neither is observable once a tile is generated). Its chunk
/// array holds the `GenChunk`s `chunk_writer` and the heightmap fills create, as Engine's does.
/// Disposal and resource pools are not ported. A negative array size (only from absurd sizes or
/// borders) gives an empty backing where Java throws.
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
    chunks: Vec<Option<GenChunk>>,
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
        let chunks = usize::try_from(chunk_size.array_size).unwrap_or(0);
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
            blocks: vec![Cell::new(terrains); cells],
            chunks: vec![None; chunks],
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

    /// `generate(Heightmap)`, the fill `TileGenerator.generateRegion` runs before the filters: for
    /// every tile-local chunk, z rows then x (border included), the chunk at that slot
    /// (`computeChunk`, as [`Self::chunk_writer`]: a slot already holding a chunk keeps it, so its
    /// cells are the ones filled), then for each of its cells, z rows then x, `apply_base`
    /// (`heightmap.applyBase`), the river map of the cell's continent cell, reusing the previous
    /// cell's map when it matches (`Rivermap.get(cell, rivermap, heightmap)`, with `lookup` as
    /// `heightmap.getContinent().getRivermap`), `apply_rivers` (`heightmap.applyRivers(cell, x, z,
    /// rivermap)`, i.e. [`HeightmapRivers::apply`]) and `apply_climate` (`heightmap.applyClimate`).
    /// The stages get the cell's world block position (`chunk block + d`, Java `int` math) as
    /// floats. The map is carried across chunks, from the tile's first cell to its last.
    ///
    /// `Heightmap` itself is not ported yet, so its stages are closures. Panics where Engine
    /// throws: a slot holding a chunk past the tile's edge whose cells fall outside the backing, or
    /// a size factor so large that the chunk array's Java `int` length wrapped (Engine already
    /// throws building such a tile). A stage that panics leaves the tile partly filled.
    ///
    /// [`HeightmapRivers::apply`]: super::terraforged_heightmap::HeightmapRivers::apply
    pub fn generate_heightmap(
        &mut self,
        mut apply_base: impl FnMut(&mut Cell, f32, f32),
        mut lookup: impl FnMut(i32, i32) -> Arc<Rivermap>,
        mut apply_rivers: impl FnMut(&mut Cell, f32, f32, &Rivermap),
        mut apply_climate: impl FnMut(&mut Cell, f32, f32),
    ) {
        let mut current = None;
        self.fill_cells(|cell, x, z| {
            let (x, z) = (x as f32, z as f32);
            apply_base(cell, x, z);
            let rivermap = Rivermap::get_for_cell(cell, current.take(), &mut lookup);
            apply_rivers(cell, x, z, &rivermap);
            apply_climate(cell, x, z);
            current = Some(rivermap);
        });
    }

    /// `generate(HeightmapCache)`: the same chunk and cell order as [`Self::generate_heightmap`],
    /// but each cell goes through [`HeightmapCache::generate`] at its world block position
    /// (`int`, the cache key `PosUtil.pack(x, z)`): a cached block is copied into the cell and
    /// keeps the river map carried so far; a miss runs the stages on the cell (the river map
    /// reused or looked up as in `generate_heightmap`) and caches it. The river map is carried
    /// from the tile's first cell to its last, across hits. Engine caches the tile's own cell
    /// objects, so a cell that a later chunk refills (a slot holding an aliasing chunk) changes the
    /// block cached first; the port caches snapshots (see [`HeightmapCache::generate`]).
    pub fn generate_cached_heightmap(
        &mut self,
        cache: &HeightmapCache,
        mut apply_base: impl FnMut(&mut Cell, f32, f32),
        mut lookup: impl FnMut(i32, i32) -> Arc<Rivermap>,
        mut apply_rivers: impl FnMut(&mut Cell, f32, f32, &Rivermap),
        mut apply_climate: impl FnMut(&mut Cell, f32, f32),
    ) {
        let mut current = None;
        self.fill_cells(|cell, x, z| {
            current = cache.generate(
                cell,
                x,
                z,
                current.take(),
                &mut apply_base,
                &mut lookup,
                &mut apply_rivers,
                &mut apply_climate,
            );
        });
    }

    /// `generate(Heightmap, offsetX, offsetZ, zoom)`: [`Self::generate_heightmap`] with each cell's
    /// world block position scaled by `zoom` and shifted by the offset less half the zoomed tile
    /// width before the stages see it. As in Engine, in `float` math, left to right:
    /// `translate = offset - (float) blockSize.size * zoom / 2`, then `(float) block * zoom +
    /// translate` for x and z (any zoom, NaN and infinities included). The same chunk and cell
    /// order, river-map reuse (the lookup gets the cell's continent cell) and panics.
    #[allow(clippy::too_many_arguments)]
    pub fn generate_zoomed_heightmap(
        &mut self,
        offset_x: f32,
        offset_z: f32,
        zoom: f32,
        mut apply_base: impl FnMut(&mut Cell, f32, f32),
        mut lookup: impl FnMut(i32, i32) -> Arc<Rivermap>,
        mut apply_rivers: impl FnMut(&mut Cell, f32, f32, &Rivermap),
        mut apply_climate: impl FnMut(&mut Cell, f32, f32),
    ) {
        let translate_x = offset_x - self.block_size.size as f32 * zoom / 2.0;
        let translate_z = offset_z - self.block_size.size as f32 * zoom / 2.0;
        let mut current = None;
        self.fill_cells(|cell, x, z| {
            let x = x as f32 * zoom + translate_x;
            let z = z as f32 * zoom + translate_z;
            apply_base(cell, x, z);
            let rivermap = Rivermap::get_for_cell(cell, current.take(), &mut lookup);
            apply_rivers(cell, x, z, &rivermap);
            apply_climate(cell, x, z);
            current = Some(rivermap);
        });
    }

    /// The loop the heightmap fills share: every tile-local chunk, z rows then x (border
    /// included), taken as [`Self::chunk_writer`] does (`computeChunk`), then its cells in
    /// [`ChunkWriter::generate`]'s order with their world block position.
    fn fill_cells(&mut self, mut fill: impl FnMut(&mut Cell, i32, i32)) {
        let total = self.chunk_size.total;
        for chunk_z in 0..total {
            for chunk_x in 0..total {
                self.chunk_writer(chunk_x, chunk_z)
                    .expect("tile-local chunk inside the chunk array")
                    .generate(|cell, _, _, x, z| fill(cell, x, z));
            }
        }
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

    /// `getChunkWriter(chunkX, chunkZ)`: the chunk at `indexOf(chunkX, chunkZ)` of the chunk array
    /// (tile-local chunk coordinates, border included), created there with these coordinates if
    /// the slot is empty (`computeChunk`). As in Engine, a slot keeps the chunk first created in it,
    /// so coordinates past the tile's edge that alias a filled slot return that slot's chunk.
    /// Engine throws `ArrayIndexOutOfBoundsException` for an index outside the array; this returns
    /// `None`.
    pub fn chunk_writer(&mut self, chunk_x: i32, chunk_z: i32) -> Option<ChunkWriter<'_>> {
        let index = usize::try_from(self.chunk_size.index_of(chunk_x, chunk_z)).ok()?;
        let chunk = match *self.chunks.get(index)? {
            Some(chunk) => chunk,
            None => {
                let chunk = GenChunk::new(self, chunk_x, chunk_z);
                self.chunks[index] = Some(chunk);
                chunk
            }
        };
        Some(ChunkWriter { tile: self, chunk })
    }

    /// `getChunk(chunkX, chunkZ)`: the chunk at a world chunk position, wrapped into the tile with
    /// `border + mask(chunk)` like `cell`. `None` where Engine throws a `NullPointerException`: no
    /// `chunk_writer` call has created that chunk yet. Panics where Engine throws
    /// `ArrayIndexOutOfBoundsException` (a negative border can put the index outside the array).
    pub fn chunk(&self, chunk_x: i32, chunk_z: i32) -> Option<ChunkReader<'_>> {
        let chunk = self.chunks[self.masked_chunk_index(chunk_x, chunk_z) as usize]?;
        Some(ChunkReader { tile: self, chunk })
    }

    /// `border + mask(chunk)` on both axes: the chunk-array index `getChunk` reads.
    fn masked_chunk_index(&self, chunk_x: i32, chunk_z: i32) -> i32 {
        let size = self.chunk_size;
        let x = size.border.wrapping_add(size.mask(chunk_x));
        let z = size.border.wrapping_add(size.mask(chunk_z));
        size.index_of(x, z)
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

/// Engine's `Tile.GenChunk` without its tile: a chunk's world chunk and block position and the
/// block offset of its 16x16 cells in the tile backing, from its tile-local chunk coordinates
/// (`regionChunkX`, border included). `ChunkReader`/`ChunkWriter` pair it with the tile.
#[derive(Clone, Copy)]
pub struct GenChunk {
    chunk_x: i32,
    chunk_z: i32,
    block_x: i32,
    block_z: i32,
    region_block_x: i32,
    region_block_z: i32,
}

impl GenChunk {
    /// `new GenChunk(regionChunkX, regionChunkZ)`: chunk `tile.chunk + regionChunk - border`.
    fn new(tile: &Tile, region_chunk_x: i32, region_chunk_z: i32) -> Self {
        let chunk_x = tile
            .chunk_x
            .wrapping_add(region_chunk_x)
            .wrapping_sub(tile.border);
        let chunk_z = tile
            .chunk_z
            .wrapping_add(region_chunk_z)
            .wrapping_sub(tile.border);
        Self {
            chunk_x,
            chunk_z,
            block_x: chunk_x.wrapping_shl(4),
            block_z: chunk_z.wrapping_shl(4),
            region_block_x: region_chunk_x.wrapping_shl(4),
            region_block_z: region_chunk_z.wrapping_shl(4),
        }
    }

    pub const fn chunk_x(&self) -> i32 {
        self.chunk_x
    }

    pub const fn chunk_z(&self) -> i32 {
        self.chunk_z
    }

    /// `getBlockX()`: the world block position of the chunk's first cell.
    pub const fn block_x(&self) -> i32 {
        self.block_x
    }

    pub const fn block_z(&self) -> i32 {
        self.block_z
    }

    /// The backing index `getCell`/`genCell` read, `indexOf(regionBlock + (block & 15))`; `None`
    /// outside the backing, where Engine throws (only for a chunk past the tile's edge or with a
    /// negative border), and `cell`/`gen_cell` panic.
    fn index(&self, tile: &Tile, block_x: i32, block_z: i32) -> Option<usize> {
        let x = self.region_block_x.wrapping_add(block_x & 15);
        let z = self.region_block_z.wrapping_add(block_z & 15);
        usize::try_from(tile.block_size.index_of(x, z))
            .ok()
            .filter(|&index| index < tile.blocks.len())
    }
}

/// Engine's `ChunkReader` (a `GenChunk` from `Tile::chunk`): reads a chunk's cells. Engine's
/// `open`/`close` reference count belongs to its disposal, which is not ported.
pub struct ChunkReader<'tile> {
    tile: &'tile Tile,
    chunk: GenChunk,
}

impl ChunkReader<'_> {
    /// The chunk's position.
    pub const fn chunk(&self) -> GenChunk {
        self.chunk
    }

    /// `getCell(blockX, blockZ)`: the cell of a block, by its position inside the chunk (`& 15`).
    pub fn cell(&self, block_x: i32, block_z: i32) -> &Cell {
        let index = self.chunk.index(self.tile, block_x, block_z);
        &self.tile.blocks[index.expect("chunk cell outside the tile backing")]
    }

    /// `visit(minX, minZ, maxX, maxZ, visitor)`, Engine's bugs included: it returns early when
    /// `maxZ` is past the chunk's last block (rather than `minZ`), and its loops run `z` from
    /// `minZ` to the clamped `maxX` and `x` from `minX` to the clamped `maxZ`. Where Engine's loop
    /// never ends (a clamped bound of `i32::MAX`: a chunk at block x or z 0x7FFF_FFF0), this one
    /// stops.
    pub fn visit(
        &self,
        min_x: i32,
        min_z: i32,
        max_x: i32,
        max_z: i32,
        mut visitor: impl FnMut(&Cell, i32, i32),
    ) {
        let (region_min_x, region_min_z) = (self.chunk.block_x, self.chunk.block_z);
        if max_x < region_min_x || max_z < region_min_z {
            return;
        }
        let region_max_x = region_min_x.wrapping_add(15);
        let region_max_z = region_min_z.wrapping_add(15);
        if min_x > region_max_x || max_z > region_max_z {
            return;
        }
        let min_x = min_x.max(region_min_x);
        let min_z = min_z.max(region_min_z);
        let max_x = max_x.min(region_max_x);
        let max_z = max_z.min(region_max_z);
        // With swapped bounds the outer loop can span ~2^32 rows of an empty inner loop (a chunk far
        // off the x = z diagonal); skipping it visits the same cells.
        if min_x > max_z {
            return;
        }
        for z in min_z..=max_x {
            for x in min_x..=max_z {
                visitor(self.cell(x, z), x, z);
            }
        }
    }

    /// `iterate(visitor)`: every cell with its position in the chunk, row by row.
    pub fn iterate(&self, mut visitor: impl FnMut(&Cell, i32, i32)) {
        for dz in 0..16 {
            for dx in 0..16 {
                visitor(self.cell(dx, dz), dx, dz);
            }
        }
    }
}

/// Engine's `ChunkWriter` (a `GenChunk` from `Tile::chunk_writer`): fills a chunk's cells.
pub struct ChunkWriter<'tile> {
    tile: &'tile mut Tile,
    chunk: GenChunk,
}

impl ChunkWriter<'_> {
    /// The chunk's position.
    pub const fn chunk(&self) -> GenChunk {
        self.chunk
    }

    /// `genCell(blockX, blockZ)`: the cell of a block, by its position inside the chunk (`& 15`).
    pub fn gen_cell(&mut self, block_x: i32, block_z: i32) -> &mut Cell {
        let index = self.chunk.index(self.tile, block_x, block_z);
        &mut self.tile.blocks[index.expect("chunk cell outside the tile backing")]
    }

    /// `generate(ctx, visitor)`: every cell, row by row, with its position in the chunk and in the
    /// world (`blockX + dx`, wrapping like Java's `int`).
    pub fn generate(&mut self, mut visitor: impl FnMut(&mut Cell, i32, i32, i32, i32)) {
        let (block_x, block_z) = (self.chunk.block_x, self.chunk.block_z);
        for dz in 0..16 {
            for dx in 0..16 {
                let (x, z) = (block_x.wrapping_add(dx), block_z.wrapping_add(dz));
                visitor(self.gen_cell(dx, dz), dx, dz, x, z);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_engine::Levels;
    use super::super::terraforged_filters::java_cell_raw;
    use super::super::terraforged_river::GenWarp;
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

    /// Coordinates `TileOracle.java` reads cells and chunks at.
    const TILE_VALUES: [i32; 33] = [
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

    /// The tiles `TileOracle.java` checks: region x and z, size factor, border chunks.
    const TILE_CONFIGS: [(i32, i32, i32, i32); 12] = [
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
    ];

    /// Replays `TileOracle.java` (`.local/terraforged/oracles/tile/`) on Engine 0.3.0's `Tile` and
    /// its `FilterRegion`, for tiles with negative, `i32::MIN`/`MAX` and wrapping region positions,
    /// shift factors Java masks to five bits (33, -31, 31: a zero block size), no border and
    /// negative borders. Each `t` line has the tile's getters and its filter region's position and
    /// size; the next line folds, over every pair of `VALUES` (negative, border, edge and
    /// overflowing coordinates), `getCell`, `getRawCell` (-2 where Java throws) and the filter
    /// region's `getCellRaw` (-1 for `Cell.empty()`), with each cell's value set to its index.
    #[test]
    fn tile_matches_engine_oracle() {
        let code =
            |cell: Option<&Cell>, missing: i32| cell.map_or(missing, |cell| cell.value as i32);
        let terrains = TerrainRegistry::new();
        let mut out = String::new();
        for (region_x, region_z, size, border) in TILE_CONFIGS {
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
            for a in TILE_VALUES {
                for b in TILE_VALUES {
                    // `getCell` throws where its masked index leaves the backing (negative borders,
                    // a zero block size); `cell` panics there, so the test reads the index itself.
                    let index = tile.masked_index(a, b);
                    cells = fold(cells, code(tile.blocks.get(index), -2));
                    raws = fold(raws, code(tile.raw_cell(a, b), -2));
                }
            }
            let filter = tile.filterable();
            for a in TILE_VALUES {
                for b in TILE_VALUES {
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

    #[test]
    fn tile_chunks_match_engine_0_3_0() {
        let terrains = TerrainRegistry::new();
        let mut tile = Tile::new(-2, 3, 2, 1, &terrains);
        for (index, cell) in tile.blocks.iter_mut().enumerate() {
            cell.value = index as f32;
        }
        assert!(tile.chunk(-7, 14).is_none());
        let mut writer = tile.chunk_writer(2, 3).unwrap();
        let chunk = writer.chunk();
        assert_eq!(
            (
                chunk.chunk_x(),
                chunk.chunk_z(),
                chunk.block_x(),
                chunk.block_z()
            ),
            (-7, 14, -112, 224)
        );
        assert_eq!(writer.gen_cell(0, 0).value, 4_640.0);
        writer.gen_cell(17, -1).value = -1.0;
        let mut generated = Vec::new();
        writer.generate(|cell, dx, dz, x, z| {
            if (dx, dz) == (1, 15) {
                generated.push((cell.value, x, z));
            }
        });
        assert_eq!(generated, [(-1.0, -111, 239)]);
        assert!(tile.chunk_writer(-1, 0).is_none());
        assert!(tile.chunk_writer(6, 6).is_none());

        let reader = tile.chunk(-7, 14).unwrap();
        assert_eq!(reader.chunk().chunk_x(), -7);
        assert_eq!(reader.cell(-111, 239).value, -1.0);
        let mut count = 0;
        reader.iterate(|_, _, _| count += 1);
        assert_eq!(count, 256);
    }

    const OFFSETS: [i32; 9] = [0, 1, 15, 16, -1, 17, -16, i32::MIN, i32::MAX];

    /// `TileOracle.writer`: a writer's position, `genCell` at `OFFSETS` pairs (-2 outside the
    /// backing) and, when every cell is inside, a `generate` pass adding 1 to each cell.
    fn fold_writer(tile: &mut Tile, f: &mut i64, chunk_x: i32, chunk_z: i32) {
        let Some(mut writer) = tile.chunk_writer(chunk_x, chunk_z) else {
            *f = fold(*f, -2);
            return;
        };
        let chunk = writer.chunk();
        for value in [
            chunk.chunk_x(),
            chunk.chunk_z(),
            chunk.block_x(),
            chunk.block_z(),
        ] {
            *f = fold(*f, value);
        }
        let mut inside = true;
        for a in OFFSETS {
            for b in OFFSETS {
                // `gen_cell` panics where Engine throws: test the index first.
                if chunk.index(writer.tile, a, b).is_some() {
                    *f = fold(*f, writer.gen_cell(a, b).value as i32);
                } else {
                    *f = fold(*f, -2);
                    inside = false;
                }
            }
        }
        if inside {
            writer.generate(|cell, dx, dz, x, z| {
                for value in [cell.value as i32, dx, dz, x, z] {
                    *f = fold(*f, value);
                }
                cell.value += 1.0;
            });
        }
    }

    /// `TileOracle.reader`: a reader's position, `getCell` at `OFFSETS` pairs, `iterate` and, with
    /// `visits`, `visit` over rectangles around the chunk; -3 for a missing chunk, -2 outside.
    fn fold_reader(tile: &Tile, f: &mut i64, chunk_x: i32, chunk_z: i32, visits: bool) {
        let index = tile.masked_chunk_index(chunk_x, chunk_z);
        let Some(slot) = usize::try_from(index).ok().and_then(|i| tile.chunks.get(i)) else {
            *f = fold(*f, -2);
            return;
        };
        assert_eq!(slot.is_some(), tile.chunk(chunk_x, chunk_z).is_some());
        let Some(reader) = tile.chunk(chunk_x, chunk_z) else {
            *f = fold(*f, -3);
            return;
        };
        let chunk = reader.chunk();
        for value in [
            chunk.chunk_x(),
            chunk.chunk_z(),
            chunk.block_x(),
            chunk.block_z(),
        ] {
            *f = fold(*f, value);
        }
        let mut inside = true;
        for a in OFFSETS {
            for b in OFFSETS {
                if chunk.index(tile, a, b).is_some() {
                    *f = fold(*f, reader.cell(a, b).value as i32);
                } else {
                    *f = fold(*f, -2);
                    inside = false;
                }
            }
        }
        if !inside {
            return;
        }
        reader.iterate(|cell, dx, dz| {
            for value in [cell.value as i32, dx, dz] {
                *f = fold(*f, value);
            }
        });
        if !visits {
            return;
        }
        let bounds = |block: i32| {
            [-1, 0, 7, 16]
                .map(|offset| block.wrapping_add(offset))
                .into_iter()
                .chain([i32::MIN, i32::MAX])
        };
        for x0 in bounds(chunk.block_x()) {
            for z0 in bounds(chunk.block_z()) {
                for x1 in bounds(chunk.block_x()) {
                    for z1 in bounds(chunk.block_z()) {
                        reader.visit(x0, z0, x1, z1, |cell, x, z| {
                            for value in [cell.value as i32, x, z] {
                                *f = fold(*f, value);
                            }
                        });
                        *f = fold(*f, -7);
                    }
                }
            }
        }
    }

    /// Replays the `k` lines of `TileOracle.java` on Engine 0.3.0's `getChunk`/`getChunkWriter`
    /// and its `ChunkReader`/`ChunkWriter` defaults, for every `TILE_CONFIGS` tile: `getChunk`
    /// over `TILE_VALUES` pairs before any chunk exists; `getChunkWriter` over tile-local chunks
    /// (negative, last, one and two past the edge, `i32::MIN`/`MAX`: aliasing and out-of-range
    /// slots) followed by Engine's `generate(Consumer)` loop over every chunk; `getChunk` over
    /// `TILE_VALUES` pairs again; and `visit` (Engine's swapped bounds included) over rectangles
    /// around a few chunks.
    #[test]
    fn tile_chunks_match_engine_oracle() {
        let terrains = TerrainRegistry::new();
        let mut out = String::new();
        for (region_x, region_z, size, border) in TILE_CONFIGS {
            let mut tile = Tile::new(region_x, region_z, size, border, &terrains);
            for (index, cell) in tile.blocks.iter_mut().enumerate() {
                cell.value = index as f32;
            }
            let total = tile.chunk_size().total;
            let mut folds = [17_i64; 4];
            for a in TILE_VALUES {
                for b in TILE_VALUES {
                    fold_reader(&tile, &mut folds[0], a, b, false);
                }
            }
            let chunks = [
                0,
                1,
                -1,
                total.wrapping_sub(1),
                total,
                total.wrapping_add(1),
                total.wrapping_mul(2).wrapping_add(1),
                i32::MIN,
                i32::MAX,
            ];
            for a in chunks {
                for b in chunks {
                    fold_writer(&mut tile, &mut folds[1], a, b);
                }
            }
            for z in 0..total {
                for x in 0..total {
                    let chunk = tile.chunk_writer(x, z).unwrap().chunk();
                    folds[1] = fold(fold(folds[1], chunk.chunk_x()), chunk.chunk_z());
                }
            }
            for a in TILE_VALUES {
                for b in TILE_VALUES {
                    fold_reader(&tile, &mut folds[2], a, b, false);
                }
            }
            let visited = [0, -1, 3, 0x4000_0000];
            for a in visited {
                for b in visited {
                    fold_reader(&tile, &mut folds[3], a, b, true);
                }
            }
            let [p, w, r, v] = folds;
            out += &format!("k {region_x} {region_z} {size} {border}: {p:x} {w:x} {r:x} {v:x}\n");
        }
        assert_eq!(
            out,
            "k -2 3 2 1: 44a46f16d2f1560c f354ca027ee4b45b a3b5537885ba0e92 ff7c713e91248951\n\
             k 0 0 3 2: 44a46f16d2f1560c 9f40df21cb6669c1 99aa32e702134cea c3cabf4b7ad872c1\n\
             k 5 -7 1 0: 44a46f16d2f1560c 4132099aa7b90c96 87a518fda6f90f03 f29cd237f42d3ed1\n\
             k -2147483648 2147483647 2 1: 44a46f16d2f1560c f91411f858cda42b 4993606e7bc5762a 4686187ea8988151\n\
             k 1000 -1000 33 1: 44a46f16d2f1560c 2b116ee6f7155cfd e43456961d996dfe a1d1d8c1027f3e91\n\
             k 3 -3 -31 1: 44a46f16d2f1560c 4cf41f85a782d815 6ea3cd20ef9861ea af98c9a1a9e1da91\n\
             k 1 1 0 0: 44a46f16d2f1560c 8f6aaba4859b7618 102d785cdea4f553 4846a14635fc811\n\
             k -1 -1 4 1: 44a46f16d2f1560c f6298a7ef48fb3cf d08b7b4214c802b8 db6c0fb974ead861\n\
             k 7 -9 31 1: 3873a8cb5279868f 6f52d94832ae78a6 158272460fad3556 6d49e0a4b24306d4\n\
             k 2 2 1 -1: ee6f3c4f6bd9120d dfdc731c059a160d ee6f3c4f6bd9120d c1d46ccbbc4d7c11\n\
             k -3 4 3 -1: 9f22577826ba2b3e c5d08c6ccb5d1c23 36aea14c8a36bc41 7e15d1295b601ab4\n\
             k 305419896 -124076833 2 1: 44a46f16d2f1560c 54a98b432968876b 94f7691aa357e28a 3d600e7d4bee6151\n"
        );
    }

    /// Replays the `w` lines of `TileOracle.java`: Engine's `WorldFilters.apply(tile, optional)` on
    /// tiles with hashed cells (heights, river and erosion masks, continent edges, terrains), built
    /// from two settings groups, with and without borders and the optional filters, at negative
    /// regions. Each line folds every backing cell's value, erosion, sediment and gradient bits and
    /// terrain id.
    #[test]
    fn world_filters_on_tiles_match_engine_oracle() {
        use super::super::terraforged_engine::{Levels, WorldControlPoints};
        use super::super::terraforged_filters::{FilterSettings, WorldFilters};

        let terrains = TerrainRegistry::new();
        let kinds = ["none", "coast", "beach", "hills"]
            .map(|name| terrains.get_by_name(name).expect("built-in terrain"));
        let filters =
            [(12345, 20, 1, 0.327), (-9, 5, 2, 0.4)].map(|(seed, droplets, passes, beach)| {
                let mut settings = FilterSettings::default();
                settings.erosion.droplets_per_chunk = droplets;
                settings.smoothing.iterations = passes;
                let points = WorldControlPoints {
                    beach,
                    ..WorldControlPoints::default()
                };
                WorldFilters::new(seed, settings, Levels::new(256, 63), points, &terrains)
            });
        let mut out = String::new();
        for (run, (group, region_x, region_z, size, border, optional)) in [
            (0, -1, 2, 1, 1, true),
            (0, 3, -5, 2, 1, true),
            (1, 0, 0, 1, 0, true),
            (1, -1, 2, 1, 1, false),
            (1, -7, 9, 1, 1, true),
        ]
        .into_iter()
        .enumerate()
        {
            let salt = run as i32;
            let mut tile = Tile::new(region_x, region_z, size, border, &terrains);
            let total = tile.block_size().total;
            for (index, cell) in tile.blocks.iter_mut().enumerate() {
                let index = index as i32;
                let (x, z) = (index % total, index / total);
                let mut h = index
                    .wrapping_mul(7919)
                    .wrapping_add(salt.wrapping_mul(104_729));
                h ^= ((h as u32) >> 13) as i32;
                h = h.wrapping_mul(0x5bd1_e995);
                h ^= ((h as u32) >> 15) as i32;
                let h = h as u32;
                *cell = Cell::new(&terrains);
                cell.value = ((3 * x + 2 * z + salt) % 97) as f32 / 2000.0
                    + 0.2
                    + ((h >> 8) & 0xFFFF) as f32 / 40_000_000.0;
                cell.river_mask = if (h >> 4) & 3 == 0 { 0.05 } else { 1.0 };
                cell.erosion_mask = h & 63 == 17;
                cell.continent_edge = ((h >> 12) & 0x3FF) as f32 / 1600.0;
                cell.terrain = kinds[((h >> 1) & 3) as usize].clone();
            }
            filters[group].apply_tile(&mut tile, optional);
            let bits = |value: f32| {
                if value.is_nan() {
                    0x7fc0_0000
                } else {
                    value.to_bits() as i32
                }
            };
            let mut folds = [17_i64; 5];
            for cell in &tile.blocks {
                let values = [
                    bits(cell.value),
                    bits(cell.erosion),
                    bits(cell.sediment),
                    bits(cell.gradient),
                    cell.terrain.id(),
                ];
                for (slot, value) in folds.iter_mut().zip(values) {
                    *slot = fold(*slot, value);
                }
            }
            let [v, e, s, g, t] = folds;
            out += &format!("w {run} {v:x} {e:x} {s:x} {g:x} {t:x}\n");
        }
        assert_eq!(
            out,
            "w 0 78353e881e79d874 94d49cd6ced8988f 4c789901940be6fc 392a6fa117a2a29a 9b357af0388e9be0\n\
             w 1 e36a67ca0adfd553 15b9d13d12cc6d8a 8a93963318e02e8a 59f11be02765b987 27ad111cd483b1ef\n\
             w 2 e1152b41c2272c6e 89dafe5a36917f5d 655ba97b673166e6 aae493e9fd6049 f94ef798a48a202a\n\
             w 3 f1abc2a41207e9fe bcb74212e5be0011 bcb74212e5be0011 e3e693d1b08be63d 8d940be0f635f540\n\
             w 4 3bcfc87a7ec4212e 9a4db5329ff5152c d10543f7129cf897 90e87f4ad4a4bba 35389eeae71f8f8a\n"
        );
    }

    /// Engine's tile generators build a tile on one thread and hand it to others (cache, chunk
    /// readers).
    #[test]
    fn tiles_are_shared_between_threads() {
        fn shareable<T: Send + Sync>() {}
        shareable::<Tile>();
    }

    /// `TileHeightmapOracle.java`'s output (`h <tile> <lookups> <trace> <cells> <chunks>`).
    const ORACLE_HEIGHTMAP: &str = "\
h 0 137 c579e321cc64ef46 7822c03163bbbc11 607ea5d406ecf6e3
h 1 392 c0e0a699c90ced0d 2981a38add7f911 53f9d0616309a431
h 2 4 8f581bc24f772753 ae8d5f770c7cf611 e6a262e262c7fb09
h 3 326 818e2be4c3bf53ee 173ca2a1adf97291 3491e8a4a9b0d6af
h 4 397 ee81d23a3022096c 3d61aa1f75ddd511 d5f772aa44118431
h 5 394 a824e4624bec2cf6 a63ce1c7f0e710c1 53f9d0616309a431
c 0 137 c579e321cc64ef46 7822c03163bbbc11 607ea5d406ecf6e3
c 1 392 c0e0a699c90ced0d 2981a38add7f911 53f9d0616309a431
c 2 2 fd28976be4c0c612 ae8d5f770c7cf611 e6a262e262c7fb09
c 3 230 47100b8cd34dabbe 173ca2a1adf97291 3491e8a4a9b0d6af
c 4 262 cbc232019b55f74f 3d61aa1f75ddd511 d5f772aa44118431
c 5 4 d7c7d9a9730594b3 3c74c270245f10c1 53f9d0616309a431
r 0 0 11 7822c03163bbbc11 607ea5d406ecf6e3
r 1 0 11 2981a38add7f911 53f9d0616309a431
r 2 0 11 ae8d5f770c7cf611 e6a262e262c7fb09
r 3 0 11 173ca2a1adf97291 3491e8a4a9b0d6af
r 4 0 11 3d61aa1f75ddd511 d5f772aa44118431
z0 0 134 8ea15ead27a7ffee 95ff100ddc948151 a1534faf119d76e3
z1 1 397 a4920f9406cf23fe 9f022fd4197c4b11 87c99a472732a431
z2 2 64 9eb40a75d6253b23 b3f6c662a8ada9f1 762abd6ba4b2a741
z3 3 6400 adc52486a5625211 f182775492fb2581 3582f5566a8202af
z4 4 1536 ef93d983ff962049 5b680d8e2d335951 389702ecc1f58431
z5 1 1 97ce42bd56402149 3522cae4d88f0011 efddc98dcf89a431
z6 3 1 5d72963933ef2149 a65e2ef6c57fc011 27d53d6c44cd6af
z7 2 1 1a30fe44b3e02149 80c4fb1889d70011 cbcecae73e4ffb09
z8 0 8 5cda1b5e2ac65c8d f6d78c512b21b051 81ad7116de88f6e3
z9 4 192 388dc8006597a611 d1994d5cf898d4d1 34af015dc4958431
z10 0 1 1368551b5e1d0e92 8184acb51a1f7011 81ad7116de88f6e3
z11 5 3 2df8e02d6cd392c5 9ad4e7a2c49f5836 f941d58eec9b5de1
z12 4 1 6ce2a26113d5f45e 4b01fc4e28169c11 774fcf24a9588431
";

    /// `TileHeightmapOracle.java`'s zoomed runs: offset x, offset z, zoom, then the
    /// [`HEIGHTMAP_TILES`] tile (fractional, unit, large, negative, signed-zero, NaN and
    /// overflowing zooms; NaN and infinite offsets).
    const HEIGHTMAP_ZOOMS: [(f32, f32, f32, usize); 13] = [
        (100.0, -50.0, 0.5, 0),
        (0.0, 0.0, 1.0, 1),
        (-12345.678, 9876.5, 0.3, 2),
        (3.0e6, -7.0e5, 1000.0, 3),
        (17.25, -3.5, -2.5, 4),
        (-0.0, 0.0, 0.0, 1),
        (-0.0, -0.0, -0.0, 3),
        (5.0, 5.0, f32::NAN, 2),
        (f32::NAN, 1.0, 1.0, 0),
        (0.0, 0.0, 1e37, 4),
        (f32::INFINITY, f32::NEG_INFINITY, 2.0, 0),
        (1.0, 2.0, 0.1, 5),
        (-1.0e7, 1.0e7, 1.0e-3, 4),
    ];

    /// How [`replay_heightmap`] fills its tile.
    #[derive(Clone, Copy)]
    enum HeightmapFill<'a> {
        Direct,
        Cached(&'a HeightmapCache),
        Zoomed(f32, f32, f32),
    }

    /// The tiles `TileHeightmapOracle.java` fills: region x and z, size factor, border chunks,
    /// then the tile-local chunk writers made before `generate` (aliasing slots past the edge).
    const HEIGHTMAP_TILES: [(i32, i32, i32, i32, &[(i32, i32)]); 6] = [
        (0, 0, 1, 1, &[]),
        (-2, 3, 2, 1, &[]),
        (1, -1, 1, 0, &[]),
        (-1, -1, 0, 2, &[]),
        (0x1000_0000, -0x1000_0001, 2, 1, &[]),
        (-2, 3, 2, 1, &[(-1, 1), (6, 0), (12, 3)]),
    ];

    /// Replays `TileHeightmapOracle.java` (`.local/terraforged/oracles/tile_heightmap/`): Engine
    /// 0.3.0's `Tile.generate(Heightmap)` (`h` lines) over a heightmap whose stages are
    /// deterministic fills and whose continent makes a new empty river map per lookup, then
    /// `Tile.generate(HeightmapCache)` through one real `HeightmapCache` shared by the six tiles in
    /// order (`c` lines: overlapping tiles mix hits and misses) and over tiles 0-4 again in fresh
    /// tiles (`r` lines: all hits). Folds the stage trace (stage, coordinate bits, the map each cell
    /// gets), every backing cell and `getChunk` over every tile-local chunk position. Tile 5 is
    /// not repeated: Engine's cache holds the tile's cell objects, which its aliasing chunk refills.
    /// Last, `Tile.generate(Heightmap, offsetX, offsetZ, zoom)` over [`HEIGHTMAP_ZOOMS`] (`z`
    /// lines). Floats fold as `floatToIntBits` (NaNs canonical).
    #[test]
    fn tile_heightmap_generation_matches_engine_0_3_0() {
        let terrains = Arc::new(TerrainRegistry::new());
        let cache =
            HeightmapCache::with_capacity(Levels::new(256, 63), Arc::clone(&terrains), 1 << 20);
        let mut out = String::new();
        for run in 0..HEIGHTMAP_TILES.len() {
            out += &replay_heightmap(&terrains, "h", run, HeightmapFill::Direct);
        }
        for run in 0..HEIGHTMAP_TILES.len() {
            out += &replay_heightmap(&terrains, "c", run, HeightmapFill::Cached(&cache));
        }
        for run in 0..5 {
            out += &replay_heightmap(&terrains, "r", run, HeightmapFill::Cached(&cache));
        }
        for (run, (offset_x, offset_z, zoom, tile)) in HEIGHTMAP_ZOOMS.into_iter().enumerate() {
            let fill = HeightmapFill::Zoomed(offset_x, offset_z, zoom);
            out += &replay_heightmap(&terrains, &format!("z{run}"), tile, fill);
        }
        assert_eq!(out, ORACLE_HEIGHTMAP);
    }

    /// One line of `TileHeightmapOracle.java`: fills tile `run` of [`HEIGHTMAP_TILES`] with
    /// `generate_heightmap`, `generate_cached_heightmap` or `generate_zoomed_heightmap`, and
    /// folds it.
    fn replay_heightmap(
        terrains: &TerrainRegistry,
        tag: &str,
        run: usize,
        fill: HeightmapFill<'_>,
    ) -> String {
        let (region_x, region_z, size, border, writers) = HEIGHTMAP_TILES[run];
        let mut tile = Tile::new(region_x, region_z, size, border, terrains);
        for &(chunk_x, chunk_z) in writers {
            assert!(tile.chunk_writer(chunk_x, chunk_z).is_some());
        }
        // The stages share the trace, as Engine's oracle mixes into one fold.
        let trace = std::cell::Cell::new(17_i64);
        let mix = |value: i32| trace.set(fold(trace.get(), value));
        let mix_point = |stage: i32, x: f32, z: f32| {
            mix(stage);
            mix(java_bits(x));
            mix(java_bits(z));
        };
        let lookups = std::cell::Cell::new(0);
        let apply_base = |cell: &mut Cell, x: f32, z: f32| {
            mix_point(1, x, z);
            cell.value = x * 0.25 + z;
            cell.moisture += x;
            cell.continent_x = (x as i32).div_euclid(24);
            cell.continent_z = (z as i32).div_euclid(40);
        };
        let lookup = |x, z| {
            lookups.set(lookups.get() + 1);
            mix(1000);
            mix(x);
            mix(z);
            Arc::new(Rivermap::new(x, z, Vec::new(), GenWarp::direct()))
        };
        let apply_rivers = |cell: &mut Cell, x: f32, z: f32, map: &Rivermap| {
            mix_point(2, x, z);
            mix(map.x());
            mix(map.z());
            cell.river_mask = (map.x() * 10 + map.z()) as f32 + cell.value;
        };
        let apply_climate = |cell: &mut Cell, x: f32, z: f32| {
            mix_point(3, x, z);
            cell.temperature = z - cell.river_mask;
            cell.erosion += 1.0;
        };
        match fill {
            HeightmapFill::Direct => {
                tile.generate_heightmap(apply_base, lookup, apply_rivers, apply_climate)
            }
            HeightmapFill::Cached(cache) => tile.generate_cached_heightmap(
                cache,
                apply_base,
                lookup,
                apply_rivers,
                apply_climate,
            ),
            HeightmapFill::Zoomed(offset_x, offset_z, zoom) => tile.generate_zoomed_heightmap(
                offset_x,
                offset_z,
                zoom,
                apply_base,
                lookup,
                apply_rivers,
                apply_climate,
            ),
        }
        let mut cells = 17_i64;
        for cell in &tile.blocks {
            for value in [
                cell.value,
                cell.erosion,
                cell.sediment,
                cell.gradient,
                cell.moisture,
                cell.temperature,
                cell.continent_id,
                cell.continent_edge,
                cell.terrain_region_id,
                cell.terrain_region_edge,
                cell.biome_region_id,
                cell.biome_region_edge,
                cell.macro_biome_id,
                cell.river_mask,
            ] {
                cells = fold(cells, java_bits(value));
            }
            for value in [
                cell.continent_x,
                cell.continent_z,
                i32::from(cell.erosion_mask),
                cell.terrain.id(),
            ] {
                cells = fold(cells, value);
            }
        }
        let mut chunks = 17_i64;
        let total = tile.chunk_size.total;
        for chunk_z in -1..=total {
            for chunk_x in -1..=total {
                let world_x = tile.chunk_x.wrapping_add(chunk_x).wrapping_sub(border);
                let world_z = tile.chunk_z.wrapping_add(chunk_z).wrapping_sub(border);
                let reader = tile.chunk(world_x, world_z).unwrap();
                let chunk = reader.chunk();
                for value in [
                    chunk.chunk_x(),
                    chunk.chunk_z(),
                    chunk.block_x(),
                    chunk.block_z(),
                    java_bits(reader.cell(3, 5).value),
                ] {
                    chunks = fold(chunks, value);
                }
            }
        }
        let (lookups, trace) = (lookups.get(), trace.get());
        format!("{tag} {run} {lookups} {trace:x} {cells:x} {chunks:x}\n")
    }

    /// Java's `Float.floatToIntBits`: every NaN as the canonical `0x7fc00000`.
    fn java_bits(value: f32) -> i32 {
        if value.is_nan() {
            0x7fc0_0000
        } else {
            value.to_bits() as i32
        }
    }
}
