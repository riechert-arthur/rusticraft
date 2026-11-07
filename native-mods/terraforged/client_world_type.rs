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
use horizon_world::worldgen::generator::terraforged_mod_biomes::mod_biome_files;
use horizon_world::worldgen::generator::terraforged_settings::{self, Settings};
use horizon_world::worldgen::generator::terraforged_world_preset::{
    GENERATOR_TYPE, PRESET, overworld_type_json, preset_json,
};
use std::cell::Cell;
use std::ops::ControlFlow;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// The world type's language key, as vanilla's world tab names a preset
/// (`Identifier.toLanguageKey("generator")` of `terraforged:normal`).
pub const LANG_KEY: &str = "generator.terraforged.normal";
/// [`LANG_KEY`]'s text in TerraForged's `en_us.json` (6dd607eb; TerraForged Reimagined
/// has the same entry), shown when no language in use has the key.
pub const NAME: &str = "TerraForged";

/// The Create World screen's choice. The screen runs on the main thread and the world it
/// creates is written on the server thread, so this one is shared.
static CHOSEN: AtomicBool = AtomicBool::new(false);

/// The Horizon settings of the world Re-Create started from, for the world it creates (a
/// fresh screen has none: the new world takes the settings file's). Shared for the same
/// reason as [`CHOSEN`].
static CHOSEN_SETTINGS: std::sync::Mutex<Option<Settings>> = std::sync::Mutex::new(None);

thread_local! {
    /// Set by Re-Create for the `WorldOptions::default` its body ends with: whether the
    /// world is TerraForged's, and its Horizon settings.
    static RECREATING: Cell<Option<(bool, Option<Settings>)>> = const { Cell::new(None) };
    /// Set by `WorldOptions::world_gen` when the screen's choice is TerraForged, for the
    /// `write_world_gen_settings_with` that takes its result on the same thread. Other
    /// writes (`Level::write` for a world without settings, a dedicated server's new world)
    /// stay vanilla whatever the screen last showed.
    static CREATING: Cell<bool> = const { Cell::new(false) };
}

pub fn chosen() -> bool {
    CHOSEN.load(Ordering::Relaxed)
}

/// The world-type button's label value: the translation of [`LANG_KEY`], else [`NAME`].
pub fn label() -> String {
    crate::lang::get_or(LANG_KEY, NAME)
}

/// A fresh screen starts at Normal; Re-Create keeps the world's own type.
pub fn reset() {
    let (chosen, settings) = RECREATING.take().unwrap_or((false, None));
    CHOSEN.store(chosen, Ordering::Relaxed);
    *CHOSEN_SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = settings;
}

/// Re-Create: the world's type and Horizon settings, applied when its options' defaults
/// are made, so the world it makes generates as this one did.
pub fn recreate(world: &Path) {
    let generator = terraforged_generator(world);
    RECREATING.set(Some((
        generator.is_some(),
        generator.as_ref().map(saved_settings),
    )));
}

/// `WorldOptions::world_gen`: the new world's settings are about to be written.
pub fn arm() {
    CREATING.set(chosen());
}

/// The world-type button.
pub fn press(world_type: &mut WorldPreset, id: u8) -> ControlFlow<()> {
    let mut chosen = chosen();
    let pressed = cycle(&mut chosen, world_type, id);
    CHOSEN.store(chosen, Ordering::Relaxed);
    pressed
}

/// The world-type button's cycle with TerraForged's preset in it: `#minecraft:normal` is
/// vanilla's five presets, then TerraForged's tag entry, so Single Biome, then TerraForged,
/// then Normal. Other buttons, and presses between vanilla presets, go to the screen.
fn cycle(chosen: &mut bool, world_type: &mut WorldPreset, id: u8) -> ControlFlow<()> {
    if id != 25 {
        return ControlFlow::Continue(());
    }
    if *chosen {
        *chosen = false;
    } else if *world_type == WorldPreset::SingleBiomeSurface {
        *chosen = true;
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

/// The Horizon settings a saved TerraForged world was created with (its generator's
/// `horizon_settings`; a world without them, made before they existed, is faithful).
fn saved_settings(generator: &Tag) -> Settings {
    generator
        .get("horizon_settings")
        .map_or_else(Settings::default, |saved| {
            Settings::from_json(&to_json(saved))
        })
}

/// `WorldGen::preset`: TerraForged worlds generate with TerraForged's preset, and with
/// the Horizon settings the world was created with, set for the generators the world
/// builds from now on (the game reads the preset as it opens the world, before it
/// builds them). Other worlds put the settings back to the faithful port's.
pub fn preset(world: &Path) -> ControlFlow<&'static str> {
    match terraforged_generator(world) {
        Some(generator) => {
            terraforged_settings::set(saved_settings(&generator));
            ControlFlow::Break(PRESET)
        }
        None => {
            terraforged_settings::set(Settings::default());
            ControlFlow::Continue(())
        }
    }
}

/// The settings a new TerraForged world starts with: `config/terraforged/settings.json` in
/// the game folder, written with every setting off the first time (a file that isn't JSON
/// gives every setting its default).
pub fn new_world_settings() -> Settings {
    settings_file(
        &crate::logging::game_home()
            .join("config")
            .join("terraforged")
            .join("settings.json"),
    )
}

/// The settings in the file `path`, which is written with every setting off when it is
/// missing.
fn settings_file(path: &Path) -> Settings {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(|json| Settings::from_json(&json))
            .unwrap_or_default(),
        Err(_) => {
            let defaults = Settings::default();
            if let Some(dir) = path.parent()
                && std::fs::create_dir_all(dir).is_ok()
            {
                let text = serde_json::to_string_pretty(&defaults.to_json()).unwrap_or_default();
                let _ = std::fs::write(path, text + "\n");
            }
            defaults
        }
    }
}

/// `write_world_gen_settings_with` for the Create screen's TerraForged world, with TerraForged's
/// built-in pack ([`write_pack`]). TerraForged's `DatapackHook.injectDatapack` copies that pack
/// into every world the screen makes, whatever its preset; here only TerraForged worlds get it,
/// so vanilla world types keep 26.3's overworld.
pub fn write(
    dir: &Path,
    seed: i64,
    options: WorldGen,
    flat: &Option<Tag>,
) -> ControlFlow<Result<()>> {
    if !CREATING.take() || options.preset != WorldPreset::Normal {
        return ControlFlow::Continue(());
    }
    // Re-Create's world keeps its source's settings. Tests never read or write the game
    // folder's settings file.
    let recreated = *CHOSEN_SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let horizon = match recreated {
        Some(settings) => settings,
        None if cfg!(test) => Settings::default(),
        None => new_world_settings(),
    };
    ControlFlow::Break(write_terraforged(dir, seed, options, flat.clone(), horizon))
}

/// Vanilla's settings for `options`, with TerraForged's overworld stem, whose generator
/// keeps Horizon's `horizon_settings` (`TerraForged`'s own settings live in the generator
/// too). The inner write finds nothing armed, so it is Horizon's own.
fn write_terraforged(
    dir: &Path,
    seed: i64,
    options: WorldGen,
    flat: Option<Tag>,
    horizon: Settings,
) -> Result<()> {
    super::write_world_gen_settings_with(dir, seed, options, flat)?;
    let mut settings = read_saved_data(dir, "world_gen_settings")
        .ok_or_else(|| std::io::Error::other("world_gen_settings was not written"))?;
    let mut stem = overworld_stem();
    if let Some(Tag::Compound(generator)) = stem.get_mut("generator") {
        generator.insert("horizon_settings".into(), to_tag(&horizon.to_json()));
    }
    if let Some(Tag::Compound(dimensions)) = settings.get_mut("dimensions") {
        dimensions.insert("minecraft:overworld".into(), stem);
    }
    write_saved_data(dir, "world_gen_settings", settings)?;
    write_pack(dir)
}

/// The folder name of the world's copy of TerraForged's built-in pack.
const PACK_FOLDER: &str = "terraforged";

/// The world's own copy of TerraForged's built-in pack, with its taller overworld dimension
/// type ([`overworld_type_json`], the built-in pack's file): from y -64, 1024 blocks, so the
/// generator's range, the level shapes and the synced `dimension_type` registry follow it as
/// for any world pack, and TerraForged's two biomes ([`biome_files`]), which the generator
/// reads from here as from any world pack. TerraForged's built-in pack also holds its other worldgen
/// registries; the port bundles those, so they are not copied. The Create screen copies the
/// packs its Data Packs screen showed into `datapacks` first: a folder already named
/// `terraforged` holding exactly this pack (Re-Create's copy) is kept as it is, and any other
/// (a pack of the player's own) is left alone and this pack takes the next free name,
/// `terraforged-1`, ..., which Horizon enables as a new pack, above the player's, as
/// TerraForged selects its pack last.
fn write_pack(dir: &Path) -> Result<()> {
    let datapacks = dir.join("datapacks");
    let meta = pack_meta();
    let biomes = biome_bytes();
    let mut pack = datapacks.join(PACK_FOLDER);
    for n in 1.. {
        if !pack.exists() {
            break;
        }
        if is_this_pack(&pack, &meta, &biomes) {
            return Ok(());
        }
        pack = datapacks.join(format!("{PACK_FOLDER}-{n}"));
    }
    let types = pack.join("data").join("minecraft").join("dimension_type");
    std::fs::create_dir_all(&types)?;
    std::fs::write(pack.join("pack.mcmeta"), meta)?;
    std::fs::write(types.join("overworld.json"), overworld_type_json())?;
    for (id, json) in &biomes {
        let file = pack.join(biome_file(id));
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(file, json)?;
    }
    Ok(())
}

/// TerraForged's own biomes (6dd607eb `ModBiomes`), as `(id, JSON)`: copies of the game's own
/// 26.3 biomes (`terraforged_mod_biomes`), so the table and the synchronized registry take them
/// through `native_biome_files`, and the generator from the world's pack ([`write_pack`]). A
/// biome whose vanilla parent can't be read is left out, with a warning.
pub fn biome_files() -> Vec<(String, serde_json::Value)> {
    let (biomes, missing) = mod_biome_files();
    for reason in missing {
        log::warn!("TerraForged: biome {reason}; it is left out");
    }
    biomes
}

/// [`biome_files`] as [`write_pack`] writes them.
fn biome_bytes() -> Vec<(String, Vec<u8>)> {
    biome_files()
        .into_iter()
        .filter_map(|(id, json)| Some((id, serde_json::to_vec_pretty(&json).ok()?)))
        .collect()
}

/// The pack file of the biome `id` (`terraforged:<path>`).
fn biome_file(id: &str) -> std::path::PathBuf {
    let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", id));
    Path::new("data")
        .join(namespace)
        .join("worldgen")
        .join("biome")
        .join(format!("{path}.json"))
}

fn pack_meta() -> String {
    let (major, _) = horizon_core::version::DATA_PACK_FORMAT;
    serde_json::json!({
        "pack": {
            "description": "TerraForged's overworld",
            "min_format": major,
            "max_format": major,
        }
    })
    .to_string()
}

/// Whether the folder `pack` holds exactly the pack [`write_pack`] writes, with `biomes`.
fn is_this_pack(pack: &Path, meta: &str, biomes: &[(String, Vec<u8>)]) -> bool {
    let read = |path: &Path| std::fs::read(path).ok();
    let overworld = pack.join("data/minecraft/dimension_type/overworld.json");
    let files = walk(pack);
    files.len() == 2 + biomes.len()
        && read(&pack.join("pack.mcmeta")).as_deref() == Some(meta.as_bytes())
        && read(&overworld).as_deref() == Some(overworld_type_json().as_bytes())
        && biomes
            .iter()
            .all(|(id, json)| read(&pack.join(biome_file(id))).as_ref() == Some(json))
}

/// The files under `dir` (none when it is not a folder).
fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        for entry in std::fs::read_dir(&next).into_iter().flatten().flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                stack.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files
}

/// The overworld stem of TerraForged's world preset, as NBT.
pub fn overworld_stem() -> Tag {
    let preset: serde_json::Value =
        serde_json::from_str(preset_json()).expect("TerraForged world preset JSON");
    to_tag(&preset["dimensions"]["minecraft:overworld"])
}

/// The stem's JSON as NBT: booleans are bytes, whole numbers ints, other numbers floats.
/// Only the generator's `type` ([`is_terraforged`]) and `horizon_settings` ([`preset`]) are
/// read back: the world generates from the bundled preset's stem, as Horizon's vanilla
/// worlds do from theirs.
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
    use std::path::PathBuf;

    /// A fresh folder per test; the tests leave `CHOSEN` alone, as Horizon's own title
    /// tests run the screen beside them.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tf-world-type-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn stem_of(dir: &Path, dimension: &str) -> Tag {
        world_gen_settings(dir)
            .and_then(|settings| settings.get("dimensions")?.get(dimension).cloned())
            .unwrap()
    }

    #[test]
    fn terraforged_follows_single_biome_and_leads_back_to_normal() {
        use WorldPreset::*;
        let (mut chosen, mut world_type) = (false, Normal);
        let mut seen = Vec::new();
        for _ in 0..7 {
            if cycle(&mut chosen, &mut world_type, 25).is_continue() {
                world_type = world_type.next();
            }
            seen.push((world_type, chosen));
        }
        assert_eq!(
            seen,
            [
                (Flat, false),
                (LargeBiomes, false),
                (Amplified, false),
                (SingleBiomeSurface, false),
                (Normal, true),
                (Normal, false),
                (Flat, false),
            ]
        );
        // Other buttons are the screen's, whatever the choice.
        chosen = true;
        assert_eq!(
            cycle(&mut chosen, &mut world_type, 27),
            ControlFlow::Continue(())
        );
        assert!(chosen);
    }

    #[test]
    fn a_terraforged_world_saves_and_reloads_as_terraforged() {
        let vanilla_data = vanilla_biomes();
        let dir = scratch("round-trip");
        let options = WorldGen {
            preset: WorldPreset::Normal,
            structures: false,
            bonus_chest: true,
        };
        write_terraforged(&dir, 9, options, None, Settings::default()).unwrap();
        assert!(is_terraforged(&dir));
        assert_eq!(WorldGen::preset(&dir), PRESET);
        assert_eq!(terraforged_settings::current(), Settings::default());
        // Re-Create picks TerraForged again, with the world's other choices.
        recreate(&dir);
        assert_eq!(RECREATING.take(), Some((true, Some(Settings::default()))));
        assert_eq!(WorldGen::read(&dir), Some(options));
        let settings = world_gen_settings(&dir).unwrap();
        assert_eq!(settings.get("seed"), Some(&Tag::Long(9)));
        // Only the overworld is TerraForged's, with its preset's levels.
        let overworld = stem_of(&dir, "minecraft:overworld");
        let mut expected = overworld_stem();
        if let Some(Tag::Compound(generator)) = expected.get_mut("generator") {
            generator.insert(
                "horizon_settings".into(),
                Tag::compound([
                    ("deterministic", Tag::Byte(0)),
                    ("sparse_forests", Tag::Byte(0)),
                ]),
            );
        }
        assert_eq!(overworld, expected);
        let levels = overworld.get("generator").and_then(|g| g.get("levels"));
        assert_eq!(levels.and_then(|l| l.get("max_y")), Some(&Tag::Int(480)));
        assert_eq!(
            levels.and_then(|l| l.get("auto_scale")),
            Some(&Tag::Byte(1))
        );
        let vanilla = scratch("vanilla");
        super::super::write_world_gen_settings(&vanilla, 9, options).unwrap();
        for dimension in ["minecraft:the_nether", "minecraft:the_end"] {
            assert_eq!(stem_of(&dir, dimension), stem_of(&vanilla, dimension));
        }
        assert!(!is_terraforged(&vanilla));
        assert_eq!(WorldGen::preset(&vanilla), "minecraft:normal");
        // The world's own pack gives its overworld TerraForged's height; vanilla's has none.
        let pack = dir.join("datapacks/terraforged/data/minecraft/dimension_type/overworld.json");
        let written = std::fs::read_to_string(pack).unwrap();
        assert_eq!(written, overworld_type_json());
        let height = horizon_world::worldgen::overrides::LevelHeight::of_json(
            &serde_json::from_str(&written).unwrap(),
        )
        .unwrap();
        assert_eq!((height.min_y, height.height), (-64, 1024));
        // It holds TerraForged's biomes for the generator, copied from the game's 26.3 data.
        if vanilla_data {
            let biomes = biome_bytes();
            assert_eq!(biomes.len(), 2);
            for (id, json) in biomes {
                let file = dir.join("datapacks/terraforged").join(biome_file(&id));
                assert_eq!(std::fs::read(file).unwrap(), json, "{id}");
            }
        }
        assert_eq!(
            biome_file("terraforged:oak_forest"),
            Path::new("data/terraforged/worldgen/biome/oak_forest.json")
        );
        assert!(is_this_pack(
            &dir.join("datapacks/terraforged"),
            &pack_meta(),
            &biome_bytes()
        ));
        // A world written outside the Create screen has no pack.
        assert!(!vanilla.join("datapacks").exists());
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&vanilla).unwrap();
    }

    /// Points the biome copies at the game's 26.3 data when it is there (`HORIZON_TEST_RESOURCES`, else the
    /// checkout's `.local`); without it the tests check only what needs no vanilla data. Tests that write a pack call
    /// it first: Horizon's own tests load the same folder in parallel (`Biomes::load` remembers it), so the copies
    /// must not appear between a write and its check.
    fn vanilla_biomes() -> bool {
        let root = std::env::var_os("HORIZON_TEST_RESOURCES")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/horizon-client/26.3")
            });
        let present = root
            .join("data/minecraft/worldgen/biome/plains.json")
            .is_file();
        if present {
            horizon_world::worldgen::generator::terraforged_mod_biomes::remember_root(&root);
        } else {
            eprintln!(
                "skipping the biome copies: no 26.3 data at {}",
                root.display()
            );
        }
        present
    }

    #[test]
    fn the_biomes_copy_the_games_own() {
        if !vanilla_biomes() {
            return;
        }
        let files = biome_files();
        let ids: Vec<&str> = files.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, ["terraforged:cave", "terraforged:oak_forest"]);
        let root = std::env::var_os("HORIZON_TEST_RESOURCES")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/horizon-client/26.3")
            });
        let plains: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join("data/minecraft/worldgen/biome/plains.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(files[1].1, plains);
        assert_eq!(files[0].1["carvers"], serde_json::json!([]));
    }

    /// The `native_biome_files` hook: a TerraForged world takes TerraForged's biomes, a vanilla world none (its table
    /// and registry stay 26.3's), and a client of another server keeps those the server's registry names.
    /// A world keeps the settings it was made with in its generator, whatever the settings
    /// file says later; the file is written with every setting off when it is missing.
    #[test]
    fn a_world_keeps_its_horizon_settings() {
        vanilla_biomes();
        let on = Settings {
            deterministic: true,
            sparse_forests: true,
        };
        let dir = scratch("settings");
        write_terraforged(&dir, 3, WorldGen::default(), None, on).unwrap();
        let generator = terraforged_generator(&dir).unwrap();
        assert_eq!(
            generator.get("horizon_settings"),
            Some(&Tag::compound([
                ("deterministic", Tag::Byte(1)),
                ("sparse_forests", Tag::Byte(1)),
            ]))
        );
        assert_eq!(saved_settings(&generator), on);
        // Re-Create keeps them.
        recreate(&dir);
        assert_eq!(RECREATING.take(), Some((true, Some(on))));
        // A world from before the settings existed is faithful.
        let mut old = generator.clone();
        if let Tag::Compound(entries) = &mut old {
            entries.retain(|key, _| key.to_string_lossy() != "horizon_settings");
        }
        assert_eq!(saved_settings(&old), Settings::default());
        let file = dir.join("config").join("settings.json");
        assert_eq!(settings_file(&file), Settings::default());
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "{\n  \"deterministic\": false,\n  \"sparse_forests\": false\n}\n"
        );
        std::fs::write(&file, "{\"deterministic\": true, \"sparse_forests\": true}").unwrap();
        assert_eq!(settings_file(&file), on);
        // A key left out keeps its default (dense forests).
        std::fs::write(&file, "{\"deterministic\": true}").unwrap();
        assert_eq!(
            settings_file(&file),
            Settings {
                sparse_forests: false,
                ..on
            }
        );
        std::fs::write(&file, "not json").unwrap();
        assert_eq!(settings_file(&file), Settings::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_terraforged_worlds_take_terraforged_biomes() {
        use crate::world_biomes::{native_biomes, native_biomes_for_registry};
        let ids = |biomes: Vec<(String, serde_json::Value)>| -> Vec<String> {
            biomes.into_iter().map(|(id, _)| id).collect()
        };
        let both: &[&str] = if vanilla_biomes() {
            &["terraforged:cave", "terraforged:oak_forest"]
        } else {
            &[]
        };
        let terraforged = scratch("biomes-terraforged");
        write_terraforged(
            &terraforged,
            5,
            WorldGen::default(),
            None,
            Settings::default(),
        )
        .unwrap();
        let vanilla = scratch("biomes-vanilla");
        super::super::write_world_gen_settings(&vanilla, 5, WorldGen::default()).unwrap();
        assert_eq!(ids(native_biomes(Some(&terraforged))), both);
        assert!(native_biomes(Some(&vanilla)).is_empty());
        assert!(native_biomes(Some(&scratch("biomes-missing"))).is_empty());
        assert_eq!(ids(native_biomes(None)), both);
        assert!(native_biomes_for_registry(["minecraft:plains"]).is_empty());
        assert_eq!(
            ids(native_biomes_for_registry([
                "minecraft:plains",
                "terraforged:cave"
            ])),
            both[..both.len().min(1)]
        );
        std::fs::remove_dir_all(&terraforged).unwrap();
        std::fs::remove_dir_all(&vanilla).unwrap();
    }

    #[test]
    fn only_the_create_screens_write_is_terraforged() {
        vanilla_biomes();
        let dir = scratch("armed");
        // Nothing armed (`Level::write` for a world without settings): Horizon's write, no pack.
        assert!(write(&dir, 1, WorldGen::default(), &None).is_continue());
        assert!(!dir.join("datapacks").exists());
        CREATING.set(true);
        assert!(matches!(
            write(&dir, 1, WorldGen::default(), &None),
            ControlFlow::Break(Ok(()))
        ));
        assert!(!CREATING.get());
        assert!(is_terraforged(&dir));
        assert!(is_this_pack(
            &dir.join("datapacks/terraforged"),
            &pack_meta(),
            &biome_bytes()
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn vanilla_presets_stay_vanilla_without_the_pack() {
        use WorldPreset::*;
        let dir = scratch("vanilla-presets");
        for preset in [Normal, Flat, LargeBiomes, Amplified, SingleBiomeSurface] {
            let options = WorldGen {
                preset,
                ..WorldGen::default()
            };
            // Vanilla's choice, or TerraForged chosen while the screen shows another preset.
            for terraforged in [false, true] {
                if terraforged && preset == Normal {
                    continue;
                }
                CREATING.set(terraforged);
                assert!(write(&dir, 3, options, &None).is_continue());
                assert!(!CREATING.get());
                assert!(!dir.exists());
            }
        }
    }

    #[test]
    fn a_players_own_terraforged_pack_is_kept() {
        vanilla_biomes();
        let dir = scratch("clash");
        let own = dir.join("datapacks/terraforged");
        std::fs::create_dir_all(own.join("data/minecraft/dimension_type")).unwrap();
        std::fs::write(
            own.join("pack.mcmeta"),
            "{\"pack\":{\"description\":\"mine\"}}",
        )
        .unwrap();
        // A pack with this pack's dimension type but its own `pack.mcmeta` is the player's.
        std::fs::write(
            own.join("data/minecraft/dimension_type/overworld.json"),
            overworld_type_json(),
        )
        .unwrap();
        std::fs::write(dir.join("datapacks/terraforged-1"), "a file").unwrap();
        let before = walk(&own);
        CREATING.set(true);
        assert!(write(&dir, 1, WorldGen::default(), &None).is_break());
        assert_eq!(walk(&own), before);
        assert_eq!(
            std::fs::read_to_string(own.join("pack.mcmeta")).unwrap(),
            "{\"pack\":{\"description\":\"mine\"}}"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("datapacks/terraforged-1")).unwrap(),
            "a file"
        );
        assert!(is_this_pack(
            &dir.join("datapacks/terraforged-2"),
            &pack_meta(),
            &biome_bytes()
        ));
        // Re-Create copies the world's pack first: it is kept, and no second copy is made.
        let again = scratch("clash-recreate");
        let copy = again.join("datapacks/terraforged");
        let written = dir.join("datapacks/terraforged-2");
        for file in walk(&written) {
            let to = copy.join(file.strip_prefix(&written).unwrap());
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::copy(&file, to).unwrap();
        }
        CREATING.set(true);
        assert!(write(&again, 1, WorldGen::default(), &None).is_break());
        assert!(is_this_pack(&copy, &pack_meta(), &biome_bytes()));
        assert_eq!(
            std::fs::read_dir(again.join("datapacks")).unwrap().count(),
            1
        );
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&again).unwrap();
    }
}
