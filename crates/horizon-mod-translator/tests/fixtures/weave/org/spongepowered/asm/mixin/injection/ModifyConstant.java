package org.spongepowered.asm.mixin.injection;

public @interface ModifyConstant {
    String[] method();

    Slice[] slice() default {};

    Constant[] constant() default {};
}
