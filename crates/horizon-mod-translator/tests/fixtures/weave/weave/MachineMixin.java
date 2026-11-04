package weave;

import com.llamalad7.mixinextras.injector.ModifyExpressionValue;
import com.llamalad7.mixinextras.injector.ModifyReturnValue;
import com.llamalad7.mixinextras.injector.v2.WrapWithCondition;
import com.llamalad7.mixinextras.injector.wrapmethod.WrapMethod;
import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.llamalad7.mixinextras.injector.wrapoperation.WrapOperation;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Overwrite;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.Unique;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.ModifyArg;
import org.spongepowered.asm.mixin.injection.Redirect;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

@Mixin(Machine.class)
public abstract class MachineMixin implements Boosted {
    @Shadow public static int built;
    @Shadow public int ticks;
    @Unique private static int seenAtLoad = 7;
    @Unique private int boost = 3;
    @Unique private int afterAdds;

    @Shadow
    public abstract int base();

    // A tick past the fourth does nothing.
    @Inject(method = "tick", at = @At("HEAD"), cancellable = true)
    private void stopAtFour(CallbackInfo ci) {
        if (ticks >= 4) {
            ci.cancel();
        }
    }

    // Each tick adds `boost` times what it would.
    @ModifyArg(method = "tick", at = @At(value = "INVOKE", target = "Lweave/Machine;add(I)V"))
    private int boosted(int n) {
        return n * boost;
    }

    @Inject(method = "tick", at = @At(value = "INVOKE", target = "Lweave/Machine;add(I)V", shift = At.Shift.AFTER))
    private void countAdd(CallbackInfo ci) {
        afterAdds++;
    }

    // Logs on even ticks only.
    @WrapWithCondition(method = "tick", at = @At(value = "INVOKE", target = "Lweave/Machine;log(Ljava/lang/String;)V"))
    private boolean everyOther(Machine self, String message) {
        return ticks % 2 == 0;
    }

    // No load: -1.
    @Inject(method = "speed", at = @At("RETURN"), cancellable = true)
    private void idle(int load, CallbackInfoReturnable<Integer> cir) {
        if (load == 0) {
            cir.setReturnValue(-1);
        }
    }

    @ModifyExpressionValue(method = "power", at = @At(value = "INVOKE", target = "Lweave/Machine;base()I"))
    private int moreBase(int original) {
        return original + 5;
    }

    @ModifyReturnValue(method = "name", at = @At("RETURN"))
    private String shout(String original) {
        return original + "!";
    }

    @Redirect(method = "describe", at = @At(value = "INVOKE", target = "Lweave/Machine;name()Ljava/lang/String;"))
    private String renamed(Machine self) {
        return "mixed" + built;
    }

    @Inject(method = "twice", at = @At("HEAD"), cancellable = true)
    private static void negative(int n, CallbackInfoReturnable<Integer> cir) {
        if (n < 0) {
            cir.setReturnValue(0);
        }
    }

    // The call made with its first argument one more, then 1000 added.
    @WrapOperation(method = "scale", at = @At(value = "INVOKE", target = "Lweave/Machine;mul(II)I"))
    private int wrapMul(Machine self, int a, int b, Operation<Integer> original) {
        return original.call(self, a + 1, b) + 1000;
    }

    // Negative: 0; else the method on one more, doubled.
    @WrapMethod(method = "heavy")
    private long wrapHeavy(long x, Operation<Long> original) {
        if (x < 0) {
            return 0;
        }
        return original.call(x + 1) * 2;
    }

    /**
     * @author test
     * @reason a heavier machine
     */
    @Overwrite
    public int weight() {
        return 50 + base();
    }

    @Override
    public int boost() {
        return boost;
    }

    @Override
    public int afterAdds() {
        return afterAdds;
    }

    @Override
    public int seen() {
        return seenAtLoad;
    }
}
