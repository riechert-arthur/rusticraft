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

// Adapted from TerraForged Reimagined (https://github.com/Romoslayer/TerraForged-Reimagined, a24d450):
// `TerrainNoise.CODEC` and `NoiseCodec.CODEC` (`SuperCodec`, `DataUtil.toData`, Cereal's `DataSpec`/`SubSpec`/
// `DataValue` and the noise specs `NoiseSpec` registers), as Minecraft 26.3 reads a `worldgen/terrain_noise` file.
// MIT License, Copyright (c) 2020 TerraForged, Copyright (c) 2026 Romoslayer; see `LICENSE-TERRAFORGED-REIMAGINED`.
// The `Valley` spec is not Reimagined's (a24d450's `NoiseCodec` dropped it): it is Engine 0.3.0's erosion
// `Ridge.spec()`/`Ridge.create`, which TerraForged 6dd607eb's `NoiseCodec` registers (MIT, TerraForged; see
// `LICENSE-TERRAFORGED`).

use super::terraforged::{
    CurveFunction, DistanceFunction, Domain, EdgeFunction, ErosionMode, ErosionRidge,
    Interpolation, Module, Source, SourceConfig, SourceKind,
};
use super::terraforged_engine::Terrain;
use super::terraforged_terrain_blender::TerrainNoise;
use serde_json::value::RawValue;
use std::collections::HashMap;

#[derive(Debug)]
pub struct TerrainAssetError(String);

impl std::fmt::Display for TerrainAssetError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for TerrainAssetError {}

impl From<serde_json::Error> for TerrainAssetError {
    fn from(error: serde_json::Error) -> Self {
        Self(error.to_string())
    }
}

fn error<T>(message: impl Into<String>) -> Result<T, TerrainAssetError> {
    Err(TerrainAssetError(message.into()))
}

/// Every spec name Reimagined's `NoiseSpec` registers (`MidPointCurve` and `SCurve` are only sub-specs of
/// `CurveFunc`; `AddWarp` and `CompoundWarp` register under `CumulativeWarp`'s name), plus Engine 0.3.0's `Valley`,
/// which 6dd607eb's `NoiseCodec` registers.
const SPEC_NAMES: &str = "Abs Add AdvTerrace Alpha Base Bias Billow Blend Boost Cache Cell CellEdge Clamp Const Cubic CumulativeWarp Curve DirectionWarp DomainWarp Freq Grad Interpolation Invert LegacyTerrace Line Map Max Min Modulate Mult MultiBlend Perlin Perlin2 Pow PowCurve Rand Ridge Scale Select Simplex Simplex2 SimplexRidge Sin Steps Sub Terrace Threshold Valley VariBlend VariCurve Vec2f Vec2i Warp";

fn registered(name: &str) -> bool {
    SPEC_NAMES.split(' ').any(|spec| spec == name)
}

/// A terrain-noise asset (`data/<namespace>/worldgen/terrain_noise/*.json`) as Reimagined's `TerrainNoise.CODEC`
/// reads it from Minecraft's strict JSON:
///
/// - `type` (a `TerrainType` registry reference): a string, parsed as `Identifier.read` does (no namespace, or a
///   leading `:`, means `minecraft`); `resolve` gets the full `namespace:path` and does the registry lookup. Inline
///   `TerrainType` objects are not supported (an error).
/// - `weight` (`Codec.FLOAT`): a JSON number, read with `Float.parseFloat` on its text (the noise's numbers are not:
///   see [`decode_noise`]).
/// - `noise` (`NoiseCodec.CODEC`): a Cereal graph; see [`decode_noise`].
///
/// The result goes through `new TerrainNoise(...)`, which raises a noise whose minimum is below 5/255.
pub fn decode_terrain_asset(
    json: &str,
    mut resolve: impl FnMut(&str) -> Option<Terrain>,
) -> Result<TerrainNoise, TerrainAssetError> {
    let fields: HashMap<String, &RawValue> = serde_json::from_str(json)?;
    let field = |name: &str| {
        fields
            .get(name)
            .map(|raw| raw.get())
            .ok_or_else(|| TerrainAssetError(format!("missing field {name:?}")))
    };
    let Ok(name) = serde_json::from_str::<String>(field("type")?) else {
        return error("field \"type\" must be a string");
    };
    let Some(id) = identifier(&name) else {
        return error(format!("invalid identifier {name:?}"));
    };
    // Gson's `LazilyParsedNumber.floatValue`: `Float.parseFloat` on the text, one rounding.
    let Some(weight) = codec_float(field("weight")?) else {
        return error("field \"weight\" must be a number");
    };
    let noise = decode_noise(field("noise")?)?;
    let terrain =
        resolve(&id).ok_or_else(|| TerrainAssetError(format!("unknown terrain type {id:?}")))?;
    Ok(TerrainNoise::new(terrain, weight, noise))
}

/// `NoiseCodec.CODEC`'s decode of one JSON value (`Cereal.deserialize(DataUtil.toData(json).asObj(), Module.class,
/// Context.NONE)`), with Cereal's rules: a missing or non-numeric scalar takes its spec's default (`asNum()` reads
/// any non-number as 0), numbers are first rebuilt by DFU as a `long`, `float` or `double` (see `Number`) and read
/// with Java's `intValue`/`floatValue`/`doubleValue`; enums must name a constant exactly; a missing or non-object child is an error; a
/// combiner's non-object entries are skipped. The specs' own quirks are kept: `Bias` reads its bias with `asFloat`
/// (so a module there, as Reimagined's assets have, reads as 0), `CellEdge` ignores `distance` (always 1), `Boost`
/// runs at least one iteration, and sources read `cell_func`/`edge_func`/`dist_func`/`source` when present (an
/// invalid one is an error even where unused). JSON `null` anywhere in the graph is an error (`DataUtil.toData`).
///
/// Supported specs: the octave sources (`Perlin`, `Perlin2`, `Simplex`, `Simplex2`, `Cubic`, `Ridge`, `Billow`,
/// `SimplexRidge`), `CellEdge`, `Const`, `Abs`, `Alpha`, `Bias`, `Boost`, `Clamp`, `Invert`, `Map`, `Pow`, `PowCurve`,
/// `Scale`, `Warp` (with `DomainWarp` or `DirectionWarp`), `Blend`, `Terrace`, `AdvTerrace`, `Steps` (with an
/// `Interpolation` curve), `Add`, `Mult`, `Max`, `Min` and `Sub`: every node the 14 Reimagined terrain assets and
/// the 5 cave assets (`cave_assets.rs`) use;
/// and Engine 0.3.0's erosion `Valley` (registered by 6dd607eb, whose generated mountain assets use it; Reimagined
/// a24d450 rejects it). The other registered specs, which Java reads, return an "unsupported" error here.
pub fn decode_noise(json: &str) -> Result<Module, TerrainAssetError> {
    match data_of(json)? {
        Data::Obj(object) => module(&object),
        _ => error("noise must be an object"),
    }
}

/// Cereal's `DataValue` tree of a JSON value (`DataUtil.toData` after `JsonOps.convertTo`); `vegetation.rs` reads
/// its viability specs from it.
pub(super) enum Data {
    Obj(Obj),
    List(Vec<Data>),
    Num(Number),
    Str(String),
    Bool,
}

/// A `DataObject`: its spec name (the `type` key, `""` without one) and its other fields.
pub(super) struct Obj {
    pub(super) kind: String,
    fields: HashMap<String, Data>,
}

pub(super) fn data_of(json: &str) -> Result<Data, TerrainAssetError> {
    data(serde_json::from_str(json)?)
}

/// `DataUtil.toData`; a duplicate key keeps its last value, as Gson's `JsonObject` does.
fn data(raw: &RawValue) -> Result<Data, TerrainAssetError> {
    let text = raw.get();
    Ok(match text.as_bytes().first() {
        Some(b'{') => {
            let mut fields: HashMap<String, &RawValue> = serde_json::from_str(text)?;
            let kind = match fields.remove("type") {
                Some(raw) => type_name(raw)?,
                None => String::new(),
            };
            let fields = fields
                .into_iter()
                .map(|(key, raw)| Ok((key, data(raw)?)))
                .collect::<Result<_, TerrainAssetError>>()?;
            Data::Obj(Obj { kind, fields })
        }
        Some(b'[') => Data::List(
            serde_json::from_str::<Vec<&RawValue>>(text)?
                .into_iter()
                .map(data)
                .collect::<Result<_, _>>()?,
        ),
        Some(b'"') => Data::Str(serde_json::from_str(text)?),
        Some(b't' | b'f') => Data::Bool,
        Some(b'n') => return error("Unsupported data type: null"),
        _ => Data::Num(Number::parse(text)?),
    })
}

/// Gson's `getAsString` of the `type` value: a string, a number's text, a boolean, or an array's single element.
fn type_name(raw: &RawValue) -> Result<String, TerrainAssetError> {
    let text = raw.get();
    match text.as_bytes().first() {
        Some(b'"') => Ok(serde_json::from_str(text)?),
        Some(b'[') => match serde_json::from_str::<Vec<&RawValue>>(text)?.as_slice() {
            [single] => type_name(single),
            _ => error("type must be a single value"),
        },
        Some(b'{' | b'n') => error("type must be a primitive"),
        _ => Ok(text.to_owned()),
    }
}

/// `Codec.FLOAT` over Minecraft's strict JSON: a JSON number (not a string or a boolean) read with Gson's
/// `LazilyParsedNumber.floatValue`, `Float.parseFloat` on its text (one rounding). `None` for anything else.
pub(super) fn codec_float(raw: &str) -> Option<f32> {
    raw.starts_with(|c: char| c == '-' || c.is_ascii_digit())
        .then(|| raw.parse().unwrap_or(0.0))
}

/// `Identifier.read` (26.3): `namespace:path` split at the first `:`; no `:`, or a leading one, means `minecraft`.
/// The namespace allows `a-z0-9_.-`, the path also `/`; either may be empty.
pub(super) fn identifier(name: &str) -> Option<String> {
    let (namespace, path) = match name.find(':') {
        Some(0) => ("minecraft", &name[1..]),
        Some(colon) => (&name[..colon], &name[colon + 1..]),
        None => ("minecraft", name),
    };
    let allowed = |c: char, slash: bool| {
        matches!(c, 'a'..='z' | '0'..='9' | '_' | '-' | '.') || (slash && c == '/')
    };
    (namespace.chars().all(|c| allowed(c, false)) && path.chars().all(|c| allowed(c, true)))
        .then(|| format!("{namespace}:{path}"))
}

/// A JSON number as DFU's `JsonOps.convertTo` rebuilds it before Cereal sees it (`SuperCodec` converts the whole
/// tree): Gson's `getAsBigDecimal` (an error past Gson's limits: over 10,000 characters, or a scale of 10,000 or
/// more either way), then a `long` when the value is a whole number in range (`longValueExact`), else its `double`,
/// kept as a `float` when that narrows exactly.
#[derive(Clone, Copy)]
pub(super) enum Number {
    Long(i64),
    Float(f32),
    Double(f64),
}

impl Number {
    fn parse(text: &str) -> Result<Self, TerrainAssetError> {
        let invalid = || TerrainAssetError(format!("number {text:?} is outside Gson's limits"));
        let (negative, unsigned) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
            Some(e) => (
                &unsigned[..e],
                unsigned[e + 1..].parse::<i64>().map_err(|_| invalid())?,
            ),
            None => (unsigned, 0),
        };
        let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        let scale = (fraction.len() as i64).saturating_sub(exponent);
        if text.len() > 10_000 || scale.unsigned_abs() >= 10_000 {
            return Err(invalid());
        }
        // The value is `digits * 10^-scale`; it is whole when the digits it drops are zeros.
        let digits = format!("{whole}{fraction}");
        let digits = digits.trim_start_matches('0');
        let kept = digits.len() as i64 - scale;
        let whole_number = digits.is_empty()
            || (kept >= 0
                && digits
                    .bytes()
                    .skip(kept as usize)
                    .all(|digit| digit == b'0'));
        if whole_number && kept <= 19 {
            let mut value = 0i128;
            for index in 0..kept.max(0) as usize {
                let digit = digits.as_bytes().get(index).map_or(0, |digit| digit - b'0');
                value = value * 10 + i128::from(digit);
            }
            let value = if negative { -value } else { value };
            if let Ok(value) = i64::try_from(value) {
                return Ok(Self::Long(value));
            }
        }
        let value: f64 = text.parse().map_err(|_| invalid())?;
        Ok(if f64::from(value as f32) == value {
            Self::Float(value as f32)
        } else {
            Self::Double(value)
        })
    }

    /// `Number.floatValue()`.
    fn float(self) -> f32 {
        match self {
            Self::Long(value) => value as f32,
            Self::Float(value) => value,
            Self::Double(value) => value as f32,
        }
    }

    /// `(float) Number.doubleValue()`.
    fn double(self) -> f32 {
        match self {
            Self::Long(value) => value as f64 as f32,
            Self::Float(value) => value,
            Self::Double(value) => value as f32,
        }
    }

    /// `Number.intValue()`: a `long` keeps its low 32 bits; a `float` or `double` saturates (NaN gives 0).
    fn int(self) -> i32 {
        match self {
            Self::Long(value) => value as i32,
            Self::Float(value) => value as i32,
            Self::Double(value) => value as i32,
        }
    }
}

impl Data {
    /// `DataValue.asFloat()`: a number's `floatValue`, anything else 0.
    pub(super) fn as_float(&self) -> f32 {
        match self {
            Self::Num(number) => number.float(),
            _ => 0.0,
        }
    }
}

impl Obj {
    /// `DataSpec.getValue`: the field, or the spec's default when it is absent.
    pub(super) fn scalar(&self, key: &str) -> Option<&Data> {
        self.fields.get(key)
    }

    /// `spec.get(key, data, DataValue::asFloat)`.
    pub(super) fn float(&self, key: &str, default: f32) -> f32 {
        self.scalar(key).map_or(default, Data::as_float)
    }

    /// `spec.get(key, data, DataValue::asDouble)` narrowed to `float`.
    fn double(&self, key: &str, default: f32) -> f32 {
        match self.scalar(key) {
            None => default,
            Some(Data::Num(number)) => number.double(),
            Some(_) => 0.0,
        }
    }

    /// `spec.get(key, data, DataValue::asInt)`.
    fn int(&self, key: &str, default: i32) -> i32 {
        match self.scalar(key) {
            None => default,
            Some(Data::Num(number)) => number.int(),
            Some(_) => 0,
        }
    }

    /// `Enum.valueOf(type, value.asString())` over the constants' names.
    fn name<T: Copy>(
        &self,
        key: &str,
        default: T,
        constants: &[(&str, T)],
    ) -> Result<T, TerrainAssetError> {
        match self.scalar(key) {
            None => Ok(default),
            Some(Data::Str(name)) => constants
                .iter()
                .find(|(constant, _)| constant == name)
                .map(|&(_, value)| value)
                .ok_or_else(|| TerrainAssetError(format!("no enum constant {name:?} for {key:?}"))),
            Some(_) => error(format!("field {key:?} must name an enum constant")),
        }
    }

    /// `spec.get(key, data, Module.class, context)`: a missing or non-object field reads as `NULL_OBJ`, which no
    /// spec decodes.
    pub(super) fn child(&self, key: &str) -> Result<Module, TerrainAssetError> {
        match self.fields.get(key) {
            Some(Data::Obj(object)) => module(object),
            _ => error(format!("no spec registered for field {key:?}")),
        }
    }

    /// `Combiner`'s `data.getList("modules")` read with `Cereal.deserialize`: a non-list is empty and only objects
    /// are read.
    fn children(&self) -> Result<Vec<Module>, TerrainAssetError> {
        match self.fields.get("modules") {
            Some(Data::List(list)) => list
                .iter()
                .filter_map(|value| match value {
                    Data::Obj(object) => Some(module(object)),
                    _ => None,
                })
                .collect(),
            _ => Ok(Vec::new()),
        }
    }
}

const INTERPOLATIONS: [(&str, Interpolation); 3] = [
    ("LINEAR", Interpolation::Linear),
    ("CURVE3", Interpolation::Curve3),
    ("CURVE4", Interpolation::Curve4),
];
const EDGE_FUNCTIONS: [(&str, EdgeFunction); 5] = [
    ("DISTANCE_2", EdgeFunction::Distance2),
    ("DISTANCE_2_ADD", EdgeFunction::Distance2Add),
    ("DISTANCE_2_SUB", EdgeFunction::Distance2Sub),
    ("DISTANCE_2_MUL", EdgeFunction::Distance2Mul),
    ("DISTANCE_2_DIV", EdgeFunction::Distance2Div),
];
const DISTANCE_FUNCTIONS: [(&str, DistanceFunction); 3] = [
    ("EUCLIDEAN", DistanceFunction::Euclidean),
    ("MANHATTAN", DistanceFunction::Manhattan),
    ("NATURAL", DistanceFunction::Natural),
];
const EROSION_MODES: [(&str, ErosionMode); 3] = [
    ("CONSTANT", ErosionMode::Constant),
    ("INPUT_LINEAR", ErosionMode::InputLinear),
    ("OUTPUT_LINEAR", ErosionMode::OutputLinear),
];
const CELL_FUNCTIONS: [(&str, ()); 3] =
    [("CELL_VALUE", ()), ("NOISE_LOOKUP", ()), ("DISTANCE", ())];

/// `Cereal.deserialize(data, Module.class)`: the spec named by the object's type, which must build a `Module`.
fn module(object: &Obj) -> Result<Module, TerrainAssetError> {
    let o = object;
    let source = |kind| octave_source(o, kind);
    Ok(match o.kind.as_str() {
        "Const" => Module::new_constant(o.double("value", 1.0)),
        "Perlin" => source(SourceKind::Perlin)?,
        "Perlin2" => source(SourceKind::Perlin2)?,
        "Simplex" => source(SourceKind::Simplex)?,
        "Simplex2" => source(SourceKind::Simplex2)?,
        "Cubic" => source(SourceKind::Cubic)?,
        "Ridge" => source(SourceKind::Ridge)?,
        "Billow" => source(SourceKind::Billow)?,
        "SimplexRidge" => source(SourceKind::SimplexRidge)?,
        "CellEdge" => {
            let config = source_config(o, SourceKind::Cubic)?;
            let edge = o.name("edge_func", EdgeFunction::Distance2, &EDGE_FUNCTIONS)?;
            let distance = o.name(
                "dist_func",
                DistanceFunction::Euclidean,
                &DISTANCE_FUNCTIONS,
            )?;
            Module::cell_edge_source(config, 1.0, edge, distance)
        }
        "Abs" => Module::new_abs(o.child("source")?),
        "Alpha" => Module::new_alpha(o.child("source")?, o.child("alpha")?),
        "Bias" => Module::new_bias(o.child("source")?, o.float("bias", 0.0)),
        "Boost" => Module::new_boost(o.child("source")?, o.int("iterations", 1)),
        "Clamp" => Module::new_clamp(o.child("source")?, o.child("min")?, o.child("max")?),
        "Invert" => o.child("source")?.invert(),
        "Map" => Module::new_map(o.child("source")?, o.child("min")?, o.child("max")?),
        "Pow" => Module::new_power(o.child("source")?, o.child("power")?),
        "PowCurve" => o.child("source")?.power_curve(o.float("power", 1.0)),
        "Scale" => Module::new_scale(o.child("source")?, o.child("scale")?),
        "Warp" => o.child("source")?.warp(domain(o.fields.get("domain"))?),
        "Blend" => o.child("control")?.blend(
            o.child("lower")?,
            o.child("upper")?,
            o.float("midpoint", 0.5),
            o.float("blend_range", 0.0),
            o.name("interpolation", Interpolation::Linear, &INTERPOLATIONS)?,
        ),
        "Terrace" => {
            let (source, ramp, cliff) = (o.child("source")?, o.child("ramp")?, o.child("cliff")?);
            let ramp_height = o.child("ramp_height")?;
            // `new Step[steps]` throws for a negative count; with none, every sample throws.
            let steps = o.int("steps", 1);
            if steps < 1 {
                return error(format!("{steps} terrace steps"));
            }
            let steps = steps as usize;
            source.terrace(ramp, cliff, ramp_height, steps, o.float("blend_range", 1.0))
        }
        "AdvTerrace" => o.child("source")?.advanced_terrace(
            o.child("modulation")?,
            o.child("mask")?,
            o.child("slope")?,
            o.float("blend_min", 0.0),
            o.float("blend_max", 1.0),
            o.int("steps", 1),
            o.int("octaves", 1),
        ),
        "Steps" => o.child("source")?.steps(
            o.child("steps")?,
            o.child("slope_min")?,
            o.child("slope_max")?,
            curve(o.fields.get("curve"))?,
        ),
        // Engine 0.3.0's `Ridge.create`: `new Ridge(seed, octaves, strength, gridSize, amplitude, lacunarity,
        // fallOff, blend)`, then `ridge.wrap(source)`. The constructor checks nothing: zero or negative octaves
        // sample NaN (0/0), and a huge count makes every sample run that many octaves (up to 25 source reads
        // each), as in Java; nothing caps it.
        "Valley" => ErosionRidge::full(
            o.int("seed", 1337),
            o.int("octaves", 1),
            o.float("strength", 1.0),
            o.float("grid_size", 100.0),
            o.float("amplitude", 0.5),
            o.float("lacunarity", 2.25),
            o.float("fall_off", 0.75),
            o.name("blend", ErosionMode::Constant, &EROSION_MODES)?,
        )
        .wrap(o.child("source")?),
        "Add" => Module::add(o.children()?),
        "Mult" => Module::multiply(o.children()?),
        "Max" => Module::max(o.children()?),
        "Min" => Module::min(o.children()?),
        "Sub" => Module::subtract(o.children()?),
        name if registered(name) => {
            return error(format!("unsupported noise spec {name:?}"));
        }
        name => return error(format!("no spec registered for name {name:?}")),
    })
}

/// An octave source through `NoiseSource.readData` and its Java constructor.
fn octave_source(object: &Obj, kind: SourceKind) -> Result<Module, TerrainAssetError> {
    let config = source_config(object, kind)?;
    Ok(Module::source(Source::new(kind, config)))
}

/// `NoiseSource.readData` (`Builder` defaults: seed 1337, gain 0.5, one octave, frequency 1, lacunarity 2,
/// `CURVE3`), with the octave counts the `kind`'s constructor accepts: the signal tables of Perlin, Simplex and
/// their `2` forms throw for a negative count, and `RidgeNoise` (`Billow` too) and `SimplexRidge` for a negative
/// count or more than 30 (`SimplexRidge` when sampling). Cubic and cell sources accept any count; a negative one acts as 0.
fn source_config(object: &Obj, kind: SourceKind) -> Result<SourceConfig, TerrainAssetError> {
    let o = object;
    let seed = o.int("seed", 1337);
    let gain = o.double("gain", 0.5);
    let octaves = o.int("octaves", 1);
    let frequency = o.double("frequency", 1.0);
    let lacunarity = o.double("lacunarity", 2.0);
    let interpolation = o.name("interpolation", Interpolation::Curve3, &INTERPOLATIONS)?;
    o.name("cell_func", (), &CELL_FUNCTIONS)?;
    o.name("edge_func", EdgeFunction::Distance2, &EDGE_FUNCTIONS)?;
    o.name(
        "dist_func",
        DistanceFunction::Euclidean,
        &DISTANCE_FUNCTIONS,
    )?;
    if o.fields.contains_key("source") {
        o.child("source")?;
    }
    let throws = match kind {
        SourceKind::Cubic => false,
        SourceKind::Ridge | SourceKind::Billow => !(0..=30).contains(&octaves),
        // `SimplexRidgeNoise` builds, but every sample then reads past its 30 spectral weights and throws: such a
        // graph can never be sampled, so it is rejected here.
        SourceKind::SimplexRidge => !(0..=30).contains(&octaves),
        _ => octaves < 0,
    };
    if throws {
        return error(format!("{octaves} octaves are out of range"));
    }
    Ok(SourceConfig {
        seed,
        octaves: octaves.max(0) as usize,
        // `Builder.getGain`: `Float.MAX_VALUE` is the builder's "unset" marker.
        gain: if gain == f32::MAX { 0.5 } else { gain },
        frequency,
        lacunarity,
        interpolation,
    })
}

/// `spec.get("domain", data, Domain.class, context)`.
fn domain(value: Option<&Data>) -> Result<Domain, TerrainAssetError> {
    let Some(Data::Obj(o)) = value else {
        return error("no spec registered for the domain");
    };
    match o.kind.as_str() {
        "DomainWarp" => Ok(Domain::domain_warp(
            o.child("x")?,
            o.child("y")?,
            o.child("distance")?,
        )),
        "DirectionWarp" => Ok(Domain::direction(
            o.child("direction")?,
            o.child("strength")?,
        )),
        "CumulativeWarp" => error("unsupported domain spec \"CumulativeWarp\""),
        name => error(format!("no domain spec for name {name:?}")),
    }
}

/// `spec.get("curve", data, CurveFunc.class, context)`: the `Interpolation` spec by name; for a name no spec has
/// (including none), `CurveFunc`'s sub-spec picks the first sub-spec whose fields are all present with the default's
/// type, so an object with a string `mode` reads as an `Interpolation` (`MidPointCurve` and `SCurve` are not
/// supported here).
fn curve(value: Option<&Data>) -> Result<CurveFunction, TerrainAssetError> {
    let o = match value {
        Some(Data::Obj(o)) => o,
        _ => return error("unsupported curve data"),
    };
    let typed = o.kind == "Interpolation";
    if typed || (!registered(&o.kind) && matches!(o.scalar("mode"), Some(Data::Str(_)))) {
        let mode = o.name("mode", Interpolation::Linear, &INTERPOLATIONS)?;
        return Ok(CurveFunction::Interpolation(mode));
    }
    error(format!("unsupported curve data {:?}", o.kind))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::terraforged_engine::TerrainCategory;
    use super::*;

    /// `Float.floatToIntBits`.
    pub(crate) fn bits(value: f32) -> u32 {
        if value.is_nan() {
            0x7fc0_0000
        } else {
            value.to_bits()
        }
    }

    /// `TerrainAssetOracle`'s FNV-1a hash of `noise`'s seed-first samples (a sample that throws in Java, here a
    /// panic, folds as `0xffffffff`).
    pub(crate) fn sample_hash(noise: &Module) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut fold = |x: f32, y: f32, seed: i32| {
            let sample = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                noise.try_value_seed_first(seed, x, y)
            }));
            let sample = match sample {
                Ok(Ok(value)) => bits(value),
                Ok(Err(_)) => 0xffff_fffe,
                Err(_) => 0xffff_ffff,
            };
            hash = (hash ^ u64::from(sample)).wrapping_mul(0x0100_0000_01b3);
        };
        for seed in [0, 1, -1, 1337, 123_456_789, i32::MIN, i32::MAX] {
            for i in 0..40 {
                let x = ((i * 7919) % 2001 - 1000) as f32 * 3.25 + i as f32 * 0.125;
                let y = ((i * 104_729) % 1999 - 999) as f32 * 4.5 - i as f32 * 0.375;
                fold(x, y, seed);
            }
            fold(1.0e6, -2.5e6, seed);
            fold(-123_456.78, 98_765.43, seed);
        }
        hash
    }

    /// One `TerrainAssetOracle` line: the decoded id, weight bits, the raised noise's bounds and an FNV-1a hash of
    /// seed-first samples (a sample that throws in Java, here a panic, folds as `0xffffffff`).
    fn line(name: &str, json: &str) -> String {
        let terrain =
            Terrain::configured(31, "hills", TerrainCategory::Highland, 0.5, false, false);
        let mut id = String::new();
        let decoded = decode_terrain_asset(json, |name| {
            id = name.to_owned();
            Some(terrain.clone())
        });
        let Ok(decoded) = decoded else {
            return format!("{name} err");
        };
        let noise = decoded.noise();
        let hash = sample_hash(noise);
        format!(
            "{name} ok type={id} weight={:08x} min={:08x} max={:08x} hash={hash:016x}",
            bits(decoded.weight()),
            bits(noise.min_value()),
            bits(noise.max_value()),
        )
    }

    #[test]
    fn cases_match_reimagined_java() {
        let mut rust = format!("specs {SPEC_NAMES}\n");
        for case in CASES.lines() {
            let (name, json) = case.split_once('\t').unwrap();
            rust.push_str(&line(name, json));
            rust.push('\n');
        }
        // Where Java decodes a graph whose every sample throws, the port rejects it when decoding.
        let java: String = ORACLE
            .lines()
            .map(|java| match java.split_once(" ok ") {
                Some((name, _)) if java.ends_with(&format!("hash={:016x}", throwing_hash())) => {
                    format!("{name} err\n")
                }
                _ => format!("{java}\n"),
            })
            .collect();
        for (rust, java) in rust.lines().zip(java.lines()) {
            assert_eq!(rust, java);
        }
        assert_eq!(rust, java);
    }

    /// The hash of a graph whose every sample throws.
    fn throwing_hash() -> u64 {
        (0..7 * 42).fold(0xcbf2_9ce4_8422_2325u64, |hash, _| {
            (hash ^ 0xffff_ffff).wrapping_mul(0x0100_0000_01b3)
        })
    }

    /// Reimagined a24d450's 14 terrain assets, read where the TerraForged sources are unpacked locally (they are not
    /// copied into this repository); skipped when absent.
    #[test]
    fn terrain_noise_assets_match_reimagined_java() {
        const ASSETS: &str = "src/main/resources/default/data/terraforged/worldgen/terrain_noise";
        let root = std::env::var_os("TERRAFORGED_REIMAGINED")
            .map(std::path::PathBuf::from)
            .into_iter()
            .chain(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .ancestors()
                    .flat_map(|dir| {
                        [
                            ".local/terraforged/terraforged-reimagined-26.3",
                            "terraforged-reimagined-26.3",
                        ]
                        .map(|tree| dir.join(tree))
                    }),
            )
            .map(|tree| tree.join(ASSETS))
            .find(|dir| dir.is_dir());
        let Some(root) = root else {
            eprintln!("skipping: TerraForged Reimagined's terrain_noise assets are not unpacked");
            return;
        };
        let mut files: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        files.sort();
        let rust: Vec<String> = files
            .iter()
            .map(|path| {
                let name = path.file_name().unwrap().to_str().unwrap();
                line(name, &std::fs::read_to_string(path).unwrap())
            })
            .collect();
        assert_eq!(rust.join("\n"), ASSET_ORACLE.trim_end());
    }

    /// TerraForged 6dd607eb's generated `terrain/noise` assets that use Engine's `Valley` (`mountains_1..3`), read
    /// where the TerraForged sources are unpacked locally (not copied here); skipped when absent.
    #[test]
    fn valley_assets_match_terraforged_java() {
        const ASSETS: &str = "generated/resources/default/data/terraforged/worldgen/terrain/noise";
        let root = std::env::var_os("TERRAFORGED_SOURCE")
            .map(std::path::PathBuf::from)
            .into_iter()
            .chain(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .ancestors()
                    .flat_map(|dir| {
                        [
                            ".local/terraforged/terraforged-source",
                            "terraforged-source",
                        ]
                        .map(|tree| dir.join(tree))
                    }),
            )
            .map(|tree| tree.join(ASSETS))
            .find(|dir| dir.is_dir());
        let Some(root) = root else {
            eprintln!(
                "skipping: TerraForged 6dd607eb's generated terrain noise assets are not unpacked"
            );
            return;
        };
        // `mountains_1`, `_2` and `_3`; an empty sync would otherwise pass.
        assert_eq!(SOURCE_ASSET_ORACLE.lines().count(), 3);
        let rust: Vec<String> = SOURCE_ASSET_ORACLE
            .lines()
            .map(|java| {
                let name = java.split_once(' ').unwrap().0;
                let file = name.strip_prefix("6dd607eb/").unwrap();
                line(name, &std::fs::read_to_string(root.join(file)).unwrap())
            })
            .collect();
        assert_eq!(rust.join("\n"), SOURCE_ASSET_ORACLE.trim_end());
    }

    /// `.local/terraforged/oracles/terrain_asset/cases.txt` (name, tab, the asset's JSON); `sync.py` there copies it.
    const CASES: &str = r#"const	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Const","value":0.25}}
const_default	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Const"}}
const_neg_zero	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Const","value":-0.0}}
const_string	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Const","value":"0.5"}}
const_bool	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Const","value":true}}
const_object	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Const","value":{"type":"Const","value":3}}}
const_list	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Const","value":[0.5]}}
const_double_rounding	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Const","value":1.0000000596046447753906251}}
perlin_defaults	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin"}}
perlin_full	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","seed":-91,"octaves":3,"gain":0.65,"frequency":0.0078125,"lacunarity":2.4,"interpolation":"CURVE4"}}
perlin_int_wrap	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","seed":2147483648,"octaves":2.9,"frequency":0.01}}
perlin_exponent_ints	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","seed":1e3,"octaves":3E0,"frequency":1e-2}}
perlin_long_seed	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","seed":4294967297,"frequency":0.02}}
perlin_decimal_seed	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","seed":1e10,"frequency":0.02}}
perlin_negative_decimal_seed	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","seed":-12345678901.75,"frequency":0.02}}
perlin_negative_octaves	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","octaves":-2,"frequency":0.02}}
perlin_bad_interpolation	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","interpolation":"curve3"}}
perlin_number_interpolation	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","interpolation":1}}
perlin_bad_edge_func	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","edge_func":"BAD"}}
perlin_cell_keys	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","cell_func":"NOISE_LOOKUP","dist_func":"MANHATTAN","edge_func":"DISTANCE_2_DIV","frequency":0.03}}
perlin_bad_source	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","source":{"type":"Nope"}}}
perlin_good_source	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","source":{"type":"Const","value":0.5},"frequency":0.03}}
perlin_max_gain	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","gain":3.4028234663852886e38,"frequency":0.02,"octaves":3}}
ridge	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Ridge","seed":5,"octaves":4,"gain":0.975,"frequency":0.004,"lacunarity":2.1}}
ridge_defaults	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Ridge"}}
ridge_negative_octaves	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Ridge","octaves":-1}}
simplex	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Simplex","seed":11,"octaves":3,"gain":0.45,"frequency":0.006,"interpolation":"LINEAR"}}
cubic	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Cubic","seed":-3,"octaves":2,"frequency":0.05}}
billow	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Billow","seed":99,"octaves":2,"frequency":0.02}}
simplex_ridge	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"SimplexRidge","seed":13,"octaves":3,"gain":0.8,"frequency":0.01}}
cell_edge	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"CellEdge","seed":37,"frequency":0.01,"distance":0.3,"edge_func":"DISTANCE_2_SUB","dist_func":"NATURAL"}}
cell_edge_defaults	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"CellEdge","frequency":0.05}}
bias_module	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Bias","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"bias":{"type":"Const","value":0.3}}}
bias_number	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Bias","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"bias":0.3}}
bias_missing	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Bias","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
bias_missing_source	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Bias","bias":0.3}}
alpha_out_of_range	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Alpha","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"alpha":{"type":"Const","value":2}}}
boost_zero	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Boost","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"iterations":0}}
boost_negative	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Boost","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"iterations":-3}}
boost_default	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Boost","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
clamp	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Clamp","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"min":{"type":"Const","value":0.2},"max":{"type":"Const","value":0.7}}}
clamp_same_bounds	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Clamp","source":{"type":"Const","value":0.75},"min":{"type":"Const","value":0.75},"max":{"type":"Const","value":0.75}}}
map	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Map","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"min":{"type":"Const","value":-1},"max":{"type":"Const","value":2}}}
map_missing_max	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Map","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"min":{"type":"Const","value":-1}}}
invert	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Invert","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
pow_zero	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Pow","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"power":{"type":"Const","value":0}}}
pow_one	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Pow","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"power":{"type":"Const","value":1}}}
scale_one	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Scale","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"scale":{"type":"Const","value":1}}}
scale	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Scale","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"scale":{"type":"Const","value":0.6}}}
pow_curve_default	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"PowCurve","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
pow_curve	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"PowCurve","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"power":0.8}}
blend_defaults	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Blend","control":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"lower":{"type":"Const","value":0.2},"upper":{"type":"Const","value":0.8}}}
blend_float_rounding	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Blend","control":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"lower":{"type":"Const","value":0.2},"upper":{"type":"Const","value":0.8},"midpoint":0.50000002980232238769531251,"blend_range":0.25,"interpolation":"CURVE3"}}
blend_bad_interpolation	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Blend","control":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"lower":{"type":"Const","value":0.2},"upper":{"type":"Const","value":0.8},"interpolation":"SMOOTH"}}
terrace_defaults	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Terrace","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"ramp":{"type":"Const","value":0.2},"cliff":{"type":"Const","value":0.7},"ramp_height":{"type":"Const","value":0.8}}}
terrace	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Terrace","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"ramp":{"type":"Const","value":0.2},"cliff":{"type":"Const","value":0.7},"ramp_height":{"type":"Const","value":0.8},"steps":5,"blend_range":0.25}}
terrace_negative_steps	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Terrace","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"ramp":{"type":"Const","value":0.2},"cliff":{"type":"Const","value":0.7},"ramp_height":{"type":"Const","value":0.8},"steps":-1}}
terrace_no_steps	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Terrace","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"ramp":{"type":"Const","value":0.2},"cliff":{"type":"Const","value":0.7},"ramp_height":{"type":"Const","value":0.8},"steps":0}}
adv_terrace_defaults	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"AdvTerrace","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"modulation":{"type":"Const","value":0.1},"mask":{"type":"Const","value":0.9},"slope":{"type":"Const","value":0.4}}}
adv_terrace	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"AdvTerrace","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"modulation":{"type":"Const","value":0.1},"mask":{"type":"Const","value":0.9},"slope":{"type":"Const","value":0.4},"blend_min":0.1,"blend_max":0.8,"steps":6.5,"octaves":2}}
steps_untyped_curve	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Steps","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"steps":{"type":"Const","value":5},"slope_min":{"type":"Const","value":0.05},"slope_max":{"type":"Const","value":0.75},"curve":{"mode":"CURVE4"}}}
steps_unregistered_curve	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Steps","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"steps":{"type":"Const","value":5},"slope_min":{"type":"Const","value":0.05},"slope_max":{"type":"Const","value":0.75},"curve":{"type":"Foo","mode":"CURVE3"}}}
steps_default_mode	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Steps","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"steps":{"type":"Const","value":5},"slope_min":{"type":"Const","value":0.05},"slope_max":{"type":"Const","value":0.75},"curve":{"type":"Interpolation"}}}
steps_bad_mode	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Steps","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"steps":{"type":"Const","value":5},"slope_min":{"type":"Const","value":0.05},"slope_max":{"type":"Const","value":0.75},"curve":{"type":"Interpolation","mode":"X"}}}
steps_untyped_bad_mode	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Steps","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"steps":{"type":"Const","value":5},"slope_min":{"type":"Const","value":0.05},"slope_max":{"type":"Const","value":0.75},"curve":{"mode":"X"}}}
steps_module_curve	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Steps","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"steps":{"type":"Const","value":5},"slope_min":{"type":"Const","value":0.05},"slope_max":{"type":"Const","value":0.75},"curve":{"type":"Const","value":1}}}
steps_missing_curve	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Steps","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"steps":{"type":"Const","value":5},"slope_min":{"type":"Const","value":0.05},"slope_max":{"type":"Const","value":0.75}}}
add_empty	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Add"}}
add	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Add","modules":[{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},{"type":"Const","value":0.25}]}}
mult_skips_non_objects	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Mult","modules":[2,{"type":"Const","value":0.5},"x",[],{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}]}}
max_modules_object	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Max","modules":{"type":"Const","value":0.5}}}
max	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Max","modules":[{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},{"type":"Const","value":0.4}]}}
domain_warp	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Warp","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"domain":{"type":"DomainWarp","x":{"type":"Perlin","seed":2,"frequency":0.02},"y":{"type":"Simplex","seed":3,"frequency":0.02},"distance":{"type":"Const","value":40}}}}
direction_warp	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Warp","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"domain":{"type":"DirectionWarp","direction":{"type":"Perlin","seed":4,"frequency":0.02},"strength":{"type":"Const","value":25}}}}
warp_const_domain	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Warp","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"domain":{"type":"Const","value":1}}}
warp_missing_domain	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Warp","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
unknown_type	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Unknown"}}
missing_type	{"type":"terraforged:hills","weight":1.5,"noise":{"value":1}}
nested_null	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Const","value":1,"x":null}}
type_array	{"type":"terraforged:hills","weight":1.5,"noise":{"type":["Const"],"value":0.5}}
noise_number	{"type":"terraforged:hills","weight":1.5,"noise":1.5}
noise_interpolation	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Interpolation","mode":"LINEAR"}}
duplicate_key	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Const","value":1,"value":0.25}}
raise_zero	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Const","value":0}}
weight_float_rounding	{"type":"terraforged:hills","weight":1.0000000596046447753906251,"noise":{"type":"Const","value":0.5}}
weight_string	{"type":"terraforged:hills","weight":"2","noise":{"type":"Const","value":0.5}}
weight_bool	{"type":"terraforged:hills","weight":true,"noise":{"type":"Const","value":0.5}}
weight_missing	{"type":"terraforged:hills","noise":{"type":"Const","value":0.5}}
weight_integer	{"type":"terraforged:hills","weight":3,"noise":{"type":"Const","value":0.5}}
type_default_namespace	{"type":"hills","weight":1.5,"noise":{"type":"Const","value":0.5}}
type_leading_colon	{"type":":hills","weight":1.5,"noise":{"type":"Const","value":0.5}}
type_invalid	{"type":"Terraforged:hills","weight":1.5,"noise":{"type":"Const","value":0.5}}
type_number	{"type":5,"weight":1.5,"noise":{"type":"Const","value":0.5}}
type_other_namespace	{"type":"mymod:hills/tall_2","weight":1.5,"noise":{"type":"Const","value":0.5}}
noise_missing	{"type":"terraforged:hills","weight":1}
top_level_array	[1]
trailing_data	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Const","value":0.5}} x
extra_null_field	{"type":"terraforged:hills","weight":1,"noise":{"type":"Const","value":0.5},"extra":null}
octaves_perlin_neg1	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","seed":21,"octaves":-1,"frequency":0.02}}
octaves_perlin_0	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","seed":21,"octaves":0,"frequency":0.02}}
octaves_perlin_31	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","seed":21,"octaves":31,"frequency":0.02}}
octaves_simplex_neg1	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Simplex","seed":21,"octaves":-1,"frequency":0.02}}
octaves_simplex_0	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Simplex","seed":21,"octaves":0,"frequency":0.02}}
octaves_simplex_31	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Simplex","seed":21,"octaves":31,"frequency":0.02}}
octaves_cubic_neg1	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Cubic","seed":21,"octaves":-1,"frequency":0.02}}
octaves_cubic_0	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Cubic","seed":21,"octaves":0,"frequency":0.02}}
octaves_cubic_31	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Cubic","seed":21,"octaves":31,"frequency":0.02}}
octaves_ridge_neg1	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Ridge","seed":21,"octaves":-1,"frequency":0.02}}
octaves_ridge_0	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Ridge","seed":21,"octaves":0,"frequency":0.02}}
octaves_ridge_31	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Ridge","seed":21,"octaves":31,"frequency":0.02}}
octaves_billow_neg1	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Billow","seed":21,"octaves":-1,"frequency":0.02}}
octaves_billow_0	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Billow","seed":21,"octaves":0,"frequency":0.02}}
octaves_billow_31	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Billow","seed":21,"octaves":31,"frequency":0.02}}
octaves_simplexridge_neg1	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"SimplexRidge","seed":21,"octaves":-1,"frequency":0.02}}
octaves_simplexridge_0	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"SimplexRidge","seed":21,"octaves":0,"frequency":0.02}}
octaves_simplexridge_31	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"SimplexRidge","seed":21,"octaves":31,"frequency":0.02}}
octaves_celledge_neg1	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"CellEdge","seed":21,"octaves":-1,"frequency":0.02}}
octaves_celledge_0	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"CellEdge","seed":21,"octaves":0,"frequency":0.02}}
octaves_celledge_31	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"CellEdge","seed":21,"octaves":31,"frequency":0.02}}
octaves_perlin2_neg1	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin2","seed":21,"octaves":-1,"frequency":0.02}}
octaves_perlin2_0	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin2","seed":21,"octaves":0,"frequency":0.02}}
octaves_perlin2_31	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin2","seed":21,"octaves":31,"frequency":0.02}}
octaves_simplex2_neg1	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Simplex2","seed":21,"octaves":-1,"frequency":0.02}}
octaves_simplex2_0	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Simplex2","seed":21,"octaves":0,"frequency":0.02}}
octaves_simplex2_31	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Simplex2","seed":21,"octaves":31,"frequency":0.02}}
perlin2	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin2","seed":8,"octaves":3,"gain":0.55,"frequency":0.015,"interpolation":"CURVE4"}}
simplex2	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Simplex2","seed":9,"octaves":3,"frequency":0.015}}
min	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Min","modules":[{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},{"type":"Const","value":0.4}]}}
sub	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Sub","modules":[{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},{"type":"Const","value":0.4},{"type":"Simplex","seed":3,"frequency":0.02}]}}
seed_bigdecimal	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","seed":123456789012345678901234567890,"frequency":0.02}}
seed_negative_fraction	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","seed":-2.5,"frequency":0.02}}
seed_small_fraction	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","seed":1.5e-5,"frequency":0.02}}
seed_huge_exponent	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","seed":1e9999,"frequency":0.02}}
seed_too_large_exponent	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","seed":1e10000,"frequency":0.02}}
seed_too_small_exponent	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","seed":1e-10000,"frequency":0.02}}
seed_fraction_exponent	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","seed":-98765.4321e3,"frequency":0.02}}
frequency_overflow	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","frequency":1e400}}
type_trailing_colon	{"type":"hills:","weight":1.5,"noise":{"type":"Const","value":0.5}}
type_two_colons	{"type":"a:b:c","weight":1.5,"noise":{"type":"Const","value":0.5}}
type_uppercase_path	{"type":"terraforged:Hills","weight":1.5,"noise":{"type":"Const","value":0.5}}
type_space	{"type":"terraforged:hills ","weight":1.5,"noise":{"type":"Const","value":0.5}}
type_empty	{"type":"","weight":1.5,"noise":{"type":"Const","value":0.5}}
type_all_chars	{"type":"a-b_c.9:x/y.z-w_0","weight":1.5,"noise":{"type":"Const","value":0.5}}
type_namespace_slash	{"type":"a/b:c","weight":1.5,"noise":{"type":"Const","value":0.5}}
type_array_string	{"type":["terraforged:hills"],"weight":1.5,"noise":{"type":"Const","value":0.5}}
noise_type_number	{"type":"terraforged:hills","weight":1.5,"noise":{"type":5,"value":1}}
noise_type_bool	{"type":"terraforged:hills","weight":1.5,"noise":{"type":true}}
noise_type_object	{"type":"terraforged:hills","weight":1.5,"noise":{"type":{},"value":1}}
noise_type_null	{"type":"terraforged:hills","weight":1.5,"noise":{"type":null,"value":1}}
noise_type_two	{"type":"terraforged:hills","weight":1.5,"noise":{"type":["Const","Const"],"value":1}}
noise_unsupported_line	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Line"}}
seed_exponent_min	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","seed":1e-9223372036854775808,"frequency":0.02}}
seed_exponent_overflow	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Perlin","seed":1e99999999999999999999,"frequency":0.02}}
valley_constant	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","seed":310154,"octaves":2,"strength":0.65,"grid_size":128,"amplitude":0.15,"lacunarity":3.1,"fall_off":0.8,"blend":"CONSTANT","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
valley_input_linear	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","seed":310154,"octaves":2,"strength":0.65,"grid_size":128,"amplitude":0.15,"lacunarity":3.1,"fall_off":0.8,"blend":"INPUT_LINEAR","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
valley_output_linear	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","seed":310154,"octaves":2,"strength":0.65,"grid_size":128,"amplitude":0.15,"lacunarity":3.1,"fall_off":0.8,"blend":"OUTPUT_LINEAR","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
valley_defaults	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
valley_missing_source	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","seed":310154,"octaves":2,"strength":0.65,"grid_size":128,"amplitude":0.15,"lacunarity":3.1,"fall_off":0.8}}
valley_list_source	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","seed":310154,"octaves":2,"strength":0.65,"grid_size":128,"amplitude":0.15,"lacunarity":3.1,"fall_off":0.8,"source":[{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}]}}
valley_bad_blend	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","seed":310154,"octaves":2,"strength":0.65,"grid_size":128,"amplitude":0.15,"lacunarity":3.1,"fall_off":0.8,"blend":"SIDEWAYS","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
valley_lowercase_blend	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","seed":310154,"octaves":2,"strength":0.65,"grid_size":128,"amplitude":0.15,"lacunarity":3.1,"fall_off":0.8,"blend":"constant","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
valley_number_blend	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","seed":310154,"octaves":2,"strength":0.65,"grid_size":128,"amplitude":0.15,"lacunarity":3.1,"fall_off":0.8,"blend":1,"source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
valley_zero_octaves	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","seed":5,"octaves":0,"grid_size":64,"source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
valley_negative_octaves	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","seed":5,"octaves":-2,"grid_size":64,"source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
valley_fraction_octaves	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","seed":5,"octaves":2.9,"grid_size":64,"source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
valley_many_octaves	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","seed":5,"octaves":6,"grid_size":300,"blend":"OUTPUT_LINEAR","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
valley_seed_wrap	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","seed":2147483648,"grid_size":64,"source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
valley_seed_double	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","seed":1e10,"grid_size":64,"source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
valley_seed_bool	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","seed":true,"grid_size":64,"source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
valley_string_strength	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","strength":"0.5","grid_size":64,"source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
valley_module_amplitude	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","octaves":3,"amplitude":{"type":"Const","value":0.5},"grid_size":64,"source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
valley_float_rounding	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","strength":0.65000000000000002220446049250313,"grid_size":96.00000000000001,"blend":"INPUT_LINEAR","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
valley_zero_grid	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","grid_size":0,"source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
valley_negative_grid	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","grid_size":-50,"strength":-0.25,"blend":"OUTPUT_LINEAR","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
valley_long_grid	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","grid_size":4294967297,"source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
valley_const_sentinel	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","grid_size":64,"source":{"type":"Const","value":-1}}}
valley_nested	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","seed":11,"grid_size":80,"blend":"INPUT_LINEAR","source":{"type":"Valley","seed":12,"octaves":2,"grid_size":40,"source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}}
valley_in_add	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Add","modules":[{"type":"Valley","grid_size":64,"source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}},{"type":"Const","value":0.1}]}}
valley_bad_source	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","grid_size":64,"source":{"type":"Line"}}}
valley_null_field	{"type":"terraforged:mountains","weight":1.25,"noise":{"type":"Valley","grid_size":64,"blend":null,"source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01}}}
steps_valley_curve	{"type":"terraforged:hills","weight":1.5,"noise":{"type":"Steps","source":{"type":"Perlin","seed":7,"octaves":2,"frequency":0.01},"steps":{"type":"Const","value":5},"slope_min":{"type":"Const","value":0.05},"slope_max":{"type":"Const","value":0.75},"curve":{"type":"Valley","mode":"CURVE3"}}}
"#;

    /// `.local/terraforged/oracles/terrain_asset/run.sh`'s `out.txt` up to the asset lines.
    const ORACLE: &str = r#"specs Abs Add AdvTerrace Alpha Base Bias Billow Blend Boost Cache Cell CellEdge Clamp Const Cubic CumulativeWarp Curve DirectionWarp DomainWarp Freq Grad Interpolation Invert LegacyTerrace Line Map Max Min Modulate Mult MultiBlend Perlin Perlin2 Pow PowCurve Rand Ridge Scale Select Simplex Simplex2 SimplexRidge Sin Steps Sub Terrace Threshold Valley VariBlend VariCurve Vec2f Vec2i Warp
const ok type=terraforged:hills weight=3fc00000 min=3e800000 max=3e800000 hash=3d608a822f1743bd
const_default ok type=terraforged:hills weight=3fc00000 min=3f800000 max=3f800000 hash=7b15fa60031743bd
const_neg_zero ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=123df1760ad9a6eb
const_string ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=123df1760ad9a6eb
const_bool ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=123df1760ad9a6eb
const_object ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=123df1760ad9a6eb
const_list ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=123df1760ad9a6eb
const_double_rounding ok type=terraforged:hills weight=3fc00000 min=3f800000 max=3f800000 hash=7b15fa60031743bd
perlin_defaults ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=28cd60f8d394a819
perlin_full ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=95792762ce8e6242
perlin_int_wrap ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=e3802047df346167
perlin_exponent_ints ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=9ebdb8f7b7443b76
perlin_long_seed ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=6e9bd685422f4089
perlin_decimal_seed ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=078da475a5f4c698
perlin_negative_decimal_seed ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=ecb0a735ac9bab83
perlin_negative_octaves err
perlin_bad_interpolation err
perlin_number_interpolation err
perlin_bad_edge_func err
perlin_cell_keys ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=25cd34b7628c4b4a
perlin_bad_source err
perlin_good_source ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=25cd34b7628c4b4a
perlin_max_gain ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=f1d27e66266c6816
ridge ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=528951020a557b03
ridge_defaults ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=0cd8afcc59c5aae3
ridge_negative_octaves err
simplex ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=cf9c98c12dcd9d19
cubic ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=d452b1d71f104db0
billow ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=a5ef0371a0816566
simplex_ridge ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=beee2838f3d77b29
cell_edge ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=f9193c8c6a8c9a89
cell_edge_defaults ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=5c10b3e7d3c25f57
bias_module ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=176939b733532bf4
bias_number ok type=terraforged:hills weight=3fc00000 min=3e99999a max=3fa66666 hash=2359fbf397770ae8
bias_missing ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=176939b733532bf4
bias_missing_source err
alpha_out_of_range ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=971b4bda52955758
boost_zero ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=9531831e856c73f6
boost_negative ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=9531831e856c73f6
boost_default ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=9531831e856c73f6
clamp ok type=terraforged:hills weight=3fc00000 min=3e4ccccd max=3f333333 hash=da5f3cdadc5c7275
clamp_same_bounds ok type=terraforged:hills weight=3fc00000 min=3f400000 max=3f400000 hash=4d97bccb899743bd
map ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=6b2c36d07b8aac53
map_missing_max err
invert ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=81547e8c2c8edb58
pow_zero ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=7b15fa60031743bd
pow_one ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=176939b733532bf4
scale_one ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=176939b733532bf4
scale ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=9ef0f570dd7d395c
pow_curve_default ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=176939b733532bf4
pow_curve ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=cc08caa20df3b1e3
blend_defaults ok type=terraforged:hills weight=3fc00000 min=3e4ccccd max=3f4ccccd hash=f9d5873a079a30f6
blend_float_rounding ok type=terraforged:hills weight=3fc00000 min=3e4ccccd max=3f4ccccd hash=cbfdc85ca08c1872
blend_bad_interpolation err
terrace_defaults ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=13a61517ea9743bd
terrace ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=e9fddea8d30a84c1
terrace_negative_steps err
terrace_no_steps ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=ed144a389d4e42cf
adv_terrace_defaults ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=52ff86b9dd833187
adv_terrace ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=2fa77e628d70b24c
steps_untyped_curve ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=c9b105a0ca8dd85e
steps_unregistered_curve ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=5e36888c46452b6f
steps_default_mode ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=e741ae2bf22989ca
steps_bad_mode err
steps_untyped_bad_mode err
steps_module_curve err
steps_missing_curve err
add_empty ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=123df1760ad9a6eb
add ok type=terraforged:hills weight=3fc00000 min=3e800000 max=3fa00000 hash=b6cb396888a4144c
mult_skips_non_objects ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=ffaf959bd5870830
max_modules_object ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=123df1760ad9a6eb
max ok type=terraforged:hills weight=3fc00000 min=3ecccccd max=3f800000 hash=dbf8c031200ffe66
domain_warp ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=7f2d68abefe1f8f5
direction_warp ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=7d3f156b8ee2af42
warp_const_domain err
warp_missing_domain err
unknown_type err
missing_type err
nested_null err
type_array ok type=terraforged:hills weight=3fc00000 min=3f000000 max=3f000000 hash=18e7c4bf891743bd
noise_number err
noise_interpolation err
duplicate_key ok type=terraforged:hills weight=3fc00000 min=3e800000 max=3e800000 hash=3d608a822f1743bd
raise_zero ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=123df1760ad9a6eb
weight_float_rounding ok type=terraforged:hills weight=3f800001 min=3f000000 max=3f000000 hash=18e7c4bf891743bd
weight_string err
weight_bool err
weight_missing err
weight_integer ok type=terraforged:hills weight=40400000 min=3f000000 max=3f000000 hash=18e7c4bf891743bd
type_default_namespace ok type=minecraft:hills weight=3fc00000 min=3f000000 max=3f000000 hash=18e7c4bf891743bd
type_leading_colon ok type=minecraft:hills weight=3fc00000 min=3f000000 max=3f000000 hash=18e7c4bf891743bd
type_invalid err
type_number err
type_other_namespace ok type=mymod:hills/tall_2 weight=3fc00000 min=3f000000 max=3f000000 hash=18e7c4bf891743bd
noise_missing err
top_level_array err
trailing_data err
extra_null_field ok type=terraforged:hills weight=3f800000 min=3f000000 max=3f000000 hash=18e7c4bf891743bd
octaves_perlin_neg1 err
octaves_perlin_0 ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=7b15fa60031743bd
octaves_perlin_31 ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=26ef18201184e09a
octaves_simplex_neg1 err
octaves_simplex_0 ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=7b15fa60031743bd
octaves_simplex_31 ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=92fb6847582d5800
octaves_cubic_neg1 ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=1edfe9390412535f
octaves_cubic_0 ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=1edfe9390412535f
octaves_cubic_31 ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=bbc58d577c476cbf
octaves_ridge_neg1 err
octaves_ridge_0 ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=7b15fa60031743bd
octaves_ridge_31 err
octaves_billow_neg1 err
octaves_billow_0 ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=123df1760ad9a6eb
octaves_billow_31 err
octaves_simplexridge_neg1 err
octaves_simplexridge_0 ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=7b15fa60031743bd
octaves_simplexridge_31 ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=ed144a389d4e42cf
octaves_celledge_neg1 ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=695ed19433388f0d
octaves_celledge_0 ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=695ed19433388f0d
octaves_celledge_31 ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=695ed19433388f0d
octaves_perlin2_neg1 err
octaves_perlin2_0 ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=7b15fa60031743bd
octaves_perlin2_31 ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=ead9e76485d743bd
octaves_simplex2_neg1 err
octaves_simplex2_0 ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=7b15fa60031743bd
octaves_simplex2_31 ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=7552036a7c543abd
perlin2 ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=5680e64e6709c296
simplex2 ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=447ca733613e93b6
min ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=a79a380bc17c6f36
sub ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=452440fe956e1824
seed_bigdecimal ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=695e7b2aa691e353
seed_negative_fraction ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=27edc064292bb929
seed_small_fraction ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=ecb0a735ac9bab83
seed_huge_exponent ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=695e7b2aa691e353
seed_too_large_exponent err
seed_too_small_exponent err
seed_fraction_exponent ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=b5ee172c93a7924e
frequency_overflow ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=13a61517ea9743bd
type_trailing_colon ok type=hills: weight=3fc00000 min=3f000000 max=3f000000 hash=18e7c4bf891743bd
type_two_colons err
type_uppercase_path err
type_space err
type_empty ok type=minecraft: weight=3fc00000 min=3f000000 max=3f000000 hash=18e7c4bf891743bd
type_all_chars ok type=a-b_c.9:x/y.z-w_0 weight=3fc00000 min=3f000000 max=3f000000 hash=18e7c4bf891743bd
type_namespace_slash err
type_array_string err
noise_type_number err
noise_type_bool err
noise_type_object err
noise_type_null err
noise_type_two err
noise_unsupported_line err
seed_exponent_min err
seed_exponent_overflow err
valley_constant ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=0ffc7153f1573476
valley_input_linear ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=2c6f1a4588e91719
valley_output_linear ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=8bd7c66018449b79
valley_defaults ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=154df07c9b85c4f5
valley_missing_source err
valley_list_source err
valley_bad_blend err
valley_lowercase_blend err
valley_number_blend err
valley_zero_octaves ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=13a61517ea9743bd
valley_negative_octaves ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=13a61517ea9743bd
valley_fraction_octaves ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=f7560e0989b3e1e9
valley_many_octaves ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=5994a5f98a66bae6
valley_seed_wrap ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=cf8cf9c07fb3eb5a
valley_seed_double ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=aba0af7d4d8896ac
valley_seed_bool ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=cf8cf9c07fb3eb5a
valley_string_strength ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=f8f6b15e7b666866
valley_module_amplitude ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=46a1c54318031757
valley_float_rounding ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=80953898bc56ebf2
valley_zero_grid ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=7b15fa60031743bd
valley_negative_grid ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=176939b733532bf4
valley_long_grid ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=bba50aae60aed6b0
valley_const_sentinel ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=94225aa940417b51
valley_nested ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=617cab86f2abfe88
valley_in_add ok type=terraforged:mountains weight=3fa00000 min=3dcccccd max=3f8ccccd hash=3f3ec99cdc3e7fe3
valley_bad_source err
valley_null_field err
steps_valley_curve err
"#;

    /// `out.txt`'s `6dd607eb/` lines: TerraForged 6dd607eb's generated assets that use `Valley`.
    const SOURCE_ASSET_ORACLE: &str = r#"6dd607eb/mountains_1.json ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=572329a5df58853d
6dd607eb/mountains_2.json ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=ca4419f386874a92
6dd607eb/mountains_3.json ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=166d75b93555c110
"#;

    /// The rest of `out.txt`: Reimagined's 14 `terrain_noise` assets.
    const ASSET_ORACLE: &str = r#"badlands.json ok type=terraforged:badlands weight=3fe00000 min=00000000 max=3f800000 hash=884a7a1eddc2613a
dales.json ok type=terraforged:hills weight=3fc00000 min=00000000 max=3f800000 hash=2a75f650c0578dc8
dolomites.json ok type=terraforged:dolomites weight=3fa00000 min=3f100000 max=3f400000 hash=ad60d1f97d47d31b
hills_1.json ok type=terraforged:hills weight=40000000 min=00000000 max=3f800000 hash=16d5050ff2aba2c1
hills_2.json ok type=terraforged:hills weight=40000000 min=00000000 max=3f800000 hash=e013cf848d1b6c6e
mountains_1.json ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=ccca98604429a1f1
mountains_2.json ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=d61f0888df4f862f
mountains_3.json ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=6161c8d3d400d586
mountains_ridge_1.json ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=2e4f26a6d2b720af
mountains_ridge_2.json ok type=terraforged:mountains weight=3fa00000 min=00000000 max=3f800000 hash=ae6266c76fcd7cce
plains.json ok type=terraforged:flats weight=40200000 min=00000000 max=3f800000 hash=54dcba40e02603d8
plateau.json ok type=terraforged:plateau weight=40000000 min=00000000 max=3f800000 hash=5a96e6ffb27bc660
steppe.json ok type=terraforged:flats weight=3fc00000 min=00000000 max=3f800000 hash=93dd73538804253f
torridonian.json ok type=terraforged:torridonian weight=40200000 min=00000000 max=3f800000 hash=0d9ba14ddc9c79fe
"#;
}
