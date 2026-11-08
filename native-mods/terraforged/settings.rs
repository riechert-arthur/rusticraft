// Horizon's own TerraForged settings, beyond the original's. Every one is off by
// default, which is the faithful port. A world keeps the settings it was created with
// in its overworld stem (`generator.horizon_settings`), beside the generator's own
// fields (6dd607eb's `levels`; Reimagined keeps its `settings` there too). The game sets
// them here through `WorldGen::preset` (`terraforged_world_type::preset`) as it opens
// the world, before it builds the world's generators, which read them once.

use serde_json::{Map, Value};
use std::sync::RwLock;

/// The world's settings, as `generator.horizon_settings` holds them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// The trees depend much less on the order the chunks decorate in: each
    /// vegetation placement is seeded from its kind, biome, column or pass and
    /// feature index, on the ground under any trees, instead of from how many
    /// placed before it (the original's count, which a neighbour's tree changes).
    pub deterministic: bool,
    /// Start each chunk sample's river noise at 1, a fresh sample's value and what
    /// TerraForged's code intends, instead of the 0 TerraForged's reused samples settle
    /// at in play (the default, see `ErodedNoiseGenerator::generate`): tree fitness
    /// (`SaturationViability`, 1 - river noise) then falls away from rivers and lakes,
    /// and so do the forests.
    pub sparse_forests: bool,
    /// TerraForged Reimagined's biome fixes: keep cave, ocean, river and beach biomes
    /// (and stony shore) out of the climate pools (the original lets `deep_dark` and
    /// `deep_ocean` grow on dry land; its own overrides still place the water ones),
    /// and draw unique caves from the cave biomes, not from every registered biome.
    pub corrected_biomes: bool,
    /// How far TerraForged's land reaches above sea level (and its oceans below), as a
    /// multiple of the original's ([`HEIGHT_SCALES`]; 1 is the original). Landforms keep
    /// their width, so they get steeper as well as taller.
    pub height_scale: f32,
    /// Show what the erosion pass deposited: gravel and clay in river and lake beds,
    /// gravel on their banks, scree at the foot of slopes.
    pub erosion_deposits: bool,
}

impl Default for Settings {
    fn default() -> Self {
        FAITHFUL
    }
}

/// The original's behaviour: every setting off.
pub const FAITHFUL: Settings = Settings {
    deterministic: false,
    sparse_forests: false,
    corrected_biomes: false,
    height_scale: 1.0,
    erosion_deposits: false,
};

/// The range `height_scale` is held to. TerraForged's land reaches at most 476 blocks above
/// the sea at 1 (its base range, 128, plus 1.2 times its terrain range, 290), so at 1.85 even
/// that stays under the 1024-block overworld's top block (y 959: 62 + 1.85 x 476 = 943) and
/// nothing is cut flat; at 0.5 the base range (64) still clears the sea level, which
/// `TerrainLevels` holds it above.
pub const HEIGHT_SCALES: std::ops::RangeInclusive<f32> = 0.5..=1.85;

static CURRENT: RwLock<Settings> = RwLock::new(FAITHFUL);

thread_local! {
    /// `set_for_thread`: settings for one thread only, ahead of `CURRENT`.
    static LOCAL: std::cell::Cell<Option<Settings>> = const { std::cell::Cell::new(None) };
}

/// The settings of the world being generated.
pub fn current() -> Settings {
    LOCAL
        .get()
        .unwrap_or_else(|| *CURRENT.read().unwrap_or_else(|e| e.into_inner()))
}

/// Settings for the generators this thread builds, whatever the world's (`None`
/// goes back to them): for tests, which run side by side.
pub fn set_for_thread(settings: Option<Settings>) {
    LOCAL.set(settings);
}

/// Use `settings` for the generators built from now on.
pub fn set(settings: Settings) {
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = settings;
}

impl Settings {
    /// The JSON object's settings; what it leaves out, or holds in the wrong type,
    /// keeps its default.
    pub fn from_json(value: &Value) -> Self {
        let flag = |key: &str, default: bool| value[key].as_bool().unwrap_or(default);
        Self {
            deterministic: flag("deterministic", FAITHFUL.deterministic),
            sparse_forests: flag("sparse_forests", FAITHFUL.sparse_forests),
            corrected_biomes: flag("corrected_biomes", FAITHFUL.corrected_biomes),
            height_scale: value["height_scale"]
                .as_f64()
                .map_or(FAITHFUL.height_scale, |scale| {
                    (scale as f32).clamp(*HEIGHT_SCALES.start(), *HEIGHT_SCALES.end())
                }),
            erosion_deposits: flag("erosion_deposits", FAITHFUL.erosion_deposits),
        }
    }

    pub fn to_json(self) -> Value {
        let mut map = Map::new();
        map.insert("deterministic".into(), self.deterministic.into());
        map.insert("sparse_forests".into(), self.sparse_forests.into());
        map.insert("corrected_biomes".into(), self.corrected_biomes.into());
        map.insert("height_scale".into(), f64::from(self.height_scale).into());
        map.insert("erosion_deposits".into(), self.erosion_deposits.into());
        Value::Object(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trips_and_defaults_missing_keys() {
        let on = Settings {
            deterministic: true,
            sparse_forests: true,
            corrected_biomes: true,
            height_scale: 1.5,
            erosion_deposits: true,
        };
        assert_eq!(Settings::from_json(&on.to_json()), on);
        assert_eq!(Settings::from_json(&serde_json::json!({})), FAITHFUL);
        assert_eq!(
            Settings::from_json(&serde_json::json!({"height_scale": 9})).height_scale,
            1.85
        );
        assert_eq!(
            Settings::from_json(&serde_json::json!({"height_scale": -1})).height_scale,
            0.5
        );
        assert_eq!(
            Settings::from_json(&serde_json::json!({"height_scale": "2"})).height_scale,
            1.0
        );
        assert_eq!(
            Settings::from_json(&serde_json::json!({"deterministic": 3})),
            FAITHFUL
        );
    }
}
