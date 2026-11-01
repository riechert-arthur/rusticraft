"""Run original NoiseChunkUtil cache initialization with coordinate fixtures."""

import argparse
from pathlib import Path
import subprocess
import tempfile


FIXTURE = """
class Long2IntMap extends java.util.TreeMap<Long,Integer> {
    void put(long key,int value){super.put(key,value);}
}
record ChunkPos(int x,int z) {
    int getMinBlockX(){return x<<4;} int getMinBlockZ(){return z<<4;}
}
record ChunkAccess(ChunkPos getPos) {}
record NoiseChunk(Long2IntMap cache) {}
class QuartPos { static int fromBlock(int v){return v>>2;} static int toBlock(int v){return v<<2;} }
class ColumnPos { static long asLong(int x,int z){return (x&0xffffffffL)|((z&0xffffffffL)<<32);} }
class TerrainData {
    int calls;
    int getHeight(int x,int z){calls++;return 70+x-z*2;}
}
public class SurfaceCacheOracle {
    public static void main(String[] args) throws Exception {
        var method=NoiseChunkUtil.class.getDeclaredMethod("initChunk",ChunkAccess.class,NoiseChunk.class,java.util.concurrent.CompletableFuture.class);
        method.setAccessible(true);
        for(int[] pos:new int[][]{{0,0},{-1,2},{Integer.MAX_VALUE,Integer.MIN_VALUE}}){
            var cache=new Long2IntMap(); var data=new TerrainData();
            method.invoke(null,new ChunkAccess(new ChunkPos(pos[0],pos[1])),new NoiseChunk(cache),
                java.util.concurrent.CompletableFuture.completedFuture(data));
            long hash=0xcbf29ce484222325L;
            for(var entry:cache.entrySet()){
                hash=(hash^entry.getKey())*0x100000001b3L;
                hash=(hash^Integer.toUnsignedLong(entry.getValue()))*0x100000001b3L;
            }
            System.out.printf("%d %d 0x%016x%n",cache.size(),data.calls,hash);
        }
        var cache=new Long2IntMap();cache.put(17L,-31);
        method.invoke(null,new ChunkAccess(new ChunkPos(0,0)),new NoiseChunk(cache),null);
        System.out.println(cache);
    }
}
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    args = parser.parse_args()
    source = (args.source / "src/main/java/com/terraforged/mod/worldgen/util/NoiseChunkUtil.java").read_text()
    methods = source[source.index("    private static void initChunk("):source.index("    private static Long2IntMap getCache(")]
    reference = "import java.util.concurrent.CompletableFuture;\nclass NoiseChunkUtil {\n" + methods + "static Long2IntMap getCache(NoiseChunk chunk){return chunk.cache();}\n}\n"
    with tempfile.TemporaryDirectory(prefix="tf-surface-cache-") as temporary:
        directory = Path(temporary)
        (directory / "NoiseChunkUtil.java").write_text(reference)
        (directory / "SurfaceCacheOracle.java").write_text(FIXTURE)
        subprocess.run(["javac", "-d", str(directory), str(directory / "NoiseChunkUtil.java"),
                        str(directory / "SurfaceCacheOracle.java")], check=True)
        subprocess.run(["java", "-XX:-UsePerfData", "-cp", str(directory),
                        "SurfaceCacheOracle"], check=True)


if __name__ == "__main__":
    main()
