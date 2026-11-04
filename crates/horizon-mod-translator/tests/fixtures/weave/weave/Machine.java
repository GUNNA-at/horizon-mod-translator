package weave;

/** The class a mixin targets, as an addon's mixin targets a mod's class. */
public class Machine {
    public static int built;
    public int ticks;
    public int total;
    public int logged;
    private final String label;

    public Machine(String label) {
        this.label = label;
        built++;
    }

    public String name() {
        return label;
    }

    public void add(int n) {
        total += n;
    }

    public void log(String message) {
        logged++;
    }

    public int base() {
        return total;
    }

    public void tick() {
        ticks++;
        add(1);
        log("tick");
    }

    public int speed(int load) {
        return 10 - load;
    }

    public int power() {
        return base() * 2;
    }

    public int weight() {
        return 1;
    }

    public String describe() {
        return name() + ":" + total;
    }

    public int mul(int a, int b) {
        return a * b;
    }

    public int scale(int n) {
        return mul(n, 4);
    }

    public long heavy(long x) {
        return x * 3;
    }

    public static int twice(int n) {
        return n * 2;
    }
}
