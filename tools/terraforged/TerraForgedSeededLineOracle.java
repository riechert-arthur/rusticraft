import com.terraforged.noise.Module;
import com.terraforged.noise.Source;

public class TerraForgedSeededLineOracle {
    public static void main(String[] args) {
        Module radius = Source.builder().seed(1401).frequency(1.0 / 64.0).octaves(3).gain(0.5).simplex().scale(100.0);
        Module fadeIn = Source.builder().seed(1402).frequency(1.0 / 80.0).octaves(2).gain(0.5).simplex2();
        Module fadeOut = Source.builder().seed(1403).frequency(1.0 / 96.0).octaves(2).gain(0.5).ridge();
        Module line = Source.line(-10, 5, 30, 25, radius, fadeIn, fadeOut, 0.4);
        for (int seed : new int[]{0, 17, -91, Integer.MAX_VALUE}) {
            long hash = 0xcbf29ce484222325L;
            for (float[] point : new float[][]{{0,0}, {10,10}, {20,15}, {40,25}}) {
                hash ^= Integer.toUnsignedLong(Float.floatToRawIntBits(line.getValue(seed, point[0], point[1])));
                hash *= 0x100000001b3L;
            }
            System.out.printf("0x%016x%n", hash);
        }
    }
}
