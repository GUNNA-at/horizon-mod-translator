package weave;

import com.llamalad7.mixinextras.injector.ModifyExpressionValue;
import com.llamalad7.mixinextras.injector.wrapoperation.Operation;
import com.llamalad7.mixinextras.injector.wrapoperation.WrapOperation;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Constant;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.ModifyConstant;
import org.spongepowered.asm.mixin.injection.ModifyVariable;
import org.spongepowered.asm.mixin.injection.Slice;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/** Handlers at constants, `new`s and an `instanceof`, and in slices of their methods. */
@Mixin(Dial.class)
public abstract class DialMixin {
    @Shadow public int seen;

    // The `step` after `mark()` only (a slice from it), and an injection before the one up to it.
    @ModifyExpressionValue(method = "sliced", at = @At(value = "INVOKE", target = "Lweave/Dial;step(I)I"), slice = @Slice(from = @At(value = "INVOKE", target = "Lweave/Dial;mark()V")))
    private int laterStep(int v) {
        return v * 2;
    }

    @Inject(method = "sliced", at = @At(value = "INVOKE", target = "Lweave/Dial;step(I)I"), slice = @Slice(to = @At(value = "INVOKE", target = "Lweave/Dial;mark()V")))
    private void countEarly(int n, CallbackInfoReturnable<Integer> cir) {
        seen++;
    }

    // `7` made `8`.
    @ModifyConstant(method = "sevens", constant = @Constant(intValue = 7))
    private int eight(int seven) {
        return seven + 1;
    }

    // Every `double` constant (none named), with the method's argument.
    @ModifyConstant(method = "half")
    private double moreHalf(double c, double d) {
        return c + d;
    }

    @ModifyExpressionValue(method = "name", at = @At(value = "CONSTANT", args = "stringValue=dial"))
    private String louderName(String s) {
        return s + "!";
    }

    // The `10` after `mark()` only.
    @ModifyExpressionValue(method = "tens", at = @At(value = "CONSTANT", args = "intValue=10"), slice = @Slice(from = @At(value = "INVOKE", target = "Lweave/Dial;mark()V")))
    private int moreTen(int ten) {
        return ten + 5;
    }

    // Before `new ArrayList<>()`; `count` (ints: n, count) tripled before `new StringBuilder()`.
    @Inject(method = "made", at = @At(value = "NEW", target = "java/util/ArrayList"))
    private void beforeList(int n, CallbackInfoReturnable<Integer> cir) {
        seen += 40;
    }

    @ModifyVariable(method = "counted", at = @At(value = "NEW", target = "()Ljava/lang/StringBuilder;"), ordinal = 1)
    private int tripleCount(int count) {
        return count * 3;
    }

    // MixinExtras' wrapping of an `instanceof`.
    @WrapOperation(method = "isText", constant = @Constant(classValue = String.class))
    private boolean textOrDial(Object o, Operation<Boolean> original) {
        return original.call(o) || o instanceof Dial;
    }

    // The method's first argument only, after the call's value.
    @ModifyExpressionValue(method = "first", at = @At(value = "INVOKE", target = "Lweave/Dial;step(I)I"))
    private int plusFirst(int v, int a) {
        return v + a * 1000;
    }
}
