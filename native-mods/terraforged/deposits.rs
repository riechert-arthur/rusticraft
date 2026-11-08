// The `erosion_deposits` setting (Horizon's, in neither TerraForged tree): what TerraForged's
// erosion pass deposited shows on the surface. Erosion lowers nearly all ground a little; where a
// column came out at least as high as the 7x7 around it (it lost less than its surroundings, or
// gained: `ErodedNoiseGenerator::generate` measures it on the erosion region's map), the sediment
// settles as gravel and clay in river and lake beds, gravel on their banks, and scree (gravel and
// cobblestone) at the foot of steeper ground. It only changes the blocks of the natural surface,
// never a height: the sea, beaches and everything above the ground are left as they are.

use super::super::terrain::{TerrainBlockStates, TerrainChunk, TerrainError};
use super::terraforged_engine::TerrainProperties;
use super::terraforged_terrain_data::TerrainData;
use crate::heightmap::{HeightmapState, HeightmapType};
use crate::registry::{BlockRegistry, BlockStateId};

/// How much higher than the ground around it (in blocks, erosion's change against the 7x7's mean)
/// a column must come out to hold sediment.
const MIN_DEPTH: f32 = 0.0;
/// Blocks of sediment per block a column stands out by.
const THICKNESS: f32 = 5.0;
/// The most blocks a deposit replaces under the surface.
const MAX_DEPTH: i32 = 3;
/// How much higher ground within two blocks makes a column the foot of a slope.
const SLOPE: i32 = 4;

/// The blocks deposits are made of, and those they may cover.
pub struct DepositBlocks {
    gravel: BlockStateId,
    clay: BlockStateId,
    cobblestone: BlockStateId,
    natural: Vec<BlockStateId>,
}

impl DepositBlocks {
    /// `None` where the registry lacks gravel, clay or cobblestone.
    pub fn new(registry: &BlockRegistry) -> Option<Self> {
        let state = |name: &str| registry.default_state(name);
        Some(Self {
            gravel: state("minecraft:gravel")?,
            clay: state("minecraft:clay")?,
            cobblestone: state("minecraft:cobblestone")?,
            natural: [
                "minecraft:grass_block",
                "minecraft:dirt",
                "minecraft:coarse_dirt",
                "minecraft:podzol",
                "minecraft:sand",
                "minecraft:red_sand",
                "minecraft:stone",
                "minecraft:mud",
            ]
            .iter()
            .filter_map(|name| state(name))
            .collect(),
        })
    }
}

/// Lay the chunk's deposits over its surface; `seed` picks the material of each column.
pub fn apply<B: TerrainBlockStates>(
    seed: i32,
    terrain: &TerrainData,
    chunk: &mut TerrainChunk<'_, B>,
    blocks: &DepositBlocks,
) -> Result<(), TerrainError> {
    let position = chunk.position();
    let mut tops = [0i32; 256];
    {
        let heights = chunk
            .heightmap(HeightmapType::OceanFloorWg)
            .ok_or(TerrainError::InvalidState)?;
        for (i, top) in tops.iter_mut().enumerate() {
            let (dx, dz) = ((i & 15) as i32, (i >> 4) as i32);
            *top = heights.first_available(dx, dz)? - 1;
        }
    }
    let top = |dx: i32, dz: i32| tops[(dz * 16 + dx) as usize];
    // A river or lake column under water (the sea's floor and shores keep their own blocks).
    let inland_water = |chunk: &TerrainChunk<'_, B>, dx: i32, dz: i32| {
        terrain
            .terrain()
            .get(dx, dz)
            .is_some_and(|terrain| terrain.is_river() || terrain.is_lake())
            && chunk
                .block_properties(chunk.block(dx, top(dx, dz) + 1, dz))
                .has_fluid()
    };
    let wet = |chunk: &TerrainChunk<'_, B>, dx: i32, dz: i32| {
        chunk
            .block_properties(chunk.block(dx, top(dx, dz) + 1, dz))
            .has_fluid()
    };
    for dz in 0..16 {
        for dx in 0..16 {
            let deposit = terrain.deposit_at(dx, dz);
            if deposit.is_nan() || deposit < MIN_DEPTH {
                continue;
            }
            let y = top(dx, dz);
            let (x, z) = (
                position.x.wrapping_shl(4).wrapping_add(dx),
                position.z.wrapping_shl(4).wrapping_add(dz),
            );
            let roll = hash(seed, x, z);
            let neighbours = |radius: i32| {
                (-radius..=radius)
                    .flat_map(move |oz| (-radius..=radius).map(move |ox| (dx + ox, dz + oz)))
                    .filter(|&(nx, nz)| (0..16).contains(&nx) && (0..16).contains(&nz))
            };
            let material = if wet(chunk, dx, dz) {
                if !inland_water(chunk, dx, dz) {
                    continue;
                }
                // River and lake beds.
                if roll % 3 == 0 {
                    blocks.clay
                } else {
                    blocks.gravel
                }
            } else if neighbours(1)
                .any(|(nx, nz)| inland_water(chunk, nx, nz) && top(nx, nz) >= y - 2)
            {
                // Banks.
                blocks.gravel
            } else if neighbours(2).any(|(nx, nz)| top(nx, nz) - y >= SLOPE) {
                // Scree at the foot of a slope.
                if roll % 4 == 0 {
                    blocks.cobblestone
                } else {
                    blocks.gravel
                }
            } else {
                continue;
            };
            let depth = (1 + (deposit * THICKNESS) as i32).clamp(1, MAX_DEPTH);
            // Down from the surface block, through natural ground only.
            for at in (y - depth + 1..=y).rev() {
                if !blocks.natural.contains(&chunk.block(dx, at, dz)) {
                    break;
                }
                chunk.set_block_worldgen(dx, at, dz, material)?;
            }
        }
    }
    Ok(())
}

/// A well-mixed hash of the world's seed and a column.
fn hash(seed: i32, x: i32, z: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1)
        ^ (z as u32).wrapping_mul(0x85EB_CA77)
        ^ (seed as u32).wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^ (h >> 12)
}
