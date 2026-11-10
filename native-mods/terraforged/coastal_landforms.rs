// The `coastal_landforms` setting (Horizon's, in neither TerraForged tree): cliffs on exposed coasts, tidal flats in
// sheltered ones. TerraForged's shore is one profile everywhere: in continent noise from `BEACH` to `COAST` the land
// rises linearly from the sea level to the land's height (`NoiseGenerator.getBlend`), and from `SHALLOW_OCEAN` to
// `BEACH` the sea floor rises linearly to the sea level. Real coasts sort by the wave energy that reaches them and the
// rock they are cut in (Sunamura 1992; Trenhaile 1987): exposed coasts of resistant rock retreat as steep cliffs, while
// sheltered coasts with fine sediment build wide, nearly flat intertidal mud and sand flats (Dyer 1998; Masselink &
// Hughes 2003). This setting reshapes the two linear profiles by a "wave exposure" field:
//
// - exposure: two octaves of low-frequency simplex noise (900 and 250 blocks) of the seed, plus, with `rock_strata`,
//   a bias by the hardness of the rock at the sea level (hard bands hold cliffs, soft ones give way to flats);
// - a cliff where exposure is high: within the first [`CLIFF_WIDTH`] of the coast band the land rises
//   [`CLIFF_HEIGHT`] blocks more than the faithful ramp (`1 - (1 - a / w)²`: steepest at the foot, a rounded top),
//   never above the land behind it, then follows the ramp up. The shore is a wall of rock: TerraForged's `Surface`
//   turns its steep face to stone, and the beach biome on its top becomes a stony shore. The sea floor at its foot
//   is TerraForged's, a few blocks deep, like a wave-cut platform;
// - a tidal flat where exposure is low: the land rises as `a²` (a low coastal plain), and the outer [`FLAT_WIDTH`]
//   of the shallow sea floor rises to half a block under the sea level, so it is covered by one block of water.
//
// The change is a height offset per column, added to the eroded height before the rivers carve it, both where chunks
// are filled and where structures and the distant terrain read heights. It is a function of the seed, the column's
// own position, its un-eroded continent sample and the height it is added to, so chunks agree at every border,
// whatever the threads or order.
// Off, nothing calls it and the world is bit-exact.

use super::terraforged::simplex;
use super::terraforged_biome_sampler::{BEACH, COAST, SHALLOW_OCEAN};
use super::terraforged_terrain_data::NoiseSample;
use super::terraforged_terrain_levels::NoiseLevels;

/// With `rock_strata`, the hardness of the rock at the sea level in column `(x, z)` (`terraforged_rock_strata`).
pub fn sea_level_hardness(seed: i32, x: i32, z: i32, sea_level: i32) -> f32 {
    super::terraforged_rock_strata::Strata::new(seed)
        .area(x, z, 1, 1)
        .rock_at(x, sea_level, z)
        .hardness()
}

/// The large octave of the exposure noise, in blocks: a stretch of coast stays exposed or sheltered over a few
/// hundred blocks.
pub const EXPOSURE_SCALE: f32 = 900.0;
/// The small octave, in blocks: headlands and coves within a stretch.
pub const DETAIL_SCALE: f32 = 250.0;
/// How much of the exposure the small octave makes up.
pub const DETAIL_WEIGHT: f32 = 0.35;
/// Exposure from which a coast starts to steepen into a cliff, and at which it is a full cliff.
pub const CLIFF_FROM: f32 = 0.55;
pub const CLIFF_FULL: f32 = 0.7;
/// Exposure under which a coast starts to flatten, and under which it is a full tidal flat.
pub const FLAT_FROM: f32 = 0.42;
pub const FLAT_FULL: f32 = 0.28;
/// The share of the coast band (`BEACH..COAST` in continent noise, about 100 to 400 blocks wide on seed 777) a
/// cliff rises in.
pub const CLIFF_WIDTH: f32 = 0.06;
/// How many blocks a full cliff stands over the faithful ramp, at most (real sea cliffs are mostly 10 to 100 m).
pub const CLIFF_HEIGHT: f32 = 30.0;
/// The share of the shallow sea floor band (`SHALLOW_OCEAN..BEACH`, about 70 to 220 blocks wide), from the shore out,
/// a tidal flat covers (its outer half ramps down to the faithful sea floor).
pub const FLAT_WIDTH: f32 = 0.4;
/// How far under the sea level a tidal flat's floor lies, in blocks: its top block is one under the water's.
pub const FLAT_DEPTH: f32 = 0.5;
/// With `rock_strata`, how much the rock's hardness (0.2 for stone to 0.8 for granite) moves the exposure, around
/// [`HARDNESS_MIDDLE`].
pub const HARDNESS_BIAS: f32 = 0.5;
pub const HARDNESS_MIDDLE: f32 = 0.5;

/// The world's coasts: its seed and noise levels.
#[derive(Clone, Copy, Debug)]
pub struct Coasts {
    seed: i32,
    levels: NoiseLevels,
}

/// Which landform a column's exposure makes, and how strongly (0 to 1): the faithful shore has both at 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Landform {
    pub cliff: f32,
    pub flat: f32,
}

impl Coasts {
    pub const fn new(seed: i32, levels: NoiseLevels) -> Self {
        Self { seed, levels }
    }

    /// The wave exposure at block `(x, z)`: about 0 to 1, from the seed alone.
    pub fn exposure(&self, x: i32, z: i32) -> f32 {
        let (x, z) = (x as f32, z as f32);
        let large = simplex(
            x / EXPOSURE_SCALE,
            z / EXPOSURE_SCALE,
            self.seed.wrapping_add(7919),
        );
        let detail = simplex(
            x / DETAIL_SCALE,
            z / DETAIL_SCALE,
            self.seed.wrapping_add(7927),
        );
        0.5 + 0.5 * ((1.0 - DETAIL_WEIGHT) * large + DETAIL_WEIGHT * detail)
    }

    /// The landform at `(x, z)`, where the rock at the sea level has `hardness` (`None` without `rock_strata`).
    pub fn landform(&self, x: i32, z: i32, hardness: Option<f32>) -> Landform {
        let bias = hardness.map_or(0.0, |h| HARDNESS_BIAS * (h - HARDNESS_MIDDLE));
        let exposure = self.exposure(x, z) + bias;
        Landform {
            cliff: smoothstep(CLIFF_FROM, CLIFF_FULL, exposure),
            flat: smoothstep(FLAT_FROM, FLAT_FULL, exposure),
        }
    }

    /// Whether a beach (TerraForged's beach band) at `(x, z)` lies on a cliff's top, and so is a stony shore.
    pub fn stony_shore(&self, x: i32, z: i32, hardness: Option<f32>) -> bool {
        self.landform(x, z, hardness).cliff > 0.5
    }

    /// Whether the un-eroded sample lies in a band the landforms reshape (`SHALLOW_OCEAN..COAST`); outside it (and for
    /// NaN) [`Self::offset`] is 0, so callers can skip the hardness lookup.
    pub fn reshapes(sample: &NoiseSample) -> bool {
        (SHALLOW_OCEAN..COAST).contains(&sample.continent_noise)
    }

    /// The height noise to add to the column's height, from its sample (`sample_terrain`'s continent noise and
    /// [`NoiseSample::coast_land_height`], with `height_noise` the height the offset is added to: the eroded one in
    /// chunks, the raw one in distant tiles) and its landform. A tidal flat lifts the sea floor from that height, so
    /// erosion's deepening of the near-shore floor doesn't leave the flat under water.
    pub fn offset(&self, sample: &NoiseSample, landform: Landform) -> f32 {
        let continent = sample.continent_noise;
        let sea = self.levels.height_min;
        if (BEACH..COAST).contains(&continent) {
            // The land: a cliff stands up to `CLIFF_HEIGHT` over the faithful `lerp(sea, land, a)`, never over the
            // land; a flat lowers it to `lerp(sea, land, a²)`.
            let rise = sample.coast_land_height - sea;
            if rise.is_nan() || rise <= 0.0 {
                return 0.0;
            }
            let a = (continent - BEACH) / (COAST - BEACH);
            let cliff = (CLIFF_HEIGHT * self.levels.unit * cliff_profile(a)).min(rise * (1.0 - a));
            let flat = (a * a - a) * rise;
            landform.cliff * cliff + landform.flat * flat
        } else if (SHALLOW_OCEAN..BEACH).contains(&continent) {
            // The sea floor: a tidal flat lifts the outer part toward half a block under the sea level.
            let b = (continent - SHALLOW_OCEAN) / (BEACH - SHALLOW_OCEAN);
            let reach = smoothstep(1.0 - FLAT_WIDTH, 1.0 - FLAT_WIDTH / 2.0, b);
            let target = sea - FLAT_DEPTH * self.levels.unit;
            let floor = sample.height_noise;
            if floor.is_nan() {
                return 0.0;
            }
            landform.flat * reach * (target - floor)
        } else {
            0.0
        }
    }
}

/// A cliff's share of its height at `a` (0 at the shore, 1 at `COAST`): all of it from [`CLIFF_WIDTH`] on, steepest
/// at the foot.
fn cliff_profile(a: f32) -> f32 {
    if a >= CLIFF_WIDTH {
        1.0
    } else {
        let t = 1.0 - a / CLIFF_WIDTH;
        1.0 - t * t
    }
}

/// 0 at `from`, 1 at `to` (either order), smooth in between.
fn smoothstep(from: f32, to: f32, value: f32) -> f32 {
    let t = ((value - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_engine::TerrainRegistry;
    use super::super::terraforged_terrain_levels::TerrainLevels;
    use super::*;

    fn coasts() -> Coasts {
        Coasts::new(777, TerrainLevels::default().noise_levels)
    }

    /// A synthetic shore sample: continent noise `c`, the faithful height and the land height `land` blocks over the
    /// sea level.
    fn shore(c: f32, land: f32) -> NoiseSample {
        let levels = TerrainLevels::default().noise_levels;
        let mut sample = NoiseSample::new(&TerrainRegistry::new());
        let sea = levels.height_min;
        sample.continent_noise = c;
        sample.coast_land_height = sea + land * levels.unit;
        sample.height_noise = if c >= BEACH {
            sea + (c - BEACH) / (COAST - BEACH) * land * levels.unit
        } else {
            // A sea floor 30 blocks deep at SHALLOW_OCEAN.
            sea - (BEACH - c) / (BEACH - SHALLOW_OCEAN) * 30.0 * levels.unit
        };
        sample
    }

    fn blocks(value: f32) -> f32 {
        value / TerrainLevels::default().noise_levels.unit
    }

    const CLIFF: Landform = Landform {
        cliff: 1.0,
        flat: 0.0,
    };
    const FLAT: Landform = Landform {
        cliff: 0.0,
        flat: 1.0,
    };
    const FAITHFUL: Landform = Landform {
        cliff: 0.0,
        flat: 0.0,
    };

    /// A cliff lifts the shore `CLIFF_HEIGHT` over the ramp within `CLIFF_WIDTH` of the band, never above the land
    /// behind it, then follows the ramp up; nothing changes at the band's edges, on the sea floor or inland.
    #[test]
    fn cliffs_rise_the_land_at_the_shore() {
        let c = coasts();
        let at = |a: f32| BEACH + a * (COAST - BEACH);
        for land in [20.0, 100.0] {
            for a in [0.0, 0.01, 0.03, 0.06, 0.1, 0.2, 0.5, 0.75, 0.9] {
                let sample = shore(at(a), land);
                let height =
                    blocks(sample.height_noise - c.levels.height_min + c.offset(&sample, CLIFF));
                let want = (land * a + CLIFF_HEIGHT * cliff_profile(a)).min(land);
                assert!((height - want).abs() < 0.01, "{land} {a}: {height} {want}");
            }
        }
        // The face: most of its height in the first third of its width, steepest at the foot.
        assert!(blocks(c.offset(&shore(at(CLIFF_WIDTH / 3.0), 100.0), CLIFF)) > 16.0);
        for (continent, land) in [
            (BEACH, 40.0),
            (COAST, 40.0),
            (0.4, 40.0),
            (0.7, 40.0),
            (at(0.3), -5.0),
        ] {
            assert_eq!(c.offset(&shore(continent, land), CLIFF), 0.0, "{continent}");
        }
        assert_eq!(c.offset(&shore(at(0.3), 40.0), FAITHFUL), 0.0);
        assert_eq!(c.offset(&shore(f32::NAN, 40.0), CLIFF), 0.0);
    }

    /// A tidal flat keeps the land low near the shore and lifts the outer sea floor to half a block under the sea
    /// level, so it is under one block of water or less, and leaves the deeper floor alone.
    #[test]
    fn tidal_flats_are_shallow_and_low() {
        let c = coasts();
        let sea = c.levels.height_min;
        let land = shore(BEACH + 0.4 * (COAST - BEACH), 40.0);
        let height = blocks(land.height_noise - sea + c.offset(&land, FLAT));
        assert!((height - 40.0 * 0.16).abs() < 0.01, "{height}");
        for b in [1.0 - FLAT_WIDTH / 2.0, 0.9, 0.99] {
            let sample = shore(SHALLOW_OCEAN + b * (BEACH - SHALLOW_OCEAN), 40.0);
            let depth = blocks(sea - sample.height_noise - c.offset(&sample, FLAT));
            assert!((depth - FLAT_DEPTH).abs() < 0.01, "{b}: {depth}");
        }
        let deep = shore(
            SHALLOW_OCEAN + (1.0 - FLAT_WIDTH) * (BEACH - SHALLOW_OCEAN),
            40.0,
        );
        assert_eq!(c.offset(&deep, FLAT), 0.0);
        // Between the full flat and the deeper floor the floor only rises, and stays under the flat.
        let mid = shore(
            SHALLOW_OCEAN + (1.0 - 0.75 * FLAT_WIDTH) * (BEACH - SHALLOW_OCEAN),
            40.0,
        );
        let lifted = c.offset(&mid, FLAT);
        assert!(lifted > 0.0 && blocks(sea - mid.height_noise - lifted) > FLAT_DEPTH);
    }

    /// The exposure is a smooth field of the seed: both landforms are common, a column's never depends on anything
    /// but its position, and neighbouring columns differ by little (no seams).
    #[test]
    fn exposure_sorts_coasts_smoothly() {
        let c = coasts();
        let (mut cliffs, mut flats, mut largest) = (0, 0, 0.0f32);
        let n = 200;
        for i in 0..n * n {
            let (x, z) = ((i % n) * 61 - 6000, (i / n) * 67 - 6500);
            let form = c.landform(x, z, None);
            cliffs += usize::from(form.cliff > 0.5);
            flats += usize::from(form.flat > 0.5);
            assert!(form.cliff == 0.0 || form.flat == 0.0);
            largest = largest
                .max((c.exposure(x, z) - c.exposure(x + 1, z)).abs())
                .max((c.exposure(x, z) - c.exposure(x, z + 1)).abs());
            assert_eq!(c.landform(x, z, None), form);
        }
        let total = (n * n) as usize;
        assert!(
            cliffs * 6 > total && cliffs * 2 < total,
            "{cliffs} of {total}"
        );
        assert!(flats * 6 > total && flats * 2 < total, "{flats} of {total}");
        assert!(largest < 0.01, "{largest}");
        assert_ne!(
            Coasts::new(778, c.levels).exposure(100, 100),
            c.exposure(100, 100)
        );
    }

    /// With `rock_strata`, hard rock makes cliffs where soft rock makes flats.
    #[test]
    fn hard_rock_holds_cliffs() {
        let c = coasts();
        let (mut hard, mut soft) = (0.0, 0.0);
        for i in 0..10_000 {
            let (x, z) = ((i % 100) * 97, (i / 100) * 89);
            hard += c.landform(x, z, Some(0.8)).cliff - c.landform(x, z, Some(0.8)).flat;
            soft += c.landform(x, z, Some(0.2)).cliff - c.landform(x, z, Some(0.2)).flat;
        }
        assert!(hard > soft + 2000.0, "{hard} {soft}");
    }
}
