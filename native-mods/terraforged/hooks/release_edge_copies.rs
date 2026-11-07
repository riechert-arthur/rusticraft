fn release_edge_copies(x: i32, z: i32, region: &[Option<Proto>; 9]) {
    // The empty slots are the finished neighbours the edge decoration generated again.
    let copies: Vec<(i32, i32)> = (0..9)
        .filter(|&i| region[i].is_none())
        .map(|i| (x.wrapping_add(i as i32 % 3 - 1), z.wrapping_add(i as i32 / 3 - 1)))
        .collect();
    horizon_world::worldgen::generator::terraforged_pipeline::release_finished(&copies);
}
