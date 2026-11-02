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

// TerraForged's own biomes (6dd607eb `ModBiomes`): copies of vanilla biomes, built at runtime from the player's own
// 26.3 data as `ModBiome.create` builds them, so no vanilla biome file is bundled.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// TerraForged's biomes and the vanilla biome each copies: `cave` copies dripstone caves (with its own generation
/// settings), `oak_forest` copies plains unchanged (6dd607eb comments out its forest category).
pub const MOD_BIOMES: [(&str, &str); 2] = [
    ("terraforged:cave", "minecraft:dripstone_caves"),
    ("terraforged:oak_forest", "minecraft:plains"),
];

/// `GenerationStep.Decoration.LOCAL_MODIFICATIONS` and `UNDERGROUND_DECORATION`.
const LOCAL_MODIFICATIONS: usize = 2;
const UNDERGROUND_DECORATION: usize = 7;

/// The resources folder the game reads vanilla's biome files from (`<root>/data/minecraft/worldgen/biome`), as
/// `Biomes::load` last saw it.
static ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Remembers the resources folder `Biomes::load` reads (a HEAD hook): the parents are read from there. A folder
/// without them (tests load small biome folders of their own) is not kept.
pub fn remember_root(root: &Path) {
    if MOD_BIOMES
        .iter()
        .all(|(_, parent)| parent_file(root, parent).is_file())
    {
        *ROOT.write().unwrap_or_else(|e| e.into_inner()) = Some(root.to_path_buf());
    }
}

/// The vanilla biome file of `name` under `root`: `data/<ns>/worldgen/biome/<path>.json`.
fn parent_file(root: &Path, name: &str) -> PathBuf {
    let (namespace, path) = name.split_once(':').unwrap_or(("minecraft", name));
    root.join("data")
        .join(namespace)
        .join("worldgen")
        .join("biome")
        .join(format!("{path}.json"))
}

/// `ModBiome.create(parent, modifier)`: the parent's climate, effects, attributes (spawns) and generation settings,
/// then `ModBiomes`' modifier. Only `cave` replaces anything: its generation settings are a new builder's with large
/// dripstone and pointed dripstone, so no carvers and no other features. Steps past the builder's last one are empty
/// either way; the parent's step count is kept.
pub fn mod_biome(id: &str, mut parent: Value) -> Value {
    if id == "terraforged:cave" {
        let steps = parent["features"]
            .as_array()
            .map_or(0, Vec::len)
            .max(UNDERGROUND_DECORATION + 1);
        let mut features = vec![json!([]); steps];
        features[LOCAL_MODIFICATIONS] = json!(["minecraft:large_dripstone"]);
        features[UNDERGROUND_DECORATION] = json!(["minecraft:pointed_dripstone"]);
        parent["features"] = Value::Array(features);
        parent["carvers"] = json!([]);
    }
    parent
}

/// [`MOD_BIOMES`] built from the parents `parent` reads, as `(id, JSON)`, and why any is missing (a parent that
/// can't be read or isn't a JSON object): that biome is left out.
pub fn mod_biomes(
    mut parent: impl FnMut(&str) -> Result<Value, String>,
) -> (Vec<(String, Value)>, Vec<String>) {
    let (mut biomes, mut missing) = (Vec::new(), Vec::new());
    for (id, name) in MOD_BIOMES {
        match parent(name) {
            Ok(json) if json.is_object() => biomes.push((id.to_owned(), mod_biome(id, json))),
            Ok(_) => missing.push(format!("{id}: {name} is not a biome")),
            Err(error) => missing.push(format!("{id}: {name}: {error}")),
        }
    }
    (biomes, missing)
}

/// [`mod_biomes`] from the vanilla biome files under `root` (`data/<ns>/worldgen/biome/<path>.json`).
pub fn mod_biomes_in(root: &Path) -> (Vec<(String, Value)>, Vec<String>) {
    mod_biomes(|name| {
        let file = parent_file(root, name);
        let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", file.display()))
    })
}

/// [`mod_biomes_in`] the folder [`remember_root`] kept; with none yet, no biome and one reason.
pub fn mod_biome_files() -> (Vec<(String, Value)>, Vec<String>) {
    let root = ROOT.read().unwrap_or_else(|e| e.into_inner()).clone();
    match root {
        Some(root) => mod_biomes_in(&root),
        None => (
            Vec::new(),
            vec!["the game's vanilla biomes are not loaded".to_owned()],
        ),
    }
}
