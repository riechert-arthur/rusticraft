import com.terraforged.noise.Module;
import com.terraforged.noise.Source;
import com.terraforged.noise.func.CellFunc;
import com.terraforged.noise.func.DistanceFunc;
import com.terraforged.noise.func.EdgeFunc;

public class TerraForgedSeededSpecialSourceOracle {
    public static void main(String[] args) {
        Module lookup = Source.builder().seed(1303).frequency(1.0 / 64.0).octaves(3).gain(0.5).simplex();
        Module alpha = Source.builder().seed(1304).frequency(1.0 / 72.0).octaves(2).gain(0.5).simplex2();
        Module[] graphs = {
            lookup.boost(2),
            Source.builder().seed(1301).frequency(1.0 / 96.0).displacement(0.85).distFunc(DistanceFunc.MANHATTAN).cellFunc(CellFunc.CELL_VALUE).cell(),
            Source.builder().seed(1301).frequency(1.0 / 96.0).displacement(0.85).distFunc(DistanceFunc.NATURAL).cellFunc(CellFunc.NOISE_LOOKUP).source(lookup).cell(),
            Source.builder().seed(1302).frequency(1.0 / 80.0).displacement(0.7).distFunc(DistanceFunc.NATURAL).edgeFunc(EdgeFunc.DISTANCE_2_ADD).cellEdge(),
            Source.builder().frequency(1.0 / 64.0).source(alpha).sin()
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
