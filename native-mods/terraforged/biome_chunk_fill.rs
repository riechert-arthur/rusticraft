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

use super::super::biome_chunk::{BiomeChunk, BiomeFillError};
use horizon_core::position::ChunkPos;

pub const CAVE_BIOME_MAX_Y: i32 = 0;

pub fn is_cave_biome_section(section_y: i32) -> bool {
    section_y.wrapping_shl(4).wrapping_add(15) <= CAVE_BIOME_MAX_Y
}

pub fn is_cave_biome_quart(quart_y: i32) -> bool {
    is_cave_biome_section(quart_y >> 2)
}

pub fn fill_noise_biomes<T: Clone, E>(
    chunk: &mut BiomeChunk<T>,
    position: ChunkPos,
    mut surface_biome: impl FnMut(i32, i32) -> Result<T, E>,
    mut cave_biome: impl FnMut(i32, i32) -> Result<Option<T>, E>,
) -> Result<(), BiomeFillError<E>> {
    let origin_x = position.x.wrapping_shl(2);
    let origin_z = position.z.wrapping_shl(2);
    let mut surface = Vec::with_capacity(16);
    let mut cave = Vec::with_capacity(16);
    let mut any_cave = false;
    for dz in 0..4 {
        for dx in 0..4 {
            let x = origin_x.wrapping_add(dx);
            let z = origin_z.wrapping_add(dz);
            let surface_value = surface_biome(x, z).map_err(BiomeFillError::Resolve)?;
            let cave_value = cave_biome(x, z).map_err(BiomeFillError::Resolve)?;
            any_cave |= cave_value.is_some();
            surface.push(surface_value.clone());
            cave.push(cave_value.unwrap_or(surface_value));
        }
    }

    let (min_quart, max_quart) = chunk.quart_range();
    chunk.fill(
        min_quart.wrapping_shl(2),
        max_quart.wrapping_shl(2).wrapping_add(3),
        |quart| {
            let dx = quart[0].wrapping_sub(origin_x) & 3;
            let dz = quart[2].wrapping_sub(origin_z) & 3;
            let index = dx.wrapping_add(dz.wrapping_mul(4)) as usize;
            let values = if any_cave && is_cave_biome_quart(quart[1]) {
                &cave
            } else {
                &surface
            };
            Ok(values[index].clone())
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, convert::Infallible, rc::Rc};

    #[test]
    fn cave_band_boundaries_match_current_terraforged_source() {
        assert!(is_cave_biome_section(-1));
        assert!(!is_cave_biome_section(0));
        assert!(is_cave_biome_quart(-1));
        assert!(!is_cave_biome_quart(0));
    }

    #[test]
    fn two_dimensional_layers_fill_whole_sections_in_java_order() {
        let position = ChunkPos::new(-2, 3);
        let mut chunk = BiomeChunk::new(position, -64, 96, String::new()).unwrap();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let surface_calls = Rc::clone(&calls);
        let cave_calls = Rc::clone(&calls);
        fill_noise_biomes(
            &mut chunk,
            position,
            move |x, z| {
                surface_calls.borrow_mut().push(("surface", x, z));
                Ok::<_, Infallible>(format!("surface:{x}:{z}"))
            },
            move |x, z| {
                cave_calls.borrow_mut().push(("cave", x, z));
                Ok::<_, Infallible>(((x + z) & 1 == 0).then(|| format!("cave:{x}:{z}")))
            },
        )
        .unwrap();

        let expected_calls = (0..4)
            .flat_map(|dz| {
                (0..4).flat_map(move |dx| {
                    let x = -8 + dx;
                    let z = 12 + dz;
                    [("surface", x, z), ("cave", x, z)]
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(*calls.borrow(), expected_calls);
        assert_eq!(chunk.noise_biome([-8, 0, 12]), "surface:-8:12");
        assert_eq!(chunk.noise_biome([-8, -1, 12]), "cave:-8:12");
        assert_eq!(chunk.noise_biome([-7, -1, 12]), "surface:-7:12");
        assert_eq!(chunk.noise_biome([-8, -4, 12]), "cave:-8:12");
    }

    #[test]
    fn absent_cave_layer_keeps_surface_biomes_everywhere() {
        let position = ChunkPos::new(0, 0);
        let mut chunk = BiomeChunk::new(position, -64, 80, 0).unwrap();
        fill_noise_biomes(
            &mut chunk,
            position,
            |x, z| Ok::<_, Infallible>(x * 10 + z),
            |_, _| Ok::<_, Infallible>(None),
        )
        .unwrap();
        assert_eq!(*chunk.noise_biome([3, -16, 2]), 32);
        assert_eq!(*chunk.noise_biome([3, 0, 2]), 32);
    }
}
