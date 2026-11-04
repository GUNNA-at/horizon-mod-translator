package lib;

/** Made by the library and handed out as a JDK interface, which code that isn't translated may call. */
final class Counter implements java.util.function.IntSupplier {
    private int n;

    Counter(int n) {
        this.n = n;
    }

    @Override
    public int getAsInt() {
        return n++;
    }

    public int peek() {
        return n;
    }
}
