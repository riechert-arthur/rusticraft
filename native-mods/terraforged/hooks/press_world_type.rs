fn press_world_type(options: &mut WorldOptions, id: u8) -> std::ops::ControlFlow<()> {
    crate::save::terraforged_world_type::press(&mut options.world_type, id)
}
