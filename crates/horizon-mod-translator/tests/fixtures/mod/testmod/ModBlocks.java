package testmod;
import net.minecraft.core.registries.Registries;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.entity.BlockEntityType;
import net.minecraft.world.level.block.state.BlockBehaviour;
import net.neoforged.neoforge.registries.*;
public class ModBlocks {
    public static final DeferredRegister.Blocks BLOCKS = DeferredRegister.createBlocks("testmod");
    public static final DeferredBlock<GrowBlock> GROW = BLOCKS.register("grow", () -> new GrowBlock(BlockBehaviour.Properties.of().strength(1.5f)));
    public static final DeferredBlock<CounterBlock> COUNTER = BLOCKS.register("counter", () -> new CounterBlock(BlockBehaviour.Properties.of()));
    public static final DeferredRegister<BlockEntityType<?>> BLOCK_ENTITIES = DeferredRegister.create(Registries.BLOCK_ENTITY_TYPE, "testmod");
    public static final DeferredHolder<BlockEntityType<?>, BlockEntityType<CounterBlockEntity>> COUNTER_BE =
        BLOCK_ENTITIES.register("counter", () -> BlockEntityType.Builder.of(CounterBlockEntity::new, COUNTER.get()).build(null));
    public static final DeferredBlock<SwitchBlock> SWITCH = BLOCKS.register("switch", () -> new SwitchBlock(BlockBehaviour.Properties.of()));
    static {
        net.neoforged.neoforge.common.NeoForge.EVENT_BUS.register(new ModEvents.LoadCounter());
    }
}
