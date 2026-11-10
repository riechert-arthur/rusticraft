fn table_biomes(overrides: &mut WorldgenOverrides, known: &dyn Fn(&str) -> bool) {
    crate::worldgen::generator::terraforged_mod_biomes::join_table_biomes(overrides, known);
}
