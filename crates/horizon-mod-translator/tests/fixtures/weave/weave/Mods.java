package weave;

/** The mods an addon knows: an id of its own, or its name. */
public enum Mods {
    CREATE,
    EXTRA("extra_mod"),
    GONE;

    public final String id;

    Mods() {
        this.id = name().toLowerCase();
    }

    Mods(String id) {
        this.id = id;
    }
}
