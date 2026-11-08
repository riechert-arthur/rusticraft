// The `altitude_climate` setting (Horizon's, in neither TerraForged tree): the climate cools with height.
// TerraForged picks a column's biome from 2D temperature and moisture noise alone, so a summit hundreds of blocks
// above the sea keeps a jungle's or a plain's climate; only vanilla's height-adjusted temperature
// (`Biome.getHeightAdjustedTemperature`: 0.05 / 40 = 0.00125 colder per block above sea level + 17, with a little
// noise) cools it, too slowly to snow on anything warmer than a taiga below y 300. With the setting, every block
// above that same start cools by `strength * EXTRA_LAPSE` more per block (the environmental lapse rate, about
// 6.5 degrees per kilometre, steepened to Minecraft's short mountains). The biomes stay where TerraForged puts
// them; what follows the colder air is
//
// - the snow line: after the decoration (vanilla's `freeze_top_layer` included), each column's top gets snow, and
//   open water ice, where the cooled temperature is under vanilla's 0.15, by vanilla's own rules
//   (`Biome.shouldSnow` and `shouldFreeze`), and TerraForged's `Surface.applyPost` then smooths that snow on gentle
//   ground and strips it to stone on steep slopes, as it does on every snowy mountain;
// - the treeline: a tree's fitness fades from 1 to 0 as the cooled temperature falls through a band
//   ([`treeline`]) just under the snow line in warm biomes and some way above the lowland in cold ones.
//
// The lapse counts blocks above the sea, not a share of the land's range, so `height_scale`'s taller land reaches
// further into the cold. Every value is a function of the biome and the block's own position, so chunks agree at
// their borders and with any thread or order.

use crate::worldgen::biome_temperature::{BiomeTemperatureSettings, TemperatureModifier};
use horizon_core::position::BlockPos;

/// Vanilla's cooling per block above sea level + [`START`] (`Biome.getHeightAdjustedTemperature`'s `0.05F / 40.0F`).
pub const VANILLA_LAPSE: f32 = 0.05 / 40.0;
/// How far above the sea the cooling starts, as vanilla's does.
pub const START: i32 = 17;
/// Horizon's cooling per block at strength 1, on top of vanilla's. With it the air cools by 0.004 per block, so
/// snow lies from about 140 blocks above the start in a forest (temperature 0.7), 160 in plains (0.8) and 200 in a
/// jungle (0.95); a desert (2.0) would need 460, more than TerraForged's land reaches at height scale 1.
pub const EXTRA_LAPSE: f32 = 0.002_75;
/// Vanilla's snow threshold (`Biome.warmEnoughToRain`: 0.15 and over rains).
pub const SNOW: f32 = 0.15;
/// The temperature at which trees stop in a warm biome: 0.1 over the snow line's, so the last trees stand about 25
/// blocks under it at strength 1.
pub const TREELINE: f32 = SNOW + 0.1;
/// How much colder than its own base temperature a cold biome's air may get before its trees stop: a taiga's or a
/// snowy plain's forests reach 75 blocks above the start at strength 1 (their lowland is as cold as a warm biome's
/// treeline, or colder, so the warm biomes' line would leave them bare).
pub const COLD_TREES: f32 = 0.3;
/// The temperature band over which the trees thin from all to none.
pub const TREELINE_BAND: f32 = 0.1;

/// The world's `altitude_climate`, when it has the setting on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AltitudeClimate {
    /// `altitude_climate_strength`: a multiple of [`EXTRA_LAPSE`].
    pub strength: f32,
}

impl AltitudeClimate {
    /// The setting of `settings`' world, `None` when it is off.
    pub fn of(settings: super::terraforged_settings::Settings) -> Option<Self> {
        settings.altitude_climate.then_some(Self {
            strength: settings.altitude_climate_strength,
        })
    }

    /// Horizon's cooling at height `y` (beyond vanilla's): `strength * EXTRA_LAPSE` per block above `sea_level +
    /// START`, none below.
    pub fn cooling(self, y: i32, sea_level: i32) -> f32 {
        let above = i64::from(y) - i64::from(sea_level) - i64::from(START);
        if above <= 0 {
            0.0
        } else {
            self.strength * EXTRA_LAPSE * above as f32
        }
    }

    /// A biome's climate settings as they stand at `pos`: its base temperature (after its `temperature_modifier`
    /// there) less [`cooling`](Self::cooling), so vanilla's own `height_adjusted` (and the snow and freeze tests
    /// that read it) give the cooled temperature there.
    pub fn at(
        self,
        biome: BiomeTemperatureSettings,
        pos: [i32; 3],
        sea_level: i32,
    ) -> BiomeTemperatureSettings {
        let at = BlockPos::new(pos[0], pos[1], pos[2]);
        BiomeTemperatureSettings {
            base_temperature: biome.modifier.modify(at, biome.base_temperature)
                - self.cooling(pos[1], sea_level),
            modifier: TemperatureModifier::None,
            ..biome
        }
    }

    /// The cooled temperature at `pos` in a biome with `biome`'s climate: vanilla's height-adjusted temperature less
    /// [`cooling`](Self::cooling).
    pub fn temperature(
        self,
        biome: BiomeTemperatureSettings,
        pos: [i32; 3],
        sea_level: i32,
    ) -> f32 {
        self.at(biome, pos, sea_level)
            .height_adjusted(BlockPos::new(pos[0], pos[1], pos[2]), sea_level)
    }

    /// How much of a tree's fitness a column keeps with its ground at `pos`: [`treeline`] of the biome's base
    /// temperature and the cooled one there.
    pub fn trees(self, biome: BiomeTemperatureSettings, pos: [i32; 3], sea_level: i32) -> f32 {
        treeline(
            biome.base_temperature,
            self.temperature(biome, pos, sea_level),
        )
    }
}

/// How much of a tree's fitness ground at `temperature` keeps in a biome whose own temperature is `base`: 1 down to
/// the treeline's band, 0 under it, linear across it. The line lies at [`TREELINE`] (just warmer than the snow
/// line), or [`COLD_TREES`] under `base` when that is colder, so cold biomes keep their lowland forests.
pub fn treeline(base: f32, temperature: f32) -> f32 {
    let line = TREELINE.min(base - COLD_TREES);
    let share = (temperature - line) / TREELINE_BAND;
    // NaN (a NaN base temperature) keeps the trees: `clamp` would pass it on as a fitness.
    if share.is_nan() {
        1.0
    } else {
        share.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn biome(base_temperature: f32) -> BiomeTemperatureSettings {
        BiomeTemperatureSettings {
            base_temperature,
            modifier: TemperatureModifier::None,
            has_precipitation: true,
            downfall: 0.5,
        }
    }

    const ON: AltitudeClimate = AltitudeClimate { strength: 1.0 };

    /// The cooling starts where vanilla's does and grows with the height and the strength; the temperature falls
    /// monotonically with height, faster than vanilla's alone, and strength 0 is vanilla's.
    #[test]
    fn the_air_cools_with_height() {
        let sea = 62;
        assert_eq!(ON.cooling(sea + START, sea), 0.0);
        assert_eq!(ON.cooling(-64, sea), 0.0);
        assert!((ON.cooling(sea + START + 100, sea) - 0.275).abs() < 1e-6);
        let double = AltitudeClimate { strength: 2.0 };
        assert!((double.cooling(sea + START + 100, sea) - 0.55).abs() < 1e-6);
        assert!(ON.cooling(i32::MAX, i32::MIN).is_finite());
        let plains = biome(0.8);
        let off = AltitudeClimate { strength: 0.0 };
        let mut previous = f32::INFINITY;
        for y in (-64..960).step_by(7) {
            let pos = [123, y, -456];
            let vanilla = plains.height_adjusted(BlockPos::new(123, y, -456), sea);
            let cooled = ON.temperature(plains, pos, sea);
            assert!(cooled <= previous, "{y}");
            previous = cooled;
            assert_eq!(off.temperature(plains, pos, sea), vanilla, "{y}");
            if y > sea + START {
                assert!(cooled < vanilla, "{y}");
            } else {
                assert_eq!(cooled, vanilla, "{y}");
            }
        }
    }

    /// Snow lies (under 0.15) from about 160 blocks above the start in plains and 200 in a jungle at strength 1,
    /// lower with a higher strength; a desert stays warm through the 1024-block overworld's land at strength 1.
    #[test]
    fn the_snow_line_follows_the_biome_and_the_strength() {
        let sea = 62;
        let snow_line = |climate: AltitudeClimate, base: f32| {
            (sea..960)
                .find(|&y| climate.temperature(biome(base), [0, y, 0], sea) < SNOW)
                .unwrap_or(960)
        };
        let (plains, jungle) = (snow_line(ON, 0.8), snow_line(ON, 0.95));
        assert!(
            (sea + START + 155..sea + START + 175).contains(&plains),
            "{plains}"
        );
        assert!(
            (sea + START + 190..sea + START + 215).contains(&jungle),
            "{jungle}"
        );
        assert!(snow_line(AltitudeClimate { strength: 2.0 }, 0.8) < plains - 60);
        assert!(snow_line(ON, 2.0) > sea + 476);
        // Vanilla alone snows on plains only 520 blocks above the start.
        assert!(snow_line(AltitudeClimate { strength: 0.0 }, 0.8) > sea + START + 500);
    }

    /// Trees fade over a band just under the snow line in warm biomes, and 0.3 colder than the biome's own
    /// temperature in cold ones; NaN keeps them.
    #[test]
    fn the_treeline_lies_under_the_snow_line() {
        assert_eq!(treeline(0.8, 0.8), 1.0);
        assert_eq!(treeline(0.8, TREELINE + TREELINE_BAND + 0.01), 1.0);
        assert!((treeline(0.8, TREELINE + 0.05) - 0.5).abs() < 1e-5);
        assert_eq!(treeline(0.8, TREELINE), 0.0);
        assert_eq!(treeline(0.8, SNOW), 0.0);
        // A taiga (0.25) and a snowy plain (0.0) keep their lowland forests.
        assert_eq!(treeline(0.25, 0.25), 1.0);
        assert_eq!(treeline(0.0, 0.0), 1.0);
        assert_eq!(treeline(0.25, 0.25 - COLD_TREES), 0.0);
        assert!(treeline(0.25, 0.0) > 0.0 && treeline(0.25, 0.0) < 1.0);
        assert_eq!(treeline(f32::NAN, 0.5), 1.0);
        assert_eq!(treeline(0.8, f32::NAN), 1.0);
        // In height: plains lose their last trees about 25 blocks under their snow line.
        let sea = 62;
        let plains = biome(0.8);
        let first = |test: &dyn Fn(i32) -> bool| (sea..960).find(|&y| test(y)).unwrap();
        let bare = first(&|y| ON.trees(plains, [0, y, 0], sea) == 0.0);
        let thinning = first(&|y| ON.trees(plains, [0, y, 0], sea) < 1.0);
        let snow = first(&|y| ON.temperature(plains, [0, y, 0], sea) < SNOW);
        assert!((15..40).contains(&(snow - bare)), "{bare} {snow}");
        assert!((15..40).contains(&(bare - thinning)), "{thinning} {bare}");
    }

    /// A frozen ocean's modifier is applied before the cooling, so the cooled settings need no modifier.
    #[test]
    fn the_modifier_comes_first() {
        let frozen = BiomeTemperatureSettings {
            modifier: TemperatureModifier::Frozen,
            ..biome(0.0)
        };
        for x in 0..64 {
            let pos = [x * 37, 200, x * -53];
            let at = ON.at(frozen, pos, 62);
            assert_eq!(at.modifier, TemperatureModifier::None);
            let modified = frozen
                .modifier
                .modify(BlockPos::new(pos[0], pos[1], pos[2]), 0.0);
            assert_eq!(at.base_temperature, modified - ON.cooling(200, 62));
        }
    }
}
