/*
 * MIT License
 *
 * Copyright (c) 2021 TerraForged
 * Copyright (c) 2026 Romoslayer
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

// Adapted from TerraForged Reimagined (https://github.com/Romoslayer/TerraForged-Reimagined, a24d450): the asset
// `TerrainType` (`DIRECT` codec, `forName`) with its engine `TerrainType.get`/`getOrCreate`, and `RegistryKey.entries`/
// `entryIds`, loading the `terrain_type` and `terrain_noise` registries in Minecraft 26.3's registry order. MIT
// License, Copyright (c) 2020 TerraForged, Copyright (c) 2026 Romoslayer; see `LICENSE-TERRAFORGED-REIMAGINED`.

use super::terraforged_biome_registry::identifier_order;
use super::terraforged_engine::{Terrain, TerrainRegistry};
use super::terraforged_terrain_asset::{decode_terrain_asset, identifier};
use super::terraforged_terrain_blender::TerrainNoise;
use serde_json::value::RawValue;
use std::{collections::HashMap, sync::Arc};

#[derive(Debug)]
pub struct TerrainAssetRegistryError(String);

impl std::fmt::Display for TerrainAssetRegistryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for TerrainAssetRegistryError {}

fn error<T>(message: impl Into<String>) -> Result<T, TerrainAssetRegistryError> {
    Err(TerrainAssetRegistryError(message.into()))
}

/// One decoded `terrain_type` asset: Reimagined's asset `TerrainType` (`name`, `parentType`, `terrain`).
#[derive(Clone)]
pub struct TerrainTypeAsset {
    name: String,
    parent: Terrain,
    terrain: Terrain,
}

impl TerrainTypeAsset {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `getParentType`: the terrain the `parent` field named (`none` when it names none), even when `terrain`
    /// already existed and delegates elsewhere.
    pub fn parent(&self) -> &Terrain {
        &self.parent
    }

    pub fn terrain(&self) -> &Terrain {
        &self.terrain
    }
}

/// Registry keys as Minecraft 26.3 keys them: each key is read as an `Identifier` (`namespace:path`, no namespace
/// meaning `minecraft`); a key that is not one, or two keys naming the same identifier, is an error. Sorted by
/// `Identifier.compareTo` (path, then namespace), the order `RegistryDataLoader` registers entries in and
/// `RegistryKey.entries`/`entryIds` list them in. Key errors come first (an invalid key in input order, then the
/// first duplicate in sorted order), before any entry is decoded.
fn sorted_keys<K, J>(
    entries: impl IntoIterator<Item = (K, J)>,
    kind: &str,
) -> Result<Vec<(String, J)>, TerrainAssetRegistryError>
where
    K: AsRef<str>,
{
    let mut sorted = Vec::new();
    for (key, json) in entries {
        let key = key.as_ref();
        let Some(id) = identifier(key) else {
            return error(format!("invalid {kind} asset key {key:?}"));
        };
        sorted.push((id, json));
    }
    sorted.sort_by(|(left, _), (right, _)| identifier_order(left, right));
    if let Some(pair) = sorted.windows(2).find(|pair| pair[0].0 == pair[1].0) {
        return error(format!("duplicate {kind} asset {:?}", pair[0].0));
    }
    Ok(sorted)
}

/// `index` of `key` (read as an identifier) in entries sorted by [`sorted_keys`].
fn find<T>(entries: &[(String, T)], key: &str) -> Option<usize> {
    let id = identifier(key)?;
    entries
        .binary_search_by(|(entry, _)| identifier_order(entry, &id))
        .ok()
}

/// A loaded `terrain_type` registry, in registry order (see [`decode_terrain_type_assets`]).
#[derive(Default)]
pub struct TerrainTypeAssets {
    entries: Vec<(String, TerrainTypeAsset)>,
}

impl TerrainTypeAssets {
    /// The asset registered under `key` (an identifier; no namespace means `minecraft`).
    pub fn get(&self, key: &str) -> Option<&TerrainTypeAsset> {
        find(&self.entries, key).map(|index| &self.entries[index].1)
    }

    /// The terrain a `terrain_noise` asset's `type` reference resolves to.
    pub fn resolve(&self, key: &str) -> Option<Terrain> {
        self.get(key).map(|asset| asset.terrain.clone())
    }

    /// The entries with their full `namespace:path` keys, in registry order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &TerrainTypeAsset)> {
        self.entries
            .iter()
            .map(|(key, asset)| (key.as_str(), asset))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// One JSON object's fields; a duplicate key keeps its last value, as Gson's `JsonObject` does.
fn fields(json: &str) -> Result<HashMap<String, &RawValue>, TerrainAssetRegistryError> {
    serde_json::from_str(json).map_err(|error| TerrainAssetRegistryError(error.to_string()))
}

/// `Codec.STRING.fieldOf(name)`: a JSON string (`null`, a number or anything else is an error).
fn string_field(
    fields: &HashMap<String, &RawValue>,
    name: &str,
) -> Result<String, TerrainAssetRegistryError> {
    let Some(raw) = fields.get(name) else {
        return error(format!("missing field {name:?}"));
    };
    serde_json::from_str(raw.get())
        .map_err(|_| TerrainAssetRegistryError(format!("field {name:?} must be a string")))
}

/// The terrain registered under exactly `name`: Reimagined's engine `TerrainType` keys its registry by the exact
/// name (`String.equals`), where [`TerrainRegistry::get_by_name`] (Engine 0.3.0) ignores case.
fn exact(registry: &TerrainRegistry, name: &str) -> Option<Terrain> {
    registry.find(|terrain| terrain.name() == name)
}

/// One `terrain_type` asset (`data/<namespace>/worldgen/terrain_type/*.json`) as Reimagined's asset
/// `TerrainType.DIRECT` reads it from Minecraft's strict JSON, registering into `registry` as its constructor does:
///
/// - `name` and `parent` are JSON strings (`Codec.STRING`); other fields are ignored.
/// - `parent` goes through `forName` (`TerrainType.get`): the terrain registered under exactly that name, else
///   `none` (an unknown parent is not an error).
/// - `TerrainType.getOrCreate(name, parent)`: the terrain registered under exactly `name` (its own parent kept,
///   whatever `parent` says), else a new terrain delegating to `parent`.
///
/// Error: `name` differs only in case from a registered terrain. Reimagined registers it as a separate terrain, but
/// this registry (Engine 0.3.0's) compares names ignoring case and cannot hold both.
pub fn decode_terrain_type_asset(
    json: &str,
    registry: &mut TerrainRegistry,
) -> Result<TerrainTypeAsset, TerrainAssetRegistryError> {
    let fields = fields(json)?;
    let name = string_field(&fields, "name")?;
    let parent = string_field(&fields, "parent")?;
    let parent = exact(registry, &parent).unwrap_or_else(|| registry.get_by_id(0));
    let terrain = match exact(registry, &name) {
        Some(terrain) => terrain,
        None => {
            if let Some(other) = registry.get_by_name(&name) {
                return error(format!(
                    "terrain type {name:?} differs from the registered {:?} only in case",
                    other.name()
                ));
            }
            registry.register(Terrain::with_parent(0, name.as_str(), parent.clone()))
        }
    };
    Ok(TerrainTypeAsset {
        name,
        parent,
        terrain,
    })
}

/// The `terrain_type` registry: `entries` are (registry key, JSON) pairs in any order. They are decoded and
/// registered in registry order (see [`sorted_keys`]), so a parent must sort before the terrains naming it, and the
/// terrain ids this assigns do not depend on the input order. (26.3 decodes a registry's files in parallel, so in
/// Java a custom parent that is itself an asset resolves only if its file happened to decode first; Reimagined's own
/// assets name only built-in parents.) The first failing entry, in that order, is the error; the terrains decoded
/// before it stay registered (Java fails the whole load), so discard `registry` on an error.
pub fn decode_terrain_type_assets<K, J>(
    entries: impl IntoIterator<Item = (K, J)>,
    registry: &mut TerrainRegistry,
) -> Result<TerrainTypeAssets, TerrainAssetRegistryError>
where
    K: AsRef<str>,
    J: AsRef<str>,
{
    let mut assets = TerrainTypeAssets::default();
    for (key, json) in sorted_keys(entries, "terrain type")? {
        let asset = decode_terrain_type_asset(json.as_ref(), registry)
            .map_err(|error| TerrainAssetRegistryError(format!("terrain type {key:?}: {error}")))?;
        assets.entries.push((key, asset));
    }
    Ok(assets)
}

/// A loaded `terrain_noise` registry, in registry order: `RegistryKey.entries` (the noises, as
/// `GeneratorPreset.createUnerodedNoiseGenerator` hands them to the blender) and `entryIds`.
pub struct TerrainNoiseAssets {
    entries: Vec<(String, Arc<TerrainNoise>)>,
}

impl TerrainNoiseAssets {
    /// The noise registered under `key` (an identifier; no namespace means `minecraft`).
    pub fn get(&self, key: &str) -> Option<&Arc<TerrainNoise>> {
        find(&self.entries, key).map(|index| &self.entries[index].1)
    }

    /// The entries with their full `namespace:path` keys, in registry order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Arc<TerrainNoise>)> {
        self.entries
            .iter()
            .map(|(key, noise)| (key.as_str(), noise))
    }

    /// The full `namespace:path` keys, in registry order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(key, _)| key.as_str())
    }

    /// `RegistryKey.entryIds`: the keys in registry order, `terraforged:` ones by path alone (what
    /// `apply_terrain_overrides` takes, in the same order as [`Self::into_values`]).
    pub fn entry_ids(&self) -> Vec<String> {
        self.keys()
            .map(|key| match key.strip_prefix("terraforged:") {
                Some(path) => path.to_owned(),
                None => key.to_owned(),
            })
            .collect()
    }

    /// `RegistryKey.entries`: the noises in registry order.
    pub fn into_values(self) -> impl Iterator<Item = Arc<TerrainNoise>> {
        self.entries.into_iter().map(|(_, noise)| noise)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl std::ops::Index<&str> for TerrainNoiseAssets {
    type Output = Arc<TerrainNoise>;

    fn index(&self, key: &str) -> &Self::Output {
        self.get(key)
            .unwrap_or_else(|| panic!("no terrain noise asset {key:?}"))
    }
}

/// The `terrain_noise` registry: `entries` are (registry key, JSON) pairs in any order, each decoded with
/// [`decode_terrain_asset`], its `type` resolved in `types` (a key `types` lacks is an error, as 26.3's registry
/// reference is). The result is in registry order (see [`sorted_keys`]); the first failing entry in that order is
/// the error.
pub fn decode_terrain_noise_assets<K, J>(
    entries: impl IntoIterator<Item = (K, J)>,
    types: &TerrainTypeAssets,
) -> Result<TerrainNoiseAssets, TerrainAssetRegistryError>
where
    K: AsRef<str>,
    J: AsRef<str>,
{
    let mut assets = Vec::new();
    for (key, json) in sorted_keys(entries, "terrain noise")? {
        let terrain =
            decode_terrain_asset(json.as_ref(), |name| types.resolve(name)).map_err(|error| {
                TerrainAssetRegistryError(format!("terrain noise {key:?}: {error}"))
            })?;
        assets.push((key, Arc::new(terrain)));
    }
    Ok(TerrainNoiseAssets { entries: assets })
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_engine::TerrainCategory;
    use super::*;

    /// `.local/terraforged/oracles/terrain_asset_registry/` (`run.sh`): Reimagined a24d450's asset
    /// `TerrainType.DIRECT` and engine `TerrainType` registry, compiled from source against 26.3's server jar (strict
    /// JSON, DFU `JsonOps`, `Identifier`), over these cases (`T`: decoded in order into one registry; `O`: keys
    /// sorted by `Identifier.compareTo`, then `entryIds`).
    const ORACLE_CASES: &str = r##"# type cases share one registry, in order
T {"name":"flats","parent":"flats"}
T {"name":"hills","parent":"hills"}
T {"name":"torridonian","parent":"hills"}
T {"name":"orphan","parent":"missing"}
T {"name":"caps","parent":"HILLS"}
T {"name":"Hills","parent":"flats"}
T {"name":"FLATS","parent":"FLATS"}
T {"name":"torridonian","parent":"flats"}
T {"name":"child","parent":"torridonian"}
T {"name":"none","parent":"hills"}
T {"name":"","parent":""}
T {"name":"dup","name":"dup2","parent":"hills","parent":"flats"}
T {"name":"extra","parent":"hills","extra":[1,{"a":null}]}
T {"name":"lake","parent":"none"}
T {"name":"mountains","parent":"mountains"}
T {"name":"dolomites","parent":"mountains"}
T {"name":"orphan","parent":"hills"}
T {"name":5,"parent":"hills"}
T {"name":"x","parent":5}
T {"name":"x","parent":true}
T {"name":"x","parent":null}
T {"name":null,"parent":"hills"}
T {"name":["x"],"parent":"hills"}
T {"name":"x"}
T {"parent":"hills"}
T {}
T []
T "flats"
T 1
T {"name":"x","parent":"hills"} trailing
T {"name":"é","parent":"hills"}
# registry order: Identifier.compareTo (path, then namespace), entryIds strip terraforged:
O terraforged:mountains_2 terraforged:mountains terraforged:mountains_1 terraforged:hills
O minecraft:z terraforged:a other:a terraforged:z a:z
O terraforged:a/b terraforged:a_b terraforged:a.b terraforged:a-b terraforged:a terraforged:ab
O x:y terraforged:y minecraft:y y"##;

    /// The oracle's output (interpreted and JIT-compiled runs agree).
    const ORACLE_OUT: &str = r##"T 0 ok|flats|flats/category:FLATLAND|flats/category:FLATLAND|FLATLAND|true|false
T 1 ok|hills|hills/category:LOWLAND|hills/category:LOWLAND|LOWLAND|true|false
T 2 ok|torridonian|hills/category:LOWLAND|torridonian/terrain:hills|LOWLAND|false|false
T 3 ok|orphan|none/category:NONE|orphan/terrain:none|NONE|false|true
T 4 ok|caps|none/category:NONE|caps/terrain:none|NONE|false|true
T 5 ok|Hills|flats/category:FLATLAND|Hills/terrain:flats|FLATLAND|false|false
T 6 ok|FLATS|none/category:NONE|FLATS/terrain:none|NONE|false|true
T 7 ok|torridonian|flats/category:FLATLAND|torridonian/terrain:hills|LOWLAND|false|false
T 8 ok|child|torridonian/terrain:hills|child/terrain:torridonian|LOWLAND|false|false
T 9 ok|none|hills/category:LOWLAND|none/category:NONE|NONE|false|false
T 10 ok||none/category:NONE|/terrain:none|NONE|false|true
T 11 ok|dup2|flats/category:FLATLAND|dup2/terrain:flats|FLATLAND|false|false
T 12 ok|extra|hills/category:LOWLAND|extra/terrain:hills|LOWLAND|false|false
T 13 ok|lake|none/category:NONE|lake/category:LAKE|LAKE|false|true
T 14 ok|mountains|mountains/category:HIGHLAND|mountains/category:HIGHLAND|HIGHLAND|true|false
T 15 ok|dolomites|mountains/category:HIGHLAND|dolomites/terrain:mountains|HIGHLAND|false|false
T 16 ok|orphan|hills/category:LOWLAND|orphan/terrain:none|NONE|false|false
T 17 err
T 18 err
T 19 err
T 20 err
T 21 err
T 22 err
T 23 err
T 24 err
T 25 err
T 26 err
T 27 err
T 28 err
T 29 err
T 30 ok|é|hills/category:LOWLAND|é/terrain:hills|LOWLAND|false|false
O 31 terraforged:hills terraforged:mountains terraforged:mountains_1 terraforged:mountains_2 | hills mountains mountains_1 mountains_2
O 32 other:a terraforged:a a:z minecraft:z terraforged:z | other:a a a:z minecraft:z z
O 33 terraforged:a terraforged:a-b terraforged:a.b terraforged:a/b terraforged:a_b terraforged:ab | a a-b a.b a/b a_b ab
O 34 minecraft:y minecraft:y terraforged:y x:y | minecraft:y minecraft:y y x:y"##;

    /// The type cases Rust cannot represent: names differing only in case from a registered terrain (Reimagined
    /// registers "Hills" beside "hills"; this registry compares names ignoring case). They return an error.
    const CASE_VARIANTS: [&str; 2] = ["Hills", "FLATS"];

    fn oracle_cases() -> impl Iterator<Item = (usize, &'static str)> {
        ORACLE_CASES
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .enumerate()
    }

    fn oracle_line(index: usize) -> &'static str {
        ORACLE_OUT
            .lines()
            .find(|line| line.split(' ').nth(1) == Some(index.to_string().as_str()))
            .unwrap()
    }

    fn category_name(category: TerrainCategory) -> String {
        let mut name = String::new();
        for (index, c) in format!("{category:?}").chars().enumerate() {
            if c.is_ascii_uppercase() && index > 0 {
                name.push('_');
            }
            name.push(c.to_ascii_uppercase());
        }
        name
    }

    fn describe(terrain: &Terrain) -> String {
        match terrain.parent() {
            Some(parent) => format!("{}/terrain:{}", terrain.name(), parent.name()),
            None => format!(
                "{}/category:{}",
                terrain.name(),
                category_name(terrain.category())
            ),
        }
    }

    fn noise_json(kind: &str) -> String {
        format!(r#"{{"type":"{kind}","weight":1,"noise":{{"type":"Const","value":0}}}}"#)
    }

    #[test]
    fn type_assets_match_reimagined() {
        let mut registry = TerrainRegistry::new();
        let none = registry.get_by_id(0);
        let mut variants = 0;
        let mut checked = 0;
        for (index, line) in oracle_cases() {
            let Some(json) = line.strip_prefix("T ") else {
                continue;
            };
            let java = oracle_line(index);
            checked += 1;
            let rust = match decode_terrain_type_asset(json, &mut registry) {
                Ok(asset) => format!(
                    "ok|{}|{}|{}|{}|{}|{}",
                    asset.name(),
                    describe(asset.parent()),
                    describe(asset.terrain()),
                    category_name(asset.terrain().category()),
                    asset.terrain().is_same(asset.parent()),
                    asset.parent().is_same(&none),
                ),
                Err(error) if error.to_string().contains("only in case") => {
                    variants += 1;
                    let name = java.split('|').nth(1).unwrap();
                    assert!(CASE_VARIANTS.contains(&name), "{java}");
                    continue;
                }
                Err(_) => "err".to_owned(),
            };
            assert_eq!(format!("T {index} {rust}"), java, "{json}");
        }
        assert_eq!(variants, CASE_VARIANTS.len());
        assert_eq!(checked, 31);
    }

    #[test]
    fn registry_order_matches_identifier_order() {
        let mut registry = TerrainRegistry::new();
        let types = decode_terrain_type_assets(
            [("terraforged:flats", r#"{"name":"flats","parent":"flats"}"#)],
            &mut registry,
        )
        .unwrap();
        let json = noise_json("terraforged:flats");
        let mut checked = 0;
        for (index, line) in oracle_cases() {
            let Some(keys) = line.strip_prefix("O ") else {
                continue;
            };
            let java = oracle_line(index);
            checked += 1;
            let entries = keys.split(' ').map(|key| (key, json.as_str()));
            match decode_terrain_noise_assets(entries, &types) {
                Ok(noises) => {
                    let rust = format!(
                        "O {index} {} | {}",
                        noises.keys().collect::<Vec<_>>().join(" "),
                        noises.entry_ids().join(" ")
                    );
                    assert_eq!(rust, java);
                }
                // Java lists `y` and `minecraft:y` as one identifier twice; a registry holds it once.
                Err(error) => {
                    assert_eq!(
                        error.to_string(),
                        "duplicate terrain noise asset \"minecraft:y\""
                    );
                    assert!(java.contains("minecraft:y minecraft:y"));
                }
            }
        }
        assert_eq!(checked, 4);
    }

    const TYPE_ASSETS: [(&str, &str); 4] = [
        ("terraforged:flats", r#"{"name":"flats","parent":"flats"}"#),
        ("terraforged:hills", r#"{"name":"hills","parent":"hills"}"#),
        (
            "terraforged:torridonian",
            r#"{"name":"torridonian","parent":"hills"}"#,
        ),
        (
            "terraforged:orphan",
            r#"{"name":"orphan","parent":"missing"}"#,
        ),
    ];

    #[test]
    fn current_type_assets_keep_builtin_identity_and_create_delegates() {
        let mut registry = TerrainRegistry::new();
        let flats = registry.get_by_name("flats").unwrap();
        let assets = decode_terrain_type_assets(TYPE_ASSETS, &mut registry).unwrap();

        assert_eq!(assets.len(), 4);
        assert!(assets.resolve("terraforged:flats").unwrap().is_same(&flats));
        let torridonian = assets.get("terraforged:torridonian").unwrap();
        assert_eq!(torridonian.name(), "torridonian");
        assert_eq!(torridonian.parent().name(), "hills");
        assert!(
            torridonian
                .terrain()
                .parent()
                .unwrap()
                .is_same(torridonian.parent())
        );
        assert!(!torridonian.terrain().is_same(torridonian.parent()));

        let orphan = assets.get("terraforged:orphan").unwrap();
        assert_eq!(orphan.parent().name(), "none");
        assert_eq!(orphan.terrain().name(), "orphan");
        assert!(!orphan.terrain().is_same(orphan.parent()));
        assert_eq!(
            assets.iter().map(|(key, _)| key).collect::<Vec<_>>(),
            [
                "terraforged:flats",
                "terraforged:hills",
                "terraforged:orphan",
                "terraforged:torridonian"
            ]
        );
    }

    #[test]
    fn type_registration_does_not_depend_on_input_order() {
        let assets = [
            (
                "terraforged:z_custom",
                r#"{"name":"z_custom","parent":"hills"}"#,
            ),
            (
                "terraforged:a_child",
                r#"{"name":"a_child","parent":"z_custom"}"#,
            ),
            (
                "terraforged:b_child",
                r#"{"name":"b_child","parent":"a_custom"}"#,
            ),
            ("other:a_custom", r#"{"name":"a_custom","parent":"flats"}"#),
        ];
        let load = |entries: Vec<(&str, &str)>| {
            let mut registry = TerrainRegistry::new();
            let types = decode_terrain_type_assets(entries, &mut registry).unwrap();
            types
                .iter()
                .map(|(key, asset)| {
                    let terrain = asset.terrain();
                    (key.to_owned(), terrain.id(), describe(terrain))
                })
                .collect::<Vec<_>>()
        };
        let forward = load(assets.to_vec());
        let reversed = load(assets.iter().rev().copied().collect());
        assert_eq!(forward, reversed);
        // Registered in key order (path, then namespace): `a_child` before its parent `z_custom` (so its parent is
        // `none`), `a_custom` before `b_child`.
        let expected = [
            ("terraforged:a_child", 16, "a_child/terrain:none"),
            ("other:a_custom", 17, "a_custom/terrain:flats"),
            ("terraforged:b_child", 18, "b_child/terrain:a_custom"),
            ("terraforged:z_custom", 19, "z_custom/terrain:hills"),
        ]
        .map(|(key, id, terrain)| (key.to_owned(), id, terrain.to_owned()));
        assert_eq!(forward, expected);
    }

    #[test]
    fn current_noise_assets_resolve_registry_keys_in_registry_order() {
        let mut registry = TerrainRegistry::new();
        let types = decode_terrain_type_assets(TYPE_ASSETS, &mut registry).unwrap();
        let noises = decode_terrain_noise_assets(
            [
                ("terraforged:z", r#"{"type":"terraforged:torridonian","weight":2.5,"noise":{"type":"Const","value":0.75}}"#),
                ("terraforged:a", r#"{"type":"terraforged:flats","weight":1.5,"noise":{"type":"Const","value":0.25}}"#),
                ("other:b", r#"{"type":"terraforged:hills","weight":1,"noise":{"type":"Const","value":0.5}}"#),
            ],
            &types,
        )
        .unwrap();
        assert_eq!(
            noises.keys().collect::<Vec<_>>(),
            ["terraforged:a", "other:b", "terraforged:z"]
        );
        assert_eq!(noises.entry_ids(), ["a", "other:b", "z"]);
        assert_eq!(noises["terraforged:a"].terrain().name(), "flats");
        let torridonian = types.resolve("terraforged:torridonian").unwrap();
        assert!(noises["terraforged:z"].terrain().is_same(&torridonian));
        assert_eq!(
            noises["terraforged:z"].weight().to_bits(),
            2.5_f32.to_bits()
        );
        let names = noises
            .into_values()
            .map(|noise| noise.terrain().name().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(names, ["flats", "hills", "torridonian"]);
    }

    #[test]
    fn registry_keys_read_as_identifiers() {
        let mut registry = TerrainRegistry::new();
        let types = decode_terrain_type_assets(
            [("flats", r#"{"name":"flats","parent":"flats"}"#)],
            &mut registry,
        )
        .unwrap();
        assert_eq!(types.iter().next().unwrap().0, "minecraft:flats");
        assert!(types.get(":flats").is_some());
        assert!(types.get("terraforged:flats").is_none());
        let noises =
            decode_terrain_noise_assets([("plains", noise_json("flats"))], &types).unwrap();
        assert_eq!(noises.entry_ids(), ["minecraft:plains"]);

        let invalid = decode_terrain_type_assets(
            [("terraforged:Flats", r#"{"name":"flats","parent":"flats"}"#)],
            &mut registry,
        );
        assert_eq!(
            invalid.err().unwrap().to_string(),
            "invalid terrain type asset key \"terraforged:Flats\""
        );
    }

    #[test]
    fn registry_errors_name_the_failed_asset() {
        let mut registry = TerrainRegistry::new();
        let duplicate = match decode_terrain_type_assets(
            [
                ("minecraft:flats", TYPE_ASSETS[0].1),
                ("flats", TYPE_ASSETS[0].1),
            ],
            &mut registry,
        ) {
            Ok(_) => panic!("duplicate terrain type decoded"),
            Err(error) => error,
        };
        assert_eq!(
            duplicate.to_string(),
            "duplicate terrain type asset \"minecraft:flats\""
        );
        // A duplicate is reported before anything registers.
        assert_eq!(registry.terrains().len(), 16);

        let types = decode_terrain_type_assets(TYPE_ASSETS, &mut registry).unwrap();
        let missing = match decode_terrain_noise_assets(
            [(
                "terraforged:missing",
                r#"{"type":"terraforged:missing","weight":1,"noise":{"type":"Const","value":0}}"#,
            )],
            &types,
        ) {
            Ok(_) => panic!("unknown terrain type decoded"),
            Err(error) => error,
        };
        assert_eq!(
            missing.to_string(),
            "terrain noise \"terraforged:missing\": unknown terrain type \"terraforged:missing\""
        );

        let variant = decode_terrain_type_assets(
            [(
                "terraforged:hills_caps",
                r#"{"name":"HILLS","parent":"hills"}"#,
            )],
            &mut registry,
        );
        assert_eq!(
            variant.err().unwrap().to_string(),
            "terrain type \"terraforged:hills_caps\": terrain type \"HILLS\" differs from the registered \"hills\" only in case"
        );
    }
}
