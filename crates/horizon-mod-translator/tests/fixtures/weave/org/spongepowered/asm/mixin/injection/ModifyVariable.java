package org.spongepowered.asm.mixin.injection;

public @interface ModifyVariable {
    String[] method();

    At at();

    int ordinal() default -1;

    int index() default -1;

    String[] name() default {};

    boolean argsOnly() default false;
}
