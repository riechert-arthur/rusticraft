fn for_dimension(
    data: Arc<GeneratorData>,
    seed: i64,
    preset: &str,
    dimension: &str,
) -> Result<OverworldGenerator, GeneratorError> {
    terraforged_pipeline::build(data, seed, preset, dimension, true)
}
