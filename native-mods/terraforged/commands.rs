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

// `TFCommands`: `/locateterrain <terrain> [radius]` and `/tf export structures` for
// operators (level 2), on the native command hooks.

use super::super::Game;
use super::super::native_server::{NativeCommand, NativeServerPlayer};
use horizon_client::text::{ClickEvent, Style, Text};
use horizon_world::worldgen::generator::{
    terraforged_engine::TerrainRegistry,
    terraforged_generator_facade::{TerraForgedGenerator, TerraHeightmap},
    terraforged_generator_preset::{build_with_terrains, terrain_levels},
    terraforged_util::{unpack_left, unpack_right},
    terraforged_world_preset::preset_json,
};
use std::sync::{Arc, Mutex};

const GREEN: u32 = 0x55_ff_55;
const YELLOW: u32 = 0xff_ff_55;
const RED: u32 = 0xff_55_55;

pub(in crate::game) fn register(commands: &mut Vec<NativeCommand>) {
    commands.push(NativeCommand::op(
        "locateterrain <terrain:word> [radius:integer]",
        2,
    ));
    commands.push(NativeCommand::op("tf export structures", 2));
}

/// A TerraForged command: answers and returns true, or leaves the line to others.
pub(in crate::game) fn perform(
    game: &mut Game<'_>,
    player: &NativeServerPlayer,
    command: &str,
) -> bool {
    let words: Vec<&str> = command.split_whitespace().collect();
    match words.as_slice() {
        ["tf", "export", "structures"] => {
            // The original's export is commented out; it only answers.
            game.native_feedback(Text::colored("Exported structure settings", GREEN));
            true
        }
        ["locateterrain", terrain, rest @ ..] if rest.len() <= 1 => {
            let radius = match rest.first() {
                Some(radius) => match radius.parse::<i32>() {
                    Ok(radius) if radius >= 1 => radius,
                    _ => return false,
                },
                None => 1,
            };
            if let Some(line) = locate(game, player, terrain.trim_matches('"'), radius) {
                game.native_feedback(line);
            }
            true
        }
        _ => false,
    }
}

/// `TFCommands.locate`; nothing outside a TerraForged world.
fn locate(game: &Game<'_>, player: &NativeServerPlayer, name: &str, radius: i32) -> Option<Text> {
    let world = game.native_world_dir()?;
    if !horizon_client::save::terraforged_world_type::is_terraforged(&world) {
        return None;
    }
    let built = generator(game.level.seed)?;
    let (generator, terrains) = (&built.0, &built.1);
    let Some(terrain) = terrains.get_by_name(name) else {
        return Some(Text::colored(format!("Invalid terrain: {name}"), RED));
    };
    let at = player.position.map(|v| v.floor() as i32);
    let max_radius = 100.min(radius + 50);
    let pos = generator.noise_generator().find(
        game.level.seed as i32,
        at[0],
        at[2],
        radius,
        max_radius,
        &terrain,
    );
    if pos == 0 {
        return Some(Text::colored(
            format!("Unable to locate terrain: {name}"),
            RED,
        ));
    }
    let (x, z) = (unpack_left(pos), unpack_right(pos));
    let y = generator.base_height(x, z, TerraHeightmap::MotionBlocking);
    // Vec3i.distToCenterSqr from the player's block.
    let d = [
        f64::from(at[0]) + 0.5 - f64::from(x),
        f64::from(at[1]) + 0.5 - f64::from(y),
        f64::from(at[2]) + 0.5 - f64::from(z),
    ];
    let distance = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let teleport = Text::styled(
        "Teleport",
        Style {
            underlined: Some(true),
            click: Some(ClickEvent::RunCommand(format!("/tp {x} {y} {z}"))),
            hover: Some(Box::new(
                Text::colored("Location: ", GREEN)
                    .append(Text::colored(format!("{x};{y};{z}"), YELLOW)),
            )),
            ..Style::color(YELLOW)
        },
    );
    Some(
        Text::colored("Found terrain: ", GREEN)
            .append(Text::colored(terrain.name().to_owned(), YELLOW))
            .append(Text::colored(" Distance: ", GREEN))
            .append(Text::colored(format!("{distance:.1}"), YELLOW))
            .append(Text::colored(". ", GREEN))
            .append(teleport),
    )
}

type Built = Arc<(TerraForgedGenerator, Arc<TerrainRegistry>)>;

/// The world's generator, built once per seed.
fn generator(seed: i64) -> Option<Built> {
    static CACHE: Mutex<Option<(i64, Built)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((cached, built)) = cache.as_ref()
        && *cached == seed
    {
        return Some(Arc::clone(built));
    }
    let preset: serde_json::Value = serde_json::from_str(preset_json()).ok()?;
    let levels =
        terrain_levels(&preset["dimensions"]["minecraft:overworld"]["generator"]["levels"]).ok()?;
    let (generator, terrains) = build_with_terrains(levels).ok()?;
    generator.capture_seed(seed);
    let built = Arc::new((generator, terrains));
    *cache = Some((seed, Arc::clone(&built)));
    Some(built)
}
