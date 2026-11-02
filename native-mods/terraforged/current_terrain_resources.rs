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
    TerrainAssetRegistryError, TerrainTypeAssets, decode_terrain_noise_assets,
    decode_terrain_type_assets,
};
use super::terraforged_terrain_blender::TerrainNoise;
use std::{collections::BTreeMap, sync::Arc};

pub const TERRAIN_TYPE_COUNT: usize = 18;
pub const TERRAIN_NOISE_COUNT: usize = 2;

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
    ]
}

pub fn decode_current_terrain_noises(
    types: &TerrainTypeAssets,
) -> Result<BTreeMap<String, Arc<TerrainNoise>>, TerrainAssetRegistryError> {
    decode_terrain_noise_assets(current_terrain_noise_documents(), types)
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

    // Sample points and raw f32 bits from tools/terraforged/TerraForgedTerrainNoiseOracle.java.
    const NOISE_POINTS: [(f32, f32); 8] = [
        (0.0, 0.0),
        (128.5, -63.25),
        (-4096.0, 8192.0),
        (1234.75, 987.5),
        (-30000.5, 15000.25),
        (7.125, -7.875),
        (65536.0, -65536.0),
        (301.0, 4022.5),
    ];

    #[allow(clippy::type_complexity)]
    const EXPECTED_NOISES: [(&str, &str, f32, [u32; 8]); TERRAIN_NOISE_COUNT] = [
        (
            "terraforged:plains",
            "flats",
            2.5,
            [
                1033843302, 1036612047, 1032918386, 1033591155, 1033703030, 1034165600, 1034728482,
                1032267292,
            ],
        ),
        (
            "terraforged:steppe",
            "flats",
            1.5,
            [
                1028606507, 1029134818, 1022905425, 1028641916, 1028182330, 1029685008, 1028097517,
                1028229816,
            ],
        ),
    ];

    #[test]
    fn bundled_terrain_noises_decode() {
        let mut registry = TerrainRegistry::new();
        let types = decode_current_terrain_types(&mut registry).unwrap();
        let noises = decode_current_terrain_noises(&types).unwrap();
        assert_eq!(noises.len(), TERRAIN_NOISE_COUNT);
        for (key, terrain, weight, bits) in EXPECTED_NOISES {
            let noise = &noises[key];
            assert_eq!(noise.terrain().name(), terrain, "{key}");
            assert_eq!(noise.weight().to_bits(), weight.to_bits(), "{key}");
            let values = NOISE_POINTS.map(|(x, y)| noise.noise().value(x, y).to_bits());
            assert_eq!(values, bits, "{key}");
        }
    }
}
