package org.spongepowered.asm.mixin.injection;

public @interface At {
    enum Shift { NONE, BEFORE, AFTER, BY }

    String value();

    String target() default "";

    Shift shift() default Shift.NONE;

    int ordinal() default -1;
}
