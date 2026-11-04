package lib;

final class Square implements Shape {
    private final int s;

    Square(int s) {
        this.s = s;
    }

    @Override
    public int side() {
        return s;
    }

    @Override
    public String toString() {
        return "Square" + s;
    }
}
