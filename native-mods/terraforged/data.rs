/// The bytes of a file in this bundle's `data/` folder (`terraforged_data!("worldgen/terrain_type/hills.json")`),
/// embedded in the build: horizon-mixins stages that folder under `native-data/terraforged/` in the workspace.
#[macro_export]
macro_rules! terraforged_data {
    ($file:literal $(,)?) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../native-data/terraforged/",
            $file
        ))
    };
}
