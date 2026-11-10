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

use crate::worldgen::overrides::WorldgenOverrides;
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

/// `WorldgenOverrides::finish` (a HEAD hook): each of TerraForged's biomes the world's biome table holds (`known`) and
/// its packs' entries lack joins them, built by [`mod_biome_files`] as the table's copy is, so the table and the
/// generator always agree. The table holds them for TerraForged worlds only (the `native_biome_files` hook), so this
/// changes only a TerraForged world made before its pack held them (#226), or before it had a pack (#215): its GLOBAL
/// caves get `terraforged:cave` and its UNIQUE pool both biomes, as a new world's. A pack's own file is kept. Nothing
/// is written to the world (horizon-world has no log: the table's own build warns about a biome it leaves out).
pub fn join_table_biomes(overrides: &mut WorldgenOverrides, known: &dyn Fn(&str) -> bool) {
    join_biomes(overrides, known, || mod_biome_files().0);
}

/// [`join_table_biomes`] over the biomes `files` builds (called only when one is wanted); returns the ids it added.
fn join_biomes(
    overrides: &mut WorldgenOverrides,
    known: &dyn Fn(&str) -> bool,
    files: impl FnOnce() -> Vec<(String, Value)>,
) -> Vec<String> {
    let missing = |overrides: &WorldgenOverrides, id: &str| {
        known(id) && !overrides.entries("biomes").any(|(have, _)| have == id)
    };
    if !MOD_BIOMES.iter().any(|(id, _)| missing(overrides, id)) {
        return Vec::new();
    }
    let mut added = Vec::new();
    for (id, json) in files() {
        if missing(overrides, &id) {
            overrides.set("biomes", &id, json.to_string());
            added.push(id);
        }
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic parent: the copy keeps everything but the cave's generation settings.
    #[test]
    fn copies_keep_the_parent_but_the_caves_generation() {
        let parent = json!({
            "temperature": 0.8, "downfall": 0.4, "has_precipitation": true,
            "attributes": { "a": 1 }, "effects": { "water_color": "#3f76e4" },
            "carvers": ["minecraft:cave"],
            "features": [[], ["x:lake"], ["x:geode"], [], [], [], [], ["x:ore"], [], [], ["x:top"]],
        });
        assert_eq!(mod_biome("terraforged:oak_forest", parent.clone()), parent);
        let cave = mod_biome("terraforged:cave", parent.clone());
        let mut expected = parent;
        expected["carvers"] = json!([]);
        expected["features"] = json!([
            [],
            [],
            ["minecraft:large_dripstone"],
            [],
            [],
            [],
            [],
            ["minecraft:pointed_dripstone"],
            [],
            [],
            []
        ]);
        assert_eq!(cave, expected);
        // A parent with fewer steps still gets UNDERGROUND_DECORATION.
        let short = mod_biome("terraforged:cave", json!({ "features": [] }));
        assert_eq!(short["features"].as_array().unwrap().len(), 8);
    }

    /// The biomes the table holds and the packs lack join the packs' entries; a pack's own file wins, and a world
    /// whose table holds none of them (a vanilla world) builds nothing.
    #[test]
    fn the_tables_biomes_join_the_packs_that_lack_them() {
        let files = || {
            MOD_BIOMES
                .iter()
                .map(|(id, _)| (id.to_string(), json!({ "biome": id })))
                .collect()
        };
        // A world made before its pack held the biomes: its packs give only the dimension type.
        let mut older = WorldgenOverrides::default();
        older.set("dimension_types", "minecraft:overworld", "{}".into());
        let all = |_: &str| true;
        let added = join_biomes(&mut older, &all, files);
        assert_eq!(added, ["terraforged:cave", "terraforged:oak_forest"]);
        let biomes: Vec<_> = older.entries("biomes").collect();
        assert_eq!(
            biomes,
            [
                ("terraforged:cave", r#"{"biome":"terraforged:cave"}"#),
                (
                    "terraforged:oak_forest",
                    r#"{"biome":"terraforged:oak_forest"}"#
                ),
            ]
        );
        // Again: nothing more (the overrides are the same, so is the generator data they key).
        let before = older.clone();
        assert!(join_biomes(&mut older, &all, || unreachable!()).is_empty());
        assert_eq!(older, before);
        // A new world's pack holds them: kept as they are, nothing built.
        let mut fresh = WorldgenOverrides::default();
        fresh.set("biomes", "terraforged:cave", "pack cave".into());
        fresh.set("biomes", "terraforged:oak_forest", "pack oak".into());
        let before = fresh.clone();
        assert!(join_biomes(&mut fresh, &all, || unreachable!()).is_empty());
        assert_eq!(fresh, before);
        // A pack with one: the other joins.
        let mut half = WorldgenOverrides::default();
        half.set("biomes", "terraforged:cave", "pack cave".into());
        assert_eq!(
            join_biomes(&mut half, &all, files),
            ["terraforged:oak_forest"]
        );
        assert_eq!(half.entries("biomes").next().unwrap().1, "pack cave");
        // A vanilla world's table holds neither: nothing is built or added.
        let vanilla = |id: &str| !id.starts_with("terraforged:");
        let mut plain = WorldgenOverrides::default();
        assert!(join_biomes(&mut plain, &vanilla, || unreachable!()).is_empty());
        assert!(plain.is_empty());
        // A table without one parent's biome (it could not be built): only the other joins.
        let cave_only = |id: &str| id == "terraforged:cave";
        let mut some = WorldgenOverrides::default();
        assert_eq!(
            join_biomes(&mut some, &cave_only, files),
            ["terraforged:cave"]
        );
    }

    #[test]
    fn a_missing_parent_leaves_its_biome_out() {
        let (biomes, missing) = mod_biomes(|name| match name {
            "minecraft:plains" => Ok(json!({ "temperature": 0.8 })),
            "minecraft:dripstone_caves" => Ok(json!([])),
            _ => Err("absent".to_owned()),
        });
        let ids: Vec<&str> = biomes.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, ["terraforged:oak_forest"]);
        assert_eq!(missing.len(), 1);
        let (biomes, missing) = mod_biomes_in(Path::new("/nonexistent/terraforged-biomes"));
        assert!(biomes.is_empty());
        assert_eq!(missing.len(), 2);
    }
}
