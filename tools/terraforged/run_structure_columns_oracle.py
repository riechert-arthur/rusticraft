"""Execute original StructureTerrain column methods with minimal world stand-ins."""

import argparse
from pathlib import Path
import subprocess
import tempfile


FIXTURE = """
import java.util.*;
class BlockState { final int id; BlockState(int id) { this.id=id; } }
class BlockPos {
    static class MutableBlockPos {
        int x,y,z;
        MutableBlockPos set(int x,int y,int z) { this.x=x;this.y=y;this.z=z;return this; }
    }
}
class Heightmap { enum Types { OCEAN_FLOOR_WG } }
class BoundingBox {
    int[] values;
    BoundingBox(int... values) { this.values=values; }
    int minX(){return values[0];} int minY(){return values[1];} int minZ(){return values[2];}
    int maxX(){return values[3];} int maxY(){return values[4];} int maxZ(){return values[5];}
    int getXSpan(){return maxX()-minX()+1;} int getZSpan(){return maxZ()-minZ()+1;}
}
class StructurePiece {
    BoundingBox box;
    StructurePiece(BoundingBox box){this.box=box;}
    BoundingBox getBoundingBox(){return box;}
}
class PoolElementStructurePiece extends StructurePiece {
    int delta;
    PoolElementStructurePiece(BoundingBox box,int delta){super(box);this.delta=delta;}
    int getGroundLevelDelta(){return delta;}
}
class Cursor {
    List<StructurePiece> pieces; int index;
    Cursor(List<StructurePiece> pieces){this.pieces=pieces;}
    boolean hasNext(){return index<pieces.size();}
    StructurePiece next(){return pieces.get(index++);}
    void back(int count){index-=count;}
}
class TerrainData {
    Integer height;
    TerrainData getHeight(){return this;}
    void set(int x,int z,int height){this.height=height;}
}
class ChunkAccess {
    TreeSet<Integer> stone=new TreeSet<>();
    long hash=0xcbf29ce484222325L; int writes;
    ChunkAccess(int surface){for(int y=-64;y<=surface;y++)stone.add(y);}
    int getHeight(Heightmap.Types type,int x,int z){return stone.isEmpty()?-65:stone.last();}
    void setBlockState(BlockPos.MutableBlockPos pos,BlockState state,boolean flag){
        if(state.id==0)stone.remove(pos.y);else stone.add(pos.y);
        hash=(hash^Integer.toUnsignedLong(pos.y))*0x100000001b3L;
        hash=(hash^state.id)*0x100000001b3L;
        writes++;
    }
}
public class StructureColumnOracle {
    public static void main(String[] args) {
        var pieces=List.of(new StructurePiece(new BoundingBox(0,70,0,6,75,6)),
            new PoolElementStructurePiece(new BoundingBox(2,90,2,4,94,4),2));
        var operation=new StructureTerrain(pieces);
        for(int[] point:new int[][]{{-3,3,60},{3,3,60},{3,3,110},{0,0,80},{30,30,60},{6,6,60}}){
            ChunkAccess chunk=new ChunkAccess(point[2]); TerrainData data=new TerrainData();
            operation.modify(point[0],point[1],chunk,data);
            System.out.printf("%s %d 0x%016x%n",data.height,chunk.writes,chunk.hash);
        }
    }
}
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--classpath", required=True)
    args = parser.parse_args()
    source = (args.source / "src/main/java/com/terraforged/mod/worldgen/terrain/StructureTerrain.java").read_text()
    methods = source[source.index("    public void modify("):]
    prefix = """
import com.terraforged.noise.util.NoiseUtil;
class StructureTerrain {
    final java.util.List<StructurePiece> rigids;
    final Cursor pieceIterator;
    final BlockState air=new BlockState(0),solid=new BlockState(1);
    final BlockPos.MutableBlockPos pos=new BlockPos.MutableBlockPos();
    static final int RADIUS=20;
    StructureTerrain(java.util.List<StructurePiece> pieces){
        rigids=new java.util.ArrayList<>(pieces);
        rigids.sort(java.util.Comparator.comparing(piece->piece.getBoundingBox().minY()));
        pieceIterator=new Cursor(rigids);
    }
"""
    with tempfile.TemporaryDirectory(prefix="tf-structure-column-") as temporary:
        directory = Path(temporary)
        reference = directory / "StructureTerrain.java"
        reference.write_text(prefix + methods)
        fixture = directory / "StructureColumnOracle.java"
        fixture.write_text(FIXTURE)
        subprocess.run(["javac", "-cp", args.classpath, "-d", str(directory),
                        str(reference), str(fixture)], check=True)
        subprocess.run(["java", "-XX:-UsePerfData", "-cp", f"{directory}:{args.classpath}",
                        "StructureColumnOracle"], check=True)


if __name__ == "__main__":
    main()
