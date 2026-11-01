package net.minecraft.world.level.block;
import net.minecraft.world.level.block.entity.*;
public abstract class BaseEntityBlock extends Block implements EntityBlock {
    protected BaseEntityBlock(net.minecraft.world.level.block.state.BlockBehaviour.Properties p) { super(p); }
    protected static <E extends BlockEntity, A extends BlockEntity> BlockEntityTicker<A> createTickerHelper(BlockEntityType<A> given, BlockEntityType<E> expected, BlockEntityTicker<? super E> ticker) { return null; }
}
