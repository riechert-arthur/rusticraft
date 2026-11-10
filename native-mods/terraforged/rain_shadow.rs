// Horizon's rain shadow (in neither TerraForged tree), the `rain_shadow` setting (#280): moisture from the terrain
// upwind. TerraForged's moisture is 2D noise alone, so a desert can sit on a range's windward side and a rainforest in
// its lee.
//
// Clean-room, from published methods, much simplified: an air parcel carried along a prevailing wind loses moisture
// where the terrain forces it up (orographic precipitation, as in the linear upslope models of Smith 1979 and Smith &
// Barstad 2004, "A linear theory of orographic precipitation"), sinks and dries in the lee (the rain shadow), and
// takes moisture back up over the sea and, slowly, over land. Every value is a pure function of the seed, the settings
// and a biome cell's key: the profile reads only pointwise terrain samples along a fixed ray, and the cache below only
// remembers results.

use super::terraforged_current_climate::BiomeCell;
use super::terraforged_lossy_cache::ConcurrentLossyCache;
use super::terraforged_noise_generator::NoiseGenerator;

/// Blocks between two terrain samples along the wind.
pub const STEP: f32 = 32.0;
/// How far upwind the parcel starts, in blocks (64 steps).
pub const REACH: f32 = 2048.0;
/// Samples on each side of a point in the smoothing window (9 samples, 288 blocks): the parcel feels ranges and hills,
/// not every bump.
const HALF: usize = 4;
/// Steps of [`REACH`].
const STEPS: usize = (REACH / STEP) as usize;
/// Samples along the ray: the reach, the smoothing window's half before the start, and its half past the cell.
pub const SAMPLES: usize = STEPS + 2 * HALF + 1;
/// The ray's index of the cell itself.
const TARGET: usize = SAMPLES - 1 - HALF;

/// The model's constants (blocks): see the README's "Rain shadow".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    /// A parcel lifted by this much rains out all but 1/e of its moisture.
    pub rain_height: f64,
    /// Over the sea the moisture deficit recovers by 1/e every this many blocks.
    pub sea_recovery: f64,
    /// Over land it recovers that much slower (evaporation and mixing).
    pub land_recovery: f64,
    /// Past a summit the parcel sinks this many blocks per block it travels, unless the ground is higher.
    pub subsidence: f64,
    /// The rain of the last [`WINDWARD`] steps, times this, is added to the moisture: windward slopes are wetter.
    pub windward_gain: f64,
}

/// Steps of the windward window: 256 blocks.
const WINDWARD: usize = 8;

/// The setting's constants.
pub const PARAMS: Params = Params {
    rain_height: 120.0,
    sea_recovery: 256.0,
    land_recovery: 1024.0,
    subsidence: 0.125,
    windward_gain: 2.0,
};

/// The prevailing wind: one direction for the whole world, from the west (the mid-latitudes' westerlies), blowing
/// towards +x. A range's west face is its windward side, its east its lee.
pub const WIND: (f32, f32) = (1.0, 0.0);

/// The moisture factor of a point from its upwind profile: `heights[i]` is the terrain (blocks) at
/// `(i - TARGET) * STEP` blocks along the wind from the point, [`SAMPLES`] of them, `sea` the sea level. 1 on flat
/// ground and over open sea; under 1 in a range's lee, over 1 on its windward slopes. Never NaN: a NaN height reads as
/// the sea.
pub fn factor(heights: &[f32; SAMPLES], sea: f32, params: Params) -> f32 {
    let sea = f64::from(sea);
    // The ground the air flows over: the water surface over the sea.
    let ground: Vec<f64> = heights
        .iter()
        .map(|&h| {
            let h = f64::from(h);
            if h > sea { h } else { sea }
        })
        .collect();
    let smooth = |i: usize| ground[i - HALF..=i + HALF].iter().sum::<f64>() / (2 * HALF + 1) as f64;
    let sea_keep = (-f64::from(STEP) / params.sea_recovery).exp();
    let land_keep = (-f64::from(STEP) / params.land_recovery).exp();
    let mut moisture = 1.0f64;
    let mut parcel = smooth(HALF);
    let mut windward = 0.0f64;
    for i in HALF + 1..=TARGET {
        let surface = smooth(i);
        let keep = if ground[i] <= sea {
            sea_keep
        } else {
            land_keep
        };
        moisture = 1.0 - (1.0 - moisture) * keep;
        let lift = surface - parcel;
        let rain = if lift > 0.0 {
            parcel = surface;
            moisture * (1.0 - (-lift / params.rain_height).exp())
        } else {
            parcel = (parcel - f64::from(STEP) * params.subsidence).max(surface);
            0.0
        };
        moisture -= rain;
        if i > TARGET - WINDWARD {
            windward += rain;
        }
    }
    let factor = moisture + params.windward_gain * windward;
    if factor.is_finite() {
        factor as f32
    } else {
        1.0
    }
}

/// TerraForged's moisture `base` (0..1) under the rain shadow's `factor`, held to 0..1.
pub fn moisture(base: f32, factor: f32) -> f32 {
    (base * factor).clamp(0.0, 1.0)
}

/// The `rain_shadow` setting's map: one factor per biome cell (the climate gives a whole cell its moisture), measured
/// at the cell's point, memoised by cell. A miss samples [`SAMPLES`] terrain columns (about 0.1 ms in the fast profile);
/// every biome sample of the cell after it is a lookup. No drainage tiles: the climate is queried by the spawn search,
/// `/locate biome` and the maps, which must stay cheap.
pub struct RainShadow {
    cache: ConcurrentLossyCache<f32>,
}

impl Default for RainShadow {
    fn default() -> Self {
        Self::new()
    }
}

impl RainShadow {
    pub fn new() -> Self {
        Self {
            cache: ConcurrentLossyCache::concurrent(4096),
        }
    }

    /// The moisture of biome `cell` at runtime `seed`, from TerraForged's `base` moisture and `noise`'s terrain.
    pub fn moisture(&self, noise: &NoiseGenerator, seed: i32, cell: BiomeCell, base: f32) -> f32 {
        let key = (i64::from(cell.x) << 32) | i64::from(cell.z as u32);
        let factor = *self
            .cache
            .compute_if_absent(seed, key, |seed, _| cell_factor(noise, seed, cell));
        moisture(base, factor)
    }
}

/// [`factor`] at `cell`'s point over `noise`'s un-eroded terrain (TerraForged's `sampleTerrain`: no rivers, no Horizon
/// setting), along [`WIND`].
pub fn cell_factor(noise: &NoiseGenerator, seed: i32, cell: BiomeCell) -> f32 {
    factor(
        &profile(noise, seed, cell),
        noise.levels().sea_level as f32,
        PARAMS,
    )
}

/// [`factor`]'s heights at `cell`'s point: [`SAMPLES`] columns of `noise`'s un-eroded terrain along [`WIND`] (a column
/// that fails to sample reads as the sea).
pub fn profile(noise: &NoiseGenerator, seed: i32, cell: BiomeCell) -> [f32; SAMPLES] {
    let levels = noise.levels();
    let frequency = levels.noise_levels.frequency;
    let mut sample =
        super::terraforged_terrain_data::NoiseSample::new(&noise.continent().context().terrain);
    let mut blender = noise.blender_resource();
    let sea = levels.sea_level as f32;
    let mut heights = [sea; SAMPLES];
    for (i, height) in heights.iter_mut().enumerate() {
        let along = (i as f32 - TARGET as f32) * STEP * frequency;
        let x = cell.noise_x + WIND.0 * along;
        let z = cell.noise_z + WIND.1 * along;
        if noise
            .sample_terrain(seed, x, z, &mut sample, &mut blender)
            .is_ok()
        {
            *height = levels.scaled_height(sample.height_noise);
        }
    }
    heights
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEA: f32 = 62.0;

    /// A ridge `top` blocks over a plain at y 100, `width` blocks across its foot, its crest at `crest` blocks along
    /// the wind from the origin; the profile of a point `at` blocks along the wind.
    fn ridge(at: f32, crest: f32, top: f32, width: f32) -> [f32; SAMPLES] {
        let mut heights = [0.0; SAMPLES];
        for (i, h) in heights.iter_mut().enumerate() {
            let x = at + (i as f32 - TARGET as f32) * STEP;
            let d = ((x - crest) / (width / 2.0)).abs();
            *h = 100.0 + if d < 1.0 { top * (1.0 - d * d) } else { 0.0 };
        }
        heights
    }

    #[test]
    fn flat_ground_and_open_sea_keep_terraforged_s_moisture() {
        for level in [100.0, 62.0, 20.0, -40.0, f32::NAN] {
            let flat = [level; SAMPLES];
            assert_eq!(factor(&flat, SEA, PARAMS), 1.0, "{level}");
        }
        for base in [0.0, 0.3, 0.55, 1.0] {
            assert_eq!(moisture(base, 1.0).to_bits(), base.to_bits());
        }
    }

    #[test]
    fn windward_slopes_are_wetter_than_the_lee() {
        // A 300-block range 1200 blocks across, its crest at x 0; the wind blows towards +x.
        let at = |x: f32| factor(&ridge(x, 0.0, 300.0, 1200.0), SEA, PARAMS);
        let windward = at(-300.0);
        let summit = at(0.0);
        let lee = at(150.0);
        let far_lee = at(1500.0);
        eprintln!("windward {windward} summit {summit} lee {lee} far lee {far_lee}");
        assert!(windward > 1.2, "{windward}");
        assert!(lee < 0.7, "{lee}");
        assert!(
            windward > summit && summit > lee,
            "{windward} {summit} {lee}"
        );
        // The lee recovers slowly with distance, but stays drier than the plain upwind.
        assert!(far_lee > lee && far_lee < 0.95, "{far_lee} {lee}");
        assert_eq!(at(-1800.0), 1.0);
        // No step as the range leaves the ray's far end: the deficit has recovered by then.
        let mut last = at(1000.0);
        for x in (1000..2600).step_by(50) {
            let f = at(x as f32);
            assert!((f - last).abs() < 0.05, "{x}: {last} -> {f}");
            last = f;
        }
        assert!((at(2300.0) - 1.0).abs() < 0.01, "{}", at(2300.0));
        // A taller range casts a deeper shadow.
        let tall = factor(&ridge(400.0, 0.0, 450.0, 1200.0), SEA, PARAMS);
        assert!(tall < at(400.0), "{tall} {}", at(400.0));
        // Every factor stays finite and positive.
        for x in (-2400..2400).step_by(37) {
            let f = at(x as f32);
            assert!(
                f.is_finite() && f > 0.0 && f < 1.0 + PARAMS.windward_gain as f32,
                "{x}: {f}"
            );
        }
    }

    #[test]
    fn the_sea_gives_the_moisture_back() {
        // The same range with 300 blocks of sea between its foot and the point: the air comes in moister.
        let mut heights = ridge(1000.0, 0.0, 300.0, 1200.0);
        let dry = factor(&heights, SEA, PARAMS);
        for (i, h) in heights.iter_mut().enumerate() {
            let x = 1000.0 + (i as f32 - TARGET as f32) * STEP;
            if (650.0..950.0).contains(&x) {
                *h = 30.0;
            }
        }
        let wet = factor(&heights, SEA, PARAMS);
        assert!(dry < 0.85 && wet > dry + 0.1 && wet <= 1.0, "{dry} {wet}");
    }

    #[test]
    fn small_hills_cast_no_shadow() {
        // Hills 12 blocks high every 96 blocks: the smoothed ground barely rises.
        let mut heights = [0.0; SAMPLES];
        for (i, h) in heights.iter_mut().enumerate() {
            *h = 90.0
                + 12.0
                    * ((i as f32 * STEP / 96.0) * std::f32::consts::TAU)
                        .sin()
                        .abs();
        }
        let f = factor(&heights, SEA, PARAMS);
        assert!((0.9..1.1).contains(&f), "{f}");
    }

    /// Seed 777's biome cells over 18432 blocks around the origin, with and without the setting: only moisture and
    /// the climate type change, a cell's factor is the same through the cache from any thread, and cells with a range
    /// upwind are drier than cells with flat ground upwind.
    #[test]
    fn terraforged_seed_777_has_rain_shadows() {
        use super::super::terraforged_current_biome_sampler::CurrentBiomeSampler;
        use super::super::terraforged_generator_preset::{
            build, build_noise_generator, terrain_levels,
        };
        use std::collections::BTreeMap;
        let levels = terrain_levels(None).unwrap();
        let seed = {
            let generator = build(levels).unwrap();
            generator.capture_seed(777);
            generator.seed()
        };
        let faithful = build_noise_generator(levels).unwrap();
        let shadowed = build_noise_generator(levels).unwrap().with_rain_shadow();
        let (off, on) = (
            CurrentBiomeSampler::new(&faithful),
            CurrentBiomeSampler::new(&shadowed),
        );
        let sea = levels.sea_level as f32;
        // One point per biome cell: the first column sampled in it.
        let mut cells = BTreeMap::new();
        let start = std::time::Instant::now();
        let (mut columns, mut changed_types) = (0, 0);
        for i in 0..96 * 96 {
            let (x, z) = (-9216 + (i % 96) * 192, -9216 + (i / 96) * 192);
            let (a, b) = (off.get_sample(seed, x, z), on.get_sample(seed, x, z));
            let bits = |n: &super::super::terraforged_terrain_data::NoiseSample| {
                (
                    n.continent_center,
                    n.continent_noise.to_bits(),
                    n.river_noise.to_bits(),
                    n.terrain_type.id(),
                )
            };
            assert_eq!(bits(&a.noise), bits(&b.noise));
            assert_eq!(a.biome_noise.to_bits(), b.biome_noise.to_bits());
            assert_eq!(a.biome_edge_noise.to_bits(), b.biome_edge_noise.to_bits());
            assert_eq!(a.temperature.to_bits(), b.temperature.to_bits());
            assert!((0.0..=1.0).contains(&b.moisture), "{x} {z}: {}", b.moisture);
            columns += 1;
            if a.climate_type != b.climate_type {
                changed_types += 1;
            }
            let cell = off.cell(seed, x, z);
            cells.entry((cell.x, cell.z)).or_insert((
                cell,
                a.moisture,
                b.moisture,
                a.climate_type,
                b.climate_type,
                (x, z),
            ));
        }
        let elapsed = start.elapsed();
        // The factor through the cache is the direct one, from six threads in another order on a fresh map.
        let fresh = build_noise_generator(levels).unwrap().with_rain_shadow();
        let list: Vec<_> = cells.values().copied().collect();
        std::thread::scope(|scope| {
            for t in 0..6 {
                let (fresh, list) = (&fresh, &list);
                scope.spawn(move || {
                    let shadow = fresh.rain_shadow().unwrap();
                    for (cell, base, moisture, ..) in list.iter().rev().skip(t).step_by(6) {
                        assert_eq!(
                            shadow.moisture(fresh, seed, *cell, *base).to_bits(),
                            moisture.to_bits()
                        );
                    }
                });
            }
        });
        let (mut flat, mut ranged) = (Vec::new(), Vec::new());
        let (mut lowest, mut driest) = (f32::MAX, None);
        for &(cell, ..) in cells.values() {
            let heights = profile(&faithful, seed, cell);
            let f = factor(&heights, sea, PARAMS);
            let own = heights[TARGET].max(sea);
            if own <= sea {
                continue;
            }
            let upwind = heights[HALF..TARGET].iter().fold(sea, |m, &h| m.max(h));
            let entry = cells[&(cell.x, cell.z)];
            if entry.3 != entry.4 && f < 0.8 {
                eprintln!(
                    "moved by the shadow: factor {f}, upwind relief {}, {entry:?}",
                    upwind - own
                );
            }
            if upwind - own < 20.0 {
                flat.push(f);
            } else if upwind - own > 60.0 {
                ranged.push(f);
                if f < lowest {
                    lowest = f;
                    driest = Some(cells[&(cell.x, cell.z)]);
                }
            }
        }
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len().max(1) as f32;
        eprintln!(
            "seed 777: {} cells, {changed_types} of {columns} columns change climate type ({elapsed:?}); \
             land cells with flat ground upwind {} (mean factor {}), with a range upwind {} (mean {}); driest {driest:?}",
            cells.len(),
            flat.len(),
            mean(&flat),
            ranged.len(),
            mean(&ranged)
        );
        assert!(changed_types > 0);
        assert!(
            flat.len() > 10 && ranged.len() > 10,
            "{} {}",
            flat.len(),
            ranged.len()
        );
        assert!(
            mean(&ranged) + 0.05 < mean(&flat),
            "{} {}",
            mean(&ranged),
            mean(&flat)
        );
    }

    #[test]
    fn moisture_stays_in_range() {
        assert_eq!(moisture(0.8, 2.0), 1.0);
        assert_eq!(moisture(0.5, 0.0), 0.0);
        assert_eq!(moisture(0.5, 0.5), 0.25);
    }
}
