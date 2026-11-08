//! Night Config (`com.electronwill.nightconfig`), which NeoForge ships and some mods' config
//! libraries use themselves (SpectreLib, inside comforts): its configs in memory, natively. A
//! config is an object holding its values (`$map`, a `LinkedHashMap`, where a nested config is a
//! value) and its comments (`$comments`, by key: a nested value's comment is its own level's, as
//! Night Config keeps them); a path is a list of keys or a dotted string. NeoForge's mod configs
//! and their TOML files are `mod_configs.rs`'s; a file config a mod opens through Night Config
//! itself is a config with a path, whose `load` reads nothing yet (its values stay what the mod
//! puts in, its defaults) and whose `save` writes nothing.
use crate::{Data, Rt, Table, V, jutil};

const NC: &str = "com/electronwill/nightconfig/";
const COMMENTED: &str = "com/electronwill/nightconfig/core/CommentedConfig";
const FILE_CONFIG: &str = "com/electronwill/nightconfig/core/file/CommentedFileConfig";
const BUILDER: &str = "com/electronwill/nightconfig/core/file/CommentedFileConfigBuilder";
const IN_MEMORY: &str = "com/electronwill/nightconfig/core/InMemoryFormat";
const ENUM_GET: &str = "com/electronwill/nightconfig/core/EnumGetMethod";
const SUPPLIER: &str = "java/util/function/Supplier.get.()Ljava/lang/Object;";

/// Night Config's enums: (class, constants in order).
const ENUMS: [(&str, &[&str]); 4] = [
    (ENUM_GET, &["NAME", "NAME_IGNORECASE", "ORDINAL_OR_NAME", "ORDINAL_OR_NAME_IGNORECASE"]),
    ("com/electronwill/nightconfig/core/ConfigSpec$CorrectionAction", &["ADD", "REPLACE", "REMOVE"]),
    ("com/electronwill/nightconfig/core/io/WritingMode", &["REPLACE", "REPLACE_ATOMIC", "APPEND"]),
    ("com/electronwill/nightconfig/core/io/ParsingMode", &["REPLACE", "MERGE", "ADD"]),
];

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    owner.starts_with(NC)
}

/// Night Config's enum constants (`EnumGetMethod.NAME_IGNORECASE`) and `FileNotFoundAction`'s.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    if !owner.starts_with(NC) {
        return None;
    }
    if let Some((class, names)) = ENUMS.iter().find(|(c, _)| *c == owner) {
        let i = names.iter().position(|n| *n == field)?;
        return Some(crate::api::api_enum(rt, class, names[i], i as i32));
    }
    if owner == "com/electronwill/nightconfig/core/file/FileNotFoundAction" {
        let f = field.to_owned();
        return Some(rt.intern(format!("compat:nc-file-action/{f}"), move |rt| {
            let a = rt.new_object("com/electronwill/nightconfig/core/file/FileNotFoundAction");
            let n = rt.string(&f);
            rt.put_field(a, "$name", n);
            a
        }));
    }
    None
}

fn new_map(rt: &mut Rt) -> V {
    rt.alloc("java/util/LinkedHashMap", Data::Table(Table::default()))
}

/// A config of `class` holding `map` (none: a new one).
pub(crate) fn new_config(rt: &mut Rt, class: &'static str, map: Option<V>) -> V {
    let c = rt.new_object(class);
    init(rt, c, map);
    c
}

fn init(rt: &mut Rt, c: V, map: Option<V>) {
    let m = match map {
        Some(m) if m.r() != 0 => m,
        _ => new_map(rt),
    };
    let comments = new_map(rt);
    rt.put_field(c, "$map", m);
    rt.put_field(c, "$comments", comments);
}

pub(crate) fn is_config(rt: &Rt, v: V) -> bool {
    v.r() != 0 && rt.get_field(v, "$map").r() != 0
}

/// `c`'s values, made if a mod's own config class never ran a constructor of Night Config's.
pub(crate) fn values(rt: &mut Rt, c: V) -> V {
    if !is_config(rt, c) {
        init(rt, c, None);
    }
    rt.get_field(c, "$map")
}

/// A path as Night Config takes it: a list of keys, or a string split at its dots.
fn path(rt: &Rt, p: V) -> Vec<String> {
    if let Some(s) = rt.str_of(p) {
        return s.split('.').map(str::to_owned).collect();
    }
    jutil::elements(rt, p).into_iter().map(|k| rt.str_of(k).unwrap_or("").to_owned()).collect()
}

/// The config holding `path`'s last key, and that key: nested configs made on the way when `make`,
/// else None where a level is missing.
pub(crate) fn parent(rt: &mut Rt, config: V, path: &[String], make: bool) -> Option<(V, String)> {
    let (last, levels) = path.split_last()?;
    let mut c = config;
    for k in levels {
        let map = values(rt, c);
        let key = rt.string(k);
        c = match jutil::table_get(rt, map, key) {
            Some(sub) if is_config(rt, sub) => sub,
            _ if make => {
                let sub = new_config(rt, COMMENTED, None);
                jutil::table_put(rt, map, key, sub);
                sub
            }
            _ => return None,
        };
    }
    Some((c, last.clone()))
}

/// The value at `p`, None when nothing is there.
fn get(rt: &mut Rt, config: V, p: V) -> Option<V> {
    let path = path(rt, p);
    let (c, k) = parent(rt, config, &path, false)?;
    let map = values(rt, c);
    let key = rt.string(&k);
    jutil::table_get(rt, map, key).filter(|v| v.r() != 0 || !matches!(v, V::R(_)))
}

/// Puts `v` at `p`, making the levels on the way; the value it replaces (null for none).
fn set(rt: &mut Rt, config: V, p: V, v: V) -> V {
    let path = path(rt, p);
    let Some((c, k)) = parent(rt, config, &path, true) else { return V::NULL };
    let map = values(rt, c);
    let key = rt.string(&k);
    let old = jutil::table_get(rt, map, key).unwrap_or(V::NULL);
    jutil::table_put(rt, map, key, v);
    old
}

fn remove(rt: &mut Rt, config: V, p: V) -> V {
    let path = path(rt, p);
    let Some((c, k)) = parent(rt, config, &path, false) else { return V::NULL };
    let map = values(rt, c);
    let key = rt.string(&k);
    jutil::table_remove(rt, map, key).unwrap_or(V::NULL)
}

/// A boxed number's value (`Integer.valueOf(5)`), or the raw one.
fn number(rt: &Rt, v: V) -> V {
    match v {
        V::R(_) => rt.get_field(v, "value"),
        raw => raw,
    }
}

fn as_i64(v: V) -> Option<i64> {
    match v {
        V::I(i) => Some(i64::from(i)),
        V::J(j) => Some(j),
        V::F(f) => Some(f as i64),
        V::D(d) => Some(d as i64),
        _ => None,
    }
}

/// The constant of enum `class` that `value` names (its name, or its ordinal where `method` allows),
/// as `EnumGetMethod.get` finds it; a constant is itself.
pub(crate) fn enum_of(rt: &mut Rt, value: V, class: &str, method: V) -> Option<V> {
    let how = if method.r() != 0 { crate::api_common::enum_name(rt, method, false) } else { V::NULL };
    let how = rt.str_of(how).unwrap_or("NAME").to_owned();
    let constants = crate::jlang::enum_constants(rt, class)?;
    let constants = jutil::elements(rt, constants);
    if constants.contains(&value) {
        return Some(value);
    }
    if let Some(name) = rt.str_of(value).map(str::to_owned) {
        let ignore = how.ends_with("IGNORECASE");
        return constants.into_iter().find(|&c| {
            let n = crate::api_common::enum_name(rt, c, false);
            rt.str_of(n).is_some_and(|n| if ignore { n.eq_ignore_ascii_case(&name) } else { n == name })
        });
    }
    if how.starts_with("ORDINAL")
        && let Some(i) = as_i64(number(rt, value))
    {
        return usize::try_from(i).ok().and_then(|i| constants.get(i).copied());
    }
    None
}

/// The class of the enum `getEnum*` asks for: its `Class` argument's, or its default constant's.
fn enum_class(rt: &Rt, desc: &str, args: &[V]) -> String {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if desc.contains(";Ljava/lang/Class;") {
        rt.str_of(a(2)).unwrap_or("").replace('.', "/")
    } else {
        rt.class_of(a(2)).to_owned()
    }
}

/// `c`'s comments, by key (made with its values if need be).
pub(crate) fn comments(rt: &mut Rt, c: V) -> V {
    values(rt, c);
    match rt.get_field(c, "$comments") {
        m if m.r() != 0 => m,
        _ => {
            let m = new_map(rt);
            rt.put_field(c, "$comments", m);
            m
        }
    }
}

/// Where the comment of `p` is kept: the comments of the level holding its last key (made on the
/// way when `make`), and that key; None where a level is missing.
fn comment_slot(rt: &mut Rt, config: V, p: V, make: bool) -> Option<(V, V)> {
    let path = path(rt, p);
    let (c, k) = parent(rt, config, &path, make)?;
    let comments = comments(rt, c);
    Some((comments, rt.string(&k)))
}

/// A format of Night Config's (`InMemoryFormat.withUniversalSupport()`, `TomlFormat.instance()`):
/// one object per class.
fn format(rt: &mut Rt, class: &'static str) -> V {
    rt.intern(format!("compat:nc-format/{class}"), move |rt| rt.new_object(class))
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !owner.starts_with(NC) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let commented = owner.contains("Commented") || owner.contains("toml/");
    let config_class: &'static str = if commented { COMMENTED } else { "com/electronwill/nightconfig/core/Config" };
    // Static factories first: their first argument is no config.
    let made = match name {
        "getDefaultMapCreator" => Some(crate::functions::constructor(rt, "java/util/LinkedHashMap", "()V")),
        "withUniversalSupport" | "withSupport" | "defaultInstance" if owner == IN_MEMORY => Some(format(rt, IN_MEMORY)),
        "instance" if owner.ends_with("Format") => Some(format(rt, owner)),
        "setInsertionOrderPreserved" => Some(V::Void),
        "isInsertionOrderPreserved" => Some(V::I(1)),
        "inMemory" | "inMemoryUniversal" | "inMemoryConcurrent" | "inMemoryUniversalConcurrent" => Some(new_config(rt, config_class, None)),
        // `Config.of(mapCreator, format)`, `Config.of(format)`, `Config.wrap(map, format)`.
        "of" | "wrap" if owner.ends_with("core/Config") || owner.ends_with("core/CommentedConfig") => {
            let map = if desc.starts_with("(Ljava/util/function/Supplier;") {
                Some(rt.call_functional(SUPPLIER, a(0), &[]))
            } else if desc.starts_with("(Ljava/util/Map;") {
                Some(a(0))
            } else {
                None
            };
            let c = new_config(rt, config_class, map);
            if desc.contains("ConfigFormat;)")
                && let Some(&f) = args.last()
            {
                rt.put_field(c, "$format", f);
            }
            Some(c)
        }
        // `Config.copy(config[, format])`: a new config of the same values (nested ones shared).
        "copy" if owner.ends_with("core/Config") || owner.ends_with("core/CommentedConfig") => {
            let from = values(rt, a(0));
            let c = new_config(rt, config_class, None);
            let to = values(rt, c);
            for (k, v) in jutil::table_pairs(rt, from) {
                jutil::table_put(rt, to, k, v);
            }
            Some(c)
        }
        // A file config and its builder: the path kept, nothing read or written.
        "of" | "builder" if owner.contains("/file/") => {
            let b = rt.new_object(if name == "of" { FILE_CONFIG } else { BUILDER });
            if name == "of" {
                init(rt, b, None);
            }
            rt.put_field(b, "$path", a(0));
            Some(b)
        }
        _ => None,
    };
    if made.is_some() {
        return made;
    }
    let this = a(0);
    // A file config's builder: `build()` makes the config; the rest set how it reads and writes.
    if owner.contains("Builder") && owner.contains("/file/") {
        return Some(match name {
            "<init>" => {
                rt.put_field(this, "$path", a(1));
                V::Void
            }
            "build" => {
                let c = new_config(rt, FILE_CONFIG, None);
                let p = rt.get_field(this, "$path");
                rt.put_field(c, "$path", p);
                c
            }
            _ if desc.ends_with("Builder;") => this,
            _ => return None,
        });
    }
    // Formats: in memory or TOML, each making configs.
    if owner.ends_with("Format") {
        return Some(match name {
            "createConfig" | "createConcurrentConfig" => {
                let map = if desc.starts_with("(Ljava/util/function/Supplier;") { Some(rt.call_functional(SUPPLIER, a(1), &[])) } else { None };
                new_config(rt, if owner.contains("toml/") { COMMENTED } else { "com/electronwill/nightconfig/core/Config" }, map)
            }
            "supportsComments" => V::I(owner.contains("toml/") as i32),
            "supportsType" | "isInMemory" => V::I(1),
            "initEmptyFile" => V::Void,
            _ => return None,
        });
    }
    if owner == ENUM_GET {
        return Some(match name {
            "get" => {
                let class = rt.str_of(a(2)).unwrap_or("").replace('.', "/");
                enum_of(rt, a(1), &class, this).unwrap_or(V::NULL)
            }
            "isEnumValue" => {
                let class = rt.str_of(a(2)).unwrap_or("").replace('.', "/");
                V::I(enum_of(rt, a(1), &class, this).is_some() as i32)
            }
            _ => return None,
        });
    }
    // An entry of a config's `entrySet()`: the map's own entries.
    if owner.ends_with("$Entry") && !is_config(rt, this) {
        let k = match name {
            "getKey" => "java/util/Map$Entry.getKey.()Ljava/lang/Object;",
            "getValue" | "getRawValue" => "java/util/Map$Entry.getValue.()Ljava/lang/Object;",
            "setValue" => "java/util/Map$Entry.setValue.(Ljava/lang/Object;)Ljava/lang/Object;",
            _ => return None,
        };
        return Some(crate::api::call(rt, k, args));
    }
    Some(match name {
        // `super(...)` from a mod's own config class (`AbstractConfig(map)`, `(mapCreator)`, `(concurrent)`).
        "<init>" => {
            let map = if desc.starts_with("(Ljava/util/Map;") {
                Some(a(1))
            } else if desc.starts_with("(Ljava/util/function/Supplier;") {
                Some(rt.call_functional(SUPPLIER, a(1), &[]))
            } else {
                None
            };
            init(rt, this, map);
            V::Void
        }
        "set" | "update" if args.len() == 3 => set(rt, this, a(1), a(2)),
        "add" if args.len() == 3 => {
            let there = get(rt, this, a(1)).is_some();
            if !there {
                set(rt, this, a(1), a(2));
            }
            V::I(!there as i32)
        }
        "get" | "getRaw" => get(rt, this, a(1)).unwrap_or(V::NULL),
        "getOptional" => {
            let v = get(rt, this, a(1)).unwrap_or(V::NULL);
            crate::api_common::optional(rt, v)
        }
        "getOrElse" => match get(rt, this, a(1)) {
            Some(v) => v,
            None if desc.contains(";Ljava/util/function/Supplier;)") => rt.call_functional(SUPPLIER, a(2), &[]),
            None => a(2),
        },
        "getInt" | "getLong" | "getByte" | "getShort" | "getChar" | "getIntOrElse" | "getLongOrElse" | "getByteOrElse" | "getShortOrElse" | "getCharOrElse" => {
            let long = name.starts_with("getLong");
            let found = get(rt, this, a(1)).and_then(|v| as_i64(number(rt, v)));
            let n = match found {
                Some(n) => n,
                None if name.ends_with("OrElse") => {
                    let d = if desc.contains("Supplier;)") {
                        let k = if long { "java/util/function/LongSupplier.getAsLong.()J" } else { "java/util/function/IntSupplier.getAsInt.()I" };
                        rt.call_functional(k, a(2), &[])
                    } else {
                        a(2)
                    };
                    as_i64(d).unwrap_or(0)
                }
                None => 0,
            };
            if long { V::J(n) } else { V::I(n as i32) }
        }
        "getEnum" | "getEnumOrElse" | "getOptionalEnum" => {
            let class = enum_class(rt, desc, args);
            let method = args.iter().copied().find(|&v| v.r() != 0 && rt.class_of(v) == ENUM_GET).unwrap_or(V::NULL);
            let found = get(rt, this, a(1)).and_then(|v| enum_of(rt, v, &class, method));
            match (name, found) {
                ("getOptionalEnum", v) => crate::api_common::optional(rt, v.unwrap_or(V::NULL)),
                (_, Some(v)) => v,
                ("getEnumOrElse", None) if desc.contains("Supplier;)") => rt.call_functional(SUPPLIER, args[args.len() - 1], &[]),
                ("getEnumOrElse", None) => a(2),
                _ => V::NULL,
            }
        }
        "contains" => V::I(get(rt, this, a(1)).is_some() as i32),
        "isNull" => V::I(get(rt, this, a(1)).is_none() as i32),
        "remove" if args.len() == 2 => remove(rt, this, a(1)),
        "clear" => {
            let fresh = new_map(rt);
            rt.put_field(this, "$map", fresh);
            V::Void
        }
        "valueMap" => values(rt, this),
        "entrySet" => {
            let map = values(rt, this);
            crate::api::call(rt, "java/util/Map.entrySet.()Ljava/util/Set;", &[map])
        }
        "size" => {
            let map = values(rt, this);
            V::I(jutil::table_pairs(rt, map).len() as i32)
        }
        "isEmpty" => {
            let map = values(rt, this);
            V::I(jutil::table_pairs(rt, map).is_empty() as i32)
        }
        "createSubConfig" => new_config(rt, COMMENTED, None),
        "configFormat" => match rt.get_field(this, "$format") {
            f if f.r() != 0 => f,
            _ => format(rt, IN_MEMORY),
        },
        // The comment kept by the level holding the path's last key; a null one removed. The
        // previous comment.
        "setComment" => match comment_slot(rt, this, a(1), true) {
            Some((comments, k)) if a(2).is_null() => {
                jutil::table_remove(rt, comments, k).unwrap_or(V::NULL)
            }
            Some((comments, k)) => jutil::table_put(rt, comments, k, a(2)),
            None => V::NULL,
        },
        "getComment" | "removeComment" | "containsComment" => {
            match (name, comment_slot(rt, this, a(1), false)) {
                ("getComment", Some((comments, k))) => {
                    jutil::table_get(rt, comments, k).unwrap_or(V::NULL)
                }
                ("removeComment", Some((comments, k))) => {
                    jutil::table_remove(rt, comments, k).unwrap_or(V::NULL)
                }
                ("containsComment", Some((comments, k))) => {
                    V::I(jutil::table_get(rt, comments, k).is_some_and(|c| !c.is_null()) as i32)
                }
                ("containsComment", None) => V::I(0),
                _ => V::NULL,
            }
        }
        // This level's comments, by key.
        "commentMap" => comments(rt, this),
        "clearComments" => {
            let fresh = new_map(rt);
            rt.put_field(this, "$comments", fresh);
            V::Void
        }
        "checked" | "unmodifiable" => this,
        // A file config: no file behind it here.
        "load" | "save" | "close" => V::Void,
        "getNioPath" | "getFile" => rt.get_field(this, "$path"),
        "toString" => {
            let map = values(rt, this);
            let n = jutil::table_pairs(rt, map).len();
            rt.string(&format!("{} with {n} values", owner.rsplit('/').next().unwrap_or("Config")))
        }
        "hashCode" => V::I(this.r() as i32),
        "equals" => V::I((this == a(1)) as i32),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SpectreLib's way (comforts): a config of the default map creator, values set by path lists
    /// and dotted strings into nested configs, read back with defaults for what isn't there; an enum
    /// stored as its name read back as the constant, ignoring case as asked; comments by path, each
    /// kept by its own level.
    #[test]
    fn a_config_keeps_values_by_path_and_reads_enums_by_name() {
        let mut rt = Rt::new();
        crate::install();
        let c = |rt: &mut Rt, key: &'static str, args: &[V]| crate::api::call(rt, key, args);
        let creator = c(&mut rt, "com/electronwill/nightconfig/core/Config.getDefaultMapCreator.(ZZ)Ljava/util/function/Supplier;", &[V::I(0), V::I(0)]);
        let universal = c(&mut rt, "com/electronwill/nightconfig/core/InMemoryFormat.withUniversalSupport.()Lcom/electronwill/nightconfig/core/InMemoryFormat;", &[]);
        let config = c(&mut rt, "com/electronwill/nightconfig/core/Config.of.(Ljava/util/function/Supplier;Lcom/electronwill/nightconfig/core/ConfigFormat;)Lcom/electronwill/nightconfig/core/Config;", &[creator, universal]);
        let other = c(&mut rt, "com/electronwill/nightconfig/core/Config.of.(Ljava/util/function/Supplier;Lcom/electronwill/nightconfig/core/ConfigFormat;)Lcom/electronwill/nightconfig/core/Config;", &[creator, universal]);
        let (general, enabled) = (rt.string("general"), rt.string("enabled"));
        let path = crate::jutil::new_list(&mut rt, vec![general, enabled]);
        let on = crate::api_common::boolean(&mut rt, true);
        let old = c(&mut rt, "com/electronwill/nightconfig/core/Config.set.(Ljava/util/List;Ljava/lang/Object;)Ljava/lang/Object;", &[config, path, on]);
        assert_eq!(old, V::NULL);
        let dotted = rt.string("general.enabled");
        assert_eq!(c(&mut rt, "com/electronwill/nightconfig/core/Config.get.(Ljava/lang/String;)Ljava/lang/Object;", &[config, dotted]), on);
        // The other config, of its own new map, has none of it.
        assert_eq!(c(&mut rt, "com/electronwill/nightconfig/core/Config.contains.(Ljava/lang/String;)Z", &[other, dotted]), V::I(0));
        let general_only = rt.string("general");
        let sub = c(&mut rt, "com/electronwill/nightconfig/core/Config.get.(Ljava/lang/String;)Ljava/lang/Object;", &[config, general_only]);
        assert_eq!(c(&mut rt, "com/electronwill/nightconfig/core/Config.size.()I", &[sub]), V::I(1));
        let missing = rt.string("general.radius");
        assert_eq!(c(&mut rt, "com/electronwill/nightconfig/core/Config.getIntOrElse.(Ljava/lang/String;I)I", &[config, missing, V::I(16)]), V::I(16));

        let (slot_key, head) = (rt.string("slot"), rt.string("Head"));
        c(&mut rt, "com/electronwill/nightconfig/core/Config.set.(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;", &[config, slot_key, head]);
        let class = crate::jlang::class_object(&mut rt, "net/minecraft/world/entity/EquipmentSlot");
        let ignore_case = crate::api::get_static(&mut rt, "com/electronwill/nightconfig/core/EnumGetMethod.NAME_IGNORECASE");
        let slot = c(
            &mut rt,
            "com/electronwill/nightconfig/core/Config.getEnumOrElse.(Ljava/util/List;Ljava/lang/Class;Lcom/electronwill/nightconfig/core/EnumGetMethod;Ljava/util/function/Supplier;)Ljava/lang/Enum;",
            &[config, slot_key, class, ignore_case, V::NULL],
        );
        assert_eq!(slot, crate::api::get_static(&mut rt, "net/minecraft/world/entity/EquipmentSlot.HEAD"));
        let by_name = crate::api::get_static(&mut rt, "com/electronwill/nightconfig/core/EnumGetMethod.NAME");
        assert_eq!(c(&mut rt, "com/electronwill/nightconfig/core/EnumGetMethod.get.(Ljava/lang/Object;Ljava/lang/Class;)Ljava/lang/Enum;", &[by_name, head, class]), V::NULL);

        let note = rt.string("Whether sleeping bags work");
        c(&mut rt, "com/electronwill/nightconfig/core/CommentedConfig.setComment.(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;", &[config, dotted, note]);
        assert_eq!(c(&mut rt, "com/electronwill/nightconfig/core/CommentedConfig.getComment.(Ljava/lang/String;)Ljava/lang/String;", &[config, dotted]), note);
        // Kept by the level holding it, under its own key, as Night Config keeps it; a null one
        // removes it.
        assert_eq!(c(&mut rt, "com/electronwill/nightconfig/core/CommentedConfig.getComment.(Ljava/lang/String;)Ljava/lang/String;", &[sub, enabled]), note);
        let previous = c(&mut rt, "com/electronwill/nightconfig/core/CommentedConfig.setComment.(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;", &[config, dotted, V::NULL]);
        assert_eq!(previous, note);
        assert_eq!(c(&mut rt, "com/electronwill/nightconfig/core/CommentedConfig.containsComment.(Ljava/lang/String;)Z", &[sub, enabled]), V::I(0));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
