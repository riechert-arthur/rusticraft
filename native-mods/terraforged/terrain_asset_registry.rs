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

use super::terraforged_engine::{Terrain, TerrainRegistry};
use super::terraforged_terrain_asset::decode_terrain_asset;
use super::terraforged_terrain_blender::TerrainNoise;
use serde::Deserialize;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug)]
pub struct TerrainAssetRegistryError(String);

impl std::fmt::Display for TerrainAssetRegistryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for TerrainAssetRegistryError {}

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

    pub fn parent(&self) -> &Terrain {
        &self.parent
    }

    pub fn terrain(&self) -> &Terrain {
        &self.terrain
    }
}

#[derive(Default)]
pub struct TerrainTypeAssets {
    entries: BTreeMap<String, TerrainTypeAsset>,
}

impl TerrainTypeAssets {
    pub fn get(&self, key: &str) -> Option<&TerrainTypeAsset> {
        self.entries.get(key)
    }

    pub fn resolve(&self, key: &str) -> Option<Terrain> {
        self.get(key).map(|asset| asset.terrain.clone())
    }

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

#[derive(Deserialize)]
struct TerrainTypeDocument {
    name: String,
    parent: String,
}

pub fn decode_terrain_type_asset(
    json: &str,
    registry: &mut TerrainRegistry,
) -> Result<TerrainTypeAsset, TerrainAssetRegistryError> {
    let document: TerrainTypeDocument =
        serde_json::from_str(json).map_err(|error| TerrainAssetRegistryError(error.to_string()))?;
    let parent = registry
        .get_by_name(&document.parent)
        .unwrap_or_else(|| registry.get_by_id(0));
    let terrain = registry.get_by_name(&document.name).unwrap_or_else(|| {
        registry.register(Terrain::with_parent(
            0,
            document.name.clone(),
            parent.clone(),
        ))
    });
    Ok(TerrainTypeAsset {
        name: document.name,
        parent,
        terrain,
    })
}

pub fn decode_terrain_type_assets<K, J>(
    entries: impl IntoIterator<Item = (K, J)>,
    registry: &mut TerrainRegistry,
) -> Result<TerrainTypeAssets, TerrainAssetRegistryError>
where
    K: Into<String>,
    J: AsRef<str>,
{
    let mut assets = TerrainTypeAssets::default();
    for (key, json) in entries {
        let key = key.into();
        if assets.entries.contains_key(&key) {
            return Err(TerrainAssetRegistryError(format!(
                "duplicate terrain type asset {key:?}"
            )));
        }
        let asset = decode_terrain_type_asset(json.as_ref(), registry)
            .map_err(|error| TerrainAssetRegistryError(format!("terrain type {key:?}: {error}")))?;
        assets.entries.insert(key, asset);
    }
    Ok(assets)
}

pub fn decode_terrain_noise_assets<K, J>(
    entries: impl IntoIterator<Item = (K, J)>,
    types: &TerrainTypeAssets,
) -> Result<BTreeMap<String, Arc<TerrainNoise>>, TerrainAssetRegistryError>
where
    K: Into<String>,
    J: AsRef<str>,
{
    let mut assets = BTreeMap::new();
    for (key, json) in entries {
        let key = key.into();
        if assets.contains_key(&key) {
            return Err(TerrainAssetRegistryError(format!(
                "duplicate terrain noise asset {key:?}"
            )));
        }
        let terrain =
            decode_terrain_asset(json.as_ref(), |name| types.resolve(name)).map_err(|error| {
                TerrainAssetRegistryError(format!("terrain noise {key:?}: {error}"))
            })?;
        assets.insert(key, Arc::new(terrain));
    }
    Ok(assets)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            torridonian.terrain().category(),
            registry.get_by_name("hills").unwrap().category()
        );
        assert!(!torridonian.terrain().is_same(torridonian.parent()));

        let orphan = assets.get("terraforged:orphan").unwrap();
        assert_eq!(orphan.parent().name(), "none");
        assert_eq!(orphan.terrain().name(), "orphan");
        assert!(!orphan.terrain().is_same(orphan.parent()));
    }

    #[test]
    fn current_noise_assets_resolve_registry_keys_and_sort_entries() {
        let mut registry = TerrainRegistry::new();
        let types = decode_terrain_type_assets(TYPE_ASSETS, &mut registry).unwrap();
        let noises = decode_terrain_noise_assets(
            [
                ("terraforged:z", r#"{"type":"terraforged:torridonian","weight":2.5,"noise":{"type":"Const","value":0.75}}"#),
                ("terraforged:a", r#"{"type":"terraforged:flats","weight":1.5,"noise":{"type":"Const","value":0.25}}"#),
            ],
            &types,
        ).unwrap();
        assert_eq!(
            noises.keys().map(String::as_str).collect::<Vec<_>>(),
            ["terraforged:a", "terraforged:z"]
        );
        assert_eq!(noises["terraforged:a"].terrain().name(), "flats");
        assert_eq!(
            noises["terraforged:z"].weight().to_bits(),
            2.5_f32.to_bits()
        );
    }

    #[test]
    fn registry_errors_name_the_failed_asset() {
        let mut registry = TerrainRegistry::new();
        let duplicate = match decode_terrain_type_assets(
            [
                ("terraforged:flats", TYPE_ASSETS[0].1),
                ("terraforged:flats", TYPE_ASSETS[0].1),
            ],
            &mut registry,
        ) {
            Ok(_) => panic!("duplicate terrain type decoded"),
            Err(error) => error,
        };
        assert_eq!(
            duplicate.to_string(),
            "duplicate terrain type asset \"terraforged:flats\""
        );

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
    }
}
