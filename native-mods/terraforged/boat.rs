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

// `BoatHook.floatTheBoat`: a player's boat under a slightly higher water surface (a
// river's stepped levels) rises toward it instead of sinking.

use super::{Boat, Game, HEIGHT};

/// Whether the boat floated; then its underwater status is cleared.
pub(super) fn float_the_boat(game: &Game<'_>, b: &mut Boat) -> bool {
    // Ignore if not being ridden by a player.
    if game.riding != Some(b.id) {
        return false;
    }
    let target_y = game.water_level_above(b) - HEIGHT + 0.101;
    let delta_y = target_y - b.position.y;
    // Can't go up if it's greater than half a block.
    if delta_y > 0.8 {
        return false;
    }
    // Lerp by 50% until the height difference is very small.
    let lerp = if delta_y > 0.01 { 0.5 } else { 1.0 };
    b.position.y += delta_y * lerp;
    true
}
