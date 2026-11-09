// The `glacial_landforms` setting (Horizon's, in neither TerraForged tree): glaciers' valleys. TerraForged cuts every
// valley with one river profile (`RiverCarver`: from the bank level up to the terrain across the valley, linear or
// squared by the base noise), in the tropics as on frozen summits. A valley a glacier has filled is wider and has a
// different cross-section: a broad, flat floor and walls that steepen as they rise, the "U" (or parabolic) trough
// (Svensson 1959; Harbor 1992), whose cross-sections fit a power law `y = a·x^b` with `b` near 2 (Graf 1970). This
// first slice of #276 adds:
//
// - a glaciation field G, 0 to 1 ([`Glaciers::glaciation`]): the column's air temperature at its terrain height
//   (TerraForged's climate temperature taken at the column itself, on vanilla's scale, cooled with height as
//   `altitude_climate` cools it), with a low-frequency noise moving the glaciers' limit up and down by a few dozen
//   blocks. Ice starts a little warmer than vanilla's snow line (glaciers flow down past it) and is full a band
//   colder. Cold climates are glaciated down to the sea; temperate ones only where their mountains reach the cold;
// - U-shaped troughs ([`trough_height`]): where G > 0 the river valley widens by up to [`WIDEN`] and its cross-section
//   becomes the trough's (the bank level across a flat floor reaching [`FLOOR_SHARE`] of the way out, then rising as
//   the square of the distance to the terrain at the valley's widened edge); the column takes the lower of the river's profile and the trough's, blended by G, so the
//   river's bed stays and nothing rises.
//
// The trough is a function of the column's own position, its terrain sample and the nearest river node (which the
// carver finds per column), so chunks agree at every border, whatever the threads or the order. Off, nothing calls it
// and the world is bit-exact. Cirques, fjords and moraines are later slices.

use super::terraforged::simplex;
use super::terraforged_altitude_climate::{EXTRA_LAPSE, SNOW, START, VANILLA_LAPSE};
use super::terraforged_current_climate::CurrentClimateNoise;
use super::terraforged_noise_generator::NoiseGenerator;
use super::terraforged_settings::Settings;
use super::terraforged_terrain_levels::NoiseLevels;

/// TerraForged's climate temperature (0 to 0.98) onto vanilla's scale: `TEMPERATURE_SCALE * t + TEMPERATURE_OFFSET`.
/// It puts the warm edge of TerraForged's tundra (0.21) on vanilla's snow line (0.15) and its temperate middle (0.6)
/// on a forest's or plains' 0.7 to 0.8.
pub const TEMPERATURE_SCALE: f32 = 1.6;
pub const TEMPERATURE_OFFSET: f32 = -0.15;
/// The air temperature (vanilla's scale) at which glaciation starts: a little over the snow line, since glaciers flow
/// down below the line where their snow builds up.
pub const ICE_FROM: f32 = SNOW + 0.05;
/// The air temperature at which glaciation is full: 75 blocks above [`ICE_FROM`] at `altitude_climate`'s strength 1.
pub const ICE_FULL: f32 = SNOW - 0.25;
/// The scale of the noise that moves the glaciers' limit, in blocks.
pub const LIMIT_SCALE: f32 = 1200.0;
/// How far that noise moves the air temperature (about 25 blocks of height at strength 1).
pub const LIMIT_SHIFT: f32 = 0.1;
/// How much wider than the river's valley a fully glaciated trough is (0.35: a third wider).
pub const WIDEN: f32 = 0.35;
/// How far out, from the river's banks towards the trough's edge, a fully glaciated trough's flat floor reaches.
pub const FLOOR_SHARE: f32 = 0.4;

/// The world's glaciers: its climate's temperature and the cooling with height.
pub struct Glaciers {
    levels: NoiseLevels,
    /// Cooling per block above the sea level + [`START`], on vanilla's scale.
    lapse: f32,
    climate: CurrentClimateNoise,
    /// Subtracted from every air temperature: 0 in worlds; tests use it to find ice on small maps.
    chill: f32,
}

// World generation shares the generator across threads.
const _: fn() = || {
    fn shared<T: Send + Sync>() {}
    shared::<Glaciers>();
};

impl Glaciers {
    /// The glaciers of `noise`'s world with `settings`: the climate is the biome sampler's (built from the continent's
    /// context seed as `CurrentBiomeSampler::new` builds it, so the temperature is the one the biomes read), and the
    /// lapse is vanilla's plus `altitude_climate`'s at its strength (its strength 1 when the setting is off, so the
    /// glaciers do not need it, and follow its snow line when it is on).
    pub fn new(noise: &NoiseGenerator, settings: Settings) -> Self {
        let context = noise.continent().context();
        let mut seed = context.seed;
        let strength = if settings.altitude_climate {
            settings.altitude_climate_strength
        } else {
            1.0
        };
        Self {
            levels: noise.levels().noise_levels,
            lapse: VANILLA_LAPSE + strength * EXTRA_LAPSE,
            climate: CurrentClimateNoise::new(&mut seed, context.settings.climate),
            chill: 0.0,
        }
    }

    /// The same glaciers with every air temperature `chill` colder.
    pub fn with_chill(mut self, chill: f32) -> Self {
        self.chill = chill;
        self
    }

    /// The air temperature (vanilla's scale) over block `(x, z)` (noise coordinates `noise`) with its terrain at
    /// `height_noise`: the climate's temperature there, cooled by the lapse above the sea level + [`START`], and moved
    /// by the limit noise.
    pub fn air_temperature(
        &self,
        seed: i32,
        (x, z): (i32, i32),
        (noise_x, noise_z): (f32, f32),
        height_noise: f32,
    ) -> f32 {
        let climate = self.climate.local_temperature(seed, noise_x, noise_z);
        let above = (height_noise - self.levels.height_min) / self.levels.unit - START as f32;
        let cooling = self.lapse * above.max(0.0);
        let shift = LIMIT_SHIFT
            * simplex(
                x as f32 / LIMIT_SCALE,
                z as f32 / LIMIT_SCALE,
                seed.wrapping_add(4243),
            );
        TEMPERATURE_SCALE * climate + TEMPERATURE_OFFSET - cooling + shift - self.chill
    }

    /// The glaciation G at block `(x, z)` with its terrain at `height_noise`: 0 at [`ICE_FROM`] and warmer, 1 at
    /// [`ICE_FULL`] and colder, smooth between; 0 for NaN.
    pub fn glaciation(
        &self,
        seed: i32,
        block: (i32, i32),
        noise: (f32, f32),
        height_noise: f32,
    ) -> f32 {
        smoothstep(
            ICE_FROM,
            ICE_FULL,
            self.air_temperature(seed, block, noise, height_noise),
        )
    }
}

/// The trough's share of the way from its floor to its rim at `t` (0 on the floor's edge, 1 at the trough's edge):
/// Graf's power law with exponent 2, flat at the floor, steepest at the rim.
pub fn trough_profile(t: f32) -> f32 {
    let t = if t.is_nan() { 1.0 } else { t.clamp(0.0, 1.0) };
    t * t
}

/// The trough's height (or base noise) at `distance` from the river's line: `floor` out to `floor_radius`, then
/// [`trough_profile`] up to `rim` at `radius`, `rim` beyond. Shared by every river geometry that cuts troughs.
pub fn trough_height(floor: f32, rim: f32, distance: f32, floor_radius: f32, radius: f32) -> f32 {
    let t = if distance <= floor_radius {
        0.0
    } else if distance >= radius {
        1.0
    } else {
        (distance - floor_radius) / (radius - floor_radius)
    };
    floor + (rim - floor) * trough_profile(t)
}

/// The radius of a trough at glaciation `g` over a river valley of radius `valley`.
pub fn trough_radius(valley: f32, g: f32) -> f32 {
    valley * (1.0 + WIDEN * g)
}

/// The radius of a trough's flat floor at glaciation `g`, with the river's banks at `banks` and the trough's edge at
/// `radius`: [`FLOOR_SHARE`] of the way from one to the other at full glaciation, the banks without ice.
pub fn floor_radius(banks: f32, radius: f32, g: f32) -> f32 {
    banks + FLOOR_SHARE * g * (radius - banks)
}

/// 0 at `from`, 1 at `to` (either order), smooth in between; 0 for NaN.
fn smoothstep(from: f32, to: f32, value: f32) -> f32 {
    let t = (value - from) / (to - from);
    if t.is_nan() {
        return 0.0;
    }
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The profile meets the floor and the rim (continuous at both edges of the trough), rises monotonically, and is
    /// flatter than the river's linear profile near the floor and steeper near the rim.
    #[test]
    fn trough_profiles_are_continuous_and_monotone() {
        assert_eq!(trough_profile(0.0), 0.0);
        assert_eq!(trough_profile(1.0), 1.0);
        assert_eq!(trough_profile(-1.0), 0.0);
        assert_eq!(trough_profile(2.0), 1.0);
        let (floor, rim, inner, outer) = (0.2, 0.5, 0.1, 0.4);
        let mut last = f32::NEG_INFINITY;
        for i in 0..=1000 {
            let d = i as f32 * 0.0006;
            let h = trough_height(floor, rim, d, inner, outer);
            assert!(h >= last, "{d}");
            assert!((floor..=rim).contains(&h));
            last = h;
        }
        assert_eq!(trough_height(floor, rim, inner, inner, outer), floor);
        assert_eq!(trough_height(floor, rim, 0.0, inner, outer), floor);
        assert_eq!(trough_height(floor, rim, outer, inner, outer), rim);
        assert_eq!(trough_height(floor, rim, 0.9, inner, outer), rim);
        // Continuous: tiny steps either side of the edges move the height by tiny amounts.
        for edge in [inner, outer] {
            let (a, b) = (
                trough_height(floor, rim, edge - 1e-5, inner, outer),
                trough_height(floor, rim, edge + 1e-5, inner, outer),
            );
            assert!((a - b).abs() < 1e-3, "{edge}: {a} {b}");
        }
        // A U, not a V: a quarter of the way out it is lower than a straight wall, three quarters out steeper.
        assert!(trough_profile(0.25) < 0.25 && trough_profile(0.75) < 0.75);
        assert!(trough_profile(1.0) - trough_profile(0.9) > 0.1 * 1.5);
        // NaN distances leave the terrain alone.
        assert_eq!(trough_height(floor, rim, f32::NAN, inner, outer), rim);
        assert_eq!(trough_radius(0.3, 0.0), 0.3);
        assert_eq!(trough_radius(0.3, 1.0), 0.3 * (1.0 + WIDEN));
        assert_eq!(floor_radius(0.1, 0.5, 0.0), 0.1);
        assert!((floor_radius(0.1, 0.5, 1.0) - (0.1 + 0.4 * FLOOR_SHARE)).abs() < 1e-6);
    }

    /// Glaciation is 0 in the warm, 1 in the cold, monotone between, and 0 for NaN.
    #[test]
    fn glaciation_follows_the_air_temperature() {
        assert_eq!(smoothstep(ICE_FROM, ICE_FULL, 0.8), 0.0);
        assert_eq!(smoothstep(ICE_FROM, ICE_FULL, ICE_FROM), 0.0);
        assert_eq!(smoothstep(ICE_FROM, ICE_FULL, ICE_FULL), 1.0);
        assert_eq!(smoothstep(ICE_FROM, ICE_FULL, -1.0), 1.0);
        assert_eq!(smoothstep(ICE_FROM, ICE_FULL, f32::NAN), 0.0);
        let mut last = 0.0;
        for i in 0..=100 {
            let t = ICE_FROM - (ICE_FROM - ICE_FULL) * i as f32 / 100.0;
            let g = smoothstep(ICE_FROM, ICE_FULL, t);
            assert!(g >= last);
            last = g;
        }
        // The scale's anchors.
        assert!((TEMPERATURE_SCALE * 0.21 + TEMPERATURE_OFFSET - SNOW).abs() < 0.04);
        assert!((0.7..=0.85).contains(&(TEMPERATURE_SCALE * 0.6 + TEMPERATURE_OFFSET)));
    }
}
