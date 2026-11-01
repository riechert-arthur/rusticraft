import com.terraforged.noise.Module;
import com.terraforged.noise.Source;
import com.terraforged.noise.domain.Domain;
public class TerraForgedSeededDomainOracle {
    public static void main(String[] args) {
        Module source = Source.builder().seed(Integer.MAX_VALUE).frequency(0.125).octaves(2).gain(0.65).simplex();
        Module control = Source.builder().seed(-91).frequency(0.25).octaves(2).cubic();
        Module strength = source.scale(3).bias(0.25);
        Domain axes = Domain.warp(source, control, strength);
        Domain direction = Domain.direction(source, strength);
        Domain[] domains = {Domain.DIRECT, axes, direction, axes.add(direction), axes.warp(direction),
            axes.then(direction), axes.then(direction).warp(axes.add(direction))};
        for (Domain domain : domains) {
            for (int seed : new int[]{0, 17, -91, Integer.MAX_VALUE}) {
                long hash = 0xcbf29ce484222325L;
                for (float[] point : new float[][]{{0,0}, {0.125f,-0.75f}, {12.34f,56.78f}, {-1234.5f,987.25f}}) {
                    float x = point[0], y = point[1];
                    for (float value : new float[]{domain.getOffsetX(seed,x,y), domain.getOffsetY(seed,x,y),
                            domain.getX(seed,x,y), domain.getY(seed,x,y), source.warp(domain).getValue(seed,x,y)}) {
                        hash ^= Integer.toUnsignedLong(Float.floatToRawIntBits(value));
                        hash *= 0x100000001b3L;
                    }
                }
                System.out.printf("0x%016x, ", hash);
            }
            System.out.println();
        }
    }
}
