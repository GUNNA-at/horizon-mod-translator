package testmod;
import net.neoforged.bus.api.IEventBus;
import net.neoforged.fml.common.Mod;

/** The mod's main class. FML constructs it first; its registers' classes initialise when it reaches them. */
@Mod("testmod")
public class TestMod {
    public static String order = "";

    public TestMod(IEventBus bus) {
        order += "constructed ";
        ModBlocks.BLOCKS.register(bus);
    }
}
