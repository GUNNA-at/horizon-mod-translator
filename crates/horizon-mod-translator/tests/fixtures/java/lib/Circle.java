package lib;

/** Only tested for (`instanceof`), never made: none of its methods can run. */
final class Circle implements Shape {
    @Override
    public int side() {
        return 1;
    }

    @Override
    public int area() {
        return 3;
    }
}
