package lib;

/** A library class the mod extends: what the mod's objects inherit can run. */
public abstract class Base {
    public int hello(int x) {
        return x + 40;
    }

    /** Never called on anything: left out. */
    public int unusedHello() {
        return 0;
    }
}
