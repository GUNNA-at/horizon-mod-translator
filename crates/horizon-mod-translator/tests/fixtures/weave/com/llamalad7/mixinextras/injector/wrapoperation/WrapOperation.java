package com.llamalad7.mixinextras.injector.wrapoperation;

import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Constant;

public @interface WrapOperation {
    String[] method();

    At[] at() default {};

    Constant[] constant() default {};
}
