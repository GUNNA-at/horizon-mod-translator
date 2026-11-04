package weave;

import org.spongepowered.asm.mixin.Mixin;

@Mixin(Gauge.class)
@ModMixin(mods = Mods.GONE, applyIfPresent = false)
public abstract class WithoutGoneMixin {
}
