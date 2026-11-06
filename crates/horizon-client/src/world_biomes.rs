//! The biomes native mods add to a world. A world's biome table (`biome::BiomeTable`), its
//! synchronized `minecraft:worldgen/biome` registry and the commands' biome ids all come from
//! one list, read when the world opens, so a world a mod doesn't take keeps 26.3's biomes.
use serde_json::Value;
use std::path::Path;

/// The biomes native mods add, as `(id, biome JSON)` in 26.3's format. A native mod adds its own
/// with a RETURN hook here. `world` is the folder (beside `level.dat`) of the world this game
/// serves: a mod adds only the biomes that world takes, e.g. none to a world type it doesn't
/// generate. `None` is a client of another server: every biome the mod can add, of which the
/// client keeps those the server's registry names. The game reads them through
/// [`native_biomes`]; the generator reads a mod's biomes from its data pack.
pub fn native_biome_files(world: Option<&Path>) -> Vec<(String, Value)> {
    let _ = world;
    Vec::new()
}

/// [`native_biome_files`] as the game takes them: a valid `Identifier` each, with a non-empty
/// namespace and path (`Identifier.isValidNamespace`/`isValidPath`; a client can't read
/// others), the first of each id.
pub fn native_biomes(world: Option<&Path>) -> Vec<(String, Value)> {
    valid(native_biome_files(world))
}

fn valid(files: Vec<(String, Value)>) -> Vec<(String, Value)> {
    fn valid(id: &str) -> bool {
        let Some((ns, path)) = id.split_once(':') else {
            return false;
        };
        let common = |c: char| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-' | '.');
        // Both parts non-empty: `Identifier.parse(":x")` reads `minecraft:x`, another name.
        !ns.is_empty()
            && !path.is_empty()
            && ns.chars().all(common)
            && path.chars().all(|c| common(c) || c == '/')
    }
    let mut out: Vec<(String, Value)> = Vec::new();
    for (id, json) in files {
        if !valid(&id) {
            log::warn!("Horizon: a native mod's biome {id:?} is not a namespaced id; skipped");
        } else if !out.iter().any(|(seen, _)| *seen == id) {
            out.push((id, json));
        }
    }
    out
}

/// What a client of another server keeps of every biome native mods can add: those the
/// server's `minecraft:worldgen/biome` registry names, so the client's table draws the
/// server's mod biomes and no others.
pub fn native_biomes_for_registry<'a>(
    registry: impl IntoIterator<Item = &'a str>,
) -> Vec<(String, Value)> {
    registered(native_biomes(None), registry)
}

fn registered<'a>(
    native: Vec<(String, Value)>,
    registry: impl IntoIterator<Item = &'a str>,
) -> Vec<(String, Value)> {
    let names: std::collections::HashSet<&str> = registry.into_iter().collect();
    native
        .into_iter()
        .filter(|(id, _)| names.contains(id.as_str()))
        .collect()
}

/// A server's biome ids for its commands' biome arguments: its world's table (vanilla's
/// biomes and the native mods' ones the world takes), which its synchronized registry names
/// too, sorted.
pub fn biome_ids(table: &crate::biome::Biomes) -> Vec<String> {
    let mut ids: Vec<String> = table.list.iter().map(|b| b.name.clone()).collect();
    ids.sort();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn no_native_biomes_without_a_mod() {
        let world = Path::new("saves/World");
        assert!(native_biome_files(Some(world)).is_empty());
        assert!(native_biomes(Some(world)).is_empty());
        assert!(native_biomes(None).is_empty());
        assert!(native_biomes_for_registry(["minecraft:plains", "mod:cave"]).is_empty());
    }

    #[test]
    fn native_biomes_keep_valid_ids_once() {
        let blue = json!({"effects": {"water_color": 0x0000ff}});
        let red = json!({"effects": {"water_color": 0xff0000}});
        let native = valid(vec![
            ("mod:cave".to_owned(), blue.clone()),
            ("cave".to_owned(), blue.clone()),
            ("Mod:Upper".to_owned(), blue.clone()),
            ("mod:".to_owned(), blue.clone()),
            (":cave".to_owned(), blue.clone()),
            ("mod:deep/caves".to_owned(), blue.clone()),
            ("mod:cave".to_owned(), red.clone()),
            ("minecraft:plains".to_owned(), red),
        ]);
        let ids: Vec<&str> = native.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, ["mod:cave", "mod:deep/caves", "minecraft:plains"]);
        assert_eq!(native[0].1, blue);
    }

    /// The prepared resources (`HORIZON_TEST_RESOURCES`), when there are some.
    fn resources() -> Option<std::path::PathBuf> {
        let root = std::env::var_os("HORIZON_TEST_RESOURCES")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../.local/horizon-client")
                    .join(horizon_core::VANILLA_VERSION)
            });
        root.join("data/minecraft/worldgen/biome")
            .is_dir()
            .then_some(root)
    }

    /// A mod like TerraForged (#226): two biomes, in its own world type only.
    fn mod_biomes(world: Option<&Path>) -> Vec<(String, Value)> {
        let mine = world.is_none_or(|w| w.ends_with("modded"));
        let biome = |path: &str| {
            let json = json!({"temperature": 0.7, "downfall": 0.8, "has_precipitation": true,
                "effects": {"water_color": 0x0000ff}});
            (format!("tf:{path}"), json)
        };
        if mine {
            vec![biome("cave"), biome("oak_forest")]
        } else {
            Vec::new()
        }
    }

    #[test]
    fn a_mods_biomes_reach_only_its_worlds_table_registry_and_commands() {
        let Some(root) = resources() else {
            eprintln!("skipped: no prepared resources");
            return;
        };
        let none = &horizon_world::loot::PackOverlay::NONE;
        let names = |sync: &crate::lan::data::SyncData| -> Vec<String> {
            sync.dynamic.registries["minecraft:worldgen/biome"]
                .iter()
                .map(|(name, _)| name.clone())
                .collect()
        };
        let vanilla = std::sync::Arc::new(crate::biome::Biomes::load(&root).unwrap());
        let vanilla_names: Vec<String> = vanilla.list.iter().map(|b| b.name.clone()).collect();
        let vanilla_registry = names(&crate::lan::data::SyncData::load(&root, none).unwrap());
        // The game's table, shared by a world's server and its chunk workers, through worlds
        // of both kinds in turn.
        let table = crate::biome::BiomeTable::new(vanilla.clone());
        for world in [
            "saves/vanilla",
            "saves/modded",
            "saves/vanilla",
            "saves/modded",
        ] {
            let modded = world.ends_with("modded");
            table.set_native(valid(mod_biomes(Some(Path::new(world)))));
            let server = table.current();
            let sync =
                crate::lan::data::SyncData::load_for_world(&root, none, server.native()).unwrap();
            let registry = names(&sync);
            let commands = biome_ids(&server);
            // A client of that server, with the mod installed, keeps what the registry names.
            let client = vanilla.with_native(registered(
                mod_biomes(None),
                registry.iter().map(|n| n.as_str()),
            ));
            let mods = ["tf:cave", "tf:oak_forest"];
            if modded {
                for id in mods {
                    assert!(server.contains(id), "{world}: table has {id}");
                    assert!(
                        registry.iter().any(|n| n == id),
                        "{world}: registry has {id}"
                    );
                    assert!(
                        commands.iter().any(|n| n == id),
                        "{world}: commands take {id}"
                    );
                    assert_eq!(client.id(id), server.id(id), "{world}: client draws {id}");
                }
                // Vanilla's ids stay put.
                for (i, name) in vanilla_names.iter().enumerate() {
                    assert_eq!(usize::from(server.id(name)), i);
                }
            } else {
                // 26.3's biomes exactly, everywhere.
                let table_names: Vec<String> = server.list.iter().map(|b| b.name.clone()).collect();
                assert_eq!(table_names, vanilla_names, "{world}: table");
                assert_eq!(registry, vanilla_registry, "{world}: registry");
                let mut sorted = vanilla_names.clone();
                sorted.sort();
                assert_eq!(commands, sorted, "{world}: commands");
                assert_eq!(client.list.len(), vanilla.list.len(), "{world}: client");
                assert!(server.native().is_empty() && !server.contains("tf:cave"));
            }
            // The table and the registry name the same biomes, so no chunk's biome is sent
            // as the registry's first entry (`ChunkEncoder`'s fallback).
            let mut sorted_registry = registry.clone();
            sorted_registry.sort();
            assert_eq!(commands, sorted_registry, "{world}: table = registry");
        }
    }
}
