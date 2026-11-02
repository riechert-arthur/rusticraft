fn cloud_height(input: &Input<'_>, value: (MeshKey, Uniform)) -> (MeshKey, Uniform) {
    // DimensionEffects: TerraForged's overworld effects put the clouds at 300.
    let (mut key, mut uniform) = value;
    let height = crate::datapack::terraforged_builtin_pack::CLOUD_HEIGHT;
    let bottom = (height - input.camera.y) as f32;
    key.top = bottom <= 0.0;
    key.bottom = bottom + 4.0 >= 0.0;
    uniform.offset[1] = bottom;
    (key, uniform)
}
