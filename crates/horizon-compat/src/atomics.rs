//! `java.util.concurrent.atomic` and `VarHandle`s, on the one thread a runtime has: there a
//! value read, changed and written back is atomic with nothing more. An atomic keeps its value
//! in a field, an atomic array in a Java array; a field updater and a field's `VarHandle` name
//! the field they change, an array's handle the element. Each of those places is a [`Cell`],
//! and every operation ([`operate`]) reads one and writes it.
//!
//! Flywheel marks its changed instances in a bit set over `arrayElementVarHandle(long[].class)`,
//! Steam 'n' Rails reads Create's registrate through `findStaticVarHandle`, and Kotlin's
//! coroutines keep their states with field updaters.
use crate::jlang::{
    box_as, descriptor_class, descriptor_of, field_object, field_store, field_value, find_field,
    return_kind, text,
};
use crate::{Data, Rt, V};

pub(crate) const VAR_HANDLE: &str = "java/lang/invoke/VarHandle";
/// Where an atomic array keeps its elements, and a field updater its field's name.
const ARRAY: &str = "$array";
const FIELD: &str = "$field";

/// Where an operation reads and writes.
enum Cell {
    /// An object's own field, by name: an atomic's `value`, a field updater's field.
    Named(V, &'static str),
    /// A `Field` in an object, or its class's static given none (a `VarHandle` to a field).
    Reflected(V, V),
    /// An array's element.
    Element(V, i32),
    /// The bytes of a `byte[]` from an index, as a wider primitive
    /// (`byteArrayViewVarHandle`): its kind, and whether the first byte is the least
    /// significant.
    Bytes(V, i32, char, bool),
}

/// The bytes a primitive of `kind` takes in a byte array's view.
fn width(kind: char) -> i32 {
    match kind {
        'S' | 'C' => 2,
        'I' | 'F' => 4,
        _ => 8,
    }
}

/// What a field or an element of `kind` holds before anything is written to it.
fn zero_of(kind: char) -> V {
    match kind {
        'Z' | 'B' | 'C' | 'S' | 'I' => V::I(0),
        'J' => V::J(0),
        'F' => V::F(0.0),
        'D' => V::D(0.0),
        _ => V::NULL,
    }
}

impl Cell {
    /// What it holds: nothing (`Void`) for a field never written, or where reading it threw
    /// (an index outside the array).
    fn get(&self, rt: &mut Rt) -> V {
        match *self {
            Cell::Named(object, name) => rt.get_field(object, name),
            Cell::Reflected(field, object) => field_value(rt, field, object),
            Cell::Element(array, i) => rt.array_load(array, i).unwrap_or(V::Void),
            Cell::Bytes(array, at, kind, little) => {
                let (mut bits, n) = (0u64, width(kind));
                for k in 0..n {
                    let Some(b) = rt.array_load(array, at + k) else {
                        return V::Void;
                    };
                    bits |= u64::from(b.i() as u8) << (if little { k } else { n - 1 - k } * 8);
                }
                match kind {
                    'S' => V::I(i32::from(bits as i16)),
                    'C' => V::I(i32::from(bits as u16)),
                    'I' => V::I(bits as i32),
                    'F' => V::F(f32::from_bits(bits as u32)),
                    'D' => V::D(f64::from_bits(bits)),
                    _ => V::J(bits as i64),
                }
            }
        }
    }

    fn set(&self, rt: &mut Rt, v: V) {
        match *self {
            Cell::Named(object, name) => rt.put_field(object, name, v),
            Cell::Reflected(field, object) => field_store(rt, field, object, v),
            Cell::Element(array, i) => {
                rt.array_store(array, i, v);
            }
            Cell::Bytes(array, at, kind, little) => {
                let bits = match v {
                    V::I(i) => i as u32 as u64,
                    V::J(j) => j as u64,
                    V::F(f) => u64::from(f.to_bits()),
                    V::D(d) => d.to_bits(),
                    _ => 0,
                };
                let n = width(kind);
                for k in 0..n {
                    let byte = (bits >> (if little { k } else { n - 1 - k } * 8)) as u8;
                    if !rt.array_store(array, at + k, V::I(i32::from(byte as i8))) {
                        return;
                    }
                }
            }
        }
    }
}

/// Whether what a cell holds is what a compare-and-set expects: the same value or object. A
/// box or a string of equal value passes too, as the JVM's own cached boxes and interned
/// strings make it pass there for the values code compares so; any other object must be the
/// very one.
fn same(rt: &mut Rt, held: V, expected: V) -> bool {
    let value = |rt: &Rt, v: V| {
        v.r() != 0
            && matches!(
                rt.class_of(v),
                "java/lang/String"
                    | "java/lang/Integer"
                    | "java/lang/Long"
                    | "java/lang/Boolean"
                    | "java/lang/Short"
                    | "java/lang/Byte"
                    | "java/lang/Character"
                    | "java/lang/Float"
                    | "java/lang/Double"
            )
    };
    held == expected
        || (value(rt, held) && value(rt, expected) && crate::jutil::equals(rt, held, expected))
}

/// One atomic operation `name` on `cell`, which holds `zero` before anything is written to it,
/// with the operands `x` (what follows the object or the index in the call): `get` and `set`
/// under each access mode's name, the exchanges, the arithmetic, and the updates through a
/// function. None for a name that is none of them.
fn operate(rt: &mut Rt, cell: &Cell, zero: V, name: &str, x: &[V]) -> Option<V> {
    // One thread sees no difference between the access modes of an operation.
    let mut op = name;
    for mode in ["Volatile", "Acquire", "Release", "Opaque", "Plain"] {
        op = op.strip_suffix(mode).unwrap_or(op);
    }
    let op = match op {
        "lazySet" => "set",
        "weakCompareAndSet" => "compareAndSet",
        op => op,
    };
    let known = matches!(
        op,
        "get"
            | "set"
            | "getAndSet"
            | "compareAndSet"
            | "compareAndExchange"
            | "incrementAndGet"
            | "getAndIncrement"
            | "decrementAndGet"
            | "getAndDecrement"
            | "addAndGet"
            | "getAndAdd"
            | "getAndBitwiseOr"
            | "getAndBitwiseAnd"
            | "getAndBitwiseXor"
            | "updateAndGet"
            | "getAndUpdate"
            | "accumulateAndGet"
            | "getAndAccumulate"
    );
    if !known {
        return None;
    }
    let primitive = !matches!(zero, V::R(_));
    let held = match cell.get(rt) {
        V::Void => zero,
        V::R(0) if primitive => zero,
        v => v,
    };
    if rt.thrown.is_some() {
        return Some(zero);
    }
    let x = |i: usize| x.get(i).copied().unwrap_or_default();
    Some(match op {
        "get" => held,
        "set" => {
            cell.set(rt, x(0));
            V::Void
        }
        "getAndSet" => {
            cell.set(rt, x(0));
            held
        }
        "compareAndSet" | "compareAndExchange" => {
            let same = same(rt, held, x(0));
            if same {
                cell.set(rt, x(1));
            }
            if op == "compareAndSet" {
                V::I(i32::from(same))
            } else {
                held
            }
        }
        "incrementAndGet" | "getAndIncrement" | "decrementAndGet" | "getAndDecrement"
        | "addAndGet" | "getAndAdd" => {
            let by = match op {
                "addAndGet" | "getAndAdd" => x(0),
                op if op.contains("ecrement") => V::I(-1),
                _ => V::I(1),
            };
            let next = match (held, by) {
                (V::I(h), V::I(d)) => V::I(h.wrapping_add(d)),
                (V::J(h), V::J(d)) => V::J(h.wrapping_add(d)),
                (V::J(h), V::I(d)) => V::J(h.wrapping_add(i64::from(d))),
                (V::F(h), V::F(d)) => V::F(h + d),
                (V::D(h), V::D(d)) => V::D(h + d),
                _ => held,
            };
            cell.set(rt, next);
            if op.starts_with("getAnd") { held } else { next }
        }
        "getAndBitwiseOr" | "getAndBitwiseAnd" | "getAndBitwiseXor" => {
            let bits = |h: i64, m: i64| match op {
                "getAndBitwiseOr" => h | m,
                "getAndBitwiseAnd" => h & m,
                _ => h ^ m,
            };
            let next = match (held, x(0)) {
                (V::I(h), V::I(m)) => V::I(bits(i64::from(h), i64::from(m)) as i32),
                (V::J(h), V::J(m)) => V::J(bits(h, m)),
                _ => held,
            };
            cell.set(rt, next);
            held
        }
        // Through a function of the value's kind: an operator on it, or on it and the operand.
        _ => {
            let both = op.contains("ccumulate");
            let key = match (zero, both) {
                (V::I(_), false) => "java/util/function/IntUnaryOperator.applyAsInt.(I)I",
                (V::I(_), true) => "java/util/function/IntBinaryOperator.applyAsInt.(II)I",
                (V::J(_), false) => "java/util/function/LongUnaryOperator.applyAsLong.(J)J",
                (V::J(_), true) => "java/util/function/LongBinaryOperator.applyAsLong.(JJ)J",
                (_, false) => {
                    "java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;"
                }
                (_, true) => {
                    "java/util/function/BiFunction.apply.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;"
                }
            };
            let next = if both {
                rt.call_functional(key, x(1), &[held, x(0)])
            } else {
                rt.call_functional(key, x(0), &[held])
            };
            if rt.thrown.is_some() {
                return Some(held);
            }
            cell.set(rt, next);
            if op.starts_with("getAnd") { held } else { next }
        }
    })
}

/// A whole number as the primitive of `kind` (`Number`'s `intValue`, `longValue`...).
fn number(v: V, kind: char) -> V {
    let n = match v {
        V::I(i) => i64::from(i),
        V::J(j) => j,
        _ => 0,
    };
    match kind {
        'J' => V::J(n),
        'F' => V::F(n as f32),
        'D' => V::D(n as f64),
        'S' => V::I(i32::from(n as i16)),
        'B' => V::I(i32::from(n as i8)),
        _ => V::I(n as i32),
    }
}

/// A call on `java.util.concurrent.atomic`'s class `class` (its simple name), or None for what
/// isn't modelled.
pub(crate) fn call(rt: &mut Rt, class: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let rest = |from: usize| args.get(from..).unwrap_or(&[]);
    match class {
        "AtomicInteger" | "AtomicLong" | "AtomicBoolean" | "AtomicReference" => {
            let zero = match class {
                "AtomicLong" => V::J(0),
                "AtomicReference" => V::NULL,
                _ => V::I(0),
            };
            if name == "<init>" {
                rt.put_field(this, "value", args.get(1).copied().unwrap_or(zero));
                return Some(V::Void);
            }
            if let Some(v) = operate(rt, &Cell::Named(this, "value"), zero, name, rest(1)) {
                return Some(v);
            }
            let held = match rt.get_field(this, "value") {
                V::Void => zero,
                v => v,
            };
            Some(match name {
                "intValue" | "longValue" | "floatValue" | "doubleValue" | "shortValue"
                | "byteValue" => number(held, return_kind(desc)),
                "toString" => match class {
                    "AtomicReference" => crate::api::call(
                        rt,
                        "java/lang/String.valueOf.(Ljava/lang/Object;)Ljava/lang/String;",
                        &[held],
                    ),
                    "AtomicBoolean" => rt.string(if held.i() != 0 { "true" } else { "false" }),
                    _ => {
                        let V::J(n) = number(held, 'J') else {
                            return None;
                        };
                        rt.string(&n.to_string())
                    }
                },
                _ => return None,
            })
        }
        "AtomicIntegerArray" | "AtomicLongArray" | "AtomicReferenceArray" => {
            let (zero, of) = match class {
                "AtomicIntegerArray" => (V::I(0), "[I"),
                "AtomicLongArray" => (V::J(0), "[J"),
                _ => (V::NULL, "[Ljava/lang/Object;"),
            };
            if name == "<init>" {
                // Of a length, or a copy of an array.
                let items = if desc.starts_with("(I)") {
                    vec![zero; a(1).i().max(0) as usize]
                } else {
                    crate::jutil::elements(rt, a(1))
                };
                let array = rt.alloc(of, Data::Array(items));
                rt.put_field(this, ARRAY, array);
                return Some(V::Void);
            }
            let array = rt.get_field(this, ARRAY);
            match name {
                "length" => Some(V::I(rt.array_len(array))),
                // `[1, 2, 3]`, as `Arrays.toString` writes one.
                "toString" => {
                    let mut out = String::from("[");
                    for (i, v) in crate::jutil::elements(rt, array).into_iter().enumerate() {
                        if i > 0 {
                            out.push_str(", ");
                        }
                        match v {
                            V::I(n) => out.push_str(&n.to_string()),
                            V::J(n) => out.push_str(&n.to_string()),
                            v => {
                                let text = crate::api::call(
                                    rt,
                                    "java/lang/String.valueOf.(Ljava/lang/Object;)Ljava/lang/String;",
                                    &[v],
                                );
                                out.push_str(rt.str_of(text).unwrap_or("null"));
                            }
                        }
                    }
                    out.push(']');
                    Some(rt.string(&out))
                }
                _ => operate(rt, &Cell::Element(array, a(1).i()), zero, name, rest(2)),
            }
        }
        "AtomicIntegerFieldUpdater" | "AtomicLongFieldUpdater" | "AtomicReferenceFieldUpdater" => {
            let (zero, own) = match class {
                "AtomicIntegerFieldUpdater" => (
                    V::I(0),
                    "java/util/concurrent/atomic/AtomicIntegerFieldUpdater",
                ),
                "AtomicLongFieldUpdater" => (
                    V::J(0),
                    "java/util/concurrent/atomic/AtomicLongFieldUpdater",
                ),
                _ => (
                    V::NULL,
                    "java/util/concurrent/atomic/AtomicReferenceFieldUpdater",
                ),
            };
            // `newUpdater(class, [type,] name)`: the field by its name, in whatever object.
            if name == "newUpdater" {
                let updater = rt.new_object(own);
                rt.put_field(updater, FIELD, args.last().copied().unwrap_or_default());
                return Some(updater);
            }
            let field = text(rt, rt.get_field(this, FIELD));
            let field = rt.static_key(&field);
            operate(rt, &Cell::Named(a(1), field), zero, name, rest(2))
        }
        _ => None,
    }
}

/// A `VarHandle` of `kind` (`field`, `array`, `bytes`) to variables of class `ty`.
fn var_handle(rt: &mut Rt, kind: &str, ty: V) -> V {
    let h = rt.new_object(VAR_HANDLE);
    let kind = rt.string(kind);
    rt.put_field(h, "$kind", kind);
    rt.put_field(h, "$type", ty);
    h
}

/// `findVarHandle(class, name, type)` and `findStaticVarHandle`: a handle to that field, in an
/// object or the class's own.
pub(crate) fn field_handle(rt: &mut Rt, class: V, name: V, ty: V, is_static: bool) -> V {
    let h = var_handle(rt, "field", ty);
    let field = field_object(rt, class, name);
    rt.put_field(h, FIELD, field);
    rt.put_field(h, "$static", V::I(i32::from(is_static)));
    h
}

/// `unreflectVarHandle(field)`: a handle to what the `Field` names, static or not as its class
/// declares it.
pub(crate) fn reflected_handle(rt: &mut Rt, field: V) -> V {
    let class = text(rt, rt.get_field(field, "$class")).replace('.', "/");
    let name = text(rt, rt.get_field(field, "$name"));
    let (access, desc) = find_field(rt, &class, &name).unwrap_or((0, "Ljava/lang/Object;".into()));
    let ty = rt.class_literal(descriptor_class(&desc));
    let h = var_handle(rt, "field", ty);
    rt.put_field(h, FIELD, field);
    rt.put_field(h, "$static", V::I(i32::from(access & 0x0008 != 0)));
    h
}

/// The class of the elements of array class `array` (`[I` holds `int`s).
fn element_class(rt: &mut Rt, array: V) -> V {
    let name = rt.str_of(array).unwrap_or("[Ljava/lang/Object;").to_owned();
    rt.class_literal(descriptor_class(
        name.strip_prefix('[').unwrap_or("Ljava/lang/Object;"),
    ))
}

/// `MethodHandles.arrayElementVarHandle(arrayClass)`: a handle to an element of such an array.
pub(crate) fn array_handle(rt: &mut Rt, array: V) -> V {
    let ty = element_class(rt, array);
    var_handle(rt, "array", ty)
}

/// `MethodHandles.byteArrayViewVarHandle(viewArrayClass, order)`: a handle to the bytes of a
/// `byte[]` read as the view's elements, in that byte order.
pub(crate) fn bytes_handle(rt: &mut Rt, view: V, order: V) -> V {
    let ty = element_class(rt, view);
    let h = var_handle(rt, "bytes", ty);
    rt.put_field(
        h,
        "$little",
        V::I(i32::from(crate::nio::is_little(rt, order))),
    );
    h
}

/// A call on a `VarHandle`: its access methods take the variable's coordinates (an object, an
/// array and an index, or nothing for a static) and then their operands, and answer as the
/// call site's own descriptor says (they are signature-polymorphic).
pub(crate) fn var_handle_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    match name {
        // Memory fences: one thread needs none.
        "fullFence" | "acquireFence" | "releaseFence" | "loadLoadFence" | "storeStoreFence" => {
            return Some(V::Void);
        }
        "withInvokeExactBehavior" | "withInvokeBehavior" => return Some(this),
        "hasInvokeExactBehavior" => return Some(V::I(0)),
        "isAccessModeSupported" => return Some(V::I(1)),
        "varType" => return Some(rt.get_field(this, "$type")),
        _ => {}
    }
    let ty = rt.get_field(this, "$type");
    let kind = descriptor_of(rt.str_of(ty).unwrap_or("java/lang/Object"))
        .chars()
        .next()
        .unwrap_or('L');
    let (zero, primitive) = (zero_of(kind), !matches!(kind, 'L' | '['));
    let (cell, operands) = match text(rt, rt.get_field(this, "$kind")).as_str() {
        "field" => {
            let field = rt.get_field(this, FIELD);
            if rt.get_field(this, "$static").i() != 0 {
                (Cell::Reflected(field, V::NULL), args.get(1..))
            } else {
                (Cell::Reflected(field, a(1)), args.get(2..))
            }
        }
        "array" => (Cell::Element(a(1), a(2).i()), args.get(3..)),
        "bytes" => (
            Cell::Bytes(a(1), a(2).i(), kind, rt.get_field(this, "$little").i() != 0),
            args.get(3..),
        ),
        _ => return None,
    };
    // A primitive's operands as themselves, whatever the call site boxed.
    let operands: Vec<V> = operands
        .unwrap_or(&[])
        .iter()
        .map(|v| {
            if primitive {
                crate::mixin::unbox(rt, *v, kind)
            } else {
                *v
            }
        })
        .collect();
    let r = operate(rt, &cell, zero, name, &operands)?;
    Some(match return_kind(desc) {
        'V' => V::Void,
        'L' | '[' if primitive => box_as(rt, r, kind),
        'L' | '[' => r,
        k => crate::mixin::unbox(rt, r, k),
    })
}
