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
    super::terrain::{TerrainBlockStates, TerrainChunk, TerrainError},
    terraforged_engine::TerrainProperties,
    terraforged_terrain_data::TerrainData,
};
use crate::{
    heightmap::{HeightmapState, HeightmapType},
    registry::BlockStateId,
};

/// TerraForged 6dd607eb's `Surface.apply` (the 1.18.2-0.3.1 jar's bytecode is the same), which
/// `SurfaceDecorator.decoratePost` runs. Each column whose OCEAN_FLOOR_WG height is at or above `sea_level` and whose
/// gradient, normalised by `55 * gen_depth / 255`, is not below 0.6 (NaN erodes, as Java's `<` test) loses its
/// erodible top: from that height down to the first non-erodible state found strictly above `max(0, y - 20)`
/// (`findSolid`), every block becomes that state. Nothing happens when the top block is not erodible or no
/// non-erodible state is found.
///
/// `gen_depth` is the generator's `getGenDepth()` and `sea_level` its `getSeaLevel()`. `is_erodible` is Java's
/// `isErodible`: `#minecraft:dirt` or `#minecraft:snow`. With 26.3's tags that is dirt, coarse dirt, rooted dirt and
/// the snow blocks, but not grass, podzol, mycelium, moss or mud, which the tag held before 26.x. `terrain` is the
/// chunk's cached data (`TerrainDataHandle::read()`); never call this while that thread holds or waits for the
/// same entry's write guard. Blocks go through `set_block_worldgen`, so only the chunk stage's heightmaps follow the
/// edits, as 26.3's `ProtoChunk.setBlockState` does: from TERRAIN on (where the surface step runs) OCEAN_FLOOR_WG
/// keeps its pre-erosion heights. A missing OCEAN_FLOOR_WG is primed first, as `ChunkAccess.getHeight` does.
pub fn apply<B: TerrainBlockStates>(
    terrain: &TerrainData,
    chunk: &mut TerrainChunk<'_, B>,
    gen_depth: i32,
    sea_level: i32,
    mut is_erodible: impl FnMut(BlockStateId) -> bool,
) -> Result<(), TerrainError> {
    let norm = 55.0 * (gen_depth as f32 / 255.0);
    if chunk.heightmap(HeightmapType::OceanFloorWg).is_none() {
        chunk.prime_heightmaps(&[HeightmapType::OceanFloorWg])?;
    }
    for z in 0..16 {
        for x in 0..16 {
            let mut y = chunk
                .heightmap(HeightmapType::OceanFloorWg)
                .expect("OCEAN_FLOOR_WG primed above")
                .highest_taken(x, z)?;
            let gradient = terrain.normalized_gradient(x, z, norm);
            if y < sea_level || gradient < 0.6 {
                continue;
            }
            let Some((bottom, solid)) = find_solid(chunk, x, y, z, &mut is_erodible) else {
                continue;
            };
            while y > bottom {
                chunk.set_block_worldgen(x, y, z, solid)?;
                y -= 1;
            }
        }
    }
    Ok(())
}

/// TerraForged 6dd607eb's `Surface.smoothWater` with `shouldSmooth`, `isSmoothable` and `sameChunk` (the
/// 1.18.2-0.3.1 jar's bytecode and TerraForged Reimagined 26.3 are the same), which `BiomeGenerator.decorate` runs
/// after the features and cave decoration. Each river or lake column (`Terrain.isRiver() || isLake()`) whose river
/// value is `== 0` (so `-0.0` passes and NaN doesn't) and whose MOTION_BLOCKING_NO_LEAVES top block is source water
/// becomes level-2 water when any block at that height within the radius-6 disc around it (offset rows `dz` from -6,
/// then `dx`, skipping the centre) is air (`BlockState.isAir`); the search stops at the first air block. Blocks of
/// this chunk are read from `chunk`, others through `region_block(x, y, z)` (world block coordinates, Java's
/// `WorldGenLevel.getBlockState`); an error there is returned at once, before that column is written.
///
/// `source_water` is `minecraft:water`'s default state (level 0, water's only property, so it is the one state
/// `is(Blocks.WATER) && LEVEL == 0` accepts), `level_two_water` the same block with `level=2`. Writes go through
/// `set_block_worldgen` (26.3's `ProtoChunk.setBlockState`); nothing updates heightmaps by hand. A missing
/// MOTION_BLOCKING_NO_LEAVES is primed when the first smoothable column reads it, as `ChunkAccess.getHeight` does,
/// so a chunk without one stays without it. `terrain` is the chunk's cached data, only read (see [`apply`]).
pub fn smooth_water<B: TerrainBlockStates>(
    terrain: &TerrainData,
    chunk: &mut TerrainChunk<'_, B>,
    source_water: BlockStateId,
    level_two_water: BlockStateId,
    mut region_block: impl FnMut(i32, i32, i32) -> Result<BlockStateId, TerrainError>,
) -> Result<(), TerrainError> {
    const RADIUS: i32 = 6;
    let kind = HeightmapType::MotionBlockingNoLeaves;
    let position = chunk.position();
    let min_x = position.x.wrapping_shl(4);
    let min_z = position.z.wrapping_shl(4);
    for dz in 0..16 {
        for dx in 0..16 {
            if !is_smoothable(terrain, dx, dz) {
                continue;
            }
            let x = min_x.wrapping_add(dx);
            let z = min_z.wrapping_add(dz);
            if chunk.heightmap(kind).is_none() {
                chunk.prime_heightmaps(&[kind])?;
            }
            let y = chunk
                .heightmap(kind)
                .expect("MOTION_BLOCKING_NO_LEAVES primed above")
                .highest_taken(dx, dz)?;
            if chunk.block(dx, y, dz) != source_water {
                continue;
            }
            if should_smooth(chunk, x, y, z, RADIUS, &mut region_block)? {
                chunk.set_block_worldgen(dx, y, dz, level_two_water)?;
            }
        }
    }
    Ok(())
}

/// `isSmoothable`: river or lake terrain with a river value `== 0`.
fn is_smoothable(terrain: &TerrainData, x: i32, z: i32) -> bool {
    let river = terrain.river().get(x, z);
    let land = terrain
        .terrain()
        .get(x, z)
        .expect("TerrainData holds a terrain for every column");
    (land.is_river() || land.is_lake()) && river == 0.0
}

/// `shouldSmooth`: whether any block in the disc of `radius` around `(x, y, z)` (centre excluded) is air, in Java's
/// order; `sameChunk` (`x >> 4`, `z >> 4`) picks the chunk or the region.
fn should_smooth<B: TerrainBlockStates>(
    chunk: &TerrainChunk<'_, B>,
    x: i32,
    y: i32,
    z: i32,
    radius: i32,
    region_block: &mut impl FnMut(i32, i32, i32) -> Result<BlockStateId, TerrainError>,
) -> Result<bool, TerrainError> {
    let position = chunk.position();
    let radius2 = radius * radius;
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            let d2 = dx * dx + dz * dz;
            if d2 == 0 || d2 > radius2 {
                continue;
            }
            let (sample_x, sample_z) = (x.wrapping_add(dx), z.wrapping_add(dz));
            let state = if sample_x >> 4 == position.x && sample_z >> 4 == position.z {
                chunk.block(sample_x & 15, y, sample_z & 15)
            } else {
                region_block(sample_x, y, sample_z)?
            };
            if chunk.block_properties(state).is_air() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// `findSolid`: the first non-erodible state below an erodible `y`, strictly above `max(0, y - 20)`, with its height.
/// Below the build range it reads void air (as `ProtoChunk.getBlockState`), which is not erodible.
fn find_solid<B: TerrainBlockStates>(
    chunk: &TerrainChunk<'_, B>,
    x: i32,
    y: i32,
    z: i32,
    is_erodible: &mut impl FnMut(BlockStateId) -> bool,
) -> Option<(i32, BlockStateId)> {
    if !is_erodible(chunk.block(x, y, z)) {
        return None;
    }
    let bottom = 0.max(y.wrapping_sub(20));
    let mut scan_y = y.wrapping_sub(1);
    while scan_y > bottom {
        let state = chunk.block(x, scan_y, z);
        if !is_erodible(state) {
            return Some((scan_y, state));
        }
        scan_y -= 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::{
        terraforged_engine::TerrainRegistry,
        terraforged_terrain_data::{NoiseData, NoiseSample},
        terraforged_terrain_levels::TerrainLevels,
    };
    use super::*;
    use crate::{
        DATA_VERSION, heightmap::HeightmapState, nbt::Tag, registry::BlockRegistry,
        worldgen::terrain::PreLightChunkStage,
    };
    use horizon_core::position::ChunkPos;

    /// The oracle's palette: air, void air, stone, water, dirt, snow block, gravel, grass block, coarse dirt, sand.
    const NAMES: [&str; 10] = [
        "minecraft:air",
        "minecraft:void_air",
        "minecraft:stone",
        "minecraft:water",
        "minecraft:dirt",
        "minecraft:snow_block",
        "minecraft:gravel",
        "minecraft:grass_block",
        "minecraft:coarse_dirt",
        "minecraft:sand",
    ];

    #[derive(Clone, Copy)]
    struct State(u32);

    impl HeightmapState for State {
        fn is_air(self) -> bool {
            self.0 < 2
        }

        fn is_air_block(self) -> bool {
            self.0 == 0
        }

        fn has_fluid(self) -> bool {
            self.0 == 3
        }

        fn blocks_motion_in_heightmap(self) -> bool {
            self.0 >= 2 && self.0 != 3
        }

        fn blocks_motion_in_heightmap_no_leaves(self) -> bool {
            self.blocks_motion_in_heightmap()
        }
    }

    struct Properties;

    impl TerrainBlockStates for Properties {
        type State = State;

        fn heightmap_state(&self, id: BlockStateId) -> Self::State {
            State(id.0)
        }
    }

    fn registry() -> BlockRegistry {
        BlockRegistry::from_nbt(&Tag::compound([
            ("DataVersion", Tag::Int(DATA_VERSION)),
            (
                "states",
                Tag::List(
                    NAMES
                        .iter()
                        .map(|name| Tag::compound([("id", Tag::String((*name).into()))]))
                        .collect(),
                ),
            ),
            (
                "defaults",
                Tag::compound(
                    NAMES
                        .iter()
                        .enumerate()
                        .map(|(i, name)| (*name, Tag::Int(i as i32)))
                        .collect::<Vec<_>>(),
                ),
            ),
        ]))
        .unwrap()
    }

    /// The oracle's LCG.
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

    fn fold(hash: u64, value: u32) -> u64 {
        (hash ^ u64::from(value)).wrapping_mul(0x100000001b3)
    }

    /// `SurfaceErosionOracle` (`.local/terraforged/oracles/surface_erosion/run.sh`): 6dd607eb's `Surface.apply`,
    /// `isErodible` and `findSolid` over real 26.3 ProtoChunks, heightmaps and block tags.
    const JAVA_SURFACE: &str = r"p 0 minecraft:air false false false false
p 1 minecraft:void_air false false false false
p 2 minecraft:stone false true true true
p 3 minecraft:water false false true true
p 4 minecraft:dirt true true true true
p 5 minecraft:snow_block true true true true
p 6 minecraft:gravel false true true true
p 7 minecraft:grass_block false true true true
p 8 minecraft:coarse_dirt true true true true
p 9 minecraft:sand false true true true
0 0x2ae74c271959f1ea 0x7dc9c6b2368a5348 0x9be5af13a37dc5dd 266
1 0xd80ac658736bb725 0x813b253d2516baad 0xe9441c2dfdd46ec3 0
2 0xd730a44f669635a8 0x77baf9510c38156e 0x10920254fb51b846 68
3 0x49e266f8272a4ccb 0x10ec19cadaf29ae7 0x3adb6eb414cfd221 1782
4 0x7a114dd9c11f9a8a 0x143f2f643ee4e079 0x369b465b97f706dd 13
5 0x775c067afeb904cf 0xed4b33d8f2599b43 0x5370fdf2f2bceffa 163
6 0x46d92312a16bb725 0xea37d8578e778ce1 0xdc15fb2313d09da2 27
7 0x53880426c31b60af 0x6843cea4053308d4 0xc6d1f0b1de81538e 0
8 0x8fc6d80808eed528 0x392510fe0ce6e055 0xb71d7ec570b8740f 0
9 0xd80ac658736bb725 0x052b76ec63ecb57e 0x40f7e5cbbe34f82d 0
10 0xc2ddf23a461d7aff 0x11f6da997d11629f 0x6a21533e73f68375 0
11 0x0b2dee7f4c928a1b 0xe95e43b6e17c5d75 0x662b5b8f0acb089c 0
12 0xd80ac658736bb725 0x1b4f250f64b164a9 0xbf8f7a210013eb08 0
13 0xe7f59b77c056eb0e 0xb6033b4205e551bc 0x9267dcba45d81c38 0
14 0x2416f7496fd77a98 0xaace0012e0a76f70 0x9f9c7268025a6e51 164
15 0x8372cd81ca6bb725 0x2c615d1ad18115aa 0xf17368772ae54903 0
16 0xc0eb2c6f1ad23abf 0x1221da5b8a26c5fa 0x5f782176357136a1 143
17 0x299debf5bd6fc857 0x73f37a0cf63d0e23 0xf36b9f61af333ce5 367
18 0x4cf787ae6bd74d11 0x40c4242df4af743b 0x58d942c35a434e09 86
19 0xa3c4946f00262a10 0x13e85f0108f6ea59 0x99f98d55e43dfd5f 1679
20 0x1d6b27954f91f086 0x9f3e4a7ee8f1b7bc 0x3c60694e3c11c8e3 293
21 0x9614dedfba7492ca 0xd93db0a7f60447cc 0xadb0123e96a89807 29
22 0x7293a4331b265590 0x9f267bd53a2aa1ac 0x15ae534fc14df5e3 301
23 0xd80ac658736bb725 0x37401caffda7115d 0xffe26927c8dd24df 0
24 0x415684ea6543fe88 0x71c7151d7ad0df27 0xc9258119aa95df2e 0
25 0x9d8779e7de48442c 0xcaf34672760c2306 0x964e5038170261db 31
26 0xd80ac658736bb725 0xeab5139e067126dd 0xf0bd17fd6ac8d4bd 0
27 0x4dc2a15200abfe32 0x3a43c68c2bff2640 0x1a8f424be345dc4d 0
28 0x46d92312a16bb725 0x439e41166374650b 0x0af7f4d4e3dceba9 27
29 0xd80ac658736bb725 0xba8999d1ad2a217e 0x957ccc4ad71ecc2d 0
30 0xf164187aad239922 0x897890b9102bbf29 0x16670099e1e8d4f1 0
31 0xd00860291d717ecc 0xcfd9d997e8ffb5fc 0x033c2ade911ae01d 237
32 0xfe58a798ac4093c0 0x594292ca952f9f8e 0xc368e0644e478acf 0
33 0xfa823741a7805cb6 0xbbd1ab6ae0efb1cc 0xd6e5e1a0ae19a0ef 0
34 0x46d92312a16bb725 0x2c14a0d08e27171e 0x405d24c216059337 380
35 0x46d92312a16bb725 0xfc520fa2ca7d321b 0x2efb8475b97e43a5 1788
36 0x38e5d2325d4773cf 0x69122c168c1e0d49 0xf74567063822dbbe 209
37 0xd80ac658736bb725 0xe7d44d04da1528b3 0x4997e0213854bb58 0
38 0x44c13289aec38bb1 0x0c46982c6634ac94 0xfb01e905405fc7aa 0
39 0x74876dc0998b4ad1 0xe58cee64cbb9e7f0 0x8ac6937aa5b06437 353
40 0xd80ac658736bb725 0xfd70781b71c7c443 0x2787a89d2ba04780 0
41 0x1d1d55c2163f454c 0x1b5d801ef455aab2 0x42d891dc22ac9d12 26
42 0x98fdaf201c6f0b51 0x7622a54b0a3fb463 0x5b681fca4e4262e5 104
43 0xd80ac658736bb725 0x3c5dc70f34a3cf23 0x945a867b1d0dfe21 0
44 0x46d92312a16bb725 0x749f355059c625bb 0xac4d896e8dc8ac75 61
45 0x420dbec2de4a6f0e 0x9cf9b64351a7140c 0x883316440583348d 0
46 0x804d4e4ddeb9812d 0xb4664c0c1144bf6e 0x48c22693d937f865 339
47 0x9d8fa61fc3ebb725 0xcde3fe69cf0a924b 0x21d59685d661ce19 1
48 0xf08c8e832d56eaf4 0x577e1f0e6b80fda8 0x5ed4fb41c77813c5 213
49 0xdea77e17a21cbbf9 0x44b1efadd7764ab1 0x126d5bbd9620f0ef 15
50 0xd80ac658736bb725 0xe7ddee0f9d7254d0 0xb9ede3c23f207160 0
51 0xa7ebcf7d6e936cb2 0x21f400bdee6de189 0x0838aef493608d51 1946
52 0x13cf9bf517be348a 0x39a1ef1852bff03b 0x853d2bd95d13c782 231
53 0xd80ac658736bb725 0x26a240f983e0b964 0xd84a8f91ef4e837c 0
54 0xc29924266a11b553 0x7d51be64c2d966e3 0x64b9b21e902f3e6b 301
55 0x8fec747a8442497b 0x35ea34e849a8852b 0x4a0ca7b185aff2a5 279
56 0x7fda22ad4ce4829c 0x16cc81ca7d862d38 0x67972466d93386f0 0
57 0xd079269a61a357cd 0x3662983241af4124 0x1fdec17d5f4178cf 0
58 0x2ff8ee107f324457 0x30f2dacbcbdc9246 0xdab5d7e6f3b37bb5 371
59 0x072a35479ee79a64 0xd708bfee8595270f 0x2f2f029185cbf3a3 0
60 0xb4361687b2f06153 0xa063bdd6110e1551 0x34cd0b0aeb76dcb5 0
61 0xd80ac658736bb725 0xd5e10702e8348030 0x2b97b60e9f93b573 0
62 0x9b73e417b8adcd41 0xfcb7e8d39db14185 0xabc170e6e6bae75d 0
63 0xcc53cb517567c26e 0x01a73f3254711360 0x11be90b8f439b0c9 361
64 0xd80ac658736bb725 0x1ea8ab07fbb105d8 0x8084ea7f98d49f51 0
65 0x347ad7a0fd9aab9b 0xb0b13744c919c03d 0xd431fe7a93f1d4a3 311
66 0x46d92312a16bb725 0xa9e8ab40c26bd5a5 0x10a48b45bff84a4e 0
67 0x25b4062891f276d7 0x0c58e1433ede7330 0x5e1b8fd9bea4412b 1934
68 0xa7b04856ad425e1b 0x94e4afb4239e3641 0x72f01f6527d7b536 0
69 0x190526a0c5178933 0x3962eddebd1b114c 0x533b1bca9afad7e9 0
70 0x47a383ce673eed6b 0x9620d768d95ea695 0x452c99e73e5df356 59
71 0xb120d846923ba541 0x4b1ca176d417c2ef 0x561c50054a75ef5f 171
72 0x46d92312a16bb725 0x576c1617e095e48c 0xc4d0092e08ad20f4 317
73 0x9f7b5174956a9362 0x39413e4582f9e314 0xa0fc389734402344 0
74 0xf338895c346beace 0x140040536eeb00a5 0xaf36d82466897093 225
75 0xd80ac658736bb725 0xc1885943c0c07d98 0x2d2f031e068f6996 0
76 0xe679592fb5871ef9 0x1690e096e6b969cc 0x10c60e837a536345 0
77 0xc2d85825672e63c6 0x625d6fa5f667fc35 0xe0e6a7c3790fca61 330
78 0xd80ac658736bb725 0x28b4501cb15c8990 0x0b56db2eab28632d 0
79 0xe2b3aae54aabe63f 0x76915724266f92cb 0xca480799bff93121 195
80 0x1fa96e9ca4800154 0x38504f8a2b58322b 0x2eaa9293896942db 317
81 0xd80ac658736bb725 0xed184649c0a3d666 0x414642ff13e1e276 0
82 0xc69205f7919b7a5b 0x9d44b20ccf5aa28c 0xc93e6feafe128f81 275
83 0x431636cc13bb5ea9 0x8ad1de3ec17326a6 0xef93bb0798c1b41f 1848
84 0x9fcde395597e32ce 0x9c6941d8ce87ff2b 0x5aa69b790a4d0582 78
85 0x7c7d10f9dabb2417 0x35ec8f6e9f5dd7fb 0x6e7a5063e49da891 80
86 0x314576a973606c01 0xc83aac0238f8a807 0xdbce7a928faaba61 315
87 0x7ef8692613eb9e44 0x7f5a82426c4b74e9 0x29da28e389c38113 14
88 0x4d1ec8b7092c9081 0x0e53ff495ca2c795 0xd699e2ab78653924 74
89 0xd80ac658736bb725 0x33a0c9ddfea86d1f 0xca204bfb8d858ebb 0
90 0x053ef529b82edc26 0x7792a6dbbcffe4d8 0xa00a9393940a7d36 0
91 0x1b42ca4f1590fb80 0x76e4f5493b226e22 0x94c0954c1b9e1f73 1488
92 0xd80ac658736bb725 0xd5e9101810f333ca 0x249202c3db663f75 0
93 0xc8a50f92cd548400 0xc5402f65cdc57c1b 0xf0072091de72e9d9 161
94 0x46d92312a16bb725 0x881665b07f954ecb 0x28e125f1e139a253 0
95 0x20903912dd2bb725 0x83ba547aa5fc15cf 0xada364eda3aec331 21
96 0x162a3f85c79cb18b 0x60809be51c32974f 0xb85c8d46055b74a3 256
97 0x46d92312a16bb725 0x47346e6e8e0420d7 0x82aa325d37ec374b 0
98 0x7b2672b763ed0d5d 0xff0a95b3c6b19787 0x2f54372c7ce40738 47
99 0x46d92312a16bb725 0xcc950590fcebc24d 0xdb059c5acad1b131 2086
100 0xc8132b1764985404 0x82de9bcb66533eb9 0x73e6074d23f3d815 147
101 0x21979834e1d4c778 0xc5dec90f0a938023 0xa80ee616541fd526 10
102 0xaf7faf2308413597 0xe46e7034244faab4 0x744f93a04914a247 149
103 0xd80ac658736bb725 0xa7cd301f73792c84 0xe719620bbdccf144 0
104 0xad3dbb2847ca65a6 0x869a52138b189794 0xe995e4dfc9ea69d7 22
105 0x1b6cd98fd76a9718 0xf47eb92b6f12682f 0x966d5afe5f2a0db3 160
106 0xd80ac658736bb725 0x5b6653b7e133828f 0x4ca3af488d1fa3c5 0
107 0x42f2dcca8bb6ce28 0x2f0a9cc6d3b68756 0xa17e153a04aef463 786
108 0xeb70b8752f14b18f 0xf5edab31b6802325 0xf8380e86e7da3f51 0
109 0xd80ac658736bb725 0x59999dae32c00e11 0xa8a52d37967ddabf 0
110 0xde09bec15794857e 0xcb2b5608b8c9fb82 0x2a161083a9c76575 177
111 0xc20aab1af02bb725 0x7c8b21d9c35a589e 0x098fa0c3626d10a5 55
112 0x7a7d8844f1449a4d 0xb7474e54e834c744 0xb2b81622712a15c5 232
113 0x2f9adc83c738bb46 0x20c79ee7b1eae7c7 0x1125933edb132003 0
114 0xa0970b4d85b67349 0xc3579063acc56fe3 0x16c1059ad8a9be81 42
115 0xc9b38fe2132e8b87 0xda1fcf99c909f916 0xc3d9a8879045e5a9 824
116 0xa269346b0cd4627d 0x5b3064e4bc8088d7 0xe6cc514d5e87fe5a 0
117 0xd80ac658736bb725 0x3c6e9b29a565826b 0xc1b15e35eb5216b4 0
118 0x1f7e9e02b0a8bacb 0xced200c1cf715c47 0x9486f38fa5b3fd02 0
119 0x464867056f18fa1c 0x8a29c45a6ec26550 0x572cc1c48a6a8deb 255
120 0xd80ac658736bb725 0x89572950a6095d46 0xb0424a969d041628 0
121 0x739c1050ff8d5e07 0xbd3338aa39bd83b6 0x3b67cdebec7fb4db 89
122 0x7103f2ea572708c8 0x306ab6a5d56dd69d 0x2fe3c9cfecdf1c16 236
123 0xd80ac658736bb725 0xcc27233d2fc90560 0xb267faf8d134ed55 0
124 0x67ea97ac9ca2cf1a 0x36de4cef7e76e014 0x0bf0f1f3c506e845 96
125 0x084fd0fd933500b8 0xcd8bf3e7015916ba 0x098c9c9160b36785 56
126 0x54f0513fa6fbbdca 0x0a26066d602ad27b 0x942c36f32cc5a553 0
127 0x9c6df212d511aa5e 0xda869cadfe340389 0xb80523de92b920e5 352
128 0xc033e303d4f6b2f6 0x4d81f0a27816171c 0xf7c77e3ac1555651 108
129 0xd6602690fc82a06f 0x72efdb0d3822c5db 0xe3fdd878538fb211 285
130 0xfbb1a80119f8813c 0xaebec33434b7c58e 0x500c9f23047358f5 71
131 0x46d92312a16bb725 0x9ec632b7006523f0 0x2390f740f6573c51 728
132 0xa3bc9b61fec5f3d5 0x64aa0d716e6474eb 0x5fc74601aa8b3d17 347
133 0x6007efcc1b3df989 0xaa6afef51524a3a3 0x1a92e41f6a86abd0 0
134 0xd80ac658736bb725 0x1d44916f6eb5ae92 0xc30b727fe9d89ee3 0
135 0xe5c4491603f226ad 0x6f4a009534856a34 0x1b1d814698f6a858 0
136 0x1633e97f5b93b724 0xb4083aad9e08aa94 0x78715743c23e17fb 327
137 0xdcda861eabc0e6ce 0x71d361e860d3dd1c 0x43c8b745726df75a 96
138 0x09e3bb2f7244b66b 0xc4694d211bc70396 0x2e0d684f177dff5f 0
139 0xe9e2180399226947 0xdf87de2aca5d3823 0x7032a63b839e7d64 1830
140 0x3ddc21f972d113d9 0x3c45f6979eb30c89 0x25faa885dc672672 0
141 0xa280799c594fcdae 0x06405b78eff56f02 0x9bb0a17deb099b25 206
142 0x46d92312a16bb725 0xe4ab245a71d49404 0x81d60f79f4cff54d 326
143 0x7ff585744f711cfe 0xb8ac7f4ac9b48a75 0x10c5d905d5328d4b 0
144 0x0dbd555e5bfa79e3 0xa3eba16a27d3f0dc 0xaf13e0569e8ad291 0
145 0xd80ac658736bb725 0x7a7c462228f2c8b5 0xe1414b3881cef988 0
146 0x802d1f5e8a2ed14b 0x8b0c9c5bccf9e313 0xddd2f7189abb9332 0
147 0x46d92312a16bb725 0x6c52d81fad2a9454 0x3a9a243d6d1012d1 817
148 0xd80ac658736bb725 0x9b262c83f1f5faa8 0x8084ea7f98d49f51 0
149 0xba98d219a9ab88b2 0xa210115deb7eadfb 0xfa7269019a2e6612 334
150 0x5a29c4866a5b52ae 0x548954a160ad3a20 0xf0d53e12667fb087 110
151 0x61c475b151610ff1 0x31db95f06ebde1d9 0x37c0a9eaf850a486 85
152 0xe9036b64dc2ff315 0xd2d92e9dc5d8c484 0x600b6ecbc11c2453 61
153 0xda7e6f099757d6ca 0x79c079e00f46c9d9 0x49d193b2dd83d39f 387
154 0x0e4ae26c1070f452 0x99fdde80504a2812 0xcefb3aecc1cfd63a 0
155 0xdfdb1d2f210069fc 0x156aebf93ea1970d 0xf0c3a6475bccd8c6 1341
156 0xcd95ee070f10fab6 0xd0c7341de8f118a2 0x676d16eeb1df2028 0
157 0x9e35c4f187f47d3c 0x1ccb509a6bb9dc3a 0x04917f7f2ceb8327 0
158 0x81dc52ed29e18d19 0x4d07b75cd2a5110f 0x34f37e466867cb3d 256
159 0xd7efce3596abb725 0x510e76e91ce08249 0x5a08d4f662db37e5 0";

    /// 160 generated chunks (build ranges -64..320, 0..256 and 16..80, the persisted statuses EMPTY, BIOMES,
    /// TERRAIN and FEATURES, primed or unprimed WG heightmaps, gen depths 255/480/1024, NaN gradients) give Java's
    /// gradients, blocks and all six heightmaps; 91 of them erode 34659 blocks. Every 8th uses erodible soil only, and
    /// every 16th puts steep soil at the bottom of a 16..80 chunk, where `findSolid` reads void air below the build
    /// range and copies it up the column (about 12600 blocks), as Java does.
    #[test]
    fn surface_erosion_matches_6dd607eb_java_on_26_3_chunks() {
        let mut lines = JAVA_SURFACE.lines();
        let mut erodible = [false; NAMES.len()];
        for (i, line) in lines.by_ref().take(NAMES.len()).enumerate() {
            let fields: Vec<_> = line.split(' ').collect();
            assert_eq!(fields[..3], ["p", i.to_string().as_str(), NAMES[i]]);
            let flag = |k: usize| fields[k] == "true";
            erodible[i] = flag(3);
            let state = State(i as u32);
            assert_eq!(flag(4), state.blocks_motion_in_heightmap(), "{line}");
            assert_eq!(
                flag(5),
                state.blocks_motion_in_heightmap() || state.has_fluid()
            );
            assert_eq!(flag(6), !state.is_air());
        }
        // 26.3's #minecraft:dirt no longer holds grass: dirt, snow block and coarse dirt only.
        assert_eq!(erodible.iter().filter(|&&e| e).count(), 3);
        assert!(erodible[4] && erodible[5] && erodible[8] && !erodible[7]);

        let blocks = registry();
        let terrains = TerrainRegistry::new();
        let soil = [4, 4, 4, 5, 5, 8, 7, 6, 9, 2];
        let depths = [255, 480, 1024];
        let amps = [0.0f32, 0.004, 0.012, 0.03, 0.1];
        let ranges = [(-64, 384), (-64, 384), (0, 256), (16, 64)];
        let stages = [
            PreLightChunkStage::Terrain,
            PreLightChunkStage::Terrain,
            PreLightChunkStage::Terrain,
            PreLightChunkStage::Empty,
            PreLightChunkStage::Biomes,
            PreLightChunkStage::Features,
        ];
        let (mut scenarios, mut eroding, mut total) = (0, 0, 0);
        for (s, line) in lines.enumerate() {
            let mut rng = Lcg((s as u64)
                .wrapping_mul(0x9E3779B97F4A7C15)
                .wrapping_add(175));
            let mut range = ranges[rng.next(4) as usize];
            let pure = s % 8 == 3;
            if s % 16 == 3 {
                range = ranges[3];
            }
            let (min_y, height) = range;
            let max_y = min_y + height;
            let stage = stages[rng.next(6) as usize];
            let prime = rng.next(2) == 0;
            let gen_depth = depths[rng.next(3) as usize];
            let mut base = min_y - 8 + rng.next(height + 16);
            let mut sea = if rng.next(4) == 0 {
                min_y + rng.next(height)
            } else {
                base - 10 + rng.next(20)
            };
            let water = rng.next(2) == 0;
            let mut amp = amps[rng.next(5) as usize];
            if s % 16 == 3 {
                (base, sea, amp) = (min_y + 2 + (base & 7), min_y, 0.1);
            }
            let mut heights = [0.0f32; 18 * 18];
            for h in &mut heights {
                *h = 0.3 + (rng.next(2001) - 1000) as f32 / 1000.0 * amp;
            }
            if s % 16 == 15 {
                for _ in 0..6 {
                    heights[rng.next(18 * 18) as usize] = f32::NAN;
                }
            }
            let mut noise = NoiseData::new(&terrains);
            for z in NoiseData::MIN..NoiseData::MAX {
                for x in NoiseData::MIN..NoiseData::MAX {
                    let mut sample = NoiseSample::new(&terrains);
                    sample.height_noise = heights[((z + 1) * 18 + x + 1) as usize];
                    noise.set_noise(x, z, &sample);
                }
            }
            let mut terrain = TerrainData::new(TerrainLevels::default());
            terrain.accept(&noise);
            let pos = ChunkPos::new(rng.next(9) - 5, rng.next(9) - 5);
            let mut chunk = TerrainChunk::new(pos, min_y, height, &blocks, Properties).unwrap();
            chunk.set_generation_stage(stage);
            for z in 0..16 {
                for x in 0..16 {
                    let top = base + rng.next(13) - 6;
                    let depth = rng.next(26);
                    for y in min_y..max_y {
                        let id = if y <= top - depth {
                            2
                        } else if y <= top {
                            soil[rng.next(if pure { 5 } else { 10 }) as usize]
                        } else if water && y <= sea {
                            3
                        } else {
                            0
                        };
                        if id != 0 {
                            chunk.set_block_raw(x, y, z, BlockStateId(id)).unwrap();
                        }
                    }
                }
            }
            if prime {
                chunk
                    .prime_heightmaps(&[HeightmapType::WorldSurfaceWg, HeightmapType::OceanFloorWg])
                    .unwrap();
            }
            let mut before = Vec::new();
            for z in 0..16 {
                for x in 0..16 {
                    before.extend((min_y..max_y).map(|y| chunk.block(x, y, z)));
                }
            }
            apply(&terrain, &mut chunk, gen_depth, sea, |state| {
                erodible[state.0 as usize]
            })
            .unwrap();

            let norm = 55.0 * (gen_depth as f32 / 255.0);
            let mut grads = 0xcbf29ce484222325u64;
            let mut block_hash = 0xcbf29ce484222325u64;
            let mut changed = 0;
            let mut index = 0;
            for z in 0..16 {
                for x in 0..16 {
                    let g = terrain.normalized_gradient(x, z, norm);
                    grads = fold(grads, if g.is_nan() { 0x7fc0_0000 } else { g.to_bits() });
                    for y in min_y..max_y {
                        let state = chunk.block(x, y, z);
                        changed += i32::from(state != before[index]);
                        index += 1;
                        block_hash = fold(block_hash, state.0);
                    }
                }
            }
            let mut maps = 0xcbf29ce484222325u64;
            for kind in HeightmapType::ALL {
                let Some(map) = chunk.heightmap(kind) else {
                    maps = fold(maps, u32::MAX);
                    continue;
                };
                for z in 0..16 {
                    for x in 0..16 {
                        maps = fold(maps, map.first_available(x, z).unwrap() as u32);
                    }
                }
            }
            let rust = format!("{s} 0x{grads:016x} 0x{block_hash:016x} 0x{maps:016x} {changed}");
            assert_eq!(rust, line, "scenario {s} ({stage:?}, {min_y}..{max_y})");
            assert_eq!(chunk.generation_stage(), stage);
            scenarios += 1;
            eroding += i32::from(changed > 0);
            total += changed;
        }
        assert_eq!((scenarios, eroding, total), (160, 91, 34659));
    }

    /// The water oracle's palette: air, void air, stone, water (level 0, 1, 2), oak leaves, kelp plant, sand, cave air.
    const WATER_NAMES: [(&str, Option<&str>); 10] = [
        ("minecraft:air", None),
        ("minecraft:void_air", None),
        ("minecraft:stone", None),
        ("minecraft:water", Some("0")),
        ("minecraft:water", Some("1")),
        ("minecraft:water", Some("2")),
        ("minecraft:oak_leaves", None),
        ("minecraft:kelp_plant", None),
        ("minecraft:sand", None),
        ("minecraft:cave_air", None),
    ];

    /// Heightmap facts of `WATER_NAMES` (checked against the oracle's palette lines).
    #[derive(Clone, Copy)]
    struct WaterState(u32);

    impl HeightmapState for WaterState {
        fn is_air(self) -> bool {
            matches!(self.0, 0 | 1 | 9)
        }

        fn is_air_block(self) -> bool {
            self.0 == 0
        }

        fn has_fluid(self) -> bool {
            matches!(self.0, 3..=5 | 7)
        }

        fn blocks_motion_in_heightmap(self) -> bool {
            matches!(self.0, 2 | 6 | 8)
        }

        fn blocks_motion_in_heightmap_no_leaves(self) -> bool {
            matches!(self.0, 2 | 8)
        }
    }

    struct WaterProperties;

    impl TerrainBlockStates for WaterProperties {
        type State = WaterState;

        fn heightmap_state(&self, id: BlockStateId) -> Self::State {
            WaterState(id.0)
        }
    }

    fn water_registry() -> BlockRegistry {
        let state = |(name, level): &(&str, Option<&str>)| match level {
            None => Tag::compound([("id", Tag::String((*name).into()))]),
            Some(level) => Tag::compound([
                ("id", Tag::String((*name).into())),
                (
                    "properties",
                    Tag::compound([("level", Tag::String((*level).into()))]),
                ),
            ]),
        };
        let mut defaults = Vec::new();
        for (i, (name, level)) in WATER_NAMES.iter().enumerate() {
            if matches!(level, None | Some("0")) {
                defaults.push((*name, Tag::Int(i as i32)));
            }
        }
        BlockRegistry::from_nbt(&Tag::compound([
            ("DataVersion", Tag::Int(DATA_VERSION)),
            ("states", Tag::List(WATER_NAMES.iter().map(state).collect())),
            ("defaults", Tag::compound(defaults)),
        ]))
        .unwrap()
    }

    /// `WaterSmoothingOracle` (`.local/terraforged/oracles/surface_erosion/run.sh`, `water.txt`): 6dd607eb's
    /// `Surface.smoothWater`, `shouldSmooth`, `isSmoothable` and `sameChunk` over real 26.3 ProtoChunks and heightmaps,
    /// Engine 0.3.0's TerrainTypes, and a region proxy over the 8 neighbouring ProtoChunks that records every read.
    const JAVA_WATER: &str = r"p 0 minecraft:air - true false false false false
p 1 minecraft:void_air - true false false false false
p 2 minecraft:stone - false true true true true
p 3 minecraft:water 0 false false true true true
p 4 minecraft:water 1 false false true true true
p 5 minecraft:water 2 false false true true true
p 6 minecraft:oak_leaves - false true true false true
p 7 minecraft:kelp_plant - false false true true true
p 8 minecraft:sand - false true true true true
p 9 minecraft:cave_air - true false false false false
t none false false
t deep_ocean false false
t ocean false false
t coast false false
t beach false false
t river true false
t lake false true
t wetland false false
t flats false false
t badlands false false
t plateau false false
t hills false false
t mountains false false
t mountain_chain false false
t volcano false false
t volcano_pipe false false
0 0xefeb65e1ad885998 0x9fd6da0aea8f3af4 218 0x2727fe491c22f39c 57
1 0x75f92444c1ea319c 0xdeb1289c94183700 2414 0x15fab0bb6153c8a6 0
2 0x331690e374ab0e53 0x2018bfd2a552bc3f 278 0x73c670685bafa0a8 48
3 0xfcd560da2a356391 0x015ea56499a84f11 329 0x80776b423a30f9ce 55
4 0x47fd78a383719b1b 0x5f9392b90c1cbc6c 155 0x9feaa503b72f247d 36
5 0xd130a25c18fce7f2 0xd570caba89c37606 2503 0x8c1b8ba8b7ada54a 4
6 0xaaedb02516a89dcb 0x73d827c023f8ccd4 172 0x171ae5e060803ac3 41
7 0x0cace2030f0e2696 0x39d30fb9fe8af610 734 0xd9d0de4f1177341d 153
8 0x3cd1e039535d6a91 0x79dd25c6a9300bda 319 0x0b6d2e39a43c9336 57
9 0xcb961a1b0e3db1eb 0x1462a5bc376f55aa 0 0xcbf29ce484222325 0
10 0xf2fce340589e6020 0xbf96438d15b4435d 179 0xecc47de99f394649 52
11 0xbd27703df4d5709f 0x3119b26467ce5460 273 0x21cf176489b90225 52
12 0xccc95857559cafe4 0x71b202eb0ed362c2 223 0xf770ad47e9441e92 45
13 0xb0ff751a010d6a93 0x2f7067c3ec9701e2 2250 0x14a8a8a469e1b5e4 19
14 0x566734e81ccd2d79 0x2aa30184d8bbb73f 260 0x894d2e4bd582b453 48
15 0x64d80da4011da7c8 0xd3d22ecfc88d2f66 725 0x8e32913064f81012 151
16 0xa3d86ef0ba4472a9 0x9b96e09751e9cea0 213 0x5e3a51cccc8bda23 36
17 0x8fe0adf109185087 0x15cb766c0fc43e44 2264 0xd3d627e6daa7441a 7
18 0x8c5b6a4ad960cf03 0x57631e9ec68aeb59 316 0xba01d3fbb00d90c0 50
19 0x74891be713770bd9 0x896267f7e84bb564 238 0xa3126691f2ffdfe7 52
20 0xd20787227a9eb051 0x044de70844e55b81 208 0x6950b59e023c2d35 39
21 0x4d34274f2c5281a9 0x202e2cab90baea4b 3070 0x671003b312b13ba1 0
22 0xf0930b00c8cd3a55 0x11cb12da77aae0be 200 0xc0f8e01723da0a1d 50
23 0x902d9dcdbcfec7d7 0x4f474d96c16aa2b8 738 0x8a757f2304de500e 156
24 0x39504ecd53babd21 0x1af3a06c055d72e8 264 0xcf54b118e31a24b8 46
25 0x4d6df55b3089c657 0x5b9f49acf943a6ed 0 0xcbf29ce484222325 0
26 0xc6f938f99d014748 0x6f63b69e79690df1 272 0x3433280cd185c87e 55
27 0x2aa27bf262778ee8 0xce33acabba6c2d31 421 0x85f59c81ffde4f49 52
28 0x3546203fe5232009 0x2ea5490718f17758 214 0x71a362e41deedb38 46
29 0x73a3beb3fae45134 0x7d05b56fec0860b3 2166 0x65c6f34279dacade 34
30 0x35ab8b773a585961 0x65cb18e15af1542b 168 0x701aa68337a2dc06 51
31 0x8490fb3b617f015d 0xfa7dab3515e900a8 810 0xc8ca566ec042d557 152
32 0xbda44d290160c6f7 0xa767e07635dc2101 367 0xc225b7932aeb921b 52
33 0x5b69c867c4abb173 0x4c9a1c925a4be2c2 2441 0x7d6ed59d5b69ae5b 25
34 0x324bd63eb1fb94ca 0xaecd2235d539f353 274 0xbd6d49419227590c 58
35 0x6168930e555b8153 0x31234df92c5f053a 162 0x986f5bf2d1b443a1 53
36 0x8db1b775ef47be04 0x09807175e279c3d4 590 0xb39029781959a1e6 65
37 0xb2a421178fee0232 0x499577905041ca87 1883 0x5f8eeb1d01c1e337 30
38 0xba8d8781a5d11c89 0x81cd1763487ae0c3 259 0xf54a808847a7c0a5 52
39 0x117b49ccc2241f7a 0xc6f9ec526ab65022 467 0xe97355123b4bd756 147
40 0x87ed5872faefccd4 0xee94df3a7d9508db 199 0xfe585c0f00bbc5e7 58
41 0xe4b0660f2f6fb837 0x1686a4f7b7d6cc70 0 0xcbf29ce484222325 0
42 0x7ccdbf53b8e27852 0xe53ed79cfb7b1ce5 343 0xf87a1b806eede3d1 44
43 0xf8a0a7be3f0826fd 0x6d2dceae244d00c9 262 0x1eebdc7189f6f3e4 49
44 0x04b9f8ca28f7f8cc 0x626b267ce8ce0421 210 0x0dbbd68684fde3f9 44
45 0xfb6b398b547c6163 0x1944be2d20736baa 2478 0x28bcd34430176ea0 0
46 0xe4f2e79fc307bb07 0x49c7bcdd1cd3944f 180 0xd2167d4341e5cdd3 50
47 0xa1255f5bd83549ec 0x25d4919ca92b5420 852 0x43b0246a0c806f1a 160
48 0x8099deedbb84f640 0x02cbebcb446d3d50 412 0xa95b368772c5685e 53
49 0x49ca7a73c8cf3c02 0x90b11ed579c87aba 2674 0x3ada46dab7f9543c 14
50 0x1ec1d4d11eb6066a 0x71a608e93e610d07 564 0x9e432fdf542e3de0 62
51 0x1fd49ba10973e9d1 0x21f32c98b1096ecd 142 0x42321a5d331c1a74 43
52 0x2a63f7215c94b5d6 0x66203b93c0e7dd83 422 0xd8ecc212448066eb 40
53 0x033d7430dd97ffd0 0xadde5a6bb45388c5 2238 0x6dc310b1fa677652 5
54 0xb43dcb2c6e5b0919 0x35e37765073f6439 315 0x02a89e7bf1fb7cfa 41
55 0x01f8402973cfaea7 0xadb8b5f2195cb11d 638 0x1b2cda5fc3e138c2 160
56 0xbe988caa46ff919c 0x94c6b80dfa64f8b7 362 0x41bd79c14b2c8f89 54
57 0x26b2e983ca18d19c 0x2e8c9722f39cd648 0 0xcbf29ce484222325 0
58 0xc4faa02a8a796034 0xc5ebb6a24886a41b 142 0x1eaf2bada2b30ea2 44
59 0x96688729881db630 0xc4dacb45f24f6715 155 0x585ff82c60e54883 43
60 0x2b0bcdfc1a9ea38c 0x225139e8be257ffc 339 0xb23ae9bf4327ff75 57
61 0x479656f0d166585e 0x95d7065590b25545 2493 0xaed25952ed8e07e3 47
62 0xc77217699e94dea4 0xee55e8fde763bb83 313 0x75f5e50ef9293404 46
63 0xf70962b2c848e1f0 0x93ce31b6b16712bb 593 0x9ec940481c4d9004 143
64 0x46c42c46b268ec36 0x3bf66fba8693b21e 269 0xf3dfad0813f8f7e8 40
65 0xb5f0667788319523 0xc67cb4157134d28a 2082 0xbc0f2a0cbbdfd0ad 18
66 0xeeef5570dc3511df 0x112708f08fcb3c84 324 0x34cf4426030bf566 55
67 0xa9efd08c2af98b7a 0x2e43229e24920fbd 318 0x4cf47efad04d2f0b 51
68 0x3ccad4350d2dc04e 0xe89ce79a3241ea6b 210 0x90d7bbda67d8fb3a 52
69 0x23bb257d7624562f 0x57f8fa4992301136 2377 0xfe5e5e62c9e2cf43 33
70 0x6e40f9709f1ea35c 0xef5cb69920b4b56f 163 0x5e9eb06fc1df35af 49
71 0xfdd4a74867910397 0xf6e014f74f2f092e 447 0xee722012391d72ff 160
72 0x1ae0beb7acc14609 0x0fc6f51086a184c7 273 0x88b0bfcc4c7af094 52
73 0xc95864a6c74582ff 0x4a6c0816eb906c6f 0 0xcbf29ce484222325 0
74 0x583cf58daba75803 0x41f2bfab373ab9b1 263 0xb1239d1efe0455ff 56
75 0x020170cc4bda2245 0xcf1c73f4950b8b8f 283 0x8c0e36337872e51b 57
76 0x6b4168648578551b 0x9a6fbf976ef6a84e 219 0xfd65e296f931ec2f 46
77 0x7d0dee6d460377c7 0x3a34ea083d234d3d 2625 0x2cc664e96df1e887 8
78 0xbe32104dbcfb5be0 0xa42d1add74db5dfc 196 0xf47f1ec98c578f48 50
79 0xea6c98a65575e557 0x7479ef3c66c99730 547 0x1a1658aba249fc5f 165
80 0x3d2cdbf4ae39d88b 0x3c1a30d68287123b 493 0xa2e3eef48e4ec6a7 59
81 0x940bb19f6d378fb5 0x6d46f47daa0d5dd9 2205 0x55d67abdb089835a 7
82 0x78a42addcb986e0f 0xe86f832d6669d431 220 0xe19ec7903b66fd70 46
83 0x35712510c71976fe 0x9d569a6a9b1c2649 142 0x702788beffc0840d 47
84 0xb1e7a671d9d7fc45 0x572fa6d305fbba35 260 0xb6e3d1a213cf9cbc 52
85 0x6ce9225a0d75d527 0xceb2804f95c412a0 2665 0x236461fbce77aed1 1
86 0x9ab40c53fb3bafa0 0xe0cf8b6eb0f99c4f 206 0xda9fc7d5602a4750 51
87 0xede3f0289ec9fe81 0x56405364f0431ba4 743 0x7b89fbd82cfc5a42 143
88 0xf6028812844300fb 0xe54d01f0da656352 204 0x1dab1cb53a113662 48
89 0x9085c95cc0156759 0x4a6c0816eb906c6f 0 0xcbf29ce484222325 0
90 0x8f7477e1058f3895 0xe8e54c7645d7283f 358 0xc4eca368a15d8be1 55
91 0x1149d383d092e3d5 0x9b37cbbfd1b02b8d 218 0xdea36a56770810f8 48
92 0x4b6b6b68f9e2f8cb 0x321a95a5b3be266f 393 0x8948e9009a52956d 55
93 0xbee3961cc79fb8b7 0x44497669638a3796 2527 0xcaea35486330ee53 0
94 0xa784dce82686a8d1 0x9427f7cccb00b1e4 334 0x735e3f370d9aed3f 48
95 0x1a6afcb233ee8f13 0x27d10f821087adac 810 0x23fd18797144cd14 145
96 0xef1612342a972cf2 0xd202bc89eaf6f19e 306 0xa693a042c720da39 52
97 0xa924b3478e1513c8 0xa5cbb7cd2c6bd49d 2540 0xa2d3697647356de7 44
98 0x6aa038aa913da1f7 0x9e3596ffc758bae3 126 0xa9cb3832cd1d3288 37
99 0xe00865fd045360d0 0xe700117cfde03af7 427 0x56cfe29d2e7f78e4 59
100 0xc8bebd0d6f29e5e9 0xbb0b7eab4e79de89 169 0xfe5150292aca3b82 48
101 0x707b89d41e336990 0xc4a5f7e6f12c10f4 1534 0xfa568c5338c08cdc 26
102 0xe7dcb25b3de3b74e 0x1467de311955fd50 276 0xe1757f4abeb732a7 61
103 0x4bcc5fc44d9d83f6 0x8ab7baacb540d9bd 838 0x0e09a291f8f16fd5 150
104 0x47cb5b5e566e6936 0x9577680b62a11050 185 0x324466e377c630d7 53
105 0xfcb21458afbddf7d 0xef808aa8e1e542aa 0 0xcbf29ce484222325 0
106 0x1ff0674d2ea5e47a 0xab26b3921cc4736a 235 0x90d4e95d0b09dd57 46
107 0xf58b4563f42ad2e1 0x7ccc14e8b1468d98 279 0xcfc2b31df06d81d4 58
108 0x7546a0977ad2f136 0x81866d933809a983 239 0xba278b2a14a17816 53
109 0x341121a591c0d16f 0xefd8ebcff639e646 2199 0x692c1ad17f87a185 7
110 0xf170a32686f395b8 0x080613539ec87cda 226 0x08cd02cfd77530b1 45
111 0x862383b3ad33316d 0x893f6effc03cdb21 803 0x60f91c6e31967b0e 158
112 0xd349eb65f36cd84c 0x7b0f9b57e28105d7 72 0x51b2549037e6ea3f 36
113 0x3d2c527f6d82fa71 0xfcb37a83d1763d31 1967 0xd64ddbf289cb00b5 15
114 0xe8f18d9fb6037e43 0x30d96c27fa4c3b2e 130 0x6747f0fffa2f7937 48
115 0x29b7ee10afb54536 0xf4f1bee79382b08c 315 0x2ba96f94d147131f 61
116 0x202e978f35b21887 0x20c2574ac1750d31 259 0x3ea34dbe0eac8b36 63
117 0x803cc35aeca7fe92 0x030c0573d988ccff 2309 0x254dfdb4582f8415 32
118 0x35e9779c6973e1c9 0x9325a4a69d30a4c7 183 0x272d1f632c255f36 47
119 0xd9dec4ded000da68 0xca24d512e63f5794 595 0x29f5ed9a8bc56551 156
120 0xa9b9761962ac827f 0xf655d8c3c8f71de6 238 0x2a631d69447d145c 40
121 0x6ebb9498ad09d86b 0xe49be268cce6fbaa 0 0xcbf29ce484222325 0
122 0x59dd0583779fc59e 0xd3020afbed2e7c8c 396 0x50f3c0dd1cf8059c 44
123 0xa4226e32e1127531 0x68c3da9e32d2b69e 210 0x37bd9698b19b220d 41
124 0x61de8d7c5301f54b 0xcf3e20b66828c28a 208 0x9d4f4ff717e752e4 55
125 0xd08af0e0399269a3 0x3096dbbb68ec9daa 2145 0x18e1226b1ecd93e6 15
126 0x703d392956eed49b 0x29718410aae97142 194 0x1df8c3578d40610b 39
127 0x622f137b53a7772c 0x3140b4cdaeea1357 825 0x1d9a3473472a5c37 161";

    /// Java's terrain names in `TerrainType` order (the oracle's `TerrainType.get(name)`).
    const TERRAIN_NAMES: [&str; 16] = [
        "none",
        "deep_ocean",
        "ocean",
        "coast",
        "beach",
        "river",
        "lake",
        "wetland",
        "flats",
        "badlands",
        "plateau",
        "hills",
        "mountains",
        "mountain_chain",
        "volcano",
        "volcano_pipe",
    ];

    /// 128 generated 3x3 chunk grids (build ranges -64..320, 0..256 and 16..80, the persisted statuses TERRAIN,
    /// FEATURES, EMPTY and BIOMES, no / MOTION_BLOCKING_NO_LEAVES / WG heightmaps primed, chunk positions -5..3) with
    /// source and flowing water, shores topped with air or cave air, leaves over water and kelp tops, and per-column
    /// terrains and river values (0, -0.0, 0.001, NaN, 1) give Java's blocks, all six heightmaps (absent ones too: a
    /// chunk with no smoothable column is never primed) and the region reads (count and ordered positions). Every 8th
    /// grid is all river with river 0; in every 4th from 1 shores are rare, so most searches cover the whole disc;
    /// every 16th from 9 has no river or lake, so nothing is read or primed. 116 grids smooth 6830 columns over
    /// 88165 region reads.
    #[test]
    fn water_smoothing_matches_6dd607eb_java_on_26_3_chunks() {
        let mut lines = JAVA_WATER.lines();
        for (i, line) in lines.by_ref().take(WATER_NAMES.len()).enumerate() {
            let fields: Vec<_> = line.split(' ').collect();
            let (name, level) = WATER_NAMES[i];
            let i_text = i.to_string();
            assert_eq!(
                fields[..4],
                ["p", i_text.as_str(), name, level.unwrap_or("-")]
            );
            let flag = |k: usize| fields[k] == "true";
            let state = WaterState(i as u32);
            assert_eq!(flag(4), state.is_air(), "{line}");
            for (k, kind) in [
                HeightmapType::OceanFloorWg,
                HeightmapType::MotionBlocking,
                HeightmapType::MotionBlockingNoLeaves,
                HeightmapType::WorldSurface,
            ]
            .into_iter()
            .enumerate()
            {
                assert_eq!(flag(5 + k), kind.is_opaque(state), "{line} {kind:?}");
            }
        }
        let terrains = TerrainRegistry::new();
        for (name, line) in TERRAIN_NAMES.iter().zip(lines.by_ref()) {
            let terrain = terrains.get_by_name(name).unwrap();
            let rust = format!("t {name} {} {}", terrain.is_river(), terrain.is_lake());
            assert_eq!(rust, line);
        }

        let blocks = water_registry();
        let ranges = [(-64, 384), (0, 256), (16, 64)];
        let stages = [
            PreLightChunkStage::Terrain,
            PreLightChunkStage::Features,
            PreLightChunkStage::Empty,
            PreLightChunkStage::Biomes,
        ];
        let rivers = [0.0f32, 0.0, 0.0, -0.0, 0.001, f32::NAN, 1.0];
        let (mut scenarios, mut smoothing, mut total, mut all_reads) = (0, 0, 0, 0);
        for (s, line) in lines.enumerate() {
            let mut rng = Lcg((s as u64)
                .wrapping_mul(0x9E3779B97F4A7C15)
                .wrapping_add(177));
            let (min_y, height) = ranges[rng.next(3) as usize];
            let max_y = min_y + height;
            let stage = stages[rng.next(4) as usize];
            let prime = rng.next(3);
            let (cx, cz) = (rng.next(9) - 5, rng.next(9) - 5);
            let sea = min_y + 4 + rng.next(height - 8);
            let mut grid = Vec::new();
            for gz in -1..=1 {
                for gx in -1..=1 {
                    let pos = ChunkPos::new(cx + gx, cz + gz);
                    let mut chunk =
                        TerrainChunk::new(pos, min_y, height, &blocks, WaterProperties).unwrap();
                    for z in 0..16 {
                        for x in 0..16 {
                            let kind = rng.next(if s % 4 == 1 { 400 } else { 10 });
                            let floor = sea - 1 - rng.next(6);
                            let top = sea + rng.next(3) - 1;
                            for y in min_y..max_y {
                                let id = if kind == 6 || kind == 7 {
                                    if y < top {
                                        2
                                    } else if y == top {
                                        if kind == 7 { 8 } else { 2 }
                                    } else if y == top + 1 && kind == 7 {
                                        9
                                    } else {
                                        0
                                    }
                                } else if y <= floor {
                                    2
                                } else if y < sea {
                                    3
                                } else if y == sea {
                                    match kind {
                                        5 => 4,
                                        9 => 7,
                                        _ => 3,
                                    }
                                } else if y == sea + 1 && kind == 8 {
                                    6
                                } else {
                                    0
                                };
                                if id != 0 {
                                    chunk.set_block_raw(x, y, z, BlockStateId(id)).unwrap();
                                }
                            }
                        }
                    }
                    grid.push(chunk);
                }
            }
            let mut center = grid.remove(4);
            center.set_generation_stage(stage);
            match prime {
                1 => center
                    .prime_heightmaps(&[HeightmapType::MotionBlockingNoLeaves])
                    .unwrap(),
                2 => center
                    .prime_heightmaps(&[HeightmapType::WorldSurfaceWg, HeightmapType::OceanFloorWg])
                    .unwrap(),
                _ => {}
            }
            let mut noise = NoiseData::new(&terrains);
            for z in 0..16 {
                for x in 0..16 {
                    let pick = rng.next(4);
                    let mut name = if pick < 2 {
                        TERRAIN_NAMES[5 + pick as usize]
                    } else {
                        TERRAIN_NAMES[rng.next(16) as usize]
                    };
                    if s % 16 == 9 {
                        name = "hills";
                    }
                    let mut river = rivers[rng.next(7) as usize];
                    if s % 8 == 7 {
                        (name, river) = ("river", 0.0);
                    }
                    let mut sample = NoiseSample::new(&terrains);
                    sample.river_noise = river;
                    sample.terrain_type = terrains.get_by_name(name).unwrap();
                    noise.set_noise(x, z, &sample);
                }
            }
            let mut terrain = TerrainData::new(TerrainLevels::default());
            terrain.accept(&noise);
            let mut before = Vec::new();
            for z in 0..16 {
                for x in 0..16 {
                    before.extend((min_y..max_y).map(|y| center.block(x, y, z)));
                }
            }
            let (mut reads, mut read_hash) = (0, 0xcbf29ce484222325u64);
            smooth_water(
                &terrain,
                &mut center,
                BlockStateId(3),
                BlockStateId(5),
                |x, y, z| {
                    reads += 1;
                    for v in [x, y, z] {
                        read_hash = fold(read_hash, v as u32);
                    }
                    let (gx, gz) = ((x >> 4) - cx, (z >> 4) - cz);
                    assert!((-1..=1).contains(&gx) && (-1..=1).contains(&gz) && (gx, gz) != (0, 0));
                    let index = ((gz + 1) * 3 + gx + 1) as usize;
                    let chunk = &grid[if index > 4 { index - 1 } else { index }];
                    Ok(chunk.block(x & 15, y, z & 15))
                },
            )
            .unwrap();

            let mut block_hash = 0xcbf29ce484222325u64;
            let mut changed = 0;
            let mut index = 0;
            for z in 0..16 {
                for x in 0..16 {
                    for y in min_y..max_y {
                        let state = center.block(x, y, z);
                        changed += i32::from(state != before[index]);
                        index += 1;
                        block_hash = fold(block_hash, state.0);
                    }
                }
            }
            let mut maps = 0xcbf29ce484222325u64;
            for kind in HeightmapType::ALL {
                let Some(map) = center.heightmap(kind) else {
                    maps = fold(maps, u32::MAX);
                    continue;
                };
                for z in 0..16 {
                    for x in 0..16 {
                        maps = fold(maps, map.first_available(x, z).unwrap() as u32);
                    }
                }
            }
            let rust = format!(
                "{s} 0x{block_hash:016x} 0x{maps:016x} {reads} 0x{read_hash:016x} {changed}"
            );
            assert_eq!(rust, line, "scenario {s} ({stage:?}, {min_y}..{max_y})");
            assert_eq!(center.generation_stage(), stage);
            scenarios += 1;
            smoothing += i32::from(changed > 0);
            total += changed;
            all_reads += reads;
        }
        assert_eq!(
            (scenarios, smoothing, total, all_reads),
            (128, 116, 6830, 88165)
        );
    }

    /// A failing region read stops the pass at once: that column keeps its source water (Java's exception leaves
    /// the chunk as it is at the throw).
    #[test]
    fn water_smoothing_returns_region_errors_before_writing() {
        let blocks = water_registry();
        let terrains = TerrainRegistry::new();
        let mut chunk =
            TerrainChunk::new(ChunkPos::new(-1, 2), 0, 64, &blocks, WaterProperties).unwrap();
        chunk.set_block_raw(0, 10, 0, BlockStateId(3)).unwrap();
        let mut noise = NoiseData::new(&terrains);
        let mut sample = NoiseSample::new(&terrains);
        sample.river_noise = 0.0;
        sample.terrain_type = terrains.get_by_name("lake").unwrap();
        noise.set_noise(0, 0, &sample);
        let mut terrain = TerrainData::new(TerrainLevels::default());
        terrain.accept(&noise);
        let mut reads = Vec::new();
        let result = smooth_water(
            &terrain,
            &mut chunk,
            BlockStateId(3),
            BlockStateId(5),
            |x, y, z| {
                reads.push((x, y, z));
                Err(TerrainError::InvalidVolume)
            },
        );
        assert!(matches!(result, Err(TerrainError::InvalidVolume)));
        // The first disc offset (dx 0, dz -6) from block (-16, 32) is in chunk (-1, 1): a region read.
        assert_eq!(reads, [(-16, 10, 26)]);
        assert_eq!(chunk.block(0, 10, 0), BlockStateId(3));
    }
}
