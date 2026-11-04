package com.llamalad7.mixinextras.injector.v2;

import org.spongepowered.asm.mixin.injection.At;

public @interface WrapWithCondition {
    String[] method();

    At[] at();
}
