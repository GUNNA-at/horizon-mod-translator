package weave;

import java.util.ArrayList;
import java.util.Arrays;
import org.spongepowered.asm.mixin.Final;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Mutable;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.gen.Invoker;

/** A new constant made by the enum's constructor (a static invoker of `<init>`). */
@Mixin(Mode.class)
public abstract class ModeMixin {
    @Shadow @Final @Mutable private static Mode[] $VALUES;

    private static final Mode TURBO = addMode("TURBO", 9);

    @Invoker("<init>")
    public static Mode invokeInit(String name, int ordinal, int speed) {
        throw new AssertionError();
    }

    private static Mode addMode(String name, int speed) {
        ArrayList<Mode> values = new ArrayList<>(Arrays.asList($VALUES));
        Mode mode = invokeInit(name, values.get(values.size() - 1).ordinal() + 1, speed);
        values.add(mode);
        $VALUES = values.toArray(new Mode[0]);
        return mode;
    }
}
