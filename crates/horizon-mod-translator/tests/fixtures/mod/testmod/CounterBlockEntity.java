package testmod;
import net.minecraft.core.BlockPos;
import net.minecraft.core.HolderLookup;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;
public class CounterBlockEntity extends BlockEntity {
    int count;
    public CounterBlockEntity(BlockPos pos, BlockState state) { super(ModBlocks.COUNTER_BE.get(), pos, state); }
    public static void tick(Level level, BlockPos pos, BlockState state, CounterBlockEntity be) {
        be.count++;
        if (be.count == 3 && level.getBlockState(pos.above()).isAir()) level.setBlock(pos.above(), Blocks.STONE.defaultBlockState(), 3);
        be.setChanged();
    }
    @Override protected void loadAdditional(CompoundTag tag, HolderLookup.Provider r) { count = tag.getInt("count"); }
    @Override protected void saveAdditional(CompoundTag tag, HolderLookup.Provider r) { tag.putInt("count", count); }
}
