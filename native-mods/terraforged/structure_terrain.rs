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

use super::super::structure::{
    jigsaw::Projection,
    start::{Piece as WorldPiece, StructureRegistry, StructureStart},
};
use super::terraforged::{clamp, java_max, java_pow, lerp, round};
use super::{
    super::terrain::{TerrainBlockStates, TerrainChunk, TerrainError},
    terraforged_terrain_data::TerrainData,
};
use crate::{heightmap::HeightmapType, registry::BlockStateId};
use horizon_core::{density::TerrainAdjustment, position::ChunkPos};
use std::sync::Arc;

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

/// [`Columns`] over a Horizon proto-chunk, as `modify` sees a 26.3 `ProtoChunk`: `ocean_floor` is
/// `ChunkAccess.getHeight(OCEAN_FLOOR_WG, x, z)` (first available minus one, `x & 15`, `z & 15`), `set` is
/// `ProtoChunk.setBlockState` ([`TerrainChunk::set_block_worldgen`]): writes outside the build range are dropped and
/// the write updates the heightmaps of the chunk's persisted status, priming any it lacks. Before TERRAIN (where
/// `fillFromNoise` runs) those are the two WG maps, so `ocean_floor` reflects earlier writes; from TERRAIN on they
/// are the four final maps and OCEAN_FLOOR_WG stays as it was, as in Java.
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
            .expect("modify_chunk primes OCEAN_FLOOR_WG")
            .highest_taken(x & 15, z & 15)
            .expect("masked chunk coordinates")
    }

    fn set(&mut self, x: i32, y: i32, z: i32, fill: Fill) -> Result<(), Self::Error> {
        let state = match fill {
            Fill::Stone => self.stone,
            Fill::Air => self.air,
        };
        self.chunk.set_block_worldgen(x & 15, y, z & 15, state)?;
        Ok(())
    }
}

/// TerraForged 6dd607eb's `worldgen.terrain.StructureTerrain`.
pub struct StructureTerrain {
    pieces: Vec<Piece>,
}

impl StructureTerrain {
    /// The constructor `StructureTerrain(chunk, structureManager)`: from `startsForStructure(chunkPos, TERRAIN_MODIFIED)`
    /// (pass the starts referencing `chunk` in the order 26.3 returns them, e.g. the generator's `references`; across
    /// structures that is a `HashMap`'s identity-hash order, not fixed in Java), each BEARD_BOX or
    /// BEARD_THIN start's pieces that are `isCloseToChunk(chunkPos, 20)` (box within 20 blocks of the chunk, Java `int`
    /// wrap), except pool pieces whose element is not RIGID; then [`from_rigid_pieces`](Self::from_rigid_pieces)'s
    /// stable `minY` sort. Starts of other structures, and starts without pieces (`isValid`), add nothing.
    pub fn from_starts(
        registry: &StructureRegistry,
        starts: &[Arc<StructureStart>],
        chunk: ChunkPos,
    ) -> Self {
        let origin_x = chunk.x.wrapping_shl(4);
        let origin_z = chunk.z.wrapping_shl(4);
        let mut pieces = Vec::new();
        for start in starts {
            if !matches!(
                registry.structures[start.structure].terrain,
                TerrainAdjustment::BeardBox | TerrainAdjustment::BeardThin
            ) {
                continue;
            }
            for piece in &start.pieces {
                let bounds = piece.bounding_box();
                if bounds.max[0] < origin_x.wrapping_sub(20)
                    || bounds.min[0] > origin_x.wrapping_add(35)
                    || bounds.max[2] < origin_z.wrapping_sub(20)
                    || bounds.min[2] > origin_z.wrapping_add(35)
                {
                    continue;
                }
                let ground_delta = match piece {
                    WorldPiece::Pool(pool) => {
                        if pool.element.projection() != Projection::Rigid {
                            continue;
                        }
                        pool.ground_level_delta
                    }
                    WorldPiece::Legacy(_) => 0,
                };
                pieces.push(Piece {
                    bounds: Bounds {
                        min_x: bounds.min[0],
                        min_y: bounds.min[1],
                        min_z: bounds.min[2],
                        max_x: bounds.max[0],
                        max_y: bounds.max[1],
                        max_z: bounds.max[2],
                    },
                    ground_delta,
                });
            }
        }
        Self::from_rigid_pieces(pieces)
    }

    /// The constructor's `rigids.sort(PIECE_SORTER)`: pieces by box `minY`, stable (fastutil's sort), from already
    /// collected pieces ([`from_starts`](Self::from_starts) collects them from structure starts).
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

    /// `ChunkUtil.buildStructureTerrain(chunk, terrainData, ..)` after the constructor's piece collection: `modify`
    /// on every column (dz, then dx, from the chunk's minimum block, Java `int` math), each returned height written
    /// into `terrain`'s height map ([`TerrainData::set_height_at`]: `Index.CHUNK`, the `int` widened to float; the
    /// extrema stay as `accept` left them, as in Java). `stone` and `air` are `Blocks.STONE`/`Blocks.AIR`'s default
    /// states. A missing OCEAN_FLOOR_WG is primed first, as the first `getHeight` primes it; nothing else is primed
    /// here (a write primes its status's maps). Fails only where a write does (an unknown state id).
    pub fn modify_chunk<B: TerrainBlockStates>(
        &self,
        chunk: &mut TerrainChunk<'_, B>,
        terrain: &mut TerrainData,
        stone: BlockStateId,
        air: BlockStateId,
    ) -> Result<(), TerrainError> {
        if chunk.heightmap(HeightmapType::OceanFloorWg).is_none() {
            chunk.prime_heightmaps(&[HeightmapType::OceanFloorWg])?;
        }
        let origin = chunk.position();
        let (min_x, min_z) = (origin.x.wrapping_shl(4), origin.z.wrapping_shl(4));
        let mut columns = ChunkColumns { chunk, stone, air };
        for dz in 0..16 {
            for dx in 0..16 {
                let (x, z) = (min_x.wrapping_add(dx), min_z.wrapping_add(dz));
                if let Some(height) = self.modify(x, z, &mut columns)? {
                    terrain.set_height_at(x, z, height);
                }
            }
        }
        Ok(())
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

    /// `SelectionOracle.java`'s output (`.local/terraforged/oracles/structure_terrain/selection/`, `run.sh`): scenario,
    /// the starts 26.3's `startsForStructure` visited (`structure.start`, `-` for none), the number of pieces the
    /// constructor collected, and a fold of their boxes and ground deltas in `rigids` order.
    const JAVA_SELECTION: &str = "\
0 2.0,3.0,1.0,1.1 2 0x5b40718030a9fe83
1 2.0,2.1,2.2 4 0xe036ea2a969e238b
2 1.0,0.1,0.0,0.2,2.1,2.0 8 0x7c8eb83252d09119
3 1.2,1.1,3.2,3.0,3.1 6 0x8678655245a4a71c
4 2.1,2.2,2.0,0.0 5 0x5efaf8bb1d46ec1d
5 0.0 0 0xcbf29ce484222325
6 0.0,0.2,0.1 2 0xb73386496b754fcb
7 0.2,0.1 2 0x13b7bd0c2e6a5ace
8 - 0 0xcbf29ce484222325
9 - 0 0xcbf29ce484222325
10 - 0 0xcbf29ce484222325
11 0.2,0.0 3 0x8786955738dd127d
12 0.0,0.1,0.2 0 0xcbf29ce484222325
13 0.1 2 0x3dad39e3caaad952
14 0.0,0.1,1.2,1.0,1.1 0 0xcbf29ce484222325
15 0.1,0.0,0.2 0 0xcbf29ce484222325
16 - 0 0xcbf29ce484222325
17 - 0 0xcbf29ce484222325
18 0.1,0.0 2 0x72d8301efd6506c3
19 - 0 0xcbf29ce484222325
20 0.0,1.0 1 0xfa33b6cc8429e456
21 2.1,2.0,2.2,0.1,0.0 5 0x966c6a3000652b40
22 2.0,0.0,0.1 4 0xeafc27f51c69777f
23 2.2,2.1,0.0,1.2,1.0,1.1 6 0x4d771ac8e057c453
24 0.0 0 0xcbf29ce484222325
25 1.0,2.0 2 0x4e5e0a471df7d716
26 2.1,2.0,1.0 4 0xa4c08f63aea28e41
27 0.0 1 0x6d84319d59207248
28 - 0 0xcbf29ce484222325
29 0.0,0.2,1.0,1.2,1.1 4 0x337b489f9f97a0e1
30 1.1,0.0 0 0xcbf29ce484222325
31 3.0,0.1,2.0,2.1 0 0xcbf29ce484222325
32 3.1,3.0,1.1,1.0,2.1,2.0,0.0 7 0x7a0679b3223ac823
33 2.0,2.1,3.0,3.1 5 0x788de08fb368a3c6
34 1.1,1.0,1.2,3.0 10 0x44be1f95cae8d420
35 2.0 1 0x49567a9884e832b6
36 0.0,0.1,2.2,2.0,2.1,3.0 6 0xc5439034a1203eed
37 - 0 0xcbf29ce484222325
38 0.0 0 0xcbf29ce484222325
39 0.1,0.0 3 0xa94b77c612e3795a
40 0.1,0.0 2 0xe0c15fa57fc96bc7
41 - 0 0xcbf29ce484222325
42 - 0 0xcbf29ce484222325
43 0.0 0 0xcbf29ce484222325
44 - 0 0xcbf29ce484222325
45 0.0,0.1 3 0xf4eaef5d6f5eb19c
46 0.0 0 0xcbf29ce484222325
47 0.0 0 0xcbf29ce484222325
48 0.2,0.0,0.1 2 0xe540eb187a3377f3
49 - 0 0xcbf29ce484222325
50 - 0 0xcbf29ce484222325
51 1.0,1.2,1.1 8 0x40a735b437344c39
52 0.1,0.0,0.2,1.1,1.0 1 0x26aad90dde8b8c2b
53 - 0 0xcbf29ce484222325
54 0.0,2.1,2.0,1.0,1.1 4 0x1ea5ab4394853c81
55 2.2,2.0,0.0 0 0xcbf29ce484222325
56 2.0,2.1,1.0 5 0x51787d8e00a2c87d
57 2.1,2.2,2.0,0.0,0.2,0.1,1.0 11 0x434010a9b31fa751
58 1.0,2.0,2.2,2.1 5 0xe1aba4acb41bae03
59 2.0 0 0xcbf29ce484222325
60 1.0,1.1 3 0xae22e9ca748ace71
61 0.0 0 0xcbf29ce484222325
62 1.0,1.1,0.2,0.1,3.0,3.2,3.1 0 0xcbf29ce484222325
63 0.0,3.1,3.0 0 0xcbf29ce484222325
64 3.2,3.0,3.1,1.0,0.0 8 0x83cc2b6d6286313f
65 1.1,1.0,3.0,0.0 1 0x420de108a6c15c12
66 1.0,1.1,3.0,3.1 6 0xaf1060dd3d7f8cac
67 1.0,3.0 2 0xd932db799117062d
68 1.0,2.1,2.2 2 0x090473d3c8844b42
69 1.2,1.1,1.0,2.2,2.1,2.0,3.1,3.0 5 0x03367a0d3c355774
70 0.0,0.2,0.1 3 0xc6a443ab05f1f284
71 0.1,0.2,0.0 4 0xe1f7398e99068e34
72 0.2,0.0,0.1 3 0x91e6abbac7c183f8
73 0.1,0.0 0 0xcbf29ce484222325
74 - 0 0xcbf29ce484222325
75 - 0 0xcbf29ce484222325
76 - 0 0xcbf29ce484222325
77 0.0,0.1 2 0xf53fd64a5453eeae
78 1.1,0.1,0.0 0 0xcbf29ce484222325
79 1.1,1.0,0.0 0 0xcbf29ce484222325
80 1.1,1.0 3 0xd33fd1d290928bd0
81 0.0 0 0xcbf29ce484222325
82 0.0,0.1 5 0xcaefe762ed4dc563
83 - 0 0xcbf29ce484222325
84 1.0 1 0x93c6f4ee596c4b44
85 1.0 0 0xcbf29ce484222325
86 2.0,0.2,0.1,1.0 6 0xe714a64452b28b44
87 0.0,2.0 6 0xafa22aa5356cc594
88 0.0,2.2,2.0,2.1 4 0x89fa3c69c4ac14cd
89 0.2,0.1,0.0,2.2,2.1,2.0 5 0xd4f67ea70890229d
90 1.0,1.1,0.0,0.1,0.2 8 0xe796825d98dbdd7f
91 2.1,2.2,1.1,1.0 7 0xc2877313db0a5008
92 2.0,1.0,1.1 3 0x159364965cd5bc0e
93 0.0,0.1,0.2,1.0,2.1,2.0 5 0x62b961725c58c299
94 1.0,1.2,1.1,2.0,2.1,2.2,3.1 0 0xcbf29ce484222325
95 3.0,0.0,0.1,2.0,2.2,2.1 0 0xcbf29ce484222325
96 0.1,0.0,1.1,1.0,3.0 2 0xe46a1571f0815987
97 1.0,0.0,2.0,2.1,2.2,3.1 6 0xe2766470ca5df613
98 0.0,3.1,3.2,3.0,2.1,2.2,2.0,1.0 4 0xe3068bf7657845b4
99 3.0 3 0x4b51c8f168371331
100 3.1,3.2,3.0,1.1,1.2,1.0 7 0xcd1937c0ece5ed79
101 1.0,3.1,2.0,2.1 4 0x1d4ccef5beae03fa
102 2.2,2.0,1.0,0.0 5 0xc868f814a96fdbd0
103 0.0 0 0xcbf29ce484222325
104 0.0,0.2,0.1 5 0x02d191951cabd43f
105 - 0 0xcbf29ce484222325
106 0.0 0 0xcbf29ce484222325
107 - 0 0xcbf29ce484222325
108 - 0 0xcbf29ce484222325
109 0.0,0.1,0.2 3 0xf2243ea791aaeba2
110 - 0 0xcbf29ce484222325
111 1.1,1.2,1.0 0 0xcbf29ce484222325
112 0.0,0.2,0.1,1.0 8 0x7701080d8a40ab3a
113 0.0,1.0 5 0x8abdec5882414f9c
114 0.1,0.0 2 0xae71eb22f2fc0396
115 0.0,0.1 3 0x309535d68e228af1
116 - 0 0xcbf29ce484222325
117 1.0,1.1 2 0x98b6c0da7fbe55c0
118 0.0,0.1 2 0x1528577fb4a356f8
119 2.0,2.1 2 0x03af53962cb9e9ad
120 0.0,1.1,1.0,1.2 1 0x339882a076cd216d
121 2.0,0.0 2 0x7ec5dd0586874cb3
122 1.1,1.0,0.1,0.0 4 0x4e6c42acfd40f3d4
123 0.1 0 0xcbf29ce484222325
124 1.0 0 0xcbf29ce484222325
125 - 0 0xcbf29ce484222325
126 2.0 0 0xcbf29ce484222325
127 3.2,3.1,3.0,0.0,0.1 0 0xcbf29ce484222325";

    /// One box axis as `SelectionOracle.axis` draws it.
    fn selection_axis(random: &mut Lcg, origin: i32) -> (i32, i32) {
        let (mode, span) = (random.next(5), random.next(24));
        let lo = match mode {
            0 => origin.wrapping_add(35).wrapping_add(random.next(2)),
            1 => origin.wrapping_add(-21 + random.next(2) - span),
            _ => origin.wrapping_add(random.next(70) - 40),
        };
        let hi = lo.wrapping_add(span);
        (lo, if hi < lo { i32::MAX } else { hi })
    }

    /// The scenarios of `SelectionOracle.java`, which runs 6dd607eb's constructor through a real 26.3
    /// `StructureManager`: chunk positions where `x << 4` and the 20-block margin wrap, 1-4 structures of every
    /// `TerrainAdjustment`, 1-3 starts each (some without pieces, which `isValid` drops), legacy pieces and pool pieces
    /// (the empty element, rigid and terrain-matching elements), boxes on both edges of the margin, tied `minY`s.
    /// 26.3 visits the structures in a `HashMap`'s identity-hash order, so Java's visit order is an input here; the
    /// starts Java never visits (other adjustments, empty starts) are interleaved to show they add nothing.
    #[test]
    fn from_starts_matches_6dd607eb_on_26_3_structure_starts() {
        use super::super::super::structure::{
            BoundingBox, Rotation,
            jigsaw::{PoolElement, PoolPiece},
            legacy::{LegacyPiece, PieceContext, Result as PieceResult},
            start::{StructureDef, StructureKind},
        };
        use std::collections::BTreeMap;

        #[derive(Debug)]
        struct Legacy(BoundingBox);
        impl LegacyPiece for Legacy {
            fn bounding_box(&self) -> BoundingBox {
                self.0
            }
            fn type_name(&self) -> &'static str {
                "test"
            }
            fn post_process(&self, _: &mut PieceContext<'_>) -> PieceResult<()> {
                Ok(())
            }
        }
        // TerrainAdjustment.values() order.
        const ADJUSTMENTS: [TerrainAdjustment; 5] = [
            TerrainAdjustment::None,
            TerrainAdjustment::Bury,
            TerrainAdjustment::BeardThin,
            TerrainAdjustment::BeardBox,
            TerrainAdjustment::Encapsulate,
        ];

        let mut scenarios = 0;
        for line in JAVA_SELECTION.lines() {
            let fields: Vec<&str> = line.split(' ').collect();
            let s: u32 = fields[0].parse().unwrap();
            let count: usize = fields[2].parse().unwrap();
            let hash = u64::from_str_radix(fields[3].trim_start_matches("0x"), 16).unwrap();
            let mut random = Lcg(u64::from(s)
                .wrapping_mul(0x9E3779B97F4A7C15)
                .wrapping_add(170));
            let cx = match s % 16 {
                13 => i32::MAX - random.next(2),
                14 => 0x0800_0000 + random.next(2),
                15 => 0x07ff_ffff - random.next(2),
                _ => random.next(9) - 5,
            };
            let cz = random.next(9) - 5;
            let (ox, oz) = (cx.wrapping_shl(4), cz.wrapping_shl(4));
            let mut registry = StructureRegistry::new(
                &BTreeMap::new(),
                &BTreeMap::new(),
                BTreeMap::new(),
                &|_| Ok(BTreeSet::new()),
            )
            .unwrap();
            let mut starts = BTreeMap::new();
            let mut valid_beards = BTreeSet::new();
            for t in 0..1 + random.next(4) {
                let a = random.next(8);
                let terrain = match a {
                    0..5 => ADJUSTMENTS[a as usize],
                    5 => TerrainAdjustment::BeardThin,
                    _ => TerrainAdjustment::BeardBox,
                };
                let structure = registry.structures.len();
                registry.structures.push(StructureDef {
                    name: format!("test_{t}"),
                    kind: StructureKind::Unsupported("test".into()),
                    biomes: BTreeSet::new(),
                    step: 0,
                    terrain,
                    enhanced_terrain: None,
                });
                for k in 0..1 + random.next(3) {
                    let (rx, rz) = (cx.wrapping_add(k - 1), cz + random.next(3) - 1);
                    let mut pieces = Vec::new();
                    for _ in 0..random.next(8) {
                        let kind = random.next(4);
                        let (x, z) = (
                            selection_axis(&mut random, ox),
                            selection_axis(&mut random, oz),
                        );
                        let y0 = 40 + random.next(6) * 4;
                        let y1 = y0 + random.next(10);
                        let bounding_box = BoundingBox::new([x.0, y0, z.0], [x.1, y1, z.1]);
                        let ground_level_delta = random.next(9) - 4;
                        let element = match kind {
                            0 => {
                                pieces.push(WorldPiece::Legacy(Box::new(Legacy(bounding_box))));
                                continue;
                            }
                            1 => PoolElement::Empty,
                            _ => PoolElement::Feature {
                                feature: "minecraft:test".into(),
                                projection: if random.next(2) == 0 {
                                    Projection::Rigid
                                } else {
                                    Projection::TerrainMatching
                                },
                            },
                        };
                        pieces.push(WorldPiece::Pool(PoolPiece {
                            element: Arc::new(element),
                            position: [0; 3],
                            ground_level_delta,
                            rotation: Rotation::None,
                            bounding_box,
                            junctions: Vec::new(),
                            waterlogging: true,
                        }));
                    }
                    let name = format!("{t}.{k}");
                    if matches!(
                        terrain,
                        TerrainAdjustment::BeardBox | TerrainAdjustment::BeardThin
                    ) && !pieces.is_empty()
                    {
                        valid_beards.insert(name.clone());
                    }
                    let bounding_box = pieces
                        .first()
                        .map_or(BoundingBox::new([0; 3], [0; 3]), |piece| {
                            piece.bounding_box()
                        });
                    starts.insert(
                        name.clone(),
                        Arc::new(StructureStart {
                            structure,
                            name,
                            chunk: ChunkPos::new(rx, rz),
                            pieces,
                            bounding_box,
                        }),
                    );
                }
            }
            let visited: Vec<&str> = match fields[1] {
                "-" => Vec::new(),
                list => list.split(',').collect(),
            };
            assert_eq!(
                visited
                    .iter()
                    .map(|name| name.to_string())
                    .collect::<BTreeSet<_>>(),
                valid_beards,
                "scenario {s}: the mirrored scenario differs from Java's"
            );
            let mut others = starts
                .iter()
                .filter(|(name, _)| !visited.contains(&name.as_str()))
                .map(|(_, start)| start.clone());
            let mut input = Vec::new();
            for name in &visited {
                input.extend(others.next());
                input.push(starts[*name].clone());
            }
            input.extend(others);
            let selected = StructureTerrain::from_starts(&registry, &input, ChunkPos::new(cx, cz));
            let folded = selected
                .pieces
                .iter()
                .fold(0xcbf29ce484222325, |hash, piece| {
                    let b = piece.bounds;
                    [
                        b.min_x,
                        b.min_y,
                        b.min_z,
                        b.max_x,
                        b.max_y,
                        b.max_z,
                        piece.ground_delta,
                    ]
                    .into_iter()
                    .fold(hash, fold)
                });
            assert_eq!(
                (selected.pieces.len(), folded),
                (count, hash),
                "scenario {s}"
            );
            scenarios += 1;
        }
        assert_eq!(scenarios, 128);
    }

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

    /// `oracles/structure_terrain/chunk/` (`run.sh`): 6dd607eb's `StructureTerrain.java` (constructor given the rigid
    /// pieces, 26.3's int `setBlockState` flags; `patch.py` prints the diff) over REAL 26.3 `ProtoChunk`s and
    /// `Heightmap`s, then `buildStructureTerrain`'s loop. Per scenario: FNV-1a folds of every block (z, x, y), of the
    /// six heightmaps (absent = -1, else 256 first-available heights) and of the 256 height-map floats, plus how
    /// many of those floats are not +0.0 (their zero start). Interpreted and JIT runs agree.
    #[rustfmt::skip]
    const JAVA_CHUNKS: [(u32, u64, u64, u32, u64); 96] = [
        (0, 0xc205c385c60f587f, 0xab1fd3fb787cdd39, 256, 0x2573aaa7c331b725),
        (1, 0x1a821c3e96f81225, 0x0d4ea4864aa734f1, 242, 0x5c71591e4c6db725),
        (2, 0x9d7d0dcf14245dc5, 0x1ae1e32a6f6c69fd, 256, 0x6abd7d790fd5b725),
        (3, 0xe4c802944344ff4f, 0xe839ea2a6b0dd615, 226, 0x569bf07e88f5b725),
        (4, 0xb56c6fb0dcd0939f, 0x0b643a6bf2226189, 0, 0xd80ac658736bb725),
        (5, 0x64bf2fb991a9b5b5, 0x282714474e605391, 247, 0x0fbe78a478e5b725),
        (6, 0x44dd19b3ac6c7a3d, 0x916dae83caa5a09e, 0, 0xd80ac658736bb725),
        (7, 0x0422d71ba5b79e67, 0x14d9b8cb6500054d, 202, 0x6e1d3809f445b725),
        (8, 0x9e32dbf34c955925, 0x936cf02f1af10129, 148, 0xc580638fe0e9b725),
        (9, 0x1f5840586646d725, 0x01a0f9260fcfdb9d, 204, 0x9e094d388fb9b725),
        (10, 0x3539b45b7eee3a35, 0xe1e7b8a072950169, 30, 0xfb9b1d7cb3e7b725),
        (11, 0x70b591f3a8ef64cd, 0x248291d1c2d54fa1, 134, 0x395fac3cd303b725),
        (12, 0x01b442c6e67da235, 0x61906eff1eab8f09, 96, 0x3c63f12f81ebb725),
        (13, 0x5da1f0edae092235, 0x651a765bdcb79eaa, 57, 0xe58eec540d57b725),
        (14, 0xc97940d64b79b217, 0x5a1a8a64f5d0cc69, 112, 0xc11d02705fac3725),
        (15, 0xbf8c78ec5194bcd5, 0x873456af0f19a621, 245, 0x33b3290dde37b725),
        (16, 0xeae68e7a4f75c1c7, 0x7332c9bb384634a5, 0, 0xd80ac658736bb725),
        (17, 0x574593c777fa0de5, 0xc10b7092e8b6bfe4, 20, 0xe149b06d2ae5b725),
        (18, 0x6a8f66ae6ed385e4, 0x89a4afb9e0d67045, 0, 0xd80ac658736bb725),
        (19, 0x765dc064e1317071, 0x78ab1ebe1ba45b72, 0, 0xd80ac658736bb725),
        (20, 0xc2d68cd45a04b4e8, 0x0369aaf1af890fc7, 115, 0xe29e0ed6c3b9b725),
        (21, 0x5c67e085af7ffe71, 0xca4b9fdcab4b18a6, 251, 0x5dafb5151747b725),
        (22, 0x3d4b5ca52883fa02, 0x35fb46edbfa23b04, 29, 0xab99fe7ffb043725),
        (23, 0x6053ac5b2a9eec78, 0x30e1cd5d857c5865, 206, 0x05c9a4d732d3b725),
        (24, 0xa729d7647c991b03, 0x584a78ae1b4997f5, 246, 0x8e0eb6df7693b725),
        (25, 0xade848d0695c2c6f, 0xf70c3f9e89be48a6, 77, 0xacf2fc5b4a1fb725),
        (26, 0x268bb02e36b3fcd4, 0x86dc180f2258bcb2, 215, 0x0bd791241861b725),
        (27, 0x59c5a246fb6362e5, 0x48708a579d47f888, 256, 0xbcdd03888d93b725),
        (28, 0xe2c323e48d67b7c7, 0xdeccdc49273390b9, 240, 0xf1b3dea798b1b725),
        (29, 0x3e348ee6e66e2325, 0xda1810a11f69cd51, 256, 0x4ff4932c38ebb725),
        (30, 0xd8a579734d4c486f, 0x56876c7acffc5b51, 5, 0xce5e5e5327f33725),
        (31, 0x735cd1b602e9cf25, 0xa3c8df2da396d9a0, 0, 0xd80ac658736bb725),
        (32, 0xc88ad9a53dd25dda, 0xc9b92aa372e2de01, 239, 0x759b010e7ec5b725),
        (33, 0x7b84d25d0910455b, 0xcc87d87a73e9881f, 210, 0xc441a2bcf6d9b725),
        (34, 0x4793d9df21ab28e3, 0xec55d9fc7c742232, 0, 0xd80ac658736bb725),
        (35, 0x3c8c839f8115b663, 0xbc15f9d69608b6b5, 240, 0x26ef0267f65bb725),
        (36, 0x764c90507de726d9, 0x7568177d7f56b83b, 152, 0x05ce73b6c029b725),
        (37, 0x5759fc5155e61c0b, 0xa6b445e4141c3591, 0, 0xd80ac658736bb725),
        (38, 0xd4c6e41c8baaf06d, 0xcbc9f2d2792a82dd, 136, 0x3036f1071e263725),
        (39, 0x48c4914104c25127, 0x313a1e087debdc65, 26, 0x25f7a6c0275fb725),
        (40, 0x7bc43cb2f97ab565, 0xc2f6840b888bd01d, 200, 0x013385377c11b725),
        (41, 0x77a12fafd8ca5e5d, 0x472dfc958dc60551, 74, 0xa783f338ef15b725),
        (42, 0x710452e095fbf7d5, 0x2347784525b5dbd5, 121, 0xdb06752c1839b725),
        (43, 0xcbdeb7de035b631d, 0xe981978d8f73a431, 208, 0xf6d3c75d225bb725),
        (44, 0xf5028a51c5f15f8d, 0x57424f5dce599899, 160, 0x6da7a6cab45db725),
        (45, 0xc79f098878f58d2f, 0x06dc1dbe265f1049, 193, 0xba93b3017c4bb725),
        (46, 0x063def343f5df255, 0x355e232e5f47daee, 212, 0x9a2afd868b0db725),
        (47, 0xc7e9ceb199a71007, 0xb3b92eb60674efbd, 64, 0xf9e5447acd8bb725),
        (48, 0xce5f282ae05bbfe7, 0x819518bc20a1cf57, 232, 0x02a783cc99e3b725),
        (49, 0xbf65fba2fa044da7, 0xbcd7b27fac024401, 256, 0xa5942101f4f7b725),
        (50, 0xca4777668dd6458f, 0x7197c5bf85ed8351, 0, 0xd80ac658736bb725),
        (51, 0x5f92675550e0fd55, 0x4c20302214de65e5, 144, 0x65dc650bae5db725),
        (52, 0x1ad0255229e99d4d, 0xd282743a89b47b4e, 0, 0xd80ac658736bb725),
        (53, 0xed21349e406240d7, 0xd544fae9436947c1, 0, 0xd80ac658736bb725),
        (54, 0xcfbed9379223965d, 0x15617ef5bb4ae069, 0, 0xd80ac658736bb725),
        (55, 0x7251de468e0e6c37, 0x9983602625db5e31, 144, 0x510b02232f1db725),
        (56, 0x4a8413d08f1ab3bd, 0xefe169a49f69c2ba, 33, 0x35b9a2d9fa7fb725),
        (57, 0x867fe49e3c8a70ef, 0x81b55704d013017d, 132, 0xcabf2bc4403db725),
        (58, 0x5c1108a4034d15de, 0x4c70c15c56afa2b9, 172, 0x2ac3cc8f3d1bb725),
        (59, 0x74af49154e8ec6e0, 0x3b9d2a6c4e970719, 0, 0xd80ac658736bb725),
        (60, 0x79a4d9b30e036a94, 0x58c939c2ed06cc09, 0, 0xd80ac658736bb725),
        (61, 0xb3ece7e7940a897f, 0x95d6e72b5ad346a5, 64, 0xea287ca3fc63b725),
        (62, 0x2b231a7ff8105d75, 0x61681cedf79eb229, 0, 0xd80ac658736bb725),
        (63, 0x9de59421a9c29f4c, 0x6967665c83482529, 0, 0xd80ac658736bb725),
        (64, 0x6de10783c18d80c5, 0xc2de2f169693a867, 0, 0xd80ac658736bb725),
        (65, 0x7a63ab203957a3e4, 0xca6029432970e7c4, 0, 0xd80ac658736bb725),
        (66, 0x7014201c5f91b768, 0xfd914aadeea72a12, 256, 0x26e5584cdd79b725),
        (67, 0xc1c816623f592bae, 0xd3ba782a2c35c25a, 213, 0x97ee8f06ea45b725),
        (68, 0xcda1356f48d9b150, 0x6d74c236f7a71704, 236, 0xfa770f020c63b725),
        (69, 0xec42375daefba508, 0x81c5b8758d12330c, 215, 0x9143ff032723b725),
        (70, 0x4a4a1310a384d86c, 0x33c9dce3ff3e399c, 232, 0x1ebb8eb5f651b725),
        (71, 0xc8808cf50278a047, 0xe3f89c14b7a3612d, 255, 0x682ff4b1468db725),
        (72, 0x59e5a9e173607537, 0x4114e5a32ab8aa07, 66, 0x4ad7d5ff8ae9b725),
        (73, 0x82cb134b96f8d7e9, 0x8b432937a335e6bb, 144, 0x2769409abd75b725),
        (74, 0xcbc940b5850066ca, 0xced0e486f8a89062, 230, 0x23785267b74fb725),
        (75, 0x59bf071fc20861b6, 0x87534330040dc97c, 17, 0x009439f078c7b725),
        (76, 0x14921732cd198b66, 0x69309fe3878f07fe, 145, 0x090b4a24d359b725),
        (77, 0xfdd6e41d8105ebff, 0xb8991c39cec1633d, 0, 0xd80ac658736bb725),
        (78, 0xa9f256bc7eb7540f, 0x47b5e507cda86a4d, 7, 0x77916b6a7999b725),
        (79, 0x59ff43bb987118cf, 0x71f00382f7659de5, 41, 0x372ca43c0ab5b725),
        (80, 0x15cac46199025eb7, 0x6db209aaf28792b9, 44, 0x190cb1adeb95b725),
        (81, 0x9bbfbac51f630e9f, 0xab95d79528a0a4ac, 110, 0xf6a62c16ae1db725),
        (82, 0x25a4233609a0675f, 0x0e9f07c655363861, 35, 0x58b222d9d0a1b725),
        (83, 0x81c4dbe3f47d86f5, 0x447e7f5ed972e221, 0, 0xd80ac658736bb725),
        (84, 0x0c9c95ab95e70345, 0x4495b447b121a2d1, 256, 0x51ab0bd3b937b725),
        (85, 0x251db85ace453cd5, 0x3948568cbb26b3d8, 0, 0xd80ac658736bb725),
        (86, 0xd508f844e6b7250f, 0x56876c7acffc5b51, 127, 0x7ef0a26fef363725),
        (87, 0xc602a630075e575f, 0xe948f4ab89b79b01, 0, 0xd80ac658736bb725),
        (88, 0xa02cd281b68056d7, 0xb28598c2ff44f70d, 25, 0xe25bf4e6c7adb725),
        (89, 0xcae9852d65882485, 0xa64e13288d54b720, 251, 0xca6c5cdaa41bb725),
        (90, 0x9de734d18f4f3f8f, 0x906cc04bd0ebfb49, 245, 0x3ae46d92f019b725),
        (91, 0xfa5c7f6ae6090a07, 0xc77b0a1c52dfc70d, 115, 0xf74d1bdeb649b725),
        (92, 0xc8e16a54833a90a5, 0x1f93933c5cd087e5, 256, 0x101ea3e350f7b725),
        (93, 0x760b4916a8bb0e7f, 0x0ceb01e7f226d335, 0, 0xd80ac658736bb725),
        (94, 0x4c8b5fe20e3d7ddd, 0x22af460bf8d74c79, 181, 0x6c588a04c3203725),
        (95, 0x0b2d29ce31a3880d, 0xc554ae439387d571, 205, 0x8cad5bb6993fb725),
    ];

    /// The scenarios of `ChunkAdapterOracle.java`: persisted status (mostly BIOMES, where `fillFromNoise` runs; also
    /// EMPTY, TERRAIN and FEATURES), WG maps primed first or not, stone columns and water written straight into the
    /// sections (surfaces near both build limits), up to 7 rigid pieces (pool pieces with ground deltas).
    #[test]
    fn modify_chunk_matches_6dd607eb_on_26_3_proto_chunks() {
        use super::super::{
            super::terrain::PreLightChunkStage,
            terraforged_chunk_fill::tests::{AIR, Properties, STONE, WATER, registry},
            terraforged_terrain_levels::TerrainLevels,
        };
        use horizon_core::position::ChunkPos;
        const STAGES: [PreLightChunkStage; 4] = [
            PreLightChunkStage::Biomes,
            PreLightChunkStage::Empty,
            PreLightChunkStage::Terrain,
            PreLightChunkStage::Features,
        ];
        const MAPS: [HeightmapType; 6] = [
            HeightmapType::WorldSurfaceWg,
            HeightmapType::OceanFloorWg,
            HeightmapType::WorldSurface,
            HeightmapType::OceanFloor,
            HeightmapType::MotionBlocking,
            HeightmapType::MotionBlockingNoLeaves,
        ];
        let registry = registry();
        for (s, blocks, maps, sets, heights) in JAVA_CHUNKS {
            let mut random = Lcg(u64::from(s)
                .wrapping_mul(0x9E3779B97F4A7C15)
                .wrapping_add(168));
            let (cx, cz) = (random.next(9) - 5, random.next(9) - 5);
            let k = random.next(8);
            let stage = STAGES[if k < 5 { 0 } else { k as usize - 4 }];
            let prime = random.next(2) == 0;
            let (min_y, height) = (-64, 384);
            let base = match s % 8 {
                6 => 300 + random.next(30),
                7 => -70 + random.next(10),
                _ => 40 + random.next(60),
            };
            let sea = if random.next(2) == 0 {
                base + random.next(12)
            } else {
                i32::MIN
            };
            let mut chunk =
                TerrainChunk::new(ChunkPos::new(cx, cz), min_y, height, &registry, Properties)
                    .unwrap();
            chunk.set_generation_stage(stage);
            for z in 0..16 {
                for x in 0..16 {
                    let top = base + random.next(13) - 6;
                    for y in min_y..min_y + height {
                        if y <= top {
                            chunk.set_block_raw(x, y, z, STONE).unwrap();
                        } else if y <= sea {
                            chunk.set_block_raw(x, y, z, WATER).unwrap();
                        }
                    }
                }
            }
            if prime {
                chunk
                    .prime_heightmaps(&[HeightmapType::WorldSurfaceWg, HeightmapType::OceanFloorWg])
                    .unwrap();
            }
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
            let mut terrain = TerrainData::new(TerrainLevels::default());
            let extrema = (terrain.min(), terrain.max(), terrain.max_base());
            StructureTerrain::from_rigid_pieces(pieces)
                .modify_chunk(&mut chunk, &mut terrain, STONE, AIR)
                .unwrap();
            let mut block_hash = 0xcbf29ce484222325;
            for z in 0..16 {
                for x in 0..16 {
                    for y in min_y..min_y + height {
                        block_hash = fold(block_hash, chunk.block(x, y, z).0 as i32);
                    }
                }
            }
            let mut map_hash = 0xcbf29ce484222325;
            for kind in MAPS {
                let Some(map) = chunk.heightmap(kind) else {
                    map_hash = fold(map_hash, -1);
                    continue;
                };
                for z in 0..16 {
                    for x in 0..16 {
                        map_hash = fold(map_hash, map.first_available(x, z).unwrap());
                    }
                }
            }
            let cells = (0..256).map(|i| terrain.height().get_index(i).to_bits());
            let written = cells.clone().filter(|&bits| bits != 0).count() as u32;
            let height_hash = cells.fold(0xcbf29ce484222325, |hash, bits| fold(hash, bits as i32));
            assert_eq!(
                (block_hash, map_hash, written, height_hash),
                (blocks, maps, sets, heights),
                "scenario {s}"
            );
            assert_eq!((terrain.min(), terrain.max(), terrain.max_base()), extrema);
        }
    }
}
