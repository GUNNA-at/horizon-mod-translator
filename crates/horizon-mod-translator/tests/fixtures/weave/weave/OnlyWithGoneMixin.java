package weave;

import org.spongepowered.asm.mixin.Mixin;

@Mixin(Gauge.class)
@ModMixin(mods = Mods.GONE)
public abstract class OnlyWithGoneMixin {
}
