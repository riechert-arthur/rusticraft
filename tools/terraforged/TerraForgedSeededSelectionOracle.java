import com.terraforged.noise.Module;
import com.terraforged.noise.Source;
import com.terraforged.noise.func.Interpolation;
import com.terraforged.noise.selector.*;
public class TerraForgedSeededSelectionOracle {
    public static void main(String[] args) {
        Module source = Source.builder().seed(Integer.MAX_VALUE).frequency(0.125).octaves(2).gain(0.65).simplex();
        Module control = Source.builder().seed(-91).frequency(0.25).octaves(2).simplex2();
        Module lower = control.scale(0.35), upper = source.bias(0.2);
        for (Interpolation interpolation : Interpolation.values()) {
            Module[] graphs = {new Select(control, lower, upper, 0.25f, 0.75f, 0.125f, interpolation),
                new Select(control, lower, upper, 0.25f, 0.75f, 0, interpolation),
                new Select(control, lower, upper, 0.75f, 0.25f, 0, interpolation),
                new MultiBlend(0.8f, interpolation, control, lower, source, upper),
                new MultiBlend(0, interpolation, control, lower, source, upper)};
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
        for (float value : new float[]{0,0.125f,0.25f,0.375f,0.5f,0.625f,0.75f,0.875f,1}) {
            Module selector = new Select(Source.constant(value), Source.constant(0.25), Source.constant(0.75), 0.25f, 0.75f, 0.125f, Interpolation.LINEAR);
            System.out.printf("boundary %08x%n",Float.floatToRawIntBits(selector.getValue(17,0,0)));
        }
    }
}
