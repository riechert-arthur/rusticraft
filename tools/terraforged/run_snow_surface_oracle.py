"""Run original TerraForged snow postprocessing against column fixtures."""

import argparse
from pathlib import Path
import subprocess
import tempfile


FIXTURE = """
class BlockPos {
    static class MutableBlockPos {
        int x,y,z;
        MutableBlockPos set(int x,int y,int z){this.x=x;this.y=y;this.z=z;return this;}
        MutableBlockPos setY(int y){this.y=y;return this;}
        int getX(){return x;} int getY(){return y;} int getZ(){return z;}
    }
}
class Block { final int id; Block(int id){this.id=id;} BlockState defaultBlockState(){return new BlockState(id);} }
class SnowLayerBlock extends Block { static final int LAYERS=0; SnowLayerBlock(){super(9);} }
class Blocks { static final Block AIR=new Block(0),STONE=new Block(2); }
class BlockTags { static final int DIRT=4,SNOW=5; }
record BlockState(int id) {
    Block getBlock(){return id>=9?new SnowLayerBlock():new Block(id);}
    boolean isAir(){return id==0;}
    boolean is(int tag){return tag==5?(id==5||id>=9):id==tag;}
    BlockState setValue(int property,int layers){return new BlockState(8+layers);}
}
class Heightmap { enum Types { MOTION_BLOCKING_NO_LEAVES } }
record ChunkGenerator(int getGenDepth) {}
record Height(float value) { float get(int x,int z){return value;} }
class Levels { int getHeight(float height){return com.terraforged.noise.util.NoiseUtil.floor(height);} }
record TerrainData(float height,float gradient) {
    Height getHeight(){return new Height(height);} Levels getLevels(){return new Levels();}
    float getGradient(int x,int z,float norm){return Math.min(1,Math.max(0,gradient*norm));}
}
class ChunkAccess {
    final int[] states=new int[192]; final int top,initial;
    int writes;
    ChunkAccess(int top,int initial,int bottom){
        this.top=top;this.initial=initial;
        for(int y=-64;y<=top;y++)states[y+64]=y==top?initial:y<=bottom?2:4;
    }
    int getHeight(Heightmap.Types type,int x,int z){return x==0&&z==0?top-(initial>=9?1:0):-65;}
    BlockState getBlockState(BlockPos.MutableBlockPos pos){
        return new BlockState(pos.x==0&&pos.z==0&&pos.y>=-64&&pos.y<128?states[pos.y+64]:0);
    }
    void setBlockState(BlockPos.MutableBlockPos pos,BlockState state,boolean flag){states[pos.y+64]=state.id();writes++;}
}
public class SnowSurfaceOracle {
    public static void main(String[] args){
        float[] heights={96,96.125f,96.25f,96.5f,96.999f,-1,-1.25f,96,96,96,96,96,96};
        int[][] cases={{80,9,60,0},{80,9,60,0},{80,9,60,0},{80,9,60,0},
            {80,9,60,0},{80,9,60,0},{80,9,60,0},{80,9,60,1},
            {80,9,77,1},{80,5,60,1},{80,4,60,1},{2,9,-2,1},{0,9,-4,1}};
        for(int index=0;index<cases.length;index++){
            var row=cases[index]; var chunk=new ChunkAccess(row[0],row[1],row[2]);
            float height=(heights[index]/480f)*480f;
            Surface.applyPost(chunk,new TerrainData(height,row[3]==1?.02f:0),new ChunkGenerator(255));
            long hash=0xcbf29ce484222325L;
            for(int state:chunk.states)hash=(hash^state)*0x100000001b3L;
            System.out.printf("%d %d 0x%016x%n",chunk.states[row[0]+64],chunk.writes,hash);
        }
    }
}
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--classpath", required=True)
    args = parser.parse_args()
    source = (args.source / "src/main/java/com/terraforged/mod/worldgen/biome/surface/Surface.java").read_text()
    post = source[source.index("    public static void applyPost("):source.index("    public static void smoothWater(")]
    snow = source[source.index("    protected static void smoothSnow("):source.index("    protected static boolean sameChunk(")]
    reference = "import com.terraforged.noise.util.NoiseUtil;\nclass Surface { static final int ERODIBLE=BlockTags.DIRT;\n" + post + snow + "}\n"
    with tempfile.TemporaryDirectory(prefix="tf-snow-surface-") as temporary:
        directory = Path(temporary)
        (directory / "Surface.java").write_text(reference)
        (directory / "SnowSurfaceOracle.java").write_text(FIXTURE)
        subprocess.run(["javac", "-cp", args.classpath, "-d", str(directory),
                        str(directory / "Surface.java"), str(directory / "SnowSurfaceOracle.java")], check=True)
        subprocess.run(["java", "-XX:-UsePerfData", "-cp", f"{directory}:{args.classpath}",
                        "SnowSurfaceOracle"], check=True)


if __name__ == "__main__":
    main()
