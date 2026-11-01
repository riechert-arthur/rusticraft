"""Execute TerraForged's original surface erosion methods with column fixtures."""

import argparse
from pathlib import Path
import subprocess
import tempfile


FIXTURE = """
class BlockTags { static final int DIRT=4,SNOW=5; }
record BlockState(int id) { boolean is(int tag){return id==tag;} }
class Heightmap { enum Types { OCEAN_FLOOR_WG } }
class BlockPos {
    static class MutableBlockPos {
        int x,y,z;
        MutableBlockPos set(int x,int y,int z){this.x=x;this.y=y;this.z=z;return this;}
        MutableBlockPos setY(int y){this.y=y;return this;}
        int getY(){return y;}
    }
}
record ChunkGenerator(int getGenDepth,int getSeaLevel) {}
record TerrainData(float gradient) {
    float getGradient(int x,int z,float norm){return Math.min(1,Math.max(0,gradient*norm));}
}
class ChunkAccess {
    int[] states=new int[192]; final int top; int writes;
    ChunkAccess(int top,int bottom,int base,int cover){
        this.top=top;
        for(int y=-64;y<=top;y++)states[y+64]=y<bottom?2:y==bottom?base:cover;
    }
    int getHeight(Heightmap.Types type,int x,int z){return x==0&&z==0?top:-65;}
    BlockState getBlockState(BlockPos.MutableBlockPos pos){return new BlockState(states[pos.y+64]);}
    void setBlockState(BlockPos.MutableBlockPos pos,BlockState state,boolean flag){
        states[pos.y+64]=state.id();writes++;
    }
}
public class SurfaceErosionOracle {
    public static void main(String[] args){
        for(int[] row:new int[][]{
            {80,77,6,4,62,1},{80,77,0,4,62,1},{80,77,3,5,62,1},
            {80,61,6,4,62,1},{80,60,6,4,62,1},{1,0,6,4,0,1},
            {-10,-12,6,4,-64,1},{61,58,6,4,62,1},{62,59,6,4,62,1},
            {80,80,6,4,62,1},{80,77,6,4,62,0}
        }){
            var chunk=new ChunkAccess(row[0],row[1],row[2],row[3]);
            Surface.apply(new TerrainData(row[5]==1?0.02f:0.01f),chunk,new ChunkGenerator(255,row[4]));
            long hash=0xcbf29ce484222325L;
            for(int state:chunk.states)hash=(hash^state)*0x100000001b3L;
            System.out.printf("%d 0x%016x%n",chunk.writes,hash);
        }
    }
}
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    args = parser.parse_args()
    source = (args.source / "src/main/java/com/terraforged/mod/worldgen/biome/surface/Surface.java").read_text()
    apply = source[source.index("    public static void apply("):source.index("    public static void applyPost(")]
    erodible = source[source.index("    public static boolean isErodible("):source.index("    protected static boolean sameChunk(")]
    find = source[source.index("    protected static BlockState findSolid("):]
    reference = "class Surface { static final int ERODIBLE=BlockTags.DIRT;\n" + apply + erodible + find
    with tempfile.TemporaryDirectory(prefix="tf-surface-erosion-") as temporary:
        directory = Path(temporary)
        (directory / "Surface.java").write_text(reference)
        (directory / "SurfaceErosionOracle.java").write_text(FIXTURE)
        subprocess.run(["javac", "-d", str(directory), str(directory / "Surface.java"),
                        str(directory / "SurfaceErosionOracle.java")], check=True)
        subprocess.run(["java", "-XX:-UsePerfData", "-cp", str(directory),
                        "SurfaceErosionOracle"], check=True)


if __name__ == "__main__":
    main()
