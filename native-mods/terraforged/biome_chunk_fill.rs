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

// Adapted from TerraForged Reimagined (https://github.com/Romoslayer/TerraForged-Reimagined, a24d450):
// `src/main/java/com/terraforged/mod/worldgen/util/ChunkUtil.java` (`CAVE_BIOME_MAX_Y`, `isCaveBiomeSection`,
// `isCaveBiomeQuart`, `fillNoiseBiomes`) and `util/BiomeBuffer2D.java`, as `Generator.createBiomes` runs them.
// TerraForged 6dd607eb has only the surface buffer (one `getNoiseBiome(x, -1, z)` layer); the cave band is
// Reimagined's. MIT License, Copyright (c) 2020 TerraForged, Copyright (c) 2026 Romoslayer; see
// `LICENSE-TERRAFORGED-REIMAGINED`.

use super::super::biome_chunk::{BiomeChunk, BiomeFillError};
use horizon_core::position::ChunkPos;

/// `ChunkUtil.CAVE_BIOME_MAX_Y`: the highest block that can be given a cave biome.
pub const CAVE_BIOME_MAX_Y: i32 = 0;

/// `ChunkUtil.isCaveBiomeSection`: whether the whole section lies in the cave-biome band, in Java `int` math
/// (`sectionToBlockCoord(sectionY) + 15 <= CAVE_BIOME_MAX_Y`, both steps wrapping).
pub fn is_cave_biome_section(section_y: i32) -> bool {
    section_y.wrapping_shl(4).wrapping_add(15) <= CAVE_BIOME_MAX_Y
}

/// `ChunkUtil.isCaveBiomeQuart`: `isCaveBiomeSection(blockToSectionCoord(QuartPos.toBlock(quartY)))`, written
/// in Java's form (wrapping shifts); the section's `<< 4` keeps only the low 28 bits, so it equals `quart_y >> 2`.
pub fn is_cave_biome_quart(quart_y: i32) -> bool {
    is_cave_biome_section(quart_y.wrapping_shl(2) >> 4)
}

/// `ChunkUtil.fillNoiseBiomes(chunk, source, resource)`: samples the chunk's 4x4 quart columns once, `dz` then
/// `dx`, each column's surface biome and then its cave (layer) biome (`Source.getSurfaceBiome`/`getLayerBiome`,
/// quart coordinates from `QuartPos.fromBlock(chunk.getPos().getMinBlockX())`), into the two `BiomeBuffer2D`s;
/// a column without a cave biome keeps its surface one in the cave buffer. Then it fills every section of the
/// chunk's build range from one buffer: the cave buffer for sections wholly below `CAVE_BIOME_MAX_Y` when any
/// column has a cave biome, the surface buffer otherwise. Within each band the biome is constant down the column.
///
/// `position` must be the chunk's own position (Java reads `chunk.getPos()`; Horizon's `BiomeChunk` keeps it
/// private), or this panics before writing anything. A failing sampler returns its error before any section is
/// written, as Java's exception does. Like Reimagined, it fills the sections directly (`LevelChunkSection
/// .fillBiomesFromNoise`), so 26.3's blending and below-zero retrogen decorations of
/// `ChunkAccess.fillBiomesFromNoise` do not apply.
pub fn fill_noise_biomes<T: Clone, E>(
    chunk: &mut BiomeChunk<T>,
    position: ChunkPos,
    mut surface_biome: impl FnMut(i32, i32) -> Result<T, E>,
    mut cave_biome: impl FnMut(i32, i32) -> Result<Option<T>, E>,
) -> Result<(), BiomeFillError<E>> {
    let origin_x = position.x.wrapping_shl(4) >> 2;
    let origin_z = position.z.wrapping_shl(4) >> 2;
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
            assert!(
                quart[0] & !3 == origin_x && quart[2] & !3 == origin_z,
                "fill_noise_biomes: {position:?} is not the biome chunk's position"
            );
            // BiomeBuffer2D: `z << 2 | x` of the quart's low two bits; the band is chosen per section.
            let index = ((quart[2] & 3) << 2 | (quart[0] & 3)) as usize;
            let values = if any_cave && is_cave_biome_section(quart[1] >> 2) {
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
    use std::{
        cell::{Cell, RefCell},
        convert::Infallible,
        rc::Rc,
    };

    #[test]
    fn cave_band_boundaries_match_reimagined() {
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

    /// `BiomeChunkFillOracle.java` (`.local/terraforged/oracles/biome_chunk_fill/`, `run.sh`): Reimagined's
    /// `fillNoiseBiomes`/`isCaveBiome*` extracted verbatim, its real `BiomeBuffer2D`, 26.3's section fill.
    const JAVA: &str = "s -2 true\ns -1 true\ns 0 false\ns 1 false\ns -2147483648 false\ns 2147483647 true\ns 134217727 false\ns 134217728 true\ns -134217728 true\ns -134217729 false\nq -5 true\nq -4 true\nq -3 true\nq -2 true\nq -1 true\nq 0 false\nq 1 false\nq 2 false\nq 3 false\nq 4 false\nq -2147483648 false\nq 2147483647 true\nq 536870912 true\nq 536870911 false\nq -536870912 true\nq -536870913 false\nq 1073741824 false\nq -1073741825 true\nq 2147483644 true\nc 0 a6c49fce10351d4f\nc 1 704a169f475b512f\nc 2 d783f946ef356def\nc 3 5303def04c31f3cf\nc 4 0b671740e737ad0f\nc 5 0d0d531ccd4fb12f\nc 6 66b129c7b937cecf\nc 7 be98f8ef86809e4f\nc 8 862089404334cc4f\nc 9 d5c6b5c74fc603cf\nc 10 45b96ec5ec2a824f\nc 11 5a1a0d85ff32a12f\nc 12 8b4289f3fde760af\nc 13 2b3858f6b4adc4f7\nc 14 ce9751fde69195f7\nc 15 d5c6b5c74fc603cf\nc 16 772ea69c1544b18f\nc 17 5369c70cf37df38f\nc 18 0be4bc8905e61f6f\nc 19 5d1eb5258d43d287\nc 20 21042bb123532b67\nc 21 93039bc9be6701ef\nc 22 cc217c17adb2ea6f\nc 23 64a4053af7ed2e8f\nc 24 e1d090db11bb8fef\nc 25 d2dc741c967f30af\nc 26 04784c4e2f10742f\nc 27 ba1c7b95f82af5ef\nc 28 cf4e10c859bb7def\nc 29 6ad0f94aafd0f26f\nc 30 bca09479175e9a8f\nc 31 a9faba4e4e39e94f\nc 32 e78c26ca7dad5e4f\nc 33 61386da7e25746ef\nc 34 61a84143aa3b758f\nc 35 8c0b08cd16743437\nc 36 bca09479175e9a8f\nc 37 b2f39bd7e5c5260f\nc 38 5ab7a65adb0f128f\nc 39 8ef00cbe7c4568ef\nc 40 8a3c737a15b1b94f\nc 41 10695cb936e629a7\nc 42 c98c41ca721003cf\nc 43 1414d6145fae1e2f\nc 44 f0fea1e3c87bc28f\nc 45 0f9317f7c5930fcf\nc 46 3faea2634e5652cf\nc 47 177d1012a2e991cf\nc 48 734bde409e2f33cf\nc 49 f0b74d79d5b9dc4f\nc 50 1e51ccf0fad4cacf\nc 51 623dc1d7f732858f\nc 52 fe6a361cedaf9fcf\nc 53 49a002240d78d80f\nc 54 659610f25f8d77cf\nc 55 60dccbb4a03c8e97\nc 56 15c617eb6e6fb87f\nc 57 623dc1d7f732858f\nc 58 deda769d08b76ecf\nc 59 8f77248e17706a0f\nc 60 4cc329db31f44fcf\nc 61 4c4a206966cb7927\nc 62 4c35b0be6783681f\nc 63 0ad6007e694ef74f\nc 64 4bed362fedc4f98f\nc 65 5b8a00cd734eb34f\nc 66 cd66d4cfdf0e744f\nc 67 e08d7d7e8b72874f\nc 68 83257747abe4ec0f\nc 69 6df49b9074f3da4f\nc 70 7a63cacf35a9304f\nc 71 2a0c6cf3e34e94cf\nc 72 1e7391147d8f50cf\nc 73 66adceaaccc275cf\nc 74 a2e24736acdd764f\nc 75 b2e440a31ab7ca8f\nc 76 fda55f174c9af97f\nc 77 59a0f2a54953661f\nc 78 1e7391147d8f50cf\nc 79 31bc4acc05f38ccf\nc 80 65eb3d6a7017360f\nc 81 02789f62ca45390f\nc 82 27361d48393bb29f\nc 83 9507934a4992909f\nc 84 3f6f7782087e2a0f\nc 85 998f5df80bae756f\nc 86 484baa3a036cca4f\nc 87 33d2289c2bfc0c4f\nc 88 20e2d572f32482ef\nc 89 915fbe609a6f65ef\nc 90 b2e96e3aa60389cf\nc 91 5b5745b7c4f9284f\nc 92 b11fed69b0a6b64f\nc 93 8f2ac79e9e0d080f\nc 94 343a042f94e4326f\nc 95 3a769b6a06fd240f\nc 96 7c7483a630f6601f\nc 97 2c3814d304a99687\nc 98 1f5c29e396e9e60f\nc 99 8f2ac79e9e0d080f\nc 100 cf4e7033bbd17f0f\nc 101 54d8f511eed67faf\nc 102 8917fc9c25c8c5ff\nc 103 f1e225d9d54c4b57\nc 104 b592162b9d010e8f\nc 105 b31fb4ac7221f70f\nc 106 582a12eed6da2e8f\nc 107 aea09ce9b724d76f\nc 108 200479bdb04c958f\nc 109 9fbb17a631bc21cf\nc 110 65fd187b31576b8f\nc 111 c851efd19e94468f\nc 112 42788d022d02588f\nc 113 be5247cdbafa3a8f\nc 114 ce32baf67c0b16af\nc 115 5fd20da037142bef\nc 116 8097bbd6f7cbb3af\nc 117 7db731107c1c61af\nc 118 f82130f27ed750ef\nc 119 704bfc0b1113a4f7\nc 120 ce32baf67c0b16af\nc 121 eab76ab2776af72f\nc 122 32a848fb087f89af\nc 123 ec6905a4856f70ef\nc 124 42779086c8a8baaf\nc 125 e23f279a92492a07\nc 126 b77ce5e52408896f\nc 127 ccca29f8133828af\nc 128 bf191dbb27f5966f\nc 129 df27e32128be9f2f\nc 130 b6406d02ffc5bd6f\nc 131 658261e59d16c62f\nc 132 c22afe125e5e7daf\nc 133 15793975415b672f\nc 134 c3410f79d27101af\nc 135 5b96cacc7ab54fcf\nc 136 e33dea46975bcd6f\nc 137 9a7269edbec0dd6f\nc 138 5845b713fc056a1f\nc 139 980e24050e82c44f\nc 140 db0c0c1fa23ccaaf\nc 141 5b96cacc7ab54fcf\nc 142 ceb43eff194ef5af\nc 143 3e4cfb577eebc26f\nc 144 63d25d3330f538bf\nc 145 0fd68bf7cab6b18f\nc 146 0afc4415a8bba88f\nc 147 89696226426002cf\nc 148 ef6aec3c7a2a694f\nc 149 42948b7ff512390f\nc 150 83279216bc6d4d8f\nc 151 9ea32f618841448f\nc 152 a9a638bb3333186f\nc 153 7104554d24d2fb0f\nc 154 7ffa87c9d269938f\nc 155 86a9376bbfddc98f\nc 156 a80e43da8196682f\nc 157 c5d5347cd2e7054f\nc 158 b67a58e89c6070ef\nc 159 ba28f5115e7c8e9f\nc 160 b124b730344fb43f\nc 161 e6e7126f6245f32f\nc 162 a80e43da8196682f\nc 163 b05ad27d35453a4f\nc 164 a0570f11d92ed54f\nc 165 ad9dda96f29142ff\nc 166 ae55c06e0c24085f\nc 167 16b423c746a7296f\nc 168 754aaea48f808fef\nc 169 a7d15261e38e714f\nc 170 15d79f2b230670ef\nc 171 ae7faa84fe00bdef\nc 172 c6cf677fc14ca6cf\nc 173 16ec1251b892b0af\nc 174 c00466ec863001ef\nc 175 f8277e25b684e86f\nc 176 f4dd4508dcd5f5ef\nc 177 80f97ae9341a036f\nc 178 0631797a9116e96f\nc 179 1d441fb6a23b38af\nc 180 d94961109bf0526f\nc 181 56df67e39cdb6417\nc 182 78d3c0ac63a02b2f\nc 183 80f97ae9341a036f\nc 184 d096bae58b132e4f\nc 185 9500b8eeef4bf56f\nc 186 f1929e4fef33df6f\nc 187 07fef9695e759347\nc 188 9cc4ce45ea1b89af\nf 1 threw 6611332cde6f78fe\nf 2 threw 6d2ac15e1eb26f88\nf 17 threw 140d933ea2d571e0\nf 31 threw e60703b4282c032d\nf 32 threw 44046bad034d75cf\nr 0 be553706c88be82f\nr 1 bd35e104eacf71cf\nr 2 d05ddfd006585b6f\n";

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
        cave_mode: i64,
        fail_at: u32,
        calls: Cell<u32>,
        trace: Cell<u64>,
    }

    impl Stub {
        fn new(seed: i64, cave_mode: i64, fail_at: u32) -> Self {
            Self {
                seed,
                cave_mode,
                fail_at,
                calls: Cell::new(0),
                trace: Cell::new(0xcbf2_9ce4_8422_2325),
            }
        }
        fn call(&self, kind: i32, x: i32, z: i32) -> Result<(), u32> {
            self.calls.set(self.calls.get() + 1);
            self.trace.set(fnv(fnv(fnv(self.trace.get(), kind), x), z));
            if self.calls.get() == self.fail_at {
                return Err(self.calls.get());
            }
            Ok(())
        }
        fn surface(&self, x: i32, z: i32) -> Result<i32, u32> {
            self.call(0, x, z)?;
            Ok(((mix(self.seed, x, z, 1) as u64) >> 40) as i32)
        }
        fn cave(&self, x: i32, z: i32) -> Result<Option<i32>, u32> {
            self.call(1, x, z)?;
            let c = mix(self.seed, x, z, 2);
            Ok(((c & 7) < self.cave_mode).then(|| 0x100_0000 | ((c as u64) >> 40) as i32))
        }
        fn fill(
            &self,
            chunk: &mut BiomeChunk<i32>,
            position: ChunkPos,
        ) -> Result<(), BiomeFillError<u32>> {
            fill_noise_biomes(
                chunk,
                position,
                |x, z| self.surface(x, z),
                |x, z| self.cave(x, z),
            )
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
    fn cave_band_tests_match_reimagined_java() {
        let mut lines = java("s ").into_iter().chain(java("q "));
        let sections = [
            -2,
            -1,
            0,
            1,
            i32::MIN,
            i32::MAX,
            (1 << 27) - 1,
            1 << 27,
            -(1 << 27),
            -(1 << 27) - 1,
        ];
        let quarts = [
            -5,
            -4,
            -3,
            -2,
            -1,
            0,
            1,
            2,
            3,
            4,
            i32::MIN,
            i32::MAX,
            1 << 29,
            (1 << 29) - 1,
            -(1 << 29),
            -(1 << 29) - 1,
            1 << 30,
            -(1 << 30) - 1,
            i32::MAX - 3,
        ];
        for s in sections {
            assert_eq!(
                lines.next(),
                Some(format!("{s} {}", is_cave_biome_section(s)).as_str())
            );
        }
        for q in quarts {
            assert_eq!(
                lines.next(),
                Some(format!("{q} {}", is_cave_biome_quart(q)).as_str())
            );
        }
        assert_eq!(lines.next(), None);
    }

    #[test]
    fn fill_matches_reimagined_java() {
        let expected = java("c ");
        let mut n = 0;
        for (cx, cz) in CHUNKS {
            for (min_y, height) in RANGES {
                for cave_mode in [0, 3, 8] {
                    let position = ChunkPos::new(cx, cz);
                    let mut chunk = BiomeChunk::new(position, min_y, height, -1).unwrap();
                    let stub = Stub::new(SEEDS[n % SEEDS.len()], cave_mode, 0);
                    stub.fill(&mut chunk, position).unwrap();
                    assert_eq!(
                        format!("{n} {:016x}", stub.digest(&chunk)),
                        expected[n],
                        "chunk {cx},{cz} range {min_y}+{height} cave mode {cave_mode}"
                    );
                    n += 1;
                }
            }
        }
        assert_eq!(n, expected.len());
    }

    #[test]
    fn failing_sampler_leaves_the_chunk_as_java_does() {
        let expected = java("f ");
        for (line, fail_at) in expected.iter().zip([1, 2, 17, 31, 32]) {
            let position = ChunkPos::new(-2, 3);
            let mut chunk = BiomeChunk::new(position, -64, 384, -1).unwrap();
            let stub = Stub::new(5, 3, fail_at);
            assert_eq!(
                stub.fill(&mut chunk, position),
                Err(BiomeFillError::Resolve(fail_at))
            );
            assert_eq!(
                *line,
                format!("{fail_at} threw {:016x}", stub.digest(&chunk))
            );
        }
        assert_eq!(expected.len(), 5);
    }

    #[test]
    fn reused_resource_carries_nothing_across_chunks() {
        let expected = java("r ");
        for (i, line) in (0..3).zip(&expected) {
            let position = ChunkPos::new(i, -i);
            let mut chunk = BiomeChunk::new(position, -64, 384, -1).unwrap();
            let stub = Stub::new(9, if i == 1 { 0 } else { 8 }, 0);
            stub.fill(&mut chunk, position).unwrap();
            assert_eq!(*line, format!("{i} {:016x}", stub.digest(&chunk)));
        }
        assert_eq!(expected.len(), 3);
    }

    #[test]
    #[should_panic(expected = "is not the biome chunk's position")]
    fn wrong_position_panics() {
        let mut chunk = BiomeChunk::new(ChunkPos::new(1, 1), -64, 384, -1).unwrap();
        let stub = Stub::new(0, 3, 0);
        let _ = stub.fill(&mut chunk, ChunkPos::new(1, 2));
    }
}
