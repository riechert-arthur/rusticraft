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

// TerraForged as a world type: the Create World cycle, and the overworld stem a
// TerraForged world's `world_gen_settings` keeps (`{"type": "terraforged:generator",
// "levels": ...}`, as its world preset's). The screen's preset enum is vanilla's, so the
// choice is held here beside it, as `Normal` plus this flag.

use super::{Result, WorldGen, WorldPreset, read_saved_data, world_gen_settings, write_saved_data};
use horizon_world::nbt::Tag;
use horizon_world::worldgen::generator::terraforged_settings::{self, Settings};
use horizon_world::worldgen::generator::terraforged_world_preset::{
    GENERATOR_TYPE, PRESET, preset_json,
};
use std::cell::Cell;
use std::ops::ControlFlow;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// The language key of the world type, and its text in TerraForged's `en_us.json`.
pub const LABEL: &str = "TerraForged";

static CHOSEN: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Set by Re-Create for the `WorldOptions::default` its body ends with.
    static RECREATING: Cell<Option<bool>> = const { Cell::new(None) };
    /// `write_world_gen_settings_with` writing the vanilla settings it then amends.
    static WRITING: Cell<bool> = const { Cell::new(false) };
}

pub fn chosen() -> bool {
    CHOSEN.load(Ordering::Relaxed)
}

/// A fresh screen starts at Normal; Re-Create keeps the world's own type.
pub fn reset() {
    CHOSEN.store(RECREATING.take().unwrap_or(false), Ordering::Relaxed);
}

/// Re-Create: the world's type, applied when its options' defaults are made.
pub fn recreate(world: &Path) {
    RECREATING.set(Some(is_terraforged(world)));
}

/// The world-type button: Single Biome, then TerraForged, then Normal.
pub fn press(world_type: &mut WorldPreset, id: u8) -> ControlFlow<()> {
    if id != 25 {
        return ControlFlow::Continue(());
    }
    if chosen() {
        CHOSEN.store(false, Ordering::Relaxed);
    } else if *world_type == WorldPreset::SingleBiomeSurface {
        CHOSEN.store(true, Ordering::Relaxed);
    } else {
        return ControlFlow::Continue(());
    }
    *world_type = WorldPreset::Normal;
    ControlFlow::Break(())
}

/// A saved world's overworld generator, when it is TerraForged's.
fn terraforged_generator(world: &Path) -> Option<Tag> {
    let generator = world_gen_settings(world)?
        .get("dimensions")?
        .get("minecraft:overworld")?
        .get("generator")?
        .clone();
    let kind = generator.get("type")?;
    matches!(kind, Tag::String(text) if text.to_string_lossy() == GENERATOR_TYPE)
        .then_some(generator)
}

/// Whether a saved world's overworld is TerraForged's.
pub fn is_terraforged(world: &Path) -> bool {
    terraforged_generator(world).is_some()
}

/// `WorldGen::preset`: TerraForged worlds generate with TerraForged's preset, and
/// with the Horizon settings the world was created with.
pub fn preset(world: &Path) -> ControlFlow<&'static str> {
    match terraforged_generator(world) {
        Some(generator) => {
            let saved = generator.get("horizon_settings").map(to_json);
            terraforged_settings::set(
                saved.map_or_else(Settings::default, |json| Settings::from_json(&json)),
            );
            ControlFlow::Break(PRESET)
        }
        None => ControlFlow::Continue(()),
    }
}

/// The settings a new TerraForged world starts with: `config/terraforged/settings.json`,
/// written with every setting off the first time.
pub fn new_world_settings() -> Settings {
    let path = crate::logging::game_home()
        .join("config")
        .join("terraforged")
        .join("settings.json");
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(|json| Settings::from_json(&json))
            .unwrap_or_default(),
        Err(_) => {
            let defaults = Settings::default();
            if let Some(dir) = path.parent()
                && std::fs::create_dir_all(dir).is_ok()
            {
                let text = serde_json::to_string_pretty(&defaults.to_json()).unwrap_or_default();
                let _ = std::fs::write(&path, text + "\n");
            }
            defaults
        }
    }
}

/// `write_world_gen_settings_with` for a TerraForged world: vanilla's settings with
/// TerraForged's overworld stem.
pub fn write(
    dir: &Path,
    seed: i64,
    options: WorldGen,
    flat: &Option<Tag>,
) -> ControlFlow<Result<()>> {
    if !chosen() || WRITING.get() {
        return ControlFlow::Continue(());
    }
    WRITING.set(true);
    let written = super::write_world_gen_settings_with(dir, seed, options, flat.clone());
    WRITING.set(false);
    ControlFlow::Break(written.and_then(|()| {
        let mut settings = read_saved_data(dir, "world_gen_settings")
            .ok_or_else(|| std::io::Error::other("world_gen_settings was not written"))?;
        let mut stem = overworld_stem();
        if let Some(Tag::Compound(generator)) = stem.get_mut("generator") {
            generator.insert(
                "horizon_settings".into(),
                to_tag(&new_world_settings().to_json()),
            );
        }
        if let Some(Tag::Compound(dimensions)) = settings.get_mut("dimensions") {
            dimensions.insert("minecraft:overworld".into(), stem);
        }
        write_saved_data(dir, "world_gen_settings", settings)
    }))
}

/// The overworld stem of TerraForged's world preset, as NBT.
pub fn overworld_stem() -> Tag {
    let preset: serde_json::Value =
        serde_json::from_str(preset_json()).expect("TerraForged world preset JSON");
    to_tag(&preset["dimensions"]["minecraft:overworld"])
}

/// JSON to NBT as `JsonOps` to `NbtOps` converts: booleans are bytes, whole numbers ints,
/// other numbers floats.
fn to_tag(value: &serde_json::Value) -> Tag {
    match value {
        serde_json::Value::Bool(value) => Tag::Byte(i8::from(*value)),
        serde_json::Value::Number(number) => {
            match number.as_i64().and_then(|n| i32::try_from(n).ok()) {
                Some(int) => Tag::Int(int),
                None => Tag::Float((number.as_f64().unwrap_or(0.0) as f32).to_bits()),
            }
        }
        serde_json::Value::String(text) => Tag::String(text.as_str().into()),
        serde_json::Value::Array(values) => Tag::List(values.iter().map(to_tag).collect()),
        serde_json::Value::Object(entries) => Tag::compound(
            entries
                .iter()
                .map(|(key, value)| (key.as_str(), to_tag(value))),
        ),
        serde_json::Value::Null => Tag::End,
    }
}

/// NBT to JSON as `NbtOps` to `JsonOps` converts, for the settings: bytes are booleans.
fn to_json(tag: &Tag) -> serde_json::Value {
    match tag {
        Tag::Byte(value) => serde_json::Value::Bool(*value != 0),
        Tag::Int(value) => (*value).into(),
        Tag::Float(bits) => f64::from(f32::from_bits(*bits)).into(),
        Tag::String(text) => text.to_string_lossy().into(),
        Tag::List(values) => values.iter().map(to_json).collect(),
        Tag::Compound(entries) => entries
            .iter()
            .map(|(key, value)| (key.to_string_lossy().to_string(), to_json(value)))
            .collect::<serde_json::Map<_, _>>()
            .into(),
        _ => serde_json::Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_world_type_cycles_and_persists() {
        CHOSEN.store(false, Ordering::Relaxed);
        let mut world_type = WorldPreset::SingleBiomeSurface;
        assert_eq!(press(&mut world_type, 25), ControlFlow::Break(()));
        assert!(chosen());
        assert_eq!(world_type, WorldPreset::Normal);
        let dir = std::env::temp_dir().join(format!("tf-world-type-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(matches!(
            write(&dir, 9, WorldGen::default(), &None),
            ControlFlow::Break(Ok(()))
        ));
        assert!(is_terraforged(&dir));
        assert_eq!(super::super::WorldGen::preset(&dir), PRESET);
        recreate(&dir);
        reset();
        assert!(chosen());
        assert_eq!(press(&mut world_type, 25), ControlFlow::Break(()));
        assert!(!chosen());
        assert_eq!(press(&mut world_type, 25), ControlFlow::Continue(()));
        assert_eq!(press(&mut world_type, 7), ControlFlow::Continue(()));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
