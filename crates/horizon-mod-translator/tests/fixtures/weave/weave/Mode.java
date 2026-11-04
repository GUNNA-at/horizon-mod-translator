package weave;

/** An enum an addon adds a constant to, as Create: Connected adds sequencer instructions. */
public enum Mode {
    SLOW(1),
    FAST(5);

    public final int speed;

    Mode(int speed) {
        this.speed = speed;
    }
}
