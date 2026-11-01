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

pub fn fill_noise_biomes<T: Clone, E>(
    chunk: &mut BiomeChunk<T>,
    position: ChunkPos,
    mut source: impl FnMut(i32, i32, i32) -> Result<T, E>,
) -> Result<(), BiomeFillError<E>> {
    let origin_x = position.x.wrapping_shl(4) >> 2;
    let origin_z = position.z.wrapping_shl(4) >> 2;
    let mut biomes = Vec::with_capacity(16);
    for dz in 0..4 {
        for dx in 0..4 {
            biomes.push(
                source(origin_x.wrapping_add(dx), -1, origin_z.wrapping_add(dz))
                    .map_err(BiomeFillError::Resolve)?,
            );
        }
    }
    let (min_quart, max_quart) = chunk.quart_range();
    chunk.fill(
        min_quart.wrapping_shl(2),
        max_quart.wrapping_shl(2).wrapping_add(3),
        |quart| {
            let dx = quart[0].wrapping_sub(origin_x) & 3;
            let dz = quart[2].wrapping_sub(origin_z) & 3;
            Ok(biomes[(dx + dz * 4) as usize].clone())
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;

    #[test]
    fn one_buffer_repeats_through_all_sections() {
        let position = ChunkPos::new(-2, 3);
        let mut chunk = BiomeChunk::new(position, -64, 96, String::new()).unwrap();
        let mut calls = Vec::new();
        fill_noise_biomes(&mut chunk, position, |x, y, z| {
            calls.push([x, y, z]);
            Ok::<_, Infallible>(format!("{x}:{y}:{z}"))
        })
        .unwrap();
        let expected = (0..4)
            .flat_map(|dz| (0..4).map(move |dx| [-8 + dx, -1, 12 + dz]))
            .collect::<Vec<_>>();
        assert_eq!(calls, expected);
        for y in -16..8 {
            for dz in 0..4 {
                for dx in 0..4 {
                    assert_eq!(
                        chunk.noise_biome([-8 + dx, y, 12 + dz]),
                        &format!("{}:-1:{}", -8 + dx, 12 + dz)
                    );
                }
            }
        }
    }

    #[test]
    fn source_failure_does_not_partially_fill_chunk() {
        let position = ChunkPos::new(0, 0);
        let mut chunk = BiomeChunk::new(position, -64, 80, 7).unwrap();
        let mut calls = 0;
        let result = fill_noise_biomes(&mut chunk, position, |_, y, _| {
            assert_eq!(y, -1);
            calls += 1;
            if calls == 3 {
                Err("source failed")
            } else {
                Ok(9)
            }
        });
        assert_eq!(result, Err(BiomeFillError::Resolve("source failed")));
        assert_eq!(calls, 3);
        for section in 0..5 {
            assert!(
                chunk
                    .section(section)
                    .unwrap()
                    .iter()
                    .all(|value| *value == 7)
            );
        }
    }
}
