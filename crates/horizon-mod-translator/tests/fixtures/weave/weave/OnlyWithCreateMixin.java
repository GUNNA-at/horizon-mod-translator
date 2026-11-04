package weave;

import org.spongepowered.asm.mixin.Mixin;

@Mixin(Gauge.class)
@ModMixin(mods = Mods.CREATE)
public abstract class OnlyWithCreateMixin {
}
