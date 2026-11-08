// The `realistic_vegetation` setting (Horizon's, in neither TerraForged tree): trees follow the
// light and the water. TerraForged's vegetation configs already weigh a column's slope, height,
// noise and "saturation" (1 - river noise, which in play is the same everywhere inland); none
// knows which way a slope faces or how far the nearest water is. With the setting each config's
// fitness gains two of Horizon's rules: `Aspect`, the ground's topographic northness
// (`cos(aspect) * sin(slope)`, `TerrainData::facing`), and `WaterDistance`, the least of the
// distances to the nearest river, lake and coast (`RiverCarver::water_distance`). North-facing
// slopes, which get less sun, stay cooler and moister and grow more trees; south-facing ones
// grow fewer, as on the slopes of the Northern Hemisphere's mid latitudes (Minecraft's sun rises in the
// east and stands overhead at noon, so the north is shaded here by convention, not by the sky);
// riverbanks, lakesides and coasts grow more, dry ground far from water fewer. On level ground
// half-way out from water the fitness is the original's. The ground cover follows too
// ([`ground_cover`]): podzol and moss on shaded slopes near water, coarse dirt on sunny slopes far
// from it, in patches; only grass blocks open to the sky change, never a height.

use super::super::terrain::{TerrainBlockStates, TerrainChunk, TerrainError};
use super::terraforged_biome_viability::Viability;
use super::terraforged_terrain_data::{Facing, TerrainData};
use crate::heightmap::HeightmapType;
use crate::registry::{BlockRegistry, BlockStateId};

/// How much the most north-facing slopes add to a column's fitness (and the most south-facing take
/// away): half of the `Aspect` rule's weight.
const ASPECT_WEIGHT: f32 = 0.4;
/// The northness at which the `Aspect` rule is fully on (and, negated, fully off): a slope of 30
/// degrees facing due north (sin 30° = 0.5), or a steeper one facing further from it.
const ASPECT_RANGE: f32 = 0.5;
/// How much a column on the water's edge gains over one far from water (half each way from the
/// midpoint).
const WATER_WEIGHT: f32 = 0.3;

/// `viability` with Horizon's aspect and water rules: `clamp(fitness + 0.4 * aspect - 0.3 *
/// water - 0.05, 0, 1)`, where `aspect` ramps from 0 on slopes facing south to 1 on slopes facing
/// north (0.5 on level ground) and `water` from 0 at the water to 1 far from it.
pub fn overlay(viability: Viability) -> Viability {
    Viability::Sum {
        initial: -0.5 * ASPECT_WEIGHT + 0.5 * WATER_WEIGHT,
        rules: vec![
            viability,
            Viability::Aspect {
                facing: Facing::North,
                min: -ASPECT_RANGE,
                max: ASPECT_RANGE,
            },
            Viability::WaterDistance { min: 0.0, max: 1.0 },
        ],
        amounts: vec![1.0, ASPECT_WEIGHT, -WATER_WEIGHT],
    }
}

/// The northness from which a slope counts as shaded (or, negated, as sunny) for the ground cover (a slope of
/// about 9 degrees facing due north), and the one at which it is fully so (30 degrees).
const COVER_FACING: f32 = 0.15;
const COVER_FULL: f32 = 0.5;
/// The side, in blocks, of the squares a patch of ground cover is decided over.
const PATCH: i32 = 2;

/// The blocks of the ground cover.
pub struct CoverBlocks {
    grass: BlockStateId,
    air: BlockStateId,
    podzol: BlockStateId,
    moss: BlockStateId,
    coarse_dirt: BlockStateId,
}

impl CoverBlocks {
    /// `None` where the registry lacks one of them.
    pub fn new(registry: &BlockRegistry) -> Option<Self> {
        let state = |name: &str| registry.default_state(name);
        Some(Self {
            grass: state("minecraft:grass_block")?,
            air: state("minecraft:air")?,
            podzol: state("minecraft:podzol")?,
            moss: state("minecraft:moss_block")?,
            coarse_dirt: state("minecraft:coarse_dirt")?,
        })
    }
}

/// What the ground cover makes of a grass column facing north by `north` (`TerrainData::facing`) at `water` from
/// water, given its patch's `roll` (0 to 1): on shaded ground (northness over [`COVER_FACING`]) a share of the
/// patches turns to podzol, growing with the shade (up to [`COVER_FULL`]) from 40% far from water to all of them at
/// it, and the wettest third of those within 0.3 of water to moss; on sunny ground (under minus that) a share
/// growing with the sun, from 40% at the water to all far from it, turns to coarse dirt. `None` keeps the grass.
pub fn cover(north: f32, water: f32, roll: f32) -> Option<Cover> {
    let shade = (north.abs() - COVER_FACING) / (COVER_FULL - COVER_FACING);
    // NaN keeps the grass (`min` would turn it into 1).
    if shade.is_nan() || shade <= 0.0 || !(0.0..=1.0).contains(&water) {
        return None;
    }
    let shade = shade.min(1.0);
    if north > 0.0 {
        let share = shade * (0.4 + 0.6 * (1.0 - water));
        (roll < share).then_some(if roll < share / 3.0 && water < 0.3 {
            Cover::Moss
        } else {
            Cover::Podzol
        })
    } else {
        (roll < shade * (0.4 + 0.6 * water)).then_some(Cover::CoarseDirt)
    }
}

/// A block of the ground cover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cover {
    Podzol,
    Moss,
    CoarseDirt,
}

/// Lays the ground cover over the chunk's surface, after `Surface` and before the caves: each grass block on top of
/// its column with air above it may become podzol, moss or coarse dirt ([`cover`]), decided per 2x2 patch from
/// `seed` and the patch's position, the slope and water of the column.
pub fn ground_cover<B: TerrainBlockStates>(
    seed: i32,
    terrain: &TerrainData,
    chunk: &mut TerrainChunk<'_, B>,
    blocks: &CoverBlocks,
) -> Result<(), TerrainError> {
    let position = chunk.position();
    let mut tops = [0i32; 256];
    {
        let heights = chunk
            .heightmap(HeightmapType::OceanFloorWg)
            .ok_or(TerrainError::InvalidState)?;
        for (i, top) in tops.iter_mut().enumerate() {
            *top = heights.first_available((i & 15) as i32, (i >> 4) as i32)? - 1;
        }
    }
    for dz in 0..16 {
        for dx in 0..16 {
            let y = tops[(dz * 16 + dx) as usize];
            if chunk.block(dx, y, dz) != blocks.grass || chunk.block(dx, y + 1, dz) != blocks.air {
                continue;
            }
            let (x, z) = (
                position.x.wrapping_shl(4).wrapping_add(dx),
                position.z.wrapping_shl(4).wrapping_add(dz),
            );
            let roll = (hash(seed, x.div_euclid(PATCH), z.div_euclid(PATCH)) >> 8) as f32
                / (1u32 << 24) as f32;
            let block = match cover(
                terrain.facing(dx, dz, Facing::North),
                terrain.water_distance(dx, dz),
                roll,
            ) {
                Some(Cover::Podzol) => blocks.podzol,
                Some(Cover::Moss) => blocks.moss,
                Some(Cover::CoarseDirt) => blocks.coarse_dirt,
                None => continue,
            };
            chunk.set_block_worldgen(dx, y, dz, block)?;
        }
    }
    Ok(())
}

/// A well-mixed hash of the world's seed and a patch (another mix than `terraforged_deposits`', so the two settings
/// don't pick the same columns).
fn hash(seed: i32, x: i32, z: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x27D4_EB2F)
        ^ (z as u32).wrapping_mul(0x1656_67B1)
        ^ (seed as u32).wrapping_mul(0x9E37_79B9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^ (h >> 13)
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_biome_viability::ViabilityContext;
    use super::*;

    /// A column whose fitness rules read only a northness and a distance to water.
    struct Column {
        north: f32,
        water: f32,
    }

    impl ViabilityContext for Column {
        fn seed(&self) -> i32 {
            0
        }
        fn edge(&self) -> bool {
            false
        }
        fn max_y(&self) -> i32 {
            480
        }
        fn base_height(&self, _x: i32, _z: i32) -> i32 {
            62
        }
        fn height(&self, _x: i32, _z: i32) -> i32 {
            62
        }
        fn normalized_gradient(&self, _x: i32, _z: i32, _norm: f32) -> f32 {
            0.0
        }
        fn river(&self, _x: i32, _z: i32) -> f32 {
            0.0
        }
        fn climate_shape(&self, _seed: i32, _x: i32, _z: i32) -> f32 {
            0.0
        }
        fn facing(&self, _x: i32, _z: i32, facing: Facing) -> f32 {
            match facing {
                Facing::North => self.north,
                Facing::South => -self.north,
                Facing::East | Facing::West => 0.0,
            }
        }
        fn water_distance(&self, _x: i32, _z: i32) -> f32 {
            self.water
        }
    }

    fn fitness(base: f32, north: f32, water: f32) -> f32 {
        let rule = overlay(Viability::Sum {
            initial: base,
            rules: Vec::new(),
            amounts: Vec::new(),
        });
        rule.fitness(0, 0, &Column { north, water })
    }

    #[test]
    fn shade_and_water_raise_the_fitness_sun_and_drought_lower_it() {
        // Level ground half-way out from water keeps the original's fitness.
        for base in [0.0, 0.25, 0.5, 1.0] {
            assert!((fitness(base, 0.0, 0.5) - base).abs() < 1e-6, "{base}");
        }
        let shaded_wet = fitness(0.5, 0.5, 0.0);
        let sunny_dry = fitness(0.5, -0.5, 1.0);
        assert!((shaded_wet - 0.85).abs() < 1e-6, "{shaded_wet}");
        assert!((sunny_dry - 0.15).abs() < 1e-6, "{sunny_dry}");
        // Each rule alone moves the fitness its own way, and the sum stays in 0..1.
        assert!(fitness(0.5, 0.3, 0.5) > 0.5 && fitness(0.5, -0.3, 0.5) < 0.5);
        assert!(fitness(0.5, 0.0, 0.1) > 0.5 && fitness(0.5, 0.0, 0.9) < 0.5);
        assert_eq!(fitness(1.0, 1.0, 0.0), 1.0);
        assert_eq!(fitness(0.0, -1.0, 1.0), 0.0);
    }

    /// The rules ramp as `Saturation` does, NaN passing through; configs name them `horizon:Aspect` and
    /// `horizon:WaterDistance`.
    #[test]
    fn rules_ramp_like_saturation_and_decode_under_horizons_names() {
        let aspect = Viability::Aspect {
            facing: Facing::North,
            min: -0.5,
            max: 0.5,
        };
        let water = Viability::WaterDistance { min: 0.2, max: 0.6 };
        let at =
            |rule: &Viability, north: f32, water: f32| rule.fitness(0, 0, &Column { north, water });
        assert_eq!(at(&aspect, -0.6, 0.0), 0.0);
        assert_eq!(at(&aspect, 0.0, 0.0), 0.5);
        assert_eq!(at(&aspect, 0.5, 0.0), 1.0);
        assert_eq!(at(&aspect, 0.9, 0.0), 1.0);
        assert!(at(&aspect, f32::NAN, 0.0).is_nan());
        assert_eq!(at(&water, 0.0, 0.1), 0.0);
        assert!((at(&water, 0.0, 0.4) - 0.5).abs() < 1e-6);
        assert_eq!(at(&water, 0.0, 0.7), 1.0);
        assert!(at(&water, 0.0, f32::NAN).is_nan());
        let config = |rule: &str| {
            super::super::terraforged_vegetation::decode_config(&format!(
                "{{\"biomes\":\"#terraforged:trees/copses\",\"viability\":{rule}}}"
            ))
        };
        let decoded = config(r#"{"type":"horizon:Aspect","facing":"south","min":0.1}"#).unwrap();
        assert!(matches!(
            decoded.viability,
            Viability::Aspect { facing: Facing::South, min, max } if min == 0.1 && max == 1.0
        ));
        let decoded = config(r#"{"type":"horizon:WaterDistance","max":0.5}"#).unwrap();
        assert!(matches!(
            decoded.viability,
            Viability::WaterDistance { min, max } if min == 0.0 && max == 0.5
        ));
        assert!(config(r#"{"type":"horizon:Aspect","facing":"up"}"#).is_err());
        assert!(config(r#"{"type":"Aspect"}"#).is_err());
    }

    /// TerraForged's bundled configs use none of Horizon's rules; the overlay keeps each one's own rule first, at
    /// weight 1.
    #[test]
    fn the_bundled_configs_are_faithful_until_overlaid() {
        fn horizons(rule: &Viability) -> bool {
            match rule {
                Viability::Aspect { .. } | Viability::WaterDistance { .. } => true,
                Viability::Multiply(rules) | Viability::Sum { rules, .. } => {
                    rules.iter().any(horizons)
                }
                _ => false,
            }
        }
        let configs = super::super::terraforged_vegetation::current_vegetation().unwrap();
        assert_eq!(configs.len(), 8);
        for config in configs {
            assert!(!horizons(&config.viability));
            let Viability::Sum { rules, amounts, .. } = overlay(config.viability) else {
                panic!("not a sum");
            };
            assert!(matches!(rules[0], Viability::Sum { .. }) && !horizons(&rules[0]));
            assert_eq!(amounts[0], 1.0);
            assert!(horizons(&rules[1]) && horizons(&rules[2]));
        }
    }

    /// Shaded wet ground grows podzol and moss, sunny dry ground coarse dirt, more of it the stronger the slope's
    /// facing and the water's pull; level ground, sunny wet ground and shaded dry ground keep their grass.
    #[test]
    fn ground_cover_follows_the_shade_and_the_water() {
        let share = |north: f32, water: f32, kind: Cover| {
            (0..1000)
                .filter(|i| cover(north, water, *i as f32 / 1000.0) == Some(kind))
                .count()
        };
        for kind in [Cover::Podzol, Cover::Moss, Cover::CoarseDirt] {
            assert_eq!(share(0.0, 0.0, kind) + share(0.15, 0.5, kind), 0);
            assert_eq!(share(0.6, f32::NAN, kind) + share(f32::NAN, 0.0, kind), 0);
        }
        // Water draws podzol and moss, its absence coarse dirt.
        assert!(share(0.9, 1.0, Cover::Podzol) > 300 && share(0.9, 1.0, Cover::Podzol) < 450);
        assert!(share(0.9, 0.0, Cover::Podzol) + share(0.9, 0.0, Cover::Moss) == 1000);
        assert!(share(-0.9, 0.0, Cover::CoarseDirt) < share(-0.9, 1.0, Cover::CoarseDirt));
        assert!(share(0.4, 0.1, Cover::Podzol) > share(0.2, 0.1, Cover::Podzol));
        assert!(share(0.8, 0.1, Cover::Moss) > 0 && share(0.8, 0.5, Cover::Moss) == 0);
        assert!(share(-0.8, 0.9, Cover::CoarseDirt) > share(-0.8, 0.5, Cover::CoarseDirt));
        assert_eq!(
            share(-0.8, 0.9, Cover::Podzol) + share(0.8, 0.1, Cover::CoarseDirt),
            0
        );
        // A patch is decided once, whichever column of it asks.
        assert_eq!(hash(5, 3, -4), hash(5, 3, -4));
        assert_ne!(hash(5, 3, -4), hash(6, 3, -4));
    }
}
