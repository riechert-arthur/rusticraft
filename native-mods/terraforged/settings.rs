// Horizon's own TerraForged settings, beyond the original's. Every one is off by
// default, which is the faithful port. A world keeps the settings it was created with
// in its overworld stem (`generator.horizon_settings`), the way the original keeps
// its own settings in the generator; the game sets them here when it opens the world,
// before it builds the generator.

use serde_json::{Map, Value};
use std::sync::RwLock;

/// The world's settings, as `generator.horizon_settings` holds them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// The same seed makes the same world: each chunk's river values start fresh
    /// instead of carrying over from the chunk its worker sampled before (the
    /// original reuses the samples), and each vegetation placement is seeded from its
    /// position and feature index, on the ground under leaves, instead of from how
    /// many placed before it.
    pub deterministic: bool,
    /// Start each sample's river noise at 1, as TerraForged's code intends, instead
    /// of what the chunk before left (`deterministic`'s settled 0 when that is on):
    /// trees then thin out away from rivers (`SaturationViability`).
    pub sparse_forests: bool,
    /// Keep cave, ocean, river and beach biomes out of the climate pools (the
    /// original lets `deep_dark` and `deep_ocean` grow on dry land; oceans, rivers
    /// and beaches still come from its own overrides), and draw unique caves from
    /// the overworld's cave biomes, not from every registered biome.
    pub corrected_biomes: bool,
    /// How far TerraForged's land reaches above sea level (and its oceans below), as a
    /// multiple of the original's: up to 2 fills most of the 1024-block overworld.
    /// Landforms keep their width, so they get steeper as well as taller.
    pub height_scale: f32,
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
};

/// The range `height_scale` is held to: the world's top is 959.
pub const HEIGHT_SCALES: std::ops::RangeInclusive<f32> = 0.5..=2.0;

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
        }
    }

    pub fn to_json(self) -> Value {
        let mut map = Map::new();
        map.insert("deterministic".into(), self.deterministic.into());
        map.insert("sparse_forests".into(), self.sparse_forests.into());
        map.insert("corrected_biomes".into(), self.corrected_biomes.into());
        map.insert("height_scale".into(), f64::from(self.height_scale).into());
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
        };
        assert_eq!(Settings::from_json(&on.to_json()), on);
        assert_eq!(Settings::from_json(&serde_json::json!({})), FAITHFUL);
        assert_eq!(
            Settings::from_json(&serde_json::json!({"height_scale": 9})).height_scale,
            2.0
        );
        assert_eq!(
            Settings::from_json(&serde_json::json!({"deterministic": 3})),
            FAITHFUL
        );
    }
}
