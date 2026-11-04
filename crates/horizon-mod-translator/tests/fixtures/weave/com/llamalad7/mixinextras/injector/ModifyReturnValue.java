package com.llamalad7.mixinextras.injector;

import org.spongepowered.asm.mixin.injection.At;

public @interface ModifyReturnValue {
    String[] method();

    At[] at();
}
