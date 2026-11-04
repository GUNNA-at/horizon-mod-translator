package com.llamalad7.mixinextras.injector;

import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Slice;

public @interface ModifyExpressionValue {
    String[] method();

    At[] at();

    Slice[] slice() default {};
}
