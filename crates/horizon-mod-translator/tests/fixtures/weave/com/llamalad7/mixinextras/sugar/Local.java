package com.llamalad7.mixinextras.sugar;

public @interface Local {
    int ordinal() default -1;

    int index() default -1;

    String[] name() default {};

    boolean argsOnly() default false;
}
