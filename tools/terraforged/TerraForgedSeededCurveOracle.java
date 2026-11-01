import com.terraforged.noise.Module;
import com.terraforged.noise.Source;
import com.terraforged.noise.func.SCurve;

public class TerraForgedSeededCurveOracle {
    public static void main(String[] args) {
        Module base = Source.builder().seed(901).frequency(1.0 / 96.0).octaves(4).gain(0.5).perlin();
        Module[] graphs = {
            base.curve(0.4, 6.0),
            base.curve(new SCurve(0.5f, -0.2f)),
            base.curve(
                Source.builder().seed(902).frequency(1.0 / 80.0).octaves(3).gain(0.5).simplex(),
                Source.builder().seed(903).frequency(1.0 / 128.0).octaves(2).gain(0.975).ridge()
            ),
            base.powCurve(0.8)
        };
        for (Module graph : graphs) {
            for (int seed : new int[]{0, 17, -91, Integer.MAX_VALUE}) {
                long hash = 0xcbf29ce484222325L;
                for (float[] point : new float[][]{{0,0}, {0.125f,-0.75f}, {12.34f,56.78f}, {-1234.5f,987.25f}}) {
                    hash ^= Integer.toUnsignedLong(Float.floatToRawIntBits(graph.getValue(seed, point[0], point[1])));
                    hash *= 0x100000001b3L;
                }
                System.out.printf("0x%016x%n", hash);
            }
        }
    }
}
