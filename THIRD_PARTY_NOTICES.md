# Third-party notices

## TerraForged

`native-mods/terraforged` translates
[TerraForged](https://github.com/TerraForged/TerraForged/tree/6dd607ebfd41a3b7274e090b1f2cf565d53d1d44)
and its Engine 0.3.0/Noise2D dependencies by TerraForged. The unavailable dependency source was
recovered from bytecode shipped in TerraForged's official 0.3.1 alpha 2 jar. The code is licensed
under MIT; its notice is preserved in `native-mods/terraforged/LICENSE-TERRAFORGED`.
The terrain-noise decoder's `Valley` spec in `native-mods/terraforged/terrain_asset.rs` is Engine 0.3.0's erosion
`Ridge.spec()` and `Ridge.create`, as TerraForged's `NoiseCodec` registers it (TerraForged Reimagined, credited below,
no longer registers it).
TerraForged's generated mountain terrain-noise data files `mountains_1.json`, `mountains_2.json` and `mountains_3.json`
(`generated/resources/default/data/terraforged/worldgen/terrain/noise/`, which use that `Valley` node; TerraForged
Reimagined's copies differ) are bundled unchanged in `native-mods/terraforged/data/worldgen/terrain_noise/`.

The seed-first source API additionally follows
[Noise2D 2cb48431148a321f74319d7ef0ea61b26bf8e59c](https://github.com/TerraForged/Noise2D/tree/2cb48431148a321f74319d7ef0ea61b26bf8e59c),
MIT, copyright (c) 2018 dags and (c) 2020 TerraForged. Its license is preserved in
`native-mods/terraforged/LICENSE-NOISE2D`. This is the seed-first API used by the current
TerraForged source, distinct from the older shaded dependency's seed-replacement API.

The heightmap smoothing, the erosion brushes and filter and the eroded-region cache in
`native-mods/terraforged/erosion.rs` (`HeightmapSmoothing`, `HeightmapSmoothingSettings`, `ErosionBrushes`,
`ErosionFilterSettings`, `ErosionFilter`, `ErosionRegionLayout`, `ErosionRegionCache`), the eroded region build in
`native-mods/terraforged/eroded_noise_generator.rs` (`ErodedNoiseGenerator`), the terrain cache, the generator queries, the chunk fill, the biome chunk fill, the biome pool fallback and the surface-biome decisions in `terrain_cache.rs`, `generator_facade.rs`, `chunk_fill.rs`, `biome_chunk_fill.rs`, `cave_biome_sampler.rs`, `biome_map.rs`, `biome_sampler.rs`, `current_climate.rs`, `current_biome_sampler.rs`, `current_surface_biomes.rs`, `biome_registry.rs`, `live_biomes.rs`, `current_settings.rs`, `terrain_overrides.rs`, `terrain_asset.rs`, `terrain_asset_registry.rs` and `current_terrain_resources.rs` are adapted from
[TerraForged Reimagined a24d4503064780790e975cd17495866ae13eef11](https://github.com/Romoslayer/TerraForged-Reimagined/tree/a24d4503064780790e975cd17495866ae13eef11)
(`HeightmapSmoothing` and `TerraSettings.Smoothing`; `ErosionFilter`'s erosion-brush layout, its
droplet simulation with the region-wide `apply`, and `TerraSettings.Erosion`; `ErodedNoiseGenerator`'s region
layout, `getRegion` region cache, `buildRegion`, `erodedHeightNoise`, `generate` and its `INoiseGenerator` delegations, `getNoiseSample` and `sample` included; `TerrainCache` with `getErodedHeightNoise`; and `Generator`'s seed capture (`createState`, `Seeds.get`), chunk-data, hint and drop calls, level queries, `getBaseHeight`, `getBaseColumn` and `buildTerrain`'s `combineAsync` hand-off to `fillFromNoise`; `ChunkUtil.fillChunk`, `fillSection` and `primeHeightmaps` as `Generator.fillFromNoise` calls them; `ChunkUtil.fillNoiseBiomes`, `isCaveBiomeSection`, `isCaveBiomeQuart` and `BiomeBuffer2D` as `Generator.createBiomes` runs them; and `CaveBiomeSampler` with its coverage mask (`getLayerBiome`), empty-pool fallback and `WeightMap`; `BiomeMapManager.fillEmptyClimates` with its climate `distance`; and `BiomeSampler`'s `lowlandPool`, `cellRoll`, `isSteep`, `freezes`, `keepsSnowCover`, `fromPool` and `getInitialBiome` with its mountain-usage gate and plains fallback; and `ClimateNoise` with its biome-edge warp and macro noise grouping, the reconstructed Engine `Seed`'s pre-increment draws and `TerraSettings.Climate`'s defaults; and `IBiomeSampler.Sampler` with `createClimate`, joining the continent noise to that `ClimateNoise`; and `BiomeSampler.getBiomeOverride` with its `COAST - BEACH` beach band; and `BiomeSampler.sampleBiome`, joining the climate sample, `getInitialBiome` and `getBiomeOverride`; and `Generator.createBiomes` with its terrain-cache hint and `Source`'s `getSurfaceBiome`, `getLayerBiome` and `compute`, which feed `fillNoiseBiomes` the surface and cave biome samplers; and `BiomeMapManager`'s `getOverworldBiomes`, `getWeightsMap`, `getBiomeWeights`, `buildBiomeMap` and its dry and highland pools (`filtered`), with `BiomeUtil`'s `getOverworldBiomes`, `isNonSurface`, `isClimateBiome`, `getType` and `getByClimate` and the default `terraforged:non_surface`, `wetland` and `highland` tags; and the registry lookups of `getInitialBiome`'s plains fallback and `getBiomeOverride` (`BiomeMapManager.get`), with `Source`'s `new CaveBiomeSampler(800, manager)` over the registry's cave biomes; and the defaults of `TerraSettings.Miscellaneous` and `TerraSettings.Rivers.Wetlands`; and `GeneratorPreset.applyTerrainOverrides` and `isMountain` with the defaults of `TerraSettings.Terrain`, `General` and `TerrainEntry`; and `TerrainNoise.CODEC` and `NoiseCodec.CODEC` with `SuperCodec`, `DataUtil.toData`, Cereal's spec reading and the noise specs `NoiseSpec` registers; and the asset `TerrainType` (`DIRECT`, `forName`) with its engine `TerrainType.get` and `getOrCreate`, and `RegistryKey.entries` and `entryIds`, loading the terrain-type and terrain-noise registries; and its 18 `terrain_type` data files, bundled unchanged in `native-mods/terraforged/data/worldgen/terrain_type/`; and its flatland, hill and mountain `terrain_noise` data files `plains.json`, `steppe.json`, `dales.json`, `hills_1.json`, `hills_2.json`, `mountains_ridge_1.json` and `mountains_ridge_2.json`, bundled unchanged in `native-mods/terraforged/data/worldgen/terrain_noise/`), an unofficial port of TerraForged to Minecraft 26.3,
MIT, copyright (c) 2020 TerraForged and (c) 2026 Romoslayer. Its license is preserved in
`native-mods/terraforged/LICENSE-TERRAFORGED-REIMAGINED`.

## wgpu-hal

`third_party/wgpu-hal` is [wgpu-hal](https://github.com/gfx-rs/wgpu) 30.0.1 by the gfx-rs
developers, with upstream fixes applied
([gfx-rs/wgpu#10124](https://github.com/gfx-rs/wgpu/pull/10124),
[gfx-rs/wgpu#10714](https://github.com/gfx-rs/wgpu/pull/10714)). It is licensed under MIT or
Apache-2.0; both licenses are in that folder (`LICENSE.MIT`, `LICENSE.APACHE`).

## naga

`third_party/naga` is [naga](https://github.com/gfx-rs/wgpu) 30.0.1 by the gfx-rs developers, with
fixes to its GLSL frontend that shader packs need, each applied in the commit that needed it. It is licensed under MIT or Apache-2.0; both licenses
are in that folder (`LICENSE.MIT`, `LICENSE.APACHE`).

## Pixelify Sans

`crates/horizon-theme/fonts/PixelifySans-*.ttf` is [Pixelify Sans](https://github.com/eifetx/Pixelify-Sans)
by The Pixelify Sans Project Authors, licensed under the SIL Open Font License 1.1
(`crates/horizon-theme/fonts/OFL-PixelifySans.txt`).

## Pixelarticons

`crates/horizon-theme/icons/pixel/*.svg` are icons from
[Pixelarticons](https://github.com/halfmage/pixelarticons) by Gerrit Halfmann, under the MIT
license (`crates/horizon-theme/icons/pixel/LICENSE-pixelarticons.txt`). `star.svg` is Horizon's own, drawn on
the same grid.

## AMD FidelityFX SDK (FSR 3.1 frame generation)

`crates/horizon-client-render/src/gfx/frame_gen/optical_flow.wgsl` and `interpolation.wgsl` are
ports to WGSL of the optical flow and frame interpolation shaders of the
[AMD FidelityFX SDK](https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK) 1.1.4
(`sdk/include/FidelityFX/gpu/{opticalflow,frameinterpolation}`, at commit
[c6efa6bf7f20](https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK/tree/c6efa6bf7f2027b3ec94f28578bb5965eabb9e55)),
and `optical_flow.rs` follows the SDK's `sdk/src/components/opticalflow/ffx_opticalflow.cpp`
(its passes, their order and their dispatch sizes). Copyright (C) 2024 Advanced Micro Devices,
Inc., under the MIT license, whose text heads each file. AMD and FidelityFX are AMD's
trademarks; Horizon is not affiliated with AMD.

## Mod loader logos

The launcher shows the logo of the mod loader an instance plays with.

- `launcher/assets/logos/quilt.png` is the Quilt mini icon (`brand/128png/quilt_mini_icon_transparent.png`)
  from [QuiltMC/art](https://github.com/QuiltMC/art), by the Quilt Project, dedicated to the public
  domain under [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/).
- `launcher/assets/logos/neoforge.png` is NeoForged's icon (`art/neoforged_icon_16x16.png`) from
  [neoforged/.github](https://github.com/neoforged/.github/tree/main/art), © the NeoForged team,
  created by Ridanisaurus, under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). It is
  unmodified (the launcher scales it up by whole pixels).
- Fabric's and Forge's icons are not shipped: neither publishes a license for its logo. The launcher
  fetches them from their own sites when an instance needs them
  ([fabric-loader's icon](https://github.com/FabricMC/fabric-loader/blob/master/src/main/resources/assets/fabricloader/icon.png),
  [files.minecraftforge.net's](https://files.minecraftforge.net/)).

Each logo belongs to its project. Showing it says which loader an instance runs; it does not mean
the project endorses Horizon.

## Sign in with Microsoft

The launcher's sign-in buttons follow Microsoft's published branding guidelines for apps that sign in
with Microsoft accounts
([Sign in with Microsoft branding](https://learn.microsoft.com/en-us/entra/identity-platform/howto-add-branding-in-apps)).
The four-square Microsoft symbol in `launcher/src/microsoft.rs` reproduces the squares, positions and
colours of Microsoft's official `ms-symbollockup_mssymbol_19.svg` from that page, unmodified. Microsoft
and its logo are trademarks of the Microsoft group of companies; Horizon is not endorsed by Microsoft.
