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

use horizon_core::position::ChunkPos;
use std::collections::BTreeMap;

pub fn initialize(
    cache: &mut BTreeMap<i64, i32>,
    chunk: ChunkPos,
    mut height: impl FnMut(i32, i32) -> i32,
) {
    if !cache.is_empty() {
        return;
    }
    let start_x = chunk.x.wrapping_shl(4);
    let start_z = chunk.z.wrapping_shl(4);
    let mut min = i32::MAX;
    for dz in (0..16).step_by(4) {
        for dx in (0..16).step_by(4) {
            let value = height(dx, dz);
            cache.insert(
                column_key(start_x.wrapping_add(dx), start_z.wrapping_add(dz)),
                value,
            );
            min = min.min(value);
        }
    }
    for dz in (-16..32).step_by(4) {
        for dx in (-16..32).step_by(4) {
            if dx & 15 == dx && dz & 15 == dz {
                continue;
            }
            cache.insert(
                column_key(start_x.wrapping_add(dx), start_z.wrapping_add(dz)),
                min,
            );
        }
    }
}

pub fn column_key(x: i32, z: i32) -> i64 {
    i64::from(x as u32) | (i64::from(z) << 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_entries_match_original_java_initializer() {
        for (x, z, expected) in [
            (0, 0, 0x69e1e437c4071d25),
            (-1, 2, 0x9669975a0fe0b7a5),
            (i32::MAX, i32::MIN, 0xec5e939a0fe0b7a5),
        ] {
            let mut cache = BTreeMap::new();
            let mut calls = Vec::new();
            initialize(&mut cache, ChunkPos::new(x, z), |dx, dz| {
                calls.push((dx, dz));
                70 + dx - dz * 2
            });
            assert_eq!(cache.len(), 144);
            assert_eq!(
                calls,
                (0..16)
                    .step_by(4)
                    .flat_map(|dz| (0..16).step_by(4).map(move |dx| (dx, dz)))
                    .collect::<Vec<_>>()
            );
            let hash = cache
                .iter()
                .fold(0xcbf29ce484222325u64, |hash, (&key, &height)| {
                    let hash = (hash ^ key as u64).wrapping_mul(0x100000001b3);
                    (hash ^ u64::from(height as u32)).wrapping_mul(0x100000001b3)
                });
            assert_eq!(hash, expected);
            assert_eq!(
                cache[&column_key(x.wrapping_shl(4).wrapping_sub(16), z.wrapping_shl(4))],
                46
            );
        }
    }

    #[test]
    fn preserves_existing_cache_without_requesting_terrain() {
        let mut cache = BTreeMap::from([(17, -31)]);
        initialize(&mut cache, ChunkPos::new(0, 0), |_, _| {
            panic!("must not join terrain")
        });
        assert_eq!(cache, BTreeMap::from([(17, -31)]));
    }
}
