package org.spongepowered.asm.mixin.injection;

public @interface Slice {
    String id() default "";

    At from() default @At("HEAD");

    At to() default @At("TAIL");
}
