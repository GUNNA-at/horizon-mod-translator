//! What a mod's mixins get when the translator weaves them into the translated classes they
//! target (`horizon-mod-translator`'s codegen, `Weave`): Mixin's callback objects. An `@Inject`
//! handler receives a `CallbackInfo` (a `CallbackInfoReturnable` for a method returning a value),
//! and the woven code asks it, after the handler ran, whether to return early and with what.
//!
//! The generated code makes them as the Java would (`new CallbackInfoReturnable(id, cancellable,
//! value)`, the primitive constructors for a primitive return) and reads them back through their
//! own methods (`isCancelled`, `getReturnValueZ`...), so it calls nothing but the API.
use crate::{Rt, V};

pub const CALLBACK_INFO: &str = "org/spongepowered/asm/mixin/injection/callback/CallbackInfo";
pub const RETURNABLE: &str = "org/spongepowered/asm/mixin/injection/callback/CallbackInfoReturnable";
const CANCELLATION: &str = "org/spongepowered/asm/mixin/injection/callback/CancellationException";

/// Whether this module answers `owner.name desc`.
pub fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    owner == CALLBACK_INFO || owner == RETURNABLE
}

/// The box Java's autoboxing makes for a value of descriptor kind `kind` (`Z`, `I`, `F`...); an
/// object (`L`, `[`) is itself.
pub fn box_value(rt: &mut Rt, v: V, kind: char) -> V {
    let class = match kind {
        'Z' => "java/lang/Boolean",
        'B' => "java/lang/Byte",
        'C' => "java/lang/Character",
        'S' => "java/lang/Short",
        'I' => "java/lang/Integer",
        'J' => "java/lang/Long",
        'F' => "java/lang/Float",
        'D' => "java/lang/Double",
        _ => return v,
    };
    let o = rt.new_object(class);
    rt.put_field(o, "value", v);
    o
}

/// The primitive in a box (or a value held raw, as a method reference's result may be), as kind
/// `kind`; an object kind gives the value itself.
pub fn unbox(rt: &Rt, v: V, kind: char) -> V {
    let raw = match v {
        V::R(r) if r != 0 && !matches!(kind, 'L' | '[') => rt.get_field(v, "value"),
        V::R(0) | V::Void if !matches!(kind, 'L' | '[') => V::I(0),
        other => other,
    };
    match (kind, raw) {
        ('J', V::I(i)) => V::J(i as i64),
        ('F', V::I(i)) => V::F(i as f32),
        ('D', V::I(i)) => V::D(i as f64),
        ('D', V::F(f)) => V::D(f as f64),
        ('Z' | 'B' | 'C' | 'S' | 'I', V::J(j)) => V::I(j as i32),
        ('J', V::Void) => V::J(0),
        ('F', V::Void) => V::F(0.0),
        ('D', V::Void) => V::D(0.0),
        ('Z' | 'B' | 'C' | 'S' | 'I', V::Void) => V::I(0),
        (_, v) => v,
    }
}

/// `CallbackInfo` and `CallbackInfoReturnable` calls; None for anything else.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    Some(match name {
        // `(String id, boolean cancellable)`, and for a returnable `(id, cancellable, value)`
        // with the value boxed or primitive (`(Ljava/lang/String;ZI)V`...).
        "<init>" => {
            rt.put_field(this, "$id", a(1));
            rt.put_field(this, "$cancellable", V::I((a(2).i() != 0) as i32));
            if owner == RETURNABLE {
                let kind = desc.trim_start_matches('(').trim_end_matches(")V");
                let kind = kind.strip_prefix("Ljava/lang/String;Z").unwrap_or("");
                let value = match kind.chars().next() {
                    Some(k) if k != 'L' && k != '[' => box_value(rt, a(3), k),
                    Some(_) => a(3),
                    None => V::NULL,
                };
                rt.put_field(this, "$value", value);
            }
            V::Void
        }
        "getId" => rt.get_field(this, "$id"),
        "isCancellable" => V::I(rt.get_field(this, "$cancellable").i()),
        "isCancelled" => V::I(rt.get_field(this, "$cancelled").i()),
        "cancel" => {
            if rt.get_field(this, "$cancellable").i() == 0 {
                rt.throw_new(CANCELLATION);
                return Some(V::Void);
            }
            rt.put_field(this, "$cancelled", V::I(1));
            V::Void
        }
        // Setting the value returns it: the method ends with it (Mixin cancels the callback).
        "setReturnValue" => {
            if rt.get_field(this, "$cancellable").i() == 0 {
                rt.throw_new(CANCELLATION);
                return Some(V::Void);
            }
            rt.put_field(this, "$value", a(1));
            rt.put_field(this, "$cancelled", V::I(1));
            V::Void
        }
        "getReturnValue" => rt.get_field(this, "$value"),
        // `getReturnValueZ()Z`, `getReturnValueI()I`...: the value as that primitive.
        _ if name.starts_with("getReturnValue") => {
            let kind = desc.chars().last().unwrap_or('L');
            let v = rt.get_field(this, "$value");
            unbox(rt, v, kind)
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        call(rt, key, args).unwrap_or_else(|| panic!("unmodelled {key}"))
    }

    /// What woven code does with a callback: an `@Inject` into a `boolean` method makes a
    /// returnable around the value, the handler sets another, and the method returns that one
    /// as a `boolean`; a callback that may not be cancelled throws Mixin's exception instead.
    #[test]
    fn a_handler_cancels_with_a_value_the_method_then_returns() {
        let mut rt = Rt::new();
        let id = rt.string("canSurvive");
        let cir = rt.new_object(RETURNABLE);
        c(&mut rt, "org/spongepowered/asm/mixin/injection/callback/CallbackInfoReturnable.<init>.(Ljava/lang/String;ZZ)V", &[cir, id, V::I(1), V::I(1)]);
        assert_eq!(c(&mut rt, "org/spongepowered/asm/mixin/injection/callback/CallbackInfoReturnable.getReturnValueZ.()Z", &[cir]), V::I(1));
        assert_eq!(c(&mut rt, "org/spongepowered/asm/mixin/injection/callback/CallbackInfoReturnable.isCancelled.()Z", &[cir]), V::I(0));
        // The handler: `cir.setReturnValue(false)` (javac boxes it).
        let no = box_value(&mut rt, V::I(0), 'Z');
        c(&mut rt, "org/spongepowered/asm/mixin/injection/callback/CallbackInfoReturnable.setReturnValue.(Ljava/lang/Object;)V", &[cir, no]);
        assert_eq!(c(&mut rt, "org/spongepowered/asm/mixin/injection/callback/CallbackInfoReturnable.isCancelled.()Z", &[cir]), V::I(1));
        assert_eq!(c(&mut rt, "org/spongepowered/asm/mixin/injection/callback/CallbackInfoReturnable.getReturnValueZ.()Z", &[cir]), V::I(0));
        // A float method's value, boxed on the way in, comes back a float.
        let cir = rt.new_object(RETURNABLE);
        c(&mut rt, "org/spongepowered/asm/mixin/injection/callback/CallbackInfoReturnable.<init>.(Ljava/lang/String;ZF)V", &[cir, id, V::I(0), V::F(0.5)]);
        let boxed = c(&mut rt, "org/spongepowered/asm/mixin/injection/callback/CallbackInfoReturnable.getReturnValue.()Ljava/lang/Object;", &[cir]);
        assert_eq!(rt.class_of(boxed), "java/lang/Float");
        assert_eq!(unbox(&rt, boxed, 'F'), V::F(0.5));
        // Not cancellable: `cancel()` throws and the callback stays as it was.
        let ci = rt.new_object(CALLBACK_INFO);
        c(&mut rt, "org/spongepowered/asm/mixin/injection/callback/CallbackInfo.<init>.(Ljava/lang/String;Z)V", &[ci, id, V::I(0)]);
        c(&mut rt, "org/spongepowered/asm/mixin/injection/callback/CallbackInfo.cancel.()V", &[ci]);
        let thrown = rt.thrown.take().expect("thrown");
        assert_eq!(rt.class_of(thrown), CANCELLATION);
        assert_eq!(c(&mut rt, "org/spongepowered/asm/mixin/injection/callback/CallbackInfo.isCancelled.()Z", &[ci]), V::I(0));
    }
}
