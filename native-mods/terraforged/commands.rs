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
            // The original's export is commented out; it only answers.
            game.native_feedback(Text::colored("Exported structure settings", GREEN));
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
        ["tf", "preview", rest @ ..] if rest.len() <= 2 => {
            let size = rest.first().map_or(Ok(400), |s| s.parse::<u32>());
            let zoom = rest.get(1).map_or(Ok(1.0), |z| z.parse::<f32>());
            let (Ok(size), Ok(zoom)) = (size, zoom) else {
                return false;
            };
            preview(game, player, size.clamp(16, 4096), zoom.max(0.0001));
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
        game.native_feedback(Text::colored(format!("Regen failed: {error}"), RED));
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
    game.native_feedback(Text::colored("Regen complete!", GREEN));
}

/// `ContinentPreview`'s window as a picture of the view around the player, in the world's
/// folder.
fn preview(game: &mut Game<'_>, player: &NativeServerPlayer, size: u32, zoom: f32) {
    use horizon_world::worldgen::generator::terraforged_preview::{Preview, SEED, View};
    let Some(world) = game.native_world_dir() else {
        return;
    };
    let view = View {
        width: size,
        height: size,
        zoom,
        x: player.position[0] as f32,
        y: player.position[2] as f32,
    };
    let written = Preview::new().and_then(|preview| {
        let pixels = preview.render(SEED, view);
        let path = world.join("terraforged-preview.png");
        let file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), size, size);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let bytes: Vec<u8> = pixels
            .iter()
            .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8])
            .collect();
        encoder
            .write_header()
            .and_then(|mut writer| writer.write_image_data(&bytes))
            .map_err(|e| e.to_string())?;
        Ok(path)
    });
    game.native_feedback(match written {
        Ok(path) => {
            // The window's corner label: the area shown, in blocks.
            let blocks = (size as f32 * zoom).floor();
            Text::colored(
                format!("Preview {blocks}x{blocks}: {}", path.display()),
                GREEN,
            )
        }
        Err(error) => Text::colored(format!("Preview failed: {error}"), RED),
    });
}
