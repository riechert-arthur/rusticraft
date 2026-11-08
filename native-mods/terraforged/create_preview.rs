// Horizon's TerraForged map on the Create World screen (neither TerraForged tree has one there: 6dd607eb
// and the 1.18.2-0.3.1 jar show nothing, and TerraForged Reimagined's `PreviewWidget` sits on its own
// settings screen). With TerraForged as the world type, the World tab shows `/tf preview`'s picture
// (`Previewer`'s colours) of the 4096 blocks around spawn for the typed seed and the settings the new
// world gets, under the seed box. A blank seed is a random one, known only once the world is made.
//
// The picture renders on a thread of its own, never on the screen's: one at a time, always for the
// latest seed (a render for a seed already replaced stops at its next pixel, so typing quickly costs
// one render), and only while the screen shows the map. It is drawn through a world-list icon slot,
// which the screen doesn't use and the world list fills again each frame it draws.

use super::{FOOTER, grid};
use crate::{
    hud::{HudAtlas, HudBatch},
    text::Text,
};
use horizon_world::worldgen::generator::terraforged_preview::{View, new_world_map};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// The picture's side in pixels: one world-list icon slot.
const SIDE: u32 = 64;
/// Blocks per pixel: the picture spans 4096 blocks around spawn.
const ZOOM: f32 = 64.0;
const SLOT: &str = "horizon/world-icon0";
/// A render stops once the screen has not drawn the map for this long (it was left or switched tab).
const GONE: Duration = Duration::from_millis(500);

/// The map's state, shared by the screen and the render thread.
struct Map {
    /// The seed the screen shows (`None`: blank).
    wanted: Option<i64>,
    /// The latest finished picture: its seed and RGBA pixels.
    shown: Option<(i64, Arc<Vec<u8>>)>,
    /// Whether a render thread is running.
    rendering: bool,
    /// The render thread's last failure, shown in place of the map.
    failed: Option<String>,
}

static MAP: Mutex<Map> = Mutex::new(Map {
    wanted: None,
    shown: None,
    rendering: false,
    failed: None,
});
/// Bumped whenever `wanted` changes: a render whose request is older stops.
static REQUEST: AtomicU64 = AtomicU64::new(0);
/// When the screen last drew the map, in milliseconds since [`epoch`] plus one (0: never).
static DRAWN: AtomicU64 = AtomicU64::new(0);

fn epoch() -> Instant {
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

fn now() -> u64 {
    epoch().elapsed().as_millis() as u64
}

/// Whether the screen has stopped drawing the map: it was left or switched tab.
fn gone() -> bool {
    match DRAWN.load(Ordering::Acquire) {
        0 => true,
        drawn => (now() + 1).saturating_sub(drawn) > GONE.as_millis() as u64,
    }
}

fn map() -> std::sync::MutexGuard<'static, Map> {
    MAP.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A fresh Create World screen: the settings may differ from the last screen's (Re-Create, an edited
/// settings file), so its pictures are drawn again.
pub fn forget() {
    let mut map = map();
    map.shown = None;
    map.failed = None;
    REQUEST.fetch_add(1, Ordering::AcqRel);
}

/// Where the map goes on a `w` x `h` screen: its left edge, the top of its title line and its side,
/// below the World tab's controls and above the footer, at most 128 wide; `None` where less than 32
/// would fit (the screen then shows nothing).
fn place(w: f32, h: f32) -> Option<(f32, f32, f32)> {
    let (_, gy) = grid(w, h, 1);
    let top = gy + 117.0 + 10.0;
    let size = (h - FOOTER - 10.0 - top - 12.0).min(128.0).floor();
    (size >= 32.0).then(|| (((w - size) / 2.0).floor(), top, size))
}

/// The World tab's map, below its controls.
pub fn draw(atlas: &HudAtlas, batch: &mut HudBatch, seed: &str) {
    let (w, h) = (batch.width.ceil(), batch.height.ceil());
    let Some((x, top, size)) = place(w, h) else {
        return;
    };
    let centred = |batch: &mut HudBatch, text: &Text, y: f32, colour: u32| {
        let width = atlas.font.width_text(text).floor();
        batch.text_styled(atlas, text, ((w - width) / 2.0).floor(), y, colour, true);
    };
    let seed = crate::title::parse_seed(seed);
    let (shown, failed) = request(seed);
    if seed.is_none() {
        centred(
            batch,
            &Text::literal("TerraForged: the map shows once a seed is typed"),
            top + 2.0,
            0xFF_A0_A0_A0,
        );
        return;
    }
    if let Some(error) = failed {
        centred(
            batch,
            &Text::literal(format!("TerraForged's map failed: {error}")),
            top + 2.0,
            0xFF_FF_55_55,
        );
        return;
    }
    centred(batch, &Text::literal("TerraForged"), top, 0xFF_E0_E0_E0);
    let y = top + 12.0;
    match (shown, atlas.sprites.get(SLOT)) {
        (Some((drawn, rgba)), Some(rect)) => {
            batch.patches.push(crate::font::Patch {
                x: (rect.u0 * atlas.width as f32).round() as u32,
                y: (rect.v0 * atlas.height as f32).round() as u32,
                width: SIDE,
                height: SIDE,
                rgba: rgba.to_vec(),
            });
            // A picture of an earlier seed stays, dimmed, until the new one is ready.
            let tint = if Some(drawn) == seed {
                u32::MAX
            } else {
                0xFF_70_70_70
            };
            batch.sprite_scaled(atlas, SLOT, x, y, size, size, tint);
        }
        _ => batch.fill(atlas, x, y, size, size, 0xFF_20_20_20),
    }
}

/// One frame's map of `seed`: the latest picture and failure, starting a render thread when the
/// picture shown is not `seed`'s and none is running.
fn request(seed: Option<i64>) -> (Option<(i64, Arc<Vec<u8>>)>, Option<String>) {
    DRAWN.store(now() + 1, Ordering::Release);
    let mut map = map();
    if map.wanted != seed {
        map.wanted = seed;
        map.failed = None;
        REQUEST.fetch_add(1, Ordering::AcqRel);
    }
    let stale = map
        .shown
        .as_ref()
        .is_none_or(|(drawn, _)| Some(*drawn) != seed);
    if seed.is_some() && stale && !map.rendering && map.failed.is_none() {
        map.rendering = std::thread::Builder::new()
            .name("TerraForged map".to_owned())
            .spawn(render)
            .is_ok();
    }
    (map.shown.clone(), map.failed.clone())
}

/// The render thread: draws the wanted seed's picture until the picture shown is the wanted one, the
/// seed is blank, or the screen is gone.
fn render() {
    loop {
        let (seed, request) = {
            let mut map = map();
            let gone = gone();
            match map.wanted {
                Some(seed)
                    if !gone && map.shown.as_ref().is_none_or(|(drawn, _)| *drawn != seed) =>
                {
                    (seed, REQUEST.load(Ordering::Acquire))
                }
                _ => {
                    map.rendering = false;
                    return;
                }
            }
        };
        let cancelled = || REQUEST.load(Ordering::Acquire) != request || gone();
        let view = View {
            width: SIDE,
            height: SIDE,
            zoom: ZOOM,
            x: 0.0,
            y: 0.0,
        };
        // The settings the world gets: Re-Create's source world's, else the settings file's.
        let settings = crate::save::terraforged_world_type::creating_settings();
        // A panic in a sample (where Java would throw) fails the map, not the screen.
        let drawn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            new_world_map(seed, settings, view, &cancelled)
        }))
        .unwrap_or_else(|_| Err("the sampler panicked".to_owned()));
        let mut map = map();
        match drawn {
            Ok(Some(pixels)) if REQUEST.load(Ordering::Acquire) == request => {
                let rgba = pixels
                    .into_iter()
                    .flat_map(|rgb| [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 0xFF])
                    .collect();
                map.shown = Some((seed, Arc::new(rgba)));
            }
            // Stopped, or finished for a seed already replaced: the loop takes the new one.
            Ok(_) => {}
            Err(error) => {
                if REQUEST.load(Ordering::Acquire) == request {
                    map.failed = Some(error);
                }
                map.rendering = false;
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(what: &str, until: impl Fn() -> bool) {
        let start = Instant::now();
        while !until() {
            assert!(start.elapsed() < Duration::from_secs(120), "{what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn rendering() -> bool {
        map().rendering
    }

    /// The screen's side of the map, frame by frame: seeds typed quickly cost one render and end on the
    /// last one's picture; a blank seed renders nothing; leaving the screen ends the render thread; a
    /// new screen draws its pictures again. One test, as the map's state is the program's.
    /// At every screen size the game allows (320x240 GUI pixels and up), the title line and the map
    /// stay on the screen, clear of the tab bar, the World tab's buttons, the seed box and the footer.
    #[test]
    fn the_map_fits_between_the_controls_and_the_footer() {
        let options = super::super::WorldOptions {
            tab: 1,
            ..Default::default()
        };
        let mut shown = 0;
        for w in (320..=1920).step_by(7) {
            for h in (240..=1080).step_by(5) {
                let (w, h) = (w as f32, h as f32);
                let Some((x, top, size)) = place(w, h) else {
                    continue;
                };
                shown += 1;
                let map = [x.min((w - 300.0) / 2.0), top, x + size, top + 12.0 + size];
                assert!(map[0] >= 0.0 && map[2] <= w, "{w}x{h}: {map:?}");
                assert!(map[1] >= 24.0 && map[3] <= h - FOOTER, "{w}x{h}: {map:?}");
                let clear = |[x0, y0, x1, y1]: [f32; 4]| {
                    x1 <= map[0] || x0 >= map[2] || y1 <= map[1] || y0 >= map[3]
                };
                for b in super::super::buttons(w, h, false, &options) {
                    assert!(
                        clear([b.x, b.y, b.x + b.w, b.y + 20.0]),
                        "{w}x{h}: {}",
                        b.label
                    );
                }
                let (_, [fx, fy, fw, fh]) = super::super::field_rect(w, h, 1).unwrap();
                assert!(clear([fx, fy, fx + fw, fy + fh]), "{w}x{h}: seed box");
            }
        }
        assert!(shown > 1000);
    }

    #[test]
    fn maps_follow_the_typed_seed_off_the_screen_s_thread() {
        forget();
        assert_eq!(request(None), (None, None));
        assert!(!rendering(), "a blank seed renders nothing");
        // Typed quickly: every frame asks for a new seed, so each render gives way to the next.
        for seed in 0..50 {
            let (shown, _) = request(Some(seed));
            assert!(shown.is_none());
        }
        wait("the last seed's picture", || {
            request(Some(49)).0.is_some_and(|(seed, _)| seed == 49)
        });
        wait("the thread ends once the picture is shown", || !rendering());
        let (Some((_, rgba)), None) = request(Some(49)) else {
            panic!("no picture");
        };
        assert_eq!(rgba.len(), (SIDE * SIDE * 4) as usize);
        assert!(rgba.chunks(4).all(|p| p[3] == 0xFF));
        // The same seed again: nothing to render.
        request(Some(49));
        assert!(!rendering());
        // A new seed: the old picture stays until the new one is ready.
        let (shown, _) = request(Some(777));
        assert_eq!(shown.map(|(seed, _)| seed), Some(49));
        assert!(rendering());
        // Left: no frame draws the map, so the render stops and its thread ends without a picture.
        DRAWN.store(0, Ordering::Release);
        wait("the thread ends once the screen is gone", || !rendering());
        assert_eq!(map().shown.as_ref().map(|(seed, _)| *seed), Some(49));
        // A fresh screen forgets the pictures.
        forget();
        assert_eq!(request(None), (None, None));
    }
}
