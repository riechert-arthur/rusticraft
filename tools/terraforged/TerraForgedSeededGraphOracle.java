import com.terraforged.noise.Module;
import com.terraforged.noise.Source;
import com.terraforged.noise.modifier.*;
import com.terraforged.noise.combiner.*;
public class TerraForgedSeededGraphOracle {
    public static void main(String[] args) {
        Module source = Source.builder().seed(Integer.MAX_VALUE).frequency(0.125).octaves(2).gain(0.65).simplex();
        Module control = Source.builder().seed(-91).frequency(0.25).octaves(2).cubic();
        Module perlin = Source.builder().seed(53).frequency(0.5).octaves(2).perlin();
        Module[] graphs = {new Abs(new Bias(source, Source.constant(-0.5))),
            new Alpha(source, control), new Bias(source, control),
            new Clamp(source, new Scale(control, Source.constant(0.25)), new Bias(control, Source.constant(0.5))),
            new Invert(source), new Map(source, control, new Bias(control, Source.constant(1))),
            new Power(source, control), new Scale(source, control), new Threshold(source, control),
            new Add(source, control, perlin), new Multiply(source, control, perlin),
            new Max(source, control, perlin), new Min(source, control, perlin), new Sub(source, control, perlin),
            new Scale(new Bias(new Alpha(source, control), perlin), Source.constant(0.4))};
        for(Module graph:graphs) {
            for(int seed:new int[]{0,17,-91,Integer.MAX_VALUE}) {
                long hash=0xcbf29ce484222325L;
                for(float[] point:new float[][]{{0,0},{0.125f,-0.75f},{12.34f,56.78f},{-1234.5f,987.25f}}) {
                    hash ^= Integer.toUnsignedLong(Float.floatToRawIntBits(graph.getValue(seed,point[0],point[1])));
                    hash *= 0x100000001b3L;
                }
                System.out.printf("0x%016x, ",hash);
            }
            System.out.println();
        }
    }
}
