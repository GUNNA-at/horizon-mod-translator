//! `java.lang.invoke` in native Rust: lookups, `MethodType`s, handles to the mod's fields and
//! methods (and the game's), invokers, call sites, and `LambdaMetafactory` called by hand.
//!
//! A handle (`java/lang/invoke/MethodHandle`) keeps its kind and what it reaches: `getter` and
//! `setter` a `Field`, `method` a `Method` (invoked on its first argument), `static` a class,
//! name and type, `new` a class and constructor descriptor, `invoker` nothing (it invokes the
//! handle it is given), `factory` the implementation a metafactory binds, `bound` a handle with
//! leading arguments, `constant` a value. Mekanism builds its menus' field getters and setters
//! this way (`LambdaMetaFactoryUtil`): `metafactory(lookup, "apply", methodType(Function.class,
//! MethodHandle.class), ..., exactInvoker(getter.type()), ...)`, then `getTarget().invokeExact(getter)`.
use crate::jlang::{box_raw, construct, descriptor_class, descriptor_of, field_object, field_store, field_value, find_field, invoke_method, return_kind, text};
use crate::{Data, Rt, V};

const HANDLE: &str = "java/lang/invoke/MethodHandle";
const METHOD_TYPE: &str = "java/lang/invoke/MethodType";
const CALL_SITE: &str = "java/lang/invoke/ConstantCallSite";

/// A handle of `kind` reaching `target` (a `Field` or `Method`, or null).
pub(crate) fn handle(rt: &mut Rt, kind: &str, target: V) -> V {
    let h = rt.new_object(HANDLE);
    let k = rt.string(kind);
    rt.put_field(h, "$kind", k);
    rt.put_field(h, "$target", target);
    h
}

/// A `MethodType` returning `ret` (a `Class`) and taking `params` (`Class`es).
pub(crate) fn method_type(rt: &mut Rt, ret: V, params: Vec<V>) -> V {
    let t = rt.new_object(METHOD_TYPE);
    rt.put_field(t, "$ret", ret);
    let p = crate::jutil::new_list(rt, params);
    rt.put_field(t, "$params", p);
    t
}

/// A type's return class and parameter classes.
fn type_parts(rt: &Rt, t: V) -> (V, Vec<V>) {
    (rt.get_field(t, "$ret"), crate::jutil::elements(rt, rt.get_field(t, "$params")))
}

/// The class name a `Class` holds (`int`, `a/b/C`, `[I`).
fn class_name(rt: &Rt, c: V) -> String {
    rt.str_of(c).unwrap_or("java/lang/Object").to_owned()
}

/// A type as a method descriptor: `(ILjava/lang/Object;)V`.
fn descriptor(rt: &Rt, t: V) -> String {
    let (ret, params) = type_parts(rt, t);
    let mut d = String::from("(");
    for p in params {
        d.push_str(&descriptor_of(&class_name(rt, p)));
    }
    d.push(')');
    d.push_str(&descriptor_of(&class_name(rt, ret)));
    d
}

/// The box of a primitive class (`int` is `Integer`, `void` `Void`), or None for a reference.
fn wrapper(name: &str) -> Option<&'static str> {
    Some(match name {
        "int" => "java/lang/Integer",
        "long" => "java/lang/Long",
        "boolean" => "java/lang/Boolean",
        "byte" => "java/lang/Byte",
        "char" => "java/lang/Character",
        "short" => "java/lang/Short",
        "float" => "java/lang/Float",
        "double" => "java/lang/Double",
        "void" => "java/lang/Void",
        _ => return None,
    })
}

fn is_primitive(name: &str) -> bool {
    wrapper(name).is_some()
}

/// `MethodType.wrap` (`unwrap` false) and `unwrap` (true), `erase` and `generic` of one class.
fn map_class(rt: &mut Rt, c: V, how: &str) -> V {
    let n = class_name(rt, c);
    match how {
        "wrap" => wrapper(&n).map_or(c, |w| rt.class_literal(w)),
        "unwrap" => match ["int", "long", "boolean", "byte", "char", "short", "float", "double", "void"].into_iter().find(|p| wrapper(p) == Some(n.as_str())) {
            Some(p) => rt.class_literal(p),
            None => c,
        },
        "erase" if !is_primitive(&n) => rt.class_literal("java/lang/Object"),
        "generic" => rt.class_literal("java/lang/Object"),
        _ => c,
    }
}

/// The classes a `Class[]` or `List<Class>` holds.
fn classes(rt: &Rt, v: V) -> Vec<V> {
    crate::jutil::elements(rt, v)
}

/// What handle `h` takes and returns (`MethodHandle.type()`): the type it was made with, else
/// a field's (the receiver first unless static) or a method's as the mod's class declares it.
fn handle_type(rt: &mut Rt, h: V) -> V {
    if let t @ V::R(r) = rt.get_field(h, "$type")
        && r != 0
    {
        return t;
    }
    let kind = text(rt, rt.get_field(h, "$kind"));
    let target = rt.get_field(h, "$target");
    let object = rt.class_literal("java/lang/Object");
    let t = match kind.as_str() {
        "getter" | "setter" => {
            let class = text(rt, rt.get_field(target, "$class")).replace('.', "/");
            let name = text(rt, rt.get_field(target, "$name"));
            let (access, desc) = find_field(rt, &class, &name).unwrap_or((0, "Ljava/lang/Object;".into()));
            let field = rt.class_literal(descriptor_class(&desc));
            let owner = rt.class_literal(&class);
            let mut params = if access & 0x0008 != 0 { Vec::new() } else { vec![owner] };
            if kind == "getter" {
                method_type(rt, field, params)
            } else {
                params.push(field);
                let void = rt.class_literal("void");
                method_type(rt, void, params)
            }
        }
        "method" => {
            let class = text(rt, rt.get_field(target, "$class")).replace('.', "/");
            let name = text(rt, rt.get_field(target, "$name"));
            let params = classes(rt, rt.get_field(target, "$params"));
            let mut prefix = format!("{name}.(");
            for p in &params {
                prefix.push_str(&descriptor_of(&class_name(rt, *p)));
            }
            prefix.push(')');
            let ret = rt.vkeys.keys().find(|k| k.starts_with(&prefix)).map(|k| k[prefix.len()..].to_owned()).unwrap_or_else(|| "Ljava/lang/Object;".into());
            let ret = rt.class_literal(descriptor_class(&ret));
            let owner = rt.class_literal(&class);
            let mut all = vec![owner];
            all.extend(params);
            method_type(rt, ret, all)
        }
        _ => method_type(rt, object, Vec::new()),
    };
    rt.put_field(h, "$type", t);
    t
}

/// A copy of handle `h` (`asType`, `withVarargs`): the same reach under another type.
fn copy_handle(rt: &mut Rt, h: V, ty: V) -> V {
    let fields = match rt.heap.get(h.r() as usize).map(|o| &o.data) {
        Some(Data::Fields(f)) => f.clone(),
        _ => return h,
    };
    let c = rt.alloc(HANDLE, Data::Fields(fields));
    rt.put_field(c, "$type", ty);
    c
}

/// Invokes handle `h` with `args`, its result as a call site of return kind `ret` takes it: a box
/// where it returns an object, the raw value where a primitive, nothing for `V`. None (a miss)
/// where it reaches what the runtime cannot call: a mod's static method, a method no mod class has.
pub(crate) fn invoke_handle(rt: &mut Rt, h: V, args: &[V], ret: char) -> Option<V> {
    let kind = text(rt, rt.get_field(h, "$kind"));
    let target = rt.get_field(h, "$target");
    let first = args.first().copied().unwrap_or(V::NULL);
    let r = match kind.as_str() {
        "getter" => field_value(rt, target, if args.is_empty() { V::NULL } else { first }),
        "setter" => {
            let (obj, v) = match args {
                [v] => (V::NULL, *v),
                [obj, v, ..] => (*obj, *v),
                [] => (V::NULL, V::NULL),
            };
            field_store(rt, target, obj, v);
            V::Void
        }
        "method" => {
            let (obj, vals) = args.split_first().map_or((V::NULL, &[][..]), |(o, r)| (*o, r));
            invoke_method(rt, target, obj, vals)?
        }
        "static" => {
            let class = text(rt, rt.get_field(h, "$class"));
            let name = text(rt, rt.get_field(h, "$name"));
            let t = rt.get_field(h, "$type");
            let desc = descriptor(rt, t);
            if rt.is_mod_class(&class) {
                return None;
            }
            let key = rt.static_key(&format!("{class}.{name}.{desc}"));
            let mut vals = args.to_vec();
            for (v, k) in vals.iter_mut().zip(crate::jlang::kinds(&desc)) {
                *v = crate::mixin::unbox(rt, *v, k);
            }
            crate::api::call(rt, key, &vals)
        }
        "new" => {
            let class = text(rt, rt.get_field(h, "$class"));
            let desc = text(rt, rt.get_field(h, "$desc"));
            let mut vals = args.to_vec();
            for (v, k) in vals.iter_mut().zip(crate::jlang::kinds(&desc)) {
                *v = crate::mixin::unbox(rt, *v, k);
            }
            if rt.is_mod_class(&class) {
                construct(rt, &class, &desc, &vals)
            } else {
                let c: &'static str = rt.static_key(&class);
                let o = rt.new_object(c);
                let mut with_this = vec![o];
                with_this.extend(vals);
                let init = rt.static_key(&format!("{class}.<init>.{desc}"));
                crate::api::call(rt, init, &with_this);
                o
            }
        }
        // `exactInvoker(type)`: the handle it is given, invoked with the rest.
        "invoker" => {
            let (inner, rest) = args.split_first()?;
            return invoke_handle(rt, *inner, rest, ret);
        }
        // The metafactory's target: given what the lambda captures, the lambda.
        "factory" => {
            let implementation = rt.get_field(h, "$impl");
            let mut parts = vec![implementation];
            parts.extend_from_slice(args);
            crate::functions::handle_function(rt, parts)
        }
        "bound" => {
            let bound = crate::jutil::elements(rt, rt.get_field(h, "$bound"));
            let at = (rt.get_field(h, "$at").i().max(0) as usize).min(args.len());
            let mut all = args[..at].to_vec();
            all.extend(bound);
            all.extend_from_slice(&args[at..]);
            return invoke_handle(rt, target, &all, ret);
        }
        "constant" => rt.get_field(h, "$value"),
        "identity" => first,
        _ => return None,
    };
    Some(match ret {
        'L' | '[' => box_raw(rt, r),
        'V' => V::Void,
        k => crate::mixin::unbox(rt, r, k),
    })
}

/// A `java.lang.invoke` call, or None when this module doesn't model it.
pub(crate) fn call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match owner {
        "java/lang/invoke/MethodHandles" => match name {
            "lookup" | "publicLookup" | "privateLookupIn" => rt.intern("compat:lookup".into(), |rt| rt.new_object("java/lang/invoke/MethodHandles$Lookup")),
            "exactInvoker" | "invoker" => {
                let (ret, params) = type_parts(rt, a(0));
                let mh = rt.class_literal(HANDLE);
                let mut all = vec![mh];
                all.extend(params);
                let t = method_type(rt, ret, all);
                let h = handle(rt, "invoker", V::NULL);
                rt.put_field(h, "$type", t);
                h
            }
            "constant" => {
                let t = method_type(rt, a(0), Vec::new());
                let h = handle(rt, "constant", V::NULL);
                rt.put_field(h, "$value", a(1));
                rt.put_field(h, "$type", t);
                h
            }
            "identity" => {
                let t = method_type(rt, a(0), vec![a(0)]);
                let h = handle(rt, "identity", V::NULL);
                rt.put_field(h, "$type", t);
                h
            }
            // `insertArguments(target, pos, values...)`: the target with those bound at `pos`.
            "insertArguments" => {
                let values = crate::jutil::elements(rt, a(2));
                bound(rt, a(0), a(1).i(), values)
            }
            _ => return None,
        },
        "java/lang/invoke/MethodHandles$Lookup" => match name {
            "in" | "dropLookupMode" => a(0),
            "lookupClass" => rt.class_literal("java/lang/Object"),
            "unreflectGetter" => handle(rt, "getter", a(1)),
            "unreflectSetter" => handle(rt, "setter", a(1)),
            "unreflect" | "unreflectSpecial" => handle(rt, "method", a(1)),
            "unreflectConstructor" => {
                let class = rt.get_field(a(1), "$class");
                let class = class_name(rt, class);
                let desc = text(rt, rt.get_field(a(1), "$desc"));
                new_handle(rt, &class, &desc)
            }
            "findGetter" | "findStaticGetter" | "findSetter" | "findStaticSetter" => {
                let f = field_object(rt, a(1), a(2));
                handle(rt, if name.contains("Getter") { "getter" } else { "setter" }, f)
            }
            // A method by class, name and type: invoked on the receiver its first argument is.
            "findVirtual" | "findSpecial" => {
                let (ret, params) = type_parts(rt, a(3));
                let m = rt.new_object("java/lang/reflect/Method");
                rt.put_field(m, "$class", a(1));
                rt.put_field(m, "$name", a(2));
                let array = rt.alloc("[Ljava/lang/Class;", Data::Array(params.clone()));
                rt.put_field(m, "$params", array);
                let mut all = vec![a(1)];
                all.extend(params);
                let t = method_type(rt, ret, all);
                let h = handle(rt, "method", m);
                rt.put_field(h, "$type", t);
                h
            }
            "findStatic" => {
                let h = handle(rt, "static", V::NULL);
                let class = rt.string(&class_name(rt, a(1)));
                rt.put_field(h, "$class", class);
                rt.put_field(h, "$name", a(2));
                rt.put_field(h, "$type", a(3));
                h
            }
            "findConstructor" => {
                let class = class_name(rt, a(1));
                let desc = descriptor(rt, a(2));
                new_handle(rt, &class, &desc)
            }
            _ => return None,
        },
        HANDLE => match name {
            "invoke" | "invokeExact" | "invokeWithArguments" => {
                let rest: Vec<V> = if name == "invokeWithArguments" { crate::jutil::elements(rt, a(1)) } else { args[1..].to_vec() };
                let ret = if name == "invokeWithArguments" { 'L' } else { return_kind(desc) };
                return invoke_handle(rt, a(0), &rest, ret);
            }
            "type" => handle_type(rt, a(0)),
            "asType" => copy_handle(rt, a(0), a(1)),
            "withVarargs" | "asFixedArity" => a(0),
            "bindTo" => bound(rt, a(0), 0, vec![a(1)]),
            "isVarargsCollector" => V::I(0),
            _ => return None,
        },
        // `LambdaMetafactory.metafactory(lookup, name, factoryType, samType, impl, instantiated)`
        // and `altMetafactory(lookup, name, factoryType, [samType, impl, instantiated, flags...])`:
        // a call site whose target makes the lambda from what it captures.
        "java/lang/invoke/LambdaMetafactory" => match name {
            "metafactory" | "altMetafactory" => {
                let implementation = if name == "metafactory" { a(4) } else { crate::jutil::elements(rt, a(3)).get(1).copied().unwrap_or(V::NULL) };
                let target = handle(rt, "factory", V::NULL);
                rt.put_field(target, "$impl", implementation);
                rt.put_field(target, "$type", a(2));
                rt.put_field(target, "$sam", a(1));
                let site = rt.new_object(CALL_SITE);
                rt.put_field(site, "$target", target);
                site
            }
            _ => return None,
        },
        "java/lang/invoke/CallSite" | CALL_SITE | "java/lang/invoke/MutableCallSite" | "java/lang/invoke/VolatileCallSite" => match name {
            "<init>" => {
                if rt.heap.get(a(1).r() as usize).is_some_and(|o| o.class == HANDLE) {
                    rt.put_field(a(0), "$target", a(1));
                }
                V::Void
            }
            "getTarget" | "dynamicInvoker" => rt.get_field(a(0), "$target"),
            "setTarget" => {
                rt.put_field(a(0), "$target", a(1));
                V::Void
            }
            "type" => {
                let t = rt.get_field(a(0), "$target");
                handle_type(rt, t)
            }
            _ => return None,
        },
        METHOD_TYPE => method_type_call(rt, name, desc, args)?,
        _ => return None,
    })
}

/// A constructor of `class` with descriptor `desc` as a handle (it returns the new object).
fn new_handle(rt: &mut Rt, class: &str, desc: &str) -> V {
    let h = handle(rt, "new", V::NULL);
    let c = rt.string(class);
    rt.put_field(h, "$class", c);
    let d = rt.string(desc);
    rt.put_field(h, "$desc", d);
    let ret = rt.class_literal(class);
    let mut params = Vec::new();
    for p in param_descriptors(desc) {
        params.push(rt.class_literal(descriptor_class(&p)));
    }
    let t = method_type(rt, ret, params);
    rt.put_field(h, "$type", t);
    h
}

/// `target` with `values` bound as its arguments from position `at`.
fn bound(rt: &mut Rt, target: V, at: i32, values: Vec<V>) -> V {
    let h = handle(rt, "bound", target);
    let list = crate::jutil::new_list(rt, values.clone());
    rt.put_field(h, "$bound", list);
    rt.put_field(h, "$at", V::I(at));
    let t = handle_type(rt, target);
    let (ret, mut params) = type_parts(rt, t);
    let at = (at.max(0) as usize).min(params.len());
    params.drain(at..(at + values.len()).min(params.len()));
    let t = method_type(rt, ret, params);
    rt.put_field(h, "$type", t);
    h
}

/// Each parameter's descriptor in method descriptor `desc`.
fn param_descriptors(desc: &str) -> Vec<String> {
    let mut out = Vec::new();
    let params = desc.strip_prefix('(').and_then(|d| d.split(')').next()).unwrap_or("");
    let b = params.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        while b[i] == b'[' {
            i += 1;
        }
        if b[i] == b'L' {
            while b[i] != b';' {
                i += 1;
            }
        }
        i += 1;
        out.push(params[start..i].to_owned());
    }
    out
}

/// `MethodType`'s factories and methods.
fn method_type_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "methodType" => {
            let params = match desc {
                "(Ljava/lang/Class;)Ljava/lang/invoke/MethodType;" => Vec::new(),
                "(Ljava/lang/Class;Ljava/lang/Class;)Ljava/lang/invoke/MethodType;" => vec![a(1)],
                "(Ljava/lang/Class;Ljava/lang/Class;[Ljava/lang/Class;)Ljava/lang/invoke/MethodType;" => {
                    let mut p = vec![a(1)];
                    p.extend(classes(rt, a(2)));
                    p
                }
                "(Ljava/lang/Class;Ljava/lang/invoke/MethodType;)Ljava/lang/invoke/MethodType;" => type_parts(rt, a(1)).1,
                // `Class[]` or `List<Class>`.
                _ => classes(rt, a(1)),
            };
            method_type(rt, a(0), params)
        }
        "genericMethodType" => {
            let object = rt.class_literal("java/lang/Object");
            let n = a(0).i().max(0) as usize;
            method_type(rt, object, vec![object; n])
        }
        "fromMethodDescriptorString" => {
            let d = text(rt, a(0));
            let ret = d.rsplit(')').next().unwrap_or("V").to_owned();
            let ret = rt.class_literal(descriptor_class(&ret));
            let mut params = Vec::new();
            for p in param_descriptors(&d) {
                params.push(rt.class_literal(descriptor_class(&p)));
            }
            method_type(rt, ret, params)
        }
        "returnType" => rt.get_field(a(0), "$ret"),
        "parameterType" => type_parts(rt, a(0)).1.get(a(1).i().max(0) as usize).copied().unwrap_or(V::NULL),
        "lastParameterType" => {
            let params = type_parts(rt, a(0)).1;
            match params.last() {
                Some(&p) => p,
                None => rt.class_literal("void"),
            }
        }
        "parameterCount" => V::I(type_parts(rt, a(0)).1.len() as i32),
        "parameterList" => {
            let params = type_parts(rt, a(0)).1;
            crate::jutil::new_list(rt, params)
        }
        "parameterArray" => {
            let params = type_parts(rt, a(0)).1;
            rt.alloc("[Ljava/lang/Class;", Data::Array(params))
        }
        "wrap" | "unwrap" | "erase" | "generic" => {
            let (ret, params) = type_parts(rt, a(0));
            // `erase` keeps the return class's primitive and maps references to `Object`, as its
            // parameters; `generic` maps everything to `Object`.
            let ret = map_class(rt, ret, name);
            let params = params.into_iter().map(|p| map_class(rt, p, name)).collect();
            method_type(rt, ret, params)
        }
        "changeReturnType" => {
            let params = type_parts(rt, a(0)).1;
            method_type(rt, a(1), params)
        }
        "changeParameterType" => {
            let (ret, mut params) = type_parts(rt, a(0));
            if let Some(p) = params.get_mut(a(1).i().max(0) as usize) {
                *p = a(2);
            }
            method_type(rt, ret, params)
        }
        "dropParameterTypes" => {
            let (ret, mut params) = type_parts(rt, a(0));
            let (from, to) = (a(1).i().max(0) as usize, a(2).i().max(0) as usize);
            if from <= to && to <= params.len() {
                params.drain(from..to);
            }
            method_type(rt, ret, params)
        }
        "insertParameterTypes" | "appendParameterTypes" => {
            let (ret, mut params) = type_parts(rt, a(0));
            let (at, more) = if name == "appendParameterTypes" { (params.len(), classes(rt, a(1))) } else { ((a(1).i().max(0) as usize).min(params.len()), classes(rt, a(2))) };
            for (i, c) in more.into_iter().enumerate() {
                params.insert(at + i, c);
            }
            method_type(rt, ret, params)
        }
        "hasPrimitives" => {
            let (ret, params) = type_parts(rt, a(0));
            V::I(std::iter::once(ret).chain(params).any(|c| is_primitive(&class_name(rt, c))) as i32)
        }
        "hasWrappers" => {
            let (ret, params) = type_parts(rt, a(0));
            V::I(std::iter::once(ret).chain(params).any(|c| ["int", "long", "boolean", "byte", "char", "short", "float", "double", "void"].iter().any(|p| wrapper(p) == Some(class_name(rt, c).as_str()))) as i32)
        }
        "toMethodDescriptorString" | "descriptorString" => {
            let d = descriptor(rt, a(0));
            rt.string(&d)
        }
        "equals" => V::I((rt.class_of(a(1)) == METHOD_TYPE && descriptor(rt, a(0)) == descriptor(rt, a(1))) as i32),
        "hashCode" => V::I(crate::jlang::string_hash(&descriptor(rt, a(0)))),
        // `(int,Object)void`: simple names, as Java prints a type.
        "toString" => {
            let (ret, params) = type_parts(rt, a(0));
            let simple = |rt: &Rt, c: V| {
                let n = class_name(rt, c);
                n.rsplit('/').next().unwrap_or(&n).to_owned()
            };
            let names: Vec<String> = params.iter().map(|&p| simple(rt, p)).collect();
            let s = format!("({}){}", names.join(","), simple(rt, ret));
            rt.string(&s)
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions;

    fn c(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        crate::api::call(rt, key, args)
    }

    #[test]
    fn method_types_change_as_javas_do() {
        let mut rt = Rt::default();
        let int = rt.class_literal("int");
        let object = rt.class_literal("java/lang/Object");
        let string = rt.class_literal("java/lang/String");
        let t = c(&mut rt, "java/lang/invoke/MethodType.methodType.(Ljava/lang/Class;Ljava/lang/Class;)Ljava/lang/invoke/MethodType;", &[int, string]);
        let d = |rt: &mut Rt, t: V| {
            let s = c(rt, "java/lang/invoke/MethodType.toMethodDescriptorString.()Ljava/lang/String;", &[t]);
            rt.str_of(s).unwrap_or("").to_owned()
        };
        assert_eq!(d(&mut rt, t), "(Ljava/lang/String;)I");
        let wrapped = c(&mut rt, "java/lang/invoke/MethodType.wrap.()Ljava/lang/invoke/MethodType;", &[t]);
        assert_eq!(d(&mut rt, wrapped), "(Ljava/lang/String;)Ljava/lang/Integer;");
        let erased = c(&mut rt, "java/lang/invoke/MethodType.erase.()Ljava/lang/invoke/MethodType;", &[wrapped]);
        assert_eq!(d(&mut rt, erased), "(Ljava/lang/Object;)Ljava/lang/Object;");
        // `erase` keeps a primitive.
        let erased = c(&mut rt, "java/lang/invoke/MethodType.erase.()Ljava/lang/invoke/MethodType;", &[t]);
        assert_eq!(d(&mut rt, erased), "(Ljava/lang/Object;)I");
        let dropped = c(&mut rt, "java/lang/invoke/MethodType.dropParameterTypes.(II)Ljava/lang/invoke/MethodType;", &[t, V::I(0), V::I(1)]);
        assert_eq!(d(&mut rt, dropped), "()I");
        let void = rt.class_literal("void");
        let changed = c(&mut rt, "java/lang/invoke/MethodType.changeReturnType.(Ljava/lang/Class;)Ljava/lang/invoke/MethodType;", &[t, void]);
        assert_eq!(d(&mut rt, changed), "(Ljava/lang/String;)V");
        let extra = rt.alloc("[Ljava/lang/Class;", Data::Array(vec![int]));
        let three = c(&mut rt, "java/lang/invoke/MethodType.methodType.(Ljava/lang/Class;Ljava/lang/Class;[Ljava/lang/Class;)Ljava/lang/invoke/MethodType;", &[void, object, extra]);
        assert_eq!(d(&mut rt, three), "(Ljava/lang/Object;I)V");
        let n = c(&mut rt, "java/lang/invoke/MethodType.parameterCount.()I", &[three]);
        assert_eq!(n, V::I(2));
        let s = c(&mut rt, "java/lang/invoke/MethodType.toString.()Ljava/lang/String;", &[three]);
        assert_eq!(rt.str_of(s), Some("(Object,int)void"));
        let same = c(&mut rt, "java/lang/invoke/MethodType.methodType.(Ljava/lang/Class;Ljava/lang/Class;)Ljava/lang/invoke/MethodType;", &[int, string]);
        assert_eq!(c(&mut rt, "java/lang/invoke/MethodType.equals.(Ljava/lang/Object;)Z", &[t, same]), V::I(1));
        assert_eq!(c(&mut rt, "java/lang/invoke/MethodType.equals.(Ljava/lang/Object;)Z", &[t, three]), V::I(0));
    }

    /// Mekanism's `LambdaMetaFactoryUtil.createGetter` and `createSetter` for a field, as their
    /// bytecode makes them: an `exactInvoker` of the field handle's type bound by the metafactory,
    /// its target invoked with the handle; the `Function` and `BiConsumer` then read and write it.
    #[test]
    fn a_hand_made_metafactory_lambda_reads_and_writes_a_field() {
        let mut rt = Rt::default();
        rt.vcall = Some(|rt, key, _, args| crate::api::call(rt, key, args));
        let lookup = c(&mut rt, "java/lang/invoke/MethodHandles.lookup.()Ljava/lang/invoke/MethodHandles$Lookup;", &[]);
        // An object of the game's whose field the runtime keeps (`Data::Fields`).
        let obj = rt.new_object("compat:test/Machine");
        rt.put_field(obj, "energy", V::I(40));
        let class = rt.class_literal("compat:test/Machine");
        let name = rt.string("energy");
        let field = field_object(&mut rt, class, name);
        let getter = c(&mut rt, "java/lang/invoke/MethodHandles$Lookup.unreflectGetter.(Ljava/lang/reflect/Field;)Ljava/lang/invoke/MethodHandle;", &[lookup, field]);
        let ty = c(&mut rt, "java/lang/invoke/MethodHandle.type.()Ljava/lang/invoke/MethodType;", &[getter]);
        let function = rt.class_literal("java/util/function/Function");
        let mh = rt.class_literal(HANDLE);
        let factory_type = c(&mut rt, "java/lang/invoke/MethodType.methodType.(Ljava/lang/Class;Ljava/lang/Class;)Ljava/lang/invoke/MethodType;", &[function, mh]);
        let erased = c(&mut rt, "java/lang/invoke/MethodType.erase.()Ljava/lang/invoke/MethodType;", &[ty]);
        let invoker = c(&mut rt, "java/lang/invoke/MethodHandles.exactInvoker.(Ljava/lang/invoke/MethodType;)Ljava/lang/invoke/MethodHandle;", &[ty]);
        let apply = rt.string("apply");
        let site = c(
            &mut rt,
            "java/lang/invoke/LambdaMetafactory.metafactory.(Ljava/lang/invoke/MethodHandles$Lookup;Ljava/lang/String;Ljava/lang/invoke/MethodType;Ljava/lang/invoke/MethodType;Ljava/lang/invoke/MethodHandle;Ljava/lang/invoke/MethodType;)Ljava/lang/invoke/CallSite;",
            &[lookup, apply, factory_type, erased, invoker, ty],
        );
        let target = c(&mut rt, "java/lang/invoke/CallSite.getTarget.()Ljava/lang/invoke/MethodHandle;", &[site]);
        let get = c(&mut rt, "java/lang/invoke/MethodHandle.invokeExact.(Ljava/lang/invoke/MethodHandle;)Ljava/util/function/Function;", &[target, getter]);
        // `apply` answers the field's value, boxed as an `Object`.
        let v = rt.call_functional("java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;", get, &[obj]);
        assert_eq!(rt.class_of(v), "java/lang/Integer");
        assert_eq!(crate::mixin::unbox(&rt, v, 'I'), V::I(40));
        // The setter: a `BiConsumer` storing a box into the int field as an int.
        let setter = c(&mut rt, "java/lang/invoke/MethodHandles$Lookup.unreflectSetter.(Ljava/lang/reflect/Field;)Ljava/lang/invoke/MethodHandle;", &[lookup, field]);
        let st = c(&mut rt, "java/lang/invoke/MethodHandle.type.()Ljava/lang/invoke/MethodType;", &[setter]);
        let accept = rt.string("accept");
        let bi = rt.class_literal("java/util/function/BiConsumer");
        let factory_type = c(&mut rt, "java/lang/invoke/MethodType.methodType.(Ljava/lang/Class;Ljava/lang/Class;)Ljava/lang/invoke/MethodType;", &[bi, mh]);
        let invoker = c(&mut rt, "java/lang/invoke/MethodHandles.exactInvoker.(Ljava/lang/invoke/MethodType;)Ljava/lang/invoke/MethodHandle;", &[st]);
        let site = c(
            &mut rt,
            "java/lang/invoke/LambdaMetafactory.metafactory.(Ljava/lang/invoke/MethodHandles$Lookup;Ljava/lang/String;Ljava/lang/invoke/MethodType;Ljava/lang/invoke/MethodType;Ljava/lang/invoke/MethodHandle;Ljava/lang/invoke/MethodType;)Ljava/lang/invoke/CallSite;",
            &[lookup, accept, factory_type, st, invoker, st],
        );
        let target = c(&mut rt, "java/lang/invoke/CallSite.getTarget.()Ljava/lang/invoke/MethodHandle;", &[site]);
        let set = c(&mut rt, "java/lang/invoke/MethodHandle.invokeExact.(Ljava/lang/invoke/MethodHandle;)Ljava/util/function/BiConsumer;", &[target, setter]);
        let boxed = crate::mixin::box_value(&mut rt, V::I(75), 'I');
        rt.call_functional("java/util/function/BiConsumer.accept.(Ljava/lang/Object;Ljava/lang/Object;)V", set, &[obj, boxed]);
        assert_eq!(rt.get_field(obj, "energy"), V::I(75));
        // Through an interface of the mod's own (Mekanism's `ShortSetter.set(Object, short)`).
        let set_short = functions::handle_function(&mut rt, vec![setter]);
        crate::api::call(&mut rt, "mekanism/common/util/LambdaMetaFactoryUtil$ShortSetter.set.(Ljava/lang/Object;S)V", &[set_short, obj, V::I(9)]);
        assert_eq!(rt.get_field(obj, "energy"), V::I(9));
    }

    #[test]
    fn bound_and_constant_handles_answer_as_javas() {
        let mut rt = Rt::default();
        let string = rt.class_literal("java/lang/String");
        let hello = rt.string("hello");
        let k = c(&mut rt, "java/lang/invoke/MethodHandles.constant.(Ljava/lang/Class;Ljava/lang/Object;)Ljava/lang/invoke/MethodHandle;", &[string, hello]);
        let v = c(&mut rt, "java/lang/invoke/MethodHandle.invoke.()Ljava/lang/Object;", &[k]);
        assert_eq!(v, hello);
        let id = c(&mut rt, "java/lang/invoke/MethodHandles.identity.(Ljava/lang/Class;)Ljava/lang/invoke/MethodHandle;", &[string]);
        let bound = c(&mut rt, "java/lang/invoke/MethodHandle.bindTo.(Ljava/lang/Object;)Ljava/lang/invoke/MethodHandle;", &[id, hello]);
        let v = c(&mut rt, "java/lang/invoke/MethodHandle.invokeExact.()Ljava/lang/String;", &[bound]);
        assert_eq!(v, hello);
        let t = c(&mut rt, "java/lang/invoke/MethodHandle.type.()Ljava/lang/invoke/MethodType;", &[bound]);
        assert_eq!(c(&mut rt, "java/lang/invoke/MethodType.parameterCount.()I", &[t]), V::I(0));
    }
}
