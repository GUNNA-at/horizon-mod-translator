package org.spongepowered.asm.mixin.injection;

public @interface ModifyArg {
    String[] method();

    At at();

    int index() default -1;
}
