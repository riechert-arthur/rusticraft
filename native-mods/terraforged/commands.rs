/*
 * MIT License
 *
 * Copyright (c) 2021 TerraForged
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

// TerraForged 6dd607eb's `TFCommands`
// (https://github.com/TerraForged/TerraForged/blob/6dd607ebfd41a3b7274e090b1f2cf565d53d1d44/src/main/java/com/terraforged/mod/command/TFCommands.java):
// `/locateterrain <terrain> [radius]` and `/tf export structures`, for operators (level 2), on the native command
// hooks. `/tf regen <radius>` (which regenerates chunks under a live server) is not ported.

use super::super::Game;
use super::super::native_server::{NativeCommand, NativeServerPlayer};
use horizon_client::save::terraforged_world_type::is_terraforged;
use horizon_client::text::{self, ClickEvent, Style, Text};
use horizon_world::worldgen::generator::{
    terraforged_generator_facade::{TerraForgedGenerator, TerraHeightmap, live_generator},
    terraforged_util::{unpack_left, unpack_right},
};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};

/// `TFCommands.register`'s two roots, `requires(hasPermission(2))`. TerraForged registers them in every world (its
/// `/locateterrain` then answers nothing where the level's generator is not TerraForged's); here only a TerraForged
/// world has them, so other worlds keep vanilla's command tree (Arthur's choice for the pack and the boats).
/// Horizon's native commands have no quotable `string` argument and no suggestions: the terrain is a `word`
/// (TerraForged's terrain names are all words), and the radius' minimum of 1 is checked when it runs.
pub(in crate::game) fn register(game: &Game<'_>, commands: &mut Vec<NativeCommand>) {
    if !game
        .native_world_dir()
        .is_some_and(|world| is_terraforged(&world))
    {
        return;
    }
    commands.push(NativeCommand::op(
        "locateterrain <terrain:word> [radius:integer]",
        2,
    ));
    commands.push(NativeCommand::op("tf export structures", 2));
    commands.push(NativeCommand::op("tf regen <radius:integer>", 2));
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
            // `export`: its structure export is commented out in TerraForged, so it writes nothing and only answers
            // (`sendSuccess(.., false)`).
            game.feedback_unbroadcast(Text::colored("Exported structure settings", text::GREEN));
            true
        }
        ["tf", "regen", radius] => {
            let Ok(radius) = radius.parse::<i32>() else {
                return false;
            };
            if radius < 1 {
                return false;
            }
            regen(game, player, radius);
            true
        }
        ["locateterrain", terrain, rest @ ..] if rest.len() <= 1 => {
            let radius = match rest.first() {
                // `IntegerArgumentType.integer(1)`, which the tree sent to clients cannot carry.
                Some(word) => match word.parse::<i32>() {
                    Ok(radius) if radius >= 1 => radius,
                    found => {
                        let message = match found {
                            Ok(found) => Text::tr("argument.integer.low", &[&1, &found]),
                            Err(_) => Text::tr("parsing.int.invalid", &[word]),
                        };
                        let cursor = command.trim_end().len() - word.len();
                        syntax_failure(game, command, cursor, message);
                        return true;
                    }
                },
                None => 1,
            };
            // `StringArgumentType.string()`: a quoted name reads without its quotes.
            let name = terrain
                .strip_prefix('"')
                .and_then(|quoted| quoted.strip_suffix('"'))
                .unwrap_or(terrain);
            locate(game, player, name, radius);
            true
        }
        _ => false,
    }
}

/// `TFCommands.locate`. Nothing where the player's level does not generate with TerraForged
/// (`GeneratorPreset.getGenerator(level) == null`: the Nether and the End). The answers go to the player
/// (`sendSystemMessage`); a failure inside the search is Brigadier's `command.failed`.
///
/// The search runs on the server thread as in Java (`NoiseGenerator.find` takes microseconds), but the teleport's
/// height does not: [`TerraForgedGenerator::base_height`] reads the eroded height, which builds an erosion region
/// (0.25 to 1.2 s in an optimized build), so it runs on its own thread and its answer reaches the player at a later
/// server tick ([`deliver`]).
fn locate(game: &mut Game<'_>, player: &NativeServerPlayer, name: &str, radius: i32) {
    let Some(generator) = live_generator(game.level.seed, &player.dimension) else {
        return;
    };
    let terrains = Arc::clone(&generator.noise_generator().continent().context().terrain);
    let Some(terrain) = terrains.get_by_name(name) else {
        game.native_feedback(Text::colored(format!("Invalid terrain: {name}"), text::RED));
        return;
    };
    let unable = Text::colored(format!("Unable to locate terrain: {name}"), text::RED);
    // `player.blockPosition()`.
    let at = player.position.map(|v| v.floor() as i32);
    let max_radius = 100.min(radius.wrapping_add(50));
    if radius > max_radius {
        // Above 100 the minimum radius passes the maximum, and TerraForged's spiral never leaves its first cell:
        // Java's search then never ends unless that one cell happens to match (a hang the server's watchdog ends).
        // The port answers as a miss instead.
        game.native_feedback(unable);
        return;
    }
    let found = generator.noise_generator().find(
        generator.seed(),
        at[0],
        at[2],
        radius,
        max_radius,
        &terrain,
    );
    let position = match found {
        Ok(0) => return game.native_feedback(unable),
        Ok(position) => position,
        Err(cause) => return game.command_failure(failed(cause)),
    };
    let (x, z) = (unpack_left(position), unpack_right(position));
    let shown = terrain.name().to_owned();
    let (send, receive) = mpsc::channel();
    let worker = Arc::clone(&generator);
    let spawned = std::thread::Builder::new()
        .name("TerraForged locateterrain".to_owned())
        .spawn(move || drop(send.send(answer(&worker, at, x, z, &shown))));
    match spawned {
        Ok(_) => PENDING
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((player.uuid, receive)),
        Err(_) => game.native_feedback(answer(&generator, at, x, z, terrain.name())),
    }
}

/// The answer to a search that found `terrain` at `(x, z)`: `getFirstFreeHeight(x, z, MOTION_BLOCKING, ..)` (the
/// generator's `getBaseHeight`), then the teleport message.
fn answer(generator: &TerraForgedGenerator, at: [i32; 3], x: i32, z: i32, terrain: &str) -> Text {
    match generator.base_height(x, z, TerraHeightmap::MotionBlocking) {
        Ok(y) => teleport_message(at, x, y, z, terrain),
        Err(cause) => Text::empty().append(failed(cause)).with_color(text::RED),
    }
}

/// `/locateterrain` answers still being computed: the player's UUID and the answer's channel.
type Pending = Vec<(u128, Receiver<Text>)>;
static PENDING: Mutex<Pending> = Mutex::new(Vec::new());

/// Each server tick: sends the `/locateterrain` answers that are ready to their players (dropped for a player who
/// left).
pub(in crate::game) fn deliver(game: &mut Game<'_>) {
    let mut pending = PENDING.lock().unwrap_or_else(PoisonError::into_inner);
    if pending.is_empty() {
        return;
    }
    pending.retain(|(uuid, answer)| match answer.try_recv() {
        Ok(line) => {
            game.native_tell(*uuid, line);
            false
        }
        Err(TryRecvError::Empty) => true,
        Err(TryRecvError::Disconnected) => false,
    });
}

/// `createTerrainTeleportMessage(pos, x, y, z, terrain)`. The distance is `Math.sqrt(pos.distToCenterSqr(x, y, z))`
/// printed as `%.1f` (in the root locale's form; Java's uses the server's default locale).
fn teleport_message(at: [i32; 3], x: i32, y: i32, z: i32, terrain: &str) -> Text {
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
                Text::colored("Location: ", text::GREEN)
                    .append(Text::colored(format!("{x};{y};{z}"), text::YELLOW)),
            )),
            ..Style::color(text::YELLOW)
        },
    );
    Text::colored("Found terrain: ", text::GREEN)
        .append(Text::colored(terrain.to_owned(), text::YELLOW))
        .append(Text::colored(" Distance: ", text::GREEN))
        .append(Text::colored(format!("{distance:.1}"), text::YELLOW))
        .append(Text::colored(". ", text::GREEN))
        .append(teleport)
}

/// `Commands.performCommand`'s answer to an exception: `command.failed`, its message on hover.
fn failed(cause: &str) -> Text {
    Text::translate("command.failed", &[]).with_style(&Style {
        hover: Some(Box::new(Text::literal(cause))),
        ..Style::default()
    })
}

/// `Commands.performCommand`'s answer to a `CommandSyntaxException` at `cursor` of `input`: the message, then the
/// input's last 10 characters before the cursor and the rest underlined, with `<--[HERE]`.
fn syntax_failure(game: &mut Game<'_>, input: &str, cursor: usize, message: Text) {
    game.command_failure(message);
    let mut context = Text::empty();
    if cursor > 10 {
        context.push("...");
    }
    let start = (0..=cursor.saturating_sub(10))
        .rev()
        .find(|&i| input.is_char_boundary(i))
        .unwrap_or(0);
    context.push(&input[start..cursor]);
    if cursor < input.len() {
        context.push(
            Text::literal(&input[cursor..])
                .with_color(text::RED)
                .underlined(),
        );
    }
    context.push(
        Text::translate("command.context.here", &[])
            .with_color(text::RED)
            .italic(),
    );
    let suggest = Style {
        click: Some(ClickEvent::SuggestCommand(format!("/{input}"))),
        ..Style::default()
    };
    game.command_failure(context.with_color(text::GRAY).with_style(&suggest));
}

/// `Regenerator.regenerateChunks`: the chunks within `radius` of the player's chunk are
/// saved away, deleted from memory and disk, and generated again; players get them anew.
fn regen(game: &mut Game<'_>, player: &NativeServerPlayer, radius: i32) {
    let italic = |text: &str| {
        Text::styled(
            text,
            Style {
                italic: Some(true),
                ..Style::default()
            },
        )
    };
    let centre = (
        (player.position[0] as i32) >> 4,
        (player.position[2] as i32) >> 4,
    );
    let keys: Vec<(i32, i32)> = (-radius..=radius)
        .flat_map(|dz| (-radius..=radius).map(move |dx| (centre.0 + dx, centre.1 + dz)))
        .collect();
    game.native_feedback(italic("Deleting chunks"));
    // Flush out any chunks queued to be written, and the jobs still holding protos.
    game.streaming.save(&mut game.world, true);
    horizon_client::streaming::terraforged_regen::discard(&mut game.streaming, &keys);
    let set: std::collections::HashSet<(i32, i32)> = keys.iter().copied().collect();
    let _ = game.entity_chunks(&set, true);
    for &(x, z) in &keys {
        game.world.modified.remove(&(x, z));
        game.streaming.forget(&mut game.world, x, z);
    }
    game.world
        .block_entities
        .retain(|p, _| !set.contains(&(p.0 >> 4, p.2 >> 4)));
    game.world
        .block_names
        .retain(|p, _| !set.contains(&(p.0 >> 4, p.2 >> 4)));
    if let Err(error) = horizon_client::save::terraforged_regen::delete_chunks(&game.save, &keys) {
        game.native_feedback(Text::colored(format!("Regen failed: {error}"), text::RED));
        return;
    }
    game.native_feedback(italic("Regenerating chunks"));
    if let Some(host) = &mut game.lan {
        let hub = &mut host.hub;
        for index in 0..hub.guests.len() {
            if !hub.here(&hub.guests[index]) {
                continue;
            }
            let guest = &mut hub.guests[index];
            for key in &keys {
                if guest.sent_chunks.remove(key) {
                    guest.send(
                        &horizon_protocol::packets::play::Clientbound::ForgetLevelChunk(
                            horizon_protocol::packets::play::ForgetLevelChunk {
                                pos: [key.0, key.1],
                            },
                        ),
                    );
                }
            }
            guest.center = None;
        }
        if let Some(far) = hub.far.as_mut() {
            for &(x, z) in &keys {
                far.changed(x, z);
            }
        }
    }
    game.native_feedback(Text::colored("Regen complete!", text::GREEN));
}
