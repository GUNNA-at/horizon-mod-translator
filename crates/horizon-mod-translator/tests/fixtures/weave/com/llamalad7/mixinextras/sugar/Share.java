package com.llamalad7.mixinextras.sugar;

public @interface Share {
    String value();

    String namespace() default "";
}
