import com.terraforged.engine.Seed;
import com.terraforged.engine.settings.Settings;
import com.terraforged.mod.worldgen.noise.climate.ClimateNoise;
import com.terraforged.mod.worldgen.noise.climate.ClimateSample;

// Samples TerraForged 6dd607eb's ClimateNoise with the settings NoiseGenerator's
// createContinentNoise gives it. Classpath, in order: the pinned ClimateNoise,
// ClimateSample, NoiseSample, MathUtil and CellShape compiled against the seeded
// Noise2D; Engine's seeded Moisture and Temperature; that Noise2D with Cereal; then
// the official 0.3.1 jar for Engine 0.3.0's Seed, settings and BiomeType (with its
// biome_types.bin). Output: runtime seed, x and y bits, then biome noise, edge
// noise, moisture and temperature bits, then the climate type's ordinal.
public class TerraForgedClimateNoiseOracle {
    public static void main(String[] args) {
        var settings = new Settings();
        settings.climate.biomeShape.biomeSize = 220;
        settings.climate.temperature.falloff = 2;
        settings.climate.temperature.bias = 0.1f;
        settings.climate.moisture.falloff = 1;
        settings.climate.moisture.bias = -0.05f;
        var noise = new ClimateNoise(new Seed(12345), settings);
        float[][] points = {{0f, 0f}, {12.5f, -4.25f}, {-301.75f, 87.125f}, {4096f, -8192f}};
        for (int seed : new int[] {0, 17, -918273}) {
            for (float[] p : points) {
                var sample = new ClimateSample();
                noise.sample(seed, p[0], p[1], sample);
                System.out.println(seed + " " + Float.floatToRawIntBits(p[0]) + " " + Float.floatToRawIntBits(p[1])
                        + " " + Float.floatToRawIntBits(sample.biomeNoise)
                        + " " + Float.floatToRawIntBits(sample.biomeEdgeNoise)
                        + " " + Float.floatToRawIntBits(sample.moisture)
                        + " " + Float.floatToRawIntBits(sample.temperature));
                System.out.println(sample.climateType.ordinal());
            }
        }
    }
}
