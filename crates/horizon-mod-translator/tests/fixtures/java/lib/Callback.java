package lib;

/** Registrate's `BuilderCallback`: a function type with a default overload of its own method. */
@FunctionalInterface
public interface Callback {
    int accept(int a, int b);

    default int accept(int a) {
        return accept(a, 100);
    }
}
