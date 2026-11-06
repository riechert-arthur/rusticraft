fn for_world(
    data: Arc<GeneratorData>,
    seed: i64,
    preset: &str,
    dimension: &str,
    structures: bool,
) -> Result<OverworldGenerator, GeneratorError> {
    terraforged_pipeline::build(data, seed, preset, dimension, structures)
}
