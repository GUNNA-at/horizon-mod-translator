package weave;

import org.spongepowered.asm.mixin.Mixin;

@Mixin(Gauge.class)
@ModMixin(mods = {Mods.GONE, Mods.EXTRA})
public abstract class OnlyWithExtraMixin {
}
