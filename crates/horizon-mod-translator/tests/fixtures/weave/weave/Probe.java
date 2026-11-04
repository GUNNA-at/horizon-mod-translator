package weave;

/** Calls into the target as other classes would; each answer is what Mixin's weaving gives. */
public class Probe {
    static Machine ticked(int n) {
        Machine m = new Machine("m1");
        for (int i = 0; i < n; i++) {
            m.tick();
        }
        return m;
    }

    public static int ticks(int n) {
        return ticked(n).ticks;
    }

    public static int total(int n) {
        return ticked(n).total;
    }

    public static int logged(int n) {
        return ticked(n).logged;
    }

    public static int afterAdds(int n) {
        return ((Boosted) (Object) ticked(n)).afterAdds();
    }

    public static int boost() {
        return ((Boosted) (Object) new Machine("b")).boost();
    }

    public static int seen() {
        return ((Boosted) (Object) new Machine("s")).seen();
    }

    public static int speed(int load) {
        return new Machine("s").speed(load);
    }

    public static int power(int n) {
        return ticked(n).power();
    }

    public static int weight(int n) {
        return ticked(n).weight();
    }

    public static int scale(int n) {
        return new Machine("x").scale(n);
    }

    public static int heavy(int n) {
        return (int) new Machine("h").heavy(n);
    }

    public static int twice(int n) {
        return Machine.twice(n);
    }

    public static String name() {
        return new Machine("m1").name();
    }

    public static String describe(int n) {
        Machine.built = 0;
        return ticked(n).describe();
    }
}
