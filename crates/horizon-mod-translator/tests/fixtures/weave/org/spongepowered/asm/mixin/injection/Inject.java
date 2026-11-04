package org.spongepowered.asm.mixin.injection;

public @interface Inject {
    String[] method();

    At[] at();

    boolean cancellable() default false;
}
