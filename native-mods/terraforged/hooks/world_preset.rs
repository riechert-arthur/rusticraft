fn world_preset(world: &Path) -> std::ops::ControlFlow<&'static str> {
    terraforged_world_type::preset(world)
}
