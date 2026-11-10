//! Constants mods add to the game's enums.
//!
//! NeoForge's way: a jar lists them in the file its `neoforge.mods.toml` names
//! (`enumExtensions = "META-INF/enumextensions.json"`), each with the enum, the constant's name,
//! the constructor and where its arguments are: a list, or a static field of the mod's that holds
//! an `EnumProxy` (or a list). NeoForge adds them as the enum's class loads, and the mod takes
//! its constant from the proxy (`EnumProxy.getValue()`) or by name (`valueOf`). The translator
//! carries those files (`hz/enum-extensions/<n>.json` among [`crate::FILES`]).
//!
//! The older way: a mixin into the enum that shadows its `$VALUES`, has an `@Invoker("<init>")`
//! and injects a handler into the enum's static initialiser, which makes the constants with the
//! invoker and puts a longer array into `$VALUES` (Cataclysm's note block instruments, Steam 'n'
//! Rails' minecart types). The translator has such a handler listen for its place
//! (`hz/inject:<enum>.<clinit>@TAIL`); it runs here the first time the enum's constants are
//! asked for, and each constant its invoker makes joins the enum's.
//!
//! Either way `values()` and `valueOf` of the enum have the new constants after the game's own,
//! with the ordinals that follow: without them a block registered with one of Cataclysm's
//! instruments was not registered at all ("No enum constant ...NoteBlockInstrument.KOBOLEDIATOR").
//! What a constant was constructed with is kept ([`params`]) for the enum's own methods to
//! answer from.
use crate::{Data, Rt, V};

const PROXY: &str = "net/neoforged/fml/common/asm/enumextension/EnumProxy";
/// The enums that may have constants of the mods' in this runtime (`Rt::interned`): their class
/// names, one a line.
const CLASSES: &str = "compat:enum-ext";
/// What the jars declare (`Rt::interned`): a list of entries ([`ENTRY`]).
const ENTRIES: &str = "compat:enum-ext-entries";
/// One declared constant: `enum`, `name`, `desc` (its constructor), and its arguments, either
/// `params` (a list) or `class` and `field` (the static that holds them).
const ENTRY: &str = "compat:enum-entry";

/// The proxy's calls, and any constructor that may be an enum's own (`Enum(String name, int
/// ordinal, ...)`, which only a mixin's `@Invoker("<init>")` reaches): [`call`] takes it when its
/// class is one ([`own`]).
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    owner == PROXY
        || (name == "<init>" && desc.starts_with("(Ljava/lang/String;I"))
        || (owner == RARITY && matches!(name, "getStyleModifier" | "color"))
}

const RARITY: &str = "net/minecraft/world/item/Rarity";
/// The colour of each of the game's rarities, in their order.
const RARITY_COLORS: [&str; 4] = ["WHITE", "YELLOW", "AQUA", "LIGHT_PURPLE"];
const WITH_COLOR: &str = "net/minecraft/network/chat/Style.withColor.(Lnet/minecraft/ChatFormatting;)Lnet/minecraft/network/chat/Style;";

/// The name in data of a constant a mod added to an enum whose constructor takes one
/// (`Rarity(int id, String name, ...)`, `ItemDisplayContext(int id, String name, String
/// fallback)`): the name it was constructed with. None for any other constant.
pub(crate) fn serialized_name(rt: &mut Rt, constant: V) -> Option<V> {
    if !matches!(
        rt.class_of(constant),
        RARITY | "net/minecraft/world/item/ItemDisplayContext"
    ) {
        return None;
    }
    params(rt, constant)?
        .get(1)
        .copied()
        .filter(|name| rt.str_of(*name).is_some())
}

/// What a rarity says of itself: one of the game's by its place among them (a colour, and a
/// style that sets it), one a mod added by what it was constructed with (`Rarity(int id, String
/// name, UnaryOperator<Style> styleModifier)`: its own style, and black for a colour, as
/// NeoForge's constructor leaves it).
fn rarity_call(rt: &mut Rt, name: &str, this: V) -> Option<V> {
    let formatting = |rt: &mut Rt, color: &str| {
        let key = rt.static_key(&format!("net/minecraft/ChatFormatting.{color}"));
        crate::api::get_static(rt, key)
    };
    let own = RARITY_COLORS[rt.get_field(this, "$ordinal").i().clamp(0, 3) as usize];
    Some(match (name, params(rt, this)) {
        ("getStyleModifier", Some(given)) => given.get(2).copied().unwrap_or(V::NULL),
        ("color", Some(_)) => formatting(rt, "BLACK"),
        ("color", None) => formatting(rt, own),
        ("getStyleModifier", None) => {
            let color = formatting(rt, own);
            crate::functions::with(rt, WITH_COLOR, color)
        }
        _ => return None,
    })
}

/// How many constants of its own enum `class` of the game's has: by the runtime's tables, else
/// by the class the translator carried. `None` for a class that is no enum.
fn own(rt: &mut Rt, class: &str) -> Option<usize> {
    match crate::api_common::enum_names(class) {
        Some((_, names)) => Some(names.len()),
        None => crate::api_common::carried_enum(rt, class).map(|(_, names)| names.len()),
    }
}

/// The enums with constants of the mods' ([`CLASSES`]), found once for the runtime: those the
/// jars' files declare ([`ENTRIES`]) and those a mixin's handler listens at the static
/// initialiser of.
fn classes(rt: &mut Rt) -> V {
    if let Some(&known) = rt.interned.get(CLASSES) {
        return known;
    }
    // Before the files are read: reading them asks nothing of an enum, but what it asks must
    // not come back here.
    let none = rt.string("");
    rt.interned.insert(CLASSES.into(), none);
    let mut classes: Vec<String> = Vec::new();
    let mut entries = Vec::new();
    for n in 0.. {
        let Some(bytes) = crate::resources::jar_file(rt, &format!("hz/enum-extensions/{n}.json"))
        else {
            break;
        };
        // Gson's lenient reader, as NeoForge reads them: the files carry comments.
        let Ok(Some((tree, _))) = crate::gson::parse(rt, &String::from_utf8_lossy(&bytes), true)
        else {
            continue;
        };
        let json = crate::gson::to_serde(rt, tree);
        for e in json
            .get("entries")
            .and_then(|e| e.as_array())
            .into_iter()
            .flatten()
        {
            let text = |k: &str| e.get(k).and_then(|v| v.as_str()).unwrap_or("").to_owned();
            let (class, name, desc) = (text("enum"), text("name"), text("constructor"));
            if class.is_empty() || name.is_empty() {
                continue;
            }
            let entry = rt.new_object(ENTRY);
            for (field, value) in [("enum", &class), ("name", &name), ("desc", &desc)] {
                let v = rt.string(value);
                rt.put_field(entry, field, v);
            }
            match e.get("parameters") {
                Some(serde_json::Value::Array(list)) => {
                    let kinds = crate::neo::desc_params(&desc);
                    let values: Vec<V> = list
                        .iter()
                        .enumerate()
                        .map(|(i, j)| constant(rt, j, kinds.get(i).map_or("", String::as_str)))
                        .collect();
                    let params = crate::jutil::new_list(rt, values);
                    rt.put_field(entry, "params", params);
                }
                Some(held) => {
                    for field in ["class", "field"] {
                        let v = rt.string(held.get(field).and_then(|v| v.as_str()).unwrap_or(""));
                        rt.put_field(entry, field, v);
                    }
                }
                None => {}
            }
            entries.push(entry);
            if !classes.contains(&class) {
                classes.push(class);
            }
        }
    }
    const PLACE: &str = "hz/inject:";
    for (event, ..) in rt.subscribers.values().flatten() {
        if let Some(class) = event
            .strip_prefix(PLACE)
            .and_then(|e| e.split_once(".<clinit>@"))
            .map(|(c, _)| c)
            && !classes.iter().any(|c| c == class)
        {
            classes.push(class.to_owned());
        }
    }
    let entries = crate::jutil::new_list(rt, entries);
    rt.interned.insert(ENTRIES.into(), entries);
    let known = rt.string(&classes.join("\n"));
    rt.interned.insert(CLASSES.into(), known);
    known
}

/// A constant argument of a declared constant, as its constructor's parameter of descriptor
/// `kind` takes it.
fn constant(rt: &mut Rt, json: &serde_json::Value, kind: &str) -> V {
    match (json, kind) {
        (serde_json::Value::String(s), "C") => V::I(s.chars().next().map_or(0, |c| c as i32)),
        (serde_json::Value::String(s), _) => rt.string(s),
        (serde_json::Value::Bool(b), _) => V::I(*b as i32),
        (serde_json::Value::Number(n), "J") => V::J(n.as_i64().unwrap_or(0)),
        (serde_json::Value::Number(n), "F") => V::F(n.as_f64().unwrap_or(0.0) as f32),
        (serde_json::Value::Number(n), "D") => V::D(n.as_f64().unwrap_or(0.0)),
        (serde_json::Value::Number(n), _) => V::I(n.as_i64().unwrap_or(0) as i32),
        _ => V::NULL,
    }
}

/// Whether enum `class` may have constants of the mods'.
fn extended(rt: &mut Rt, class: &str) -> bool {
    let known = classes(rt);
    rt.str_of(known)
        .is_some_and(|all| all.lines().any(|c| c == class))
}

/// The list of the constants the mods added to enum `class`, made the first time it is asked
/// for: the declared ones, then those the mixins' handlers at the enum's static initialiser
/// make. `None` for an enum no mod extends.
fn list(rt: &mut Rt, class: &str) -> Option<V> {
    if !extended(rt, class) {
        return None;
    }
    let key = format!("compat:enum-ext/{class}");
    if let Some(&list) = rt.interned.get(&key) {
        return Some(list);
    }
    // Known before it is filled: a handler that fills it reads the enum's constants itself.
    let list = crate::jutil::new_list(rt, Vec::new());
    rt.interned.insert(key, list);
    let base = own(rt, class).unwrap_or(0);
    let class_key = rt.static_key(class);
    let entries = rt.interned.get(ENTRIES).copied().unwrap_or(V::NULL);
    for entry in crate::jutil::elements(rt, entries) {
        if rt.str_of(rt.get_field(entry, "enum")) != Some(class) {
            continue;
        }
        let name = rt
            .str_of(rt.get_field(entry, "name"))
            .unwrap_or("")
            .to_owned();
        let name = rt.static_key(&name);
        let ordinal = base + crate::jutil::elements(rt, list).len();
        let made = crate::api::api_enum(rt, class_key, name, ordinal as i32);
        rt.put_field(made, "$extension", entry);
        push(rt, list, made);
    }
    crate::neo::static_initialiser_places(rt, class);
    Some(list)
}

fn push(rt: &mut Rt, list: V, v: V) {
    if let Some(Data::List(items)) = rt.heap.get_mut(list.r() as usize).map(|o| &mut o.data) {
        items.push(v);
    }
}

/// The constants the mods added to enum `class`, in ordinal order after its own.
pub(crate) fn extras(rt: &mut Rt, class: &str) -> Vec<V> {
    match list(rt, class) {
        Some(list) => crate::jutil::elements(rt, list),
        None => Vec::new(),
    }
}

/// The constant of the mods' named `name` in enum `class`.
pub(crate) fn named(rt: &mut Rt, class: &str, name: &str) -> Option<V> {
    extras(rt, class)
        .into_iter()
        .find(|&c| rt.str_of(rt.get_field(c, "$name")) == Some(name))
}

/// What constant `constant` of the mods' was constructed with, after its name and ordinal: the
/// arguments its mixin's invoker passed, its declaration's list, or what the static field its
/// declaration names holds (an `EnumProxy`'s parameters, or a list). `None` for a constant of
/// the game's own.
pub(crate) fn params(rt: &mut Rt, constant: V) -> Option<Vec<V>> {
    let own = rt.get_field(constant, "$params");
    if own.r() != 0 {
        return Some(crate::jutil::elements(rt, own));
    }
    let entry = rt.get_field(constant, "$extension");
    if entry.r() == 0 {
        return None;
    }
    let listed = rt.get_field(entry, "params");
    if listed.r() != 0 {
        return Some(crate::jutil::elements(rt, listed));
    }
    let held = holder(rt, entry, true);
    let of_proxy = rt.get_field(held, "$params");
    Some(crate::jutil::elements(
        rt,
        if rt.class_of(held) == PROXY {
            of_proxy
        } else {
            held
        },
    ))
}

/// What the static field a declaration names holds; `init` first initialises its class.
fn holder(rt: &mut Rt, entry: V, init: bool) -> V {
    let text =
        |rt: &Rt, f: &'static str| rt.str_of(rt.get_field(entry, f)).unwrap_or("").to_owned();
    let (class, field) = (text(rt, "class"), text(rt, "field"));
    if class.is_empty() || field.is_empty() {
        return V::NULL;
    }
    if init {
        return crate::jlang::static_of(rt, &class, &field);
    }
    let key = rt.static_key(&format!("{class}.{field}"));
    rt.statics.get(key).copied().unwrap_or(V::NULL)
}

pub(crate) fn call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if owner == RARITY && name != "<init>" {
        return rarity_call(rt, name, a(0));
    }
    if owner != PROXY {
        if !(name == "<init>" && desc.starts_with("(Ljava/lang/String;I"))
            || own(rt, owner).is_none()
        {
            return None;
        }
        // A mixin's `@Invoker("<init>")`: the object it made is a constant of the enum from
        // here on, unless one of that name is there.
        let (this, name) = (a(0), rt.str_of(a(1)).unwrap_or("").to_owned());
        rt.put_field(this, "$name", a(1));
        rt.put_field(this, "$ordinal", V::I(a(2).i()));
        let rest = crate::jutil::new_list(rt, args.get(3..).unwrap_or(&[]).to_vec());
        rt.put_field(this, "$params", rest);
        let known = match rt.interned.get(CLASSES).copied() {
            Some(known) => known,
            None => classes(rt),
        };
        if !rt
            .str_of(known)
            .is_some_and(|all| all.lines().any(|c| c == owner))
        {
            let all = format!("{}\n{owner}", rt.str_of(known).unwrap_or(""));
            let all = rt.string(all.trim_start_matches('\n'));
            rt.interned.insert(CLASSES.into(), all);
        }
        if let Some(list) = list(rt, owner)
            && named(rt, owner, &name).is_none()
        {
            push(rt, list, this);
            rt.interned
                .entry(format!("enum/{owner}.{name}"))
                .or_insert(this);
        }
        return Some(V::Void);
    }
    Some(match name {
        // `new EnumProxy<>(Rarity.class, args...)`, or with the arguments as a list.
        "<init>" => {
            rt.put_field(a(0), "$enum", a(1));
            let params = crate::jutil::elements(rt, a(2));
            let params = crate::jutil::new_list(rt, params);
            rt.put_field(a(0), "$params", params);
            V::Void
        }
        "getParameter" => crate::jutil::elements(rt, rt.get_field(a(0), "$params"))
            .get(a(1).i().max(0) as usize)
            .copied()
            .unwrap_or(V::NULL),
        // The constant whose declaration names the field that holds this proxy.
        "getValue" => {
            let set = rt.get_field(a(0), "$value");
            if set.r() != 0 {
                return Some(set);
            }
            let class = rt
                .str_of(rt.get_field(a(0), "$enum"))
                .unwrap_or("")
                .replace('.', "/");
            for made in extras(rt, &class) {
                let entry = rt.get_field(made, "$extension");
                if entry.r() != 0 && holder(rt, entry, false) == a(0) {
                    rt.put_field(a(0), "$value", made);
                    return Some(made);
                }
            }
            V::NULL
        }
        "setValue" => {
            rt.put_field(a(0), "$value", a(1));
            V::Void
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RARITY: &str = "net/minecraft/world/item/Rarity";
    const ARM_POSE: &str = "net/minecraft/client/model/HumanoidModel$ArmPose";
    const INSTRUMENT: &str = "net/minecraft/world/level/block/state/properties/NoteBlockInstrument";
    const MINECART: &str = "net/minecraft/world/entity/vehicle/AbstractMinecart$Type";
    const DECLARED: &str = r#"{
  "entries": [
    {
      // The enum the constant is added to.
      "enum": "net/minecraft/world/item/Rarity",
      "name": "GEMS_RUBY",
      "constructor": "(ILjava/lang/String;Ljava/util/function/UnaryOperator;)V",
      "parameters": { "class": "t/Rarities", "field": "RUBY_PROXY" }
    },
    {
      "enum": "net/minecraft/client/model/HumanoidModel$ArmPose",
      "name": "GEMS_POLISH",
      "constructor": "(ZLjava/lang/String;I)V",
      "parameters": [true, "gems:polish", -1]
    }
  ]
}"#;

    /// The jars' files, as the bundle's table hands them out, and an enum the runtime lists
    /// nowhere, as the translator carries its class.
    fn files(rt: &mut Rt, class: &str, what: &str, _args: &[V]) -> Option<V> {
        match (class, what) {
            (crate::FILES, "hz/enum-extensions/0.json") => Some(crate::files::byte_array(rt, DECLARED.as_bytes())),
            ("hz/derived/net/minecraft/world/item/Rarity", "fields") => Some(rt.string(
                "16409 COMMON Lnet/minecraft/world/item/Rarity;\n16409 UNCOMMON Lnet/minecraft/world/item/Rarity;\n16409 RARE Lnet/minecraft/world/item/Rarity;\n16409 EPIC Lnet/minecraft/world/item/Rarity;\n4122 $VALUES [Lnet/minecraft/world/item/Rarity;",
            )),
            _ => None,
        }
    }

    fn names(rt: &mut Rt, class: &str) -> Vec<String> {
        let key = rt.static_key(&format!("{class}.values.()[L{class};"));
        let all = crate::api::call(rt, key, &[]);
        crate::jutil::elements(rt, all)
            .into_iter()
            .map(|c| {
                let n = crate::api::call(rt, "java/lang/Enum.name.()Ljava/lang/String;", &[c]);
                let ordinal = crate::api::call(rt, "java/lang/Enum.ordinal.()I", &[c]).i();
                format!("{}={ordinal}", rt.str_of(n).unwrap_or("?"))
            })
            .collect()
    }

    /// A constant a jar declares is among its enum's, after the game's own; the proxy in the
    /// field the declaration names is that constant, and so is the one found by name. An enum
    /// nobody extends is as it was.
    #[test]
    fn a_declared_constant_is_its_enums_and_its_proxys() {
        crate::install();
        let mut rt = Rt::new();
        rt.reflect = Some(files);
        assert_eq!(
            names(&mut rt, RARITY),
            ["COMMON=0", "UNCOMMON=1", "RARE=2", "EPIC=3", "GEMS_RUBY=4"]
        );
        // `static final EnumProxy<Rarity> RUBY_PROXY = new EnumProxy<>(Rarity.class, -1, "gems:ruby", style)`.
        let proxy = rt.new_object(PROXY);
        let class = rt.class_literal(RARITY);
        let style = rt.string("a style");
        let (id, name) = (
            crate::api_common::integer(&mut rt, -1),
            rt.string("gems:ruby"),
        );
        let array = rt.alloc("[Ljava/lang/Object;", Data::Array(vec![id, name, style]));
        crate::api::call(
            &mut rt,
            "net/neoforged/fml/common/asm/enumextension/EnumProxy.<init>.(Ljava/lang/Class;[Ljava/lang/Object;)V",
            &[proxy, class, array],
        );
        const GET: &str =
            "net/neoforged/fml/common/asm/enumextension/EnumProxy.getValue.()Ljava/lang/Enum;";
        assert_eq!(
            crate::api::call(&mut rt, GET, &[proxy]),
            V::NULL,
            "no declaration names a field that holds it"
        );
        rt.statics.insert("t/Rarities.RUBY_PROXY", proxy);
        let ruby = crate::api::call(&mut rt, GET, &[proxy]);
        let text = rt.string("GEMS_RUBY");
        let by_name = crate::api::call(
            &mut rt,
            "net/minecraft/world/item/Rarity.valueOf.(Ljava/lang/String;)Lnet/minecraft/world/item/Rarity;",
            &[text],
        );
        assert!(ruby.r() != 0 && ruby == by_name);
        assert_eq!(rt.class_of(ruby), RARITY);
        assert_eq!(params(&mut rt, ruby), Some(vec![id, name, style]));
        let epic = crate::api::get_static(&mut rt, "net/minecraft/world/item/Rarity.EPIC");
        assert_eq!(params(&mut rt, epic), None);
        // What a rarity says of itself: the mod's by what it was constructed with, the game's
        // by its colour (a style that sets it).
        let ask = |rt: &mut Rt, what: &str, rarity: V| {
            let key = rt.static_key(&format!("net/minecraft/world/item/Rarity.{what}"));
            crate::api::call(rt, key, &[rarity])
        };
        assert_eq!(
            ask(
                &mut rt,
                "getStyleModifier.()Ljava/util/function/UnaryOperator;",
                ruby
            ),
            style
        );
        let named = ask(&mut rt, "getSerializedName.()Ljava/lang/String;", ruby);
        assert_eq!(rt.str_of(named), Some("gems:ruby"));
        let named = ask(&mut rt, "getSerializedName.()Ljava/lang/String;", epic);
        assert_eq!(rt.str_of(named), Some("epic"));
        let (black, purple) = (
            crate::api::get_static(&mut rt, "net/minecraft/ChatFormatting.BLACK"),
            crate::api::get_static(&mut rt, "net/minecraft/ChatFormatting.LIGHT_PURPLE"),
        );
        assert_eq!(
            ask(&mut rt, "color.()Lnet/minecraft/ChatFormatting;", ruby),
            black
        );
        assert_eq!(
            ask(&mut rt, "color.()Lnet/minecraft/ChatFormatting;", epic),
            purple
        );
        // (A function of the runtime's own is called as generated code calls one.)
        rt.vcall = Some(|rt, key, _, args| crate::api::call(rt, key, args));
        let modifier = ask(
            &mut rt,
            "getStyleModifier.()Ljava/util/function/UnaryOperator;",
            epic,
        );
        let plain = crate::api::get_static(&mut rt, "net/minecraft/network/chat/Style.EMPTY");
        let styled = rt.call_functional(
            "java/util/function/UnaryOperator.apply.(Ljava/lang/Object;)Ljava/lang/Object;",
            modifier,
            &[plain],
        );
        assert_eq!(
            crate::api_common::style_json(&rt, styled),
            serde_json::json!({"color": "light_purple"})
        );
        // Constants given in the file itself, as the constructor takes them: of an enum the
        // runtime's own tables list.
        let own = crate::api_common::enum_names(ARM_POSE).unwrap().1.len();
        let poses = names(&mut rt, ARM_POSE);
        assert_eq!(
            (poses.len(), poses.last().cloned()),
            (own + 1, Some(format!("GEMS_POLISH={own}")))
        );
        let text = rt.string("GEMS_POLISH");
        let polish = crate::api::call(
            &mut rt,
            "net/minecraft/client/model/HumanoidModel$ArmPose.valueOf.(Ljava/lang/String;)Lnet/minecraft/client/model/HumanoidModel$ArmPose;",
            &[text],
        );
        let given = params(&mut rt, polish).unwrap();
        assert_eq!(
            (given[0], rt.str_of(given[1]), given[2]),
            (V::I(1), Some("gems:polish"), V::I(-1))
        );
        // An enum no jar extends, and a name nobody added.
        assert_eq!(names(&mut rt, MINECART).len(), 7);
        let text = rt.string("GEMS_EMERALD");
        crate::api::call(
            &mut rt,
            "net/minecraft/world/item/Rarity.valueOf.(Ljava/lang/String;)Lnet/minecraft/world/item/Rarity;",
            &[text],
        );
        let thrown = rt.thrown.take().expect("no such constant");
        assert_eq!(rt.class_of(thrown), "java/lang/IllegalArgumentException");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mixin's handler at the end of an enum's static initialiser makes constants with the
    /// enum's own constructor: it runs once, when the enum's constants are first asked for, and
    /// what it made is found by name, in `values()` and in the `$VALUES` the mixin shadows.
    #[test]
    fn a_mixin_adds_constants_at_the_enums_static_initialiser() {
        const PLACE: &str = "hz/inject:net/minecraft/world/level/block/state/properties/NoteBlockInstrument.<clinit>@TAIL";
        const INIT: &str = "net/minecraft/world/level/block/state/properties/NoteBlockInstrument.<init>.(Ljava/lang/String;ILjava/lang/String;Lnet/minecraft/core/Holder;Lnet/minecraft/world/level/block/state/properties/NoteBlockInstrument$Type;)V";
        const VALUES: &str =
            "net/minecraft/world/level/block/state/properties/NoteBlockInstrument.$VALUES";
        /// `addCustomInstruments(CallbackInfo)`: one more constant after `$VALUES`' last.
        fn handler(rt: &mut Rt, _id: u32, _args: &[V]) -> Option<V> {
            let runs = rt.statics.get("test:runs").map_or(0, |v| v.i());
            rt.statics.insert("test:runs", V::I(runs + 1));
            let all = crate::api::get_static(rt, VALUES);
            let count = crate::jutil::elements(rt, all).len() as i32;
            let made = rt.new_object(INSTRUMENT);
            let (name, text) = (rt.string("KOBOLEDIATOR"), rt.string("kobolediator"));
            crate::api::call(rt, INIT, &[made, name, V::I(count), text, V::NULL, V::NULL]);
            Some(V::Void)
        }
        crate::install();
        let mut rt = Rt::new();
        rt.lambda_call = Some(handler);
        rt.subscribers.insert(
            "t/NoteBlockInstrumentMixin",
            vec![(PLACE, "t/NoteBlockInstrumentMixin.add", true, 1)],
        );
        let own = crate::api_common::enum_names(INSTRUMENT).unwrap().1.len();
        let text = rt.string("KOBOLEDIATOR");
        let value_of = "net/minecraft/world/level/block/state/properties/NoteBlockInstrument.valueOf.(Ljava/lang/String;)Lnet/minecraft/world/level/block/state/properties/NoteBlockInstrument;";
        let made = crate::api::call(&mut rt, value_of, &[text]);
        assert!(rt.thrown.is_none() && made.r() != 0);
        assert_eq!(
            crate::api::call(&mut rt, "java/lang/Enum.ordinal.()I", &[made]).i(),
            own as i32
        );
        let all = names(&mut rt, INSTRUMENT);
        assert_eq!(
            (all.len(), all.last().map(String::as_str)),
            (own + 1, Some(format!("KOBOLEDIATOR={own}").as_str()))
        );
        let shadowed = crate::api::get_static(&mut rt, VALUES);
        assert_eq!(crate::jutil::elements(&rt, shadowed).last(), Some(&made));
        assert_eq!(rt.statics.get("test:runs"), Some(&V::I(1)));
        let given = params(&mut rt, made).unwrap();
        assert_eq!(rt.str_of(given[0]), Some("kobolediator"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
