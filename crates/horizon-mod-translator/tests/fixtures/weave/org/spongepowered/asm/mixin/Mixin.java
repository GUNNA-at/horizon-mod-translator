package org.spongepowered.asm.mixin;

public @interface Mixin {
    Class<?>[] value() default {};

    String[] targets() default {};
}
