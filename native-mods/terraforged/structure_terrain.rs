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

use super::terraforged::round;
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

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub min_x: i32,
    pub min_y: i32,
    pub min_z: i32,
    pub max_x: i32,
    pub max_y: i32,
    pub max_z: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct Piece {
    pub bounds: Bounds,
    pub ground_delta: i32,
}

impl Piece {
    pub fn floor(self) -> i32 {
        self.bounds.min_y.wrapping_add(self.ground_delta)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fill {
    Stone,
    Air,
}

pub trait Columns {
    type Error;
    fn ocean_floor(&self, x: i32, z: i32) -> i32;
    fn set(&mut self, x: i32, y: i32, z: i32, fill: Fill) -> Result<(), Self::Error>;
}

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

    pub fn from_rigid_pieces(mut pieces: Vec<Piece>) -> Self {
        pieces.sort_by_key(|piece| piece.bounds.min_y);
        Self { pieces }
    }

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

pub fn raise(x: i32, z: i32, bounds: Bounds, level: f32, surface: f32, radius: f32) -> f32 {
    let radius_squared = (radius * radius).max(1.0);
    let alpha = 1.0 - distance_alpha(x, z, bounds, radius_squared);
    let alpha = f64::from(alpha).powf(f64::from(2.0 - alpha)) as f32;
    surface + alpha * (level - surface)
}

pub fn ellipse_alpha(x: i32, z: i32, bounds: Bounds) -> f32 {
    let radius_x = bounds.max_x.wrapping_sub(bounds.min_x).wrapping_add(1) as f32 * 0.5;
    let radius_z = bounds.max_z.wrapping_sub(bounds.min_z).wrapping_add(1) as f32 * 0.5;
    let center_x = bounds.min_x.wrapping_add(bounds.max_x) as f32 * 0.5;
    let center_z = bounds.min_z.wrapping_add(bounds.max_z) as f32 * 0.5;
    let dx = x as f32 - center_x;
    let dz = z as f32 - center_z;
    (1.0 - dx * dx / (radius_x * radius_x) - dz * dz / (radius_z * radius_z)).clamp(0.0, 1.0)
}

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
