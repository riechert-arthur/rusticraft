fn level_type_world_gen(
    dir: &Path,
    level_type: &str,
    seed: i64,
    structures: bool,
    written: &mut Option<Result<(), String>>,
) {
    use horizon_client::save::terraforged_world_type as world_type;
    if written.is_some() || !world_type::is_level_type(level_type) {
        return;
    }
    let settings =
        Game::native_config_dir().map(|config| config.join("terraforged").join("settings.json"));
    *written = Some(settings.and_then(|settings| {
        world_type::write_level_type(dir, seed, structures, &settings).map_err(|e| e.to_string())
    }));
}
