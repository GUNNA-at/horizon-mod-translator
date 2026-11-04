package lib;

import java.util.function.IntSupplier;
import java.util.function.IntUnaryOperator;

/** A library the fixture mod bundles (jar-in-jar): translated only as far as the mod reaches. */
public final class Lib {
    private Lib() {}

    /** A shape made by an interface's factory, its area from a default method. */
    public static int twice(int x) {
        Shape s = Shape.of(x);
        if (s instanceof Circle) {
            return -1;
        }
        return s.area() * 2;
    }

    public static Object shape(int x) {
        return Shape.of(x);
    }

    /** A library object behind a JDK interface. */
    public static IntSupplier counter(int start) {
        return new Counter(start);
    }

    /** A lambda inside the library. */
    public static int plusOne(int x) {
        IntUnaryOperator op = y -> y + 1;
        return op.applyAsInt(x);
    }

    /** A lambda the library makes, called through its interface's default overload. */
    public static int callback(int x) {
        Callback c = (a, b) -> a * 3 + b;
        return c.accept(x);
    }

    /** Never called: left out, with the class only it uses. */
    public static int unused(int x) {
        return new Unused().value(x);
    }
}
