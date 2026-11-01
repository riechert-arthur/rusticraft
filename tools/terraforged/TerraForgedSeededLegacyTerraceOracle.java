import com.terraforged.noise.Module;
import com.terraforged.noise.Source;

public class TerraForgedSeededLegacyTerraceOracle {
    public static void main(String[] args) {
        Module source = Source.builder().seed(Integer.MAX_VALUE).frequency(0.125).octaves(3).gain(0.65).simplex();
        Module lower = Source.builder().seed(-91).frequency(0.25).octaves(2).perlin2();
        Module upper = Source.builder().seed(17).frequency(0.0625).octaves(2).simplex2();
        Module terrace = source.legacyTerrace(lower, upper, 7, 0.35);
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
