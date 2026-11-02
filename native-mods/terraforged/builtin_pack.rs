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

// TerraForged's built-in data pack, present in every world while the mod is installed, as
// the original's `default` pack is: its overworld is 1024 blocks tall from y -64, with
// clouds at 300 (`terraforged:overworld` effects). Horizon reads a pack's dimension type
// for the build range, the level shapes and the synced registry; it is written once under
// the game's `config/terraforged` and appended to every world's packs.

use super::{DataPack, Source};
use std::path::PathBuf;

pub const PACK_ID: &str = "terraforged";
pub const OVERWORLD_MIN_Y: i32 = -64;
pub const OVERWORLD_HEIGHT: i32 = 1024;
pub const CLOUD_HEIGHT: f64 = 300.0;

/// The overworld dimension type the pack holds: the fields Horizon reads from a pack's type.
pub fn overworld_type_json() -> String {
    serde_json::json!({
        "min_y": OVERWORLD_MIN_Y,
        "height": OVERWORLD_HEIGHT,
        "logical_height": OVERWORLD_HEIGHT,
        "has_skylight": true,
        "attributes": { "minecraft:visual/cloud_height": CLOUD_HEIGHT },
    })
    .to_string()
}

fn pack_dir() -> PathBuf {
    crate::logging::game_home()
        .join("config")
        .join("terraforged")
        .join("builtin-pack")
}

/// The pack's files, written when missing or stale.
fn materialize() -> std::io::Result<PathBuf> {
    let dir = pack_dir();
    let types = dir.join("data").join("minecraft").join("dimension_type");
    let (major, _) = horizon_core::version::DATA_PACK_FORMAT;
    let meta = serde_json::json!({
        "pack": {
            "description": "TerraForged",
            "min_format": major,
            "max_format": major,
        }
    })
    .to_string();
    let overworld = overworld_type_json();
    let write = |path: PathBuf, text: &str| -> std::io::Result<()> {
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text) {
            std::fs::write(path, text)?;
        }
        Ok(())
    };
    std::fs::create_dir_all(&types)?;
    write(dir.join("pack.mcmeta"), &meta)?;
    write(types.join("overworld.json"), &overworld)?;
    Ok(dir)
}

/// `discover`'s packs and TerraForged's built-in one.
pub fn with_builtin(mut packs: Vec<DataPack>) -> Vec<DataPack> {
    if packs.iter().any(|pack| pack.id == PACK_ID) {
        return packs;
    }
    match materialize() {
        Ok(dir) => packs.push(DataPack {
            id: PACK_ID.to_owned(),
            source: Source::Dir(dir),
        }),
        Err(error) => log::warn!("TerraForged: built-in pack: {error}"),
    }
    packs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_world_gets_the_taller_overworld() {
        let home = std::env::temp_dir().join(format!("tf-builtin-pack-{}", std::process::id()));
        crate::logging::set_game_home(home.clone());
        let packs = with_builtin(Vec::new());
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].id, PACK_ID);
        assert_eq!(with_builtin(packs).len(), 1);
        let json = std::fs::read_to_string(
            pack_dir().join("data/minecraft/dimension_type/overworld.json"),
        )
        .unwrap();
        let height = horizon_world::worldgen::overrides::LevelHeight::of_json(
            &serde_json::from_str(&json).unwrap(),
        )
        .unwrap();
        assert_eq!(
            (height.min_y, height.height),
            (OVERWORLD_MIN_Y, OVERWORLD_HEIGHT)
        );
        std::fs::remove_dir_all(home).ok();
    }
}
