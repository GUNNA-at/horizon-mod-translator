package net.minecraft.world.level.block.entity;
import net.minecraft.core.BlockPos;
import net.minecraft.core.HolderLookup;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.world.level.block.state.BlockState;
public abstract class BlockEntity {
    public BlockEntity(BlockEntityType<?> type, BlockPos pos, BlockState state) {}
    protected void loadAdditional(CompoundTag tag, HolderLookup.Provider registries) {}
    protected void saveAdditional(CompoundTag tag, HolderLookup.Provider registries) {}
    public void setChanged() {}
    public BlockPos getBlockPos() { return null; }
}
