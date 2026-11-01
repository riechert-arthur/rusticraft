import com.terraforged.noise.Module;
import com.terraforged.noise.Source;

public class TerraForgedSeededTransformOracle {
    public static void main(String[] args) {
        Module base = Source.builder().seed(1001).frequency(1.0 / 96.0).octaves(4).gain(0.5).perlin();
        Module simplex = Source.builder().seed(1002).frequency(1.0 / 80.0).octaves(3).gain(0.5).simplex();
        Module ridge = Source.builder().seed(1003).frequency(1.0 / 128.0).octaves(2).gain(0.975).ridge();
        Module perlin2 = Source.builder().seed(1004).frequency(1.0 / 64.0).octaves(2).gain(0.5).perlin2();
        Module[] graphs = {
            base.steps(simplex, Source.constant(0.2), ridge),
            base.freq(simplex, ridge),
            base.grad(simplex, ridge, perlin2),
            base.mod(simplex, ridge)
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
