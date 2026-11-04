package org.spongepowered.asm.mixin.injection;

import org.spongepowered.asm.mixin.injection.callback.LocalCapture;

public @interface Inject {
    String[] method();

    Slice[] slice() default {};

    At[] at();

    boolean cancellable() default false;

    LocalCapture locals() default LocalCapture.NO_CAPTURE;
}
