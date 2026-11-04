package weave;

import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;

/** An addon's mark for a mixin its config plugin applies by the mods there are. */
@Retention(RetentionPolicy.RUNTIME)
public @interface ModMixin {
    Mods[] mods();

    boolean applyIfPresent() default true;
}
