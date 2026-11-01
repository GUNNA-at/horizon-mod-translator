package testmod;
import net.minecraft.core.BlockPos;
import net.minecraft.world.InteractionResult;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.*;
import net.minecraft.world.level.block.state.properties.BlockStateProperties;
import net.minecraft.world.phys.BlockHitResult;
// A lever-like test block: right click toggles POWERED, unless sneaking. Written for these tests.
public class SwitchBlock extends Block {
    public SwitchBlock(BlockBehaviour.Properties p) { super(p); }
    @Override
    protected void createBlockStateDefinition(StateDefinition.Builder<Block, BlockState> b) { b.add(BlockStateProperties.POWERED); }

    @Override
    protected InteractionResult useWithoutItem(BlockState state, Level level, BlockPos pos, Player player, BlockHitResult hit) {
        if (player.isShiftKeyDown()) return InteractionResult.PASS;
        BlockState next = state.cycle(BlockStateProperties.POWERED);
        level.setBlock(pos, next, 3);
        if (next.getValue(BlockStateProperties.POWERED)) level.setBlock(pos.above(), net.minecraft.world.level.block.Blocks.STONE.defaultBlockState(), 3);
        return InteractionResult.sidedSuccess(level.isClientSide());
    }
}
