package testmod;
import net.neoforged.bus.api.SubscribeEvent;
import net.neoforged.fml.common.EventBusSubscriber;
import net.neoforged.neoforge.event.level.LevelEvent;

/** Listeners the loader finds by annotation: a static one here, an object's in {@link LoadCounter}. */
@EventBusSubscriber(modid = "testmod")
public class ModEvents {
    public static int loads;
    public static int objectLoads;

    @SubscribeEvent
    public static void onLoad(LevelEvent.Load event) {
        loads += event.getLevel() != null ? 11 : 1;
    }

    public static class LoadCounter {
        @SubscribeEvent
        public void onLoad(LevelEvent.Load event) {
            objectLoads += 1;
        }
    }
}
