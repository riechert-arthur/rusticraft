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

/// The `world_presets` bundle with TerraForged's preset; other bundles pass through.
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
