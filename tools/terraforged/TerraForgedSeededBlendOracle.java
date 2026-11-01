import com.terraforged.noise.Module;
import com.terraforged.noise.Source;
import com.terraforged.noise.func.Interpolation;
import com.terraforged.noise.selector.*;
public class TerraForgedSeededBlendOracle {
    public static void main(String[] args) {
        Module source = Source.builder().seed(Integer.MAX_VALUE).frequency(0.125).octaves(2).gain(0.65).simplex();
        Module control = Source.builder().seed(-91).frequency(0.25).octaves(2).simplex2();
        Module lower = control.scale(0.35);
        Module upper = source.bias(0.2);
        for (Interpolation interpolation : Interpolation.values()) {
            Module[] graphs = {new Base(lower, source, 0.3f, interpolation),
                new Blend(control, lower, upper, 0.5f, 0.4f, interpolation),
                new VariableBlend(control, source, lower, upper, 0.5f, 0.05f, 0.3f, interpolation)};
            for (Module graph : graphs) {
                for (int seed : new int[]{0, 17, -91, Integer.MAX_VALUE}) {
                    long hash = 0xcbf29ce484222325L;
                    for (float[] point : new float[][]{{0,0}, {0.125f,-0.75f}, {12.34f,56.78f}, {-1234.5f,987.25f}}) {
                        hash ^= Integer.toUnsignedLong(Float.floatToRawIntBits(graph.getValue(seed,point[0],point[1])));
                        hash *= 0x100000001b3L;
                    }
                    System.out.printf("0x%016x, ", hash);
                }
                System.out.println();
            }
        }
        System.out.println("zero-width blend NaN: " + Float.isNaN(new Blend(source, lower, upper, 0.5f, 0, Interpolation.LINEAR).getValue(17,0,0)));
        System.out.println("zero-width variable NaN: " + Float.isNaN(new VariableBlend(source, Source.constant(0), lower, upper, 0.5f, 0, 0, Interpolation.LINEAR).getValue(17,0,0)));
    }
}
