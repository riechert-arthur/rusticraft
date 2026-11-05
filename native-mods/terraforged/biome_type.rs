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
use super::terraforged_engine::{BiomeType, Cell};

// Engine computes these in `BiomeTypeLoader.getRanges` from the type map: per type, the smallest and largest
// `x / 255f` (temperature) and `y / 255f` (moisture) of the squares it covers, as raw f32 bits.
const RANGES: [[f32; 4]; 11] = [
    [
        f32::from_bits(0x3f4acacb),
        f32::from_bits(0x3f800000),
        f32::from_bits(0x3f1d9d9e),
        f32::from_bits(0x3f800000),
    ],
    [
        f32::from_bits(0x3eeeeeef),
        f32::from_bits(0x3f800000),
        f32::from_bits(0x3e20a0a1),
        f32::from_bits(0x3f29a9aa),
    ],
    [
        f32::from_bits(0x3efefeff),
        f32::from_bits(0x3f800000),
        f32::from_bits(0x00000000),
        f32::from_bits(0x3e868687),
    ],
    [
        f32::from_bits(0x3f0c8c8d),
        f32::from_bits(0x3f52d2d3),
        f32::from_bits(0x3f0c8c8d),
        f32::from_bits(0x3f52d2d3),
    ],
    [
        f32::from_bits(0x3ed4d4d5),
        f32::from_bits(0x3f55d5d6),
        f32::from_bits(0x3e2cacad),
        f32::from_bits(0x3f1c9c9d),
    ],
    [
        f32::from_bits(0x3e78f8f9),
        f32::from_bits(0x3f56d6d7),
        f32::from_bits(0x3d50d0d1),
        f32::from_bits(0x3ec8c8c9),
    ],
    [
        f32::from_bits(0x3e109091),
        f32::from_bits(0x3f169697),
        f32::from_bits(0x00000000),
        f32::from_bits(0x3e969697),
    ],
    [
        f32::from_bits(0x3ec6c6c7),
        f32::from_bits(0x3f3fbfc0),
        f32::from_bits(0x00000000),
        f32::from_bits(0x3e088889),
    ],
    [
        f32::from_bits(0x3e44c4c5),
        f32::from_bits(0x3f20a0a1),
        f32::from_bits(0x3d30b0b1),
        f32::from_bits(0x3edededf),
    ],
    [
        f32::from_bits(0x00000000),
        f32::from_bits(0x3e58d8d9),
        f32::from_bits(0x00000000),
        f32::from_bits(0x3e54d4d5),
    ],
    [
        f32::from_bits(0x00000000),
        f32::from_bits(0x3f7efeff),
        f32::from_bits(0x3b808081),
        f32::from_bits(0x3f800000),
    ],
];

// Each type's lookup colour (the first constructor arguments): `BiomeTypeLoader` gives every pixel of
// TerraForged's `biomes.png` the type whose lookup colour is nearest.
const LOOKUP_COLORS: [[u8; 3]; 11] = [
    [7, 83, 48],
    [151, 165, 39],
    [200, 113, 55],
    [10, 84, 109],
    [44, 137, 160],
    [179, 124, 6],
    [131, 112, 71],
    [199, 155, 60],
    [91, 143, 82],
    [147, 167, 172],
    [0, 0, 0],
];

// `getColor`: the colours of TerraForged's `biomes.txt`, which override every constructor default.
const COLORS: [[u8; 3]; 11] = [
    [52, 125, 63],
    [136, 173, 62],
    [230, 219, 156],
    [82, 140, 53],
    [93, 153, 72],
    [75, 179, 79],
    [194, 187, 132],
    [224, 203, 137],
    [77, 115, 59],
    [215, 219, 200],
    [239, 242, 237],
];

const ROW_OFFSETS: [u16; 257] = [
    0, 4, 9, 14, 19, 24, 29, 34, 39, 44, 49, 54, 60, 66, 73, 80, 87, 94, 101, 108, 115, 122, 129,
    136, 143, 150, 156, 162, 168, 174, 180, 186, 192, 198, 204, 210, 215, 220, 225, 230, 235, 241,
    247, 253, 260, 268, 275, 282, 290, 297, 304, 312, 319, 327, 334, 341, 347, 353, 360, 366, 372,
    379, 386, 392, 398, 404, 410, 416, 422, 427, 432, 437, 442, 447, 452, 457, 463, 468, 473, 478,
    483, 488, 494, 499, 504, 509, 514, 519, 524, 529, 534, 539, 544, 549, 554, 559, 564, 569, 574,
    579, 584, 589, 593, 597, 601, 605, 609, 613, 617, 621, 625, 629, 633, 636, 639, 642, 645, 648,
    651, 654, 657, 660, 663, 666, 669, 672, 675, 678, 681, 684, 687, 690, 693, 696, 699, 702, 705,
    708, 711, 714, 717, 721, 725, 729, 733, 737, 741, 745, 749, 753, 757, 761, 765, 769, 773, 777,
    781, 785, 789, 793, 797, 801, 805, 809, 813, 817, 821, 825, 829, 833, 837, 840, 843, 846, 849,
    852, 855, 858, 861, 864, 867, 870, 873, 876, 879, 882, 885, 888, 891, 894, 897, 900, 903, 906,
    909, 912, 915, 918, 921, 924, 927, 930, 933, 936, 939, 942, 945, 948, 951, 954, 957, 960, 962,
    964, 966, 968, 970, 972, 974, 976, 978, 980, 982, 984, 986, 988, 990, 992, 994, 996, 998, 1000,
    1002, 1004, 1006, 1008, 1010, 1012, 1014, 1016, 1018, 1020, 1022, 1024, 1026, 1028, 1030, 1032,
    1034, 1036, 1038, 1040, 1042, 1044, 1046, 1048, 1050,
];

// Engine's 256x256 type map (`BiomeTypeLoader.getTypeMap()`, row = moisture index, column = temperature index),
// run-length encoded: row `y`'s runs are `RUNS[2 * ROW_OFFSETS[y]..2 * ROW_OFFSETS[y + 1]]`, each an inclusive
// last x followed by a BiomeType ordinal. `ROW_OFFSETS` counts runs.
const RUNS: [u8; 2100] = [
    35, 9, 98, 6, 126, 7, 255, 2, 0, 10, 36, 9, 100, 6, 133, 7, 255, 2, 1, 10, 38, 9, 101, 6, 138,
    7, 255, 2, 2, 10, 40, 9, 104, 6, 144, 7, 255, 2, 3, 10, 41, 9, 107, 6, 147, 7, 255, 2, 4, 10,
    42, 9, 110, 6, 149, 7, 255, 2, 5, 10, 44, 9, 112, 6, 152, 7, 255, 2, 6, 10, 45, 9, 114, 6, 154,
    7, 255, 2, 7, 10, 46, 9, 116, 6, 156, 7, 255, 2, 8, 10, 46, 9, 118, 6, 159, 7, 255, 2, 9, 10,
    47, 9, 119, 6, 161, 7, 255, 2, 10, 10, 48, 9, 50, 8, 121, 6, 162, 7, 255, 2, 11, 10, 48, 9, 57,
    8, 123, 6, 163, 7, 255, 2, 12, 10, 48, 9, 61, 8, 75, 5, 125, 6, 164, 7, 255, 2, 13, 10, 49, 9,
    65, 8, 80, 5, 127, 6, 165, 7, 255, 2, 14, 10, 49, 9, 68, 8, 84, 5, 129, 6, 166, 7, 255, 2, 15,
    10, 49, 9, 70, 8, 87, 5, 130, 6, 167, 7, 255, 2, 16, 10, 49, 9, 71, 8, 92, 5, 131, 6, 168, 7,
    255, 2, 17, 10, 50, 9, 73, 8, 96, 5, 132, 6, 168, 7, 255, 2, 18, 10, 50, 9, 75, 8, 103, 5, 133,
    6, 169, 7, 255, 2, 19, 10, 50, 9, 77, 8, 111, 5, 134, 6, 170, 7, 255, 2, 20, 10, 50, 9, 78, 8,
    117, 5, 135, 6, 172, 7, 255, 2, 21, 10, 50, 9, 80, 8, 123, 5, 136, 6, 173, 7, 255, 2, 22, 10,
    51, 9, 81, 8, 129, 5, 137, 6, 175, 7, 255, 2, 23, 10, 51, 9, 83, 8, 136, 5, 139, 6, 176, 7,
    255, 2, 24, 10, 51, 9, 85, 8, 142, 5, 177, 7, 255, 2, 25, 10, 51, 9, 86, 8, 146, 5, 179, 7,
    255, 2, 26, 10, 52, 9, 88, 8, 151, 5, 180, 7, 255, 2, 27, 10, 52, 9, 89, 8, 159, 5, 181, 7,
    255, 2, 28, 10, 52, 9, 90, 8, 166, 5, 182, 7, 255, 2, 29, 10, 52, 9, 91, 8, 171, 5, 184, 7,
    255, 2, 30, 10, 52, 9, 92, 8, 175, 5, 186, 7, 255, 2, 31, 10, 53, 9, 94, 8, 178, 5, 187, 7,
    255, 2, 32, 10, 53, 9, 95, 8, 182, 5, 189, 7, 255, 2, 33, 10, 53, 9, 96, 8, 188, 5, 191, 7,
    255, 2, 34, 10, 53, 9, 97, 8, 192, 5, 255, 2, 35, 10, 53, 9, 98, 8, 194, 5, 255, 2, 36, 10, 54,
    9, 99, 8, 196, 5, 255, 2, 37, 10, 54, 9, 100, 8, 199, 5, 255, 2, 38, 10, 54, 9, 101, 8, 202, 5,
    255, 2, 39, 10, 54, 9, 103, 8, 204, 5, 206, 1, 255, 2, 40, 10, 54, 9, 104, 8, 205, 5, 209, 1,
    255, 2, 41, 10, 54, 9, 105, 8, 205, 5, 213, 1, 255, 2, 42, 10, 54, 9, 105, 8, 107, 4, 206, 5,
    215, 1, 255, 2, 43, 10, 54, 9, 106, 8, 108, 4, 109, 6, 206, 5, 219, 1, 255, 2, 44, 10, 54, 9,
    106, 8, 110, 4, 207, 5, 222, 1, 255, 2, 45, 10, 54, 9, 106, 8, 112, 4, 207, 5, 225, 1, 255, 2,
    46, 10, 54, 9, 106, 8, 113, 4, 114, 6, 207, 5, 227, 1, 255, 2, 47, 10, 54, 9, 107, 8, 115, 4,
    207, 5, 228, 1, 255, 2, 48, 10, 54, 9, 107, 8, 117, 4, 208, 5, 232, 1, 255, 2, 49, 10, 54, 9,
    107, 8, 118, 4, 119, 1, 208, 5, 235, 1, 255, 2, 50, 10, 54, 9, 107, 8, 120, 4, 208, 5, 237, 1,
    255, 2, 51, 10, 53, 9, 107, 8, 121, 4, 122, 6, 208, 5, 239, 1, 255, 2, 52, 10, 53, 9, 107, 8,
    123, 4, 209, 5, 241, 1, 255, 2, 53, 10, 107, 8, 124, 4, 125, 6, 209, 5, 242, 1, 255, 2, 54, 10,
    107, 8, 126, 4, 210, 5, 243, 1, 255, 2, 55, 10, 108, 8, 127, 4, 210, 5, 244, 1, 255, 2, 56, 10,
    108, 8, 128, 4, 129, 8, 210, 5, 246, 1, 255, 2, 57, 10, 108, 8, 130, 4, 210, 5, 247, 1, 255, 2,
    58, 10, 108, 8, 131, 4, 211, 5, 248, 1, 255, 2, 59, 10, 108, 8, 132, 4, 133, 6, 211, 5, 249, 1,
    255, 2, 60, 10, 109, 8, 133, 4, 134, 8, 211, 5, 250, 1, 255, 2, 61, 10, 109, 8, 135, 4, 211, 5,
    251, 1, 255, 2, 62, 10, 109, 8, 136, 4, 212, 5, 252, 1, 255, 2, 63, 10, 109, 8, 137, 4, 212, 5,
    252, 1, 255, 2, 64, 10, 109, 8, 139, 4, 212, 5, 253, 1, 255, 2, 65, 10, 109, 8, 140, 4, 212, 5,
    253, 1, 255, 2, 66, 10, 109, 8, 141, 4, 212, 5, 254, 1, 255, 2, 67, 10, 110, 8, 142, 4, 212, 5,
    255, 1, 68, 10, 110, 8, 143, 4, 213, 5, 255, 1, 69, 10, 110, 8, 144, 4, 213, 5, 255, 1, 70, 10,
    110, 8, 145, 4, 213, 5, 255, 1, 71, 10, 110, 8, 146, 4, 213, 5, 255, 1, 72, 10, 110, 8, 147, 4,
    213, 5, 255, 1, 73, 10, 110, 8, 148, 4, 213, 5, 255, 1, 74, 10, 110, 8, 149, 4, 150, 6, 213, 5,
    255, 1, 75, 10, 110, 8, 151, 4, 213, 5, 255, 1, 76, 10, 110, 8, 153, 4, 213, 5, 255, 1, 77, 10,
    110, 8, 154, 4, 213, 5, 255, 1, 78, 10, 110, 8, 156, 4, 213, 5, 255, 1, 79, 10, 110, 8, 158, 4,
    213, 5, 255, 1, 80, 10, 110, 8, 159, 4, 160, 8, 213, 5, 255, 1, 81, 10, 110, 8, 163, 4, 213, 5,
    255, 1, 82, 10, 110, 8, 165, 4, 213, 5, 255, 1, 83, 10, 110, 8, 167, 4, 213, 5, 255, 1, 84, 10,
    110, 8, 169, 4, 214, 5, 255, 1, 85, 10, 110, 8, 171, 4, 214, 5, 255, 1, 86, 10, 110, 8, 174, 4,
    214, 5, 255, 1, 87, 10, 110, 8, 176, 4, 214, 5, 255, 1, 88, 10, 110, 8, 179, 4, 214, 5, 255, 1,
    89, 10, 110, 8, 181, 4, 214, 5, 255, 1, 90, 10, 110, 8, 183, 4, 214, 5, 255, 1, 91, 10, 110, 8,
    187, 4, 214, 5, 255, 1, 92, 10, 110, 8, 190, 4, 214, 5, 255, 1, 93, 10, 110, 8, 192, 4, 214, 5,
    255, 1, 94, 10, 110, 8, 195, 4, 214, 5, 255, 1, 95, 10, 110, 8, 198, 4, 214, 5, 255, 1, 96, 10,
    110, 8, 200, 4, 213, 5, 255, 1, 97, 10, 110, 8, 203, 4, 213, 5, 255, 1, 98, 10, 110, 8, 208, 4,
    213, 5, 255, 1, 99, 10, 110, 8, 212, 4, 213, 5, 255, 1, 100, 10, 110, 8, 213, 4, 255, 1, 101,
    10, 110, 8, 213, 4, 255, 1, 102, 10, 110, 8, 213, 4, 255, 1, 103, 10, 111, 8, 213, 4, 255, 1,
    104, 10, 111, 8, 213, 4, 255, 1, 105, 10, 111, 8, 213, 4, 255, 1, 106, 10, 111, 8, 212, 4, 255,
    1, 107, 10, 111, 8, 212, 4, 255, 1, 108, 10, 111, 8, 212, 4, 255, 1, 109, 10, 111, 8, 212, 4,
    255, 1, 110, 10, 111, 8, 211, 4, 255, 1, 111, 10, 211, 4, 255, 1, 112, 10, 210, 4, 255, 1, 113,
    10, 210, 4, 255, 1, 114, 10, 209, 4, 255, 1, 115, 10, 209, 4, 255, 1, 116, 10, 209, 4, 255, 1,
    117, 10, 209, 4, 255, 1, 118, 10, 209, 4, 255, 1, 119, 10, 209, 4, 255, 1, 120, 10, 208, 4,
    255, 1, 121, 10, 208, 4, 255, 1, 122, 10, 208, 4, 255, 1, 123, 10, 208, 4, 255, 1, 124, 10,
    207, 4, 255, 1, 125, 10, 207, 4, 255, 1, 126, 10, 206, 4, 255, 1, 127, 10, 206, 4, 255, 1, 128,
    10, 206, 4, 255, 1, 129, 10, 206, 4, 255, 1, 130, 10, 205, 4, 255, 1, 131, 10, 205, 4, 255, 1,
    132, 10, 205, 4, 255, 1, 133, 10, 205, 4, 255, 1, 134, 10, 205, 4, 255, 1, 135, 10, 205, 4,
    255, 1, 136, 10, 204, 4, 255, 1, 137, 10, 204, 4, 255, 1, 138, 10, 204, 4, 255, 1, 139, 10,
    140, 3, 204, 4, 255, 1, 140, 10, 142, 3, 204, 4, 255, 1, 141, 10, 144, 3, 204, 4, 255, 1, 142,
    10, 146, 3, 203, 4, 255, 1, 143, 10, 148, 3, 203, 4, 255, 1, 144, 10, 151, 3, 203, 4, 255, 1,
    145, 10, 154, 3, 203, 4, 255, 1, 146, 10, 157, 3, 203, 4, 255, 1, 147, 10, 160, 3, 203, 4, 255,
    1, 148, 10, 162, 3, 203, 4, 255, 1, 149, 10, 166, 3, 202, 4, 255, 1, 150, 10, 169, 3, 202, 4,
    255, 1, 151, 10, 174, 3, 202, 4, 255, 1, 152, 10, 179, 3, 202, 4, 255, 1, 153, 10, 183, 3, 202,
    4, 255, 1, 154, 10, 191, 3, 201, 4, 255, 1, 155, 10, 198, 3, 201, 4, 255, 1, 156, 10, 201, 3,
    213, 0, 255, 1, 157, 10, 201, 3, 219, 0, 255, 1, 158, 10, 201, 3, 227, 0, 255, 1, 159, 10, 201,
    3, 231, 0, 255, 1, 160, 10, 201, 3, 233, 0, 255, 1, 161, 10, 201, 3, 239, 0, 255, 1, 162, 10,
    201, 3, 242, 0, 255, 1, 163, 10, 201, 3, 244, 0, 255, 1, 164, 10, 201, 3, 246, 0, 255, 1, 165,
    10, 201, 3, 249, 0, 255, 1, 166, 10, 201, 3, 251, 0, 255, 1, 167, 10, 202, 3, 252, 0, 255, 1,
    168, 10, 202, 3, 254, 0, 255, 1, 169, 10, 202, 3, 255, 0, 170, 10, 202, 3, 255, 0, 171, 10,
    202, 3, 255, 0, 172, 10, 202, 3, 255, 0, 173, 10, 202, 3, 255, 0, 174, 10, 202, 3, 255, 0, 175,
    10, 202, 3, 255, 0, 176, 10, 203, 3, 255, 0, 177, 10, 203, 3, 255, 0, 178, 10, 203, 3, 255, 0,
    179, 10, 203, 3, 255, 0, 180, 10, 203, 3, 255, 0, 181, 10, 203, 3, 255, 0, 182, 10, 204, 3,
    255, 0, 183, 10, 204, 3, 255, 0, 184, 10, 204, 3, 255, 0, 185, 10, 204, 3, 255, 0, 186, 10,
    204, 3, 255, 0, 187, 10, 204, 3, 255, 0, 188, 10, 205, 3, 255, 0, 189, 10, 205, 3, 255, 0, 190,
    10, 205, 3, 255, 0, 191, 10, 205, 3, 255, 0, 192, 10, 205, 3, 255, 0, 193, 10, 206, 3, 255, 0,
    194, 10, 206, 3, 255, 0, 195, 10, 206, 3, 255, 0, 196, 10, 206, 3, 255, 0, 197, 10, 207, 3,
    255, 0, 198, 10, 207, 3, 255, 0, 199, 10, 207, 3, 255, 0, 200, 10, 207, 3, 255, 0, 201, 10,
    208, 3, 255, 0, 202, 10, 208, 3, 255, 0, 203, 10, 208, 3, 255, 0, 204, 10, 208, 3, 255, 0, 205,
    10, 209, 3, 255, 0, 206, 10, 209, 3, 255, 0, 207, 10, 209, 3, 255, 0, 208, 10, 209, 3, 255, 0,
    209, 10, 210, 3, 255, 0, 210, 10, 255, 0, 211, 10, 255, 0, 212, 10, 255, 0, 213, 10, 255, 0,
    214, 10, 255, 0, 215, 10, 255, 0, 216, 10, 255, 0, 217, 10, 255, 0, 218, 10, 255, 0, 219, 10,
    255, 0, 220, 10, 255, 0, 221, 10, 255, 0, 222, 10, 255, 0, 223, 10, 255, 0, 224, 10, 255, 0,
    225, 10, 255, 0, 226, 10, 255, 0, 227, 10, 255, 0, 228, 10, 255, 0, 229, 10, 255, 0, 230, 10,
    255, 0, 231, 10, 255, 0, 232, 10, 255, 0, 233, 10, 255, 0, 234, 10, 255, 0, 235, 10, 255, 0,
    236, 10, 255, 0, 237, 10, 255, 0, 238, 10, 255, 0, 239, 10, 255, 0, 240, 10, 255, 0, 241, 10,
    255, 0, 242, 10, 255, 0, 243, 10, 255, 0, 244, 10, 255, 0, 245, 10, 255, 0, 246, 10, 255, 0,
    247, 10, 255, 0, 248, 10, 255, 0, 249, 10, 255, 0, 250, 10, 255, 0, 251, 10, 255, 0, 252, 10,
    255, 0, 253, 10, 255, 0, 254, 10, 255, 0,
];

impl BiomeType {
    /// `BiomeType.get(float, float)` (Java overloads `get`): the curved lookup.
    pub fn at_climate(temperature: f32, moisture: f32) -> Self {
        Self::get_curve(temperature, moisture)
    }

    /// TerraForged Reimagined a24d450's `BiomeType.get` (`getCurve`): Engine's curved lookup on the same table, but
    /// with each index clamped to `0..=255` (`getType`), where Engine's throws and [`Self::at_climate`] panics.
    pub fn at_climate_clamped(temperature: f32, moisture: f32) -> Self {
        let (x, y) = curve_index(temperature, moisture);
        type_at(x.clamp(0, 255), y.clamp(0, 255))
    }

    /// `getLinear`. Panics where Engine throws `ArrayIndexOutOfBoundsException`: a temperature or
    /// moisture index outside 0..=255.
    pub fn get_linear(temperature: f32, moisture: f32) -> Self {
        let (x, y) = linear_index(temperature, moisture);
        type_at(x, y)
    }

    /// `getCurve`. Panics where Engine throws `ArrayIndexOutOfBoundsException`: a temperature or
    /// moisture index outside 0..=255.
    pub fn get_curve(temperature: f32, moisture: f32) -> Self {
        let (x, y) = curve_index(temperature, moisture);
        type_at(x, y)
    }

    /// `apply`: `applyCurve`.
    pub fn apply(cell: &mut Cell) {
        Self::apply_curve(cell);
    }

    /// `applyLinear`. Engine's body is the same as `applyCurve` (`cell.biome = get(t, m)`), so this
    /// uses the curved lookup too, not `get_linear`.
    pub fn apply_linear(cell: &mut Cell) {
        cell.biome = Self::at_climate(cell.temperature, cell.moisture);
    }

    /// `applyCurve`.
    pub fn apply_curve(cell: &mut Cell) {
        cell.biome = Self::at_climate(cell.temperature, cell.moisture);
    }

    pub fn map_temperature(self, value: f32) -> f32 {
        let [minimum, maximum, _, _] = RANGES[self as usize];
        (value - minimum) / (maximum - minimum)
    }

    pub fn map_moisture(self, value: f32) -> f32 {
        let [_, _, minimum, maximum] = RANGES[self as usize];
        (value - minimum) / (maximum - minimum)
    }

    pub const fn min_temperature(self) -> f32 {
        RANGES[self as usize][0]
    }
    pub const fn max_temperature(self) -> f32 {
        RANGES[self as usize][1]
    }
    pub const fn min_moisture(self) -> f32 {
        RANGES[self as usize][2]
    }
    pub const fn max_moisture(self) -> f32 {
        RANGES[self as usize][3]
    }
    /// `getLookup`: the colour the type map image uses for this type (RGB; Engine's colours are opaque).
    pub const fn lookup_color(self) -> [u8; 3] {
        LOOKUP_COLORS[self as usize]
    }
    /// `getColor`: the display colour (RGB; Engine's colours are opaque).
    pub const fn color(self) -> [u8; 3] {
        COLORS[self as usize]
    }
}

/// `getLinear`'s map indices: `x = round(255 * t)`, `y = x` where moisture exceeds temperature, else
/// `round(255 * m)` (`NoiseUtil.round`).
fn linear_index(temperature: f32, moisture: f32) -> (i32, i32) {
    let x = round(255.0 * temperature);
    let y = if moisture > temperature {
        x
    } else {
        round(255.0 * moisture)
    };
    (x, y)
}

/// `getCurve`'s map indices: `x = round(255 * t)`, `y = min(x, round((x + (255 - x) / 2) * m))`, in Java int
/// math (wrapping, division towards zero).
fn curve_index(temperature: f32, moisture: f32) -> (i32, i32) {
    let x = round(255.0 * temperature);
    let max = x.wrapping_add(255_i32.wrapping_sub(x) / 2);
    let y = round(max as f32 * moisture);
    (x, x.min(y))
}

/// `getTypeMap()[y][x]`; `None` where Engine's array access throws.
fn lookup(x: i32, y: i32) -> Option<BiomeType> {
    let x = u8::try_from(x).ok()?;
    let row = usize::from(u8::try_from(y).ok()?);
    let runs = &RUNS[2 * ROW_OFFSETS[row] as usize..2 * ROW_OFFSETS[row + 1] as usize];
    runs.chunks_exact(2)
        .find(|run| x <= run[0])
        .map(|run| BiomeType::get(i32::from(run[1])))
}

fn type_at(x: i32, y: i32) -> BiomeType {
    lookup(x, y).unwrap_or_else(|| panic!("biome type map index [{y}][{x}] out of bounds"))
}

#[cfg(test)]
mod tests {
    use super::super::terraforged::{java_max, java_min};
    use super::super::terraforged_engine::TerrainRegistry;
    use super::*;

    const FNV_OFFSET: u64 = 0xcbf29ce484222325;

    fn fnv(hash: u64, bytes: &[u8]) -> u64 {
        bytes.iter().fold(hash, |hash, &byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        })
    }

    // BiomeTypeOracle.java's SPECIAL inputs (raw bits): zeros, subnormals, 1 and its neighbours, the rounding
    // edges of index 0, 1, 255 and 256, values just outside 0..1, the extremes, NaN and the infinities.
    const SPECIAL: [u32; 33] = [
        0x00000000, 0x80000000, 0x00000001, 0x80000001, 0x3f800000, 0x3f800001, 0x3f7fffff,
        0x80000001, 0x3f000000, 0x3b808081, 0x3b008081, 0x3b008080, 0x3f7f7f7f, 0x3f7f7f7e,
        0x3f804040, 0x3f80403f, 0x3f804041, 0x3fc00000, 0xbb008081, 0xbb008080, 0xbc23d70a,
        0x40000000, 0x7fc00000, 0x7f800000, 0xff800000, 0x7f7fffff, 0xff7fffff, 0x3e800000,
        0x3f400000, 0x3e99999a, 0x3f333333, 0x3f666666, 0x3dcccccd,
    ];

    fn code(kind: Option<BiomeType>) -> String {
        kind.map_or("X".to_owned(), |kind| kind.id().to_string())
    }

    fn letter(kind: Option<BiomeType>) -> u8 {
        kind.map_or(b'X', |kind| b'a' + kind as u8)
    }

    // The cell's biome after `apply`, or None (without calling it) where the curved lookup has no
    // square: `apply`'s panic is checked once in `biome_type_classification_matches_engine_0_3_0`.
    fn applied(
        registry: &TerrainRegistry,
        apply: fn(&mut Cell),
        temperature: f32,
        moisture: f32,
    ) -> Option<BiomeType> {
        let (x, y) = curve_index(temperature, moisture);
        lookup(x, y)?;
        let mut cell = Cell::new(registry);
        cell.temperature = temperature;
        cell.moisture = moisture;
        apply(&mut cell);
        Some(cell.biome)
    }

    fn canonical(value: f32) -> u32 {
        if value.is_nan() {
            0x7fc00000
        } else {
            value.to_bits()
        }
    }

    #[test]
    fn biome_type_map_matches_engine_0_3_0() {
        // FNV-1a over BiomeTypeOracle's `m` lines (the map's ordinals, row by row).
        let mut hash = FNV_OFFSET;
        for y in 0..256 {
            for x in 0..256 {
                hash = fnv(hash, &[lookup(x, y).unwrap() as u8]);
            }
        }
        assert_eq!(hash, 0xd1be24a129080e93);
        for (x, y) in [
            (-1, 0),
            (0, -1),
            (256, 0),
            (0, 256),
            (i32::MIN, 0),
            (0, i32::MAX),
        ] {
            assert_eq!(lookup(x, y), None);
        }
    }

    #[test]
    fn biome_type_ranges_and_colors_match_engine_0_3_0() {
        // BiomeTypeOracle's `t` lines: range bits, lookup colour, display colour.
        let expected: [([u32; 4], [u8; 3], [u8; 3]); 11] = [
            (
                [0x3f4acacb, 0x3f800000, 0x3f1d9d9e, 0x3f800000],
                [7, 83, 48],
                [52, 125, 63],
            ),
            (
                [0x3eeeeeef, 0x3f800000, 0x3e20a0a1, 0x3f29a9aa],
                [151, 165, 39],
                [136, 173, 62],
            ),
            (
                [0x3efefeff, 0x3f800000, 0x00000000, 0x3e868687],
                [200, 113, 55],
                [230, 219, 156],
            ),
            (
                [0x3f0c8c8d, 0x3f52d2d3, 0x3f0c8c8d, 0x3f52d2d3],
                [10, 84, 109],
                [82, 140, 53],
            ),
            (
                [0x3ed4d4d5, 0x3f55d5d6, 0x3e2cacad, 0x3f1c9c9d],
                [44, 137, 160],
                [93, 153, 72],
            ),
            (
                [0x3e78f8f9, 0x3f56d6d7, 0x3d50d0d1, 0x3ec8c8c9],
                [179, 124, 6],
                [75, 179, 79],
            ),
            (
                [0x3e109091, 0x3f169697, 0x00000000, 0x3e969697],
                [131, 112, 71],
                [194, 187, 132],
            ),
            (
                [0x3ec6c6c7, 0x3f3fbfc0, 0x00000000, 0x3e088889],
                [199, 155, 60],
                [224, 203, 137],
            ),
            (
                [0x3e44c4c5, 0x3f20a0a1, 0x3d30b0b1, 0x3edededf],
                [91, 143, 82],
                [77, 115, 59],
            ),
            (
                [0x00000000, 0x3e58d8d9, 0x00000000, 0x3e54d4d5],
                [147, 167, 172],
                [215, 219, 200],
            ),
            (
                [0x00000000, 0x3f7efeff, 0x3b808081, 0x3f800000],
                [0, 0, 0],
                [239, 242, 237],
            ),
        ];
        for (kind, (ranges, lookup_color, color)) in BiomeType::VALUES.into_iter().zip(expected) {
            let actual = [
                kind.min_temperature(),
                kind.max_temperature(),
                kind.min_moisture(),
                kind.max_moisture(),
            ];
            assert_eq!(actual.map(f32::to_bits), ranges, "{kind:?}");
            assert_eq!(kind.lookup_color(), lookup_color, "{kind:?}");
            assert_eq!(kind.color(), color, "{kind:?}");
        }
        // The ranges are `BiomeTypeLoader.getRanges` over the map (float min/max of x/255f and y/255f).
        for kind in BiomeType::VALUES {
            let mut range = [1.0_f32, 0.0, 1.0, 0.0];
            for y in 0..256 {
                for x in 0..256 {
                    if lookup(x, y) == Some(kind) {
                        let (t, m) = (x as f32 / 255.0, y as f32 / 255.0);
                        range = [
                            java_min(range[0], t),
                            java_max(range[1], t),
                            java_min(range[2], m),
                            java_max(range[3], m),
                        ];
                    }
                }
            }
            assert_eq!(
                range.map(f32::to_bits),
                RANGES[kind as usize].map(f32::to_bits)
            );
        }
    }

    #[test]
    fn biome_type_mapping_matches_engine_0_3_0() {
        // FNV-1a over BiomeTypeOracle's `p` lines: mapTemperature and mapMoisture of every SPECIAL input.
        let lines: Vec<String> = BiomeType::VALUES
            .into_iter()
            .flat_map(|kind| {
                SPECIAL.map(|bits| {
                    let value = f32::from_bits(bits);
                    format!(
                        "p {} {bits:08x} {:08x} {:08x}",
                        kind.id(),
                        canonical(kind.map_temperature(value)),
                        canonical(kind.map_moisture(value))
                    )
                })
            })
            .collect();
        assert_eq!(
            fnv(FNV_OFFSET, lines.join("\n").as_bytes()),
            0x11e7b92c7a6df795
        );
    }

    #[test]
    fn biome_type_classification_matches_engine_0_3_0() {
        // BiomeTypeOracle's `s` lines: curve, linear, get, apply, applyLinear and applyCurve over every
        // pair of SPECIAL inputs; X where Engine throws.
        let registry = TerrainRegistry::new();
        let mut lines = Vec::new();
        for t in SPECIAL {
            for m in SPECIAL {
                let (temperature, moisture) = (f32::from_bits(t), f32::from_bits(m));
                let (cx, cy) = curve_index(temperature, moisture);
                let (lx, ly) = linear_index(temperature, moisture);
                let curve = lookup(cx, cy);
                lines.push(format!(
                    "s {t:08x} {m:08x} {} {} {} {} {} {}",
                    code(curve),
                    code(lookup(lx, ly)),
                    code(curve),
                    code(applied(&registry, BiomeType::apply, temperature, moisture)),
                    code(applied(
                        &registry,
                        BiomeType::apply_linear,
                        temperature,
                        moisture
                    )),
                    code(applied(
                        &registry,
                        BiomeType::apply_curve,
                        temperature,
                        moisture
                    )),
                ));
            }
        }
        assert_eq!(
            fnv(FNV_OFFSET, lines.join("\n").as_bytes()),
            0xb2c6a733a7e3f2c5
        );
        // A few lines in full, including applyLinear using the curve where the linear lookup differs.
        assert!(lines.contains(&"s 00000000 bb008081 9 X 9 9 9 9".to_owned()));
        assert!(lines.contains(&"s 3f800000 3f800000 0 0 0 0 0 0".to_owned()));
        assert!(lines.contains(&"s 00000000 ff800000 X X X X X X".to_owned()));
        // The public lookups agree with the indices and panic where Engine throws.
        assert_eq!(BiomeType::get_curve(0.5, 0.5), lookup(128, 64).unwrap());
        assert_eq!(
            BiomeType::at_climate(0.9, 0.9),
            BiomeType::TropicalRainforest
        );
        assert_eq!(BiomeType::get_linear(0.7, 0.3), BiomeType::Grassland);
        assert!(std::panic::catch_unwind(|| BiomeType::get_curve(f32::NEG_INFINITY, 0.5)).is_err());
        assert!(std::panic::catch_unwind(|| BiomeType::get_linear(0.0, -0.01)).is_err());
        assert!(
            std::panic::catch_unwind(|| {
                let mut cell = Cell::new(&TerrainRegistry::new());
                cell.temperature = 2.0;
                BiomeType::apply_linear(&mut cell);
            })
            .is_err()
        );
    }

    #[test]
    fn biome_type_grid_matches_engine_0_3_0() {
        // BiomeTypeOracle's `g` lines: t, m = i / 600f - 0.05f for i in 0..=660, curve then linear codes.
        let axis: Vec<f32> = (0..=660).map(|i| i as f32 / 600.0 - 0.05).collect();
        let mut text = Vec::new();
        for (row, &t) in axis.iter().enumerate() {
            if row > 0 {
                text.push(b' ');
            }
            for &m in &axis {
                let (x, y) = curve_index(t, m);
                text.push(letter(lookup(x, y)));
            }
            text.push(b' ');
            for &m in &axis {
                let (x, y) = linear_index(t, m);
                text.push(letter(lookup(x, y)));
            }
        }
        assert_eq!(fnv(FNV_OFFSET, &text), 0x549bc3632ee3fd0e);
    }
}
