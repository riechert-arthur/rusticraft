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
// `/locateterrain <terrain> [radius]`, `/tf export structures` and `/tf regen <radius>` (with its `Regenerator`), for
// operators (level 2), on the native command hooks.

use super::super::Game;
use super::super::native_server::{NativeCommand, NativeServerPlayer};
use horizon_client::save::terraforged_world_type::is_terraforged;
use horizon_client::text::{self, ClickEvent, Style, Text};
use horizon_world::worldgen::generator::{
    terraforged_current_biome_sampler::CurrentBiomeSampler,
    terraforged_generator_facade::{TerraForgedGenerator, TerraHeightmap, live_generator},
    terraforged_preview::{View, world_color},
    terraforged_util::{unpack_left, unpack_right},
};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};

/// `TFCommands.register`'s two roots (`/tf` has `export structures` and `regen`), `requires(hasPermission(2))`, and
/// Horizon's `/tf preview` (TerraForged's `Previewer` window as a picture: [`preview`]).
/// TerraForged registers them in every world (its `/locateterrain` then answers nothing where the level's generator
/// is not TerraForged's); here only a TerraForged world has them, so other worlds keep vanilla's command tree
/// (Arthur's choice for the pack and the boats).
/// Horizon's native commands have no quotable `string` argument and no suggestions: the terrain is a `word`
/// (TerraForged's terrain names are all words), and the radii's minimum of 1 is checked when they run.
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
    commands.push(NativeCommand::op(
        "tf preview [size:integer] [zoom:number]",
        2,
    ));
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
        ["tf", "regen", word] => {
            // `IntegerArgumentType.integer(1)`, checked here as for `/locateterrain`, with a maximum the Java
            // argument lacks (`MAX_REGEN_RADIUS`).
            let message = match word.parse::<i32>() {
                Ok(radius) if (1..=MAX_REGEN_RADIUS).contains(&radius) => {
                    regen(game, player, radius);
                    return true;
                }
                Ok(found) if found < 1 => Text::tr("argument.integer.low", &[&1, &found]),
                Ok(found) => Text::tr("argument.integer.big", &[&MAX_REGEN_RADIUS, &found]),
                Err(_) => Text::tr("parsing.int.invalid", &[word]),
            };
            let cursor = command.trim_end().len() - word.len();
            syntax_failure(game, command, cursor, message);
            true
        }
        ["tf", "preview", rest @ ..] if rest.len() <= 2 => {
            // Horizon's: `Previewer`'s window size (one side, in pixels) and zoom (blocks per pixel), checked as
            // Brigadier's `integer(16, 4096)` and `floatArg(0.0001)` would (its wheel keeps the zoom at 0.0001 or
            // more; Brigadier reads no NaN or infinity).
            let size = match rest.first().map(|word| (word, word.parse::<i32>())) {
                None => 400,
                Some((_, Ok(size))) if (16..=MAX_PREVIEW_SIZE).contains(&size) => size,
                Some((word, found)) => {
                    let message = match found {
                        Ok(found) if found < 16 => Text::tr("argument.integer.low", &[&16, &found]),
                        Ok(found) => Text::tr("argument.integer.big", &[&MAX_PREVIEW_SIZE, &found]),
                        Err(_) => Text::tr("parsing.int.invalid", &[word]),
                    };
                    syntax_failure(game, command, offset(command, word), message);
                    return true;
                }
            };
            let zoom = match rest.get(1).map(|word| (word, word.parse::<f32>())) {
                None => 1.0,
                Some((_, Ok(zoom))) if zoom.is_finite() && zoom >= MIN_PREVIEW_ZOOM => zoom,
                Some((word, found)) => {
                    let message = match found {
                        Ok(found) if found.is_finite() => {
                            Text::tr("argument.float.low", &[&MIN_PREVIEW_ZOOM, &found])
                        }
                        _ => Text::tr("parsing.float.invalid", &[word]),
                    };
                    syntax_failure(game, command, offset(command, word), message);
                    return true;
                }
            };
            preview(game, player, size as u32, zoom);
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

/// `/locateterrain` and `/tf preview` answers still being computed: the player's UUID and the answer's channel.
type Pending = Vec<(u128, Receiver<Text>)>;
static PENDING: Mutex<Pending> = Mutex::new(Vec::new());

/// Each server tick: sends the `/locateterrain` and `/tf preview` answers that are ready to their players (dropped
/// for a player who left).
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

/// The largest `/tf regen` radius: 32 chunks (65x65 chunks), vanilla's largest view distance. TerraForged's
/// `integer(1)` has no maximum: a large radius there holds the server thread for as long as it takes to queue
/// the deletes (a watchdog crash past a minute), and here it would allocate the whole area's keys at once.
const MAX_REGEN_RADIUS: i32 = 32;

/// `TFCommands.regen` + `Regenerator.regenerateChunks`: the chunks within `radius` of the player's chunk
/// (`(int) pos.x >> 4`, Java's truncating cast) are deleted from the save and from memory and generated again
/// by the level's own generator, as the streaming reaches them. Players in the level forget them and are sent
/// the new ones. The three answers are `sendSuccess(.., true)`: operators see them too.
///
/// Departures from Java, where Horizon's server differs:
/// - Java keeps the entities and POIs: only `region/` records are written as null, and the loaded entities stay
///   in the entity manager. Horizon loads entities only with a chunk read from the save and ticks them by ticket
///   (a missing chunk reads as air), so kept entities would fall through the area until it is generated again,
///   and those of unloaded chunks would be orphaned. The area's entities are deleted with it instead (memory,
///   `entities/` and `poi/`), players excepted.
/// - Java's `save(true)` writes every chunk first. Here the deletes are queued behind every earlier write, which
///   is what the flush is for; unsaved changes in the area are dropped with it.
/// - Nothing is deleted while a load or generation of the area is still running (`settle`): its result would
///   bring the old chunk back. It answers `Regen failed` instead.
/// - The ring at the radius comes back without features (ores, trees, plants): Horizon's streaming does not
///   decorate a proto chunk next to a finished one (it can't write into it), while Java's decoration writes into
///   the neighbours. The ring inside it lacks what those features would have spread into it.
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
    // Java's loops: dz outer, dx inner, int arithmetic.
    let keys: Vec<(i32, i32)> = (-radius..=radius)
        .flat_map(|dz| {
            (-radius..=radius).map(move |dx| (centre.0.wrapping_add(dx), centre.1.wrapping_add(dz)))
        })
        .collect();
    let set: HashSet<(i32, i32)> = keys.iter().copied().collect();
    game.feedback(italic("Deleting chunks"));
    // `chunkSource.save(true)` + `flushWorker()`: the decorations and saves in flight finish first.
    game.streaming.save(&mut game.world, true);
    let failed = |game: &mut Game<'_>, error: &str| {
        game.command_failure(Text::literal(format!("Regen failed: {error}")));
    };
    if !horizon_client::streaming::terraforged_regen::settle(&mut game.streaming, &set) {
        failed(game, "chunk generation is busy, try again");
        return;
    }
    if let Err(error) = horizon_client::save::terraforged_regen::delete_chunks(&game.save, &keys) {
        // Nothing was deleted: the area stays as it is.
        failed(game, &error.to_string());
        return;
    }
    let data = horizon_client::save::terraforged_regen::delete_chunk_data(&game.save, &keys);
    // The save no longer has the area: memory follows, whatever happened to its entities' records.
    horizon_client::streaming::terraforged_regen::discard(&mut game.streaming, &set);
    // Entities read with a column that loaded since the last tick wait in `loaded_entities`: theirs go too.
    game.world
        .loaded_entities
        .retain(|(key, _)| !set.contains(key));
    let _ = game.entity_chunks(&set, true);
    for &(x, z) in &keys {
        game.world.modified.remove(&(x, z));
        game.world.inhabited.remove(&(x, z));
        game.streaming.forget(&mut game.world, x, z);
    }
    game.world
        .block_entities
        .retain(|p, _| !set.contains(&(p.0 >> 4, p.2 >> 4)));
    game.world
        .block_names
        .retain(|p, _| !set.contains(&(p.0 >> 4, p.2 >> 4)));
    if let Err(error) = data {
        failed(game, &error.to_string());
    }
    game.feedback(italic("Regenerating chunks"));
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
            // The view is computed again: the area's chunks are queued as they come back.
            guest.center = None;
        }
        if let Some(far) = hub.far.as_mut() {
            for &(x, z) in &keys {
                far.changed(x, z);
            }
        }
    }
    game.feedback(Text::colored("Regen complete!", text::GREEN));
}

/// Where `word` (a slice of `line`, from `split_whitespace`) starts in `line`.
fn offset(line: &str, word: &str) -> usize {
    word.as_ptr() as usize - line.as_ptr() as usize
}

/// `/tf preview`'s largest window side, in pixels.
const MAX_PREVIEW_SIZE: i32 = 4096;
/// `Previewer`'s smallest zoom (`Math.max(0.0001f, ..)` on the mouse wheel).
const MIN_PREVIEW_ZOOM: f32 = 0.0001;
/// Set while a preview renders: one at a time, since a render takes every processor but one.
static PREVIEWING: AtomicBool = AtomicBool::new(false);

/// Horizon's in-game form of TerraForged's `Previewer` window (a developer tool that the game never opens): the view
/// centred on the player, of the level's own generator (never a second one) at its runtime seed, written to
/// `terraforged-preview.png` in the world's folder (see `terraforged_preview`). A level that does not generate with
/// TerraForged (the Nether and the End) has nothing to show. The picture renders on its own thread, never on the
/// server's, and its answer reaches the player at a later server tick ([`deliver`]).
fn preview(game: &mut Game<'_>, player: &NativeServerPlayer, size: u32, zoom: f32) {
    let Some(world) = game.native_world_dir() else {
        return;
    };
    let Some(generator) = live_generator(game.level.seed, &player.dimension) else {
        game.command_failure(Text::literal(
            "This dimension does not generate with TerraForged",
        ));
        return;
    };
    if PREVIEWING.swap(true, Ordering::AcqRel) {
        game.command_failure(Text::literal("A TerraForged preview is already rendering"));
        return;
    }
    let view = View {
        width: size,
        height: size,
        zoom,
        x: player.position[0] as f32,
        y: player.position[2] as f32,
    };
    let (send, receive) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("TerraForged preview".to_owned())
        .spawn(move || {
            // Cleared on any exit, a panic included.
            struct Done;
            impl Drop for Done {
                fn drop(&mut self) {
                    PREVIEWING.store(false, Ordering::Release);
                }
            }
            let _done = Done;
            let line = match write_preview(&generator, &world, view) {
                Ok(path) => Text::colored(
                    format!("Preview {}: {}", view.label(), path.display()),
                    text::GREEN,
                ),
                Err(error) => Text::colored(format!("Preview failed: {error}"), text::RED),
            };
            drop(send.send(line));
        });
    match spawned {
        Ok(_) => PENDING
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((player.uuid, receive)),
        Err(error) => {
            PREVIEWING.store(false, Ordering::Release);
            game.command_failure(Text::literal(format!("Preview failed: {error}")));
        }
    }
}

/// Renders `view` of `generator`'s world and writes it as an RGB PNG beside the level.
fn write_preview(
    generator: &TerraForgedGenerator,
    world: &std::path::Path,
    view: View,
) -> Result<std::path::PathBuf, String> {
    let sampler = CurrentBiomeSampler::new(generator.noise_generator());
    let seed = generator.seed();
    let pixels = view.render(|x, y| world_color(&sampler, seed, x, y));
    let bytes: Vec<u8> = pixels
        .iter()
        .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8])
        .collect();
    let path = world.join("terraforged-preview.png");
    let file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), view.width, view.height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut writer| {
            writer.write_image_data(&bytes)?;
            writer.finish()
        })
        .map_err(|e| e.to_string())?;
    Ok(path)
}
