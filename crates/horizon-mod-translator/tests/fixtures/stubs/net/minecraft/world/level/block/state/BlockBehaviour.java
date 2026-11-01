package net.minecraft.world.level.block.state;
import net.minecraft.core.BlockPos;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.util.RandomSource;
public abstract class BlockBehaviour {
    public BlockBehaviour(Properties p) {}
    protected void tick(BlockState state, ServerLevel level, BlockPos pos, RandomSource random) {}
    public static class Properties { public static Properties of() { return null; } public Properties strength(float f) { return this; } }
}
