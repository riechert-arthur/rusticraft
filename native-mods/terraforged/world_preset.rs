/*
 * MIT License
 *
 * Copyright (c) 2021 TerraForged
 * Copyright (c) 2020 TerraForged
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */

// TerraForged's `terraforged:normal` world preset, added to the generator's bundled presets.

pub const PRESET: &str = "terraforged:normal";
pub const GENERATOR_TYPE: &str = "terraforged:generator";

pub fn preset_json() -> &'static str {
    std::str::from_utf8(crate::terraforged_data!(
        "worldgen/world_preset/normal.json"
    ))
    .expect("TerraForged world preset JSON is UTF-8")
}

/// TerraForged's taller overworld: `data/minecraft/dimension_type/overworld.json` of TerraForged Reimagined's built-in
/// pack (a24d450, MIT; 6dd607eb's copy has the same `min_y`, `height` and `logical_height` in 1.18's format), bundled
/// unchanged. From y -64, 1024 blocks and logical height, so `getGenDepth()` (`levels.maxY`, 480) always fits.
pub fn overworld_type_json() -> &'static str {
    std::str::from_utf8(crate::terraforged_data!("dimension_type/overworld.json"))
        .expect("TerraForged dimension type JSON is UTF-8")
}

/// The `world_presets` bundle with TerraForged's preset; other bundles pass through. An
/// entry already there is kept (Horizon reads no data-pack world presets today).
pub fn with_preset(
    bundle: &str,
    mut entries: std::collections::BTreeMap<String, String>,
) -> std::collections::BTreeMap<String, String> {
    if bundle == "world_presets" {
        entries
            .entry(PRESET.to_owned())
            .or_insert_with(|| preset_json().to_owned());
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn only_the_world_presets_bundle_gains_the_preset() {
        let base = BTreeMap::from([("minecraft:normal".to_owned(), "{}".to_owned())]);
        let presets = with_preset("world_presets", base.clone());
        assert_eq!(presets.len(), 2);
        assert_eq!(presets[PRESET], preset_json());
        assert_eq!(presets["minecraft:normal"], "{}");
        assert_eq!(with_preset("dimensions", base.clone()), base);
        let kept = BTreeMap::from([(PRESET.to_owned(), "kept".to_owned())]);
        assert_eq!(with_preset("world_presets", kept)[PRESET], "kept");
        let preset: serde_json::Value = serde_json::from_str(preset_json()).unwrap();
        let generator = &preset["dimensions"]["minecraft:overworld"]["generator"];
        assert_eq!(generator["type"], GENERATOR_TYPE);
        assert_eq!(generator["levels"]["max_y"], 480);
        assert_eq!(
            preset["dimensions"]["minecraft:the_nether"]["generator"]["settings"],
            "minecraft:nether"
        );
    }

    #[test]
    fn the_taller_overworld_is_terraforgeds_1024_blocks_from_y_minus_64() {
        let json: serde_json::Value = serde_json::from_str(overworld_type_json()).unwrap();
        let height = crate::worldgen::overrides::LevelHeight::of_json(&json).unwrap();
        assert_eq!(
            height,
            crate::worldgen::overrides::LevelHeight {
                min_y: -64,
                height: 1024,
                logical_height: 1024,
                has_skylight: true,
            }
        );
        // The levels' top (480, exclusive) fits under the type's (960).
        let preset: serde_json::Value = serde_json::from_str(preset_json()).unwrap();
        let levels = &preset["dimensions"]["minecraft:overworld"]["generator"]["levels"];
        assert!(levels["max_y"].as_i64().unwrap() <= i64::from(height.min_y + height.height));
        assert_eq!(
            preset["dimensions"]["minecraft:overworld"]["type"],
            "minecraft:overworld"
        );
    }
}
