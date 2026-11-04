package weave;

/** What the mixin adds to its target. */
public interface Boosted {
    int boost();

    int afterAdds();

    int seen();
}
