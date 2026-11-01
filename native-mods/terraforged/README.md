# TerraForged for Rusticraft

An exact Rust translation of
[TerraForged](https://github.com/TerraForged/TerraForged), delivered as a stacked series of small
native-mod changes. This foundation ports the deterministic Noise2D primitives used by later terrain
modules. It intentionally does not alter world generation yet.

The next layer adds TerraForged's octave configuration and Perlin, Perlin2, legacy simplex,
improved simplex and cubic sources, including the original explicit-seed behavior.

The graph layer translates TerraForged's constants, source nodes, core modifiers and arithmetic
combiners. Its tests preserve Engine 0.3.0's reported bounds as well as raw sample bits, including
the original combiner bound behavior.

## Install

Use `native-mods/terraforged` as a local native-mod bundle in the Rusticraft launcher:

```sh
horizon-launcher native-build native-mods/terraforged --trust-native-code
```

Native mods compile into a separate game build. Back up a world before opening it with a different
generator, and keep the mod enabled for that world's unexplored chunks.

## Development

The noise layer is ordinary Rust and can be tested without Minecraft resources:

```sh
rustc --edition 2024 --test native-mods/terraforged/noise.rs -o /tmp/terraforged-tests
/tmp/terraforged-tests
```

The mixin pins Rusticraft commit `1b6e55d4f3bd14079551bcad173d0fac8f8225bb`. Translation inputs are
TerraForged commit `6dd607ebfd41a3b7274e090b1f2cf565d53d1d44` and Engine 0.3.0 bytecode
shaded into the official `TerraForged-1.18.2-0.3.1-alpha-2.jar`. Tests compare raw `f32` bits with
values produced by those Java classes.
