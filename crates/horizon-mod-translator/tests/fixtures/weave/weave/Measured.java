package weave;

/** What `Vec3Mixin` adds to the game's `Vec3` (a duck interface). */
public interface Measured {
    int weave$reads();

    double weave$size();

    void weave$label(String label);

    String weave$label();

    int weave$sum();

    // Default methods: the game's `Vec3` has them through the interface, as any class has its
    // interfaces'. One the game's class declares itself (`length()`) stays the class's own.
    default int weave$twice() {
        return weave$sum() * 2;
    }

    default double length() {
        return -1.0;
    }
}
