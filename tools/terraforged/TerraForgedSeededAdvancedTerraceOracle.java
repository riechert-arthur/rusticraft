import com.terraforged.noise.Module;
import com.terraforged.noise.Source;

public class TerraForgedSeededAdvancedTerraceOracle {
    public static void main(String[] args) {
        Module source = Source.builder().seed(801).frequency(1.0 / 96.0).octaves(4).gain(0.5).perlin();
        Module modulation = Source.builder().seed(802).frequency(1.0 / 80.0).octaves(3).gain(0.5).simplex();
        Module mask = Source.builder().seed(803).frequency(1.0 / 128.0).octaves(2).gain(0.975).ridge();
        Module slope = Source.builder().seed(804).frequency(1.0 / 64.0).octaves(2).gain(0.5).perlin();
        Module terrace = source.terrace(modulation, mask, slope, 0.2, 0.8, 8, 3);
        for (int seed : new int[]{0, 17, -91, Integer.MAX_VALUE}) {
            long hash = 0xcbf29ce484222325L;
            for (float[] point : new float[][]{{0,0}, {0.125f,-0.75f}, {12.34f,56.78f}, {-1234.5f,987.25f}}) {
                hash ^= Integer.toUnsignedLong(Float.floatToRawIntBits(terrace.getValue(seed, point[0], point[1])));
                hash *= 0x100000001b3L;
            }
            System.out.printf("0x%016x%n", hash);
        }
    }
}
