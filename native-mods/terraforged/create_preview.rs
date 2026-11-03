// TerraForged's terrain on the Create World screen: when TerraForged is the world type,
// the World tab shows the continent preview (`Previewer`'s colours, `/tf preview`'s
// picture) around spawn for the typed seed, under the seed box, and follows the seed
// as it is typed. A blank seed is a random one, known only once the world is made.

use super::{FOOTER, grid};
use crate::{
    hud::{HudAtlas, HudBatch},
    text::Text,
};
use horizon_world::worldgen::generator::terraforged_preview::{Preview, View};
use std::sync::Mutex;

/// The picture's side in pixels: one world-list icon slot, which the screen doesn't use.
const SIDE: u32 = 64;
/// Blocks per pixel: the picture spans 4096 blocks around spawn.
const ZOOM: f32 = 64.0;
const SLOT: &str = "horizon/world-icon0";

/// The seed last drawn and its picture, as RGBA.
static DRAWN: Mutex<Option<(i64, Vec<u8>)>> = Mutex::new(None);

/// The World tab's map, below its controls.
pub fn draw(atlas: &HudAtlas, batch: &mut HudBatch, seed: &str) {
    let (w, h) = (batch.width.ceil(), batch.height.ceil());
    let (_, gy) = grid(w, h, 1);
    let top = gy + 117.0 + 10.0;
    let size = (h - FOOTER - 10.0 - top - 12.0).min(128.0).floor();
    if size < 32.0 {
        return;
    }
    let x = ((w - size) / 2.0).floor();
    let centred = |batch: &mut HudBatch, text: &Text, y: f32, colour: u32| {
        let width = atlas.font.width_text(text).floor();
        batch.text_styled(atlas, text, ((w - width) / 2.0).floor(), y, colour, true);
    };
    let Some(seed) = crate::title::parse_seed(seed) else {
        centred(
            batch,
            &Text::literal("TerraForged: the map shows once a seed is typed"),
            top + 2.0,
            0xFF_A0_A0_A0,
        );
        return;
    };
    let mut drawn = DRAWN.lock().unwrap_or_else(|e| e.into_inner());
    if drawn.as_ref().is_none_or(|(last, _)| *last != seed) {
        // A fresh noise for each seed: its continent and river caches are keyed by cell
        // alone, as a world only ever has one seed.
        let Ok(preview) = Preview::new() else {
            return;
        };
        let view = View {
            width: SIDE,
            height: SIDE,
            zoom: ZOOM,
            x: 0.0,
            y: 0.0,
        };
        let rgba = preview
            .render(seed as i32, view)
            .into_iter()
            .flat_map(|rgb| [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 0xFF])
            .collect();
        *drawn = Some((seed, rgba));
    }
    let Some((_, rgba)) = drawn.as_ref() else {
        return;
    };
    let Some(rect) = atlas.sprites.get(SLOT) else {
        return;
    };
    centred(batch, &Text::literal("TerraForged"), top, 0xFF_E0_E0_E0);
    batch.patches.push(crate::font::Patch {
        x: (rect.u0 * atlas.width as f32).round() as u32,
        y: (rect.v0 * atlas.height as f32).round() as u32,
        width: SIDE,
        height: SIDE,
        rgba: rgba.clone(),
    });
    batch.sprite_scaled(atlas, SLOT, x, top + 12.0, size, size, u32::MAX);
}
