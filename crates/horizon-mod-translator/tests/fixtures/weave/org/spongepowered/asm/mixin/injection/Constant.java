package org.spongepowered.asm.mixin.injection;

public @interface Constant {
    boolean nullValue() default false;

    int intValue() default 0;

    float floatValue() default 0;

    long longValue() default 0;

    double doubleValue() default 0;

    String stringValue() default "";

    Class<?> classValue() default Object.class;

    int ordinal() default -1;

    String slice() default "";
}
