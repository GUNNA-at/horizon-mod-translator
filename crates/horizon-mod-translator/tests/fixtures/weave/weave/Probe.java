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

    public static int reading(int n) {
        return new Gauge().reading(n, n + 1);
    }

    public static int readingSeen(int n) {
        Gauge g = new Gauge();
        g.reading(n, n + 1);
        return g.seen;
    }

    public static String readingNote(int n) {
        Gauge g = new Gauge();
        g.reading(n, n + 1);
        return g.note;
    }

    public static int pick(int n) {
        return new Gauge().pick(n);
    }

    public static int stored(int n) {
        return new Gauge().stored(n);
    }

    public static int sharedTwice(int n) {
        Gauge g = new Gauge();
        g.shared(n);
        return g.shared(n);
    }

    public static String label(int n) {
        return new Gauge().label(n);
    }

    public static int labelLast(int n) {
        Gauge g = new Gauge();
        g.label(n);
        return g.last;
    }

    public static int modes() {
        return Mode.values().length;
    }

    public static int turbo() {
        return Mode.values()[2].speed;
    }

    public static String turboName() {
        return Mode.values()[2].name();
    }

    public static int doubledLevel() {
        return new Gauge().doubledLevel();
    }

    public static int setLevel(int n) {
        Gauge g = new Gauge();
        g.setLevel(n);
        return g.level;
    }

    public static int limit() {
        return new Gauge().limit();
    }

    public static int bumpSeen() {
        Gauge g = new Gauge();
        g.bump();
        return g.seen;
    }

    // How many times the wrapper ran for one call.
    public static int heavyWraps(int n) {
        Machine m = new Machine("h");
        m.heavy(n);
        return m.ticks;
    }

    public static int offset(int n) {
        return new Gauge().offset(n);
    }

    public static int accumulate(int n) {
        return new Gauge().accumulate(n);
    }

    public static int spread(int n) {
        return new Gauge().spread(n);
    }

    public static int nameLength() {
        return new Gauge().nameLength("abc");
    }

    public static int sliced(int n) {
        return new Dial().sliced(n);
    }

    public static int slicedSeen(int n) {
        Dial d = new Dial();
        d.sliced(n);
        return d.seen;
    }

    public static int sevens(int n) {
        return new Dial().sevens(n);
    }

    public static int half(int n) {
        return (int) (new Dial().half(n) * 10);
    }

    public static String dialName() {
        return new Dial().name();
    }

    public static int tens() {
        return new Dial().tens();
    }

    public static int made(int n) {
        return new Dial().made(n);
    }

    public static int counted(int n) {
        return new Dial().counted(n);
    }

    public static int isText(int n) {
        Object o = n > 0 ? new Dial() : n == 0 ? (Object) "s" : new Gauge();
        return new Dial().isText(o) ? 1 : 0;
    }

    public static int first(int n) {
        return new Dial().first(n, 5);
    }
}
