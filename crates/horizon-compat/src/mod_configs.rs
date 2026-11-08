//! NeoForge's mod configs (`ModConfigSpec`) and the TOML files FML keeps them in, natively. A mod
//! builds its spec with the `Builder`: each value at its path with its default and validator, the
//! comments it is given and the ones the builder adds (` Default: 42`, ` Range: > 0`,
//! `Allowed Values: ...`), its range, translation key and restart type; each level its comment. A
//! registered config loads as FML's `ConfigTracker.loadConfig` does it, from `config/<file>` in the
//! game's folder: a missing file is copied from `defaultconfigs/` or written from the spec's
//! defaults; a file the spec finds wrong (a value out of range or of another type, a key it doesn't
//! define, a comment that isn't its own) is backed up (`<name>-1.toml.bak`, five kept) and
//! corrected; one that isn't TOML is backed up and made again. A value reads from the loaded config
//! (cached until a reload clears it), `set` writes into it, and `save` writes the file as Night
//! Config's `TomlWriter` writes it and tells the mod (`ModConfigEvent.Reloading`). A runtime with no
//! game folder set (the probe, tests that set none) keeps each config in memory, its defaults, as
//! NeoForge keeps its default server configs.
use crate::{Data, Rt, V, jutil, nightconfig};
use std::path::{Path, PathBuf};

const SPEC: &str = "net/neoforged/neoforge/common/ModConfigSpec";
const BUILDER: &str = "net/neoforged/neoforge/common/ModConfigSpec$Builder";
/// What the builder's next definition takes (`BuilderContext`, private to NeoForge).
const CONTEXT: &str = "net/neoforged/neoforge/common/ModConfigSpec$BuilderContext";
const VALUE_SPEC: &str = "net/neoforged/neoforge/common/ModConfigSpec$ValueSpec";
const LIST_SPEC: &str = "net/neoforged/neoforge/common/ModConfigSpec$ListValueSpec";
const RANGE: &str = "net/neoforged/neoforge/common/ModConfigSpec$Range";
const RESTART: &str = "net/neoforged/neoforge/common/ModConfigSpec$RestartType";
const CONFIG_VALUE: &str = "net/neoforged/neoforge/common/ModConfigSpec$ConfigValue";
const BOOLEAN_VALUE: &str = "net/neoforged/neoforge/common/ModConfigSpec$BooleanValue";
const INT_VALUE: &str = "net/neoforged/neoforge/common/ModConfigSpec$IntValue";
const LONG_VALUE: &str = "net/neoforged/neoforge/common/ModConfigSpec$LongValue";
const DOUBLE_VALUE: &str = "net/neoforged/neoforge/common/ModConfigSpec$DoubleValue";
const ENUM_VALUE: &str = "net/neoforged/neoforge/common/ModConfigSpec$EnumValue";
const MOD_CONFIG: &str = "net/neoforged/fml/config/ModConfig";
const CONFIG_TYPE: &str = "net/neoforged/fml/config/ModConfig$Type";
const LOADED: &str = "net/neoforged/fml/config/LoadedConfig";
const ILOADED: &str = "net/neoforged/fml/config/IConfigSpec$ILoadedConfig";
const ICONFIG_SPEC: &str = "net/neoforged/fml/config/IConfigSpec";
const MOD_CONFIGS: &str = "net/neoforged/fml/config/ModConfigs";
const TRACKER: &str = "net/neoforged/fml/config/ConfigTracker";
pub(crate) const CONFIG_EVENT: &str = "net/neoforged/fml/event/config/ModConfigEvent";
pub(crate) const LOADING: &str = "net/neoforged/fml/event/config/ModConfigEvent$Loading";
const RELOADING: &str = "net/neoforged/fml/event/config/ModConfigEvent$Reloading";
const UNLOADING: &str = "net/neoforged/fml/event/config/ModConfigEvent$Unloading";
const COMMENTED: &str = "com/electronwill/nightconfig/core/CommentedConfig";
const CONFIG: &str = "com/electronwill/nightconfig/core/Config";
const ACTION: &str = "com/electronwill/nightconfig/core/ConfigSpec$CorrectionAction";
const WRITING: &str = "com/electronwill/nightconfig/core/io/WritingException";
const PARSING: &str = "com/electronwill/nightconfig/core/io/ParsingException";
const LISTENER: &str = "com/electronwill/nightconfig/core/ConfigSpec$CorrectionListener.onCorrect.(Lcom/electronwill/nightconfig/core/ConfigSpec$CorrectionAction;Ljava/util/List;Ljava/lang/Object;Ljava/lang/Object;)V";
const SUPPLIER: &str = "java/util/function/Supplier.get.()Ljava/lang/Object;";
const PREDICATE: &str = "java/util/function/Predicate.test.(Ljava/lang/Object;)Z";
const IS_CORRECT: &str = "net/neoforged/fml/config/IConfigSpec.isCorrect.(Lcom/electronwill/nightconfig/core/UnmodifiableCommentedConfig;)Z";
const CORRECT: &str = "net/neoforged/fml/config/IConfigSpec.correct.(Lcom/electronwill/nightconfig/core/CommentedConfig;)V";
const ACCEPT: &str = "net/neoforged/fml/config/IConfigSpec.acceptConfig.(Lnet/neoforged/fml/config/IConfigSpec$ILoadedConfig;)V";
const SUPPLIER_DESC: &str = "Ljava/util/function/Supplier;";
/// The configs registered and not loaded yet, which load in NeoForge's "Config loading" step.
const PENDING: &str = "compat:configs";
/// Every config registered, in order (FML's `ConfigTracker`, which `ModConfigs` reads).
const ALL: &str = "compat:all-configs";
/// The backups FML keeps of a config file it corrects or makes again.
const BACKUPS: u32 = 5;
/// Night Config writes the system's line ends.
const NEWLINE: &str = if cfg!(windows) { "\r\n" } else { "\n" };
/// What `BuilderContext.buildComment` gives a comment of blank lines in a player's game.
const BLANK_COMMENT: &str = "A developer of this mod has defined this config option with a blank comment, which causes obscure bugs in NeoForge's config system and will cause a crash in the future. Please report this to the mod author.";

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    owner.starts_with(SPEC)
        || matches!(
            owner,
            LOADED | ILOADED | ICONFIG_SPEC | MOD_CONFIGS | TRACKER
        )
        || (owner == MOD_CONFIG && matches!(name, "getLoadedConfig" | "getFullPath"))
        || (owner == CONFIG_TYPE && name == "extension")
        || (owner.starts_with(CONFIG_EVENT) && name == "<init>")
        || matches!(
            (owner, name),
            ("java/util/function/Supplier", "get")
                | ("java/util/function/BooleanSupplier", "getAsBoolean")
                | ("java/util/function/IntSupplier", "getAsInt")
                | ("java/util/function/LongSupplier", "getAsLong")
                | ("java/util/function/DoubleSupplier", "getAsDouble")
        )
}

/// `ConfigTracker.INSTANCE`.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    (owner == TRACKER && field == "INSTANCE")
        .then(|| rt.intern("compat:config-tracker".into(), |rt| rt.new_object(TRACKER)))
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or(V::NULL);
    match owner {
        BUILDER => builder_call(rt, name, desc, args),
        SPEC => spec_call(rt, name, desc, args),
        ICONFIG_SPEC if rt.class_of(a(0)) == SPEC => spec_call(rt, name, desc, args),
        RANGE => range_call(rt, name, args),
        VALUE_SPEC | LIST_SPEC => value_spec_call(rt, name, args),
        RESTART if name == "with" => {
            let (this, other) = (restart_name(rt, a(0)), restart_name(rt, a(1)));
            Some(match (this.as_str(), other.as_str()) {
                (_, "NONE") => a(0),
                ("GAME", _) | (_, "GAME") => restart(rt, "GAME"),
                _ => restart(rt, "WORLD"),
            })
        }
        o if o.starts_with(SPEC) && o.ends_with("Value") => value_call(rt, name, desc, args),
        LOADED | ILOADED => loaded_call(rt, name, args),
        MOD_CONFIG => mod_config_call(rt, name, args),
        CONFIG_TYPE => {
            let n = rt.get_field(a(0), "$name");
            let lower = rt.str_of(n)?.to_ascii_lowercase();
            Some(rt.string(&lower))
        }
        MOD_CONFIGS => mod_configs_call(rt, name, args),
        TRACKER => tracker_call(rt, name, desc, args),
        o if o.starts_with(CONFIG_EVENT) => {
            event_supers(rt);
            rt.put_field(a(0), "$config", a(1));
            Some(V::Void)
        }
        // A config value handed on as the supplier it is.
        _ if is_value(rt, a(0)) => value_call(rt, name, desc, args),
        _ => None,
    }
}

// --- Values and paths -----------------------------------------------------------------------

/// A field, null where the object has none.
fn field(rt: &Rt, o: V, name: &'static str) -> V {
    match rt.get_field(o, name) {
        V::Void => V::NULL,
        v => v,
    }
}

fn text(rt: &Rt, v: V) -> Option<String> {
    rt.str_of(v).map(str::to_owned)
}

fn npe(rt: &mut Rt, message: &str) -> V {
    crate::files::throw(rt, "java/lang/NullPointerException", message)
}

/// A path's keys: a string's, split at its dots (Guava's `Splitter.on(".")`), or a list's.
fn keys(rt: &Rt, p: V) -> Vec<String> {
    match rt.str_of(p) {
        Some(s) => s.split('.').map(str::to_owned).collect(),
        None => jutil::elements(rt, p)
            .into_iter()
            .map(|k| rt.str_of(k).unwrap_or("null").to_owned())
            .collect(),
    }
}

fn key_list(rt: &mut Rt, keys: &[String]) -> V {
    let items: Vec<V> = keys.iter().map(|k| rt.string(k)).collect();
    jutil::new_list(rt, items)
}

/// The key a level's comment and translation key are kept under: its path (NeoForge keys them by
/// the path's list).
fn level_key(path: &[String]) -> String {
    path.join("\u{1}")
}

/// The value at `path` in a config, None where nothing (or null) is.
fn at(rt: &mut Rt, config: V, path: &[String]) -> Option<V> {
    let (c, k) = nightconfig::parent(rt, config, path, false)?;
    let map = nightconfig::values(rt, c);
    let key = rt.string(&k);
    jutil::table_get(rt, map, key).filter(|v| !v.is_null())
}

/// `Config.set(path, v)`: the levels on the way made.
fn put_at(rt: &mut Rt, config: V, path: &[String], v: V) {
    if let Some((c, k)) = nightconfig::parent(rt, config, path, true) {
        let map = nightconfig::values(rt, c);
        let key = rt.string(&k);
        jutil::table_put(rt, map, key, v);
    }
}

/// The values of a config tree, its levels walked.
fn leaves(rt: &Rt, tree: V) -> Vec<V> {
    let map = rt.get_field(tree, "$map");
    let mut out = Vec::new();
    for (_, v) in jutil::table_pairs(rt, map) {
        if nightconfig::is_config(rt, v) {
            out.extend(leaves(rt, v));
        } else {
            out.push(v);
        }
    }
    out
}

/// A `java.util` map the spec keeps (its levels' comments and translation keys), made on first use.
fn level_map(rt: &mut Rt, holder: V, name: &'static str) -> V {
    match rt.get_field(holder, name) {
        m if m.r() != 0 => m,
        _ => {
            let m = rt.alloc("java/util/HashMap", Data::Table(Default::default()));
            rt.put_field(holder, name, m);
            m
        }
    }
}

/// A config tree an object keeps (`$tree`...), an empty one made on first use.
fn tree(rt: &mut Rt, holder: V, name: &'static str) -> V {
    match rt.get_field(holder, name) {
        t if t.r() != 0 => t,
        _ => {
            let t = nightconfig::new_config(rt, CONFIG, None);
            rt.put_field(holder, name, t);
            t
        }
    }
}

fn is_value(rt: &Rt, v: V) -> bool {
    matches!(
        rt.class_of(v),
        CONFIG_VALUE | BOOLEAN_VALUE | INT_VALUE | LONG_VALUE | DOUBLE_VALUE | ENUM_VALUE
    )
}

/// Whether `v` is a `java.util` list.
fn is_list(rt: &Rt, v: V) -> bool {
    v.r() != 0
        && rt.class_of(v) != "java/util/Map$Entry"
        && matches!(
            rt.heap.get(v.r() as usize).map(|o| &o.data),
            Some(Data::List(_))
        )
}

/// The class Java has for `v`: a number held raw is its box's.
fn class_name(rt: &Rt, v: V) -> &'static str {
    match v {
        V::I(_) => "java/lang/Integer",
        V::J(_) => "java/lang/Long",
        V::F(_) => "java/lang/Float",
        V::D(_) => "java/lang/Double",
        _ => rt.class_of(v),
    }
}

/// A `Number`'s value, raw: a box of Java's or a number held raw.
fn number(rt: &Rt, v: V) -> Option<V> {
    match v {
        V::I(_) | V::J(_) | V::F(_) | V::D(_) => Some(v),
        _ if v.r() != 0 && is_number_class(rt.class_of(v)) => Some(rt.get_field(v, "value")),
        _ => None,
    }
}

fn is_number_class(class: &str) -> bool {
    matches!(
        class,
        "java/lang/Integer"
            | "java/lang/Long"
            | "java/lang/Short"
            | "java/lang/Byte"
            | "java/lang/Float"
            | "java/lang/Double"
            | "java/lang/Number"
    )
}

fn as_f64(v: V) -> f64 {
    match v {
        V::I(i) => f64::from(i),
        V::J(j) => j as f64,
        V::F(f) => f64::from(f),
        V::D(d) => d,
        _ => 0.0,
    }
}

/// `Number.intValue()` (a long cut to its low half, a double to an int as Java converts it).
fn int_of(rt: &Rt, v: V) -> i32 {
    match number(rt, v) {
        Some(V::I(i)) => i,
        Some(V::J(j)) => j as i32,
        Some(V::F(f)) => f as i32,
        Some(V::D(d)) => d as i32,
        _ => 0,
    }
}

fn long_of(rt: &Rt, v: V) -> i64 {
    match number(rt, v) {
        Some(V::I(i)) => i64::from(i),
        Some(V::J(j)) => j,
        Some(V::F(f)) => f as i64,
        Some(V::D(d)) => d as i64,
        _ => 0,
    }
}

fn bool_of(rt: &Rt, v: V) -> bool {
    match v {
        V::I(i) => i != 0,
        _ => match rt.str_of(v) {
            Some(t) => t.eq_ignore_ascii_case("true"),
            None => rt.get_field(v, "value").i() != 0,
        },
    }
}

/// What a method of return descriptor `ret` gives when it throws.
fn zero(ret: &str) -> V {
    match ret {
        "I" | "Z" | "B" | "C" | "S" => V::I(0),
        "J" => V::J(0),
        "F" => V::F(0.0),
        "D" => V::D(0.0),
        "V" => V::Void,
        _ => V::NULL,
    }
}

/// `String.isBlank()`: Java's whitespace only.
fn java_blank(s: &str) -> bool {
    s.chars().all(|c| {
        matches!(c, '\t'..='\r' | '\u{1c}'..='\u{1f}')
            || (c.is_whitespace() && !matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}' | '\u{85}'))
    })
}

// --- RestartType ------------------------------------------------------------------------------

fn restart(rt: &mut Rt, name: &'static str) -> V {
    let i = ["NONE", "WORLD", "GAME"]
        .iter()
        .position(|n| *n == name)
        .unwrap_or(0);
    crate::api::api_enum(rt, RESTART, name, i as i32)
}

/// A restart type's name, `NONE` for none.
fn restart_name(rt: &Rt, v: V) -> String {
    text(rt, rt.get_field(v, "$name")).unwrap_or_else(|| "NONE".into())
}

// --- Defaults ---------------------------------------------------------------------------------

/// Where a value's default comes from: the value itself, or a supplier asked each time.
#[derive(Clone, Copy)]
enum DefaultFrom {
    Value(V),
    Supplier(V),
}

fn put_default(rt: &mut Rt, o: V, d: DefaultFrom) {
    let (value, supplier) = match d {
        DefaultFrom::Value(v) => (v, V::NULL),
        DefaultFrom::Supplier(s) => (V::NULL, s),
    };
    rt.put_field(o, "$default", value);
    rt.put_field(o, "$supplier", supplier);
}

/// `getDefault()`: the supplier's value now, or the value given.
fn default_of(rt: &mut Rt, o: V) -> V {
    match field(rt, o, "$supplier") {
        s if s.r() != 0 => rt.call_functional(SUPPLIER, s, &[]),
        _ => field(rt, o, "$default"),
    }
}

fn default_now(rt: &mut Rt, d: DefaultFrom) -> V {
    match d {
        DefaultFrom::Value(v) => v,
        DefaultFrom::Supplier(s) => rt.call_functional(SUPPLIER, s, &[]),
    }
}

// --- Builder ----------------------------------------------------------------------------------

/// The builder's context, made on first use.
fn context(rt: &mut Rt, b: V) -> V {
    match rt.get_field(b, "$ctx") {
        c if c.r() != 0 => c,
        _ => fresh_context(rt, b),
    }
}

/// A new context for the builder's next definition.
fn fresh_context(rt: &mut Rt, b: V) -> V {
    let c = rt.new_object(CONTEXT);
    let comments = jutil::new_list(rt, Vec::new());
    rt.put_field(c, "comment", comments);
    rt.put_field(b, "$ctx", c);
    c
}

fn context_comments(rt: &Rt, ctx: V) -> Vec<String> {
    jutil::elements(rt, rt.get_field(ctx, "comment"))
        .into_iter()
        .map(|l| rt.str_of(l).unwrap_or("").to_owned())
        .collect()
}

fn set_context(rt: &mut Rt, b: V, name: &'static str, v: V) {
    let ctx = context(rt, b);
    rt.put_field(ctx, name, v);
}

fn add_comment(rt: &mut Rt, b: V, line: &str) {
    let ctx = context(rt, b);
    let list = rt.get_field(ctx, "comment");
    let s = rt.string(line);
    jutil::list_add(rt, list, s);
}

/// `BuilderContext.buildComment`: its lines joined; one of blank lines only is NeoForge's warning,
/// as a player's game (production) has it.
fn build_comment(rt: &Rt, ctx: V) -> String {
    let lines = context_comments(rt, ctx);
    if lines.iter().all(|l| java_blank(l)) {
        return BLANK_COMMENT.into();
    }
    lines.join("\n")
}

/// `BuilderContext.ensureEmpty`: false when it threw.
fn ensure_empty(rt: &mut Rt, ctx: V) -> bool {
    let message = if !context_comments(rt, ctx).is_empty() {
        Some("Non-empty comment when empty expected".to_owned())
    } else if !field(rt, ctx, "langKey").is_null() {
        Some("Non-null translation key when null expected".to_owned())
    } else if !field(rt, ctx, "range").is_null() {
        Some("Non-null range when null expected".to_owned())
    } else {
        let r = restart_name(rt, rt.get_field(ctx, "restart"));
        (r != "NONE").then(|| format!("Dangling restart value set to {r}"))
    };
    match message {
        Some(m) => {
            crate::files::throw(rt, "java/lang/IllegalStateException", &m);
            false
        }
        None => true,
    }
}

fn current_path(rt: &Rt, b: V) -> Vec<String> {
    match rt.get_field(b, "$path") {
        p if p.r() != 0 => keys(rt, p),
        _ => Vec::new(),
    }
}

fn set_current_path(rt: &mut Rt, b: V, path: &[String]) {
    let p = key_list(rt, path);
    rt.put_field(b, "$path", p);
}

fn builder_values(rt: &mut Rt, b: V) -> V {
    match rt.get_field(b, "$values") {
        l if l.r() != 0 => l,
        _ => {
            let l = jutil::new_list(rt, Vec::new());
            rt.put_field(b, "$values", l);
            l
        }
    }
}

fn builder_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or(V::NULL);
    let b = a(0);
    Some(match name {
        "<init>" => V::Void,
        "comment" => {
            let varargs = desc.starts_with("([");
            let lines = if varargs {
                jutil::elements(rt, a(1))
            } else {
                vec![a(1)]
            };
            if let Some(i) = lines.iter().position(|l| l.is_null()) {
                let message = if varargs {
                    format!("Comment string at {i} is null.")
                } else {
                    "Passed in null value for comment".to_owned()
                };
                return Some(npe(rt, &message));
            }
            let ctx = context(rt, b);
            let list = rt.get_field(ctx, "comment");
            for l in lines {
                jutil::list_add(rt, list, l);
            }
            b
        }
        "translation" => {
            set_context(rt, b, "langKey", a(1));
            b
        }
        "worldRestart" | "gameRestart" => {
            let r = restart(
                rt,
                if name == "worldRestart" {
                    "WORLD"
                } else {
                    "GAME"
                },
            );
            set_context(rt, b, "restart", r);
            b
        }
        "push" => {
            let rel = keys(rt, a(1));
            if !push(rt, b, rel) {
                return Some(V::NULL);
            }
            b
        }
        "pop" => {
            let count = if desc.starts_with("(I") { a(1).i() } else { 1 };
            let mut path = current_path(rt, b);
            if count > 0 && count as usize > path.len() {
                let message = format!(
                    "Attempted to pop {count} elements when we only had: [{}]",
                    path.join(", ")
                );
                return Some(crate::files::throw(
                    rt,
                    "java/lang/IllegalArgumentException",
                    &message,
                ));
            }
            path.truncate(path.len() - count.max(0) as usize);
            set_current_path(rt, b, &path);
            b
        }
        "configure" => {
            let made = rt.call_functional(
                "java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;",
                a(1),
                &[b],
            );
            if rt.thrown.is_some() {
                return Some(V::NULL);
            }
            let spec = build(rt, b);
            let pair = rt.new_object("org/apache/commons/lang3/tuple/Pair");
            rt.put_field(pair, "$left", made);
            rt.put_field(pair, "$right", spec);
            pair
        }
        "build" => build(rt, b),
        n if n.starts_with("define") => match define_call(rt, b, n, desc, args) {
            Some(v) => v,
            None if rt.thrown.is_some() => V::NULL,
            None => return None,
        },
        _ => return None,
    })
}

/// `Builder.push`: the path goes deeper; the context's comment and translation key become the
/// level's. False when it threw (a range or restart type left over).
fn push(rt: &mut Rt, b: V, rel: Vec<String>) -> bool {
    let mut path = current_path(rt, b);
    path.extend(rel);
    set_current_path(rt, b, &path);
    let ctx = context(rt, b);
    let key = level_key(&path);
    if !context_comments(rt, ctx).is_empty() {
        let comment = build_comment(rt, ctx);
        let (k, c) = (rt.string(&key), rt.string(&comment));
        let map = level_map(rt, b, "$levelComments");
        jutil::table_put(rt, map, k, c);
        let cleared = jutil::new_list(rt, Vec::new());
        rt.put_field(ctx, "comment", cleared);
    }
    let lang = field(rt, ctx, "langKey");
    if !lang.is_null() {
        let k = rt.string(&key);
        let map = level_map(rt, b, "$levelKeys");
        jutil::table_put(rt, map, k, lang);
        rt.put_field(ctx, "langKey", V::NULL);
    }
    ensure_empty(rt, ctx)
}

/// `Builder.build`: the spec of every value defined, each value then reading through it.
fn build(rt: &mut Rt, b: V) -> V {
    let ctx = context(rt, b);
    if !ensure_empty(rt, ctx) {
        return V::NULL;
    }
    let spec = rt.new_object(SPEC);
    let specs = tree(rt, b, "$tree");
    let values = nightconfig::new_config(rt, CONFIG, None);
    let list = builder_values(rt, b);
    for v in jutil::elements(rt, list) {
        let path = keys(rt, rt.get_field(v, "$path"));
        put_at(rt, values, &path, v);
        rt.put_field(v, "$owner", spec);
    }
    let (comments, translations) = (
        level_map(rt, b, "$levelComments"),
        level_map(rt, b, "$levelKeys"),
    );
    for (name, v) in [
        ("$specTree", specs),
        ("$valueTree", values),
        ("$levelComments", comments),
        ("$levelKeys", translations),
    ] {
        rt.put_field(spec, name, v);
    }
    spec
}

/// What a value's validator accepts (`ValueSpec.test`).
enum Check {
    /// `define(path, default)`: an object of the default's class.
    Class(String),
    Predicate(V),
    /// The context's range.
    Range,
    /// `defineInList`: one of the collection's.
    In(V),
    /// A list whose every element the predicate accepts.
    List(V),
    /// `true`, `false` (as a string too, either case).
    Bool,
    /// One of the acceptable constants, as such or by name (or ordinal) as the converter reads it.
    Enum {
        acceptable: V,
        converter: V,
        class: String,
    },
}

fn put_check(rt: &mut Rt, s: V, check: &Check) {
    let (tag, with, of) = match check {
        Check::Class(c) => ("class", V::NULL, Some(c.as_str())),
        Check::Predicate(p) => ("predicate", *p, None),
        Check::Range => ("range", V::NULL, None),
        Check::In(c) => ("in", *c, None),
        Check::List(p) => ("list", *p, None),
        Check::Bool => ("bool", V::NULL, None),
        Check::Enum {
            acceptable,
            converter,
            class,
        } => {
            rt.put_field(s, "$converter", *converter);
            ("enum", *acceptable, Some(class.as_str()))
        }
    };
    let tag = rt.string(tag);
    rt.put_field(s, "$check", tag);
    rt.put_field(s, "$with", with);
    if let Some(of) = of {
        let of = rt.string(of);
        rt.put_field(s, "$of", of);
    }
}

/// A `ValueSpec` (a `ListValueSpec` with the list's new-element supplier and size range) of the
/// builder's context; None when it threw (no supplier or validator).
fn value_spec(
    rt: &mut Rt,
    b: V,
    default: DefaultFrom,
    check: Check,
    list: Option<(V, V)>,
) -> Option<V> {
    if let DefaultFrom::Supplier(s) = default
        && s.is_null()
    {
        npe(rt, "Default supplier can not be null");
        return None;
    }
    if matches!(&check, Check::Predicate(p) | Check::In(p) | Check::List(p) if p.is_null()) {
        npe(rt, "Validator can not be null");
        return None;
    }
    let s = rt.new_object(if list.is_some() {
        LIST_SPEC
    } else {
        VALUE_SPEC
    });
    put_check(rt, s, &check);
    if let Some((new_element, size)) = list {
        rt.put_field(s, "$newElement", new_element);
        rt.put_field(s, "$sizeRange", size);
    }
    put_default(rt, s, default);
    from_context(rt, b, s);
    Some(s)
}

/// The spec's comment, translation key, range, class and restart type, from the context.
fn from_context(rt: &mut Rt, b: V, s: V) {
    let ctx = context(rt, b);
    let comment = if context_comments(rt, ctx).is_empty() {
        V::NULL
    } else {
        let c = build_comment(rt, ctx);
        rt.string(&c)
    };
    rt.put_field(s, "$comment", comment);
    for (to, from) in [
        ("$langKey", "langKey"),
        ("$range", "range"),
        ("$clazz", "clazz"),
    ] {
        let v = field(rt, ctx, from);
        rt.put_field(s, to, v);
    }
    let r = match field(rt, ctx, "restart") {
        r if !r.is_null() => r,
        _ => restart(rt, "NONE"),
    };
    rt.put_field(s, "$restart", r);
}

/// `Builder.define(path, spec, defaultSupplier)`, where each definition ends: the spec at the
/// builder's path plus `rel`, a fresh context, and a new value of `class` there.
fn define(
    rt: &mut Rt,
    b: V,
    rel: Vec<String>,
    spec: V,
    default: DefaultFrom,
    class: &'static str,
) -> V {
    let mut path = current_path(rt, b);
    path.extend(rel);
    let specs = tree(rt, b, "$tree");
    put_at(rt, specs, &path, spec);
    fresh_context(rt, b);
    let v = rt.new_object(class);
    let p = key_list(rt, &path);
    rt.put_field(v, "$path", p);
    rt.put_field(v, "$builder", b);
    put_default(rt, v, default);
    let values = builder_values(rt, b);
    jutil::list_add(rt, values, v);
    v
}

/// A value spec of the context, defined: None when it threw.
fn checked(
    rt: &mut Rt,
    b: V,
    rel: Vec<String>,
    default: DefaultFrom,
    check: Check,
    list: Option<(V, V)>,
    class: &'static str,
) -> Option<V> {
    let s = value_spec(rt, b, default, check, list)?;
    Some(define(rt, b, rel, s, default, class))
}

/// The `define*` methods, by their descriptors: which value, default, validator and range each
/// gives, as NeoForge's overloads delegate.
fn define_call(rt: &mut Rt, b: V, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or(V::NULL);
    let params = crate::neo::desc_params(desc);
    let p = |i: usize| params.get(i).map_or("", String::as_str);
    let ret = desc.rsplit(')').next().unwrap_or("");
    let class: &'static str = [
        BOOLEAN_VALUE,
        INT_VALUE,
        LONG_VALUE,
        DOUBLE_VALUE,
        ENUM_VALUE,
    ]
    .into_iter()
    .find(|c| ret.strip_prefix('L').and_then(|r| r.strip_suffix(';')) == Some(*c))
    .unwrap_or(CONFIG_VALUE);
    let rel = keys(rt, a(1));
    let given = if p(1) == SUPPLIER_DESC {
        DefaultFrom::Supplier(a(2))
    } else {
        DefaultFrom::Value(a(2))
    };
    match name {
        "define" if p(1) == "Lnet/neoforged/neoforge/common/ModConfigSpec$ValueSpec;" => Some(
            define(rt, b, rel, a(2), DefaultFrom::Supplier(a(3)), CONFIG_VALUE),
        ),
        "define" if class == BOOLEAN_VALUE => {
            let default = match p(1) {
                "Z" => DefaultFrom::Value(crate::api_common::boolean(rt, a(2).i() != 0)),
                _ => given,
            };
            let clazz = rt.class_literal("java/lang/Boolean");
            set_context(rt, b, "clazz", clazz);
            checked(rt, b, rel, default, Check::Bool, None, class)
        }
        "define" => {
            if matches!(given, DefaultFrom::Value(v) if v.is_null()) {
                npe(rt, "Default value can not be null");
                return None;
            }
            let check = match params.len() {
                2 => Check::Class(class_name(rt, a(2)).to_owned()),
                _ => Check::Predicate(a(3)),
            };
            let clazz = if params.len() == 4 {
                a(4)
            } else {
                rt.class_literal("java/lang/Object")
            };
            set_context(rt, b, "clazz", clazz);
            checked(rt, b, rel, given, check, None, CONFIG_VALUE)
        }
        "defineInRange" => {
            let kind = match class {
                INT_VALUE => Some(('I', "java/lang/Integer")),
                LONG_VALUE => Some(('J', "java/lang/Long")),
                DOUBLE_VALUE => Some(('D', "java/lang/Double")),
                _ => None,
            };
            let (default, min, max, clazz) = match kind {
                Some((k, boxed)) => {
                    let default = match given {
                        DefaultFrom::Value(v) => {
                            DefaultFrom::Value(crate::mixin::box_value(rt, v, k))
                        }
                        supplied => supplied,
                    };
                    let (min, max) = (
                        crate::mixin::box_value(rt, a(3), k),
                        crate::mixin::box_value(rt, a(4), k),
                    );
                    (default, min, max, rt.class_literal(boxed))
                }
                None => (given, a(3), a(4), a(5)),
            };
            let range = new_range(rt, clazz, min, max)?;
            set_context(rt, b, "range", range);
            // The range keeps its class; the value's spec is of `Object`, as NeoForge's range ends
            // in `define(path, supplier, validator)`.
            let object = rt.class_literal("java/lang/Object");
            set_context(rt, b, "clazz", object);
            let shown = default_now(rt, default);
            let shown = format!(" Default: {}", rt.to_java_string(shown, 'L'));
            add_comment(rt, b, &shown);
            let bounds = format!(" Range: {}", range_text(rt, range));
            add_comment(rt, b, &bounds);
            checked(rt, b, rel, default, Check::Range, None, class)
        }
        "defineInList" => {
            let clazz = rt.class_literal("java/lang/Object");
            set_context(rt, b, "clazz", clazz);
            checked(rt, b, rel, given, Check::In(a(3)), None, CONFIG_VALUE)
        }
        "defineList" | "defineListAllowEmpty" => {
            let (new_element, validator, size) = match params.len() {
                3 => (V::NULL, a(3), None),
                4 => (a(3), a(4), None),
                _ => (a(3), a(4), Some(a(5))),
            };
            let size = match size {
                Some(r) if !r.is_null() => r,
                None if name == "defineList" => int_range(rt, 1, i32::MAX),
                _ => int_range(rt, 0, i32::MAX),
            };
            let clazz = rt.class_literal("java/util/List");
            set_context(rt, b, "clazz", clazz);
            checked(
                rt,
                b,
                rel,
                given,
                Check::List(validator),
                Some((new_element, size)),
                CONFIG_VALUE,
            )
        }
        "defineEnum" => define_enum(rt, b, rel, &params, args),
        _ => None,
    }
}

/// `defineEnum`'s overloads: the default (or its supplier), the converter names are read with
/// (`NAME_IGNORECASE` unless given), the constants it accepts (all, those given, or a predicate's),
/// and the enum's class; the comment names the constants accepted.
fn define_enum(rt: &mut Rt, b: V, rel: Vec<String>, params: &[String], args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or(V::NULL);
    let p = |i: usize| params.get(i).map_or("", String::as_str);
    let default = if p(1) == SUPPLIER_DESC {
        DefaultFrom::Supplier(a(2))
    } else {
        DefaultFrom::Value(a(2))
    };
    let mut i = 2;
    let mut converter = V::NULL;
    if p(i) == "Lcom/electronwill/nightconfig/core/EnumGetMethod;" {
        converter = a(i + 1);
        i += 1;
    }
    let (mut acceptable, mut predicate) = (None, None);
    match p(i) {
        "[Ljava/lang/Enum;" => {
            let items = jutil::elements(rt, a(i + 1));
            acceptable = Some(jutil::new_list(rt, items));
            i += 1;
        }
        "Ljava/util/Collection;" => {
            acceptable = Some(a(i + 1));
            i += 1;
        }
        "Ljava/util/function/Predicate;" => {
            predicate = Some(a(i + 1));
            i += 1;
        }
        _ => {}
    }
    let class = match (p(i), default) {
        ("Ljava/lang/Class;", _) => rt.str_of(a(i + 1)).unwrap_or("").replace('.', "/"),
        (_, DefaultFrom::Value(v)) => declaring_class(rt, v),
        _ => String::new(),
    };
    if converter.is_null() {
        converter = crate::api::get_static(
            rt,
            "com/electronwill/nightconfig/core/EnumGetMethod.NAME_IGNORECASE",
        );
    }
    let constants = crate::jlang::enum_constants(rt, &class);
    let constants = constants
        .map(|c| jutil::elements(rt, c))
        .unwrap_or_default();
    let check = match predicate {
        Some(p) => Check::Predicate(p),
        None => {
            let acceptable = match acceptable {
                Some(a) => a,
                None => jutil::new_list(rt, constants.clone()),
            };
            Check::Enum {
                acceptable,
                converter,
                class: class.clone(),
            }
        }
    };
    let clazz = rt.class_literal(&class);
    set_context(rt, b, "clazz", clazz);
    // The spec first, so its comment can name the constants it accepts.
    let s = value_spec(rt, b, default, check, None)?;
    let mut names = Vec::new();
    for c in constants {
        if passes(rt, s, c) {
            let n = crate::api_common::enum_name(rt, c, false);
            names.push(text(rt, n).unwrap_or_default());
        }
    }
    add_comment(rt, b, &format!("Allowed Values: {}", names.join(", ")));
    from_context(rt, b, s);
    let v = define(rt, b, rel, s, default, ENUM_VALUE);
    let class = rt.string(&class);
    rt.put_field(v, "$converter", converter);
    rt.put_field(v, "$enumClass", class);
    Some(v)
}

/// `Enum.getDeclaringClass()`: a constant with a body of its own is of a subclass of the enum's.
fn declaring_class(rt: &mut Rt, v: V) -> String {
    let c = rt.class_of(v);
    if crate::jlang::enum_constants(rt, c).is_some() {
        return c.to_owned();
    }
    match rt.supers.get(c) {
        Some(&s) if s != "java/lang/Enum" => s.to_owned(),
        _ => c.to_owned(),
    }
}

// --- Validation -------------------------------------------------------------------------------

/// `Predicate.test`: an exception it throws refuses the value.
fn test(rt: &mut Rt, predicate: V, v: V) -> bool {
    let r = rt.call_functional(PREDICATE, predicate, &[v]);
    if rt.thrown.take().is_some() {
        return false;
    }
    crate::mixin::unbox(rt, r, 'Z').i() != 0
}

/// `ValueSpec.test(value)`.
fn passes(rt: &mut Rt, s: V, value: V) -> bool {
    let tag = text(rt, field(rt, s, "$check")).unwrap_or_default();
    let with = field(rt, s, "$with");
    match tag.as_str() {
        "class" => {
            let want = text(rt, field(rt, s, "$of")).unwrap_or_default();
            !value.is_null() && rt.is_a(class_name(rt, value), &want)
        }
        "predicate" => test(rt, with, value),
        "range" => {
            let r = field(rt, s, "$range");
            range_test(rt, r, value)
        }
        "in" => {
            crate::api::call(
                rt,
                "java/util/Collection.contains.(Ljava/lang/Object;)Z",
                &[with, value],
            )
            .i() != 0
        }
        "list" => {
            is_list(rt, value)
                && jutil::elements(rt, value)
                    .into_iter()
                    .all(|e| test(rt, with, e))
        }
        "bool" => match rt.str_of(value) {
            Some(t) => t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("false"),
            None => class_name(rt, value) == "java/lang/Boolean",
        },
        "enum" => {
            if value.is_null() {
                return false;
            }
            let constant = if rt.str_of(value).is_none()
                && !crate::api_common::enum_name(rt, value, false).is_null()
            {
                Some(value)
            } else {
                let class = text(rt, field(rt, s, "$of")).unwrap_or_default();
                let converter = field(rt, s, "$converter");
                nightconfig::enum_of(rt, value, &class, converter)
            };
            constant.is_some_and(|c| jutil::elements(rt, with).contains(&c))
        }
        _ => true,
    }
}

/// `ValueSpec.correct(value)`: the range's nearest bound, else the default; a list (`defineList`'s
/// spec) keeps the elements its validator accepts when it is of a size its range allows and any
/// are left.
fn correct_value(rt: &mut Rt, s: V, value: V) -> V {
    if rt.class_of(s) == LIST_SPEC {
        let size = field(rt, s, "$sizeRange");
        let fits = is_list(rt, value)
            && (size.is_null()
                || range_test(rt, size, V::I(jutil::elements(rt, value).len() as i32)));
        if !fits {
            return default_of(rt, s);
        }
        let with = field(rt, s, "$with");
        let mut kept = Vec::new();
        for e in jutil::elements(rt, value) {
            if test(rt, with, e) {
                kept.push(e);
            }
        }
        if kept.is_empty() {
            return default_of(rt, s);
        }
        return jutil::new_list(rt, kept);
    }
    let default = default_of(rt, s);
    match field(rt, s, "$range") {
        r if !r.is_null() => range_correct(rt, r, value, default),
        _ => default,
    }
}

fn value_spec_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or(V::NULL);
    let this = a(0);
    Some(match name {
        "getComment" => field(rt, this, "$comment"),
        "getTranslationKey" => field(rt, this, "$langKey"),
        "getRange" => field(rt, this, "$range"),
        "restartType" => field(rt, this, "$restart"),
        "getClazz" => field(rt, this, "$clazz"),
        "test" => V::I(passes(rt, this, a(1)) as i32),
        "correct" => correct_value(rt, this, a(1)),
        "getDefault" => default_of(rt, this),
        "getNewElementSupplier" => field(rt, this, "$newElement"),
        "testElement" => {
            let with = field(rt, this, "$with");
            V::I(test(rt, with, a(1)) as i32)
        }
        "getSizeRange" => field(rt, this, "$sizeRange"),
        _ => return None,
    })
}

// --- Range ------------------------------------------------------------------------------------

/// A `Range` of the class `clazz` names; None when it threw (`min` above `max`).
fn new_range(rt: &mut Rt, clazz: V, min: V, max: V) -> Option<V> {
    if compare(rt, min, max) > 0 {
        crate::files::throw(
            rt,
            "java/lang/IllegalArgumentException",
            "Range min must be less then max.",
        );
        return None;
    }
    let r = rt.new_object(RANGE);
    for (name, v) in [("$clazz", clazz), ("$min", min), ("$max", max)] {
        rt.put_field(r, name, v);
    }
    Some(r)
}

/// `Range.of(min, max)`: of `Integer`s.
fn int_range(rt: &mut Rt, min: i32, max: i32) -> V {
    let clazz = rt.class_literal("java/lang/Integer");
    let (min, max) = (
        crate::mixin::box_value(rt, V::I(min), 'I'),
        crate::mixin::box_value(rt, V::I(max), 'I'),
    );
    new_range(rt, clazz, min, max).unwrap_or(V::NULL)
}

fn range_class(rt: &Rt, r: V) -> String {
    rt.str_of(rt.get_field(r, "$clazz"))
        .unwrap_or("")
        .replace('.', "/")
}

/// `compareTo`'s sign: numbers by value, strings as Java orders them, else the object's own.
fn compare(rt: &mut Rt, a: V, b: V) -> i32 {
    match (number(rt, a), number(rt, b)) {
        (Some(x @ (V::I(_) | V::J(_))), Some(y @ (V::I(_) | V::J(_)))) => {
            let (x, y) = (long_of(rt, x), long_of(rt, y));
            x.cmp(&y) as i32
        }
        (Some(x), Some(y)) => as_f64(x).total_cmp(&as_f64(y)) as i32,
        _ => match (rt.str_of(a), rt.str_of(b)) {
            (Some(x), Some(y)) => x.encode_utf16().cmp(y.encode_utf16()) as i32,
            _ => rt
                .call_functional(
                    "java/lang/Comparable.compareTo.(Ljava/lang/Object;)I",
                    a,
                    &[b],
                )
                .i(),
        },
    }
}

fn instance_of(rt: &Rt, v: V, class: &str) -> bool {
    !v.is_null() && rt.is_a(class_name(rt, v), class)
}

/// `Range.test`: a number of a number range between the bounds as doubles; else an instance of
/// the range's class between them.
fn range_test(rt: &mut Rt, r: V, v: V) -> bool {
    let class = range_class(rt, r);
    let (min, max) = (field(rt, r, "$min"), field(rt, r, "$max"));
    if is_number_class(&class)
        && let Some(n) = number(rt, v)
    {
        let (n, lo, hi) = (
            as_f64(n),
            number(rt, min).map_or(f64::NAN, as_f64),
            number(rt, max).map_or(f64::NAN, as_f64),
        );
        return lo <= n && n <= hi;
    }
    instance_of(rt, v, &class) && compare(rt, v, min) >= 0 && compare(rt, v, max) <= 0
}

/// `Range.correct(value, default)`: the nearest bound for a value outside, the default for one of
/// another class.
fn range_correct(rt: &mut Rt, r: V, v: V, default: V) -> V {
    let class = range_class(rt, r);
    let (min, max) = (field(rt, r, "$min"), field(rt, r, "$max"));
    if is_number_class(&class)
        && let Some(n) = number(rt, v)
    {
        let n = as_f64(n);
        return if n < number(rt, min).map_or(f64::NAN, as_f64) {
            min
        } else if n > number(rt, max).map_or(f64::NAN, as_f64) {
            max
        } else {
            v
        };
    }
    if !instance_of(rt, v, &class) {
        return default;
    }
    if compare(rt, v, min) < 0 {
        min
    } else if compare(rt, v, max) > 0 {
        max
    } else {
        v
    }
}

/// `Range.toString()`: `> min` for an int range up to `Integer.MAX_VALUE`, `< max` for one from
/// `Integer.MIN_VALUE`, else `min ~ max`.
fn range_text(rt: &Rt, r: V) -> String {
    let (min, max) = (rt.get_field(r, "$min"), rt.get_field(r, "$max"));
    if range_class(rt, r) == "java/lang/Integer" {
        if number(rt, max) == Some(V::I(i32::MAX)) {
            return format!("> {}", rt.to_java_string(min, 'L'));
        }
        if number(rt, min) == Some(V::I(i32::MIN)) {
            return format!("< {}", rt.to_java_string(max, 'L'));
        }
    }
    format!(
        "{} ~ {}",
        rt.to_java_string(min, 'L'),
        rt.to_java_string(max, 'L')
    )
}

fn range_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or(V::NULL);
    let this = a(0);
    Some(match name {
        "of" => int_range(rt, a(0).i(), a(1).i()),
        "getClazz" => field(rt, this, "$clazz"),
        "getMin" => field(rt, this, "$min"),
        "getMax" => field(rt, this, "$max"),
        "test" => V::I(range_test(rt, this, a(1)) as i32),
        "correct" => range_correct(rt, this, a(1), a(2)),
        "toString" => {
            let t = range_text(rt, this);
            rt.string(&t)
        }
        _ => return None,
    })
}

// --- Config values ----------------------------------------------------------------------------

/// The value's spec (`ConfigValue.getSpec()`): its builder's at its path.
fn value_spec_of(rt: &mut Rt, cv: V) -> V {
    let b = field(rt, cv, "$builder");
    if b.is_null() {
        return V::NULL;
    }
    let specs = tree(rt, b, "$tree");
    let path = keys(rt, rt.get_field(cv, "$path"));
    at(rt, specs, &path).unwrap_or(V::NULL)
}

/// The value as the loaded config holds it at `path`, read as the value's class reads it
/// (`getIntOrElse`, `getEnumOrElse`...); None where there is none it can read.
fn read_typed(rt: &mut Rt, cv: V, config: V, path: &[String]) -> Option<V> {
    let found = at(rt, config, path)?;
    match rt.class_of(cv) {
        INT_VALUE => {
            number(rt, found).map(|_| crate::mixin::box_value(rt, V::I(int_of(rt, found)), 'I'))
        }
        LONG_VALUE => {
            number(rt, found).map(|_| crate::mixin::box_value(rt, V::J(long_of(rt, found)), 'J'))
        }
        DOUBLE_VALUE => {
            number(rt, found).map(|n| crate::mixin::box_value(rt, V::D(as_f64(n)), 'D'))
        }
        ENUM_VALUE => {
            let class = text(rt, field(rt, cv, "$enumClass")).unwrap_or_default();
            let converter = field(rt, cv, "$converter");
            nightconfig::enum_of(rt, found, &class, converter)
        }
        _ => Some(found),
    }
}

/// `ConfigValue.getRaw()`: None when it threw (no spec built, or its config not loaded yet).
fn raw(rt: &mut Rt, cv: V) -> Option<V> {
    let spec = field(rt, cv, "$owner");
    if spec.is_null() {
        npe(rt, "Cannot get config value before spec is built");
        return None;
    }
    let loaded = field(rt, spec, "$loaded");
    if loaded.is_null() {
        crate::files::throw(
            rt,
            "java/lang/IllegalStateException",
            "Cannot get config value before config is loaded.",
        );
        return None;
    }
    let config = field(rt, loaded, "$config");
    let path = keys(rt, rt.get_field(cv, "$path"));
    Some(match read_typed(rt, cv, config, &path) {
        Some(v) => v,
        None => default_of(rt, cv),
    })
}

/// `ConfigValue.get()`: cached until a reload clears it.
fn get(rt: &mut Rt, cv: V) -> V {
    let cached = field(rt, cv, "$cached");
    if !cached.is_null() {
        return cached;
    }
    match raw(rt, cv) {
        Some(v) => {
            rt.put_field(cv, "$cached", v);
            v
        }
        None => V::NULL,
    }
}

/// The value's spec and its loaded config, or the exception Java's `Preconditions` throws (`what`:
/// "set", "save").
fn loaded_of(rt: &mut Rt, cv: V, what: &str) -> Option<(V, V)> {
    let spec = field(rt, cv, "$owner");
    if spec.is_null() {
        npe(
            rt,
            &format!("Cannot {what} config value before spec is built"),
        );
        return None;
    }
    let loaded = field(rt, spec, "$loaded");
    if loaded.is_null() {
        npe(
            rt,
            &format!("Cannot {what} config value without assigned Config object present"),
        );
        return None;
    }
    Some((spec, loaded))
}

fn value_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or(V::NULL);
    let this = a(0);
    let ret = desc.rsplit(')').next().unwrap_or("V");
    Some(match name {
        "get" | "getAsInt" | "getAsLong" | "getAsDouble" | "getAsBoolean" | "isTrue"
        | "isFalse" => {
            let v = get(rt, this);
            if rt.thrown.is_some() {
                return Some(zero(ret));
            }
            match name {
                "getAsInt" => V::I(int_of(rt, v)),
                "getAsLong" => V::J(long_of(rt, v)),
                "getAsDouble" => V::D(number(rt, v).map_or(0.0, as_f64)),
                "getAsBoolean" | "isTrue" => V::I(bool_of(rt, v) as i32),
                "isFalse" => V::I(!bool_of(rt, v) as i32),
                _ => v,
            }
        }
        // `getRaw(config, path, defaultSupplier)`: what the value reads from another config.
        "getRaw" if args.len() == 4 => {
            let path = keys(rt, a(2));
            match read_typed(rt, this, a(1), &path) {
                Some(v) => v,
                None => rt.call_functional(SUPPLIER, a(3), &[]),
            }
        }
        "getRaw" => raw(rt, this).unwrap_or(V::NULL),
        "getDefault" => default_of(rt, this),
        "getPath" => {
            let path = keys(rt, rt.get_field(this, "$path"));
            key_list(rt, &path)
        }
        "next" => field(rt, this, "$builder"),
        "getSpec" => value_spec_of(rt, this),
        "clearCache" => {
            rt.put_field(this, "$cached", V::NULL);
            V::Void
        }
        // Into the loaded config; read at once unless the value waits for a restart.
        "set" => {
            if let Some((_, loaded)) = loaded_of(rt, this, "set") {
                let config = field(rt, loaded, "$config");
                let path = keys(rt, rt.get_field(this, "$path"));
                put_at(rt, config, &path, a(1));
                let s = value_spec_of(rt, this);
                if restart_name(rt, field(rt, s, "$restart")) == "NONE" {
                    rt.put_field(this, "$cached", a(1));
                }
            }
            V::Void
        }
        "save" => {
            if let Some((_, loaded)) = loaded_of(rt, this, "save") {
                save_loaded(rt, loaded);
            }
            V::Void
        }
        _ => return None,
    })
}

// --- The spec ---------------------------------------------------------------------------------

/// How a correction goes: the spec's level comments, who hears each change, whether it only looks.
struct Fix {
    levels: V,
    listener: V,
    comments: V,
    dry: bool,
}

/// `ModConfigSpec.correct`: the spec's levels and values walked over `config`, putting in a level
/// that is missing or isn't one, the correction of a value the validator refuses, the spec's
/// comment where the config's differs, and removing what the spec doesn't define; each change told
/// to the listener (comments to their own), as Night Config's listeners hear them. A dry run stops
/// at the first difference (`isCorrect`). The number of changes.
fn correct(rt: &mut Rt, spec: V, config: V, listener: V, comments: V, dry: bool) -> i32 {
    let specs = tree(rt, spec, "$specTree");
    let levels = level_map(rt, spec, "$levelComments");
    let fix = Fix {
        levels,
        listener,
        comments,
        dry,
    };
    correct_level(rt, &fix, specs, config, &mut Vec::new())
}

fn correct_level(rt: &mut Rt, fix: &Fix, specs: V, config: V, path: &mut Vec<String>) -> i32 {
    let mut count = 0;
    let spec_map = nightconfig::values(rt, specs);
    let config_map = nightconfig::values(rt, config);
    for (k, spec) in jutil::table_pairs(rt, spec_map) {
        let key = text(rt, k).unwrap_or_default();
        let found = jutil::table_get(rt, config_map, k).filter(|v| !v.is_null());
        let action = if found.is_none() { "ADD" } else { "REPLACE" };
        path.push(key.clone());
        let wanted = if nightconfig::is_config(rt, spec) {
            match found {
                Some(sub) if nightconfig::is_config(rt, sub) => {
                    count += correct_level(rt, fix, spec, sub, path);
                    if count > 0 && fix.dry {
                        return count;
                    }
                }
                _ if fix.dry => return 1,
                _ => {
                    let sub = nightconfig::new_config(rt, COMMENTED, None);
                    jutil::table_put(rt, config_map, k, sub);
                    tell(
                        rt,
                        fix.listener,
                        action,
                        path,
                        found.unwrap_or(V::NULL),
                        sub,
                    );
                    count += 1 + correct_level(rt, fix, spec, sub, path);
                }
            }
            let level = rt.string(&level_key(path));
            jutil::table_get(rt, fix.levels, level).unwrap_or(V::NULL)
        } else {
            let value = found.unwrap_or(V::NULL);
            if !passes(rt, spec, value) {
                if fix.dry {
                    return 1;
                }
                let fixed = correct_value(rt, spec, value);
                jutil::table_put(rt, config_map, k, fixed);
                tell(rt, fix.listener, action, path, value, fixed);
                count += 1;
            }
            field(rt, spec, "$comment")
        };
        // The comment, as `stringsMatchNormalizingNewLines` compares them.
        let comments = nightconfig::comments(rt, config);
        let old = jutil::table_get(rt, comments, k).unwrap_or(V::NULL);
        if !same_comment(rt.str_of(old), rt.str_of(wanted)) {
            if !fix.comments.is_null() {
                tell(rt, fix.comments, action, path, old, wanted);
            }
            if fix.dry {
                return 1;
            }
            if wanted.is_null() {
                jutil::table_remove(rt, comments, k);
            } else {
                jutil::table_put(rt, comments, k, wanted);
            }
        }
        path.pop();
    }
    for (k, v) in jutil::table_pairs(rt, config_map) {
        if jutil::table_get(rt, spec_map, k).is_none() {
            if fix.dry {
                return 1;
            }
            jutil::table_remove(rt, config_map, k);
            path.push(text(rt, k).unwrap_or_default());
            tell(rt, fix.listener, "REMOVE", path, v, V::NULL);
            path.pop();
            count += 1;
        }
    }
    count
}

fn same_comment(a: Option<&str>, b: Option<&str>) -> bool {
    let blank = |s: Option<&str>| s.is_none_or(java_blank);
    match (blank(a), blank(b)) {
        (true, true) => true,
        (false, false) => {
            a.unwrap_or("").replace("\r\n", "\n") == b.unwrap_or("").replace("\r\n", "\n")
        }
        _ => false,
    }
}

/// `CorrectionListener.onCorrect(action, path, before, after)`.
fn tell(rt: &mut Rt, listener: V, action: &'static str, path: &[String], before: V, after: V) {
    if listener.is_null() {
        return;
    }
    let i = ["ADD", "REPLACE", "REMOVE"]
        .iter()
        .position(|a| *a == action)
        .unwrap_or(0);
    let action = crate::api::api_enum(rt, ACTION, action, i as i32);
    let path = key_list(rt, path);
    rt.call_functional(LISTENER, listener, &[action, path, before, after]);
}

/// `resetCaches(restartType)`: the values of that restart type read their config again.
fn reset_caches(rt: &mut Rt, spec: V, restart: &str) {
    let values = tree(rt, spec, "$valueTree");
    for v in leaves(rt, values) {
        if is_value(rt, v) {
            let s = value_spec_of(rt, v);
            if restart_name(rt, field(rt, s, "$restart")) == restart {
                rt.put_field(v, "$cached", V::NULL);
            }
        }
    }
}

/// `validateSpec`: a server's config can't hold a value that waits for the game to restart. False
/// when it threw.
fn validate(rt: &mut Rt, spec: V, config: V) -> bool {
    let ty = text(rt, rt.get_field(field(rt, config, "$type"), "$name")).unwrap_or_default();
    let values = tree(rt, spec, "$valueTree");
    for v in leaves(rt, values) {
        if !is_value(rt, v) {
            continue;
        }
        let s = value_spec_of(rt, v);
        if ty == "SERVER" && restart_name(rt, field(rt, s, "$restart")) == "GAME" {
            let path = keys(rt, rt.get_field(v, "$path")).join(".");
            let file = text(rt, field(rt, config, "$fileName")).unwrap_or_default();
            let message = format!(
                "Configuration value {path} defined in config {file} has restart of type GAME which cannot be used for configs of type SERVER"
            );
            crate::files::throw(rt, "java/lang/IllegalArgumentException", &message);
            return false;
        }
    }
    true
}

/// `acceptConfig(loaded)`: the spec reads from it (correcting and saving it if it isn't right yet),
/// and the values that don't wait for a restart read it again. A mod's own `IConfigSpec` takes it
/// itself.
fn accept(rt: &mut Rt, spec: V, loaded: V) {
    if spec.is_null() {
        return;
    }
    if rt.class_of(spec) != SPEC {
        rt.call_functional(ACCEPT, spec, &[loaded]);
        rt.report_thrown("IConfigSpec.acceptConfig");
        return;
    }
    rt.put_field(spec, "$loaded", loaded);
    if !loaded.is_null() {
        let config = field(rt, loaded, "$config");
        if correct(rt, spec, config, V::NULL, V::NULL, true) != 0 {
            correct(rt, spec, config, V::NULL, V::NULL, false);
            save_loaded(rt, loaded);
        }
    }
    reset_caches(rt, spec, "NONE");
}

fn is_correct_any(rt: &mut Rt, spec: V, config: V) -> bool {
    if rt.class_of(spec) == SPEC {
        return correct(rt, spec, config, V::NULL, V::NULL, true) == 0;
    }
    if spec.is_null() {
        return true;
    }
    let r = rt.call_functional(IS_CORRECT, spec, &[config]);
    rt.report_thrown("IConfigSpec.isCorrect");
    r.i() != 0
}

fn correct_any(rt: &mut Rt, spec: V, config: V) {
    if rt.class_of(spec) == SPEC {
        correct(rt, spec, config, V::NULL, V::NULL, false);
    } else if !spec.is_null() {
        rt.call_functional(CORRECT, spec, &[config]);
        rt.report_thrown("IConfigSpec.correct");
    }
}

/// `ConfigTracker.createDefaultConfig`: an empty config the spec corrects.
fn default_config(rt: &mut Rt, spec: V) -> V {
    let c = nightconfig::new_config(rt, COMMENTED, None);
    correct_any(rt, spec, c);
    c
}

fn spec_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or(V::NULL);
    let this = a(0);
    Some(match name {
        "isEmpty" => {
            let specs = tree(rt, this, "$specTree");
            let map = nightconfig::values(rt, specs);
            V::I(jutil::table_pairs(rt, map).is_empty() as i32)
        }
        "getLevelComment" | "getLevelTranslationKey" => {
            let map = level_map(
                rt,
                this,
                if name == "getLevelComment" {
                    "$levelComments"
                } else {
                    "$levelKeys"
                },
            );
            let path = keys(rt, a(1));
            let k = rt.string(&level_key(&path));
            jutil::table_get(rt, map, k).unwrap_or(V::NULL)
        }
        "acceptConfig" => {
            accept(rt, this, a(1));
            V::Void
        }
        "validateSpec" => {
            validate(rt, this, a(1));
            V::Void
        }
        "isLoaded" => V::I(!field(rt, this, "$loaded").is_null() as i32),
        "getSpec" => tree(rt, this, "$specTree"),
        "getValues" => tree(rt, this, "$valueTree"),
        "afterReload" => {
            reset_caches(rt, this, "NONE");
            V::Void
        }
        "resetCaches" => {
            let r = restart_name(rt, a(1));
            reset_caches(rt, this, &r);
            V::Void
        }
        "save" => {
            match field(rt, this, "$loaded") {
                l if l.is_null() => {
                    npe(
                        rt,
                        "Cannot save config value without assigned Config object present",
                    );
                }
                l => save_loaded(rt, l),
            }
            V::Void
        }
        "isCorrect" => V::I((correct(rt, this, a(1), V::NULL, V::NULL, true) == 0) as i32),
        "correct" => {
            let n = crate::neo::desc_params(desc).len();
            let listener = if n > 1 { a(2) } else { V::NULL };
            let comments = if n > 2 { a(3) } else { V::NULL };
            let count = correct(rt, this, a(1), listener, comments, false);
            if desc.ends_with(")V") {
                V::Void
            } else {
                V::I(count)
            }
        }
        _ => return None,
    })
}

// --- Loading ----------------------------------------------------------------------------------

/// `ModContainer.registerConfig`: the mod's `ModConfig` (its type, spec and file, FML's
/// `<mod id>-<type>.toml` unless named), loaded with the others once the registries are
/// ([`load_pending`]); a `STARTUP` config loads at once, as FML opens it there. As FML's tracker,
/// a spec a server's config can't hold, or a file another config has, throws.
pub(crate) fn register(rt: &mut Rt, container: V, ty: V, spec: V, file: V) -> V {
    let id = field(rt, container, "$modId");
    let kind = text(rt, rt.get_field(ty, "$name"))
        .unwrap_or_else(|| "COMMON".into())
        .to_ascii_lowercase();
    let file = match rt.str_of(file) {
        Some(_) => file,
        None => {
            let name = format!("{}-{kind}.toml", rt.str_of(id).unwrap_or(""));
            rt.string(&name)
        }
    };
    let config = rt.new_object(MOD_CONFIG);
    for (name, v) in [
        ("$type", ty),
        ("$spec", spec),
        ("$modId", id),
        ("$fileName", file),
        ("$container", container),
    ] {
        rt.put_field(config, name, v);
    }
    if rt.class_of(spec) == SPEC && !validate(rt, spec, config) {
        return config;
    }
    let all = rt.intern(ALL.into(), |rt| jutil::new_list(rt, Vec::new()));
    let name = text(rt, file).unwrap_or_default();
    if let Some(other) = jutil::elements(rt, all)
        .into_iter()
        .find(|&c| rt.str_of(rt.get_field(c, "$fileName")) == Some(name.as_str()))
    {
        let message = format!(
            "Detected config file conflict on {name} from {} (already registered by {})",
            rt.str_of(id).unwrap_or(""),
            rt.str_of(rt.get_field(other, "$modId")).unwrap_or("")
        );
        crate::files::throw(rt, "java/lang/RuntimeException", &message);
        return config;
    }
    jutil::list_add(rt, all, config);
    if kind == "startup" {
        let dir = config_dir(rt);
        open(rt, config, dir);
        post_event(rt, LOADING, config);
    } else {
        let pending = rt.intern(PENDING.into(), |rt| jutil::new_list(rt, Vec::new()));
        jutil::list_add(rt, pending, config);
    }
    config
}

/// NeoForge's "Config loading" step, once the registries are frozen and before common setup: each
/// config registered loads, in the order registered, then each `ModConfigEvent.Loading` is posted
/// (all loaded first: this runtime posts an event to every mod it carries, and a listener reading
/// its own values on another mod's event finds them loaded). A server's configs load with the rest
/// (NeoForge loads them as its server starts).
pub(crate) fn load_pending(rt: &mut Rt) {
    let Some(pending) = rt.interned.remove(PENDING) else {
        return;
    };
    let configs = jutil::elements(rt, pending);
    let dir = config_dir(rt);
    for &c in &configs {
        open(rt, c, dir.clone());
    }
    for c in configs {
        post_event(rt, LOADING, c);
    }
}

/// FML's config folder (`config/` in the game's folder) when this runtime keeps config files: a game
/// folder was set ([`crate::files::GAME_DIR`], as the game's runtimes set it).
fn config_dir(rt: &Rt) -> Option<PathBuf> {
    rt.statics
        .contains_key(crate::files::GAME_DIR)
        .then(|| crate::files::game_dir(rt).join("config"))
}

thread_local! {
    /// How deep this thread is in [`with_files`].
    static IN_FILES: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Runs `f` as the only one in this process reading or writing config files (the game's runtimes,
/// client, server and world generation, load the same ones); again on the same thread (a mod's
/// spec saving while it is corrected) it just runs.
fn with_files<R>(f: impl FnOnce() -> R) -> R {
    static FILES: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let depth = IN_FILES.with(|d| d.replace(d.get() + 1));
    let guard = (depth == 0).then(|| FILES.lock().unwrap_or_else(|e| e.into_inner()));
    let r = f();
    drop(guard);
    IN_FILES.with(|d| d.set(depth));
    r
}

/// `ConfigTracker.openConfig`: the config read from its file in `dir` (made first, from
/// `defaultconfigs/` or the spec's defaults, when there is none; backed up and corrected when the
/// spec finds it wrong; backed up and made again when it can't be read), else its defaults in
/// memory; then the spec takes it.
fn open(rt: &mut Rt, config: V, dir: Option<PathBuf>) {
    let spec = field(rt, config, "$spec");
    let file = text(rt, field(rt, config, "$fileName")).unwrap_or_default();
    let mut path = None;
    let values = match dir {
        Some(dir) => {
            let at = dir.join(&file);
            let defaults = crate::files::game_dir(rt)
                .join("defaultconfigs")
                .join(&file);
            match with_files(|| read_or_make(rt, spec, &at, &defaults)) {
                Ok(c) => {
                    path = Some(at);
                    c
                }
                Err(e) => {
                    eprintln!(
                        "horizon-compat: config {} can't be read or made ({e}); its defaults are used",
                        at.display()
                    );
                    default_config(rt, spec)
                }
            }
        }
        None => default_config(rt, spec),
    };
    let path = match path {
        Some(p) => rt.string(&p.to_string_lossy()),
        None => V::NULL,
    };
    let loaded = new_loaded(rt, values, path, config);
    rt.put_field(config, "$loaded", loaded);
    accept(rt, spec, loaded);
}

/// FML's `LoadedConfig(config, path, modConfig)`.
fn new_loaded(rt: &mut Rt, values: V, path: V, config: V) -> V {
    let loaded = rt.new_object(LOADED);
    for (name, v) in [("$config", values), ("$path", path), ("$modConfig", config)] {
        rt.put_field(loaded, name, v);
    }
    loaded
}

fn read_or_make(rt: &mut Rt, spec: V, path: &Path, defaults: &Path) -> Result<V, String> {
    let make = |rt: &mut Rt| -> Result<V, String> {
        set_up(rt, spec, path, defaults)?;
        read(rt, path)
    };
    let parsed = match std::fs::read(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return make(rt),
        Err(e) => Err(e.to_string()),
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|e| e.to_string())
            .and_then(|t| toml::parse(&t)),
    };
    match parsed {
        Ok(table) => {
            let values = to_config(rt, &table);
            if !is_correct_any(rt, spec, values) {
                back_up(path);
                correct_any(rt, spec, values);
                write_file(rt, path, values)?;
            }
            Ok(values)
        }
        Err(_) => {
            back_up(path);
            let _ = std::fs::remove_file(path);
            make(rt)
        }
    }
}

fn read(rt: &mut Rt, path: &Path) -> Result<V, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let table = toml::parse(&text)?;
    Ok(to_config(rt, &table))
}

/// `ConfigTracker.setupConfigFile`: the file `defaultconfigs/` has, else the spec's defaults.
fn set_up(rt: &mut Rt, spec: V, path: &Path, defaults: &Path) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if defaults.is_file() {
        return std::fs::copy(defaults, path)
            .map(|_| ())
            .map_err(|e| e.to_string());
    }
    let values = default_config(rt, spec);
    write_file(rt, path, values)
}

/// `ConfigTracker.backUpConfig`: `<name>-1.<ext>.bak` the file as it is, the older ones moved up
/// (five kept).
fn back_up(path: &Path) {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else {
        return;
    };
    let (stem, ext) = match name.rfind('.') {
        Some(i) => (&name[..i], &name[i + 1..]),
        None => (name, ""),
    };
    let bak = |i: u32| dir.join(format!("{stem}-{i}.{ext}.bak"));
    for i in (1..=BACKUPS).rev() {
        let old = bak(i);
        if old.exists() {
            let _ = if i >= BACKUPS {
                std::fs::remove_file(&old)
            } else {
                std::fs::rename(&old, bak(i + 1))
            };
        }
    }
    let _ = std::fs::copy(path, bak(1));
}

/// Writes the config as Night Config's `TomlWriter` does, whole (a file beside it, then renamed:
/// another runtime reading it never finds it half written).
fn write_file(rt: &mut Rt, path: &Path, values: V) -> Result<(), String> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let table = from_config(rt, values)?;
    let text = toml::write(&table, NEWLINE);
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.{}.{n}.tmp", std::process::id()));
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

/// `LoadedConfig.save()`: the file written (when there is one), then `ModConfigEvent.Reloading`.
fn save_loaded(rt: &mut Rt, loaded: V) {
    if let Some(p) = text(rt, field(rt, loaded, "$path")) {
        let values = field(rt, loaded, "$config");
        if let Err(e) = with_files(|| write_file(rt, Path::new(&p), values)) {
            rt.supers.insert(WRITING, "java/lang/RuntimeException");
            crate::files::throw(rt, WRITING, &e);
            return;
        }
    }
    let config = field(rt, loaded, "$modConfig");
    post_event(rt, RELOADING, config);
}

fn event_supers(rt: &mut Rt) {
    for (class, sup) in [
        (LOADING, CONFIG_EVENT),
        (RELOADING, CONFIG_EVENT),
        (UNLOADING, CONFIG_EVENT),
        (CONFIG_EVENT, "net/neoforged/bus/api/Event"),
    ] {
        rt.supers.insert(class, sup);
    }
}

/// A config's event (`Loading`, `Reloading`, `Unloading`) to the mod bus, heard by a listener for
/// it or for `ModConfigEvent` (the MDK's `Config.onLoad` copies the values into its fields there).
fn post_event(rt: &mut Rt, class: &'static str, config: V) {
    event_supers(rt);
    let ev = rt.new_object(class);
    rt.put_field(ev, "$config", config);
    crate::neo::post(rt, class, ev);
}

fn loaded_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let this = args.first().copied().unwrap_or(V::NULL);
    Some(match name {
        "config" => field(rt, this, "$config"),
        "path" => match text(rt, field(rt, this, "$path")) {
            Some(p) => crate::files::new_path(rt, &p),
            None => V::NULL,
        },
        "modConfig" => field(rt, this, "$modConfig"),
        "save" => {
            save_loaded(rt, this);
            V::Void
        }
        _ => return None,
    })
}

fn mod_config_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let this = args.first().copied().unwrap_or(V::NULL);
    let loaded = field(rt, this, "$loaded");
    Some(match name {
        "getLoadedConfig" => loaded,
        "getFullPath" => match text(rt, field(rt, loaded, "$path")) {
            Some(p) => crate::files::new_path(rt, &p),
            None => {
                let file = text(rt, field(rt, this, "$fileName")).unwrap_or_default();
                let what = if loaded.is_null() {
                    "null"
                } else {
                    "LoadedConfig"
                };
                crate::files::throw(
                    rt,
                    "java/lang/IllegalStateException",
                    &format!("Cannot call getFullPath() on non-file config {what} at path {file}"),
                )
            }
        },
        _ => return None,
    })
}

/// The configs registered (FML's tracker), those `keep` keeps.
fn registered(rt: &Rt, keep: impl Fn(&Rt, V) -> bool) -> Vec<V> {
    match rt.interned.get(ALL) {
        Some(&all) => jutil::elements(rt, all)
            .into_iter()
            .filter(|&c| keep(rt, c))
            .collect(),
        None => Vec::new(),
    }
}

fn of_type(rt: &Rt, config: V, ty: &str) -> bool {
    rt.str_of(rt.get_field(rt.get_field(config, "$type"), "$name")) == Some(ty)
}

fn mod_configs_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or(V::NULL);
    let id = text(rt, a(0));
    let ty = text(
        rt,
        rt.get_field(if name == "getConfigSet" { a(0) } else { a(1) }, "$name"),
    )
    .unwrap_or_default();
    let of_mod = |rt: &Rt, c: V| rt.str_of(rt.get_field(c, "$modId")) == id.as_deref();
    Some(match name {
        "getModConfigs" => {
            let configs = registered(rt, of_mod);
            jutil::new_list(rt, configs)
        }
        "getConfigFileNames" => {
            let configs = registered(rt, |rt, c| of_mod(rt, c) && of_type(rt, c, &ty));
            let names = configs
                .into_iter()
                .map(|c| rt.get_field(c, "$fileName"))
                .collect();
            jutil::new_list(rt, names)
        }
        "getConfigSet" => {
            let configs = registered(rt, |rt, c| of_type(rt, c, &ty));
            jutil::identity_set(rt, &configs)
        }
        "getFileMap" => {
            let map = rt.alloc("java/util/LinkedHashMap", Data::Table(Default::default()));
            for c in registered(rt, |_, _| true) {
                let file = rt.get_field(c, "$fileName");
                jutil::table_put(rt, map, file, c);
            }
            map
        }
        _ => return None,
    })
}

fn tracker_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or(V::NULL);
    Some(match name {
        "<init>" => V::Void,
        "registerConfig" => {
            let file = if crate::neo::desc_params(desc).len() > 3 {
                a(4)
            } else {
                V::NULL
            };
            register(rt, a(3), a(1), a(2), file)
        }
        // `loadConfigs(type, folder[, override folder])`: each config of the type opened from there.
        "loadConfigs" => {
            let ty = text(rt, rt.get_field(a(1), "$name")).unwrap_or_default();
            let dir = match (config_dir(rt), text(rt, a(2))) {
                (Some(_), Some(d)) => Some(crate::files::on_disk(rt, &d)),
                _ => None,
            };
            let configs = registered(rt, |rt, c| of_type(rt, c, &ty));
            for &c in &configs {
                open(rt, c, dir.clone());
            }
            for c in configs {
                post_event(rt, LOADING, c);
            }
            V::Void
        }
        "unloadConfigs" => {
            let ty = text(rt, rt.get_field(a(1), "$name")).unwrap_or_default();
            for c in registered(rt, |rt, c| {
                of_type(rt, c, &ty) && !rt.get_field(c, "$loaded").is_null()
            }) {
                rt.put_field(c, "$loaded", V::NULL);
                let spec = field(rt, c, "$spec");
                accept(rt, spec, V::NULL);
                post_event(rt, UNLOADING, c);
            }
            V::Void
        }
        "loadDefaultServerConfigs" => {
            for c in registered(rt, |rt, c| of_type(rt, c, "SERVER")) {
                let spec = field(rt, c, "$spec");
                let values = default_config(rt, spec);
                let loaded = new_loaded(rt, values, V::NULL, c);
                rt.put_field(c, "$loaded", loaded);
                accept(rt, spec, loaded);
                post_event(rt, LOADING, c);
            }
            V::Void
        }
        // `acceptSyncedConfig(config, bytes)`: the server's TOML, in memory.
        "acceptSyncedConfig" => {
            let bytes: Vec<u8> = jutil::elements(rt, a(1))
                .into_iter()
                .map(|b| b.i() as u8)
                .collect();
            match toml::parse(&String::from_utf8_lossy(&bytes)) {
                Ok(table) => {
                    let values = to_config(rt, &table);
                    let loaded = new_loaded(rt, values, V::NULL, a(0));
                    rt.put_field(a(0), "$loaded", loaded);
                    let spec = field(rt, a(0), "$spec");
                    accept(rt, spec, loaded);
                    post_event(rt, RELOADING, a(0));
                    V::Void
                }
                Err(e) => {
                    rt.supers.insert(PARSING, "java/lang/RuntimeException");
                    crate::files::throw(rt, PARSING, &e)
                }
            }
        }
        _ => return None,
    })
}

// --- TOML and the runtime's configs -----------------------------------------------------------

/// The config Night Config's parser makes of a table: a commented config per table, Java's boxes
/// for numbers and booleans, lists for arrays.
fn to_config(rt: &mut Rt, t: &toml::Table) -> V {
    let c = nightconfig::new_config(rt, COMMENTED, None);
    let map = nightconfig::values(rt, c);
    for (k, v) in &t.entries {
        let key = rt.string(k);
        let v = to_value(rt, v);
        jutil::table_put(rt, map, key, v);
    }
    let comments = nightconfig::comments(rt, c);
    for (k, text) in &t.comments {
        let (key, text) = (rt.string(k), rt.string(text));
        jutil::table_put(rt, comments, key, text);
    }
    c
}

fn to_value(rt: &mut Rt, v: &toml::Value) -> V {
    use toml::Value as T;
    match v {
        T::Str(s) | T::Temporal(s) => rt.string(s),
        T::Int(i) => crate::mixin::box_value(rt, V::I(*i), 'I'),
        T::Long(j) => crate::mixin::box_value(rt, V::J(*j), 'J'),
        T::Float(f) => crate::mixin::box_value(rt, V::F(*f), 'F'),
        T::Double(d) => crate::mixin::box_value(rt, V::D(*d), 'D'),
        T::Bool(b) => crate::api_common::boolean(rt, *b),
        T::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for i in items {
                out.push(to_value(rt, i));
            }
            jutil::new_list(rt, out)
        }
        T::Table(t) => to_config(rt, t),
    }
}

/// The table Night Config's writer writes of a config; what TOML can't hold (null, an object of
/// another kind) is an error, as its `WritingException`.
fn from_config(rt: &mut Rt, c: V) -> Result<toml::Table, String> {
    let mut t = toml::Table::default();
    let map = nightconfig::values(rt, c);
    for (k, v) in jutil::table_pairs(rt, map) {
        let key = rt.to_java_string(k, 'L');
        let value = from_value(rt, v)?;
        t.entries.push((key, value));
    }
    let comments = nightconfig::comments(rt, c);
    for (k, v) in jutil::table_pairs(rt, comments) {
        if let Some(text) = rt.str_of(v) {
            t.comments
                .push((rt.to_java_string(k, 'L'), text.to_owned()));
        }
    }
    Ok(t)
}

fn from_value(rt: &mut Rt, v: V) -> Result<toml::Value, String> {
    use toml::Value as T;
    Ok(match v {
        V::I(i) => T::Int(i),
        V::J(j) => T::Long(j),
        V::F(f) => T::Float(f),
        V::D(d) => T::Double(d),
        _ if v.is_null() => return Err("TOML doesn't support null values".into()),
        _ if nightconfig::is_config(rt, v) => T::Table(from_config(rt, v)?),
        _ if rt.str_of(v).is_some() => T::Str(rt.str_of(v).unwrap_or("").to_owned()),
        _ if is_list(rt, v) => {
            let mut out = Vec::new();
            for e in jutil::elements(rt, v) {
                out.push(from_value(rt, e)?);
            }
            T::Array(out)
        }
        _ => {
            let class = rt.class_of(v);
            let raw = rt.get_field(v, "value");
            match class {
                "java/lang/Boolean" => T::Bool(raw.i() != 0),
                "java/lang/Integer" | "java/lang/Short" | "java/lang/Byte" => T::Int(raw.i()),
                "java/lang/Long" => T::Long(long_of(rt, raw)),
                "java/lang/Float" => T::Float(as_f64(raw) as f32),
                "java/lang/Double" => T::Double(as_f64(raw)),
                _ => {
                    let name = crate::api_common::enum_name(rt, v, false);
                    match rt.str_of(name) {
                        Some(n) => T::Str(n.to_owned()),
                        None => {
                            return Err(format!(
                                "Unsupported value type: class {}",
                                class.replace('/', ".")
                            ));
                        }
                    }
                }
            }
        }
    })
}

/// TOML as Night Config 3.8 reads it (`TomlParser`) and writes it (`TomlWriter` as NeoForge has it:
/// a tab per level, the system's line ends, comments as `#` lines, values before tables, an empty
/// table inline, levels holding only tables left out).
mod toml {
    #[derive(Debug, Clone, PartialEq)]
    pub(super) enum Value {
        Str(String),
        Int(i32),
        Long(i64),
        Float(f32),
        Double(f64),
        Bool(bool),
        /// A date or a time, kept as written.
        Temporal(String),
        Array(Vec<Value>),
        Table(Table),
    }

    /// A table: its entries in order, its keys' comments.
    #[derive(Debug, Clone, Default, PartialEq)]
    pub(super) struct Table {
        pub(super) entries: Vec<(String, Value)>,
        pub(super) comments: Vec<(String, String)>,
        /// Written inline (`{a = 1}`): no header adds to it.
        pub(super) inline: bool,
    }

    impl Table {
        pub(super) fn get(&self, k: &str) -> Option<&Value> {
            self.entries.iter().find(|(e, _)| e == k).map(|(_, v)| v)
        }

        fn get_mut(&mut self, k: &str) -> Option<&mut Value> {
            self.entries
                .iter_mut()
                .find(|(e, _)| e == k)
                .map(|(_, v)| v)
        }

        /// Puts `v` at `k` (in its place when there); the value it replaces.
        fn put(&mut self, k: &str, v: Value) -> Option<Value> {
            match self.get_mut(k) {
                Some(old) => Some(std::mem::replace(old, v)),
                None => {
                    self.entries.push((k.to_owned(), v));
                    None
                }
            }
        }

        pub(super) fn comment(&self, k: &str) -> Option<&str> {
            self.comments
                .iter()
                .find(|(e, _)| e == k)
                .map(|(_, c)| c.as_str())
        }

        fn set_comment(&mut self, k: &str, c: Option<String>) {
            self.comments.retain(|(e, _)| e != k);
            if let Some(c) = c {
                self.comments.push((k.to_owned(), c));
            }
        }

        /// The table a dotted key's last part goes in, the levels on the way made (`Config.set`).
        fn level(&mut self, path: &[String]) -> Result<&mut Table, String> {
            let mut t = self;
            for k in path {
                if t.get(k).is_none() {
                    t.put(k, Value::Table(Table::default()));
                }
                t = match t.get_mut(k) {
                    Some(Value::Table(sub)) => sub,
                    _ => return Err(format!("Cannot set {path:?}: {k} is a value, not a table")),
                };
            }
            Ok(t)
        }
    }

    const END_OF_VALUE: &[char] = &['\t', ' ', '#', '\n', '\r', ',', ']', '}'];
    const END_OF_DATE: &[char] = &['\t', '#', '\n', '\r', ',', ']', '}'];
    const KEY_END: &[char] = &['\t', ' ', '=', '.', '\n', '\r', ']', ':'];

    fn control(c: char) -> bool {
        c as u32 <= 31 || c as u32 == 127
    }

    pub(super) fn bare(key: &str) -> bool {
        !key.is_empty()
            && key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    }

    fn digit(c: char, base: u32) -> bool {
        c.is_digit(base)
    }

    /// Parses `text` as Night Config does: comments before an entry (and after it on its line) are
    /// its comment, those before a table's header its.
    pub(super) fn parse(text: &str) -> Result<Table, String> {
        let mut p = Parser {
            s: text.chars().collect(),
            i: 0,
            pending: None,
        };
        let mut root = Table::default();
        p.normal(&mut root)?;
        while let Some(next) = p.peek(0) {
            let array = next == '[';
            if array {
                p.i += 1;
            }
            let path = p.table_name(array)?;
            let Some((last, parents)) = path.split_last() else {
                return Err("Tables names must not be empty.".into());
            };
            let parent = sub_table(&mut root, parents)?;
            if let Some(c) = p.pending.take() {
                parent.set_comment(last, Some(c));
            }
            if array {
                let mut table = Table::default();
                p.normal(&mut table)?;
                match parent.get_mut(last) {
                    Some(Value::Array(items)) => items.push(Value::Table(table)),
                    Some(_) => {
                        return Err(format!(
                            "Cannot create entry {path:?} because of an invalid parent that is not an array of tables"
                        ));
                    }
                    None => {
                        parent.put(last, Value::Array(vec![Value::Table(table)]));
                    }
                }
            } else {
                match parent.get_mut(last) {
                    None => {
                        let mut table = Table::default();
                        p.normal(&mut table)?;
                        parent.put(last, Value::Table(table));
                    }
                    Some(Value::Table(t)) if t.entries.iter().all(|(_, v)| holds_tables(v)) => {
                        p.normal(t)?
                    }
                    Some(Value::Table(_)) => {
                        return Err(format!("Table with path {path:?} has been declared twice."));
                    }
                    Some(_) => return Err(format!("Entry {path:?} has been defined twice.")),
                }
            }
        }
        Ok(root)
    }

    /// What a table declared again may hold: tables, and arrays of them.
    fn holds_tables(v: &Value) -> bool {
        match v {
            Value::Table(_) => true,
            Value::Array(items) => matches!(items.first(), Some(Value::Table(_))),
            _ => false,
        }
    }

    /// The table a header's path leads to: levels made on the way, an array of tables' last one.
    fn sub_table<'t>(root: &'t mut Table, path: &[String]) -> Result<&'t mut Table, String> {
        let mut t = root;
        for k in path {
            if t.get(k).is_none() {
                t.put(k, Value::Table(Table::default()));
            }
            t = match t.get_mut(k) {
                Some(Value::Table(sub)) => sub,
                Some(Value::Array(items)) if items.iter().all(|v| matches!(v, Value::Table(_))) => {
                    match items.last_mut() {
                        Some(Value::Table(sub)) => sub,
                        _ => {
                            return Err(format!(
                                "Cannot create entry {path:?} because of an invalid parent that isn't a table."
                            ));
                        }
                    }
                }
                _ => {
                    return Err(format!(
                        "Cannot create entry {path:?} because of an invalid parent that isn't a table."
                    ));
                }
            };
            if t.inline {
                return Err(format!(
                    "Cannot modify an inline table after its creation. Key path: {path:?}"
                ));
            }
        }
        Ok(t)
    }

    struct Parser {
        s: Vec<char>,
        i: usize,
        /// The comment the next entry or table takes.
        pending: Option<String>,
    }

    impl Parser {
        fn peek(&self, ahead: usize) -> Option<char> {
            self.s.get(self.i + ahead).copied()
        }

        fn next(&mut self) -> Option<char> {
            let c = self.peek(0)?;
            self.i += 1;
            Some(c)
        }

        fn need(&mut self) -> Result<char, String> {
            self.next()
                .ok_or_else(|| "Not enough data available".to_owned())
        }

        /// The next character past spaces and tabs (and line ends when `lines`).
        fn skip(&mut self, lines: bool) -> Option<char> {
            while let Some(c) = self.next() {
                if !(c == ' ' || c == '\t' || (lines && (c == '\n' || c == '\r'))) {
                    return Some(c);
                }
            }
            None
        }

        fn skip_need(&mut self, lines: bool) -> Result<char, String> {
            self.skip(lines)
                .ok_or_else(|| "Not enough data available".to_owned())
        }

        /// The characters before the next of `stop` (left unread) or the end.
        fn until(&mut self, stop: &[char]) -> String {
            let start = self.i;
            while self.peek(0).is_some_and(|c| !stop.contains(&c)) {
                self.i += 1;
            }
            self.s[start..self.i].iter().collect()
        }

        /// The rest of the line, without the `\r` before its end.
        fn line(&mut self) -> String {
            let mut l = self.until(&['\n']);
            if l.ends_with('\r') {
                l.pop();
            }
            l
        }

        /// The next character past whitespace, line ends and comments, the comments' text kept.
        fn useful(&mut self, comments: &mut Vec<String>) -> Option<char> {
            loop {
                let c = self.skip(true)?;
                if c != '#' {
                    return Some(c);
                }
                comments.push(self.line());
            }
        }

        fn useful_char(&mut self) -> Result<char, String> {
            self.useful(&mut Vec::new())
                .ok_or_else(|| "Not enough data available".to_owned())
        }

        /// Adds `lines` to the comment waiting for the next entry or table.
        fn comment(&mut self, lines: Vec<String>) -> Result<(), String> {
            if lines.is_empty() {
                return Ok(());
            }
            let text = lines.join("\n");
            if text.chars().any(|c| c != '\t' && c != '\n' && control(c)) {
                return Err(format!("Invalid control character in comment: {text}"));
            }
            self.pending = Some(match self.pending.take() {
                Some(p) => format!("{p}\n{text}"),
                None => text,
            });
            Ok(())
        }

        /// A table's entries, up to the next header (its `[` read) or the end.
        fn normal(&mut self, table: &mut Table) -> Result<(), String> {
            loop {
                let mut comments = Vec::new();
                let first = match self.useful(&mut comments) {
                    None | Some('[') => return self.comment(comments),
                    Some(c) => c,
                };
                let key = self.dotted_key(first)?;
                let c = self.skip_need(false)?;
                let value = self.value(c)?;
                let Some((last, parents)) = key.split_last() else {
                    return Err("Empty key".into());
                };
                if table.level(parents)?.put(last, value).is_some() {
                    return Err(format!(
                        "Invalid TOML data: entry {key:?} defined twice in its table."
                    ));
                }
                match self.skip(false) {
                    // The last entry's comment goes unread when no line end follows it.
                    None => return Ok(()),
                    Some('#') => comments.push(self.line()),
                    Some('\n' | '\r') => {}
                    Some(c) => {
                        return Err(format!("Invalid character '{c}' after table entry {key:?}"));
                    }
                }
                self.comment(comments)?;
                let c = self.pending.take();
                table.level(parents)?.set_comment(last, c);
            }
        }

        fn dotted_key(&mut self, first: char) -> Result<Vec<String>, String> {
            let mut keys = Vec::new();
            let mut first = first;
            loop {
                keys.push(self.key_part(first)?);
                let sep = self.skip_need(false)?;
                if sep == '=' {
                    return Ok(keys);
                }
                if sep != '.' {
                    return Err(format!("Invalid character '{sep}' after key {keys:?}"));
                }
                first = self.skip_need(false)?;
            }
        }

        fn key_part(&mut self, first: char) -> Result<String, String> {
            match first {
                '"' => self.basic(),
                '\'' => self.literal(),
                _ => {
                    let key = format!("{first}{}", self.until(KEY_END));
                    if bare(&key) {
                        Ok(key)
                    } else {
                        Err(format!("Invalid bare key: '{key}'"))
                    }
                }
            }
        }

        fn table_name(&mut self, array: bool) -> Result<Vec<String>, String> {
            let mut keys = Vec::new();
            loop {
                let first = self.skip_need(false)?;
                if first == ']' {
                    return Err("Tables names must not be empty.".into());
                }
                keys.push(self.key_part(first)?);
                let sep = self.skip_need(false)?;
                if sep == ']' {
                    if array && self.need()? != ']' {
                        return Err(format!(
                            "Invalid declaration of an element of an array of tables {keys:?}"
                        ));
                    }
                    match self.skip(false) {
                        Some('#') => {
                            let c = self.line();
                            self.comment(vec![c])?;
                        }
                        Some('\n' | '\r') | None => {}
                        Some(c) => {
                            return Err(format!(
                                "Invalid character '{c}' after a table declaration."
                            ));
                        }
                    }
                    return Ok(keys);
                }
                if sep != '.' {
                    return Err(format!("Invalid separator '{sep}' in table name."));
                }
            }
        }

        fn value(&mut self, first: char) -> Result<Value, String> {
            Ok(match first {
                '"' if self.peek(0) == Some('"') && self.peek(1) == Some('"') => {
                    self.i += 2;
                    Value::Str(self.multi(true)?)
                }
                '"' => Value::Str(self.basic()?),
                '\'' if self.peek(0) == Some('\'') && self.peek(1) == Some('\'') => {
                    self.i += 2;
                    Value::Str(self.multi(false)?)
                }
                '\'' => Value::Str(self.literal()?),
                '+' | '-' => {
                    self.i -= 1;
                    let chars = self.until(END_OF_VALUE);
                    number(&chars)?
                }
                '[' => self.array()?,
                'f' | 't' => {
                    let rest = self.until(END_OF_VALUE);
                    match (first, rest.as_str()) {
                        ('f', "alse") => Value::Bool(false),
                        ('t', "rue") => Value::Bool(true),
                        _ => {
                            return Err(format!(
                                "Invalid value {first}{rest} - Expected a boolean value."
                            ));
                        }
                    }
                }
                '{' => self.inline()?,
                _ => {
                    self.i -= 1;
                    let chars = self.until(END_OF_DATE);
                    let cs: Vec<char> = chars.chars().collect();
                    if cs.len() >= 8 && (cs[2] == ':' || (cs[4] == '-' && cs[7] == '-')) {
                        Value::Temporal(chars.trim_end().to_owned())
                    } else {
                        let t = chars.trim();
                        if t.is_empty() {
                            return Err("Invalid value containing only whitespaces".into());
                        }
                        number(t)?
                    }
                }
            })
        }

        fn array(&mut self) -> Result<Value, String> {
            let mut items = Vec::new();
            loop {
                let c = self.useful_char()?;
                if c == ']' {
                    return Ok(Value::Array(items));
                }
                if c == ',' {
                    return Err(if items.is_empty() {
                        "Invalid array: [,]"
                    } else {
                        "Invalid double comma in array."
                    }
                    .into());
                }
                items.push(self.value(c)?);
                match self.useful_char()? {
                    ']' => return Ok(Value::Array(items)),
                    ',' => {}
                    after => return Err(format!("Invalid separator '{after}' in array.")),
                }
            }
        }

        fn inline(&mut self) -> Result<Value, String> {
            let mut t = Table {
                inline: true,
                ..Table::default()
            };
            let mut expect = false;
            loop {
                let first = self.skip_need(false)?;
                if first == '}' {
                    if expect {
                        return Err("Invalid trailing comma in inline table".into());
                    }
                    return Ok(Value::Table(t));
                }
                let key = self.dotted_key(first)?;
                let c = self.skip_need(false)?;
                let v = self.value(c)?;
                let Some((last, parents)) = key.split_last() else {
                    return Err("Empty key".into());
                };
                if t.level(parents)?.put(last, v).is_some() {
                    return Err(format!(
                        "Invalid TOML data: entry {key:?} defined twice in its table."
                    ));
                }
                match self.skip_need(false)? {
                    '}' => return Ok(Value::Table(t)),
                    ',' => expect = true,
                    after => {
                        return Err(format!(
                            "Invalid entry separator '{after}' in inline table."
                        ));
                    }
                }
            }
        }

        fn basic(&mut self) -> Result<String, String> {
            let mut out = String::new();
            loop {
                match self.need()? {
                    '"' => return Ok(out),
                    '\\' => {
                        let e = self.need()?;
                        out.push_str(&self.unescape(e)?);
                    }
                    '\n' | '\r' => {
                        return Err(format!(
                            "Invalid newline in basic string, you should use a multiline string or escape the newline by writing \\n. The string begins with: \"{out}\""
                        ));
                    }
                    c if c != '\t' && control(c) => {
                        return Err(format!("Invalid control character in string: \"{out}\""));
                    }
                    c => out.push(c),
                }
            }
        }

        fn literal(&mut self) -> Result<String, String> {
            let s = self.until(&['\'', '\n', '\r']);
            if self.need()? != '\'' {
                return Err(format!(
                    "Invalid newline in literal string, you should use a multiline string. The string is '{s}'"
                ));
            }
            if s.chars().any(|c| c != '\t' && control(c)) {
                return Err(format!("Invalid control character in literal string '{s}'"));
            }
            Ok(s)
        }

        /// A multi-line string, its opening quotes read: escapes and line-ending backslashes in a
        /// basic one; its first line end dropped.
        fn multi(&mut self, basic: bool) -> Result<String, String> {
            let q = if basic { '"' } else { '\'' };
            let mut out = String::new();
            loop {
                let c = self.need()?;
                if c == q && self.peek(0) == Some(q) && self.peek(1) == Some(q) {
                    break;
                }
                if basic && c == '\\' {
                    let next = self.need()?;
                    let ends_line = next == '\n'
                        || (next == '\r' && self.peek(0) == Some('\n'))
                        || (matches!(next, '\t' | ' ')
                            && self.line().chars().all(|c| c == ' ' || c == '\t'));
                    if ends_line {
                        if self.skip(true).is_some() {
                            self.i -= 1;
                        }
                    } else if matches!(next, '\t' | ' ') {
                        return Err(format!("Invalid escapement: \\{next}"));
                    } else {
                        out.push_str(&self.unescape(next)?);
                    }
                } else {
                    if c != '\n' && c != '\r' && c != '\t' && control(c) {
                        return Err(format!(
                            "Invalid control character in multiline string: {out}"
                        ));
                    }
                    out.push(c);
                }
            }
            self.i += 2;
            for _ in 0..2 {
                if self.peek(0) == Some(q) {
                    self.i += 1;
                    out.push(q);
                }
            }
            Ok(
                match out.strip_prefix('\n').or_else(|| out.strip_prefix("\r\n")) {
                    Some(rest) => rest.to_owned(),
                    None => out,
                },
            )
        }

        fn unescape(&mut self, c: char) -> Result<String, String> {
            Ok(match c {
                '"' | '\\' => c.to_string(),
                'b' => "\u{8}".into(),
                'f' => "\u{c}".into(),
                'n' => "\n".into(),
                'r' => "\r".into(),
                't' => "\t".into(),
                'u' | 'U' => {
                    let n = if c == 'u' { 4 } else { 8 };
                    let hex = (0..n)
                        .map(|_| self.need())
                        .collect::<Result<String, String>>()?;
                    u32::from_str_radix(&hex, 16)
                        .ok()
                        .filter(|&cp| cp <= 0xD7FF || (0xE000..=0x10FFFF).contains(&cp))
                        .and_then(char::from_u32)
                        .ok_or_else(|| format!("Invalid unicode codepoint: {hex}"))?
                        .to_string()
                }
                _ => return Err(format!("Invalid escapement: \\{c}")),
            })
        }
    }

    /// A number as Night Config reads it: `inf`, `nan`, `0x`/`0o`/`0b` integers, `_` between digits;
    /// an integer an `Integer` when it fits, else a `Long`; one with `.` or an exponent a `Double`.
    fn number(chars: &str) -> Result<Value, String> {
        let (negative, rest) = match chars.chars().next() {
            Some('-') => (true, &chars[1..]),
            Some('+') => (false, &chars[1..]),
            _ => (false, chars),
        };
        if rest == "inf" {
            return Ok(Value::Double(if negative {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            }));
        }
        if rest == "nan" {
            return Ok(Value::Double(f64::NAN));
        }
        let base = match chars.get(..2) {
            Some("0x") if chars.len() > 2 => 16,
            Some("0b") if chars.len() > 2 => 2,
            Some("0o") if chars.len() > 2 => 8,
            _ => 10,
        };
        let digits = if base == 10 { chars } else { &chars[2..] };
        let simple = simplify(digits, base)?;
        let cs: Vec<char> = simple.chars().collect();
        let leading_zero = (cs.len() > 1
            && cs[0] == '0'
            && (base != 10 || cs[1].is_ascii_digit() || !simple.contains(['.', 'e', 'E'])))
            || (cs.len() > 2
                && (simple.starts_with("-0") || simple.starts_with("+0"))
                && cs[2].is_ascii_digit());
        if base == 10 && simple.contains(['.', 'e', 'E']) {
            if leading_zero && cs[0] == '0' && cs[1].is_ascii_digit()
                || (cs.len() > 2
                    && (simple.starts_with("-0") || simple.starts_with("+0"))
                    && cs[2].is_ascii_digit())
            {
                return Err(format!(
                    "Invalid leading zero in floating-point number {simple}"
                ));
            }
            return simple
                .parse::<f64>()
                .map(Value::Double)
                .map_err(|_| format!("Invalid floating-point value: {chars}"));
        }
        if base == 10 && leading_zero {
            return Err(format!("Invalid leading zero in base 10 integer {simple}"));
        }
        let n = i64::from_str_radix(&simple, base)
            .map_err(|_| format!("Invalid integer value: {chars}"))?;
        Ok(match i32::try_from(n) {
            Ok(i) => Value::Int(i),
            Err(_) => Value::Long(n),
        })
    }

    /// The number's characters without its `_`s, checked as Night Config checks them.
    fn simplify(chars: &str, base: u32) -> Result<String, String> {
        let mut out = String::with_capacity(chars.len());
        let mut previous: Option<char> = None;
        for c in chars.chars() {
            match c {
                '.' | '_' => match previous {
                    None => return Err(format!("Invalid leading '{c}' in number {chars}")),
                    Some(p) if !digit(p, base) => {
                        return Err(format!(
                            "Invalid '{c}' in number (it must be surrounded by digits): {chars}"
                        ));
                    }
                    _ if c == '.' => out.push(c),
                    _ => {}
                },
                'e' if base != 16 && !previous.is_some_and(|p| digit(p, base)) => {
                    return Err(format!("Invalid sequence e in number {chars}"));
                }
                _ => out.push(c),
            }
            previous = Some(c);
        }
        match previous {
            Some(c @ ('_' | '.' | 'e' | 'E' | '-' | '+')) => {
                Err(format!("Invalid trailing '{c}' in number: {chars}"))
            }
            _ => Ok(out),
        }
    }

    /// `StringUtils.splitLines`: at `\n`, `\r` and `\r\n`, the last part kept even when empty;
    /// nothing for an empty string.
    fn split_lines(s: &str) -> Vec<&str> {
        if s.is_empty() {
            return Vec::new();
        }
        let (mut out, bytes) = (Vec::new(), s.as_bytes());
        let (mut start, mut i) = (0, 0);
        while i < bytes.len() {
            match bytes[i] {
                b'\n' => {
                    out.push(&s[start..i]);
                    start = i + 1;
                }
                b'\r' => {
                    out.push(&s[start..i]);
                    if bytes.get(i + 1) == Some(&b'\n') {
                        i += 1;
                    }
                    start = i + 1;
                }
                _ => {}
            }
            i += 1;
        }
        out.push(&s[start..]);
        out
    }

    /// Whether Night Config writes `s` as a multi-line string: no `\r`, and three line ends, or one
    /// past its first character and before its last.
    fn multiline(s: &str) -> bool {
        let cs: Vec<char> = s.chars().collect();
        let find = |from: i64| {
            cs.iter()
                .skip(from.max(0) as usize)
                .position(|&c| c == '\n')
                .map_or(-1, |p| p as i64 + from.max(0))
        };
        let i0 = find(0);
        let i1 = find(i0 + 1);
        let i2 = find(i1 + 1);
        !cs.contains(&'\r')
            && ((i0 >= 0 && i1 > 0 && i2 > 0) || (i0 > 0 && i0 < cs.len() as i64 - 1))
    }

    fn escape_unicode(c: char) -> String {
        let hex = format!("{:X}", c as u32);
        let width = if hex.len() < 4 {
            4
        } else if hex.len() < 8 {
            8
        } else {
            hex.len()
        };
        format!("\\u{hex:0>width$}")
    }

    /// Writes `t` as Night Config's `TomlWriter` does, with `newline` ending lines.
    pub(super) fn write(t: &Table, newline: &str) -> String {
        let mut w = Writer {
            out: String::new(),
            indent: -1,
            newline,
        };
        w.table(t, "", false, false, &mut Vec::new());
        w.out
    }

    struct Writer<'n> {
        out: String,
        indent: i32,
        newline: &'n str,
    }

    type Tables<'t> = Vec<(&'t String, &'t Table)>;
    type TableArrays<'t> = Vec<(&'t String, &'t Vec<Value>)>;

    impl Writer<'_> {
        fn nl(&mut self) {
            self.out.push_str(self.newline);
        }

        fn indent(&mut self) {
            for _ in 0..self.indent.max(0) {
                self.out.push('\t');
            }
        }

        fn comment(&mut self, c: &str) {
            for line in split_lines(c) {
                self.indent();
                self.out.push('#');
                self.out.push_str(line);
                self.nl();
            }
        }

        fn key(&mut self, k: &str) {
            if bare(k) {
                self.out.push_str(k);
            } else {
                self.basic(k);
            }
        }

        fn name(&mut self, path: &[String], array: bool) {
            self.indent();
            self.out.push_str(if array { "[[" } else { "[" });
            for (i, k) in path.iter().enumerate() {
                if i > 0 {
                    self.out.push('.');
                }
                self.key(k);
            }
            self.out.push_str(if array { "]]" } else { "]" });
        }

        /// `TableWriter.writeWithHeader`: a level holding only tables (and no comment) has no
        /// header of its own; its values come first, then its tables, then its arrays of tables.
        fn table(
            &mut self,
            t: &Table,
            comment: &str,
            in_array: bool,
            header: bool,
            path: &mut Vec<String>,
        ) {
            let (mut simples, mut subs, mut arrays): (Vec<(&String, &Value)>, Tables, TableArrays) =
                Default::default();
            for (k, v) in &t.entries {
                match v {
                    Value::Table(sub) if !sub.entries.is_empty() => subs.push((k, sub)),
                    Value::Array(items)
                        if !items.is_empty()
                            && items.iter().all(|i| matches!(i, Value::Table(_))) =>
                    {
                        arrays.push((k, items))
                    }
                    _ => simples.push((k, v)),
                }
            }
            let skipped = (comment.is_empty() || !arrays.is_empty())
                && simples.is_empty()
                && (!subs.is_empty() || !arrays.is_empty());
            if skipped {
                self.indent += 1;
                if in_array {
                    self.name(path, true);
                    self.nl();
                    self.indent += 1;
                }
                self.sub_tables(t, &subs, !arrays.is_empty(), path);
                if in_array {
                    self.indent -= 1;
                }
                if !comment.is_empty() {
                    self.comment(comment);
                }
                self.table_arrays(t, &arrays, path);
                self.indent -= 1;
                return;
            }
            if !comment.is_empty() {
                self.comment(comment);
            }
            if in_array || header {
                self.name(path, in_array);
                self.nl();
            }
            self.indent += 1;
            for (k, v) in &simples {
                if let Some(c) = t.comment(k) {
                    self.comment(c);
                }
                self.indent();
                self.key(k);
                self.out.push_str(" = ");
                self.value(v);
                self.nl();
            }
            if !subs.is_empty() {
                self.nl();
            }
            self.sub_tables(t, &subs, !arrays.is_empty(), path);
            self.table_arrays(t, &arrays, path);
            self.indent -= 1;
        }

        fn sub_tables(
            &mut self,
            t: &Table,
            subs: &Tables,
            arrays_follow: bool,
            path: &mut Vec<String>,
        ) {
            for (i, (k, sub)) in subs.iter().enumerate() {
                path.push((*k).clone());
                self.table(sub, t.comment(k).unwrap_or(""), false, true, path);
                path.pop();
                if arrays_follow || i + 1 < subs.len() {
                    self.nl();
                }
            }
        }

        fn table_arrays(&mut self, t: &Table, arrays: &TableArrays, path: &mut Vec<String>) {
            for (i, (k, items)) in arrays.iter().enumerate() {
                path.push((*k).clone());
                for item in items.iter() {
                    if let Value::Table(sub) = item {
                        self.table(sub, t.comment(k).unwrap_or(""), true, true, path);
                    }
                }
                path.pop();
                if i + 1 < arrays.len() {
                    self.nl();
                }
            }
        }

        fn value(&mut self, v: &Value) {
            match v {
                Value::Table(t) => {
                    self.out.push('{');
                    for (i, (k, v)) in t.entries.iter().enumerate() {
                        if i > 0 {
                            self.out.push_str(", ");
                        }
                        self.key(k);
                        self.out.push_str(" = ");
                        self.value(v);
                    }
                    self.out.push('}');
                }
                Value::Array(items) => {
                    self.out.push('[');
                    for (i, item) in items.iter().enumerate() {
                        if i > 0 {
                            self.out.push_str(", ");
                        }
                        self.value(item);
                    }
                    self.out.push(']');
                }
                Value::Str(s) if multiline(s) => self.multi(s),
                Value::Str(s) => self.basic(s),
                Value::Temporal(s) => self.out.push_str(s),
                Value::Int(i) => self.out.push_str(&i.to_string()),
                Value::Long(j) => self.out.push_str(&j.to_string()),
                Value::Bool(b) => self.out.push_str(if *b { "true" } else { "false" }),
                Value::Float(f) => self.float(f64::from(*f), true),
                Value::Double(d) => self.float(*d, false),
            }
        }

        fn float(&mut self, d: f64, float: bool) {
            let text = if d.is_nan() {
                "nan".to_owned()
            } else if d == f64::INFINITY {
                "+inf".to_owned()
            } else if d == f64::NEG_INFINITY {
                "-inf".to_owned()
            } else {
                crate::java_double(d, float)
            };
            self.out.push_str(&text);
        }

        fn basic(&mut self, s: &str) {
            self.out.push('"');
            for c in s.chars() {
                match c {
                    '\u{8}' => self.out.push_str("\\b"),
                    '\t' => self.out.push_str("\\t"),
                    '\n' => self.out.push_str("\\n"),
                    '\u{c}' => self.out.push_str("\\f"),
                    '\r' => self.out.push_str("\\r"),
                    '"' => self.out.push_str("\\\""),
                    '\\' => self.out.push_str("\\\\"),
                    c if control(c) => self.out.push_str(&escape_unicode(c)),
                    c => self.out.push(c),
                }
            }
            self.out.push('"');
        }

        /// `StringWriter.writeBasicMultiline`: each line after a line end; a quote escaped where it
        /// would end the string.
        fn multi(&mut self, s: &str) {
            self.out.push_str("\"\"\"");
            let lines = split_lines(s);
            for (li, line) in lines.iter().enumerate() {
                let more = li + 1 < lines.len();
                self.nl();
                let cs: Vec<char> = line.chars().collect();
                for (i, &c) in cs.iter().enumerate() {
                    match c {
                        '\u{8}' => self.out.push_str("\\b"),
                        '\u{c}' => self.out.push_str("\\f"),
                        '"' => {
                            let plain = (i + 1 != cs.len() || more)
                                && (i + 1 >= cs.len()
                                    || cs[i + 1] != '"'
                                    || i + 2 >= cs.len()
                                    || cs[i + 2] != '"'
                                    || i + 3 >= cs.len());
                            self.out.push_str(if plain { "\"" } else { "\\\"" });
                        }
                        '\\' => self.out.push_str("\\\\"),
                        c if c != '\t' && c != '\n' && c != '\r' && control(c) => {
                            self.out.push_str(&escape_unicode(c))
                        }
                        c => self.out.push(c),
                    }
                }
            }
            self.out.push_str("\"\"\"");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(rt: &mut Rt, key: &str, args: &[V]) -> V {
        let key = rt.static_key(key);
        crate::api::call(rt, key, args)
    }

    /// A builder call: `name.desc` of `ModConfigSpec$Builder`.
    fn b(rt: &mut Rt, name_desc: &str, args: &[V]) -> V {
        c(rt, &format!("{BUILDER}.{name_desc}"), args)
    }

    fn lambdas(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
        Some(match id {
            // The MDK's `Config::validateItemName`, here: a string.
            1 => V::I(rt.str_of(args[1]).is_some() as i32),
            // The new element's supplier.
            2 => rt.string(""),
            // A listener: keeps what it hears (an event, a correction's action).
            _ => {
                let seen = rt.captured(args[0])[0];
                jutil::list_add(rt, seen, args[1]);
                V::Void
            }
        })
    }

    /// The MDK's example config (its `Config` class) and a section of machines, as a mod's static
    /// initialiser builds it.
    struct Ruby {
        spec: V,
        dirt: V,
        magic: V,
        items: V,
        speed: V,
        slot: V,
        seed: V,
    }

    fn ruby_spec(rt: &mut Rt) -> Ruby {
        rt.lambda_call = Some(lambdas);
        let builder = rt.new_object(BUILDER);
        b(rt, "<init>.()V", &[builder]);
        let s = |rt: &mut Rt, t: &str| rt.string(t);
        let comment =
            "comment.(Ljava/lang/String;)Lnet/neoforged/neoforge/common/ModConfigSpec$Builder;";
        let note = s(rt, "Whether to log the dirt block on common setup");
        b(rt, comment, &[builder, note]);
        let name = s(rt, "logDirtBlock");
        let dirt = b(
            rt,
            "define.(Ljava/lang/String;Z)Lnet/neoforged/neoforge/common/ModConfigSpec$BooleanValue;",
            &[builder, name, V::I(1)],
        );
        let note = s(rt, "A magic number");
        b(rt, comment, &[builder, note]);
        let name = s(rt, "magicNumber");
        let magic = b(
            rt,
            "defineInRange.(Ljava/lang/String;III)Lnet/neoforged/neoforge/common/ModConfigSpec$IntValue;",
            &[builder, name, V::I(42), V::I(0), V::I(i32::MAX)],
        );
        let note = s(
            rt,
            "What you want the introduction message to be for the magic number",
        );
        b(rt, comment, &[builder, note]);
        let (name, intro) = (
            s(rt, "magicNumberIntroduction"),
            s(rt, "The magic number is... "),
        );
        b(
            rt,
            "define.(Ljava/lang/String;Ljava/lang/Object;)Lnet/neoforged/neoforge/common/ModConfigSpec$ConfigValue;",
            &[builder, name, intro],
        );
        let note = s(rt, "A list of items to log on common setup.");
        b(rt, comment, &[builder, note]);
        let (name, iron) = (s(rt, "items"), s(rt, "minecraft:iron_ingot"));
        let default = jutil::new_list(rt, vec![iron]);
        let (fresh, valid) = (
            rt.lambda_id("java/util/function/Supplier", 2, vec![]),
            rt.lambda_id("java/util/function/Predicate", 1, vec![]),
        );
        let items = b(
            rt,
            "defineListAllowEmpty.(Ljava/lang/String;Ljava/util/List;Ljava/util/function/Supplier;Ljava/util/function/Predicate;)Lnet/neoforged/neoforge/common/ModConfigSpec$ConfigValue;",
            &[builder, name, default, fresh, valid],
        );
        let note = s(rt, "Machines");
        b(rt, comment, &[builder, note]);
        let name = s(rt, "machines");
        b(
            rt,
            "push.(Ljava/lang/String;)Lnet/neoforged/neoforge/common/ModConfigSpec$Builder;",
            &[builder, name],
        );
        let lines = {
            let (a, b) = (s(rt, "How fast"), s(rt, "in RPM"));
            rt.alloc("[Ljava/lang/String;", Data::Array(vec![a, b]))
        };
        b(
            rt,
            "comment.([Ljava/lang/String;)Lnet/neoforged/neoforge/common/ModConfigSpec$Builder;",
            &[builder, lines],
        );
        let key = s(rt, "ruby.config.speed");
        b(
            rt,
            "translation.(Ljava/lang/String;)Lnet/neoforged/neoforge/common/ModConfigSpec$Builder;",
            &[builder, key],
        );
        b(
            rt,
            "worldRestart.()Lnet/neoforged/neoforge/common/ModConfigSpec$Builder;",
            &[builder],
        );
        let name = s(rt, "speed");
        let speed = b(
            rt,
            "defineInRange.(Ljava/lang/String;DDD)Lnet/neoforged/neoforge/common/ModConfigSpec$DoubleValue;",
            &[builder, name, V::D(0.5), V::D(0.0), V::D(1.0)],
        );
        let (chest, head) = (
            crate::api::get_static(rt, "net/minecraft/world/entity/EquipmentSlot.CHEST"),
            crate::api::get_static(rt, "net/minecraft/world/entity/EquipmentSlot.HEAD"),
        );
        let allowed = rt.alloc("[Ljava/lang/Enum;", Data::Array(vec![head, chest]));
        let name = s(rt, "slot");
        let slot = b(
            rt,
            "defineEnum.(Ljava/lang/String;Ljava/lang/Enum;[Ljava/lang/Enum;)Lnet/neoforged/neoforge/common/ModConfigSpec$EnumValue;",
            &[builder, name, head, allowed],
        );
        b(
            rt,
            "pop.()Lnet/neoforged/neoforge/common/ModConfigSpec$Builder;",
            &[builder],
        );
        let note = s(rt, "Seed");
        b(rt, comment, &[builder, note]);
        let name = s(rt, "seed");
        let seed_default = crate::mixin::box_value(rt, V::J(5_000_000_000), 'J');
        let seed = b(
            rt,
            "define.(Ljava/lang/String;Ljava/lang/Object;)Lnet/neoforged/neoforge/common/ModConfigSpec$ConfigValue;",
            &[builder, name, seed_default],
        );
        let spec = b(
            rt,
            "build.()Lnet/neoforged/neoforge/common/ModConfigSpec;",
            &[builder],
        );
        assert!(rt.thrown.is_none());
        Ruby {
            spec,
            dirt,
            magic,
            items,
            speed,
            slot,
            seed,
        }
    }

    /// The file NeoForge writes for [`ruby_spec`]'s spec: each value under its comments, the builder's
    /// ` Default:` and ` Range:` lines and the constants an enum allows among them, the values
    /// before the section, which is indented a tab.
    const DEFAULT_FILE: &str = "#Whether to log the dirt block on common setup\nlogDirtBlock = true\n#A magic number\n# Default: 42\n# Range: > 0\nmagicNumber = 42\n#What you want the introduction message to be for the magic number\nmagicNumberIntroduction = \"The magic number is... \"\n#A list of items to log on common setup.\nitems = [\"minecraft:iron_ingot\"]\n#Seed\nseed = 5000000000\n\n#Machines\n[machines]\n\t#How fast\n\t#in RPM\n\t# Default: 0.5\n\t# Range: 0.0 ~ 1.0\n\tspeed = 0.5\n\t#Allowed Values: CHEST, HEAD\n\tslot = \"HEAD\"\n";

    /// A game folder of the test's own, which config files go in.
    fn game(rt: &mut Rt, test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "horizon-compat-configs-{test}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let v = rt.string(&dir.to_string_lossy());
        rt.statics.insert(crate::files::GAME_DIR, v);
        dir
    }

    /// Another runtime of the same game (the server's beside the client's, the next start's).
    fn same_game(rt: &mut Rt, dir: &Path) {
        let v = rt.string(&dir.to_string_lossy());
        rt.statics.insert(crate::files::GAME_DIR, v);
    }

    fn file(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
    }

    /// Registers `spec` as `ruby`'s common config and returns the listener's events.
    fn register_common(rt: &mut Rt, spec: V) -> V {
        let events = jutil::new_list(rt, Vec::new());
        let listener = rt.lambda_id("java/util/function/Consumer", 3, vec![events]);
        rt.listeners.push((CONFIG_EVENT.to_owned(), listener));
        let container = crate::neo::mod_container(rt, "ruby");
        let common = crate::api::api_enum(rt, CONFIG_TYPE, "COMMON", 0);
        c(
            rt,
            "net/neoforged/fml/ModContainer.registerConfig.(Lnet/neoforged/fml/config/ModConfig$Type;Lnet/neoforged/fml/config/IConfigSpec;)V",
            &[container, common, spec],
        );
        events
    }

    fn value(rt: &mut Rt, v: V, name_desc: &str) -> V {
        c(rt, &format!("{CONFIG_VALUE}.{name_desc}"), &[v])
    }

    /// NeoForge's first start with a mod: no file yet, so the config is written from the spec, and
    /// the values read back are the defaults, boxed as Java boxes them.
    #[test]
    fn a_new_config_file_is_written_with_the_specs_comments_and_defaults() {
        let mut rt = Rt::new();
        crate::install();
        let dir = game(&mut rt, "new");
        let ruby = ruby_spec(&mut rt);
        let events = register_common(&mut rt, ruby.spec);
        assert_eq!(
            c(&mut rt, &format!("{SPEC}.isLoaded.()Z"), &[ruby.spec]),
            V::I(0)
        );
        load_pending(&mut rt);
        assert_eq!(file(&dir.join("config/ruby-common.toml")), DEFAULT_FILE);
        assert!(
            !dir.join("config/ruby-common-1.toml.bak").exists(),
            "nothing to back up"
        );
        assert_eq!(
            c(&mut rt, &format!("{SPEC}.isLoaded.()Z"), &[ruby.spec]),
            V::I(1)
        );
        let heard: Vec<&str> = jutil::elements(&rt, events)
            .iter()
            .map(|e| rt.class_of(*e))
            .collect();
        assert_eq!(heard, [LOADING]);
        assert_eq!(
            c(
                &mut rt,
                &format!("{BOOLEAN_VALUE}.getAsBoolean.()Z"),
                &[ruby.dirt]
            ),
            V::I(1)
        );
        let magic = value(&mut rt, ruby.magic, "get.()Ljava/lang/Object;");
        assert_eq!(
            (rt.class_of(magic), rt.get_field(magic, "value")),
            ("java/lang/Integer", V::I(42))
        );
        let speed = value(&mut rt, ruby.speed, "get.()Ljava/lang/Object;");
        assert_eq!(
            (rt.class_of(speed), rt.get_field(speed, "value")),
            ("java/lang/Double", V::D(0.5))
        );
        let seed = value(&mut rt, ruby.seed, "get.()Ljava/lang/Object;");
        assert_eq!(
            (rt.class_of(seed), rt.get_field(seed, "value")),
            ("java/lang/Long", V::J(5_000_000_000))
        );
        let slot = value(&mut rt, ruby.slot, "get.()Ljava/lang/Object;");
        assert_eq!(
            slot,
            crate::api::get_static(&mut rt, "net/minecraft/world/entity/EquipmentSlot.HEAD")
        );
        let items = value(&mut rt, ruby.items, "get.()Ljava/lang/Object;");
        let items: Vec<String> = jutil::elements(&rt, items)
            .iter()
            .map(|i| rt.str_of(*i).unwrap_or("").to_owned())
            .collect();
        assert_eq!(items, ["minecraft:iron_ingot"]);
        // A second runtime (the server's, beside the client's) reads the same file as it is.
        let mut other = Rt::new();
        same_game(&mut other, &dir);
        let theirs = ruby_spec(&mut other);
        register_common(&mut other, theirs.spec);
        load_pending(&mut other);
        assert_eq!(file(&dir.join("config/ruby-common.toml")), DEFAULT_FILE);
        assert!(!dir.join("config/ruby-common-1.toml.bak").exists());
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A player's edited file: a value of the wrong type is the default again, one out of range its
    /// nearest bound, a list keeps its valid elements, an unknown key goes, a missing one comes, the
    /// comments are the spec's; an enum's name in another case is kept. The file as it was is
    /// backed up first.
    #[test]
    fn a_wrong_file_is_backed_up_and_corrected() {
        let mut rt = Rt::new();
        crate::install();
        let dir = game(&mut rt, "wrong");
        let wrong = "#Whether to log the dirt block on common setup\nlogDirtBlock = \"maybe\"\n#A magic number\nmagicNumber = 999999999999\nmagicNumberIntroduction = \"Hi\"\n#A list of items to log on common setup.\nitems = [\"minecraft:iron_ingot\", 5, \"minecraft:gold_ingot\"]\nremoved = 1\n\n[machines]\n\tspeed = -3\n\tslot = \"chest\"\n";
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(dir.join("config/ruby-common.toml"), wrong).unwrap();
        let ruby = ruby_spec(&mut rt);
        register_common(&mut rt, ruby.spec);
        load_pending(&mut rt);
        assert_eq!(file(&dir.join("config/ruby-common-1.toml.bak")), wrong);
        let corrected = "#Whether to log the dirt block on common setup\nlogDirtBlock = true\n#A magic number\n# Default: 42\n# Range: > 0\nmagicNumber = 2147483647\n#What you want the introduction message to be for the magic number\nmagicNumberIntroduction = \"Hi\"\n#A list of items to log on common setup.\nitems = [\"minecraft:iron_ingot\", \"minecraft:gold_ingot\"]\n#Seed\nseed = 5000000000\n\n#Machines\n[machines]\n\t#How fast\n\t#in RPM\n\t# Default: 0.5\n\t# Range: 0.0 ~ 1.0\n\tspeed = 0.0\n\t#Allowed Values: CHEST, HEAD\n\tslot = \"chest\"\n";
        assert_eq!(file(&dir.join("config/ruby-common.toml")), corrected);
        assert_eq!(
            c(&mut rt, &format!("{INT_VALUE}.getAsInt.()I"), &[ruby.magic]),
            V::I(i32::MAX)
        );
        assert_eq!(
            c(
                &mut rt,
                &format!("{DOUBLE_VALUE}.getAsDouble.()D"),
                &[ruby.speed]
            ),
            V::D(0.0)
        );
        let slot = value(&mut rt, ruby.slot, "get.()Ljava/lang/Object;");
        assert_eq!(
            slot,
            crate::api::get_static(&mut rt, "net/minecraft/world/entity/EquipmentSlot.CHEST")
        );
        // Corrected again (by the next start): one more backup, the older one moved up.
        std::fs::write(dir.join("config/ruby-common.toml"), "magicNumber = -1\n").unwrap();
        let mut again = Rt::new();
        same_game(&mut again, &dir);
        let theirs = ruby_spec(&mut again);
        register_common(&mut again, theirs.spec);
        load_pending(&mut again);
        assert_eq!(
            file(&dir.join("config/ruby-common-1.toml.bak")),
            "magicNumber = -1\n"
        );
        assert_eq!(file(&dir.join("config/ruby-common-2.toml.bak")), wrong);
        assert_eq!(
            c(
                &mut again,
                &format!("{INT_VALUE}.getAsInt.()I"),
                &[theirs.magic]
            ),
            V::I(0)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file that isn't TOML is backed up and written again; a missing one is copied from
    /// `defaultconfigs/` when the pack ships one there.
    #[test]
    fn a_broken_file_is_made_again_and_a_packs_default_is_copied() {
        let mut rt = Rt::new();
        crate::install();
        let dir = game(&mut rt, "broken");
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(dir.join("config/ruby-common.toml"), "this = = isn't toml\n").unwrap();
        let ruby = ruby_spec(&mut rt);
        register_common(&mut rt, ruby.spec);
        load_pending(&mut rt);
        assert_eq!(
            file(&dir.join("config/ruby-common-1.toml.bak")),
            "this = = isn't toml\n"
        );
        assert_eq!(file(&dir.join("config/ruby-common.toml")), DEFAULT_FILE);

        let mut pack = Rt::new();
        let packed = game(&mut pack, "pack");
        std::fs::create_dir_all(packed.join("defaultconfigs")).unwrap();
        let shipped = DEFAULT_FILE.replace("magicNumber = 42", "magicNumber = 12");
        std::fs::write(packed.join("defaultconfigs/ruby-common.toml"), &shipped).unwrap();
        let theirs = ruby_spec(&mut pack);
        register_common(&mut pack, theirs.spec);
        load_pending(&mut pack);
        assert_eq!(file(&packed.join("config/ruby-common.toml")), shipped);
        assert_eq!(
            c(
                &mut pack,
                &format!("{INT_VALUE}.getAsInt.()I"),
                &[theirs.magic]
            ),
            V::I(12)
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&packed);
    }

    /// `set` is read at once unless the value waits for a restart (until a reload of its kind);
    /// `save` writes the file and tells the mod (`Reloading`).
    #[test]
    fn set_values_are_saved_to_the_file_and_the_mod_is_told() {
        let mut rt = Rt::new();
        crate::install();
        let dir = game(&mut rt, "save");
        let ruby = ruby_spec(&mut rt);
        let events = register_common(&mut rt, ruby.spec);
        load_pending(&mut rt);
        assert_eq!(
            c(
                &mut rt,
                &format!("{DOUBLE_VALUE}.getAsDouble.()D"),
                &[ruby.speed]
            ),
            V::D(0.5)
        );
        let seven = crate::mixin::box_value(&mut rt, V::I(7), 'I');
        c(
            &mut rt,
            &format!("{CONFIG_VALUE}.set.(Ljava/lang/Object;)V"),
            &[ruby.magic, seven],
        );
        assert_eq!(
            c(&mut rt, &format!("{INT_VALUE}.getAsInt.()I"), &[ruby.magic]),
            V::I(7)
        );
        let quarter = crate::mixin::box_value(&mut rt, V::D(0.25), 'D');
        c(
            &mut rt,
            &format!("{CONFIG_VALUE}.set.(Ljava/lang/Object;)V"),
            &[ruby.speed, quarter],
        );
        assert_eq!(
            c(
                &mut rt,
                &format!("{DOUBLE_VALUE}.getAsDouble.()D"),
                &[ruby.speed]
            ),
            V::D(0.5),
            "a world restart away"
        );
        c(&mut rt, &format!("{SPEC}.save.()V"), &[ruby.spec]);
        let saved = DEFAULT_FILE
            .replace("magicNumber = 42", "magicNumber = 7")
            .replace("speed = 0.5", "speed = 0.25");
        assert_eq!(file(&dir.join("config/ruby-common.toml")), saved);
        let heard: Vec<&str> = jutil::elements(&rt, events)
            .iter()
            .map(|e| rt.class_of(*e))
            .collect();
        assert_eq!(heard, [LOADING, RELOADING]);
        c(&mut rt, &format!("{SPEC}.afterReload.()V"), &[ruby.spec]);
        assert_eq!(
            c(
                &mut rt,
                &format!("{DOUBLE_VALUE}.getAsDouble.()D"),
                &[ruby.speed]
            ),
            V::D(0.5)
        );
        let world = restart(&mut rt, "WORLD");
        c(
            &mut rt,
            &format!("{SPEC}.resetCaches.({RESTART_DESC})V"),
            &[ruby.spec, world],
        );
        assert_eq!(
            c(
                &mut rt,
                &format!("{DOUBLE_VALUE}.getAsDouble.()D"),
                &[ruby.speed]
            ),
            V::D(0.25)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    const RESTART_DESC: &str = "Lnet/neoforged/neoforge/common/ModConfigSpec$RestartType;";

    /// Without a game folder (the probe): the config is its defaults in memory, it has no file, and
    /// saving writes nothing but still tells the mod.
    #[test]
    fn without_a_game_folder_a_config_is_its_defaults_in_memory() {
        let mut rt = Rt::new();
        crate::install();
        let ruby = ruby_spec(&mut rt);
        let events = register_common(&mut rt, ruby.spec);
        let before = c(&mut rt, &format!("{INT_VALUE}.getAsInt.()I"), &[ruby.magic]);
        let thrown = rt.thrown.take().map(|e| rt.class_of(e));
        assert_eq!(
            (before, thrown),
            (V::I(0), Some("java/lang/IllegalStateException")),
            "not loaded yet"
        );
        load_pending(&mut rt);
        assert_eq!(
            c(&mut rt, &format!("{INT_VALUE}.getAsInt.()I"), &[ruby.magic]),
            V::I(42)
        );
        let configs = {
            let id = rt.string("ruby");
            c(
                &mut rt,
                "net/neoforged/fml/config/ModConfigs.getModConfigs.(Ljava/lang/String;)Ljava/util/List;",
                &[id],
            )
        };
        let config = jutil::elements(&rt, configs)[0];
        c(
            &mut rt,
            "net/neoforged/fml/config/ModConfig.getFullPath.()Ljava/nio/file/Path;",
            &[config],
        );
        let thrown = rt.thrown.take().map(|e| rt.class_of(e));
        assert_eq!(thrown, Some("java/lang/IllegalStateException"), "no file");
        let loaded = c(
            &mut rt,
            "net/neoforged/fml/config/ModConfig.getLoadedConfig.()Lnet/neoforged/fml/config/IConfigSpec$ILoadedConfig;",
            &[config],
        );
        let values = c(
            &mut rt,
            "net/neoforged/fml/config/IConfigSpec$ILoadedConfig.config.()Lcom/electronwill/nightconfig/core/CommentedConfig;",
            &[loaded],
        );
        let path = rt.string("machines.speed");
        let speed = c(
            &mut rt,
            "com/electronwill/nightconfig/core/CommentedConfig.get.(Ljava/lang/String;)Ljava/lang/Object;",
            &[values, path],
        );
        assert_eq!(rt.get_field(speed, "value"), V::D(0.5));
        let comment = c(
            &mut rt,
            "com/electronwill/nightconfig/core/CommentedConfig.getComment.(Ljava/lang/String;)Ljava/lang/String;",
            &[values, path],
        );
        assert_eq!(
            rt.str_of(comment),
            Some("How fast\nin RPM\n Default: 0.5\n Range: 0.0 ~ 1.0")
        );
        c(&mut rt, &format!("{SPEC}.save.()V"), &[ruby.spec]);
        let heard: Vec<&str> = jutil::elements(&rt, events)
            .iter()
            .map(|e| rt.class_of(*e))
            .collect();
        assert_eq!(heard, [LOADING, RELOADING]);
        let ty = crate::api::api_enum(&mut rt, CONFIG_TYPE, "COMMON", 0);
        let ext = c(
            &mut rt,
            "net/neoforged/fml/config/ModConfig$Type.extension.()Ljava/lang/String;",
            &[ty],
        );
        assert_eq!(rt.str_of(ext), Some("common"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// What a mod's config screen walks: the spec's tree of value specs (comment, translation key,
    /// restart type, range, class, validation and correction), the values' tree, the levels'
    /// comments; and the builder's mistakes, which throw as NeoForge's do.
    #[test]
    fn the_spec_describes_each_value_and_level() {
        let mut rt = Rt::new();
        crate::install();
        let ruby = ruby_spec(&mut rt);
        let specs = c(
            &mut rt,
            &format!("{SPEC}.getSpec.()Lcom/electronwill/nightconfig/core/UnmodifiableConfig;"),
            &[ruby.spec],
        );
        let path = rt.string("machines.speed");
        let speed = c(
            &mut rt,
            "com/electronwill/nightconfig/core/UnmodifiableConfig.get.(Ljava/lang/String;)Ljava/lang/Object;",
            &[specs, path],
        );
        let vs = |rt: &mut Rt, name_desc: &str, args: &[V]| {
            let mut all = vec![speed];
            all.extend_from_slice(args);
            c(rt, &format!("{VALUE_SPEC}.{name_desc}"), &all)
        };
        let key = vs(&mut rt, "getTranslationKey.()Ljava/lang/String;", &[]);
        assert_eq!(rt.str_of(key), Some("ruby.config.speed"));
        let world = vs(&mut rt, &format!("restartType.(){RESTART_DESC}"), &[]);
        assert_eq!(world, restart(&mut rt, "WORLD"));
        let range = vs(
            &mut rt,
            "getRange.()Lnet/neoforged/neoforge/common/ModConfigSpec$Range;",
            &[],
        );
        let max = c(
            &mut rt,
            &format!("{RANGE}.getMax.()Ljava/lang/Comparable;"),
            &[range],
        );
        assert_eq!(rt.get_field(max, "value"), V::D(1.0));
        let class = vs(&mut rt, "getClazz.()Ljava/lang/Class;", &[]);
        assert_eq!(
            rt.str_of(class),
            Some("java/lang/Object"),
            "NeoForge's range values are specs of Object"
        );
        let class = c(
            &mut rt,
            &format!("{RANGE}.getClazz.()Ljava/lang/Class;"),
            &[range],
        );
        assert_eq!(rt.str_of(class), Some("java/lang/Double"));
        let two = crate::mixin::box_value(&mut rt, V::D(2.0), 'D');
        assert_eq!(vs(&mut rt, "test.(Ljava/lang/Object;)Z", &[two]), V::I(0));
        let fixed = vs(
            &mut rt,
            "correct.(Ljava/lang/Object;)Ljava/lang/Object;",
            &[two],
        );
        assert_eq!(rt.get_field(fixed, "value"), V::D(1.0));
        let text = rt.string("fast");
        let fixed = vs(
            &mut rt,
            "correct.(Ljava/lang/Object;)Ljava/lang/Object;",
            &[text],
        );
        assert_eq!(
            rt.get_field(fixed, "value"),
            V::D(0.5),
            "not a number: the default"
        );
        let values = c(
            &mut rt,
            &format!("{SPEC}.getValues.()Lcom/electronwill/nightconfig/core/UnmodifiableConfig;"),
            &[ruby.spec],
        );
        assert_eq!(
            c(
                &mut rt,
                "com/electronwill/nightconfig/core/UnmodifiableConfig.get.(Ljava/lang/String;)Ljava/lang/Object;",
                &[values, path]
            ),
            ruby.speed
        );
        let level = {
            let m = rt.string("machines");
            jutil::new_list(&mut rt, vec![m])
        };
        let note = c(
            &mut rt,
            &format!("{SPEC}.getLevelComment.(Ljava/util/List;)Ljava/lang/String;"),
            &[ruby.spec, level],
        );
        assert_eq!(rt.str_of(note), Some("Machines"));
        let walked = value(&mut rt, ruby.speed, "getPath.()Ljava/util/List;");
        let walked: Vec<String> = jutil::elements(&rt, walked)
            .iter()
            .map(|k| rt.str_of(*k).unwrap_or("").to_owned())
            .collect();
        assert_eq!(walked, ["machines", "speed"]);
        // Correcting an empty config adds every level and value, each heard by the listener.
        let heard = jutil::new_list(&mut rt, Vec::new());
        let listener = rt.lambda_id(
            "com/electronwill/nightconfig/core/ConfigSpec$CorrectionListener",
            3,
            vec![heard],
        );
        let empty = nightconfig::new_config(&mut rt, COMMENTED, None);
        let n = c(
            &mut rt,
            &format!(
                "{SPEC}.correct.(Lcom/electronwill/nightconfig/core/CommentedConfig;Lcom/electronwill/nightconfig/core/ConfigSpec$CorrectionListener;)I"
            ),
            &[ruby.spec, empty, listener],
        );
        assert_eq!(n, V::I(8), "seven values and a level");
        assert_eq!(jutil::elements(&rt, heard).len(), 8);
        assert_eq!(
            c(
                &mut rt,
                &format!(
                    "{SPEC}.isCorrect.(Lcom/electronwill/nightconfig/core/UnmodifiableCommentedConfig;)Z"
                ),
                &[ruby.spec, empty]
            ),
            V::I(1)
        );
        // Ranges as their comments show them.
        for (min, max, shown) in [
            (1, i32::MAX, "> 1"),
            (i32::MIN, 5, "< 5"),
            (0, 10, "0 ~ 10"),
        ] {
            let r = c(
                &mut rt,
                &format!("{RANGE}.of.(II)L{RANGE};"),
                &[V::I(min), V::I(max)],
            );
            let t = c(
                &mut rt,
                &format!("{RANGE}.toString.()Ljava/lang/String;"),
                &[r],
            );
            assert_eq!(rt.str_of(t), Some(shown));
        }
        // A restart type left over when a section starts, and a pop past the top.
        let builder = rt.new_object(BUILDER);
        b(
            &mut rt,
            "worldRestart.()Lnet/neoforged/neoforge/common/ModConfigSpec$Builder;",
            &[builder],
        );
        let name = rt.string("late");
        b(
            &mut rt,
            "push.(Ljava/lang/String;)Lnet/neoforged/neoforge/common/ModConfigSpec$Builder;",
            &[builder, name],
        );
        let e = rt.thrown.take().unwrap();
        assert_eq!(
            (rt.class_of(e), rt.str_of(rt.get_field(e, "message"))),
            (
                "java/lang/IllegalStateException",
                Some("Dangling restart value set to WORLD")
            )
        );
        b(
            &mut rt,
            "pop.(I)Lnet/neoforged/neoforge/common/ModConfigSpec$Builder;",
            &[builder, V::I(2)],
        );
        let e = rt.thrown.take().unwrap();
        assert_eq!(
            (rt.class_of(e), rt.str_of(rt.get_field(e, "message"))),
            (
                "java/lang/IllegalArgumentException",
                Some("Attempted to pop 2 elements when we only had: [late]")
            )
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// TOML as Night Config reads it: comments (before an entry, after it on its line, before a
    /// table's header), dotted keys, quoted keys, inline tables, arrays of tables, strings of each
    /// kind with their escapes, numbers of each base, `inf` and `nan`, longs; and what it refuses.
    #[test]
    fn toml_reads_as_night_config_reads_it() {
        use toml::Value as T;
        let t = toml::parse(
            "# about a\na = 1 # and more\n\"b c\".d = 'lit'\ne = [1, 0x1F, 0o17, 0b11, 1_000, -2, +3, 9999999999]\nf = [ 1.5, -0.0, 1e3, inf, -inf ]\ng = \"tab\\tq\\\"\\u00e9\"\nh = \"\"\"\nline\\\n   joined\"\"\"\ni = { x = true, y.z = false }\n\n#about the table\n[t.u] # after\nv = 2\n[[w]]\nn = 1\n[[w]]\nn = 2\n",
        )
        .unwrap();
        assert_eq!(t.get("a"), Some(&T::Int(1)));
        assert_eq!(t.comment("a"), Some(" about a\n and more"));
        let Some(T::Table(bc)) = t.get("b c") else {
            panic!("{t:?}")
        };
        assert_eq!(bc.get("d"), Some(&T::Str("lit".into())));
        assert_eq!(
            t.get("e"),
            Some(&T::Array(vec![
                T::Int(1),
                T::Int(31),
                T::Int(15),
                T::Int(3),
                T::Int(1000),
                T::Int(-2),
                T::Int(3),
                T::Long(9_999_999_999)
            ]))
        );
        let Some(T::Array(f)) = t.get("f") else {
            panic!()
        };
        assert_eq!(f[..3], [T::Double(1.5), T::Double(-0.0), T::Double(1000.0)]);
        assert_eq!(
            f[3..],
            [T::Double(f64::INFINITY), T::Double(f64::NEG_INFINITY)]
        );
        assert_eq!(t.get("g"), Some(&T::Str("tab\tq\"é".into())));
        assert_eq!(t.get("h"), Some(&T::Str("linejoined".into())));
        let Some(T::Table(i)) = t.get("i") else {
            panic!()
        };
        assert_eq!(i.get("x"), Some(&T::Bool(true)));
        let Some(T::Table(tt)) = t.get("t") else {
            panic!()
        };
        assert_eq!(tt.comment("u"), Some("about the table\n after"));
        let Some(T::Table(u)) = tt.get("u") else {
            panic!()
        };
        assert_eq!(u.get("v"), Some(&T::Int(2)));
        let Some(T::Array(w)) = t.get("w") else {
            panic!()
        };
        assert_eq!(w.len(), 2);
        for bad in [
            "a = 01\n",
            "a = 1\na = 2\n",
            "a = \"open\n",
            "a = [1,,2]\n",
            "a = { b = 1 }\n[a]\nc = 2\n",
            "[x]\nk = 1\n[x]\nk = 2\n",
            "a = 1_\n",
            "a b = 1\n",
        ] {
            assert!(toml::parse(bad).is_err(), "{bad:?}");
        }
    }

    /// TOML as Night Config writes it: values before tables, a tab per level, a level holding only
    /// tables left out (unless it has a comment), arrays of tables, strings escaped (a multi-line
    /// one in triple quotes), Java's doubles, `nan` and `inf`, an empty table inline.
    #[test]
    fn toml_writes_as_night_config_writes_it() {
        use toml::{Table, Value as T};
        let table = |entries: Vec<(&str, T)>, comments: Vec<(&str, &str)>| Table {
            entries: entries
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v))
                .collect(),
            comments: comments
                .into_iter()
                .map(|(k, c)| (k.to_owned(), c.to_owned()))
                .collect(),
            inline: false,
        };
        let deep = table(vec![("x", T::Double(1e-5))], vec![("x", "tiny")]);
        let only_tables = table(vec![("deep", T::Table(deep))], vec![]);
        let inner = table(vec![("y", T::Int(1))], vec![]);
        let noted = table(vec![("inner", T::Table(inner))], vec![]);
        let item = |n: i32| T::Table(table(vec![("n", T::Int(n))], vec![]));
        let root = table(
            vec![
                ("a b", T::Str("say \"hi\"\\".into())),
                ("text", T::Str("one\ntwo".into())),
                (
                    "nums",
                    T::Array(vec![
                        T::Double(f64::NAN),
                        T::Double(f64::NEG_INFINITY),
                        T::Float(0.1),
                        T::Long(1 << 40),
                    ]),
                ),
                ("empty", T::Table(Table::default())),
                ("outer", T::Table(only_tables)),
                ("noted", T::Table(noted)),
                ("list", T::Array(vec![item(1), item(2)])),
                ("flag", T::Bool(false)),
            ],
            vec![("a b", "first\nsecond"), ("noted", "kept")],
        );
        let expected = "#first\n#second\n\"a b\" = \"say \\\"hi\\\"\\\\\"\ntext = \"\"\"\none\ntwo\"\"\"\nnums = [nan, -inf, 0.1, 1099511627776]\nempty = {}\nflag = false\n\n\t[outer.deep]\n\t\t#tiny\n\t\tx = 1.0E-5\n\n#kept\n[noted]\n\n\t[noted.inner]\n\t\ty = 1\n\n[[list]]\n\tn = 1\n[[list]]\n\tn = 2\n";
        assert_eq!(toml::write(&root, "\n"), expected);
        // What it writes reads back as it was.
        let back = toml::parse(expected).unwrap();
        assert_eq!(back.get("text"), Some(&T::Str("one\ntwo".into())));
        assert_eq!(back.get("a b"), Some(&T::Str("say \"hi\"\\".into())));
        assert_eq!(back.comment("a b"), Some("first\nsecond"));
        assert_eq!(back.comment("noted"), Some("kept"));
        let Some(T::Table(outer)) = back.get("outer") else {
            panic!("{back:?}")
        };
        let Some(T::Table(deep)) = outer.get("deep") else {
            panic!("{outer:?}")
        };
        assert_eq!(deep.comment("x"), Some("tiny"));
    }
}
