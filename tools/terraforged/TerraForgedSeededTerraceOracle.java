import com.terraforged.noise.Module;
import com.terraforged.noise.Source;
import com.terraforged.noise.modifier.Terrace;

public class TerraForgedSeededTerraceOracle {
    private record Fixed(float value) implements Module {
        public float getValue(int seed, float x, float y) { return value; }
        public float minValue() { return 0; }
        public float maxValue() { return 1; }
    }

    public static void main(String[] args) {
        Module source = Source.builder().seed(Integer.MAX_VALUE).frequency(0.125).octaves(3).gain(0.65).simplex();
        Module ramp = Source.builder().seed(-91).frequency(0.25).octaves(2).perlin2();
        Module cliff = Source.builder().seed(17).frequency(0.0625).octaves(2).simplex2();
        Module height = Source.builder().seed(1234).frequency(0.5).octaves(1).perlin();
        Module terrace = source.terrace(ramp, cliff, height, 7, 0.35);
        for (int seed : new int[]{0, 17, -91, Integer.MAX_VALUE}) {
            long hash = 0xcbf29ce484222325L;
            for (float[] point : new float[][]{{0,0}, {0.125f,-0.75f}, {12.34f,56.78f}, {-1234.5f,987.25f}}) {
                hash ^= Integer.toUnsignedLong(Float.floatToRawIntBits(terrace.getValue(seed, point[0], point[1])));
                hash *= 0x100000001b3L;
            }
            System.out.printf("0x%016x%n", hash);
        }

        for (float value : new float[]{-1, 0, 0.05f, 0.1f, 0.2f, 0.5f, 0.9f, 0.999999f, 1, 2}) {
            Module boundary = new Terrace(new Fixed(value), Source.constant(0.4), Source.constant(0.7), Source.constant(0.8), 5, 0.25f);
            System.out.printf("boundary=%08x%n", Float.floatToRawIntBits(boundary.getValue(17, 0, 0)));
        }
    }
}
