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

use super::terraforged::{clamp, java_max, java_pow, lerp, round};
use super::{
    super::terrain::{TerrainBlockStates, TerrainChunk, TerrainError},
    terraforged_terrain_data::TerrainData,
};
use crate::{heightmap::HeightmapType, registry::BlockStateId};

struct ChunkColumns<'a, 'r, B: TerrainBlockStates> {
    chunk: &'a mut TerrainChunk<'r, B>,
    stone: BlockStateId,
    air: BlockStateId,
}

impl<B: TerrainBlockStates> Columns for ChunkColumns<'_, '_, B> {
    type Error = TerrainError;

    fn ocean_floor(&self, x: i32, z: i32) -> i32 {
        self.chunk
            .heightmap(HeightmapType::OceanFloorWg)
            .expect("structure heightmap primed")
            .highest_taken(x & 15, z & 15)
            .expect("masked chunk coordinates")
    }

    fn set(&mut self, x: i32, y: i32, z: i32, fill: Fill) -> Result<(), Self::Error> {
        if y < self.chunk.min_y() || y >= self.chunk.min_y() + self.chunk.height() {
            return Ok(());
        }
        let state = match fill {
            Fill::Stone => self.stone,
            Fill::Air => self.air,
        };
        self.chunk.set_block_worldgen(x & 15, y, z & 15, state)?;
        for kind in [HeightmapType::OceanFloorWg, HeightmapType::WorldSurfaceWg] {
            self.chunk
                .update_heightmap(kind, x & 15, y, z & 15, state)?;
        }
        Ok(())
    }
}

/// A structure piece's `BoundingBox` (inclusive block bounds).
#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub min_x: i32,
    pub min_y: i32,
    pub min_z: i32,
    pub max_x: i32,
    pub max_y: i32,
    pub max_z: i32,
}

/// One rigid structure piece `StructureTerrain`'s constructor keeps; `ground_delta` is a pool piece's
/// `getGroundLevelDelta()` (0 for any other piece, as `getPieceY`).
#[derive(Clone, Copy, Debug)]
pub struct Piece {
    pub bounds: Bounds,
    pub ground_delta: i32,
}

impl Piece {
    /// `getPieceY`: the box's `minY` plus the ground delta (Java `int` wrap).
    pub fn floor(self) -> i32 {
        self.bounds.min_y.wrapping_add(self.ground_delta)
    }
}

/// The two blocks `modify` writes: `Blocks.STONE` (raising) and `Blocks.AIR` (carving).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fill {
    Stone,
    Air,
}

/// The chunk as `modify` sees it: `ocean_floor` is `chunk.getHeight(OCEAN_FLOOR_WG, x, z)` (the topmost
/// motion-blocking block, reflecting earlier writes), `set` is `setBlockState(pos, state, false)` (a chunk drops
/// writes outside its build range, as `ProtoChunk` does). Coordinates are world block coordinates.
pub trait Columns {
    type Error;
    fn ocean_floor(&self, x: i32, z: i32) -> i32;
    fn set(&mut self, x: i32, y: i32, z: i32, fill: Fill) -> Result<(), Self::Error>;
}

/// TerraForged 6dd607eb's `worldgen.terrain.StructureTerrain` without its constructor's piece collection.
pub struct StructureTerrain {
    pieces: Vec<Piece>,
}

impl StructureTerrain {
    pub fn modify_chunk<B: TerrainBlockStates>(
        &self,
        chunk: &mut TerrainChunk<'_, B>,
        terrain: &mut TerrainData,
        stone: BlockStateId,
        air: BlockStateId,
    ) -> Result<(), TerrainError> {
        for kind in [HeightmapType::OceanFloorWg, HeightmapType::WorldSurfaceWg] {
            if chunk.heightmap(kind).is_none() {
                chunk.prime_heightmaps(&[kind])?;
            }
        }
        let origin = chunk.position();
        let mut columns = ChunkColumns { chunk, stone, air };
        for dz in 0..16 {
            for dx in 0..16 {
                let x = origin.x.wrapping_shl(4).wrapping_add(dx);
                let z = origin.z.wrapping_shl(4).wrapping_add(dz);
                if let Some(height) = self.modify(x, z, &mut columns)? {
                    terrain.set_height_at(x, z, height);
                }
            }
        }
        Ok(())
    }

    /// The constructor's `rigids.sort(PIECE_SORTER)`: pieces by box `minY`, stable (fastutil's sort), from the
    /// pieces the caller collected (BEARD_BOX/BEARD_THIN starts, close to the chunk, rigid pool elements).
    pub fn from_rigid_pieces(mut pieces: Vec<Piece>) -> Self {
        pieces.sort_by_key(|piece| piece.bounds.min_y);
        Self { pieces }
    }

    /// `modify(x, z, chunk, terrainData)` for one column (world block coordinates): raises the ground towards the
    /// pieces above it, carves the highest piece covering the column, and returns the new OCEAN_FLOOR_WG height
    /// when it raised or carved (the value Java writes into `terrainData.getHeight()`), else `None`. A failed write
    /// stops at once with the error (Java has none).
    pub fn modify<C: Columns>(
        &self,
        x: i32,
        z: i32,
        chunk: &mut C,
    ) -> Result<Option<i32>, C::Error> {
        let surface = chunk.ocean_floor(x, z);
        let mut max_y = surface as f32;
        let mut max_position_y = surface;
        let mut highest: Option<Piece> = None;
        for piece in &self.pieces {
            let bounds = piece.bounds;
            let length = bounds
                .max_x
                .wrapping_sub(bounds.min_x)
                .wrapping_add(1)
                .max(bounds.max_z.wrapping_sub(bounds.min_z).wrapping_add(1));
            let radius = 4.max(20i32.wrapping_sub(length)) as f32;
            let floor = piece.floor();
            max_position_y = max_position_y.max(floor);
            if highest.is_none() && floor > surface {
                max_y = raise(x, z, bounds, floor as f32, max_y, radius);
            }
            if x >= bounds.min_x
                && x <= bounds.max_x
                && z >= bounds.min_z
                && z <= bounds.max_z
                && highest.is_none_or(|previous| bounds.min_y > previous.bounds.min_y)
            {
                highest = Some(*piece);
            }
        }
        let max = max_y as i32;
        let raised = surface.wrapping_add(1) < max;
        if raised {
            for y in surface..max {
                chunk.set(x, y, z, Fill::Stone)?;
            }
        }
        if let Some(piece) = highest {
            let ceiling = max_position_y.max(chunk.ocean_floor(x, z));
            let mut roof = piece.bounds.max_y;
            if ceiling > roof.wrapping_add(5) {
                let depth = ceiling.wrapping_sub(roof) as f32 * 0.5;
                roof = roof.wrapping_add(round(depth * ellipse_alpha(x, z, piece.bounds)));
            }
            for y in piece.floor()..=roof {
                chunk.set(x, y, z, Fill::Air)?;
            }
        }
        Ok((raised || highest.is_some()).then(|| chunk.ocean_floor(x, z)))
    }
}

/// `StructureTerrain.raise`: the surface lerped towards `level` by `pow(alpha, 2 - alpha)` of the distance alpha.
pub fn raise(x: i32, z: i32, bounds: Bounds, level: f32, surface: f32, radius: f32) -> f32 {
    let radius_squared = java_max(1.0, radius * radius);
    let alpha = 1.0 - distance_alpha(x, z, bounds, radius_squared);
    let alpha = java_pow(alpha, 2.0 - alpha);
    lerp(surface, level, alpha)
}

/// `getEllipseDistAlpha`: `1 - (dx/rx)^2 - (dz/rz)^2` over the box's centre, clamped to 0..1.
pub fn ellipse_alpha(x: i32, z: i32, bounds: Bounds) -> f32 {
    let radius_x = bounds.max_x.wrapping_sub(bounds.min_x).wrapping_add(1) as f32 * 0.5;
    let radius_z = bounds.max_z.wrapping_sub(bounds.min_z).wrapping_add(1) as f32 * 0.5;
    let center_x = bounds.min_x.wrapping_add(bounds.max_x) as f32 * 0.5;
    let center_z = bounds.min_z.wrapping_add(bounds.max_z) as f32 * 0.5;
    let dx = x as f32 - center_x;
    let dz = z as f32 - center_z;
    let qx = dx * dx / (radius_x * radius_x);
    let qz = dz * dz / (radius_z * radius_z);
    clamp(1.0 - qx - qz, 0.0, 1.0)
}

/// `getDistAlpha(x, z, box, radius2)`: the squared distance outside the box (`int` math) over `radius_squared`,
/// 0 inside the box, 1 at or beyond the radius, else its square root.
pub fn distance_alpha(x: i32, z: i32, bounds: Bounds, radius_squared: f32) -> f32 {
    let dx = 0.max(
        bounds
            .min_x
            .wrapping_sub(x)
            .max(x.wrapping_sub(bounds.max_x)),
    );
    let dz = 0.max(
        bounds
            .min_z
            .wrapping_sub(z)
            .max(z.wrapping_sub(bounds.max_z)),
    );
    let distance = dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz));
    if distance == 0 {
        0.0
    } else if distance as f32 >= radius_squared {
        1.0
    } else {
        f64::from(distance as f32 / radius_squared).sqrt() as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, convert::Infallible};

    struct Column {
        stone: BTreeSet<i32>,
        writes: usize,
        hash: u64,
    }

    impl Columns for Column {
        type Error = Infallible;
        fn ocean_floor(&self, _: i32, _: i32) -> i32 {
            self.stone.last().copied().unwrap_or(-65)
        }
        fn set(&mut self, _: i32, y: i32, _: i32, fill: Fill) -> Result<(), Self::Error> {
            if fill == Fill::Stone {
                self.stone.insert(y);
            } else {
                self.stone.remove(&y);
            }
            self.hash = (self.hash ^ u64::from(y as u32)).wrapping_mul(0x100000001b3);
            self.hash = (self.hash ^ u64::from(fill == Fill::Stone)).wrapping_mul(0x100000001b3);
            self.writes += 1;
            Ok(())
        }
    }

    /// The PR's original fixture (`oracles/structure_terrain/run_structure_columns_oracle.py`: 6dd607eb's methods
    /// from `modify` on over one column, folding only y and the block).
    #[test]
    fn column_edits_match_original_java_methods() {
        let low = Piece {
            bounds: Bounds {
                min_x: 0,
                min_y: 70,
                min_z: 0,
                max_x: 6,
                max_y: 75,
                max_z: 6,
            },
            ground_delta: 0,
        };
        let high = Piece {
            bounds: Bounds {
                min_x: 2,
                min_y: 90,
                min_z: 2,
                max_x: 4,
                max_y: 94,
                max_z: 4,
            },
            ground_delta: 2,
        };
        let operation = StructureTerrain::from_rigid_pieces(vec![high, low]);
        for (x, z, surface, expected, writes, hash) in [
            (-3, 3, 60, Some(82), 23, 0x039a3b6da88e6519),
            (3, 3, 60, Some(69), 13, 0x95373c2188a033ff),
            (3, 3, 110, Some(110), 11, 0xbf7cdf7e8f60ab2a),
            (0, 0, 80, Some(80), 6, 0xccf428bf82e9676c),
            (30, 30, 60, None, 0, 0xcbf29ce484222325),
            (6, 6, 60, Some(69), 16, 0x74a24909aacd2019),
        ] {
            let mut column = Column {
                stone: (-64..=surface).collect(),
                writes: 0,
                hash: 0xcbf29ce484222325,
            };
            assert_eq!(operation.modify(x, z, &mut column), Ok(expected));
            assert_eq!(
                (column.writes, column.hash),
                (writes, hash),
                "{x},{z},{surface}"
            );
        }
    }

    /// `oracles/structure_terrain/` (`run.sh`): TerraForged 6dd607eb's `StructureTerrain.java`, compiled unchanged
    /// (fastutil's `ObjectArrayList` sort and iterator, Noise2D's `NoiseUtil`), run over stub chunks by
    /// `buildStructureTerrain`'s loop. Per scenario: kept block writes, their FNV-1a fold (x, y, z, block), the
    /// height-map writes and their fold (x, z, float bits). Interpreted and JIT runs agree.
    const JAVA_COLUMNS: [(u32, u32, u64, u32, u64); 96] = [
        (0, 1040, 0x0961a3d1d3cba1b1, 110, 0x3f200c6b0ed66f28),
        (1, 5846, 0x459a0533fd4212d5, 220, 0x02de1c706fde55ae),
        (2, 2671, 0x4ca4d4f92307f58a, 179, 0xaa98e1edd63101b5),
        (3, 1239, 0x9077f5445ef6f31c, 80, 0x769a2d98f0a31d85),
        (4, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (5, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (6, 4503, 0x87e84e72f212608d, 132, 0x4ee68d20f9921569),
        (7, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (8, 2317, 0x552776865983c8c6, 243, 0x7bfc1a2623b96fb9),
        (9, 3605, 0xdb4cdc58afb12885, 126, 0x47de3b86dd61e62e),
        (10, 4088, 0xc849031f3fc8d064, 240, 0x90690ac174c662d5),
        (11, 136, 0x2b1ac9b0b6ba9a7c, 16, 0xc9c4b0c256d62d45),
        (12, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (13, 1116, 0x2410fb00a75de82d, 64, 0x13cbb3aeff3381f6),
        (14, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (15, 23405, 0xdf451069ecac6dc0, 192, 0xa097b95166b5a1a5),
        (16, 7493, 0x2c4c7d6f7184e039, 185, 0x87f8aa27c39db39e),
        (17, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (18, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (19, 448, 0x25c5ccabb66918f9, 42, 0x810fae7c5a093120),
        (20, 2559, 0xf2e9af1d6809f917, 155, 0xd63216007e2dfa55),
        (21, 193, 0x84948aba7d16dcc2, 42, 0x85d6c2cef3841ac3),
        (22, 2426, 0x7eb5c34676512c23, 208, 0x7f0f700f9c616605),
        (23, 14458, 0x63ff63431008b126, 225, 0x378b6958b32f2f11),
        (24, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (25, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (26, 45, 0xcf3edc3a6d8bc7eb, 5, 0x9d7b6acc5c44847d),
        (27, 9372, 0x59ae2fc0ca6a7936, 253, 0x3f38ae4b81c9e6cd),
        (28, 546, 0x734384bf9fedb368, 30, 0x957b0d3036aa7336),
        (29, 9970, 0x4a8f239150ae534a, 241, 0xce7db0ed67f0de87),
        (30, 9439, 0xc22557b23996a6ec, 256, 0x2d7276456e8f06e5),
        (31, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (32, 9277, 0x1aecb1551c8c3973, 235, 0x204b3df1082c8d7c),
        (33, 262, 0x11b0df495e0402b9, 45, 0x5eaf7da77afbf3a1),
        (34, 1752, 0x70792c1f49b9451a, 143, 0x2d5d737cc57ff451),
        (35, 2767, 0xa4e199d382819d91, 135, 0xe88a6bedf3ad1b7f),
        (36, 4372, 0xd2d5fe11a2ec7cf3, 206, 0x07353627191c8adc),
        (37, 451, 0xb6f965a94dced141, 45, 0x23b44ecbda0c499e),
        (38, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (39, 31655, 0x46d94007f0114f5a, 256, 0x15aa70c76e032825),
        (40, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (41, 10665, 0xf963540bc9ecaf2c, 224, 0xeabec3b6f69e2a89),
        (42, 393, 0x24c07be148e51d24, 36, 0xb5a746a802138c06),
        (43, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (44, 1141, 0x89269e726da5e0fa, 92, 0x10b5df56383198ad),
        (45, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (46, 1791, 0xed809636df76b043, 163, 0x6e03e5a1e1dbccbf),
        (47, 5372, 0xbcf00a7d9aa36da9, 147, 0xab5403657c5bc26f),
        (48, 7626, 0x76ba64cc297b4d7b, 239, 0xc47d5c43dded361c),
        (49, 226, 0xf42894abd0478ef8, 22, 0xf4d00110f2638e10),
        (50, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (51, 2795, 0xad1d019602b14a68, 109, 0xe23fcb424a36577a),
        (52, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (53, 3174, 0x4e28fce261f873b0, 221, 0xb06c42f624e9c2c8),
        (54, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (55, 40408, 0xbe64ed8c5b1390c4, 256, 0xae365c552a8c0d25),
        (56, 1554, 0xfaee073fcb778129, 98, 0x86be4fbb35b857f4),
        (57, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (58, 1262, 0x768f61bab07503b7, 112, 0x8b7dd47a83ebb005),
        (59, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (60, 4698, 0x8c320e44c488f5d0, 243, 0x569a15dc48d49057),
        (61, 238, 0x62f083084c72c3be, 16, 0x402ff9536af32225),
        (62, 2322, 0xe2b6b6601adf8512, 256, 0x550317aaaab11625),
        (63, 30310, 0xfc7b0ffbe3056b09, 256, 0x3372654b2d53e525),
        (64, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (65, 5460, 0x6d57cff6cba5bbad, 220, 0x15a863e16b5ba080),
        (66, 731, 0x89a5ab3cb1472293, 77, 0xa6018a876e4bdf80),
        (67, 1662, 0x18e750d0d4f5d935, 132, 0x969228b67cfb232c),
        (68, 13060, 0x1656e42da30b855c, 202, 0x20fb1e98cee00193),
        (69, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (70, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (71, 14069, 0x2f57ce4119e6832d, 154, 0x26e7c9157ad7d2c2),
        (72, 532, 0xf931062b5bf17c95, 56, 0xbc94cdbf61378f8d),
        (73, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (74, 6056, 0x23d5f2f5adc9d86b, 208, 0x1c2bf7dc092c0225),
        (75, 1387, 0x183f6eaec3f8bc69, 98, 0xdb1e236d70ee40c9),
        (76, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (77, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (78, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (79, 24555, 0x42c61388b3c69676, 176, 0x7c1e179d8bbb3755),
        (80, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (81, 3500, 0xf809ae1e4f505362, 121, 0x3b65cbb69586b07e),
        (82, 165, 0x464fdb103181e727, 27, 0xfba2f606060b1274),
        (83, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (84, 4849, 0x504d4b91d0af7a12, 209, 0x7412f86afe301dbd),
        (85, 2928, 0xd5452eac4903e844, 208, 0xb1541598e6a57685),
        (86, 3332, 0x2fdaf6f39bb05339, 95, 0xd9a9a85184931ff5),
        (87, 8906, 0x3a2a5e52b3ba5ddd, 141, 0x1755d826547be818),
        (88, 1878, 0x92119601b221a303, 156, 0xeca9c0f4bf73de3f),
        (89, 249, 0x955478f8b82e341a, 57, 0xb26052527c8aabdb),
        (90, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (91, 4865, 0x0a9704871fd33cf3, 244, 0x4c0dfdc03870db35),
        (92, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (93, 4257, 0x43f1800445a46252, 189, 0xff947dba50cfc380),
        (94, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
        (95, 0, 0xcbf29ce484222325, 0, 0xcbf29ce484222325),
    ];

    /// The oracle's stub chunk: stone/air columns over `min_y..max_y`, `ocean_floor` = topmost stone or `min_y - 1`,
    /// writes outside the build range dropped (as `ProtoChunk.setBlockState`).
    struct OracleChunk {
        min_y: i32,
        max_y: i32,
        stone: Vec<Vec<bool>>,
        writes: u32,
        hash: u64,
    }

    fn fold(hash: u64, value: i32) -> u64 {
        (hash ^ u64::from(value as u32)).wrapping_mul(0x100000001b3)
    }

    impl Columns for OracleChunk {
        type Error = Infallible;
        fn ocean_floor(&self, x: i32, z: i32) -> i32 {
            let column = &self.stone[(((z & 15) << 4) | (x & 15)) as usize];
            (self.min_y..self.max_y)
                .rev()
                .find(|y| column[(y - self.min_y) as usize])
                .unwrap_or(self.min_y - 1)
        }
        fn set(&mut self, x: i32, y: i32, z: i32, fill: Fill) -> Result<(), Self::Error> {
            if y < self.min_y || y >= self.max_y {
                return Ok(());
            }
            let id = i32::from(fill == Fill::Stone);
            self.stone[(((z & 15) << 4) | (x & 15)) as usize][(y - self.min_y) as usize] = id != 0;
            self.hash = fold(fold(fold(fold(self.hash, x), y), z), id);
            self.writes += 1;
            Ok(())
        }
    }

    /// The oracle's LCG (Java `long` math, `(int) ((state >>> 33) % bound)`).
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self, bound: i32) -> i32 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 33) % bound as u64) as i32
        }
    }

    #[test]
    fn chunk_scenarios_match_6dd607eb_java() {
        for (s, writes, hash, sets, heights) in JAVA_COLUMNS {
            let mut random = Lcg(u64::from(s)
                .wrapping_mul(0x9E3779B97F4A7C15)
                .wrapping_add(12345));
            let (cx, cz) = (random.next(9) - 5, random.next(9) - 5);
            let (min_y, max_y) = (-64, 320);
            let base = match s % 8 {
                6 => 300 + random.next(30),
                7 => -70 + random.next(10),
                _ => 40 + random.next(60),
            };
            let surface: Vec<i32> = (0..256).map(|_| base + random.next(13) - 6).collect();
            let count = random.next(8);
            let shared_y = 40 + random.next(90);
            let mut pieces = Vec::new();
            for _ in 0..count {
                let x0 = cx * 16 + random.next(44) - 14;
                let z0 = cz * 16 + random.next(44) - 14;
                let y0 = if random.next(3) == 0 {
                    shared_y
                } else if s % 8 == 6 {
                    280 + random.next(40)
                } else {
                    30 + random.next(110)
                };
                let bounds = Bounds {
                    min_x: x0,
                    min_y: y0,
                    min_z: z0,
                    max_x: x0 + random.next(30),
                    max_y: y0 + random.next(14),
                    max_z: z0 + random.next(30),
                };
                let ground_delta = if random.next(2) == 0 {
                    0
                } else {
                    random.next(7) - 3
                };
                pieces.push(Piece {
                    bounds,
                    ground_delta,
                });
            }
            let mut chunk = OracleChunk {
                min_y,
                max_y,
                stone: surface
                    .iter()
                    .map(|&top| (min_y..max_y).map(|y| y <= top).collect())
                    .collect(),
                writes: 0,
                hash: 0xcbf29ce484222325,
            };
            let operation = StructureTerrain::from_rigid_pieces(pieces);
            let (mut height_sets, mut height_hash) = (0, 0xcbf29ce484222325);
            for dz in 0..16 {
                for dx in 0..16 {
                    let (x, z) = (cx * 16 + dx, cz * 16 + dz);
                    if let Some(height) = operation.modify(x, z, &mut chunk).unwrap() {
                        height_hash = fold(
                            fold(fold(height_hash, x), z),
                            (height as f32).to_bits() as i32,
                        );
                        height_sets += 1;
                    }
                }
            }
            assert_eq!(
                (chunk.writes, chunk.hash, height_sets, height_hash),
                (writes, hash, sets, heights),
                "scenario {s}"
            );
        }
    }

    /// Equal `minY`s keep the caller's order (fastutil's stable sort): the first piece's ground delta wins.
    #[test]
    fn tied_piece_heights_keep_input_order() {
        let bounds = Bounds {
            min_x: 0,
            min_y: 70,
            min_z: 0,
            max_x: 6,
            max_y: 75,
            max_z: 6,
        };
        let operation = StructureTerrain::from_rigid_pieces(vec![
            Piece {
                bounds,
                ground_delta: 2,
            },
            Piece {
                bounds,
                ground_delta: 0,
            },
        ]);
        assert_eq!(operation.pieces[0].ground_delta, 2);
        let mut column = Column {
            stone: (-64..=70).collect(),
            writes: 0,
            hash: 0xcbf29ce484222325,
        };
        operation.modify(3, 3, &mut column).unwrap();
        assert!(column.stone.contains(&70));
    }
}
