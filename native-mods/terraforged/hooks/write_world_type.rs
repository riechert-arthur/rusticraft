fn write_world_type(
    dir: &Path,
    seed: i64,
    options: WorldGen,
    flat_settings: &Option<Tag>,
) -> std::ops::ControlFlow<Result<()>> {
    terraforged_world_type::write(dir, seed, options, flat_settings)
}
