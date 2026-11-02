fn perform_command(game: &mut Game<'_>, player: &NativeServerPlayer, command: &str, ran: &mut bool) {
    if !*ran && terraforged_commands::perform(game, player, command) {
        *ran = true;
    }
}
