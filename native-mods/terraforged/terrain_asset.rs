/*
 * MIT License
 *
 * Copyright (c) 2021 TerraForged
 * Copyright (c) 2026 Romoslayer
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

use super::terraforged::{
    CurveFunction, DistanceFunction, Domain, EdgeFunction, ErosionMode, ErosionRidge,
    Interpolation, Module, Source, SourceConfig, SourceKind,
};
use super::terraforged_engine::Terrain;
use super::terraforged_terrain_blender::TerrainNoise;
use serde_json::{Map, Value};

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

pub fn decode_terrain_asset(
    json: &str,
    mut resolve: impl FnMut(&str) -> Option<Terrain>,
) -> Result<TerrainNoise, TerrainAssetError> {
    let value: Value = serde_json::from_str(json)?;
    let object = object(&value)?;
    let terrain_name = string(object, "type")?;
    let terrain = resolve(terrain_name)
        .ok_or_else(|| TerrainAssetError(format!("unknown terrain type {terrain_name:?}")))?;
    Ok(TerrainNoise::new(
        terrain,
        number(object, "weight")?,
        module(field(object, "noise")?)?,
    ))
}

fn module(value: &Value) -> Result<Module, TerrainAssetError> {
    let object = object(value)?;
    let kind = string(object, "type")?;
    match kind {
        "Const" => Ok(Module::constant(number(object, "value")?)),
        "Perlin" => source(object, SourceKind::Perlin),
        "Simplex" => source(object, SourceKind::Simplex),
        "Cubic" => source(object, SourceKind::Cubic),
        "Ridge" => source(object, SourceKind::Ridge),
        "Billow" => source(object, SourceKind::Billow),
        "SimplexRidge" => source(object, SourceKind::SimplexRidge),
        "CellEdge" => Ok(Module::cell_edge_source(
            source_config(object)?,
            number(object, "distance")?,
            edge_function(string(object, "edge_func")?)?,
            distance_function(string(object, "dist_func")?)?,
        )),
        "Alpha" => Ok(child(object, "source")?.alpha(child(object, "alpha")?)),
        "Bias" => Ok(child(object, "source")?.bias(child(object, "bias")?)),
        "Boost" => Ok(child(object, "source")?.boost(unsigned(object, "iterations")?)),
        "Clamp" => Ok(child(object, "source")?.clamp(child(object, "min")?, child(object, "max")?)),
        "Invert" => Ok(child(object, "source")?.invert()),
        "Map" => Ok(child(object, "source")?.map(child(object, "min")?, child(object, "max")?)),
        "Pow" => Ok(child(object, "source")?.power(child(object, "power")?)),
        "PowCurve" => Ok(child(object, "source")?.power_curve(number(object, "power")?)),
        "Scale" => Ok(child(object, "source")?.scale(child(object, "scale")?)),
        "Warp" => Ok(child(object, "source")?.warp(domain(field(object, "domain")?)?)),
        "Blend" => Ok(child(object, "control")?.blend(
            child(object, "lower")?,
            child(object, "upper")?,
            number(object, "midpoint")?,
            number(object, "blend_range")?,
            interpolation(string(object, "interpolation")?)?,
        )),
        "Terrace" => Ok(child(object, "source")?.terrace(
            child(object, "ramp")?,
            child(object, "cliff")?,
            child(object, "ramp_height")?,
            unsigned(object, "steps")?,
            number(object, "blend_range")?,
        )),
        "AdvTerrace" => Ok(child(object, "source")?.advanced_terrace(
            child(object, "modulation")?,
            child(object, "mask")?,
            child(object, "slope")?,
            number(object, "blend_min")?,
            number(object, "blend_max")?,
            unsigned(object, "steps")?,
            unsigned(object, "octaves")?,
        )),
        "Steps" => Ok(child(object, "source")?.steps(
            child(object, "steps")?,
            child(object, "slope_min")?,
            child(object, "slope_max")?,
            curve(field(object, "curve")?)?,
        )),
        "Valley" => Ok(ErosionRidge::full(
            signed(object, "seed")?,
            signed(object, "octaves")?,
            number(object, "strength")?,
            number(object, "grid_size")?,
            number(object, "amplitude")?,
            number(object, "lacunarity")?,
            number(object, "fall_off")?,
            erosion_mode(string(object, "blend")?)?,
        )
        .wrap(child(object, "source")?)),
        "Add" => Ok(Module::add(children(object, "modules")?)),
        "Mult" => Ok(Module::multiply(children(object, "modules")?)),
        "Max" => Ok(Module::max(children(object, "modules")?)),
        other => Err(TerrainAssetError(format!("unknown noise type {other:?}"))),
    }
}

fn source(object: &Map<String, Value>, kind: SourceKind) -> Result<Module, TerrainAssetError> {
    Ok(Module::source(Source::new(kind, source_config(object)?)))
}

fn source_config(object: &Map<String, Value>) -> Result<SourceConfig, TerrainAssetError> {
    Ok(SourceConfig {
        seed: signed(object, "seed")?,
        octaves: unsigned(object, "octaves")?,
        gain: number(object, "gain")?,
        frequency: number(object, "frequency")?,
        lacunarity: number(object, "lacunarity")?,
        interpolation: interpolation(string(object, "interpolation")?)?,
    })
}

fn domain(value: &Value) -> Result<Domain, TerrainAssetError> {
    let object = object(value)?;
    match string(object, "type")? {
        "DomainWarp" => Ok(Domain::domain_warp(
            child(object, "x")?,
            child(object, "y")?,
            child(object, "distance")?,
        )),
        "DirectionWarp" => Ok(Domain::direction(
            child(object, "direction")?,
            child(object, "strength")?,
        )),
        other => Err(TerrainAssetError(format!("unknown domain type {other:?}"))),
    }
}

fn curve(value: &Value) -> Result<CurveFunction, TerrainAssetError> {
    let object = object(value)?;
    match string(object, "type")? {
        "Interpolation" => Ok(CurveFunction::Interpolation(interpolation(string(
            object, "mode",
        )?)?)),
        other => Err(TerrainAssetError(format!("unknown curve type {other:?}"))),
    }
}

fn interpolation(name: &str) -> Result<Interpolation, TerrainAssetError> {
    match name {
        "LINEAR" => Ok(Interpolation::Linear),
        "CURVE3" => Ok(Interpolation::Curve3),
        "CURVE4" => Ok(Interpolation::Curve4),
        other => Err(TerrainAssetError(format!(
            "unknown interpolation {other:?}"
        ))),
    }
}

fn erosion_mode(name: &str) -> Result<ErosionMode, TerrainAssetError> {
    match name {
        "CONSTANT" => Ok(ErosionMode::Constant),
        "INPUT_LINEAR" => Ok(ErosionMode::InputLinear),
        "OUTPUT_LINEAR" => Ok(ErosionMode::OutputLinear),
        other => Err(TerrainAssetError(format!(
            "unknown erosion blend {other:?}"
        ))),
    }
}

fn distance_function(name: &str) -> Result<DistanceFunction, TerrainAssetError> {
    match name {
        "EUCLIDEAN" => Ok(DistanceFunction::Euclidean),
        "MANHATTAN" => Ok(DistanceFunction::Manhattan),
        "NATURAL" => Ok(DistanceFunction::Natural),
        other => Err(TerrainAssetError(format!(
            "unknown distance function {other:?}"
        ))),
    }
}

fn edge_function(name: &str) -> Result<EdgeFunction, TerrainAssetError> {
    match name {
        "DISTANCE_2" => Ok(EdgeFunction::Distance2),
        "DISTANCE_2_ADD" => Ok(EdgeFunction::Distance2Add),
        "DISTANCE_2_SUB" => Ok(EdgeFunction::Distance2Sub),
        "DISTANCE_2_MUL" => Ok(EdgeFunction::Distance2Mul),
        "DISTANCE_2_DIV" => Ok(EdgeFunction::Distance2Div),
        other => Err(TerrainAssetError(format!(
            "unknown edge function {other:?}"
        ))),
    }
}

fn child(object: &Map<String, Value>, name: &'static str) -> Result<Module, TerrainAssetError> {
    module(field(object, name)?)
}

fn children(
    object: &Map<String, Value>,
    name: &'static str,
) -> Result<Vec<Module>, TerrainAssetError> {
    field(object, name)?
        .as_array()
        .ok_or_else(|| TerrainAssetError(format!("field {name:?} must be an array")))?
        .iter()
        .map(module)
        .collect()
}

fn object(value: &Value) -> Result<&Map<String, Value>, TerrainAssetError> {
    value
        .as_object()
        .ok_or_else(|| TerrainAssetError("expected an object".into()))
}

fn field<'a>(
    object: &'a Map<String, Value>,
    name: &'static str,
) -> Result<&'a Value, TerrainAssetError> {
    object
        .get(name)
        .ok_or_else(|| TerrainAssetError(format!("missing field {name:?}")))
}

fn string<'a>(
    object: &'a Map<String, Value>,
    name: &'static str,
) -> Result<&'a str, TerrainAssetError> {
    field(object, name)?
        .as_str()
        .ok_or_else(|| TerrainAssetError(format!("field {name:?} must be a string")))
}

fn number(object: &Map<String, Value>, name: &'static str) -> Result<f32, TerrainAssetError> {
    field(object, name)?
        .as_f64()
        .map(|value| value as f32)
        .ok_or_else(|| TerrainAssetError(format!("field {name:?} must be a number")))
}

fn signed(object: &Map<String, Value>, name: &'static str) -> Result<i32, TerrainAssetError> {
    let value = field(object, name)?
        .as_i64()
        .ok_or_else(|| TerrainAssetError(format!("field {name:?} must be an integer")))?;
    i32::try_from(value).map_err(|_| TerrainAssetError(format!("field {name:?} is out of range")))
}

fn unsigned(object: &Map<String, Value>, name: &'static str) -> Result<usize, TerrainAssetError> {
    let value = field(object, name)?
        .as_u64()
        .ok_or_else(|| TerrainAssetError(format!("field {name:?} must be unsigned")))?;
    usize::try_from(value).map_err(|_| TerrainAssetError(format!("field {name:?} is out of range")))
}

#[cfg(test)]
mod tests {
    use super::super::terraforged_engine::{TerrainCategory, TerrainProperties};
    use super::*;

    fn constant(value: f32) -> Value {
        serde_json::json!({"type": "Const", "value": value})
    }

    fn values(module: &Module) -> [u32; 4] {
        [
            (0.0, 0.0),
            (12.5, -4.25),
            (-301.75, 87.125),
            (4096.0, -8192.0),
        ]
        .map(|(x, y)| module.value(x, y).to_bits())
    }

    #[test]
    fn decodes_current_source_and_domain_nodes() {
        let source = serde_json::json!({
            "type": "Perlin", "seed": -91, "octaves": 3, "gain": 0.65,
            "frequency": 0.0078125, "lacunarity": 2.4, "interpolation": "CURVE3"
        });
        let json = serde_json::json!({
            "type": "Warp",
            "source": {"type": "SimplexRidge", "seed": 17, "octaves": 2, "gain": 0.5,
                "frequency": 0.03125, "lacunarity": 2.0, "interpolation": "LINEAR"},
            "domain": {"type": "DomainWarp", "x": source, "y": {
                "type": "CellEdge", "seed": 37, "octaves": 1, "gain": 0.5,
                "frequency": 0.01, "lacunarity": 2.0, "interpolation": "CURVE3",
                "distance": 0.75, "edge_func": "DISTANCE_2", "dist_func": "EUCLIDEAN"
            }, "distance": constant(64.0)}
        });
        let decoded = module(&json).unwrap();
        let expected = Module::source(Source::new(
            SourceKind::SimplexRidge,
            SourceConfig {
                seed: 17,
                octaves: 2,
                gain: 0.5,
                frequency: 0.03125,
                lacunarity: 2.0,
                interpolation: Interpolation::Linear,
            },
        ))
        .warp(Domain::domain_warp(
            Module::source(Source::new(
                SourceKind::Perlin,
                SourceConfig {
                    seed: -91,
                    octaves: 3,
                    gain: 0.65,
                    frequency: 0.0078125,
                    lacunarity: 2.4,
                    interpolation: Interpolation::Curve3,
                },
            )),
            Module::cell_edge_source(
                SourceConfig {
                    seed: 37,
                    octaves: 1,
                    gain: 0.5,
                    frequency: 0.01,
                    lacunarity: 2.0,
                    interpolation: Interpolation::Curve3,
                },
                0.75,
                EdgeFunction::Distance2,
                DistanceFunction::Euclidean,
            ),
            Module::constant(64.0),
        ));
        assert_eq!(values(&decoded), values(&expected));
    }

    #[test]
    fn decodes_current_modifier_selector_and_combiner_nodes() {
        let blend = serde_json::json!({
            "type": "Blend", "control": constant(0.45), "lower": constant(0.2),
            "upper": constant(0.8), "midpoint": 0.5, "blend_range": 0.3,
            "interpolation": "CURVE3"
        });
        let json = serde_json::json!({"type": "Clamp", "min": constant(0.0), "max": constant(1.0),
            "source": {"type": "Add", "modules": [
                {"type": "Alpha", "source": blend, "alpha": constant(0.75)},
                {"type": "Bias", "source": {"type": "Invert", "source": constant(0.4)},
                    "bias": constant(-0.1)},
                {"type": "Pow", "source": {"type": "Scale", "source": constant(0.5),
                    "scale": constant(0.6)}, "power": constant(2.0)},
                {"type": "PowCurve", "source": constant(0.7), "power": 0.8},
                {"type": "Boost", "source": constant(0.3), "iterations": 2},
                {"type": "Max", "modules": [constant(0.1), {"type": "Mult",
                    "modules": [constant(0.2), constant(0.5)]}]}
            ]}
        });
        let decoded = module(&json).unwrap();
        let expected = Module::add(vec![
            Module::constant(0.45)
                .blend(
                    Module::constant(0.2),
                    Module::constant(0.8),
                    0.5,
                    0.3,
                    Interpolation::Curve3,
                )
                .alpha(Module::constant(0.75)),
            Module::constant(0.4).invert().bias(Module::constant(-0.1)),
            Module::constant(0.5)
                .scale(Module::constant(0.6))
                .power(Module::constant(2.0)),
            Module::constant(0.7).power_curve(0.8),
            Module::constant(0.3).boost(2),
            Module::max(vec![
                Module::constant(0.1),
                Module::multiply(vec![Module::constant(0.2), Module::constant(0.5)]),
            ]),
        ])
        .clamp(Module::constant(0.0), Module::constant(1.0));
        assert_eq!(values(&decoded), values(&expected));
    }

    #[test]
    fn decodes_current_terrace_nodes_and_asset_wrapper() {
        let json = serde_json::json!({"type": "terraforged:hills", "weight": 1.25, "noise": {
            "type": "Steps", "source": {"type": "AdvTerrace", "source": {
                "type": "Terrace", "source": constant(0.63), "ramp": constant(0.2),
                "cliff": constant(0.7), "ramp_height": constant(0.8), "steps": 5,
                "blend_range": 0.25}, "modulation": constant(0.1), "mask": constant(0.9),
                "slope": constant(0.4), "blend_min": 0.1, "blend_max": 0.8,
                "steps": 6, "octaves": 2}, "steps": constant(5.0),
                "slope_min": constant(0.05), "slope_max": constant(0.75),
                "curve": {"type": "Interpolation", "mode": "LINEAR"}}
        });
        let terrain =
            Terrain::configured(31, "hills", TerrainCategory::Highland, 0.5, false, false);
        let decoded = decode_terrain_asset(&json.to_string(), |name| {
            (name == "terraforged:hills").then(|| terrain.clone())
        })
        .unwrap();
        assert_eq!(decoded.terrain().name(), "hills");
        assert_eq!(decoded.weight().to_bits(), 1.25_f32.to_bits());
        assert!(!decoded.terrain().is_mountain());
        assert!(
            values(decoded.noise())
                .iter()
                .all(|value| *value != f32::NAN.to_bits())
        );
    }

    #[test]
    fn decodes_engine_valley_erosion_nodes() {
        let source = serde_json::json!({
            "type": "Perlin", "seed": 5, "octaves": 2, "gain": 0.5,
            "frequency": 0.004, "lacunarity": 2.2, "interpolation": "CURVE3"
        });
        let source_module = || {
            Module::source(Source::new(
                SourceKind::Perlin,
                SourceConfig {
                    seed: 5,
                    octaves: 2,
                    gain: 0.5,
                    frequency: 0.004,
                    lacunarity: 2.2,
                    interpolation: Interpolation::Curve3,
                },
            ))
        };
        for (blend, mode) in [
            ("CONSTANT", ErosionMode::Constant),
            ("INPUT_LINEAR", ErosionMode::InputLinear),
            ("OUTPUT_LINEAR", ErosionMode::OutputLinear),
        ] {
            let json = serde_json::json!({
                "type": "Valley", "blend": blend, "amplitude": 0.15, "fall_off": 0.8,
                "grid_size": 128, "lacunarity": 3.1, "octaves": 2, "seed": 310154,
                "strength": 0.65, "source": source
            });
            let expected = ErosionRidge::full(310154, 2, 0.65, 128.0, 0.15, 3.1, 0.8, mode)
                .wrap(source_module());
            assert_eq!(values(&module(&json).unwrap()), values(&expected));
        }
        let error = match module(&serde_json::json!({
            "type": "Valley", "blend": "SIDEWAYS", "amplitude": 0.15, "fall_off": 0.8,
            "grid_size": 128, "lacunarity": 3.1, "octaves": 2, "seed": 1,
            "strength": 0.65, "source": constant(0.5)
        })) {
            Ok(_) => panic!("unknown erosion blend decoded"),
            Err(error) => error,
        };
        assert_eq!(error.to_string(), "unknown erosion blend \"SIDEWAYS\"");
    }

    #[test]
    fn rejects_unknown_specs_and_terrain_types() {
        let error = match module(&serde_json::json!({"type": "Unknown"})) {
            Ok(_) => panic!("unknown noise spec decoded"),
            Err(error) => error,
        };
        assert_eq!(error.to_string(), "unknown noise type \"Unknown\"");
        let error = match decode_terrain_asset(
            r#"{"type":"terraforged:missing","weight":1,"noise":{"type":"Const","value":0}}"#,
            |_| None,
        ) {
            Ok(_) => panic!("unknown terrain type decoded"),
            Err(error) => error,
        };
        assert_eq!(
            error.to_string(),
            "unknown terrain type \"terraforged:missing\""
        );
    }
}
