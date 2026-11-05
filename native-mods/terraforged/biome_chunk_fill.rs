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

// Adapted from TerraForged 6dd607eb (https://github.com/TerraForged/TerraForged/tree/6dd607ebfd41a3b7274e090b1f2cf565d53d1d44,
// MIT License, Copyright (c) 2021 TerraForged): `src/main/java/com/terraforged/mod/worldgen/util/ChunkUtil.java`
// (`fillNoiseBiomes`) and `util/BiomeBuffer2D.java`, as `Generator.createBiomes` runs them: one 4x4 layer, no cave
// band (TerraForged's cave biomes come from its noise-cave carver, not from the biome fill).

use super::super::biome_chunk::{BiomeChunk, BiomeFillError};
use horizon_core::position::ChunkPos;

/// The quart y at which `fillNoiseBiomes` asks for every column's biome (`source.getNoiseBiome(x, -1, z, ..)`).
pub const SAMPLE_QUART_Y: i32 = -1;

/// `ChunkUtil.fillNoiseBiomes(chunk, source, resource)`: samples the chunk's 4x4 quart columns once, `dz` then `dx`,
/// with `source(x, SAMPLE_QUART_Y, z)` (`BiomeSource.getNoiseBiome`, quart coordinates from
/// `QuartPos.fromBlock(chunk.getPos().getMinBlockX())`) into the `BiomeBuffer2D`, then fills every section of the
/// chunk's build range from that one buffer: each column's biome is the same at every height.
///
/// `position` must be the chunk's own position (Java reads `chunk.getPos()`; Horizon's `BiomeChunk` keeps it
/// private), or this panics before writing anything. A failing source returns its error before any section is
/// written, as Java's exception does. Like TerraForged, it fills the sections directly
/// (`LevelChunkSection.fillBiomesFromNoise`), so 26.3's blending and below-zero retrogen decorations of
/// `ChunkAccess.fillBiomesFromNoise` do not apply.
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
            let x = origin_x.wrapping_add(dx);
            let z = origin_z.wrapping_add(dz);
            biomes.push(source(x, SAMPLE_QUART_Y, z).map_err(BiomeFillError::Resolve)?);
        }
    }

    let (min_quart, max_quart) = chunk.quart_range();
    chunk.fill(
        min_quart.wrapping_shl(2),
        max_quart.wrapping_shl(2).wrapping_add(3),
        |quart| {
            assert!(
                quart[0] & !3 == origin_x && quart[2] & !3 == origin_z,
                "fill_noise_biomes: {position:?} is not the biome chunk's position"
            );
            // BiomeBuffer2D: `z << 2 | x` of the quart's low two bits.
            Ok(biomes[((quart[2] & 3) << 2 | (quart[0] & 3)) as usize].clone())
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, convert::Infallible};

    #[test]
    fn one_layer_fills_every_section_in_java_order() {
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

    /// `BiomeChunkFillOracle.java` (`.local/terraforged/oracles/biome_chunk_fill/6dd607eb/`, `run.sh`): 6dd607eb's
    /// `fillNoiseBiomes` extracted verbatim, its real `BiomeBuffer2D`, 1.19's section fill (6dd607eb's target).
    const JAVA: &str = "c 0 69ec75b28a1fb7af\nc 1 ee858ac3ec74d22f\nc 2 69dc881b2ebd86af\nc 3 e327c61ed6f09c2f\nc 4 345eee8c93d8916f\nc 5 d848acc214683c8f\nc 6 6f2447fb1d0ffb4f\nc 7 2f2eeb68d578665f\nc 8 6c374a257c08f61f\nc 9 8602fa927fc1501f\nc 10 d4decda82371181f\nc 11 2a0f73ccd995e98f\nc 12 b04032944ff3d2bf\nc 13 f69f3feb8d8c794f\nc 14 e90ad79ffe574dff\nc 15 d74d75a3c914447f\nc 16 8092bf64c096987f\nc 17 b5da0c0eeaa4a0bf\nc 18 6cb0d37e9582d57f\nc 19 27a36da778aa2c5f\nc 20 31fb930ccad1c09f\nc 21 34df773639c62c9f\nc 22 c092324d35556d9f\nc 23 61b95ce8b95be41f\nc 24 4568c626231da41f\nc 25 f660a18a1f89755f\nc 26 4e85d9bb86a5fb9f\nc 27 50f2411cd5821b5f\nc 28 186d93a429d6e14f\nc 29 86b30fe0e327028f\nc 30 acb3eaefa4d1fe0f\nc 31 f56df1cc33cc728f\nc 32 6fcad3f56be6184f\nc 33 3179e033aa58cf4f\nc 34 744f1e2f0ea0717f\nc 35 3f50909b8e8b76cf\nc 36 dd1e1d55f44e754f\nc 37 6d4827de04e991cf\nc 38 9e5bee52ffca5c6f\nc 39 6d23755d71209c6f\nc 40 cf2692f66d19400f\nc 41 1cc06165bb3e2bef\nc 42 49b413270d53df7f\nc 43 85759557d7b8953f\nc 44 0e302f93e9a0a7bf\nc 45 a8c72ef355913a5f\nc 46 981a1d98f18d005f\nc 47 020d434bfe905a7f\nc 48 fcd557567fa567cf\nc 49 e562c3b2d42ce3cf\nc 50 9ce4e0a0a7dc814f\nc 51 eee9f99e91b31ecf\nc 52 64b552ea3d3ac84f\nc 53 b28b81d32d04b08f\nc 54 9c1e64e7a4ba69ef\nc 55 a615a84cec936d2f\nc 56 d5fca3c91757303f\nc 57 c537ee2bf8402c3f\nc 58 3030612fce917a3f\nc 59 294b19bffb3a7aff\nc 60 26436dd3269b35bf\nc 61 5c7a8ab5e0281adf\nc 62 e98a9278fcea753f\nf 1 threw 6276738d1b9644d1\nf 2 threw f7ea55a70a852578\nf 15 threw 27902ad85e307ec3\nf 16 threw 1185ffb4134fca7f\nr 0 5e1a64f07e9863ef\nr 1 8182a9090ca538ff\nr 2 47280c0e07a6b1ff\n";

    // The oracle's stand-in `Source` (same functions): biome ids from (seed, quart x, quart z) and a call trace.
    fn mix(seed: i64, x: i32, z: i32, salt: i64) -> i64 {
        let mut h = seed ^ salt;
        h = h.wrapping_add((x as i64).wrapping_mul(0x9E37_79B9_7F4A_7C15_u64 as i64));
        h ^= ((h as u64) >> 31) as i64;
        h = h.wrapping_add((z as i64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F_u64 as i64));
        h ^= ((h as u64) >> 29) as i64;
        h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9_u64 as i64);
        h ^ ((h as u64) >> 32) as i64
    }

    fn fnv(h: u64, v: i32) -> u64 {
        (h ^ u64::from(v as u32)).wrapping_mul(0x100_0000_01b3)
    }

    struct Stub {
        seed: i64,
        fail_at: u32,
        calls: Cell<u32>,
        trace: Cell<u64>,
    }

    impl Stub {
        fn new(seed: i64, fail_at: u32) -> Self {
            Self {
                seed,
                fail_at,
                calls: Cell::new(0),
                trace: Cell::new(0xcbf2_9ce4_8422_2325),
            }
        }
        fn biome(&self, x: i32, y: i32, z: i32) -> Result<i32, u32> {
            self.calls.set(self.calls.get() + 1);
            self.trace.set(fnv(fnv(fnv(self.trace.get(), y), x), z));
            if self.calls.get() == self.fail_at {
                return Err(self.calls.get());
            }
            Ok(((mix(self.seed, x, z, 1) as u64) >> 40) as i32)
        }
        fn fill(
            &self,
            chunk: &mut BiomeChunk<i32>,
            position: ChunkPos,
        ) -> Result<(), BiomeFillError<u32>> {
            fill_noise_biomes(chunk, position, |x, y, z| self.biome(x, y, z))
        }
        fn digest(&self, chunk: &BiomeChunk<i32>) -> u64 {
            let mut h = fnv(self.trace.get(), self.calls.get() as i32);
            let mut i = 0;
            while let Some(section) = chunk.section(i) {
                h = section.iter().fold(h, |h, &id| fnv(h, id));
                i += 1;
            }
            h
        }
    }

    const CHUNKS: [(i32, i32); 9] = [
        (0, 0),
        (-1, -1),
        (-2, 3),
        (7, -5),
        (1_874_999, -1_875_000),
        ((1 << 27) - 1, -(1 << 27)),
        (1 << 27, -(1 << 27) - 1),
        (i32::MAX, i32::MIN),
        (0x1234_5678, -0x0abc_def1),
    ];
    const RANGES: [(i32, i32); 7] = [
        (-64, 384),
        (-64, 1024),
        (0, 256),
        (128, 64),
        (-2032, 4064),
        (-64, 64),
        (-16, 32),
    ];
    const SEEDS: [i64; 6] = [0, 1, -1, 0x7fff_ffff, i64::MIN, 0x0123_4567_89ab_cdef];

    fn java(prefix: &str) -> Vec<&'static str> {
        JAVA.lines()
            .filter_map(|line| line.strip_prefix(prefix))
            .collect()
    }

    #[test]
    fn fill_matches_terraforged_java() {
        let expected = java("c ");
        let mut n = 0;
        for (cx, cz) in CHUNKS {
            for (min_y, height) in RANGES {
                let position = ChunkPos::new(cx, cz);
                let mut chunk = BiomeChunk::new(position, min_y, height, -1).unwrap();
                let stub = Stub::new(SEEDS[n % SEEDS.len()], 0);
                stub.fill(&mut chunk, position).unwrap();
                assert_eq!(
                    format!("{n} {:016x}", stub.digest(&chunk)),
                    expected[n],
                    "chunk {cx},{cz} range {min_y}+{height}"
                );
                n += 1;
            }
        }
        assert_eq!(n, expected.len());
    }

    #[test]
    fn failing_source_leaves_the_chunk_as_java_does() {
        let expected = java("f ");
        for (line, fail_at) in expected.iter().zip([1, 2, 15, 16]) {
            let position = ChunkPos::new(-2, 3);
            let mut chunk = BiomeChunk::new(position, -64, 384, -1).unwrap();
            let stub = Stub::new(5, fail_at);
            assert_eq!(
                stub.fill(&mut chunk, position),
                Err(BiomeFillError::Resolve(fail_at))
            );
            assert_eq!(
                *line,
                format!("{fail_at} threw {:016x}", stub.digest(&chunk))
            );
            assert!((0..24).all(|i| chunk.section(i).unwrap().iter().all(|b| *b == -1)));
        }
        assert_eq!(expected.len(), 4);
    }

    #[test]
    fn reused_resource_carries_nothing_across_chunks() {
        let expected = java("r ");
        for (i, line) in (0..3).zip(&expected) {
            let position = ChunkPos::new(i, -i);
            let mut chunk = BiomeChunk::new(position, -64, 384, -1).unwrap();
            let stub = Stub::new(9 + i64::from(i), 0);
            stub.fill(&mut chunk, position).unwrap();
            assert_eq!(*line, format!("{i} {:016x}", stub.digest(&chunk)));
        }
        assert_eq!(expected.len(), 3);
    }

    #[test]
    #[should_panic(expected = "is not the biome chunk's position")]
    fn wrong_position_panics() {
        let mut chunk = BiomeChunk::new(ChunkPos::new(1, 1), -64, 384, -1).unwrap();
        let stub = Stub::new(0, 0);
        let _ = stub.fill(&mut chunk, ChunkPos::new(1, 2));
    }
}
