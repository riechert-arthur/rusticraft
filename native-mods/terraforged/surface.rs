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
    terraforged_terrain_data::TerrainData,
};
use crate::{heightmap::HeightmapType, registry::BlockStateId};

/// TerraForged 6dd607eb's `Surface.apply` (the 1.18.2-0.3.1 jar's bytecode is the same): the erosion half of
/// `SurfaceDecorator.decoratePost`. Each column whose OCEAN_FLOOR_WG height is at or above `sea_level` and whose
/// gradient, normalised by `55 * gen_depth / 255`, is not below 0.6 (NaN erodes, as Java's `<` test) loses its
/// erodible top: from that height down to the first non-erodible state found strictly above `max(0, y - 20)`
/// (`findSolid`), every block becomes that state. Nothing happens when the top block is not erodible or no
/// non-erodible state is found.
///
/// `gen_depth` is the generator's `getGenDepth()` and `sea_level` its `getSeaLevel()`. `is_erodible` is Java's
/// `isErodible`: `#minecraft:dirt` or `#minecraft:snow`. With 26.3's tags that is dirt, coarse dirt, rooted dirt and
/// the snow blocks, but not grass, podzol, mycelium, moss or mud, which 1.18's `#minecraft:dirt` held. `terrain` is the
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
}
