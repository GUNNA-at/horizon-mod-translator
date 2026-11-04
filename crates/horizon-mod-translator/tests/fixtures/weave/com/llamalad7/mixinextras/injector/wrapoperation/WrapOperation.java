package com.llamalad7.mixinextras.injector.wrapoperation;

import org.spongepowered.asm.mixin.injection.At;

public @interface WrapOperation {
    String[] method();

    At[] at();
}
