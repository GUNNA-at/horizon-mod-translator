package net.minecraft.world.level.block.entity;
public interface BlockEntityTicker<T extends BlockEntity> {
    void tick(net.minecraft.world.level.Level level, net.minecraft.core.BlockPos pos, net.minecraft.world.level.block.state.BlockState state, T be);
}
