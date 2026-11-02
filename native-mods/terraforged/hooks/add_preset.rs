fn add_preset(bundle: &str, value: BTreeMap<String, String>) -> BTreeMap<String, String> {
    crate::worldgen::generator::terraforged_world_preset::with_preset(bundle, value)
}
