package testmod;
import net.minecraft.core.BlockPos;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.BaseEntityBlock;
import net.minecraft.world.level.block.entity.*;
import net.minecraft.world.level.block.state.*;
// A machine-style test block: its block entity counts ticks. Written for these tests.
public class CounterBlock extends BaseEntityBlock {
    public CounterBlock(BlockBehaviour.Properties p) { super(p); }
    @Override
    public BlockEntity newBlockEntity(BlockPos pos, BlockState state) { return new CounterBlockEntity(pos, state); }
    @Override
    public <T extends BlockEntity> BlockEntityTicker<T> getTicker(Level level, BlockState state, BlockEntityType<T> type) {
        return createTickerHelper(type, ModBlocks.COUNTER_BE.get(), CounterBlockEntity::tick);
    }
}
