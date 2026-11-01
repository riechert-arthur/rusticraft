import com.terraforged.engine.Seed;
import com.terraforged.engine.settings.ClimateSettings;
import com.terraforged.engine.world.biome.type.BiomeType;
import com.terraforged.engine.world.climate.Moisture;
import com.terraforged.engine.world.climate.Temperature;
import com.terraforged.noise.Module;
import com.terraforged.noise.Source;
import com.terraforged.noise.domain.Domain;
import com.terraforged.noise.util.NoiseUtil;

public final class TerraForgedCurrentClimateOracle {
    private static final float JITTER = 0.8F;

    public static void main(String[] args) {
        for (int[] variant : new int[][]{{0, 0}, {4, 24}}) {
            ClimateSettings settings = settings(variant[0], variant[1]);
            Climate climate = new Climate(new Seed(12345), settings);
            for (int runtimeSeed : new int[]{0, 17, -918273}) {
                for (float[] point : new float[][]{{0F, 0F}, {12.5F, -4.25F}, {-301.75F, 87.125F}, {4096F, -8192F}}) {
                    Sample sample = climate.sample(runtimeSeed, point[0], point[1]);
                    System.out.printf("%d %d %d %d %d %d %d %d %d%n", variant[0], variant[1], runtimeSeed,
                            Float.floatToRawIntBits(point[0]), Float.floatToRawIntBits(point[1]),
                            Float.floatToRawIntBits(sample.biomeNoise), Float.floatToRawIntBits(sample.biomeEdgeNoise),
                            Float.floatToRawIntBits(sample.moisture), Float.floatToRawIntBits(sample.temperature));
                    System.out.println(sample.type.ordinal());
                }
            }
        }
    }

    private static ClimateSettings settings(int macroSize, int edgeStrength) {
        ClimateSettings settings = new ClimateSettings();
        set(settings.temperature, 0, 7, 2, 0F, 0.98F, 0.1F);
        set(settings.moisture, 8461, 6, 1, 0F, 1F, -0.05F);
        settings.biomeShape.biomeSize = 220;
        settings.biomeShape.macroNoiseSize = macroSize;
        settings.biomeShape.biomeWarpScale = 150;
        settings.biomeShape.biomeWarpStrength = 80;
        settings.biomeEdgeShape.type = Source.SIMPLEX;
        settings.biomeEdgeShape.scale = 24;
        settings.biomeEdgeShape.octaves = 2;
        settings.biomeEdgeShape.gain = 0.5F;
        settings.biomeEdgeShape.lacunarity = 2.65F;
        settings.biomeEdgeShape.strength = edgeStrength;
        return settings;
    }

    private static void set(ClimateSettings.RangeValue value, int seedOffset, int scale, int falloff,
                            float min, float max, float bias) {
        value.seedOffset = seedOffset;
        value.scale = scale;
        value.falloff = falloff;
        value.min = min;
        value.max = max;
        value.bias = bias;
    }

    private static final class Climate {
        private final float frequency;
        private final Domain warp;
        private final Domain edgeWarp;
        private final int macroSize;
        private final Module moisture;
        private final Module temperature;

        private Climate(Seed seed, ClimateSettings settings) {
            int biomeSize = settings.biomeShape.biomeSize;
            float biomeFrequency = 1F / biomeSize;
            int moistureScale = NoiseUtil.round(settings.moisture.scale * 2.5F * biomeSize * biomeFrequency);
            int temperatureScale = NoiseUtil.round(settings.temperature.scale * biomeSize * biomeFrequency);
            int climateSeed = seed.next();
            Seed moistureSeed = seed.offset(settings.moisture.seedOffset);
            moisture = settings.moisture.apply(new Moisture(moistureSeed.next(), moistureScale, settings.moisture.falloff))
                    .warp(moistureSeed.next(), Math.max(1, moistureScale / 2), 1, moistureScale / 4D)
                    .warp(moistureSeed.next(), Math.max(1, moistureScale / 6), 2, moistureScale / 12D);
            Seed temperatureSeed = seed.offset(settings.temperature.seedOffset);
            temperature = settings.temperature.apply(new Temperature(1F / temperatureScale, settings.temperature.falloff))
                    .warp(temperatureSeed.next(), temperatureScale * 4, 2, temperatureScale * 4)
                    .warp(temperatureSeed.next(), temperatureScale, 1, temperatureScale);
            warp = Domain.warp(
                    Source.build(seed.next(), settings.biomeShape.biomeWarpScale, 3).lacunarity(2.4).gain(0.3).simplex2(),
                    Source.build(seed.next(), settings.biomeShape.biomeWarpScale, 3).lacunarity(2.4).gain(0.3).simplex2(),
                    Source.constant(settings.biomeShape.biomeWarpStrength * 0.75));
            edgeWarp = settings.biomeEdgeShape.strength <= 0 ? null : Domain.warp(
                    settings.biomeEdgeShape.build(climateSeed + 7919),
                    settings.biomeEdgeShape.build(climateSeed + 104729),
                    Source.constant(settings.biomeEdgeShape.strength));
            frequency = biomeFrequency;
            macroSize = Math.max(0, settings.biomeShape.macroNoiseSize);
        }

        private Sample sample(int seed, float x, float y) {
            float px = warp.getX(seed, x, y);
            float py = warp.getY(seed, x, y);
            if (edgeWarp != null) {
                float edgeX = edgeWarp.getX(seed, px, py);
                float edgeY = edgeWarp.getY(seed, px, py);
                px = edgeX;
                py = edgeY;
            }
            return sampleBiome(seed, px * frequency, py * frequency);
        }

        private Sample sampleBiome(int seed, float x, float y) {
            float nearestX = x;
            float nearestY = y;
            int nearestHash = 0;
            int nearestCellX = 0;
            int nearestCellY = 0;
            float distance = Float.MAX_VALUE;
            float distance2 = Float.MAX_VALUE;
            for (int cellY = NoiseUtil.floor(y) - 1; cellY <= NoiseUtil.floor(y) + 2; cellY++) {
                for (int cellX = NoiseUtil.floor(x) - 1; cellX <= NoiseUtil.floor(x) + 2; cellX++) {
                    int hash = NoiseUtil.hash2D(seed, cellX, cellY);
                    float pointX = cellX + random(NoiseUtil.hash(hash, NoiseUtil.X_PRIME)) * JITTER;
                    float pointY = cellY + random(NoiseUtil.hash(hash, NoiseUtil.Y_PRIME)) * JITTER;
                    float candidate = NoiseUtil.dist2(x, y, pointX, pointY);
                    if (candidate < distance) {
                        distance2 = distance;
                        distance = candidate;
                        nearestX = pointX;
                        nearestY = pointY;
                        nearestHash = hash;
                        nearestCellX = cellX;
                        nearestCellY = cellY;
                    } else if (candidate < distance2) {
                        distance2 = candidate;
                    }
                }
            }
            int biomeHash = macroSize <= 0 ? nearestHash : NoiseUtil.hash2D(seed,
                    Math.floorDiv(nearestCellX, macroSize), Math.floorDiv(nearestCellY, macroSize));
            float biomeNoise = random(NoiseUtil.hash(biomeHash, 1236785));
            float edgeNoise = 1F - NoiseUtil.sqrt(distance / distance2);
            float moistureValue = moisture.getValue(seed, nearestX, nearestY);
            float temperatureValue = temperature.getValue(seed, nearestX, nearestY);
            BiomeType type = BiomeType.get(temperatureValue, moistureValue);
            if (type == BiomeType.COLD_STEPPE || type == BiomeType.STEPPE) type = BiomeType.GRASSLAND;
            return new Sample(biomeNoise, edgeNoise, moistureValue, temperatureValue, type);
        }
    }

    private static float random(int value) {
        value ^= 1619;
        value ^= 31337;
        float mapped = (value * value * value * 60493) / 2147483650F;
        float clamped = mapped < -1F ? -1F : mapped > 1F ? 1F : mapped;
        return (clamped + 1F) / 2F;
    }

    private record Sample(float biomeNoise, float biomeEdgeNoise, float moisture, float temperature, BiomeType type) {}
}
