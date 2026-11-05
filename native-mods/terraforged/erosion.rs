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

/// TerraForged's `worldgen.noise.erosion.NoiseTileSize` (newer source 6dd607eb; the 1.18.2-0.3.1 jar's class has the
/// same bytecode): the square of chunks around a centre chunk that `ErodedNoiseGenerator` erodes together, `chunk_radius`
/// chunks on every side. Chunk offsets run from `chunk_min` (inclusive) to `chunk_max` (exclusive), block offsets from
/// `min` to `max`, both relative to the centre chunk's origin. All fields and indices are Java `int` math (wrapping),
/// so a negative or huge radius gives Java's wrapped values, not a panic. Unrelated to Engine's tile `Size`
/// (`terraforged_tile`), which sizes power-of-two tiles plus a border.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NoiseTileSize {
    /// `chunkLength` (`chunkDiameter` in `toString`): `1 + 2 * chunkRadius`.
    pub chunk_length: i32,
    /// `chunkSize`: `chunkLength²`, the number of chunks.
    pub chunk_size: i32,
    /// `chunkMin`: `-chunkRadius`.
    pub chunk_min: i32,
    /// `chunkMax`: `chunkMin + chunkLength`.
    pub chunk_max: i32,
    /// `regionLength` (`regionDiameter`): `chunkLength * 16` blocks.
    pub region_length: i32,
    /// `regionSize`: `regionLength²`, the heightmap's length.
    pub region_size: i32,
    /// `min`: `-16 * chunkRadius`.
    pub min: i32,
    /// `max`: `min + regionLength`.
    pub max: i32,
}

impl NoiseTileSize {
    /// `CHUNK_SIZE`: blocks per chunk side.
    pub const CHUNK_SIZE: i32 = 16;
    /// `DEFAULT`: radius 1 (3x3 chunks).
    pub const DEFAULT: Self = Self::new(1);

    /// `new NoiseTileSize(chunkRadius)`.
    pub const fn new(chunk_radius: i32) -> Self {
        let chunk_length = 1_i32.wrapping_add(chunk_radius.wrapping_mul(2));
        let chunk_size = chunk_length.wrapping_mul(chunk_length);
        let chunk_min = chunk_radius.wrapping_neg();
        let chunk_max = chunk_min.wrapping_add(chunk_length);
        let region_length = chunk_length.wrapping_mul(Self::CHUNK_SIZE);
        let region_size = region_length.wrapping_mul(region_length);
        // Java's `-CHUNK_SIZE * chunkRadius`; the same bits as negating the product.
        let min = Self::CHUNK_SIZE.wrapping_neg().wrapping_mul(chunk_radius);
        let max = min.wrapping_add(region_length);
        Self {
            chunk_length,
            chunk_size,
            chunk_min,
            chunk_max,
            region_length,
            region_size,
            min,
            max,
        }
    }

    /// `chunkIndexOf(x, z)`: `z * chunkLength + x` for chunk offsets counted from 0; not bounds-checked.
    pub const fn chunk_index_of(self, x: i32, z: i32) -> i32 {
        z.wrapping_mul(self.chunk_length).wrapping_add(x)
    }

    /// `chunkIndexOfRel(x, z)`: [`Self::chunk_index_of`] for chunk offsets relative to the centre chunk.
    pub const fn chunk_index_of_rel(self, x: i32, z: i32) -> i32 {
        self.chunk_index_of(
            x.wrapping_sub(self.chunk_min),
            z.wrapping_sub(self.chunk_min),
        )
    }

    /// `indexOf(x, z)`: `z * regionLength + x` for block offsets counted from 0; not bounds-checked.
    pub const fn index_of(self, x: i32, z: i32) -> i32 {
        z.wrapping_mul(self.region_length).wrapping_add(x)
    }

    /// `indexOfRel(x, z)`: [`Self::index_of`] for block offsets relative to the centre chunk's origin.
    pub const fn index_of_rel(self, x: i32, z: i32) -> i32 {
        self.index_of(x.wrapping_sub(self.min), z.wrapping_sub(self.min))
    }
}

/// Java's `toString` (`chunkDiameter`/`regionDiameter` name the length fields).
impl std::fmt::Display for NoiseTileSize {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "NoiseTileSize{{chunkDiameter={}, chunkSize={}, chunkMin={}, chunkMax={}, regionDiameter={}, regionSize={}, min={}, max={}}}",
            self.chunk_length,
            self.chunk_size,
            self.chunk_min,
            self.chunk_max,
            self.region_length,
            self.region_size,
            self.min,
            self.max
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write;

    const RADII: [i32; 16] = [
        0,
        1,
        2,
        3,
        -1,
        -2,
        7,
        1000,
        -1000,
        46340,
        1 << 26,
        1 << 30,
        134_217_727,
        i32::MAX,
        i32::MIN,
        -(1 << 30) - 1,
    ];
    const COORDS: [i32; 16] = [
        0,
        1,
        -1,
        15,
        16,
        -16,
        -17,
        47,
        48,
        -48,
        1000,
        -1000,
        1 << 27,
        1 << 30,
        i32::MAX,
        i32::MIN,
    ];
    const LOOP_RADII: [i32; 6] = [0, 1, 2, 3, -1, -2];
    const CHUNKS: [(i32, i32); 5] = [
        (0, 0),
        (-1, 2),
        (-3, -5),
        (134_217_727, -134_217_728),
        (i32::MAX, i32::MIN),
    ];

    /// The oracle's FNV-1a 64 over Java `int`s (low 32 bits, unsigned) and its value count.
    struct Fold(u64, i32);

    impl Fold {
        fn new() -> Self {
            Self(0xcbf2_9ce4_8422_2325, 0)
        }

        fn put(&mut self, value: i32) {
            self.0 = (self.0 ^ u64::from(value as u32)).wrapping_mul(0x0100_0000_01b3);
            self.1 += 1;
        }
    }

    /// NoiseTileSizeOracle (`.local/terraforged/oracles/noise_generator/run-tile-size.sh`): the real class of the
    /// newer source (6dd607eb) and of the jar, interpreted and JIT, all give this text. Its `e` lines replay
    /// `ErodedNoiseGenerator`'s chunk-to-tile loops (`collectNeighbours`, `generateCenterChunk` over `NoiseData`'s
    /// -1..17, `awaitNeighbours`) with Java `int` math, including negative radii (empty loops) and wrapping chunks.
    #[test]
    fn noise_tile_size_matches_current_java_source() {
        let mut out = String::new();
        for radius in RADII {
            let s = NoiseTileSize::new(radius);
            writeln!(
                out,
                "s {radius} {} {} {} {} {} {} {} {} | {s}",
                s.chunk_length,
                s.chunk_size,
                s.chunk_min,
                s.chunk_max,
                s.region_length,
                s.region_size,
                s.min,
                s.max
            )
            .unwrap();
            let mut fold = Fold::new();
            for z in COORDS {
                for x in COORDS {
                    fold.put(s.chunk_index_of(x, z));
                    fold.put(s.chunk_index_of_rel(x, z));
                    fold.put(s.index_of(x, z));
                    fold.put(s.index_of_rel(x, z));
                }
            }
            writeln!(out, "i {radius} {:x}", fold.0).unwrap();
        }
        writeln!(
            out,
            "d {} {}",
            NoiseTileSize::DEFAULT,
            NoiseTileSize::CHUNK_SIZE
        )
        .unwrap();
        for radius in LOOP_RADII {
            let tile_size = NoiseTileSize::new(radius);
            for (chunk_x, chunk_z) in CHUNKS {
                let mut fold = Fold::new();
                for dz in tile_size.chunk_min..tile_size.chunk_max {
                    for dx in tile_size.chunk_min..tile_size.chunk_max {
                        if dx == 0 && dz == 0 {
                            continue;
                        }
                        fold.put(tile_size.chunk_index_of_rel(dx, dz));
                        fold.put(chunk_x.wrapping_add(dx));
                        fold.put(chunk_z.wrapping_add(dz));
                    }
                }
                let start_x = chunk_x.wrapping_shl(4);
                let start_z = chunk_z.wrapping_shl(4);
                for dz in -1..17 {
                    fold.put(start_z.wrapping_add(dz));
                    for dx in -1..17 {
                        fold.put(start_x.wrapping_add(dx));
                        fold.put(tile_size.index_of_rel(dx, dz));
                    }
                }
                for cz in tile_size.chunk_min..tile_size.chunk_max {
                    for cx in tile_size.chunk_min..tile_size.chunk_max {
                        if cx == 0 && cz == 0 {
                            continue;
                        }
                        fold.put(tile_size.chunk_index_of_rel(cx, cz));
                        let (rel_x, rel_z) = (cx.wrapping_shl(4), cz.wrapping_shl(4));
                        for i in 0..256 {
                            let (dx, dz) = (i & 15, i >> 4);
                            fold.put(
                                tile_size
                                    .index_of_rel(rel_x.wrapping_add(dx), rel_z.wrapping_add(dz)),
                            );
                        }
                    }
                }
                writeln!(
                    out,
                    "e {radius} {chunk_x} {chunk_z} {} {:x}",
                    fold.1, fold.0
                )
                .unwrap();
            }
        }
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 63);
        assert_eq!(
            lines[2],
            "s 1 3 9 -1 2 48 2304 -16 32 | NoiseTileSize{chunkDiameter=3, chunkSize=9, chunkMin=-1, chunkMax=2, regionDiameter=48, regionSize=2304, min=-16, max=32}"
        );
        assert_eq!(
            lines[22],
            "s 1073741824 -2147483647 1 -1073741824 1073741825 16 256 0 16 | NoiseTileSize{chunkDiameter=-2147483647, chunkSize=1, chunkMin=-1073741824, chunkMax=1073741825, regionDiameter=16, regionSize=256, min=0, max=16}"
        );
        assert_eq!(lines[38], "e 1 0 0 2746 6552469816fd2fae");
        assert_eq!(
            lines[62],
            "e -2 2147483647 -2147483648 666 f0b80a713d5ded92"
        );
        let fnv = out.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
        assert_eq!(fnv, 0xe62f_61b7_b3c5_c62a);
        assert_eq!(NoiseTileSize::DEFAULT, NoiseTileSize::new(1));
    }
}
