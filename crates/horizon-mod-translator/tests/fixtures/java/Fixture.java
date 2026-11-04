// Differential fixture: each static method's result is compared between the
// JVM and the translated Rust. Written for these tests.
public class Fixture {
    static abstract class Shape {
        int sides;
        Shape(int sides) { this.sides = sides; }
        abstract long area(int k);
        int sides() { return sides; }
    }
    static class Square extends Shape {
        Square() { super(4); }
        long area(int k) { return (long) k * k; }
    }
    static class Tri extends Shape {
        Tri() { super(3); }
        long area(int k) { return (long) k * k / 2; }
        int sides() { return super.sides() * 10; }
    }

    static int counter;

    public static int sumTo(int n) { int s = 0; for (int i = 1; i <= n; i++) s += i; return s; }
    public static long fib(int n) { long a = 0, b = 1; for (int i = 0; i < n; i++) { long t = a + b; a = b; b = t; } return a; }
    public static int collatz(int n) { int steps = 0; while (n != 1) { n = (n % 2 == 0) ? n / 2 : 3 * n + 1; steps++; } return steps; }
    public static int sw(int k) {
        switch (k) { case 0: return 10; case 1: return 11; case 2: return 12; case 100: return 7; case -5: return 99; default: return -1; }
    }
    public static int arrays(int n) {
        int[] a = new int[n];
        for (int i = 0; i < n; i++) a[i] = i * i;
        int s = 0;
        for (int i = n - 1; i >= 0; i -= 2) s += a[i];
        return s + a.length;
    }
    public static long shapes(int k) {
        Shape[] s = { new Square(), new Tri(), new Square() };
        long total = 0;
        for (Shape x : s) total += x.area(k) + x.sides();
        return total;
    }
    public static int statics(int n) { for (int i = 0; i < n; i++) counter += i; return counter; }
    public static double floats(int n) { double d = 0; float f = 1.5f; for (int i = 0; i < n; i++) { d += Math.sqrt(i) * f; f *= 1.01f; } return Math.floor(d * 1000) / 1000; }
    public static int bits(int x) { return (x << 3) ^ (x >>> 2) ^ (x >> 1) | (x & 0xff); }
    public static long longBits(long x) { return (x << 13) ^ (x >>> 7) ^ ~x; }
    public static int overflow(int x) { return x * 1_000_003 + Integer.MAX_VALUE; }
    public static int casts(double d) { return (int) d + (int) (float) d + (byte) (int) d + (char) (int) d; }
    public static int nested(int n) { int c = 0; outer: for (int i = 0; i < n; i++) { for (int j = 0; j < n; j++) { if (i * j > n) continue outer; c += i ^ j; } } return c; }

    interface IntOp { int apply(int x); }
    static int twice(IntOp f, int x) { return f.apply(f.apply(x)); }
    static int square(int x) { return x * x; }
    public static int lambdas(int k) {
        int base = k + 1;
        IntOp add = x -> x + base;
        IntOp sq = Fixture::square;
        java.util.function.Supplier<Shape> make = Square::new;
        return twice(add, k) + twice(sq, 3) + make.get().sides();
    }
    // A mod's own function type over the JDK's (Registrate's `NonNullFunction`): its default
    // method's lambda body is a private method of the interface.
    interface Chain extends java.util.function.Function<Integer, Integer> {
        default Chain then(Chain next) { return x -> next.apply(apply(x)); }
    }
    // Registrate's `NonNullUnaryOperator`: the lambda the default method runs on is of a sub-interface.
    interface Unary extends Chain {}
    public static int chained(int k) {
        Unary inc = x -> x + 1;
        Chain dbl = x -> x * 2;
        return inc.then(dbl).then(inc).apply(k);
    }
    interface Step { int run(int x); default Step then(Step next) { return x -> next.run(run(x)); } default int twiceRun(int x) { return run(run(x)); } }
    public static int defaults(int k) {
        Step inc = x -> x + 1;
        Step dbl = x -> x * 2;
        // Default methods called on lambdas, chained.
        return inc.then(dbl).run(k) * 1000 + dbl.twiceRun(k) + inc.then(dbl).then(inc).twiceRun(k);
    }
    static int callerName() { return StackWalker.getInstance(StackWalker.Option.RETAIN_CLASS_REFERENCE).getCallerClass().getSimpleName().hashCode(); }
    public static int caller(int k) { return callerName() + k; }
    interface Named { String name(); default int score() { return name().length() * 10; } }
    interface Ranked extends Named { default int rank() { return score() + 1; } }
    static class Plain implements Ranked { public String name() { return "plain"; } }
    static class Loud extends Plain { public int score() { return 1000; } }
    public static int inherited(int k) {
        Ranked a = new Plain();
        Ranked b = new Loud();
        // Default methods inherited from superinterfaces, and one a subclass overrides.
        return a.rank() * 10000 + b.rank() + k;
    }
    enum Form { CUBE, SLAB, STAIR_STEP }
    public static int enums(int k) {
        Form s = Form.values()[k];
        // name() and toString() feed ids such as "create:" + name().toLowerCase(Locale.ROOT).
        String id = "mod:" + s.name().toLowerCase(java.util.Locale.ROOT) + "/" + s;
        return id.hashCode() + s.compareTo(Form.SLAB) * 7 + s.ordinal();
    }
    interface Spins { }
    static class Gear implements Spins { }
    static class BigGear extends Gear { }
    public static int interfaces(int k) {
        Object o = k == 0 ? new BigGear() : "text";
        // instanceof and isAssignableFrom see interfaces a superclass implements.
        int r = (o instanceof Spins ? 1 : 0) + (o instanceof Gear ? 2 : 0);
        r += Spins.class.isAssignableFrom(BigGear.class) ? 4 : 0;
        r += Gear.class.isAssignableFrom(Spins.class) ? 8 : 0;
        return r + k * 100;
    }
    public static int concat(int k) {
        String s = "k=" + k + " half=" + (k / 2.0) + " big=" + (k * 1e9) + " ok=" + (k > 2) + " c=" + (char) ('a' + k % 26) + " L=" + (long) k * 3_000_000_000L + " n=" + null;
        return s.length() * 31 + (s.equals("k=0 half=0.0 big=0.0 ok=false c=a L=0 n=null") ? 1 : 0);
    }

    static class Boom extends IllegalStateException { int code; Boom(int c) { code = c; } }
    static int risky(int k) {
        if (k == 1) throw new Boom(41);
        if (k == 2) return 10 / (k - 2);
        if (k == 3) throw new IllegalArgumentException();
        return k * 100;
    }
    public static int exceptions(int k) {
        int r = 0;
        try {
            try { r = risky(k); }
            catch (Boom b) { r = b.code + 1; }
            catch (ArithmeticException e) { r = -2; }
            finally { r += 1000; }
        } catch (RuntimeException e) {
            r = (e instanceof IllegalArgumentException) ? -3 : -4;
        }
        Object o = new Boom(0);
        return r + (o instanceof RuntimeException ? 1 : 0) + (o instanceof Boom ? 10 : 0);
    }

    // Pattern switches (Java 21): by type with a guard, and over an enum with a pattern case.
    static int kind(Object o) {
        return switch (o) {
            case null -> -1;
            case BigGear g -> 1;
            case Gear g -> 2;
            case String s when s.length() > 3 -> 30 + s.length();
            case String s -> 3;
            case Point p -> 40 + p.x();
            default -> 5;
        };
    }
    static int form(Form f) {
        return switch (f) {
            case CUBE -> 1;
            case Form x when x.ordinal() > 1 -> 10 + x.ordinal();
            case SLAB -> 2;
            default -> 9;
        };
    }
    public static int patterns(int k) {
        Object[] all = { null, new BigGear(), new Gear(), "ab", "abcdef", new Point(k, "p"), 7 };
        int r = 0;
        for (Object o : all) {
            r = r * 7 + kind(o);
        }
        return r + form(Form.values()[Math.floorMod(k, 3)]) * 1000;
    }

    record Point(int x, String label) {}
    public static int records(int k) {
        Point a = new Point(k, "p" + k), b = new Point(k, "p" + k), c = new Point(k + 1, "q");
        return (a.equals(b) ? 1 : 0) + (a.equals(c) ? 2 : 0) + a.hashCode() + a.toString().length() * 1000 + (a.toString().equals("Point[x=" + k + ", label=p" + k + "]") ? 7 : 0);
    }
    // Reflection, as Flywheel's and catnip's loaders and Create's enum readers use it.
    public static class Impl implements Spins { public int v = 7; public Impl() { v = 42; } }
    static class Keyed {
        final int k;
        Keyed(int k) { this.k = k; }
        // super.hashCode() and super.equals() are Object's (identity), never these overrides again.
        @Override public int hashCode() { return super.hashCode() != 0 ? k * 31 : -1; }
        @Override public boolean equals(Object o) { return super.equals(o) || (o instanceof Keyed other && other.k == k); }
    }
    public static int reflection(int k) {
        int r = 0;
        try {
            Object o = Class.forName("Fixture$Impl").getConstructor().newInstance();
            r += (o instanceof Spins ? 1 : 0) + ((Impl) o).v;
            r += Form.class.getEnumConstants().length * 100 + (Form.class.isEnum() ? 1000 : 0) + (Impl.class.isEnum() ? 2000 : 0);
            try { Class.forName("not.a.Thing"); r += 5000; } catch (ClassNotFoundException e) { r += 10000; }
            Spins s = java.util.ServiceLoader.load(Spins.class).findFirst().orElse(null);
            r += s instanceof Impl i ? i.v * 1000 : -1;
        } catch (ReflectiveOperationException e) {
            return -1;
        }
        r += "%d-%s".formatted(k, "x").length() * 100000;
        Keyed a = new Keyed(k), b = new Keyed(k);
        java.util.Set<Keyed> set = new java.util.HashSet<>();
        set.add(a);
        set.add(b);
        return r + set.size() * 1000000 + (a.equals(b) ? 2000000 : 0) + (a.hashCode() == k * 31 ? 4000000 : 0);
    }
    // Static fields named through a subclass or an implementing class resolve to the class that
    // declares them (`CreativeMotorBlock.FACING` is `DirectionalKineticBlock.FACING`).
    static class Holder { static final Object KEY = new Object(); static int count = 7; }
    static class SubHolder extends Holder { }
    interface Limits { int LIMIT = limit(); }
    static int limit() { return 40; }
    static class Limited implements Limits { }
    public static int staticFields(int k) {
        SubHolder.count += k;
        return SubHolder.count * 100 + (SubHolder.KEY == Holder.KEY && SubHolder.KEY != null ? 1 : 0) + Limited.LIMIT * 10000;
    }
    // A static method reference to another class (Create hands `FanProcessingTypeRegistry::init`
    // to a registry's bake callback): calling it initialises that class first, as invokestatic does.
    static class Sorted {
        static final int[] VALUES = new int[] { 4, 2 };
        static int calls;
        static void bump() { calls += VALUES[0] + VALUES.length; }
    }
    public static int staticRefs(int k) {
        Runnable r = Sorted::bump;
        for (int i = 0; i < k; i++) r.run();
        return Sorted.calls;
    }
    // A library the mod bundles (`lib/`, packed jar-in-jar): only what the mod reaches is
    // translated, and it runs as on the JVM.
    static final class Greeter extends lib.Base { }
    public static int library(int k) {
        int a = lib.Lib.twice(k);
        java.util.function.IntSupplier c = lib.Lib.counter(k);
        int b = c.getAsInt() + c.getAsInt();
        String s = "" + lib.Lib.shape(k);
        int d = new Greeter().hello(k);
        return a + b * 3 + s.length() * 5 + d * 7 + lib.Lib.plusOne(k) * 11;
    }
    // Numbers the runtime keeps unboxed (a primitive stream's, fastutil's maps) are still objects:
    // not null.
    public static int rawBoxes(int k) {
        java.util.List<Integer> xs = java.util.stream.IntStream.range(0, 3 + k).boxed().toList();
        int n = 0;
        for (Object o : xs) {
            if (o != null) n++;
        }
        java.util.Map<String, Double> m = new java.util.HashMap<>();
        m.put("a", 2.5 * k);
        Object v = m.get("a");
        return n * 10 + xs.size() + (v == null ? 1000 : 2000);
    }
    public static int grid(int n) {
        int[][] g = new int[n][n + 1];
        for (int i = 0; i < n; i++) for (int j = 0; j <= n; j++) g[i][j] = i * j;
        int s = 0;
        for (int[] row : g) for (int v : row) s += v;
        return s + g.length * 100 + (n > 0 ? g[0].length : 0);
    }
}
