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
    TerrainAssetRegistryError, TerrainTypeAssets, decode_terrain_type_assets,
};

pub const TERRAIN_TYPE_COUNT: usize = 18;

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

pub fn decode_current_terrain_types(
    registry: &mut TerrainRegistry,
) -> Result<TerrainTypeAssets, TerrainAssetRegistryError> {
    decode_terrain_type_assets(current_terrain_type_documents(), registry)
}

fn text(bytes: &'static [u8]) -> &'static str {
    std::str::from_utf8(bytes).expect("TerraForged bundled JSON is UTF-8")
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_terrain_asset_registry::tests::{category_name, describe};
    use super::*;

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
    fn bundled_documents_are_keyed_by_file_name() {
        for (key, json) in current_terrain_type_documents() {
            let name = key.strip_prefix("terraforged:").unwrap();
            assert!(json.contains(&format!(r#""name": "{name}""#)), "{key}");
        }
    }
}
