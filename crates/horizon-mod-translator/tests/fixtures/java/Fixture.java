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
    // An assignment to a long or double array element used as a value: javac's dup2_x2.
    public static long wideStores(long x) { long[] a = new long[2]; double[] d = new double[1]; long r = (a[0] = x); r += (a[1] += r * 3); d[0] = 0.5; return r + (long) (d[0] *= 4) + a[0] + a[1]; }
    // The JDK's own plain classes, which the translator takes as code from the Java runtime (`jdk_code`).
    public static long jdkColor(int n) {
        java.awt.Color c = new java.awt.Color(n & 255, 40, 200);
        long h = c.getRGB() + 31L * c.brighter().getGreen() + java.awt.Color.HSBtoRGB(n / 7f, 0.5f, 0.8f);
        float[] hsb = java.awt.Color.RGBtoHSB(c.getRed(), c.getGreen(), c.getBlue(), null);
        h = h * 31 + Float.floatToIntBits(hsb[0]) + Float.floatToIntBits(hsb[2]) + new java.awt.Color(0x80102030, true).getAlpha();
        java.awt.Rectangle r = new java.awt.Rectangle(1, 2, 30 + n, 40);
        h = h * 31 + (r.contains(new java.awt.Point(5, 5)) ? 7 : 3) + r.intersection(new java.awt.Rectangle(10, 10, 100, 100)).width;
        return h * 31 + (long) new java.awt.geom.Point2D.Double(n, 3).distance(0, 0) + (c.equals(new java.awt.Color(c.getRGB())) ? 1 : 0);
    }
    public static long jdkMath(int n) {
        java.math.BigInteger f = java.math.BigInteger.ONE;
        for (int i = 2; i <= 25 + (n & 7); i++) f = f.multiply(java.math.BigInteger.valueOf(i));
        long h = f.toString().hashCode() + f.mod(java.math.BigInteger.valueOf(1000003)).intValue() + f.bitLength() + f.pow(3).shiftRight(40).longValue();
        java.math.BigDecimal d = new java.math.BigDecimal("1234.5678").multiply(java.math.BigDecimal.valueOf(n)).setScale(2, java.math.RoundingMode.HALF_UP);
        h = h * 31 + d.toString().hashCode() + d.intValue() + Double.doubleToLongBits(d.doubleValue()) + d.compareTo(java.math.BigDecimal.TEN);
        return h * 31 + new java.math.BigDecimal(f).divide(new java.math.BigDecimal("7.5"), 6, java.math.RoundingMode.HALF_EVEN).toPlainString().hashCode();
    }
    public static long jdkText(int n) {
        java.net.URI u = java.net.URI.create("https://example.org:8080/a/b.c/" + n + "?q=1#frag");
        long h = u.getHost().hashCode() + u.getPort() + u.getPath().hashCode() + u.getRawPath().length() + u.resolve("../x").toString().hashCode() + u.getScheme().length();
        String b64 = java.util.Base64.getEncoder().encodeToString(("horizon" + n).getBytes(java.nio.charset.StandardCharsets.UTF_8));
        h = h * 31 + b64.hashCode() + java.util.Base64.getDecoder().decode(b64).length;
        h = h * 31 + (java.lang.reflect.Modifier.isPublic(n) ? 1 : 0) + (java.lang.reflect.Modifier.isStatic(n) ? 2 : 0) + (java.lang.reflect.Modifier.isFinal(n) ? 4 : 0);
        h = h * 31 + (Character.UnicodeBlock.of((char) ('a' + n)) == Character.UnicodeBlock.BASIC_LATIN ? 1 : 0) + (Character.UnicodeBlock.of('\u044f') == Character.UnicodeBlock.CYRILLIC ? 2 : 0)
            + Character.UnicodeBlock.of(0x1F600 + n).toString().hashCode();
        java.util.StringTokenizer t = new java.util.StringTokenizer("a,b;c" + n, ",;");
        while (t.hasMoreTokens()) h = h * 31 + t.nextToken().hashCode();
        return h;
    }
    // Parts of character arrays and of texts in a builder, and a number read out of the middle of one.
    public static int charParts(int n) {
        char[] a = new char[6];
        for (int i = 0; i < a.length; i++) a[i] = (char) ('a' + (i + n) % 26);
        StringBuilder b = new StringBuilder().append(a).append('|').append(a, 1, 3).append('|').append("0123456789", 2, 5 + (n & 3)).insert(0, a);
        String s = new String(a) + String.valueOf(a) + String.valueOf(a, 2, 3) + b;
        return s.hashCode() + Integer.parseInt("x" + (1200 + n) + "y", 1, 5, 10) + (int) Long.parseLong("ff" + n % 10 + "z", 0, 3, 16);
    }
    // A thread a mod starts has run by the time it is joined; one that polls and sleeps stops at its sleep.
    public static int threads(int n) {
        int[] box = new int[2];
        Thread task = new Thread(() -> box[0] = n * 3 + 1, "worker");
        Thread poll = new Thread(() -> { try { while (true) { box[1]++; Thread.sleep(2); if (box[1] >= 1) return; } } catch (InterruptedException e) { box[1] += 0; } });
        task.start();
        poll.start();
        try { task.join(); poll.join(); } catch (InterruptedException e) { return -1; }
        String joined = new java.util.StringJoiner("-", "<", ">").add("a").add("b" + n).toString();
        return box[0] * 31 + box[1] + (task.getName().equals("worker") ? 100 : 0) + (task.isAlive() ? 1000 : 0) + joined.hashCode();
    }
    // Static helpers of the JDK the runtime has no answer of its own for: the JDK's own code stands for them.
    public static long jdkStatics(int n) {
        int[] sorted = {1, 4, 9, 16, 25, 36, 49};
        long h = java.util.Arrays.binarySearch(sorted, n) * 31L + java.util.Arrays.toString(new boolean[] {n > 3, false}).hashCode();
        return h * 31 + Long.rotateLeft(n * 7919L, n & 63) + (Character.isISOControl((char) n) ? 5 : 0);
    }
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
    // Registrate's `BuilderCallback.accept`: a default method with the lambda's own method's
    // name and another descriptor.
    interface Sink { int put(int a, int b); default int put(int a) { return put(a, 7); } }
    public static int overloads(int k) {
        Sink s = (a, b) -> a * 10 + b;
        return s.put(k) * 1000 + s.put(k, 3) + lib.Lib.callback(k) * 100000;
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

    // What the JVM throws for itself: a call on null, an array that is null, an index outside
    // an array. Code counts on them: a question asked with nulls and answered by the exception,
    // a loop through an array that ends at its end.
    static int lengthOf(String s) { return s.length(); }
    static int firstOf(int[] a) { return a[0]; }
    public static int jvmThrows(int k) {
        String[] names = { "ab", "cd", "ef" };
        int[] cells = new int[3];
        Object nothing = null;
        try {
            switch (k) {
                case 0: return nothing.hashCode();
                case 1: return lengthOf(null);
                case 2: return names[3].length();
                case 3: cells[-1] = 5; return cells[0];
                case 4: return firstOf(null);
                case 5: {
                    int i = 0;
                    try { while (!names[i].equals("zz")) i++; }
                    catch (ArrayIndexOutOfBoundsException e) { return 500 + i + e.getMessage().length(); }
                    return -5;
                }
                default: return names[k % 3].length() + cells[k % 3];
            }
        } catch (NullPointerException e) {
            return -1 - k;
        } catch (IndexOutOfBoundsException e) {
            return -100 - k - e.getMessage().length();
        } catch (Exception e) {
            return -1000;
        }
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
    // A codec's constructor reference (`RecordCodecBuilder`'s `apply(instance, Ore::new)`): the
    // interface hands its arguments boxed, the constructor takes primitives, and the metafactory
    // unboxes them, widening where the parameter's kind is wider (`Function<Integer, Long>` made
    // of a method taking a `long`).
    interface Make3<A, B, C, R> { R apply(A a, B b, C c); }
    static class Ore {
        final String name; final int size; final float chance;
        Ore(String name, int size, float chance) { this.name = name; this.size = size; this.chance = chance; }
    }
    static long widen(long x) { return x * 3_000_000_000L; }
    static double half(double x) { return x / 2; }
    public static int adapted(int k) {
        Make3<String, Integer, Float, Ore> make = Ore::new;
        Ore ore = make.apply("zinc", k + 2, 0.25f * k);
        java.util.function.Function<Integer, Long> wide = Fixture::widen;
        java.util.function.Function<Float, Double> halved = Fixture::half;
        java.util.function.BiFunction<Integer, Integer, Integer> max = Math::max;
        long w = wide.apply(k);
        double h = halved.apply(1.5f * k);
        return ore.size * 100 + (int) (ore.chance * 8) + ore.name.length() + (int) (w % 1000) + max.apply(k, 4) + (int) (h * 4);
    }
    public static int grid(int n) {
        int[][] g = new int[n][n + 1];
        for (int i = 0; i < n; i++) for (int j = 0; j <= n; j++) g[i][j] = i * j;
        int s = 0;
        for (int[] row : g) for (int v : row) s += v;
        return s + g.length * 100 + (n > 0 ? g[0].length : 0);
    }

    /** A list, a set and a map of the fixture's own, each on the JDK's abstract base: what they
     *  do not answer themselves is the base's code over what they do. One part by its number
     *  (`part` from 1), or all of them folded together for 0. */
    public static int ownCollections(int part) {
        final int n = 3;
        java.util.List<Integer> squares = new java.util.AbstractList<Integer>() {
            public Integer get(int i) { return i * i + n; }
            public int size() { return 6; }
        };
        java.util.Set<String> names = new java.util.AbstractSet<String>() {
            public java.util.Iterator<String> iterator() { return java.util.List.of("a", "bb", "ccc").iterator(); }
            public int size() { return 3; }
        };
        java.util.Map<String, Integer> lengths = new java.util.AbstractMap<String, Integer>() {
            public java.util.Set<java.util.Map.Entry<String, Integer>> entrySet() { return java.util.Set.of(java.util.Map.entry("one", 3 + n)); }
        };
        int walked = 0;
        for (int v : squares) walked = walked * 31 + v;
        int[] parts = {
            walked,
            squares.indexOf(9 + n) + squares.lastIndexOf(4 + n) * 3,
            (squares.contains(25 + n) ? 7 : 0) + (squares.isEmpty() ? 1 : 0),
            squares.subList(1, 4).size() + squares.subList(2, 5).get(1),
            squares.hashCode(),
            squares.equals(java.util.List.of(n, 1 + n, 4 + n, 9 + n, 16 + n, 25 + n)) ? 11 : 0,
            java.util.List.of(n, 1 + n, 4 + n, 9 + n, 16 + n, 25 + n).equals(squares) ? 11 : 0,
            squares.toString().length(),
            new java.util.ArrayList<Integer>(squares).size(),
            squares.toArray().length,
            names.contains("bb") ? 5 : 0,
            names.hashCode(),
            names.equals(java.util.Set.of("a", "bb", "ccc")) ? 13 : 0,
            names.toString().length(),
            lengths.get("one"),
            lengths.size() + (lengths.containsKey("one") ? 17 : 0) + (lengths.containsValue(3 + n) ? 19 : 0),
            lengths.hashCode(),
            lengths.keySet().size() + lengths.values().size(),
        };
        if (part > 0) return parts[part - 1];
        int acc = 0;
        for (int p : parts) acc = acc * 31 + p;
        return acc;
    }

    /** Maps, a list and a set of the fixture's own on the JDK's concrete classes, as a
     *  double-brace initialisation makes them: each is that map, list or set, with what its
     *  class adds or changes. One part by its number, or all of them folded together for 0. */
    public static int subCollections(int part) {
        java.util.Map<String, Integer> named = new java.util.HashMap<String, Integer>() {{
            put("one", 1);
            put("two", 2);
            putIfAbsent("two", 22);
        }};
        class Counted extends java.util.LinkedHashMap<String, Integer> {
            int puts;
            @Override public Integer put(String k, Integer v) { puts++; return super.put(k, v); }
            int total() { int t = 0; for (int v : values()) t += v; return t; }
        }
        Counted counted = new Counted();
        counted.put("b", 5);
        counted.put("a", 7);
        counted.put("b", 9);
        java.util.List<Integer> evens = new java.util.ArrayList<Integer>() {
            @Override public boolean add(Integer v) { return v % 2 == 0 && super.add(v); }
        };
        for (int i = 0; i < 9; i++) evens.add(i);
        java.util.Set<String> seen = new java.util.HashSet<String>() {{ add("x"); add("y"); add("x"); }};
        java.util.Map<String, Integer> sorted = new java.util.TreeMap<String, Integer>() {{ put("b", 2); put("a", 1); }};
        java.util.Map<String, Integer> shared = new java.util.concurrent.ConcurrentHashMap<String, Integer>() {{ put("k", 40); }};
        int walked = 0;
        for (java.util.Map.Entry<String, Integer> e : counted.entrySet()) walked = walked * 31 + e.getKey().charAt(0) + e.getValue();
        int[] parts = {
            named.size() * 100 + named.get("two"),
            (named.containsKey("one") ? 3 : 0) + (named instanceof java.util.HashMap ? 5 : 0) + (named.containsValue(22) ? 7 : 0),
            counted.puts * 1000 + counted.total() * 10 + counted.size(),
            walked,
            String.join(",", counted.keySet()).length() + counted.keySet().iterator().next().charAt(0),
            evens.size() * 10 + evens.get(2),
            seen.size() + (seen.contains("y") ? 10 : 0),
            sorted.toString().length() + sorted.keySet().iterator().next().charAt(0),
            new java.util.HashMap<>(named).size() + new java.util.ArrayList<>(evens).size(),
            shared.getOrDefault("k", 1) + shared.merge("k", 2, Integer::sum),
        };
        if (part > 0) return parts[part - 1];
        int acc = 0;
        for (int p : parts) acc = acc * 31 + p;
        return acc;
    }

    // Static methods by reflection, as AE2 runs each part class's `@PartModels` method and a mod
    // another's by its name: a public one, a private one, one the class above declares, one that
    // throws, one found by its annotation, one through a handle; and those that are not there.
    @java.lang.annotation.Retention(java.lang.annotation.RetentionPolicy.RUNTIME)
    @interface Marked { int value(); }
    static class Tools {
        static int calls;
        public static int twice(int n) { calls++; return n * 2; }
        private static long hidden(int a, String b) { return a * 1000L + b.length(); }
        public static void bump(int by) { calls += by; }
        public static String fails(int n) { throw new IllegalStateException("no " + n); }
        @Marked(7) static int marked(int n) { return n + 7; }
        public static boolean flag(boolean b, char c, long l, double d) { return !b && c == 'x' && l == 5L && d == 0.5; }
    }
    static class MoreTools extends Tools { public static int own() { return 1; } }
    interface Kept extends java.io.Serializable, java.util.function.IntUnaryOperator { }
    public static int staticReflection(int k) {
        int r = 0;
        Tools.calls = 0;
        try {
            java.lang.reflect.Method twice = Tools.class.getMethod("twice", int.class);
            r += (Integer) twice.invoke(null, k);
            r += java.lang.reflect.Modifier.isStatic(twice.getModifiers()) ? 10 : 0;
            java.lang.reflect.Method hidden = Tools.class.getDeclaredMethod("hidden", int.class, String.class);
            hidden.setAccessible(true);
            r += (int) (long) (Long) hidden.invoke(null, k, "abc") * 100;
            java.lang.reflect.Method inherited = MoreTools.class.getMethod("twice", int.class);
            r += (Integer) inherited.invoke(null, 3) * 1000 + (inherited.getDeclaringClass() == Tools.class ? 5 : 0);
            Object none = Tools.class.getMethod("bump", int.class).invoke(null, 4);
            r += (none == null ? 20 : 0) + Tools.calls * 10000;
            try {
                Tools.class.getMethod("fails", int.class).invoke(null, k);
                r += 1;
            } catch (java.lang.reflect.InvocationTargetException e) {
                r += e.getCause() instanceof IllegalStateException s && s.getMessage().equals("no " + k) ? 100000 : 0;
            }
            try { Tools.class.getMethod("absent"); r += 3; } catch (NoSuchMethodException e) { r += 200000; }
            try { Tools.class.getMethod("hidden", int.class, String.class); r += 3; } catch (NoSuchMethodException e) { r += 400000; }
            try { MoreTools.class.getDeclaredMethod("twice", int.class); r += 3; } catch (NoSuchMethodException e) { r += 800000; }
            for (java.lang.reflect.Method m : Tools.class.getDeclaredMethods()) {
                Marked marked = m.getAnnotation(Marked.class);
                if (marked != null) r += ((Integer) m.invoke(null, marked.value())) * 1000000;
            }
            r += (Boolean) Tools.class.getMethod("flag", boolean.class, char.class, long.class, double.class).invoke(null, false, 'x', 5L, 0.5) ? 40 : 0;
            java.lang.invoke.MethodHandle handle = java.lang.invoke.MethodHandles.lookup().findStatic(Tools.class, "twice", java.lang.invoke.MethodType.methodType(int.class, int.class));
            r += (int) handle.invokeExact(k) * 3 + (Integer) handle.invokeWithArguments(5) * 7;
            // What a serializable lambda was made of (MineColonies names its transitions so).
            Kept named = Fixture::sumTo;
            java.lang.reflect.Method replace = named.getClass().getDeclaredMethod("writeReplace");
            replace.setAccessible(true);
            java.lang.invoke.SerializedLambda made = (java.lang.invoke.SerializedLambda) replace.invoke(named);
            r += made.getImplMethodName().equals("sumTo") && made.getImplClass().equals("Fixture") ? 80 : 0;
        } catch (Throwable e) {
            return -1 - r;
        }
        return r;
    }

    // Atomics, their arrays, field updaters and `VarHandle`s (Flywheel's bit set of changed
    // instances is one over a `long[]`, Kotlin's coroutines keep their states with updaters):
    // one part by its number, or all of them folded together for 0.
    static class Counted {
        volatile int hits;
        volatile long total;
        volatile String label = "a";
        int plain = 3;
        boolean on;
        double ratio = 1.5;
        static int shared = 5;
        static Object held;
    }
    public static long atomics(int part) {
        java.util.concurrent.atomic.AtomicInteger i = new java.util.concurrent.atomic.AtomicInteger(5);
        java.util.concurrent.atomic.AtomicLong l = new java.util.concurrent.atomic.AtomicLong();
        java.util.concurrent.atomic.AtomicReference<String> r = new java.util.concurrent.atomic.AtomicReference<>("x");
        java.util.concurrent.atomic.AtomicBoolean b = new java.util.concurrent.atomic.AtomicBoolean();
        java.util.concurrent.atomic.AtomicIntegerArray ia = new java.util.concurrent.atomic.AtomicIntegerArray(4);
        java.util.concurrent.atomic.AtomicLongArray la = new java.util.concurrent.atomic.AtomicLongArray(new long[] { 7, 8, 9 });
        java.util.concurrent.atomic.AtomicReferenceArray<String> ra = new java.util.concurrent.atomic.AtomicReferenceArray<>(3);
        Counted c = new Counted();
        Counted.shared = 5;
        Counted.held = null;
        long[] parts = new long[18];
        try {
            parts[0] = i.updateAndGet(v -> v * 3) + i.getAndUpdate(v -> v + 1) * 100L + i.accumulateAndGet(4, (x, y) -> x * y) * 10000L + i.getAndAccumulate(2, Integer::sum) * 1000000L;
            parts[1] = i.get() + i.longValue() * 2 + (long) i.floatValue() + (i.compareAndSet(66, 7) ? 1000 : 0) + (i.compareAndSet(66, 9) ? 5000 : 0)
                + i.incrementAndGet() * 10000L + i.getAndAdd(-3) * 1000000L + i.intValue() * 100000000L + i.toString().length();
            parts[2] = l.updateAndGet(v -> v - 9) + l.addAndGet(1L << 40) + l.getAndIncrement() + l.decrementAndGet() + l.accumulateAndGet(3, Math::max)
                + l.intValue() + l.toString().length() + (l.compareAndSet(3, 1) ? 0 : 7) + (long) l.doubleValue();
            parts[3] = (r.compareAndSet("x", "y") ? 1 : 0) + (r.compareAndSet("x", "z") ? 10 : 0) + r.getAndSet("w").charAt(0) * 100 + r.updateAndGet(s -> s + "!").length() * 100000
                + r.accumulateAndGet("?", String::concat).length() * 1000000 + (r.toString().equals("w!?") ? 50000000 : 0) + r.getAndUpdate(s -> null).length()
                + (r.get() == null ? 3 : 0) + (r.compareAndExchange(null, "q") == null ? 400000000L : 0);
            parts[4] = (b.compareAndSet(false, true) ? 1 : 0) + (b.getAndSet(false) ? 2 : 0) + (b.get() ? 4 : 0) + (b.toString().equals("false") ? 8 : 0) + (b.weakCompareAndSetPlain(false, true) ? 16 : 0);
            ia.set(1, 5);
            parts[5] = ia.length() + ia.incrementAndGet(1) * 10 + ia.getAndAdd(2, 7) * 100 + ia.addAndGet(2, 1) * 1000 + ia.updateAndGet(3, v -> v - 4) * 10000L
                + ia.accumulateAndGet(0, 9, (x, y) -> x + y * 2) * 100000L + (ia.compareAndSet(1, 6, 60) ? 1000000 : 0) + ia.get(1) * 10000000L + ia.toString().length();
            parts[6] = la.get(2) + la.getAndSet(0, 70) * 10 + la.decrementAndGet(1) * 100 + la.length() * 1000 + la.updateAndGet(0, v -> v / 7) * 10000 + la.toString().length() * 100000;
            ra.set(0, "a");
            parts[7] = (ra.compareAndSet(1, null, "b") ? 1 : 0) + (ra.compareAndSet(1, null, "c") ? 10 : 0) + ra.getAndSet(0, "z").charAt(0) + ra.length() * 1000
                + (ra.get(2) == null ? 10000 : 0) + ra.updateAndGet(1, s -> s + s).length() * 100000 + ra.toString().length() * 1000000;
            java.util.concurrent.atomic.AtomicIntegerFieldUpdater<Counted> hits = java.util.concurrent.atomic.AtomicIntegerFieldUpdater.newUpdater(Counted.class, "hits");
            java.util.concurrent.atomic.AtomicLongFieldUpdater<Counted> total = java.util.concurrent.atomic.AtomicLongFieldUpdater.newUpdater(Counted.class, "total");
            java.util.concurrent.atomic.AtomicReferenceFieldUpdater<Counted, String> label =
                java.util.concurrent.atomic.AtomicReferenceFieldUpdater.newUpdater(Counted.class, String.class, "label");
            parts[8] = hits.incrementAndGet(c) + hits.getAndAdd(c, 5) * 10 + (hits.compareAndSet(c, 6, 40) ? 100 : 0) + c.hits * 1000 + hits.updateAndGet(c, v -> v + 2) * 100000L + hits.get(c) * 10000000L;
            parts[9] = total.addAndGet(c, 1L << 33) + total.getAndIncrement(c) + c.total + (total.compareAndSet(c, (1L << 33) + 1, 5) ? 3 : 0) + total.accumulateAndGet(c, 4, (x, y) -> x * y);
            parts[10] = (label.compareAndSet(c, "a", "b") ? 1 : 0) + label.getAndSet(c, "cc").length() * 10 + c.label.length() * 100 + label.updateAndGet(c, s -> s + "d").length() * 1000 + label.get(c).charAt(2);
            java.lang.invoke.MethodHandles.Lookup lookup = java.lang.invoke.MethodHandles.lookup();
            java.lang.invoke.VarHandle plain = lookup.findVarHandle(Counted.class, "plain", int.class);
            java.lang.invoke.VarHandle shared = lookup.findStaticVarHandle(Counted.class, "shared", int.class);
            java.lang.invoke.VarHandle held = lookup.findStaticVarHandle(Counted.class, "held", Object.class);
            java.lang.invoke.VarHandle on = lookup.findVarHandle(Counted.class, "on", boolean.class);
            java.lang.invoke.VarHandle ratio = lookup.findVarHandle(Counted.class, "ratio", double.class);
            parts[11] = (int) plain.get(c) + (int) plain.getAndAdd(c, 4) * 10 + (plain.compareAndSet(c, 7, 20) ? 100 : 0) + c.plain * 1000 + (int) plain.getVolatile(c) * 100000
                + (int) plain.getAndSet(c, 1) * 10000000L;
            shared.set(11);
            parts[12] = (int) shared.get() + Counted.shared * 100 + (int) shared.getAndBitwiseOr(4) * 10000 + Counted.shared * 1000000L + (int) shared.compareAndExchange(15, 2) * 100000000L;
            Object first = "first";
            parts[13] = (held.compareAndSet(null, first) ? 1 : 0) + (held.get() == first ? 10 : 0) + (Counted.held == first ? 100 : 0) + (held.getAndSet(c) == first ? 1000 : 0) + (held.get() == c ? 10000 : 0);
            parts[14] = ((boolean) on.get(c) ? 1 : 0) + (on.compareAndSet(c, false, true) ? 10 : 0) + (c.on ? 100 : 0) + (long) ((double) ratio.getAndAdd(c, 2.25) * 1000) + (long) (c.ratio * 1000000);
            java.lang.invoke.VarHandle longs = java.lang.invoke.MethodHandles.arrayElementVarHandle(long[].class);
            java.lang.invoke.VarHandle texts = java.lang.invoke.MethodHandles.arrayElementVarHandle(String[].class);
            long[] words = new long[3];
            String[] names = { "p", null };
            parts[15] = (long) longs.getAndBitwiseOr(words, 1, 1L << 40) + ((long) longs.getVolatile(words, 1) >> 40) + (longs.compareAndSet(words, 0, 0L, 9L) ? 10 : 0) + words[0] * 100
                + (long) longs.getAndAdd(words, 2, 5L) + words[2] * 1000 + (long) longs.getAndBitwiseAnd(words, 0, 8L) * 100000 + words[0] * 10000000;
            parts[16] = (texts.compareAndSet(names, 1, null, "q") ? 1 : 0) + ((String) texts.get(names, 1)).charAt(0) * 10 + ((String) texts.getAndSet(names, 0, "r")).charAt(0) * 10000 + names[0].charAt(0) * 10000000L;
            java.lang.invoke.VarHandle ints = java.lang.invoke.MethodHandles.byteArrayViewVarHandle(int[].class, java.nio.ByteOrder.LITTLE_ENDIAN);
            java.lang.invoke.VarHandle bigLongs = java.lang.invoke.MethodHandles.byteArrayViewVarHandle(long[].class, java.nio.ByteOrder.BIG_ENDIAN);
            java.lang.invoke.VarHandle shorts = java.lang.invoke.MethodHandles.byteArrayViewVarHandle(short[].class, java.nio.ByteOrder.BIG_ENDIAN);
            java.lang.invoke.VarHandle doubles = java.lang.invoke.MethodHandles.byteArrayViewVarHandle(double[].class, java.nio.ByteOrder.LITTLE_ENDIAN);
            byte[] bytes = new byte[24];
            ints.set(bytes, 1, 0x01020384);
            bigLongs.set(bytes, 8, 0x1122334455667788L);
            shorts.set(bytes, 5, (short) -2);
            doubles.set(bytes, 16, 2.5);
            parts[17] = (int) ints.get(bytes, 1) + bytes[1] * 3 + bytes[4] * 5 + (long) bigLongs.get(bytes, 8) % 1000003 + bytes[8] * 7 + bytes[15] * 11
                + (short) shorts.get(bytes, 5) * 13 + bytes[6] * 17 + (long) ((double) doubles.get(bytes, 16) * 100) + (int) ints.get(bytes, 12) % 1009;
        } catch (Throwable e) {
            return -1;
        }
        if (part > 0) return parts[part - 1];
        long acc = 0;
        for (long p : parts) acc = acc * 31 + p;
        return acc;
    }

    // More of reflection and of threads: a method's parameters and its text, the one module,
    // a thread from a builder.
    public static int reflectionMore(int k) {
        try {
            java.lang.reflect.Method twice = Tools.class.getMethod("twice", int.class);
            java.lang.reflect.Parameter[] given = twice.getParameters();
            int r = given.length + given[0].getType().getName().length() * 10 + twice.toString().length() * 100 + (given[0].getName().equals("arg0") ? 100000 : 0);
            r += (Tools.class.getModule().isNamed() ? 0 : 1000000) + twice.getExceptionTypes().length + (twice.equals(Tools.class.getMethod("twice", int.class)) ? 10000000 : 0);
            r += twice.toString().equals("public static int Fixture$Tools.twice(int)") ? 20000000 : 0;
            Thread made = Thread.ofVirtual().name("worker").unstarted(() -> { });
            r += made.getName().length() * 1000 + (Thread.getDefaultUncaughtExceptionHandler() == null ? 40000000 : 0);
            java.util.concurrent.locks.StampedLock lock = new java.util.concurrent.locks.StampedLock();
            long stamp = lock.tryOptimisticRead();
            r += (lock.validate(stamp) ? 100000000 : 0) + (lock.isWriteLocked() ? 1 : 0);
            long write = lock.writeLock();
            lock.unlockWrite(write);
            // The thread's stack begins with the question, then the class that asked (Kotlin's
            // null checks look for their own class there).
            StackTraceElement[] trace = Thread.currentThread().getStackTrace();
            r += trace[0].getMethodName().equals("getStackTrace") && trace[1].getClassName().equals("Fixture") ? 200000000 : 0;
            return r + k;
        } catch (Throwable e) {
            return -1;
        }
    }

    // Blocking queues, on one thread: a full one takes no more, a wait for what is there is none.
    public static int blockingQueues(int k) {
        java.util.concurrent.ArrayBlockingQueue<Integer> q = new java.util.concurrent.ArrayBlockingQueue<>(3);
        int r = 0;
        try {
            r += (q.offer(1) ? 1 : 0) + (q.offer(2) ? 2 : 0) + q.remainingCapacity() * 10;
            q.put(3);
            r += (q.offer(4) ? 100 : 200) + q.remainingCapacity() * 1000;
            try { q.add(5); r += 3; } catch (IllegalStateException e) { r += 4000; }
            r += q.take() * 10000 + q.poll() * 100000;
            java.util.List<Integer> out = new java.util.ArrayList<>();
            q.offer(k);
            r += q.drainTo(out) * 1000000 + out.get(1) * 3 + q.size();
            java.util.concurrent.BlockingQueue<String> names = new java.util.concurrent.LinkedBlockingQueue<>();
            names.put("a");
            names.offer("b", 1, java.util.concurrent.TimeUnit.MILLISECONDS);
            r += names.remainingCapacity() == Integer.MAX_VALUE ? 7 : 0;
            r += names.poll(1, java.util.concurrent.TimeUnit.MILLISECONDS).charAt(0) + names.take().charAt(0);
            r += names.poll(1, java.util.concurrent.TimeUnit.MILLISECONDS) == null ? 50 : 0;
            java.util.concurrent.TimeUnit.MILLISECONDS.sleep(1);
        } catch (InterruptedException e) {
            return -1;
        }
        return r;
    }

    // What `Arrays` says of arrays: their text, where a key is, where two differ, which comes first.
    public static int arraysMore(int k) {
        int[] ints = {1, 3, 5, 7, 9};
        long[] longs = {-2L, 4L, 1L << 40};
        double[] doubles = {-0.5, 0.0, 2.25, 1e10};
        float[] floats = {1.5f, -3f};
        char[] chars = {'a', 'z'};
        boolean[] flags = {true, false};
        byte[] bytes = {-1, 2};
        String[] words = {"apple", "kiwi", null, "pear"};
        String[] sorted = {"apple", "kiwi", "pear"};
        switch (k) {
            case 0:
                return (java.util.Arrays.toString(ints) + java.util.Arrays.toString(longs) + java.util.Arrays.toString(bytes)
                    + java.util.Arrays.toString(words) + java.util.Arrays.toString((int[]) null)).hashCode();
            case 1:
                return (java.util.Arrays.toString(doubles) + java.util.Arrays.toString(floats)).hashCode();
            case 2:
                return (java.util.Arrays.toString(chars) + java.util.Arrays.toString(flags)).hashCode();
            case 3: {
                Object[] nested = {ints, words, new Object[] {1, "two", new int[] {3}}, null};
                return java.util.Arrays.deepToString(nested).hashCode();
            }
            case 4:
                return java.util.Arrays.binarySearch(ints, 5) * 7 + java.util.Arrays.binarySearch(ints, 6) * 11
                    + java.util.Arrays.binarySearch(ints, 1, 3, 9) * 100 + java.util.Arrays.binarySearch(doubles, 2.25) * 13
                    + java.util.Arrays.binarySearch(longs, 5L) * 1000;
            case 5:
                return java.util.Arrays.binarySearch(sorted, "kiwi") + java.util.Arrays.binarySearch(sorted, "banana") * 3
                    + java.util.Arrays.binarySearch(sorted, "PEAR", String.CASE_INSENSITIVE_ORDER) * 50;
            case 6:
                return java.util.Arrays.mismatch(ints, new int[] {1, 3, 6}) + java.util.Arrays.mismatch(bytes, bytes) * 3
                    + java.util.Arrays.mismatch(ints, new int[] {1, 3}) * 70;
            case 7:
                return java.util.Arrays.compare(ints, new int[] {1, 3, 6}) + java.util.Arrays.compare(new int[] {1}, new int[] {1, 0}) * 3
                    + java.util.Arrays.compare(sorted, new String[] {"apple", "lime"}) * 50
                    + java.util.Arrays.compareUnsigned(bytes, new byte[] {1, 2}) * 9000;
            default: {
                Object[] same = {new int[] {1, 2}, new String[] {"x"}, 4};
                Object[] other = {new int[] {1, 2}, new String[] {"x"}, 4};
                return (java.util.Arrays.deepEquals(same, other) ? 1 : 0) + (java.util.Arrays.equals(same, other) ? 2 : 0)
                    + (java.util.Arrays.deepHashCode(same) == java.util.Arrays.deepHashCode(other) ? 4 : 0)
                    + java.util.Arrays.deepHashCode(new Object[] {1, "a", new long[] {5L}}) * 8;
            }
        }
    }

    // A pool's work: its value where it is waited for, what it threw as `join`, `get` and the
    // stages are handed it, and that it runs beside the code that gave it, never inside the
    // call that did.
    public static int pools(int k) {
        java.util.concurrent.ExecutorService pool = java.util.concurrent.Executors.newSingleThreadExecutor();
        java.util.concurrent.atomic.AtomicInteger ran = new java.util.concurrent.atomic.AtomicInteger();
        try {
            switch (k) {
                case 0: {
                    java.util.concurrent.Future<Integer> f = pool.submit(() -> 6 * 7);
                    return f.get() + (f.isDone() ? 100 : 0);
                }
                case 1:
                    return java.util.concurrent.CompletableFuture.supplyAsync(() -> "a", pool)
                        .thenApply(s -> s + "b")
                        .thenCombine(java.util.concurrent.CompletableFuture.supplyAsync(() -> "c"), (x, y) -> x + y)
                        .join().hashCode();
                case 2: {
                    String told = java.util.concurrent.CompletableFuture.<String>supplyAsync(() -> { throw new IllegalStateException("broke"); }, pool)
                        .handle((v, e) -> e.getClass().getSimpleName() + ":" + e.getCause().getMessage() + ":" + v).join();
                    return told.hashCode();
                }
                case 3: {
                    java.util.concurrent.CompletableFuture<String> failed =
                        java.util.concurrent.CompletableFuture.supplyAsync(() -> { throw new IllegalStateException("broke"); }, pool);
                    int seen = 0;
                    try { failed.join(); } catch (java.util.concurrent.CompletionException e) { seen += e.getCause().getMessage().length(); }
                    try { failed.get(); } catch (java.util.concurrent.ExecutionException e) { seen += 10 * e.getCause().getMessage().length(); }
                    seen += failed.isCompletedExceptionally() ? 1000 : 0;
                    seen += failed.thenApply(s -> s + "!").isCompletedExceptionally() ? 2000 : 0;
                    seen += failed.exceptionally(e -> "saved").join().length() * 10000;
                    return seen;
                }
                case 4: {
                    // The work waits for a gate its giver opens only after giving it.
                    java.util.concurrent.CountDownLatch gate = new java.util.concurrent.CountDownLatch(1);
                    java.util.concurrent.CountDownLatch done = new java.util.concurrent.CountDownLatch(1);
                    int[] order = {0};
                    pool.execute(() -> {
                        try { gate.await(); } catch (InterruptedException e) { return; }
                        order[0] = order[0] * 10 + 2;
                        done.countDown();
                    });
                    order[0] = order[0] * 10 + 1;
                    gate.countDown();
                    done.await();
                    return order[0];
                }
                case 5: {
                    pool.submit(() -> ran.incrementAndGet());
                    pool.execute(() -> ran.addAndGet(10));
                    pool.shutdown();
                    boolean ended = pool.awaitTermination(5, java.util.concurrent.TimeUnit.SECONDS);
                    return ran.get() + (ended ? 100 : 0) + (pool.isShutdown() ? 1000 : 0);
                }
                default: {
                    // Work cancelled while the pool's one thread is busy with other work never runs.
                    java.util.concurrent.CountDownLatch gate = new java.util.concurrent.CountDownLatch(1);
                    pool.execute(() -> {
                        try { gate.await(); } catch (InterruptedException e) { return; }
                        ran.incrementAndGet();
                    });
                    java.util.concurrent.Future<?> second = pool.submit(() -> { ran.addAndGet(10); });
                    boolean cancelled = second.cancel(false);
                    gate.countDown();
                    pool.shutdown();
                    pool.awaitTermination(5, java.util.concurrent.TimeUnit.SECONDS);
                    return ran.get() + (cancelled ? 100 : 0) + (second.isCancelled() ? 1000 : 0);
                }
            }
        } catch (InterruptedException | java.util.concurrent.ExecutionException e) {
            return -1;
        } finally {
            pool.shutdownNow();
        }
    }

    // A string as a class file keeps it: a NUL inside, a private-use character, a character
    // beyond the BMP, and a number packed into characters as protobuf's schemas pack theirs.
    public static int packedStrings(int k) {
        String info = "\u0000\u0004\u0000\u0001\u0409\ue123\u0005x\uD83D\uDE00y";
        switch (k) {
            case 0:
                return info.length() * 1000 + info.codePointCount(0, info.length());
            case 1:
                return info.charAt(0) + info.charAt(1) * 10 + info.charAt(4) * 100 + info.charAt(8) + info.charAt(9);
            case 2: {
                int i = 5;
                int next = info.charAt(i++);
                if (next >= 0xD800) {
                    int result = next & 0x1FFF;
                    int shift = 13;
                    while ((next = info.charAt(i++)) >= 0xD800) {
                        result |= (next & 0x1FFF) << shift;
                        shift += 13;
                    }
                    next = result | (next << shift);
                }
                return next * 10 + i;
            }
            case 3:
                return info.codePointAt(8) + info.indexOf('y') * 7 + info.indexOf("\u0000\u0001") * 100000;
            case 4:
                return info.hashCode();
            default:
                return info.substring(1, 3).length() + (info.contains("\u0000\u0004") ? 10 : 0)
                    + info.getBytes(java.nio.charset.StandardCharsets.UTF_8).length * 100
                    + new String(info.toCharArray(), 7, 4).length() * 10000;
        }
    }
}
