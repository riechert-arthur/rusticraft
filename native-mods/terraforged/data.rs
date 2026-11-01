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
