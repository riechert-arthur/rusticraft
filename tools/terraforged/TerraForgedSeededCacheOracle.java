import com.terraforged.noise.Module;
import com.terraforged.noise.Source;

public class TerraForgedSeededCacheOracle {
    public static void main(String[] args) {
        Module source = Source.builder()
                .seed(Integer.MAX_VALUE)
                .frequency(0.125)
                .octaves(3)
                .gain(0.65)
                .perlin2();
        Module cache = source.cache();
        int[] seeds = {17, -91, -91, 17, Integer.MAX_VALUE, 0};
        float[][] points = {
                {1.25f, -3.5f},
                {1.25f, -3.5f},
                {12.34f, 56.78f},
                {12.34f, 56.78f},
                {Float.NaN, 0},
                {Float.NaN, 0},
        };
        for (int i = 0; i < seeds.length; i++) {
            System.out.printf("%08x%n", Float.floatToRawIntBits(
                    cache.getValue(seeds[i], points[i][0], points[i][1])));
        }

        Module zeroCache = source.cache();
        System.out.printf("zero=%08x,%08x%n",
                Float.floatToRawIntBits(zeroCache.getValue(17, -0.0f, 0)),
                Float.floatToRawIntBits(zeroCache.getValue(-91, 0.0f, 0)));
    }
}
