fn terraforged_biome_files(
    world: Option<&Path>,
    mut value: Vec<(String, Value)>,
) -> Vec<(String, Value)> {
    if world.is_none_or(crate::save::terraforged_world_type::is_terraforged) {
        value.extend(crate::save::terraforged_world_type::biome_files());
    }
    value
}
