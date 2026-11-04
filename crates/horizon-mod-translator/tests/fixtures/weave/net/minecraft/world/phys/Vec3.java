package net.minecraft.world.phys;

/** The game's class, as the mods compile against it (not translated: the runtime has it). */
public class Vec3 {
    public final double x;
    public final double y;
    public final double z;

    public Vec3(double x, double y, double z) {
        this.x = x;
        this.y = y;
        this.z = z;
    }

    public double lengthSqr() {
        return x * x + y * y + z * z;
    }
}
