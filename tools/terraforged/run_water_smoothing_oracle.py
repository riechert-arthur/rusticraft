"""Execute original water smoothing with ordered cross-chunk lookup fixtures."""

import argparse
from pathlib import Path
import subprocess
import tempfile


FIXTURE = """
class BlockPos {
    int x,y,z;
    int getX(){return x;} int getZ(){return z;}
    static class MutableBlockPos extends BlockPos {
        MutableBlockPos set(int x,int y,int z){this.x=x;this.y=y;this.z=z;return this;}
        int getX(){return x;} int getZ(){return z;}
    }
}
class ChunkPos {
    final int x,z;
    ChunkPos(int x,int z){this.x=x;this.z=z;}
    int getMinBlockX(){return x<<4;} int getMinBlockZ(){return z<<4;}
}
class LiquidBlock { static final int LEVEL=0; }
class Heightmap { enum Types { MOTION_BLOCKING_NO_LEAVES } }
class Blocks { static final Block WATER=new Block(); }
class Block { BlockState defaultBlockState(){return new BlockState(3);} }
record BlockState(int id) {
    boolean is(Block block){return id==3||id==7||id==8;}
    boolean isAir(){return id==0;}
    int getValue(int property){return id==3?0:id==7?2:1;}
    BlockState setValue(int property,int value){return new BlockState(7);}
}
record Land(String kind) {
    boolean isRiver(){return kind.equals("river");} boolean isLake(){return kind.equals("lake");}
}
record RiverMap(float river) { float get(int x,int z){return x==0&&z==0?river:1;} }
record TerrainMap(String kind) { Land get(int x,int z){return new Land(kind);} }
record TerrainData(String kind,float river) {
    RiverMap getRiver(){return new RiverMap(river);} TerrainMap getTerrain(){return new TerrainMap(kind);}
}
interface WorldGenLevel { BlockState getBlockState(BlockPos.MutableBlockPos pos); }
class ChunkAccess implements WorldGenLevel {
    int water; final int airX,airZ;
    ChunkAccess(int water,int airX,int airZ){this.water=water;this.airX=airX;this.airZ=airZ;}
    ChunkPos getPos(){return new ChunkPos(-1,2);}
    int getHeight(Heightmap.Types type,int x,int z){return 60;}
    public BlockState getBlockState(BlockPos.MutableBlockPos pos){
        if(pos.x==-16&&pos.z==32)return new BlockState(water);
        return new BlockState(pos.x==airX&&pos.z==airZ?0:2);
    }
    void setBlockState(BlockPos.MutableBlockPos pos,BlockState state,boolean flag){water=state.id();}
}
class Region implements WorldGenLevel {
    final int airX,airZ; int calls; long hash=0xcbf29ce484222325L;
    Region(int x,int z){airX=x;airZ=z;}
    public BlockState getBlockState(BlockPos.MutableBlockPos pos){
        calls++;
        for(int value:new int[]{pos.x,pos.y,pos.z}) hash=(hash^Integer.toUnsignedLong(value))*0x100000001b3L;
        return new BlockState(pos.x==airX&&pos.z==airZ?0:2);
    }
}
public class WaterSmoothingOracle {
    public static void main(String[] args){
        String[] kinds={"river","river","river","river","lake","hills","river","river","river","river"};
        float[] rivers={0,0,0,0,0,0,.001f,-0.0f,Float.NaN,0};
        int[][] offsets={{0,-6},{6,0},{6,1},{99,99},{0,-6},{0,-6},{0,-6},{0,-6},{0,-6},{0,-6}};
        for(int index=0;index<kinds.length;index++){
            int x=-16+offsets[index][0],z=32+offsets[index][1];
            var chunk=new ChunkAccess(index==9?8:3,x,z); var region=new Region(x,z);
            Surface.smoothWater(chunk,region,new TerrainData(kinds[index],rivers[index]));
            System.out.printf("%d %d 0x%016x%n",chunk.water,region.calls,region.hash);
        }
    }
}
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    args = parser.parse_args()
    source = (args.source / "src/main/java/com/terraforged/mod/worldgen/biome/surface/Surface.java").read_text()
    water = source[source.index("    public static void smoothWater("):source.index("    protected static void smoothSnow(")]
    same = source[source.index("    protected static boolean sameChunk("):source.index("    protected static BlockState findSolid(")]
    with tempfile.TemporaryDirectory(prefix="tf-water-smoothing-") as temporary:
        directory = Path(temporary)
        (directory / "Surface.java").write_text("class Surface {\n" + water + same + "}\n")
        (directory / "WaterSmoothingOracle.java").write_text(FIXTURE)
        subprocess.run(["javac", "-d", str(directory), str(directory / "Surface.java"),
                        str(directory / "WaterSmoothingOracle.java")], check=True)
        subprocess.run(["java", "-XX:-UsePerfData", "-cp", str(directory),
                        "WaterSmoothingOracle"], check=True)


if __name__ == "__main__":
    main()
