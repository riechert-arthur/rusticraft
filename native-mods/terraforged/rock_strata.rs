// The `rock_strata` setting (Horizon's): bands of granite, diorite, andesite and stone underground, and a
// hardness for each that the erosion pass reads, so soft bands wear back into benches and hard ones stay as caps
// and ledges. The bands are TerraForged Reimagined's `StrataDecorator` (off by default there too), with one
// departure: Reimagined gives every region of 600x600 blocks its own band sequence, which changes in a vertical
// wall at the region's border; here each column blends the sequences of the four nearest region centres.
//
// Adapted from TerraForged Reimagined (https://github.com/Romoslayer/TerraForged-Reimagined, a24d450):
// `src/main/java/com/terraforged/mod/worldgen/util/StrataDecorator.java`. MIT License, Copyright (c) 2020
// TerraForged, Copyright (c) 2026 Romoslayer; see `LICENSE-TERRAFORGED-REIMAGINED`.

use super::super::terrain::{TerrainBlockStates, TerrainChunk, TerrainError};
use super::terraforged::{hash, hash_2d, simplex};
use super::terraforged_terrain_blend_util::random;
use crate::heightmap::HeightmapType;
use crate::registry::{BlockRegistry, BlockStateId};

/// `SOIL_MARGIN`: the bands stop this many blocks under the column's OCEAN_FLOOR_WG height, so the surface rules
/// still find plain stone to dress.
pub const SOIL_MARGIN: i32 = 8;
/// `DEEPSLATE_TOP`: the lowest banded block (the surface rules turn stone into deepslate below it).
pub const DEEPSLATE_TOP: i32 = 9;
/// `strataRegionSize`'s default: the side of a region with its own band sequence, in blocks.
pub const REGION_SIZE: i32 = 600;
const TILT_SCALE: i32 = 200;
const TILT_BLOCKS: f32 = 12.0;
/// Reimagined's `buildBands` arrays hold six bands.
const MAX_BANDS: usize = 6;

/// A banded rock, in the order of Reimagined's `ALL` palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rock {
    Granite,
    Diorite,
    Andesite,
    Stone,
    Tuff,
    Calcite,
    Dripstone,
    SmoothBasalt,
}

/// `ORE_COMPATIBLE` (`oreCompatibleStoneOnly`, on by default in Reimagined and always here): exactly the blocks of
/// `#minecraft:stone_ore_replaceables`, so ores still generate through the bands.
pub const ORE_COMPATIBLE: [Rock; 4] = [Rock::Granite, Rock::Diorite, Rock::Andesite, Rock::Stone];
/// `ALL`: Reimagined's palette with `oreCompatibleStoneOnly` off.
pub const ALL: [Rock; 8] = [
    Rock::Granite,
    Rock::Diorite,
    Rock::Andesite,
    Rock::Stone,
    Rock::Tuff,
    Rock::Calcite,
    Rock::Dripstone,
    Rock::SmoothBasalt,
];

impl Rock {
    /// The block's registry name.
    pub const fn block(self) -> &'static str {
        match self {
            Self::Granite => "minecraft:granite",
            Self::Diorite => "minecraft:diorite",
            Self::Andesite => "minecraft:andesite",
            Self::Stone => "minecraft:stone",
            Self::Tuff => "minecraft:tuff",
            Self::Calcite => "minecraft:calcite",
            Self::Dripstone => "minecraft:dripstone_block",
            Self::SmoothBasalt => "minecraft:smooth_basalt",
        }
    }

    /// How well the rock resists erosion, 0 (none) to 1 (it never wears): the share of the erosion pass's rate it
    /// takes off where a droplet erodes it. The order follows measured rock strength (Selby 1980, rock mass
    /// strength; Sklar and Dietrich 2001, tensile strength against bedrock incision): coarse plutonic rock (granite,
    /// diorite) and basalt resist best, andesite less, while plain stone, the bands' matrix, stands for the weaker
    /// sedimentary layers between them; tuff (welded ash) is the softest.
    pub const fn hardness(self) -> f32 {
        match self {
            Self::Granite => 0.8,
            Self::Diorite => 0.75,
            Self::Andesite => 0.6,
            Self::Stone => 0.2,
            Self::Tuff => 0.1,
            Self::Calcite => 0.4,
            Self::Dripstone => 0.3,
            Self::SmoothBasalt => 0.85,
        }
    }
}

/// `buildBands`: one region's band sequence, 4 to 6 bands of 3 to 14 blocks (15 where the hash maps to exactly 1)
/// drawn from the palette, repeated all the way down.
#[derive(Clone, Copy, Debug)]
pub struct Bands {
    hash: i32,
    len: usize,
    rocks: [Rock; MAX_BANDS],
    depths: [i32; MAX_BANDS],
    total: i32,
}

impl Bands {
    /// `buildBands(seed, rx, rz, palette, …)`, in Java `int` math. Where the hash maps to exactly 1 Java's
    /// `4 + (int) (rand * 3)` is 7 and its six-band arrays throw; the port keeps six bands there.
    pub fn new(seed: i32, region_x: i32, region_z: i32, palette: &[Rock]) -> Self {
        let hash = hash_2d(seed.wrapping_add(7741), region_x, region_z);
        let rand = |offset: i32| random(super::terraforged::hash(hash, offset));
        let len = (4 + (rand(1) * 3.0) as i32).min(MAX_BANDS as i32) as usize;
        let mut rocks = [Rock::Stone; MAX_BANDS];
        let mut depths = [0; MAX_BANDS];
        let count = palette.len() as i32;
        for i in 0..len {
            rocks[i] = palette[((rand(10 + i as i32) * count as f32) as i32 % count) as usize];
            depths[i] = 3 + (rand(20 + i as i32) * 12.0) as i32;
        }
        Self {
            hash,
            len,
            rocks,
            depths,
            total: depths.iter().sum(),
        }
    }

    /// `apply`'s band lookup: the rock at `depth = floorMod(y + tilt, total)` (`level` is `y + tilt`).
    pub fn rock_at(&self, level: i32) -> Rock {
        let depth = level.rem_euclid(self.total);
        let mut acc = 0;
        for i in 0..self.len {
            acc += self.depths[i];
            if depth < acc {
                return self.rocks[i];
            }
        }
        self.rocks[self.len - 1]
    }

    /// Not Reimagined's: the band index at height `level` (`y + tilt`, a real number) as a number that grows
    /// steadily by 1 through each band, counted over the repeats (band `k` is `rocks[k mod len]`). Its floor is the
    /// band [`Self::rock_at`] reads at `floor(level)`.
    fn layer(&self, level: f64) -> f64 {
        let total = f64::from(self.total);
        let cycle = (level / total).floor();
        let depth = (level - cycle * total).clamp(0.0, total - f64::EPSILON * total);
        let mut acc = 0.0;
        for i in 0..self.len {
            let thickness = f64::from(self.depths[i]);
            if depth < acc + thickness || i + 1 == self.len {
                let within = ((depth - acc) / thickness).min(1.0 - f64::EPSILON);
                return cycle * self.len as f64 + i as f64 + within;
            }
            acc += thickness;
        }
        unreachable!("bands are never empty")
    }

    /// Band `k`'s rock (counted as in [`Self::layer`]).
    fn band(&self, k: i64) -> Rock {
        self.rocks[k.rem_euclid(self.len as i64) as usize]
    }
}

/// `apply`'s tilt: `(int) (Noise.singleSimplex(x / 200F, z / 200F, seed + 31337) * 12F)` blocks.
pub fn tilt(seed: i32, x: i32, z: i32) -> i32 {
    raw_tilt(seed, x, z) as i32
}

/// [`tilt`] before Java's `(int)` cast.
fn raw_tilt(seed: i32, x: i32, z: i32) -> f32 {
    let scale = TILT_SCALE as f32;
    simplex(x as f32 / scale, z as f32 / scale, seed.wrapping_add(31337)) * TILT_BLOCKS
}

/// Not Reimagined's: a continuous stand-in for the `(int)` cast of a tilt `t`, so the blended bands don't step a
/// block where the cast does. It is `t / 2` between -1 and 1 and `t ∓ 0.5` beyond, so the block's middle plus it,
/// floored, is `y + (int) t` (except where `t` is a whole negative number).
fn smooth_tilt(t: f32) -> f64 {
    let t = f64::from(t);
    if t.abs() < 1.0 {
        t / 2.0
    } else {
        t - 0.5 * t.signum()
    }
}

/// Reimagined's own rock at block `(x, y, z)`: its region's (`floorDiv(x, max(16, region_size))`) band sequence at
/// `y + tilt`. Where `apply` places it is [`banded_range`]'s business.
pub fn faithful_rock(
    seed: i32,
    x: i32,
    y: i32,
    z: i32,
    palette: &[Rock],
    region_size: i32,
) -> Rock {
    let size = region_size.max(16);
    Bands::new(seed, x.div_euclid(size), z.div_euclid(size), palette)
        .rock_at(y.wrapping_add(tilt(seed, x, z)))
}

/// `apply`'s loop bounds for a column whose OCEAN_FLOOR_WG height (the first free block over the floor) is
/// `floor_height`, in a chunk from `min_y`: from `floor_height - SOIL_MARGIN` down to `max(min_y, DEEPSLATE_TOP)`,
/// or none where that top isn't above the bottom. Only plain stone in it is replaced.
pub fn banded_range(floor_height: i32, min_y: i32) -> Option<std::ops::RangeInclusive<i32>> {
    let top = floor_height.wrapping_sub(SOIL_MARGIN);
    let bottom = min_y.max(DEEPSLATE_TOP);
    (top > bottom).then_some(bottom..=top)
}

/// The world's strata: Reimagined's bands over [`REGION_SIZE`] regions with the [`ORE_COMPATIBLE`] palette, blended
/// between regions.
#[derive(Clone, Copy, Debug)]
pub struct Strata {
    seed: i32,
}

impl Strata {
    pub const fn new(seed: i32) -> Self {
        Self { seed }
    }

    /// The strata of the blocks from `(x0, z0)`, `width` by `depth` columns: the band sequences of the regions
    /// they blend and each column's tilt, computed once.
    pub fn area(self, x0: i32, z0: i32, width: i32, depth: i32) -> StrataArea {
        let (rx0, rz0) = (region_below(x0), region_below(z0));
        let (rx1, rz1) = (
            region_below(x0 + width - 1) + 1,
            region_below(z0 + depth - 1) + 1,
        );
        let regions_x = rx1 - rx0 + 1;
        let mut bands = Vec::with_capacity((regions_x * (rz1 - rz0 + 1)) as usize);
        for rz in rz0..=rz1 {
            for rx in rx0..=rx1 {
                bands.push(Bands::new(self.seed, rx, rz, &ORE_COMPATIBLE));
            }
        }
        let tilts = (0..depth)
            .flat_map(|dz| (0..width).map(move |dx| (dx, dz)))
            .map(|(dx, dz)| smooth_tilt(raw_tilt(self.seed, x0 + dx, z0 + dz)))
            .collect();
        StrataArea {
            x0,
            z0,
            width,
            rx0,
            rz0,
            regions_x,
            bands,
            tilts,
        }
    }
}

/// The region whose centre (block `600 r + 300`) is the nearest at or below the column, along one axis, and how far
/// (0..1) the column lies from it towards the next one.
fn region_blend(coord: i32) -> (i32, f64) {
    let u = f64::from(coord) / f64::from(REGION_SIZE) - 0.5;
    let region = u.floor();
    (region as i32, u - region)
}

fn region_below(coord: i32) -> i32 {
    region_blend(coord).0
}

/// [`Strata::area`]'s precomputed strata.
pub struct StrataArea {
    x0: i32,
    z0: i32,
    width: i32,
    rx0: i32,
    rz0: i32,
    regions_x: i32,
    bands: Vec<Bands>,
    tilts: Vec<f64>,
}

impl StrataArea {
    /// The rock at block `(x, y, z)` (a column of the area). Not Reimagined's: the column's band index
    /// ([`Bands::layer`] at the block's middle plus the tilt, smoothed by `smooth_tilt`) is the mean of the four
    /// nearest region centres' (bilinear weights by distance, so it changes smoothly between regions and equals the
    /// region's own at its centre). Its floor is the band; its rock comes from one of those regions, the one with the
    /// highest weight once each weight is scaled by a factor from 0.5 to 1.5 drawn per region and band, so each band
    /// changes rock at its own place between two centres (a lateral change of facies) instead of all at once.
    pub fn rock_at(&self, x: i32, y: i32, z: i32) -> Rock {
        let (corners, layer) = self.layer(x, y, z);
        let band = layer.floor() as i64;
        let mut best = (corners[0].0, f64::NEG_INFINITY);
        for &(bands, weight) in &corners {
            if weight <= 0.0 {
                continue;
            }
            let jitter = 0.5 + f64::from(random(hash(bands.hash, band as i32)));
            if weight * jitter > best.1 {
                best = (bands, weight * jitter);
            }
        }
        best.0.band(band)
    }

    /// The four nearest region centres' band sequences with their weights, and the column's blended band index.
    fn layer(&self, x: i32, y: i32, z: i32) -> ([(&Bands, f64); 4], f64) {
        let (rx, fx) = region_blend(x);
        let (rz, fz) = region_blend(z);
        let tilt = self.tilts[((z - self.z0) * self.width + (x - self.x0)) as usize];
        let level = f64::from(y) + 0.5 + tilt;
        let at = |ox: i32, oz: i32| {
            let index = (rz + oz - self.rz0) * self.regions_x + (rx + ox - self.rx0);
            &self.bands[index as usize]
        };
        let corners = [
            (at(0, 0), (1.0 - fx) * (1.0 - fz)),
            (at(1, 0), fx * (1.0 - fz)),
            (at(0, 1), (1.0 - fx) * fz),
            (at(1, 1), fx * fz),
        ];
        let layer = corners
            .iter()
            .map(|(bands, weight)| weight * bands.layer(level))
            .sum();
        (corners, layer)
    }
}

/// The blocks of the bands, and plain stone, which they replace.
pub struct StrataBlocks {
    stone: BlockStateId,
    rocks: [BlockStateId; ALL.len()],
}

impl StrataBlocks {
    /// `None` where the registry lacks one of the rocks.
    pub fn new(registry: &BlockRegistry) -> Option<Self> {
        let mut rocks = [BlockStateId(0); ALL.len()];
        for (slot, rock) in rocks.iter_mut().zip(ALL) {
            *slot = registry.default_state(rock.block())?;
        }
        Some(Self {
            stone: registry.default_state("minecraft:stone")?,
            rocks,
        })
    }

    fn block(&self, rock: Rock) -> BlockStateId {
        self.rocks[rock as usize]
    }
}

/// `StrataDecorator.apply` (blended, [`StrataArea::rock_at`]), right after the chunk's fill: in every column, from
/// [`SOIL_MARGIN`] blocks under its OCEAN_FLOOR_WG height down to [`DEEPSLATE_TOP`] ([`banded_range`]), each block
/// of plain stone becomes its band's rock.
pub fn apply<B: TerrainBlockStates>(
    seed: i32,
    chunk: &mut TerrainChunk<'_, B>,
    blocks: &StrataBlocks,
) -> Result<(), TerrainError> {
    let position = chunk.position();
    let (start_x, start_z) = (position.x.wrapping_shl(4), position.z.wrapping_shl(4));
    let mut floors = [0i32; 256];
    {
        let heights = chunk
            .heightmap(HeightmapType::OceanFloorWg)
            .ok_or(TerrainError::InvalidState)?;
        for (i, floor) in floors.iter_mut().enumerate() {
            *floor = heights.first_available((i & 15) as i32, (i >> 4) as i32)?;
        }
    }
    let area = Strata::new(seed).area(start_x, start_z, 16, 16);
    let min_y = chunk.min_y();
    for dz in 0..16 {
        for dx in 0..16 {
            let Some(range) = banded_range(floors[(dz * 16 + dx) as usize], min_y) else {
                continue;
            };
            for y in range.rev() {
                if chunk.block(dx, y, dz) != blocks.stone {
                    continue;
                }
                let rock = area.rock_at(start_x + dx, y, start_z + dz);
                if rock != Rock::Stone {
                    chunk.set_block_worldgen(dx, y, dz, blocks.block(rock))?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `oracles/strata/out.txt` (`run.sh`): Reimagined's own `StrataDecorator.apply` over stub chunks of stone from
    /// y -64 to a height per column with dirt mixed in and air above (`StrataOracle`): per seed, region size and
    /// palette one FNV-1a hash of the ids (the [`ALL`] order; dirt 8, air 9) of five chunks, chunk by chunk, then y,
    /// z, x, after the `b` lines that sample the first two groups' chunks.
    const JAVA: &str = "\
b 0 0 9 13 0
b 0 0 40 8 9
b 0 0 100 4 3
b 0 0 200 8 9
b -1 -1 9 13 2
b -1 -1 40 8 2
b -1 -1 100 4 9
b -1 -1 200 8 9
b 37 -38 9 13 8
b 37 -38 40 8 0
b 37 -38 100 4 2
b 37 -38 200 8 8
b -38 37 9 13 3
b -38 37 40 8 3
b -38 37 100 4 2
b -38 37 200 8 3
b 134217727 -134217728 9 13 2
b 134217727 -134217728 40 8 3
b 134217727 -134217728 100 4 9
b 134217727 -134217728 200 8 9
c 0 600 true 88ba9b5838b9aac9
b 0 0 9 13 1
b 0 0 40 8 9
b 0 0 100 4 6
b 0 0 200 8 9
b -1 -1 9 13 5
b -1 -1 40 8 5
b -1 -1 100 4 9
b -1 -1 200 8 9
b 37 -38 9 13 8
b 37 -38 40 8 1
b 37 -38 100 4 5
b 37 -38 200 8 8
b -38 37 9 13 7
b -38 37 40 8 7
b -38 37 100 4 5
b -38 37 200 8 7
b 134217727 -134217728 9 13 5
b 134217727 -134217728 40 8 7
b 134217727 -134217728 100 4 9
b 134217727 -134217728 200 8 9
c 0 600 false 85d12955d57c747a
c 0 16 true 1f02664344872aba
c 0 16 false 4bd1b285a67fcd5d
c 0 5 true 1f02664344872aba
c 0 5 false 4bd1b285a67fcd5d
c 0 100 true 55d3b8531e7caffe
c 0 100 false ee5714fbfa7f5163
c 777 600 true fe07775118dd19cf
c 777 600 false 2f0a8d8203ab0770
c 777 16 true 91b38f5a36901f4d
c 777 16 false 1775d6967bd635dd
c 777 5 true 91b38f5a36901f4d
c 777 5 false 1775d6967bd635dd
c 777 100 true 326ab121e6e42170
c 777 100 false 2c173ce04a22077d
c -5 600 true 0b0d31d6ee69cd59
c -5 600 false b929169bac8eeecb
c -5 16 true a31705fdc1ae3fc1
c -5 16 false 216f850ca540d4af
c -5 5 true a31705fdc1ae3fc1
c -5 5 false 216f850ca540d4af
c -5 100 true d64bf3ed42fec789
c -5 100 false 4ad341e1d6da8d83
c -2147483648 600 true b8d80ed47e9377a4
c -2147483648 600 false 725f8fc45c7c326e
c -2147483648 16 true bf7655c924f18e55
c -2147483648 16 false b2336b628b32147e
c -2147483648 5 true bf7655c924f18e55
c -2147483648 5 false b2336b628b32147e
c -2147483648 100 true ea96a10764dd85f8
c -2147483648 100 false 8530ba21a24b6ebd
c 123456789 600 true d90912bd0f07972f
c 123456789 600 false beaf0671b8709935
c 123456789 16 true 9312efaded618984
c 123456789 16 false b0ee56e70d6b8390
c 123456789 5 true 9312efaded618984
c 123456789 5 false b0ee56e70d6b8390
c 123456789 100 true 53fcc1d389cc8ba3
c 123456789 100 false 3a21068fce723abb
";

    const MIN_Y: i32 = -64;
    const TOP: i32 = MIN_Y + 24 * 16;

    fn oracle_lines() -> Vec<String> {
        let seeds = [0, 777, -5, i32::MIN, 123_456_789];
        let sizes = [600, 16, 5, 100];
        let chunks = [
            (0, 0),
            (-1, -1),
            (37, -38),
            (-38, 37),
            (134_217_727, -134_217_728),
        ];
        let mut lines = Vec::new();
        let mut index = 0;
        for seed in seeds {
            for size in sizes {
                for ore in [true, false] {
                    let palette: &[Rock] = if ore { &ORE_COMPATIBLE } else { &ALL };
                    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
                    for (cx, cz) in chunks {
                        let (sx, sz) = (cx << 4, cz << 4);
                        let block = |dx: i32, y: i32, dz: i32| -> u32 {
                            let (x, z) = (sx + dx, sz + dz);
                            let height = MIN_Y
                                + 40
                                + x.wrapping_mul(31)
                                    .wrapping_add(z.wrapping_mul(17))
                                    .wrapping_add(seed)
                                    .rem_euclid(300);
                            if y >= height {
                                9
                            } else if x
                                .wrapping_mul(3)
                                .wrapping_add(y * 5)
                                .wrapping_add(z.wrapping_mul(7))
                                .rem_euclid(23)
                                == 0
                            {
                                8
                            } else if banded_range(height, MIN_Y).is_some_and(|r| r.contains(&y)) {
                                faithful_rock(seed, x, y, z, palette, size) as u32
                            } else {
                                Rock::Stone as u32
                            }
                        };
                        for y in MIN_Y..TOP {
                            for dz in 0..16 {
                                for dx in 0..16 {
                                    hash ^= u64::from(block(dx, y, dz));
                                    hash = hash.wrapping_mul(0x100_0000_01b3);
                                }
                            }
                        }
                        if index < 2 {
                            for y in [9, 40, 100, 200] {
                                let z = (5 * y) & 15;
                                lines.push(format!("b {cx} {cz} {y} {z} {}", block(7, y, z)));
                            }
                        }
                    }
                    lines.push(format!("c {seed} {size} {ore} {hash:016x}"));
                    index += 1;
                }
            }
        }
        lines
    }

    #[test]
    fn strata_match_reimagined_java() {
        assert_eq!(oracle_lines(), JAVA.lines().collect::<Vec<_>>());
    }

    /// The blend is Reimagined's own sequence at a region's centre column.
    #[test]
    fn the_blend_is_reimagineds_at_region_centres() {
        for seed in [0, 777, -5] {
            for (rx, rz) in [(0, 0), (-1, 2), (3, -4)] {
                let (x, z) = (
                    rx * REGION_SIZE + REGION_SIZE / 2,
                    rz * REGION_SIZE + REGION_SIZE / 2,
                );
                let area = Strata::new(seed).area(x, z, 1, 1);
                for y in -64..400 {
                    assert_eq!(
                        area.rock_at(x, y, z),
                        faithful_rock(seed, x, y, z, &ORE_COMPATIBLE, REGION_SIZE),
                        "{seed} {x} {y} {z}"
                    );
                }
            }
        }
    }

    /// Reimagined's sequences change in a wall at region borders; blended, every band's floor moves at most one
    /// block between neighbouring columns, through the borders and the blend between them (y 9 to 320, three seeds,
    /// lines across a border along both axes).
    #[test]
    fn band_floors_step_at_most_one_block_between_neighbours() {
        let floors = |area: &StrataArea, x: i32, z: i32| -> Vec<(i64, i32)> {
            let mut out = Vec::new();
            let mut last = area.layer(x, DEEPSLATE_TOP, z).1.floor() as i64;
            for y in DEEPSLATE_TOP + 1..=320 {
                let band = area.layer(x, y, z).1.floor() as i64;
                assert!(band >= last, "the bands climb with y");
                for k in last + 1..=band {
                    out.push((k, y));
                }
                last = band;
            }
            out
        };
        let mut faithful_walls = 0;
        for seed in [0, 777, -5] {
            for (x0, z0, along_x) in [(300, 150, true), (-900, -450, true), (150, 300, false)] {
                let (w, d) = if along_x { (601, 1) } else { (1, 601) };
                let area = Strata::new(seed).area(x0, z0, w, d);
                let mut previous: Option<Vec<(i64, i32)>> = None;
                for i in 0..601 {
                    let (x, z) = if along_x { (x0 + i, z0) } else { (x0, z0 + i) };
                    let here = floors(&area, x, z);
                    if let Some(before) = &previous {
                        for &(k, y) in &here {
                            if let Some(&(_, y0)) = before.iter().find(|(k0, _)| *k0 == k) {
                                assert!(
                                    (y - y0).abs() <= 1,
                                    "{seed} {x} {z} band {k}: {y0} -> {y}"
                                );
                            }
                        }
                    }
                    previous = Some(here);
                    let (a, b) = if along_x { (x - 1, x) } else { (z - 1, z) };
                    if a.div_euclid(REGION_SIZE) != b.div_euclid(REGION_SIZE) {
                        let (ax, az) = if along_x { (a, z) } else { (x, a) };
                        faithful_walls += usize::from((DEEPSLATE_TOP..=320).any(|y| {
                            faithful_rock(seed, ax, y, az, &ORE_COMPATIBLE, REGION_SIZE)
                                != faithful_rock(seed, x, y, z, &ORE_COMPATIBLE, REGION_SIZE)
                        }));
                    }
                }
            }
        }
        assert!(
            faithful_walls > 0,
            "Reimagined's own bands change at the borders"
        );
    }

    /// Ore-compatible bands are harder than the stone between them, so erosion wears the stone back.
    #[test]
    fn hard_bands_are_harder_than_stone() {
        for rock in [Rock::Granite, Rock::Diorite, Rock::Andesite] {
            assert!(rock.hardness() > Rock::Stone.hardness() + 0.3);
        }
        for rock in ALL {
            assert!((0.0..1.0).contains(&rock.hardness()));
        }
    }
}
