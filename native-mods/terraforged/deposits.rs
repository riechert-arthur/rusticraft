// The `erosion_deposits` setting: what TerraForged's erosion pass deposited shows on
// the surface. Where erosion left a column higher than the ground around it (relative
// to the terrain before erosion), the sediment settles as
// gravel and clay in river and lake beds, gravel on the banks beside them, and scree
// (gravel and cobblestone) at the foot of steeper ground. Not in the original, which
// only keeps the eroded heights.

use super::super::terrain::{TerrainBlockStates, TerrainChunk, TerrainError};
use super::terraforged_terrain_data::TerrainData;
use crate::heightmap::{HeightmapState, HeightmapType};
use crate::registry::{BlockRegistry, BlockStateId};

/// How much higher than the ground around it (in blocks, after erosion against before)
/// a column must come out to hold sediment: where it was worn down no more than its
/// surroundings, what was washed off them settles.
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

/// Lay the chunk's deposits over its surface.
pub fn apply<B: TerrainBlockStates>(
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
    let underwater = |chunk: &TerrainChunk<'_, B>, dx: i32, dz: i32| {
        chunk
            .block_properties(chunk.block(dx, top(dx, dz) + 1, dz))
            .has_fluid()
    };
    for dz in 0..16 {
        for dx in 0..16 {
            // Erosion lowers nearly everything a little: sediment settles where a column
            // came out higher than the ground around it.
            let deposit = terrain.deposit_at(dx, dz) - local_mean(terrain, dx, dz);
            if deposit < MIN_DEPTH {
                continue;
            }
            let y = top(dx, dz);
            let (x, z) = (position.x * 16 + dx, position.z * 16 + dz);
            let roll = hash(x, z);
            let neighbours = || {
                (-2..=2)
                    .flat_map(move |oz| (-2..=2).map(move |ox| (dx + ox, dz + oz)))
                    .filter(|&(nx, nz)| (0..16).contains(&nx) && (0..16).contains(&nz))
            };
            let material = if underwater(chunk, dx, dz) {
                // River and lake beds.
                if roll % 3 == 0 {
                    blocks.clay
                } else {
                    blocks.gravel
                }
            } else if neighbours().any(|(nx, nz)| {
                (nx - dx).abs() <= 1
                    && (nz - dz).abs() <= 1
                    && underwater(chunk, nx, nz)
                    && top(nx, nz) >= y - 2
            }) {
                // Banks.
                blocks.gravel
            } else if neighbours().any(|(nx, nz)| top(nx, nz) - y >= SLOPE) {
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
            for down in 0..depth {
                let at = y - down;
                if blocks.natural.contains(&chunk.block(dx, at, dz)) {
                    chunk.set_block_worldgen(dx, at, dz, material)?;
                }
            }
        }
    }
    Ok(())
}

/// The mean erosion change over the 7x7 columns around one, within the chunk.
fn local_mean(terrain: &TerrainData, dx: i32, dz: i32) -> f32 {
    let (mut sum, mut count) = (0.0, 0.0);
    for nz in (dz - 3).max(0)..=(dz + 3).min(15) {
        for nx in (dx - 3).max(0)..=(dx + 3).min(15) {
            sum += terrain.deposit_at(nx, nz);
            count += 1.0;
        }
    }
    sum / count
}

fn hash(x: i32, z: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (z as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^ (h >> 12)
}
