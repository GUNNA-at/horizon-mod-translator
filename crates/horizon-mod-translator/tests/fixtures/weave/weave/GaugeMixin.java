package weave;

import com.llamalad7.mixinextras.injector.ModifyExpressionValue;
import com.llamalad7.mixinextras.injector.ModifyReceiver;
import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.llamalad7.mixinextras.injector.wrapoperation.WrapOperation;
import com.llamalad7.mixinextras.sugar.Local;
import com.llamalad7.mixinextras.sugar.Share;
import com.llamalad7.mixinextras.sugar.ref.LocalIntRef;
import com.llamalad7.mixinextras.sugar.ref.LocalRef;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.ModifyVariable;
import org.spongepowered.asm.mixin.injection.Redirect;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;
import org.spongepowered.asm.mixin.injection.callback.LocalCapture;

/** Handlers taking the target's locals: MixinExtras' `@Local` and `@Share`, Mixin's captured ones. */
@Mixin(Gauge.class)
public abstract class GaugeMixin {
    @Shadow public int seen;
    @Shadow public String note;
    @Shadow public int level;

    // The third `int` (a, b, sum: ordinals count the arguments), the only `String`, and the second
    // argument by Mixin's name for it.
    @Inject(method = "reading", at = @At(value = "INVOKE", target = "Lweave/Gauge;record(Ljava/lang/String;)V"))
    private void seeLocals(int a, int b, CallbackInfoReturnable<Integer> cir, @Local(ordinal = 2) int sum, @Local String tag, @Local(name = "arg1") int second) {
        seen = sum * 100 + second;
        note = tag;
    }

    // `scaled`, changed through its ref: the method returns the new value + 1.
    @Inject(method = "reading", at = @At(value = "INVOKE", target = "Lweave/Gauge;record(Ljava/lang/String;)V"))
    private void bumpScaled(int a, int b, CallbackInfoReturnable<Integer> cir, @Local(ordinal = 3) LocalIntRef scaled) {
        scaled.set(scaled.get() + 7);
    }

    // The third return only, where `doubled` is.
    @Inject(method = "pick", at = @At(value = "RETURN", ordinal = 2), cancellable = true, locals = LocalCapture.CAPTURE_FAILHARD)
    private void pickDoubled(int n, CallbackInfoReturnable<Integer> cir, int doubled) {
        cir.setReturnValue(doubled + 1000);
    }

    // After `v = twice(n)` has stored `v`.
    @Inject(method = "stored", at = @At(value = "INVOKE_ASSIGN", target = "Lweave/Gauge;twice(I)I"))
    private void storedTimesTen(int n, CallbackInfoReturnable<Integer> cir, @Local(ordinal = 1) LocalIntRef v) {
        v.set(v.get() * 10);
    }

    // One count for both handlers, new on each call.
    @Inject(method = "shared", at = @At("HEAD"))
    private void shareStart(int n, CallbackInfoReturnable<Integer> cir, @Share("count") LocalIntRef count) {
        count.set(count.get() + n * 3);
    }

    @Inject(method = "shared", at = @At("RETURN"), cancellable = true)
    private void shareEnd(int n, CallbackInfoReturnable<Integer> cir, @Share("count") LocalIntRef count) {
        cir.setReturnValue(count.get() + 1);
    }

    @Inject(method = "label", at = @At(value = "INVOKE", target = "Lweave/Gauge;record(Ljava/lang/String;)V"))
    private void question(int n, CallbackInfoReturnable<String> cir, @Local LocalRef<String> text) {
        text.set(text.get() + "?");
    }

    // Field accesses (`@At("FIELD")`): a read through the handler, a write instead of the field's
    // (`opcode` 181, `putfield`), a static read as an `Operation`, and after a write.
    @ModifyExpressionValue(method = "doubledLevel", at = @At(value = "FIELD", target = "Lweave/Gauge;level:I"))
    private int moreLevel(int original) {
        return original + 1;
    }

    @Redirect(method = "setLevel", at = @At(value = "FIELD", target = "Lweave/Gauge;level:I", opcode = 181))
    private void tripled(Gauge self, int v) {
        self.level = v * 3;
    }

    @WrapOperation(method = "limit", at = @At(value = "FIELD", target = "Lweave/Gauge;LIMIT:I"))
    private int wrapLimit(Operation<Integer> original) {
        return original.call() + 5;
    }

    @Inject(method = "bump", at = @At(value = "FIELD", target = "Lweave/Gauge;level:I", opcode = 181, shift = At.Shift.AFTER))
    private void afterBump(CallbackInfoReturnable<Integer> cir) {
        seen = level * 10;
    }

    // A local through the handler: the argument at the head, `total` after its first store
    // (ints: n, total), `width` before the call (ints: n, width).
    @ModifyVariable(method = "offset", at = @At("HEAD"), argsOnly = true)
    private int moreOffset(int n) {
        return n + 100;
    }

    @ModifyVariable(method = "accumulate", at = @At(value = "STORE", ordinal = 0), ordinal = 1)
    private int tenfold(int total) {
        return total * 10;
    }

    @ModifyVariable(method = "spread", at = @At(value = "INVOKE", target = "Lweave/Gauge;record(Ljava/lang/String;)V"), ordinal = 1)
    private int wider(int width) {
        return width * 4;
    }

    // The receiver of `length()`.
    @ModifyReceiver(method = "nameLength", at = @At(value = "INVOKE", target = "Ljava/lang/String;length()I"))
    private String louder(String s) {
        return s + "!!";
    }
}
