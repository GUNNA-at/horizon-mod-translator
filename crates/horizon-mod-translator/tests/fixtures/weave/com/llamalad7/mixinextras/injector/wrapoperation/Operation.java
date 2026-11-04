package com.llamalad7.mixinextras.injector.wrapoperation;

public interface Operation<R> {
    R call(Object... args);
}
