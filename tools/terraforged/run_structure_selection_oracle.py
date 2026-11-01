"""Run TerraForged's original structure constructor against selection fixtures."""

import argparse
from pathlib import Path
import subprocess
import tempfile


FIXTURE = """
import java.util.*;
import java.util.function.Predicate;
enum TerrainAdjustment { NONE, BURY, BEARD_BOX, BEARD_THIN, ENCAPSULATE }
record Structure(TerrainAdjustment terrainAdaptation) {}
record ChunkPos(int x, int z) {}
record ChunkAccess(ChunkPos getPos) {}
record BoundingBox(int minX, int minY, int minZ, int maxX, int maxY, int maxZ) {}
class StructurePiece {
    final BoundingBox box;
    StructurePiece(int x,int y,int z) { box=new BoundingBox(x,y,z,x,y+5,z); }
    BoundingBox getBoundingBox() { return box; }
    boolean isCloseToChunk(ChunkPos pos,int radius) {
        int x=pos.x()<<4,z=pos.z()<<4;
        return box.maxX()>=x-radius && box.minX()<=x+15+radius
            && box.maxZ()>=z-radius && box.minZ()<=z+15+radius;
    }
}
class StructureTemplatePool { enum Projection { RIGID, TERRAIN_MATCHING } }
record Element(StructureTemplatePool.Projection getProjection) {}
class PoolElementStructurePiece extends StructurePiece {
    final int delta; final Element element;
    PoolElementStructurePiece(int[] row) {
        super(row[0],row[2],row[1]); delta=row[4];
        element=new Element(StructureTemplatePool.Projection.values()[row[3]]);
    }
    Element getElement() { return element; }
}
record Start(Structure structure,List<StructurePiece> getPieces) {}
record StructureManager(List<Start> starts) {
    java.util.stream.Stream<Start> startsForStructure(ChunkPos pos,Predicate<Structure> predicate) {
        return starts.stream().filter(start->predicate.test(start.structure()));
    }
}
public class StructureSelectionOracle {
    public static void main(String[] args) {
        var starts=new ArrayList<Start>();
        for(var adjustment:TerrainAdjustment.values()) {
            var pieces=new ArrayList<StructurePiece>();
            for(int[] row:new int[][] {
                {-36,-16,80,0,2},{19,-16,60,0,3},{-37,-16,40,0,4},{20,-16,40,0,5},
                {-16,-36,60,0,6},{-16,19,90,0,7},{-16,-37,40,0,8},{-16,20,40,0,9},
                {-16,-16,40,1,10}
            }) pieces.add(new PoolElementStructurePiece(row));
            pieces.add(new StructurePiece(-16,60,-16));
            starts.add(new Start(new Structure(adjustment),pieces));
        }
        var operation=new StructureTerrain(new ChunkAccess(new ChunkPos(-1,-1)),new StructureManager(starts));
        for(var piece:operation.rigids) {
            var box=piece.getBoundingBox();
            System.out.printf("%d %d %d %d%n",box.minX(),box.minY(),box.minZ(),
                piece instanceof PoolElementStructurePiece pool?pool.delta:0);
        }
    }
}
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    args = parser.parse_args()
    source = (args.source / "src/main/java/com/terraforged/mod/worldgen/terrain/StructureTerrain.java").read_text()
    constants = source[source.index("    private static final int RADIUS"):source.index("    protected final ObjectList<")]
    constructor = source[source.index("    public StructureTerrain("):source.index("    public void modify(")]
    reference = """
import java.util.*;
import java.util.function.Predicate;
class StructureTerrain {
    final List<StructurePiece> rigids=new ArrayList<>();
    final Iterator<StructurePiece> pieceIterator;
""" + constants + constructor + "}\n"
    with tempfile.TemporaryDirectory(prefix="tf-structure-selection-") as temporary:
        directory = Path(temporary)
        (directory / "StructureTerrain.java").write_text(reference)
        (directory / "StructureSelectionOracle.java").write_text(FIXTURE)
        subprocess.run(["javac", "-d", str(directory),
                        str(directory / "StructureTerrain.java"),
                        str(directory / "StructureSelectionOracle.java")], check=True)
        subprocess.run(["java", "-XX:-UsePerfData", "-cp", str(directory),
                        "StructureSelectionOracle"], check=True)


if __name__ == "__main__":
    main()
