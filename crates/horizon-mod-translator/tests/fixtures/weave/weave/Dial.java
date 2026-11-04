package weave;

import java.util.ArrayList;
import java.util.List;

/** A class whose constants, `new`s and `instanceof`s a mixin's handlers go at, some in slices. */
public class Dial {
    public int seen;

    // `step` twice, `mark()` between.
    public int sliced(int n) {
        int a = step(n);
        mark();
        int b = step(n);
        return a * 100 + b;
    }

    static int step(int n) {
        return n + 1;
    }

    static void mark() {
    }

    public int sevens(int n) {
        return n * 7;
    }

    public double half(double d) {
        return d * 2.5;
    }

    public String name() {
        return "dial";
    }

    // The same constant before and after `mark()`.
    public int tens() {
        int a = 10;
        mark();
        int b = 10;
        return a + b;
    }

    public int made(int n) {
        List<Integer> list = new ArrayList<>();
        list.add(n);
        return list.size() + seen;
    }

    public int counted(int n) {
        int count = n;
        StringBuilder sb = new StringBuilder();
        sb.append(count);
        return count;
    }

    public boolean isText(Object o) {
        return o instanceof String;
    }

    public int first(int a, int b) {
        return step(a) + b;
    }
}
