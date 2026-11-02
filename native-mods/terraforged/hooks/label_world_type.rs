fn label_world_type(options: &WorldOptions, mut buttons: Vec<Button>) -> Vec<Button> {
    if options.tab == 1 && crate::save::terraforged_world_type::chosen() {
        for button in buttons.iter_mut().filter(|button| button.id == 25) {
            button.label = labelled(
                "selectWorld.mapType",
                crate::save::terraforged_world_type::LABEL.to_owned(),
            );
        }
    }
    buttons
}
