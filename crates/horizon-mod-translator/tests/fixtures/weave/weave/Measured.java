package weave;

/** What `Vec3Mixin` adds to the game's `Vec3` (a duck interface). */
public interface Measured {
    int weave$reads();

    double weave$size();

    void weave$label(String label);

    String weave$label();
}
