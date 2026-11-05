//! FML's scan of the mod's jars (`ModList.get().getAllScanData()`): the annotations on each class,
//! field and method, as the translator found them in the class files
//! ([`horizon_compat_core::Rt::scan_data`]), with ASM's `Type`s naming them.
//!
//! JEI and Jade find their plugins this way (`@JeiPlugin`, `@WailaPlugin`: the annotated class's
//! name, then `Class.forName` and its constructor), MrCrayfish's Framework and SuperMartijn642's
//! core library what they register, Mekanism the fields its menus sync.

use crate::{Data, Rt, V};

const SCAN_DATA: &str = "net/neoforged/neoforgespi/language/ModFileScanData";
const ANNOTATION_DATA: &str = "net/neoforged/neoforgespi/language/ModFileScanData$AnnotationData";
const MOD_FILE_INFO: &str = "net/neoforged/fml/loading/moddiscovery/ModFileInfo";
const ASM_TYPE: &str = "org/objectweb/asm/Type";
const ENUM_HOLDER: &str = "net/neoforged/fml/loading/modscan/ModAnnotation$EnumHolder";
const MOD_ANNOTATION: &str = "net/neoforged/fml/loading/modscan/ModAnnotation";
const ELEMENT_TYPE: &str = "java/lang/annotation/ElementType";
/// `ElementType`'s constants, in order.
const ELEMENT_TYPES: [&str; 12] =
    ["TYPE", "FIELD", "METHOD", "PARAMETER", "CONSTRUCTOR", "LOCAL_VARIABLE", "ANNOTATION_TYPE", "PACKAGE", "TYPE_PARAMETER", "TYPE_USE", "MODULE", "RECORD_COMPONENT"];

/// The classes [`call`] answers for.
pub(crate) fn claims(owner: &str) -> bool {
    matches!(owner, SCAN_DATA | ANNOTATION_DATA | ASM_TYPE | ENUM_HOLDER | MOD_ANNOTATION | ELEMENT_TYPE)
}

/// `ElementType.NAME`.
pub(crate) fn element_type(rt: &mut Rt, name: &str) -> Option<V> {
    let i = ELEMENT_TYPES.iter().position(|n| *n == name)?;
    Some(crate::api::api_enum(rt, ELEMENT_TYPE, ELEMENT_TYPES[i], i as i32))
}

pub(crate) fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    if owner == ELEMENT_TYPE { element_type(rt, field) } else { None }
}

/// ASM's `Type` for descriptor `desc` (`Lcom/a/B;`, `I`, `[I`): one object per descriptor.
pub(crate) fn asm_type(rt: &mut Rt, desc: &str) -> V {
    rt.intern(format!("compat:asm-type/{desc}"), |rt| {
        let t = rt.new_object(ASM_TYPE);
        let d = rt.string(desc);
        rt.put_field(t, "$desc", d);
        t
    })
}

/// The descriptor of a class by the name a `Class` object holds (`a/b/C`, `int`, `[I`).
fn descriptor(name: &str) -> String {
    let primitive = match name {
        "int" => "I",
        "long" => "J",
        "short" => "S",
        "byte" => "B",
        "char" => "C",
        "float" => "F",
        "double" => "D",
        "boolean" => "Z",
        "void" => "V",
        _ => "",
    };
    if !primitive.is_empty() {
        primitive.to_owned()
    } else if name.starts_with('[') {
        name.replace('.', "/")
    } else {
        format!("L{};", name.replace('.', "/"))
    }
}

/// `Type.getClassName()`: `com.a.B`, `int`, `int[]`.
fn class_name(desc: &str) -> String {
    let dims = desc.chars().take_while(|c| *c == '[').count();
    let base = match &desc[dims..] {
        "I" => "int".to_owned(),
        "J" => "long".to_owned(),
        "S" => "short".to_owned(),
        "B" => "byte".to_owned(),
        "C" => "char".to_owned(),
        "F" => "float".to_owned(),
        "D" => "double".to_owned(),
        "Z" => "boolean".to_owned(),
        "V" => "void".to_owned(),
        d => d.trim_start_matches('L').trim_end_matches(';').replace('/', "."),
    };
    base + &"[]".repeat(dims)
}

/// `ModList.getAllScanData()`: one `ModFileScanData` per jar, made once.
pub(crate) fn all(rt: &mut Rt) -> V {
    rt.intern("compat:scan-data".into(), |rt| {
        let files: Vec<V> = (0..rt.scan_data.len())
            .map(|i| {
                let f = rt.new_object(SCAN_DATA);
                rt.put_field(f, "$index", V::I(i as i32));
                f
            })
            .collect();
        crate::jutil::new_list(rt, files)
    })
}

/// A scan's annotations (`getAnnotations()`), read from its text the first time.
fn annotations(rt: &mut Rt, file: V) -> V {
    if let v @ V::R(_) = rt.get_field(file, "$annotations") {
        return v;
    }
    let text = rt.scan_data.get(rt.get_field(file, "$index").i() as usize).map_or("", |(_, t)| *t);
    let mut items = Vec::new();
    for line in text.lines() {
        let mut f = line.splitn(5, '\t');
        let (Some(ty), Some(target), Some(class), Some(member), Some(data)) = (f.next(), f.next(), f.next(), f.next(), f.next()) else { continue };
        let a = rt.new_object(ANNOTATION_DATA);
        let t = asm_type(rt, ty);
        rt.put_field(a, "annotationType", t);
        let e = element_type(rt, target).unwrap_or(V::NULL);
        rt.put_field(a, "targetType", e);
        let c = asm_type(rt, &format!("L{class};"));
        rt.put_field(a, "clazz", c);
        let m = rt.string(member);
        rt.put_field(a, "memberName", m);
        let values = serde_json::from_str::<serde_json::Value>(data).unwrap_or_default();
        let map = values_map(rt, &values);
        rt.put_field(a, "annotationData", map);
        items.push(a);
    }
    let set = crate::jutil::identity_set(rt, &items);
    rt.put_field(file, "$annotations", set);
    set
}

/// An annotation's values as FML keeps them (`annotationData()`): name to value.
fn values_map(rt: &mut Rt, values: &serde_json::Value) -> V {
    let m = rt.alloc("java/util/HashMap", Data::Table(Default::default()));
    if let serde_json::Value::Object(o) = values {
        for (k, v) in o {
            let key = rt.string(k);
            let value = value(rt, v);
            crate::jutil::table_put(rt, m, key, value);
        }
    }
    m
}

fn boxed(rt: &mut Rt, class: &'static str, v: V) -> V {
    let o = rt.new_object(class);
    rt.put_field(o, "value", v);
    o
}

/// One element value ([`horizon_mod_translator`]'s `Annotation::data`): a string, an
/// `EnumHolder`, a `Type`, a boxed primitive, a list (an array), a `ModAnnotation`.
fn value(rt: &mut Rt, v: &serde_json::Value) -> V {
    use serde_json::Value;
    match v {
        Value::String(s) => rt.string(s),
        Value::Array(items) => {
            let items: Vec<V> = items.iter().map(|i| value(rt, i)).collect();
            crate::jutil::new_list(rt, items)
        }
        Value::Object(o) => {
            let text = |k: &str| o.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
            if let Some(e) = o.get("e").and_then(Value::as_str) {
                let h = rt.new_object(ENUM_HOLDER);
                let (d, n) = (rt.string(e), rt.string(&text("v")));
                rt.put_field(h, "desc", d);
                rt.put_field(h, "value", n);
                return h;
            }
            if let Some(c) = o.get("c").and_then(Value::as_str) {
                return asm_type(rt, c);
            }
            if let Some(ty) = o.get("@").and_then(Value::as_str) {
                let a = rt.new_object(MOD_ANNOTATION);
                let t = asm_type(rt, ty);
                rt.put_field(a, "$type", t);
                let values = values_map(rt, o.get("v").unwrap_or(&Value::Null));
                rt.put_field(a, "$values", values);
                return a;
            }
            let Some((tag, n)) = o.iter().next() else { return V::NULL };
            let int = n.as_i64().unwrap_or(0);
            let float = n.as_f64().unwrap_or(0.0);
            match tag.as_str() {
                "I" => crate::api_common::integer(rt, int as i32),
                "Z" => crate::api_common::boolean(rt, int != 0),
                "B" => boxed(rt, "java/lang/Byte", V::I(int as i32)),
                "C" => boxed(rt, "java/lang/Character", V::I(int as i32)),
                "S" => boxed(rt, "java/lang/Short", V::I(int as i32)),
                "J" => boxed(rt, "java/lang/Long", V::J(int)),
                "F" => boxed(rt, "java/lang/Float", V::F(float as f32)),
                "D" => boxed(rt, "java/lang/Double", V::D(float)),
                _ => V::NULL,
            }
        }
        _ => V::NULL,
    }
}

pub(crate) fn call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let desc_of = |rt: &Rt, t: V| rt.str_of(rt.get_field(t, "$desc")).unwrap_or("").to_owned();
    Some(match (owner, name) {
        (SCAN_DATA, "getAnnotations") => annotations(rt, a(0)),
        // `getAnnotatedBy(annotationClass, elementType)`: those of that type on that kind of member.
        (SCAN_DATA, "getAnnotatedBy") => {
            let wanted = descriptor(rt.str_of(a(1)).unwrap_or(""));
            let set = annotations(rt, a(0));
            let mut found = Vec::new();
            for an in crate::jutil::elements(rt, set) {
                let t = rt.get_field(an, "annotationType");
                if desc_of(rt, t) == wanted && rt.get_field(an, "targetType") == a(2) {
                    found.push(an);
                }
            }
            rt.alloc("java/util/stream/Stream", Data::Stream(found))
        }
        // The jar's mods' metadata.
        (SCAN_DATA, "getIModInfoData") => {
            let ids = rt.scan_data.get(rt.get_field(a(0), "$index").i() as usize).map_or("", |(m, _)| *m);
            let first = ids.split(',').next().unwrap_or("").to_owned();
            let f = rt.new_object(MOD_FILE_INFO);
            let (id, all) = (rt.string(&first), rt.string(ids));
            rt.put_field(f, "$modId", id);
            rt.put_field(f, "$modIds", all);
            crate::jutil::new_list(rt, vec![f])
        }
        (ANNOTATION_DATA, "annotationType" | "targetType" | "clazz" | "memberName" | "annotationData") => {
            let field: &'static str = match name {
                "annotationType" => "annotationType",
                "targetType" => "targetType",
                "clazz" => "clazz",
                "memberName" => "memberName",
                _ => "annotationData",
            };
            rt.get_field(a(0), field)
        }
        (ENUM_HOLDER, "desc" | "getDesc") => rt.get_field(a(0), "desc"),
        (ENUM_HOLDER, "value" | "getValue") => rt.get_field(a(0), "value"),
        (MOD_ANNOTATION, "getASMType") => rt.get_field(a(0), "$type"),
        (MOD_ANNOTATION, "getValues") => rt.get_field(a(0), "$values"),
        (MOD_ANNOTATION, "getTargetType") => element_type(rt, "ANNOTATION_TYPE")?,
        // ASM's `Type`, by its descriptor.
        (ASM_TYPE, "getType") if desc.starts_with("(Ljava/lang/Class;)") => {
            let d = descriptor(rt.str_of(a(0)).unwrap_or(""));
            asm_type(rt, &d)
        }
        (ASM_TYPE, "getType") if desc.starts_with("(Ljava/lang/String;)") => {
            let d = rt.str_of(a(0)).unwrap_or("").to_owned();
            asm_type(rt, &d)
        }
        (ASM_TYPE, "getObjectType") => {
            let n = rt.str_of(a(0)).unwrap_or("").to_owned();
            let d = if n.starts_with('[') { n } else { format!("L{n};") };
            asm_type(rt, &d)
        }
        (ASM_TYPE, "getClassName") => {
            let n = class_name(&desc_of(rt, a(0)));
            rt.string(&n)
        }
        (ASM_TYPE, "getInternalName") => {
            let d = desc_of(rt, a(0));
            let n = if d.starts_with('L') { d[1..d.len() - 1].to_owned() } else { d };
            rt.string(&n)
        }
        (ASM_TYPE, "getDescriptor" | "toString") => {
            let d = desc_of(rt, a(0));
            rt.string(&d)
        }
        (ASM_TYPE, "equals") => V::I((rt.class_of(a(1)) == ASM_TYPE && desc_of(rt, a(0)) == desc_of(rt, a(1))) as i32),
        (ASM_TYPE, "hashCode") => V::I(crate::jlang::string_hash(&desc_of(rt, a(0)))),
        (ELEMENT_TYPE, "values") => {
            let all: Vec<V> = ELEMENT_TYPES.iter().filter_map(|n| element_type(rt, n)).collect();
            rt.alloc("[Ljava/lang/annotation/ElementType;", Data::Array(all))
        }
        (ELEMENT_TYPE, "valueOf") => {
            let n = rt.str_of(a(0)).unwrap_or("").to_owned();
            match element_type(rt, &n) {
                Some(e) => e,
                None => crate::jlang::throw_with(rt, "java/lang/IllegalArgumentException", &format!("No enum constant java.lang.annotation.ElementType.{n}")),
            }
        }
        (ELEMENT_TYPE, "name" | "toString") => rt.get_field(a(0), "$name"),
        (ELEMENT_TYPE, "ordinal" | "hashCode") => rt.get_field(a(0), "$ordinal"),
        (ELEMENT_TYPE, "compareTo") => V::I(rt.get_field(a(0), "$ordinal").i() - rt.get_field(a(1), "$ordinal").i()),
        (ELEMENT_TYPE, "equals") => V::I((a(0) == a(1)) as i32),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// JEI's and Jade's way to their plugins: the annotations of one type, each naming its class;
    /// a value's type kept (a string, an enum constant, a class, an int, an array).
    #[test]
    fn a_scan_lists_annotations_as_fml_finds_them() {
        let mut rt = Rt::new();
        crate::install();
        rt.scan_data.push((
            "mymod,mymod_compat",
            "Lmezz/jei/api/JeiPlugin;\tTYPE\tcom/my/compat/MyJei\tcom.my.compat.MyJei\t{}\n\
             Lcom/my/Sync;\tFIELD\tcom/my/Tile\tenergy\t{\"tag\":\"e\",\"side\":{\"e\":\"Lnet/neoforged/api/distmarker/Dist;\",\"v\":\"CLIENT\"},\"type\":{\"c\":\"Ljava/lang/Long;\"},\"size\":{\"I\":3},\"names\":[\"a\",\"b\"]}\n",
        ));
        let c = |rt: &mut Rt, k: &'static str, args: &[V]| crate::api::call(rt, k, args);
        let files = c(&mut rt, "net/neoforged/fml/ModList.getAllScanData.()Ljava/util/List;", &[V::NULL]);
        let files = crate::jutil::elements(&rt, files);
        assert_eq!(files.len(), 1);
        let set = c(&mut rt, "net/neoforged/neoforgespi/language/ModFileScanData.getAnnotations.()Ljava/util/Set;", &[files[0]]);
        let found = crate::jutil::elements(&rt, set);
        assert_eq!(found.len(), 2);

        // `Type.getType(JeiPlugin.class).equals(data.annotationType())`, then the class's name.
        let class = crate::jlang::class_object(&mut rt, "mezz/jei/api/JeiPlugin");
        let wanted = c(&mut rt, "org/objectweb/asm/Type.getType.(Ljava/lang/Class;)Lorg/objectweb/asm/Type;", &[class]);
        let ty = c(&mut rt, "net/neoforged/neoforgespi/language/ModFileScanData$AnnotationData.annotationType.()Lorg/objectweb/asm/Type;", &[found[0]]);
        assert_eq!(c(&mut rt, "org/objectweb/asm/Type.equals.(Ljava/lang/Object;)Z", &[wanted, ty]), V::I(1));
        let member = c(&mut rt, "net/neoforged/neoforgespi/language/ModFileScanData$AnnotationData.memberName.()Ljava/lang/String;", &[found[0]]);
        assert_eq!(rt.str_of(member), Some("com.my.compat.MyJei"));
        let target = c(&mut rt, "net/neoforged/neoforgespi/language/ModFileScanData$AnnotationData.targetType.()Ljava/lang/annotation/ElementType;", &[found[0]]);
        assert_eq!(target, crate::api::get_static(&mut rt, "java/lang/annotation/ElementType.TYPE"));
        let field_type = crate::api::get_static(&mut rt, "java/lang/annotation/ElementType.FIELD");
        let sync = crate::jlang::class_object(&mut rt, "com/my/Sync");
        let by = c(
            &mut rt,
            "net/neoforged/neoforgespi/language/ModFileScanData.getAnnotatedBy.(Ljava/lang/Class;Ljava/lang/annotation/ElementType;)Ljava/util/stream/Stream;",
            &[files[0], sync, field_type],
        );
        let by = c(&mut rt, "java/util/stream/Stream.toList.()Ljava/util/List;", &[by]);
        assert_eq!(crate::jutil::elements(&rt, by), [found[1]]);

        let data = c(&mut rt, "net/neoforged/neoforgespi/language/ModFileScanData$AnnotationData.annotationData.()Ljava/util/Map;", &[found[1]]);
        let get = |rt: &mut Rt, k: &str| {
            let key = rt.string(k);
            crate::api::call(rt, "java/util/Map.get.(Ljava/lang/Object;)Ljava/lang/Object;", &[data, key])
        };
        let tag = get(&mut rt, "tag");
        assert_eq!(rt.str_of(tag), Some("e"));
        let side = get(&mut rt, "side");
        let v = c(&mut rt, "net/neoforged/fml/loading/modscan/ModAnnotation$EnumHolder.value.()Ljava/lang/String;", &[side]);
        assert_eq!(rt.str_of(v), Some("CLIENT"));
        let ty = get(&mut rt, "type");
        let n = c(&mut rt, "org/objectweb/asm/Type.getClassName.()Ljava/lang/String;", &[ty]);
        assert_eq!(rt.str_of(n), Some("java.lang.Long"));
        let size = get(&mut rt, "size");
        assert_eq!(rt.get_field(size, "value"), V::I(3));
        let names = get(&mut rt, "names");
        let names: Vec<String> = crate::jutil::elements(&rt, names).iter().map(|&n| rt.str_of(n).unwrap_or("").to_owned()).collect();
        assert_eq!(names, ["a", "b"]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
