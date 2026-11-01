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
}
