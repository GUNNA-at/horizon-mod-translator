package testmod;
import net.minecraft.core.BlockPos;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.state.*;
// A test mod block: on a scheduled tick, fills the air above with stone, or
// turns stone above into dirt. Written for these tests.
public class GrowBlock extends Block {
    public GrowBlock(BlockBehaviour.Properties p) { super(p); }
    @Override
    protected void tick(BlockState state, ServerLevel level, BlockPos pos, RandomSource random) {
        if (level.isClientSide()) return;
        BlockPos up = pos.above();
        BlockState there = level.getBlockState(up);
        if (there.isAir()) level.setBlock(up, Blocks.STONE.defaultBlockState(), 3);
        else if (there.getBlock() == Blocks.STONE || pos.getY() < 0) level.setBlock(up, Blocks.DIRT.defaultBlockState(), 3);
    }
}
