/*
 * MIT License
 *
 * Copyright (c) 2020 TerraForged
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

use super::terraforged_engine::TerrainRegistry;
use super::terraforged_terrain_asset_registry::{
    TerrainAssetRegistryError, TerrainNoiseAssets, TerrainTypeAssets, decode_terrain_noise_assets,
    decode_terrain_type_assets,
};

/// The number of bundled terrain types.
pub const TERRAIN_TYPE_COUNT: usize = 18;
/// The number of bundled terrain noises.
pub const TERRAIN_NOISE_COUNT: usize = 5;

/// TerraForged Reimagined a24d450's `terrain_type` registry files
/// (<https://github.com/Romoslayer/TerraForged-Reimagined/tree/a24d4503064780790e975cd17495866ae13eef11>,
/// `src/main/resources/default/data/terraforged/worldgen/terrain_type/`, MIT, (c) 2020 TerraForged, (c) 2026
/// Romoslayer), bundled unchanged in `data/worldgen/terrain_type/`, as (registry key, JSON) pairs.
pub fn current_terrain_type_documents() -> [(&'static str, &'static str); TERRAIN_TYPE_COUNT] {
    [
        (
            "terraforged:badlands",
            text(crate::terraforged_data!(
                "worldgen/terrain_type/badlands.json"
            )),
        ),
        (
            "terraforged:beach",
            text(crate::terraforged_data!("worldgen/terrain_type/beach.json")),
        ),
        (
            "terraforged:coast",
            text(crate::terraforged_data!("worldgen/terrain_type/coast.json")),
        ),
        (
            "terraforged:deep_ocean",
            text(crate::terraforged_data!(
                "worldgen/terrain_type/deep_ocean.json"
            )),
        ),
        (
            "terraforged:dolomites",
            text(crate::terraforged_data!(
                "worldgen/terrain_type/dolomites.json"
            )),
        ),
        (
            "terraforged:flats",
            text(crate::terraforged_data!("worldgen/terrain_type/flats.json")),
        ),
        (
            "terraforged:hills",
            text(crate::terraforged_data!("worldgen/terrain_type/hills.json")),
        ),
        (
            "terraforged:lake",
            text(crate::terraforged_data!("worldgen/terrain_type/lake.json")),
        ),
        (
            "terraforged:mountain_chain",
            text(crate::terraforged_data!(
                "worldgen/terrain_type/mountain_chain.json"
            )),
        ),
        (
            "terraforged:mountains",
            text(crate::terraforged_data!(
                "worldgen/terrain_type/mountains.json"
            )),
        ),
        (
            "terraforged:none",
            text(crate::terraforged_data!("worldgen/terrain_type/none.json")),
        ),
        (
            "terraforged:ocean",
            text(crate::terraforged_data!("worldgen/terrain_type/ocean.json")),
        ),
        (
            "terraforged:plateau",
            text(crate::terraforged_data!(
                "worldgen/terrain_type/plateau.json"
            )),
        ),
        (
            "terraforged:river",
            text(crate::terraforged_data!("worldgen/terrain_type/river.json")),
        ),
        (
            "terraforged:torridonian",
            text(crate::terraforged_data!(
                "worldgen/terrain_type/torridonian.json"
            )),
        ),
        (
            "terraforged:volcano",
            text(crate::terraforged_data!(
                "worldgen/terrain_type/volcano.json"
            )),
        ),
        (
            "terraforged:volcano_pipe",
            text(crate::terraforged_data!(
                "worldgen/terrain_type/volcano_pipe.json"
            )),
        ),
        (
            "terraforged:wetland",
            text(crate::terraforged_data!(
                "worldgen/terrain_type/wetland.json"
            )),
        ),
    ]
}

/// Decodes the bundled terrain types into `registry`, which must be the world's own (terrains are compared by
/// identity) and still unshared: call it before the registry goes behind an `Arc`. Decoding again into the same
/// registry registers nothing new.
pub fn decode_current_terrain_types(
    registry: &mut TerrainRegistry,
) -> Result<TerrainTypeAssets, TerrainAssetRegistryError> {
    decode_terrain_type_assets(current_terrain_type_documents(), registry)
}

/// TerraForged Reimagined a24d450's flatland and hill `terrain_noise` registry files, `plains` and `steppe`, then
/// `dales`, `hills_1` and `hills_2`
/// (<https://github.com/Romoslayer/TerraForged-Reimagined/tree/a24d4503064780790e975cd17495866ae13eef11>,
/// `src/main/resources/default/data/terraforged/worldgen/terrain_noise/`, MIT, (c) 2020 TerraForged, (c) 2026
/// Romoslayer), bundled unchanged in `data/worldgen/terrain_noise/`, as (registry key, JSON) pairs.
pub fn current_terrain_noise_documents() -> [(&'static str, &'static str); TERRAIN_NOISE_COUNT] {
    [
        (
            "terraforged:plains",
            text(crate::terraforged_data!(
                "worldgen/terrain_noise/plains.json"
            )),
        ),
        (
            "terraforged:steppe",
            text(crate::terraforged_data!(
                "worldgen/terrain_noise/steppe.json"
            )),
        ),
        (
            "terraforged:dales",
            text(crate::terraforged_data!(
                "worldgen/terrain_noise/dales.json"
            )),
        ),
        (
            "terraforged:hills_1",
            text(crate::terraforged_data!(
                "worldgen/terrain_noise/hills_1.json"
            )),
        ),
        (
            "terraforged:hills_2",
            text(crate::terraforged_data!(
                "worldgen/terrain_noise/hills_2.json"
            )),
        ),
    ]
}

/// Decodes the bundled terrain noises, their `type`s resolved in `types` (from [`decode_current_terrain_types`]), in
/// registry order: hand `entry_ids()` and `into_values()` to `apply_terrain_overrides`.
pub fn decode_current_terrain_noises(
    types: &TerrainTypeAssets,
) -> Result<TerrainNoiseAssets, TerrainAssetRegistryError> {
    decode_terrain_noise_assets(current_terrain_noise_documents(), types)
}

fn text(bytes: &'static [u8]) -> &'static str {
    std::str::from_utf8(bytes).expect("TerraForged bundled JSON is UTF-8")
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_current_settings::CurrentTerrainSettings;
    use super::super::terraforged_engine::TerrainFormSettings;
    use super::super::terraforged_terrain_asset::tests::{bits, sample_hash};
    use super::super::terraforged_terrain_asset_registry::tests::{category_name, describe};
    use super::super::terraforged_terrain_overrides::apply_terrain_overrides;
    use super::*;
    use std::sync::Arc;

    /// `oracles/current_terrain_resources/` (`run.sh`): TerraForged Reimagined a24d450's asset `TerrainType.DIRECT`
    /// and engine `TerrainType` registry over its own 18 terrain_type files, in registry order (shuffled orders give
    /// the same lines). Fields: key, name, parent, terrain, category, terrain == parent, parent == none.
    const ORACLE_OUT: &str = r##"terraforged:badlands|badlands|badlands/category:LOWLAND|badlands/category:LOWLAND|LOWLAND|true|false
terraforged:beach|beach|beach/category:BEACH|beach/category:BEACH|BEACH|true|false
terraforged:coast|coast|coast/category:COAST|coast/category:COAST|COAST|true|false
terraforged:deep_ocean|deep_ocean|deep_ocean/category:DEEP_OCEAN|deep_ocean/category:DEEP_OCEAN|DEEP_OCEAN|true|false
terraforged:dolomites|dolomites|mountains/category:HIGHLAND|dolomites/terrain:mountains|HIGHLAND|false|false
terraforged:flats|flats|flats/category:FLATLAND|flats/category:FLATLAND|FLATLAND|true|false
terraforged:hills|hills|hills/category:LOWLAND|hills/category:LOWLAND|LOWLAND|true|false
terraforged:lake|lake|lake/category:LAKE|lake/category:LAKE|LAKE|true|false
terraforged:mountain_chain|mountain_chain|none/category:NONE|mountain_chain/terrain:none|NONE|false|true
terraforged:mountains|mountains|mountains/category:HIGHLAND|mountains/category:HIGHLAND|HIGHLAND|true|false
terraforged:none|none|none/category:NONE|none/category:NONE|NONE|true|true
terraforged:ocean|ocean|none/category:NONE|ocean/terrain:none|NONE|false|true
terraforged:plateau|plateau|plateau/category:HIGHLAND|plateau/category:HIGHLAND|HIGHLAND|true|false
terraforged:river|river|river/category:RIVER|river/category:RIVER|RIVER|true|false
terraforged:torridonian|torridonian|hills/category:LOWLAND|torridonian/terrain:hills|LOWLAND|false|false
terraforged:volcano|volcano|none/category:NONE|volcano/terrain:none|NONE|false|true
terraforged:volcano_pipe|volcano_pipe|none/category:NONE|volcano_pipe/terrain:none|NONE|false|true
terraforged:wetland|wetland|wetland/category:WETLAND|wetland/category:WETLAND|WETLAND|true|false"##;

    /// Where the port differs from Reimagined, and why: the registry is Engine 0.3.0's (#186's open issue), not
    /// Reimagined's rewritten engine. Engine registers `ocean`, `mountain_chain`, `volcano` and `volcano_pipe` as
    /// built-ins (Reimagined does not, so their files there create children of `none`), makes `badlands` a flatland
    /// (Reimagined: lowland) and `plateau` a lowland (Reimagined: highland). These are the port's lines today.
    const ENGINE_REGISTRY_LINES: [&str; 6] = [
        "terraforged:badlands|badlands|badlands/category:FLATLAND|badlands/category:FLATLAND|FLATLAND|true|false",
        "terraforged:mountain_chain|mountain_chain|mountain_chain/category:HIGHLAND|mountain_chain/category:HIGHLAND|HIGHLAND|true|false",
        "terraforged:ocean|ocean|ocean/category:SHALLOW_OCEAN|ocean/category:SHALLOW_OCEAN|SHALLOW_OCEAN|true|false",
        "terraforged:plateau|plateau|plateau/category:LOWLAND|plateau/category:LOWLAND|LOWLAND|true|false",
        "terraforged:volcano|volcano|volcano/category:HIGHLAND|volcano/category:HIGHLAND|HIGHLAND|true|false",
        "terraforged:volcano_pipe|volcano_pipe|volcano_pipe/category:HIGHLAND|volcano_pipe/category:HIGHLAND|HIGHLAND|true|false",
    ];

    #[test]
    fn bundled_current_terrain_types_match_reimagined() {
        let mut registry = TerrainRegistry::new();
        let none = registry.get_by_id(0);
        let assets = decode_current_terrain_types(&mut registry).unwrap();
        assert_eq!(assets.len(), TERRAIN_TYPE_COUNT);
        let rust: Vec<String> = assets
            .iter()
            .map(|(key, asset)| {
                format!(
                    "{key}|{}|{}|{}|{}|{}|{}",
                    asset.name(),
                    describe(asset.parent()),
                    describe(asset.terrain()),
                    category_name(asset.terrain().category()),
                    asset.terrain().is_same(asset.parent()),
                    asset.parent().is_same(&none),
                )
            })
            .collect();
        let java: Vec<&str> = ORACLE_OUT.lines().collect();
        assert_eq!(rust.len(), java.len());
        let mut differing = 0;
        for (rust, java) in rust.iter().zip(&java) {
            let key = java.split('|').next().unwrap();
            assert!(rust.starts_with(&format!("{key}|")), "{rust} vs {java}");
            if rust != java {
                differing += 1;
                assert!(
                    ENGINE_REGISTRY_LINES.contains(&rust.as_str()),
                    "{rust} vs {java}"
                );
            }
        }
        assert_eq!(differing, ENGINE_REGISTRY_LINES.len());
        // The terrains are the world registry's own: the built-ins themselves, and new children of them.
        let hills = registry.get_by_name("hills").unwrap();
        assert!(assets.resolve("terraforged:hills").unwrap().is_same(&hills));
        let torridonian = assets.resolve("terraforged:torridonian").unwrap();
        assert!(torridonian.parent().unwrap().is_same(&hills));
        assert!(
            registry
                .get_by_name("torridonian")
                .unwrap()
                .is_same(&torridonian)
        );
    }

    #[test]
    fn decoding_again_into_the_same_registry_reuses_its_terrains() {
        // Java's `getOrCreate` returns the registered terrain, so a second load (a world reopened with the same
        // registry) registers nothing new.
        let mut registry = TerrainRegistry::new();
        let first = decode_current_terrain_types(&mut registry).unwrap();
        let count = registry.terrains().len();
        assert_eq!(count, 18); // 16 built-ins, then torridonian and dolomites
        let second = decode_current_terrain_types(&mut registry).unwrap();
        assert_eq!(registry.terrains().len(), count);
        for ((key, a), (_, b)) in first.iter().zip(second.iter()) {
            assert!(a.terrain().is_same(b.terrain()), "{key}");
            assert!(a.parent().is_same(b.parent()), "{key}");
        }
    }

    #[test]
    fn bundled_documents_are_keyed_by_file_name() {
        for (key, json) in current_terrain_type_documents() {
            let name = key.strip_prefix("terraforged:").unwrap();
            assert!(json.contains(&format!(r#""name": "{name}""#)), "{key}");
        }
    }

    /// `oracles/current_terrain_resources/` (`run.sh`, noises.txt): Reimagined a24d450's `TerrainNoise` over its real
    /// `NoiseCodec`, its `type` bound in the terrain-type registry its 18 terrain_type files load, for each bundled
    /// terrain_noise file in registry order. Fields: key, terrain name/its delegate, whether it is the registry's own
    /// terrain of that name, weight bits, the raised noise's bounds and the #185 oracle's hash of seed-first samples.
    const NOISE_ORACLE: &str = r##"terraforged:dales|hills/category:LOWLAND|true|weight=3fc00000|min=00000000|max=3f800000|hash=2a75f650c0578dc8
terraforged:hills_1|hills/category:LOWLAND|true|weight=40000000|min=00000000|max=3f800000|hash=16d5050ff2aba2c1
terraforged:hills_2|hills/category:LOWLAND|true|weight=40000000|min=00000000|max=3f800000|hash=e013cf848d1b6c6e
terraforged:plains|flats/category:FLATLAND|true|weight=40200000|min=00000000|max=3f800000|hash=54dcba40e02603d8
terraforged:steppe|flats/category:FLATLAND|true|weight=3fc00000|min=00000000|max=3f800000|hash=93dd73538804253f"##;

    #[test]
    fn bundled_terrain_noises_match_reimagined() {
        let mut registry = TerrainRegistry::new();
        let types = decode_current_terrain_types(&mut registry).unwrap();
        let noises = decode_current_terrain_noises(&types).unwrap();
        assert_eq!(noises.len(), TERRAIN_NOISE_COUNT);
        let rust: Vec<String> = noises
            .iter()
            .map(|(key, noise)| {
                let terrain = noise.terrain();
                let own = registry
                    .get_by_name(terrain.name())
                    .unwrap()
                    .is_same(terrain);
                format!(
                    "{key}|{}|{own}|weight={:08x}|min={:08x}|max={:08x}|hash={:016x}",
                    describe(terrain),
                    bits(noise.weight()),
                    bits(noise.noise().min_value()),
                    bits(noise.noise().max_value()),
                    sample_hash(noise.noise()),
                )
            })
            .collect();
        assert_eq!(rust.join("\n"), NOISE_ORACLE);
    }

    #[test]
    fn bundled_noise_ids_reach_the_terrain_overrides() {
        // `GeneratorPreset` zips `RegistryKey.entries` with `entryIds`: overrides are keyed by the path alone.
        let mut registry = TerrainRegistry::new();
        let types = decode_current_terrain_types(&mut registry).unwrap();
        let noises = decode_current_terrain_noises(&types).unwrap();
        let ids = noises.entry_ids();
        assert_eq!(ids, ["dales", "hills_1", "hills_2", "plains", "steppe"]);
        let mut settings = CurrentTerrainSettings::default();
        settings
            .terrains
            .insert("steppe".to_owned(), TerrainFormSettings::new(4.0, 1.0, 1.0));
        settings.terrains.insert(
            "terraforged:plains".to_owned(),
            TerrainFormSettings::new(7.0, 1.0, 1.0),
        );
        let plains = Arc::clone(&noises["terraforged:plains"]);
        let values: Vec<_> = noises.into_values().collect();
        let applied = apply_terrain_overrides(&values, &ids, &settings);
        assert_eq!(applied.len(), TERRAIN_NOISE_COUNT);
        assert!(Arc::ptr_eq(&applied[3], &plains)); // a full-id key matches no terraforged: entry
        assert_eq!(applied[4].weight().to_bits(), 4.0_f32.to_bits());
        assert_eq!(applied[4].terrain().name(), "flats");
    }
}
