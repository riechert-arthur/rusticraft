fn native_biomes(mut value: Vec<(String, Value)>) -> Vec<(String, Value)> {
    value.extend(crate::datapack::terraforged_builtin_pack::biome_files());
    value
}
