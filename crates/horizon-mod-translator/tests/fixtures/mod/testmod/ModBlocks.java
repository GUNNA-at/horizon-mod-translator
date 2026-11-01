package testmod;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockBehaviour;
import net.neoforged.neoforge.registries.*;
public class ModBlocks {
    public static final DeferredRegister.Blocks BLOCKS = DeferredRegister.createBlocks("testmod");
    public static final DeferredBlock<GrowBlock> GROW = BLOCKS.register("grow", () -> new GrowBlock(BlockBehaviour.Properties.of().strength(1.5f)));
}
