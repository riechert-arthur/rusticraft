fn create_preview(
    atlas: &HudAtlas,
    batch: &mut HudBatch,
    seed: &str,
    options: &WorldOptions,
    value: (),
) {
    if options.tab == 1 && crate::save::terraforged_world_type::chosen() {
        terraforged_create_preview::draw(atlas, batch, seed);
    }
    value
}
