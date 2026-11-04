package org.spongepowered.asm.mixin.injection.callback;

public class CallbackInfo {
    public CallbackInfo(String name, boolean cancellable) {}

    public void cancel() {}

    public boolean isCancelled() {
        return false;
    }
}
