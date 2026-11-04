package net.neoforged.neoforge.registries;
import java.util.function.Supplier;
import net.minecraft.world.level.block.Block;
public class DeferredRegister<T> {
    public static <T> DeferredRegister<T> create(Object registry, String modid) { return null; }
    public <I extends T> DeferredHolder<T, I> register(String name, Supplier<? extends I> sup) { return null; }
    public static Blocks createBlocks(String modid) { return null; }
    public void register(net.neoforged.bus.api.IEventBus bus) {}
    public static class Blocks extends DeferredRegister<Block> {
        public <B extends Block> DeferredBlock<B> register(String name, Supplier<? extends B> sup) { return null; }
    }
}
