package org.spongepowered.asm.mixin.injection;

public @interface At {
    enum Shift { NONE, BEFORE, AFTER, BY }

    String value();

    String target() default "";

    Shift shift() default Shift.NONE;

    int by() default 0;

    int ordinal() default -1;

    int opcode() default -1;

    String[] args() default {};

    String slice() default "";
}
