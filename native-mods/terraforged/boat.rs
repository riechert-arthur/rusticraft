/*
 * MIT License
 *
 * Copyright (c) 2021 TerraForged
 * Copyright (c) 2020 TerraForged
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */

// `BoatHook.floatTheBoat` (TerraForged 6dd607eb, `MixinBoat` on `isUnderwater`'s RETURN): a
// player's boat under a slightly higher water surface (a river's stepped levels) rises toward
// it instead of sinking.

use super::{Aabb, Boat, Game, HEIGHT};

/// Whether the boat floated; then its underwater status is cleared.
pub(super) fn float_the_boat(game: &Game<'_>, b: &mut Boat) -> bool {
    // Ignore if not being ridden by a player. Horizon seats the window's player first in its
    // boat, and physics run only for the boat it rides or for boats nobody rides (a server
    // leaves a guest's boat to that guest's client), so this is `getFirstPassenger()
    // instanceof Player` wherever the boat moves.
    if game.riding != Some(b.id) {
        return false;
    }
    let water = water_level_above(b.bounds(), b.velocity.y, |x, y, z| {
        game.fluid_height(x, y, z).map(|(h, _)| h)
    });
    match float_target(water, b.position.y) {
        Some(y) => {
            b.position.y = y;
            true
        }
        None => false,
    }
}

/// `floatTheBoat`'s arithmetic: where a boat at `y` moves under a water surface at
/// `water_level_above`, or None when the surface is too high to reach.
pub(super) fn float_target(water_level_above: f32, y: f64) -> Option<f64> {
    // `getWaterLevelAbove() - getBbHeight()` is float arithmetic, widened before `+ 0.101D`.
    let target_y = f64::from(water_level_above - HEIGHT as f32) + 0.101;
    let delta_y = target_y - y;
    // Can't go up if it's greater than half a block.
    if delta_y > 0.8 {
        return None;
    }
    // Lerp by 50% until the height difference is very small.
    let lerp = if delta_y > 0.01 { 0.5 } else { 1.0 };
    Some(y + delta_y * lerp)
}

/// `AbstractBoat.getWaterLevelAbove` (a float), with `water_height` = the water's
/// `FluidState.getHeight` at a block (None: no water). Horizon's own `water_level_above`
/// returns a double and drops `lastYd`; the hook follows vanilla. `last_yd` is the boat's
/// vertical motion: vanilla's `lastYd` is that motion before the last move's collisions,
/// which Horizon doesn't keep (it differs only after a vertical collision).
pub(super) fn water_level_above(
    bb: Aabb,
    last_yd: f64,
    mut water_height: impl FnMut(i32, i32, i32) -> Option<f32>,
) -> f32 {
    let (lo, hi) = (bb.min_position(), bb.max_position());
    let (min_x, max_x) = (lo.x.floor() as i32, hi.x.ceil() as i32);
    let (min_y, max_y) = (hi.y.floor() as i32, (hi.y - last_yd).ceil() as i32);
    let (min_z, max_z) = (lo.z.floor() as i32, hi.z.ceil() as i32);
    'rows: for y in min_y..max_y {
        let mut height = 0.0f32;
        for x in min_x..max_x {
            for z in min_z..max_z {
                if let Some(h) = water_height(x, y, z) {
                    height = height.max(h);
                }
                if height >= 1.0 {
                    continue 'rows;
                }
            }
        }
        if height < 1.0 {
            return y as f32 + height;
        }
    }
    max_y.wrapping_add(1) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_core::geometry::Vec3;

    /// `oracle/boat_hook/` (out of Git): a Java transcription of `floatTheBoat`'s arithmetic over
    /// these inputs, folded (FNV-1a over the result bits, -1 for "can't reach").
    const FLOAT_FOLD: u64 = 0xe540ec9bbacee5b8;
    const FLOAT_NONE: usize = 270;

    #[test]
    fn float_target_matches_java_bits() {
        let bases = [-64, 0, 62, 63, 319, 959, 4095, 1 << 20];
        let offsets = [
            -0.3, -0.01, 0.0, 0.001, 0.01, 0.0100001, 0.1, 0.4275, 0.5, 0.79, 0.8, 0.8000001, 1.0,
            1.5,
        ];
        let (mut fold, mut none) = (0xcbf29ce484222325u64, 0);
        for base in bases {
            for k in 0..=9 {
                let w = base as f32 + k as f32 / 9.0;
                for off in offsets {
                    let y = (f64::from(w - HEIGHT as f32) + 0.101) - off;
                    let r = float_target(w, y);
                    none += usize::from(r.is_none());
                    fold = (fold ^ r.map_or(u64::MAX, f64::to_bits)).wrapping_mul(0x100000001b3);
                }
            }
        }
        assert_eq!((fold, none), (FLOAT_FOLD, FLOAT_NONE));
    }

    #[test]
    fn water_levels_are_floats() {
        // Java's `pos.getY() + blockHeight` rounds to a float; Horizon's double sum does not.
        let bb = Aabb::new(Vec3::new(0.0, 62.4, 0.0), Vec3::new(1.375, 62.9625, 1.375));
        let level = water_level_above(bb, 0.0, |_, y, _| (y == 62).then_some(8.0 / 9.0));
        assert_eq!(level, 62.0f32 + 8.0 / 9.0);
        assert_ne!(f64::from(level), 62.0 + f64::from(8.0f32 / 9.0));
        // Under it the boat rises halfway, then all the way once within 0.01.
        let rest = f64::from(level - 0.5625) + 0.101;
        assert_eq!(float_target(level, rest), Some(rest));
        assert_eq!(float_target(level, rest - 0.005), Some(rest));
        let half = float_target(level, rest - 0.5).unwrap();
        assert!((half - (rest - 0.25)).abs() < 1e-9);
        assert_eq!(float_target(level, rest - 0.81), None);
    }

    /// The oracle's synthetic river: two water levels in a checkerboard of columns.
    fn water(x: i32, y: i32, z: i32) -> Option<f32> {
        let surface = 62 + (x.wrapping_mul(3).wrapping_add(z.wrapping_mul(5)) & 1);
        let level = 8 - (x.wrapping_add(2 * z) & 3);
        if y > surface || y < 50 {
            None
        } else if y < surface {
            Some(1.0)
        } else {
            Some(level as f32 / 9.0)
        }
    }

    /// The Java oracle's `getWaterLevelAbove` (vanilla 26.3 bytecode) over `water`.
    const JAVA_WATER_LEVELS: [u32; 168] = [
        0x42800000, 0x42800000, 0x42800000, 0x42800000, 0x42800000, 0x427f1c72, 0x42800000,
        0x42800000, 0x427f1c72, 0x427f1c72, 0x42800000, 0x427f1c72, 0x42800000, 0x427f1c72,
        0x427f1c72, 0x427f1c72, 0x42800000, 0x427f1c72, 0x427f1c72, 0x427f1c72, 0x427f1c72,
        0x427f1c72, 0x427f1c72, 0x427f1c72, 0x427f1c72, 0x427f1c72, 0x427f1c72, 0x427f1c72,
        0x427f1c72, 0x427f1c72, 0x42800000, 0x42800000, 0x42800000, 0x42800000, 0x42800000,
        0x42800000, 0x427c0000, 0x427c0000, 0x42800000, 0x42800000, 0x427c0000, 0x427f1c72,
        0x42800000, 0x42800000, 0x42800000, 0x42800000, 0x42800000, 0x427f8e39, 0x42800000,
        0x42800000, 0x427f8e39, 0x427f8e39, 0x42800000, 0x427f8e39, 0x42800000, 0x427f8e39,
        0x427f8e39, 0x427f8e39, 0x42800000, 0x427f8e39, 0x427f8e39, 0x427f8e39, 0x427f8e39,
        0x427f8e39, 0x427f8e39, 0x427f8e39, 0x427f8e39, 0x427f8e39, 0x427f8e39, 0x427f8e39,
        0x427f8e39, 0x427f8e39, 0x42800000, 0x42800000, 0x42800000, 0x42800000, 0x42800000,
        0x42800000, 0x427c0000, 0x427c0000, 0x42800000, 0x42800000, 0x427c0000, 0x427f8e39,
        0x42800000, 0x42800000, 0x42800000, 0x42800000, 0x42800000, 0x427f8e39, 0x42800000,
        0x42800000, 0x427f8e39, 0x427f8e39, 0x42800000, 0x427f8e39, 0x42800000, 0x427f8e39,
        0x427f8e39, 0x427f8e39, 0x42800000, 0x427f8e39, 0x427f8e39, 0x427f8e39, 0x427f8e39,
        0x427f8e39, 0x427f8e39, 0x427f8e39, 0x427f8e39, 0x427f8e39, 0x427f8e39, 0x427f8e39,
        0x427f8e39, 0x427f8e39, 0x42800000, 0x42800000, 0x42800000, 0x42800000, 0x42800000,
        0x42800000, 0x427c0000, 0x427c0000, 0x42800000, 0x42800000, 0x427c0000, 0x427f8e39,
        0x42800000, 0x42800000, 0x42800000, 0x42800000, 0x42800000, 0x427f1c72, 0x42800000,
        0x42800000, 0x427f1c72, 0x427f1c72, 0x42800000, 0x427f1c72, 0x42800000, 0x427f1c72,
        0x427f1c72, 0x427f1c72, 0x42800000, 0x427f1c72, 0x427f1c72, 0x427f1c72, 0x427f1c72,
        0x427f1c72, 0x427f1c72, 0x427f1c72, 0x427f1c72, 0x427f1c72, 0x427f1c72, 0x427f1c72,
        0x427f1c72, 0x427f1c72, 0x42800000, 0x42800000, 0x42800000, 0x42800000, 0x42800000,
        0x42800000, 0x427c0000, 0x427c0000, 0x42800000, 0x42800000, 0x427c0000, 0x427f1c72,
    ];

    #[test]
    fn water_level_above_matches_java_bits() {
        let mut c = 0;
        for x in [0.3, -1.7, 5.05, 12.5] {
            for top in [62.4, 62.9905, 63.0, 63.2, 63.95, 64.3, 61.99] {
                for last_yd in [0.0, -0.005, -0.02, -0.3, 0.05, -1.5] {
                    let z = x * 0.5 - 2.25;
                    let bb = Aabb::new(
                        Vec3::new(x, top - HEIGHT, z),
                        Vec3::new(x + 1.375, top, z + 1.375),
                    );
                    let r = water_level_above(bb, last_yd, water);
                    assert_eq!(r.to_bits(), JAVA_WATER_LEVELS[c], "case {c}");
                    c += 1;
                }
            }
        }
    }
}
