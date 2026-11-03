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
    /// Keep cave, ocean, river and beach biomes out of the climate pools (the
    /// original lets `deep_dark` and `deep_ocean` grow on dry land; oceans, rivers
    /// and beaches still come from its own overrides), and draw unique caves from
    /// the overworld's cave biomes, not from every registered biome.
    pub corrected_biomes: bool,
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
};

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
        }
    }

    pub fn to_json(self) -> Value {
        let mut map = Map::new();
        map.insert("deterministic".into(), self.deterministic.into());
        map.insert("sparse_forests".into(), self.sparse_forests.into());
        map.insert("corrected_biomes".into(), self.corrected_biomes.into());
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
        };
        assert_eq!(Settings::from_json(&on.to_json()), on);
        assert_eq!(Settings::from_json(&serde_json::json!({})), FAITHFUL);
        assert_eq!(
            Settings::from_json(&serde_json::json!({"deterministic": 3})),
            FAITHFUL
        );
    }
}
