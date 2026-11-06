fn register_commands(game: &Game<'_>, commands: &mut Vec<NativeCommand>) {
    terraforged_commands::register(game, commands);
}
