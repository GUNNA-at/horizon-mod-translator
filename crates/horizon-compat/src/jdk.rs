//! What the JDK's own code asks of the JDK's insides. The translator takes the plain classes of
//! the player's Java runtime as a library (`JDK_CODE` there: `java.awt.Color`, `BigDecimal`,
//! `URI`), and their code reaches under itself for things only a JVM has: the window toolkit's
//! native library, the hooks the JDK's packages hand each other, a class's serial form. Each
//! is answered here as a JVM without a display would.
use crate::{Rt, V};

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key
/// only to the modules that claim it ([`crate::api`]'s chain).
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    matches!(owner, "java/awt/GraphicsEnvironment" | "java/awt/Toolkit" | "java/io/ObjectStreamField" | "jdk/internal/access/SharedSecrets")
}

pub fn call(_rt: &mut Rt, key: &'static str, _args: &[V]) -> Option<V> {
    let (owner, name, _) = crate::parts(key)?;
    Some(match (owner, name) {
        // No display: the classes of `java.awt` that are only values load no native library.
        ("java/awt/GraphicsEnvironment", "isHeadless") => V::I(1),
        ("java/awt/Toolkit", "loadLibraries") => V::Void,
        // A field of a class's serial form, listed by its static initialiser: nothing is
        // serialised here.
        ("java/io/ObjectStreamField", "<init>") => V::Void,
        // A package handing another its private access (`setJavaNetUriAccess`): nobody asks.
        ("jdk/internal/access/SharedSecrets", n) if n.starts_with("set") => V::Void,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use crate::{Rt, V};

    #[test]
    fn the_jdks_own_code_finds_no_display_and_keeps_its_hooks() {
        let mut rt = Rt::new();
        let c = crate::api::call;
        assert_eq!(c(&mut rt, "java/awt/GraphicsEnvironment.isHeadless.()Z", &[]), V::I(1));
        c(&mut rt, "java/awt/Toolkit.loadLibraries.()V", &[]);
        let field = rt.new_object("java/io/ObjectStreamField");
        c(&mut rt, "java/io/ObjectStreamField.<init>.(Ljava/lang/String;Ljava/lang/Class;)V", &[field, V::NULL, V::NULL]);
        c(&mut rt, "jdk/internal/access/SharedSecrets.setJavaNetUriAccess.(Ljdk/internal/access/JavaNetUriAccess;)V", &[V::NULL]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        // The bounds checks its code makes: the index back, or the exception.
        assert_eq!(c(&mut rt, "java/util/Objects.checkIndex.(II)I", &[V::I(3), V::I(4)]), V::I(3));
        assert_eq!(c(&mut rt, "java/util/Objects.checkFromToIndex.(III)I", &[V::I(1), V::I(4), V::I(4)]), V::I(1));
        assert_eq!(c(&mut rt, "java/util/Objects.checkFromIndexSize.(III)I", &[V::I(2), V::I(2), V::I(4)]), V::I(2));
        assert_eq!(c(&mut rt, "java/util/Objects.checkFromIndexSize.(JJJ)J", &[V::J(2), V::J(2), V::J(4)]), V::J(2));
        assert!(rt.thrown.is_none());
        for (key, args) in [
            ("java/util/Objects.checkIndex.(II)I", [V::I(4), V::I(4), V::I(0)]),
            ("java/util/Objects.checkIndex.(II)I", [V::I(-1), V::I(4), V::I(0)]),
            ("java/util/Objects.checkFromToIndex.(III)I", [V::I(3), V::I(2), V::I(4)]),
            ("java/util/Objects.checkFromIndexSize.(III)I", [V::I(3), V::I(2), V::I(4)]),
        ] {
            let n = if key.contains("(II)") { 2 } else { 3 };
            c(&mut rt, key, &args[..n]);
            let thrown = rt.thrown.take().expect(key);
            assert_eq!(rt.class_of(thrown), "java/lang/IndexOutOfBoundsException", "{key}");
        }
        assert_eq!(c(&mut rt, "java/lang/Integer.lowestOneBit.(I)I", &[V::I(0b1011000)]), V::I(0b1000));
        assert_eq!(c(&mut rt, "java/lang/Long.lowestOneBit.(J)J", &[V::J(3 << 40)]), V::J(1 << 40));
        assert_eq!(c(&mut rt, "java/lang/Long.highestOneBit.(J)J", &[V::J(3 << 40)]), V::J(1 << 41));
        assert_eq!(c(&mut rt, "java/lang/Integer.highestOneBit.(I)I", &[V::I(-5)]), V::I(i32::MIN));
        let n = rt.new_object("java/math/BigInteger");
        c(&mut rt, "java/lang/Number.<init>.()V", &[n]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        // The charsets the JDK's code names by their own classes are the standard ones.
        let utf8 = crate::api::get_static(&mut rt, "java/nio/charset/StandardCharsets.UTF_8");
        assert_eq!(crate::api::get_static(&mut rt, "sun/nio/cs/UTF_8.INSTANCE"), utf8);
        assert_ne!(crate::api::get_static(&mut rt, "sun/nio/cs/ISO_8859_1.INSTANCE"), utf8);
    }
}
