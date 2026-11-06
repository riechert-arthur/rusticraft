fn prepare_decoration(
    data: &Arc<GeneratorData>,
    preset: &str,
    dimension: &str,
) -> std::ops::ControlFlow<Result<(), GeneratorError>> {
    terraforged_pipeline::prepare_decoration(data, preset, dimension)
}
