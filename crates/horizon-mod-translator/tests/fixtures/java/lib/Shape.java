package lib;

public interface Shape {
    int side();

    default int area() {
        return side() * side();
    }

    static Shape of(int x) {
        return new Square(x < 0 ? -x : x);
    }
}
