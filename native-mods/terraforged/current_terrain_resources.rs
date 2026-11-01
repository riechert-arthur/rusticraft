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
    std::str::from_utf8(bytes).expect("TerraForged terrain type JSON is UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_current_terrain_types_decode() {
        let mut registry = TerrainRegistry::new();
        let assets = decode_current_terrain_types(&mut registry).unwrap();
        assert_eq!(assets.len(), TERRAIN_TYPE_COUNT);
        assert_eq!(
            assets.get("terraforged:dolomites").unwrap().parent().name(),
            "mountains"
        );
        assert_eq!(
            assets
                .get("terraforged:torridonian")
                .unwrap()
                .parent()
                .name(),
            "hills"
        );
        assert_eq!(
            assets.get("terraforged:flats").unwrap().terrain().name(),
            "flats"
        );
    }
}
