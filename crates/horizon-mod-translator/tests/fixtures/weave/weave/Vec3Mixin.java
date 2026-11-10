package weave;

import net.minecraft.world.phys.Vec3;
import org.spongepowered.asm.mixin.Final;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.Unique;

/** A mixin into a class of the game's, which is not translated: what it adds joins its objects. */
@Mixin(Vec3.class)
public abstract class Vec3Mixin implements Measured {
    @Shadow
    @Final
    public double x;

    // (Before the instance fields: a static initialiser after the last of them makes newer
    // compilers give the constructor's return that line, and Mixin, which tells a constructor's
    // body from the field initialisers by line, then takes the initialisers for the body.)
    @Unique
    private static int weave$made = 40;

    @Unique
    private int weave$reads = 10;

    @Unique
    private String weave$label;

    @Shadow
    public abstract double lengthSqr();

    @Override
    public int weave$reads() {
        return ++weave$reads;
    }

    @Override
    public double weave$size() {
        return lengthSqr() + x + weave$bonus();
    }

    @Unique
    private double weave$bonus() {
        return 0.5;
    }

    @Override
    public void weave$label(String label) {
        weave$label = label;
    }

    @Override
    public String weave$label() {
        return weave$label + weave$made;
    }
}
