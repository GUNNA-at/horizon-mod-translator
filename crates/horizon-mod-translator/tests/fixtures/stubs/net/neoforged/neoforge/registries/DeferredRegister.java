package net.neoforged.neoforge.registries;
import java.util.function.Supplier;
import net.minecraft.world.level.block.Block;
public class DeferredRegister<T> {
    public static Blocks createBlocks(String modid) { return null; }
    public static class Blocks extends DeferredRegister<Block> {
        public <B extends Block> DeferredBlock<B> register(String name, Supplier<? extends B> sup) { return null; }
    }
}
