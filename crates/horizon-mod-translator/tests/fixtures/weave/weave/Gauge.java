package weave;

/** A class whose locals a mixin's handlers read and change. */
public class Gauge {
    public static int LIMIT = 100;
    public int last;
    public int seen;
    public String note;
    public int level = 4;

    public int reading(int a, int b) {
        int sum = a + b;
        int scaled = sum * 10;
        String tag = "g" + scaled;
        record(tag);
        return scaled + 1;
    }

    public void record(String tag) {
        last = tag.length();
    }

    // Three returns; `doubled` is a local at the last only.
    public int pick(int n) {
        if (n < 0) {
            return -1;
        }
        if (n == 0) {
            return 0;
        }
        int doubled = n * 2;
        return doubled;
    }

    // A call's result stored in a local, then used.
    public int stored(int n) {
        int v = twice(n);
        return v + 1;
    }

    static int twice(int n) {
        return n * 2;
    }

    public int shared(int n) {
        record("s");
        return n;
    }

    public String label(int n) {
        String text = "n" + n;
        record(text);
        return text;
    }

    public int doubledLevel() {
        return level * 2;
    }

    public void setLevel(int v) {
        level = v;
    }

    public int limit() {
        return LIMIT;
    }

    public int bump() {
        level = level + 1;
        return level;
    }

    public int offset(int n) {
        return n * 2;
    }

    public int accumulate(int n) {
        int total = n * 3;
        total = total + 1;
        return total;
    }

    public int spread(int n) {
        int width = n + 1;
        record("w");
        return width;
    }

    public int nameLength(String s) {
        return s.length();
    }
}
