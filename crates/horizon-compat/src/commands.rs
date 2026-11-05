//! Brigadier commands as a mod builds them (`RegisterCommandsEvent`), natively. A mod's tree of
//! literals and arguments is data here: its builders (`Commands.literal`, `argument`, `then`,
//! `executes`, `requires`, `redirect`) make nodes, and the dispatcher merges each root it registers
//! into its own, as Brigadier does. Then:
//!
//! - [`register`]: the game asks for the server's commands when a world opens to players. The
//!   mods hear `RegisterCommandsEvent` with a new dispatcher, and every runnable path of its tree
//!   becomes a usage of a native command (`create clone <begin:block_pos> <end:block_pos> ...`), at
//!   the operator level its nodes' requirements ask (each requirement is tried with a source of
//!   level 0 to 4). An argument's kind is its vanilla type's parser, so a player's chat box
//!   completes and checks it as vanilla's.
//!   A mod's own argument type is shown as what its examples look like (ids, words or strings).
//! - [`perform`]: a player's command line is parsed as Brigadier 1.3.10 parses it (`parseNodes`:
//!   the literal the next word names, or else each argument the source may use, read by its type
//!   with a `StringReader`; a redirect goes on at its target in a context of its own), then run as
//!   vanilla 1.21.1 runs it (`Commands.performCommand`): NeoForge's `CommandEvent` first, then a
//!   parse error answered with its message and the line up to its place, else each redirect's
//!   modifier giving the sources the command runs for (a fork's errors silent), the command's own
//!   syntax errors said to its source and any other exception as `command.failed`.
//!
//! The vanilla argument types read as their parsers do ([`crate::command_args`]), failing with
//! their errors; a mod's own `ArgumentType` reads with its own `parse`, on a `StringReader` object
//! ([`crate::brigadier`]). A mod that parses or runs a line itself (`dispatcher.parse`,
//! `execute`) gets Brigadier's `ParseResults`, contexts and exceptions.
use crate::brigadier::{self, unit, Arg, Reader, Syntax};
use crate::{api, command_args, Data, Rt, V};

const DISPATCHER: &str = "com/mojang/brigadier/CommandDispatcher";
const LITERAL_BUILDER: &str = "com/mojang/brigadier/builder/LiteralArgumentBuilder";
const REQUIRED_BUILDER: &str = "com/mojang/brigadier/builder/RequiredArgumentBuilder";
const ARGUMENT_BUILDER: &str = "com/mojang/brigadier/builder/ArgumentBuilder";
const TREE: &str = "com/mojang/brigadier/tree/";
const ROOT_NODE: &str = "com/mojang/brigadier/tree/RootCommandNode";
const LITERAL_NODE: &str = "com/mojang/brigadier/tree/LiteralCommandNode";
const ARGUMENT_NODE: &str = "com/mojang/brigadier/tree/ArgumentCommandNode";
const CONTEXT: &str = "com/mojang/brigadier/context/CommandContext";
const SOURCE: &str = "net/minecraft/commands/CommandSourceStack";
const COMMANDS: &str = "net/minecraft/commands/Commands";
const EVENT: &str = "net/neoforged/neoforge/event/RegisterCommandsEvent";
const BUILD_CONTEXT: &str = "net/minecraft/commands/CommandBuildContext";
const SELECTION: &str = "net/minecraft/commands/Commands$CommandSelection";
const BRIGADIER_ARGS: &str = "com/mojang/brigadier/arguments/";
const MC_ARGS: &str = "net/minecraft/commands/arguments/";
const COORDINATES: &str = "net/minecraft/commands/arguments/coordinates/Coordinates";
const SUGGESTIONS: &str = "net/minecraft/commands/SharedSuggestionProvider";
const SELECTOR_PARSER: &str = "net/minecraft/commands/arguments/selector/EntitySelectorParser";
const SERVER_PLAYER: &str = "net/minecraft/server/level/ServerPlayer";
const RUN: &str = "com/mojang/brigadier/Command.run.(Lcom/mojang/brigadier/context/CommandContext;)I";
const TEST: &str = "java/util/function/Predicate.test.(Ljava/lang/Object;)Z";
const SUPPLY: &str = "java/util/function/Supplier.get.()Ljava/lang/Object;";
const MODIFY: &str = "com/mojang/brigadier/RedirectModifier.apply.(Lcom/mojang/brigadier/context/CommandContext;)Ljava/util/Collection;";
const MODIFY_SINGLE: &str = "com/mojang/brigadier/SingleRedirectModifier.apply.(Lcom/mojang/brigadier/context/CommandContext;)Ljava/lang/Object;";
/// The modifier `redirect(target, single)` makes of a `SingleRedirectModifier` (in `$single`).
const SINGLE_MODIFIER: &str = "compat:single-redirect-modifier";
/// The server's dispatcher, once the mods registered to it.
const DISPATCHER_KEY: &str = "compat:commands";
/// Brigadier's default requirement, which every source passes (`getRequirement` of a node without one).
const ALWAYS: &str = "compat:command-always";

/// The keys [`call`] may answer, by owner, name and descriptor alone.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    owner.starts_with("com/mojang/brigadier/builder/")
        || owner.starts_with(TREE)
        || matches!(
            owner,
            DISPATCHER
                | CONTEXT
                | CONTEXT_BUILDER
                | PARSE_RESULTS
                | PARSED_NODE
                | PARSED_ARGUMENT
                | STRING_RANGE
                | SOURCE
                | EVENT
                | COMMAND_EVENT
                | COORDINATES
                | SUGGESTIONS
                | SELECTOR_PARSER
        )
        || is_read_value(owner)
        || (owner == COMMANDS && matches!(name, "literal" | "argument" | "getDispatcher"))
        || is_argument_type(owner)
        || (owner == "java/util/function/Predicate" && name == "test")
        || (owner == "com/mojang/brigadier/RedirectModifier" && name == "apply")
}

/// A Brigadier or Minecraft command call, or None when this module doesn't model it.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        ("java/util/function/Predicate", "test") if rt.class_of(a(0)) == ALWAYS => V::I(1),
        ("com/mojang/brigadier/RedirectModifier", "apply") if rt.class_of(a(0)) == SINGLE_MODIFIER => match modify(rt, a(0), a(1)) {
            Some(sources) => crate::jutil::new_list(rt, sources),
            None => V::NULL,
        },
        // The builders. `then` builds its argument at once, as Brigadier's does.
        (COMMANDS | LITERAL_BUILDER, "literal") => {
            let b = rt.new_object(LITERAL_BUILDER);
            rt.put_field(b, "$name", a(0));
            new_children(rt, b);
            b
        }
        (COMMANDS | REQUIRED_BUILDER, "argument") => {
            let b = rt.new_object(REQUIRED_BUILDER);
            rt.put_field(b, "$name", a(0));
            rt.put_field(b, "$type", a(1));
            new_children(rt, b);
            b
        }
        // `server.getCommands().getDispatcher()`: the server's, once there is one.
        (COMMANDS, "getDispatcher") => match rt.interned.get(DISPATCHER_KEY) {
            Some(&d) => d,
            None => new_dispatcher(rt),
        },
        (o, n) if is_builder(o) => builder_call(rt, n, desc, args)?,
        (o, n) if o.starts_with(TREE) => node_call(rt, o, n, args)?,
        // The dispatcher: a root of its own, the mods' roots merged into it.
        (DISPATCHER, "<init>") => {
            let root = if desc.starts_with("(Lcom/mojang/brigadier/tree/RootCommandNode;") { a(1) } else { new_root(rt) };
            rt.put_field(a(0), "$root", root);
            V::Void
        }
        (DISPATCHER, "getRoot") => root_of(rt, a(0)),
        (DISPATCHER, "register") => {
            let node = build(rt, a(1));
            let root = root_of(rt, a(0));
            add_child(rt, root, node);
            // The merged node: the one already there when the name was registered before.
            let n = name_of(rt, node);
            child_named(rt, root, &n).unwrap_or(node)
        }
        // `findNode(path)`: the node a path of names reaches from the root, or null.
        (DISPATCHER, "findNode") => {
            let mut node = root_of(rt, a(0));
            for part in crate::jutil::elements(rt, a(1)) {
                let want = rt.str_of(part).unwrap_or("").to_owned();
                match child_named(rt, node, &want) {
                    Some(c) => node = c,
                    None => return Some(V::NULL),
                }
            }
            node
        }
        // `parse` and `execute`, of a line, a `StringReader` or (`execute`) parse results.
        (DISPATCHER, "parse" | "execute") => dispatcher_run(rt, name, desc, args),
        // The event: the dispatcher the mods register to, and what it was made for.
        (EVENT, "getDispatcher") => rt.get_field(a(0), "$dispatcher"),
        (EVENT, "getBuildContext") => rt.intern("compat:command-build-context".into(), |rt| rt.new_object(BUILD_CONTEXT)),
        (EVENT, "getCommandSelection") => api::api_enum(rt, SELECTION, "ALL", 0),
        // What a parse read, as Brigadier's objects.
        (CONTEXT, n) => context_call(rt, n, args)?,
        (CONTEXT_BUILDER, n) => builder_context_call(rt, n, args)?,
        (PARSE_RESULTS | PARSED_NODE | PARSED_ARGUMENT | STRING_RANGE, n) => parse_object_call(rt, owner, n, desc, args)?,
        (COMMAND_EVENT, n) => command_event_call(rt, n, args)?,
        (SOURCE, _) => source_call(rt, name, args)?,
        // The argument types: one object per use, its kind and bounds; their getters, and a mod's
        // parser reading with a vanilla one.
        (o, n) if is_argument_type(o) => {
            if n.starts_with("get") && desc.starts_with("(Lcom/mojang/brigadier/context/CommandContext;") {
                return get_argument(rt, o, n, desc, args);
            }
            match n {
                "parse" => type_parse(rt, desc, args)?,
                // Brigadier's default (and a vanilla type's, not kept here): none.
                "getExamples" => crate::jutil::new_list(rt, Vec::new()),
                _ => argument_type(rt, o, n, desc, args)?,
            }
        }
        // What a selector, coordinates, an angle, a message, profiles, an item or a block read gives.
        (command_args::SELECTOR, n) => selector_call(rt, n, args)?,
        (SELECTOR_PARSER, n) => selector_parser_call(rt, n, args)?,
        (o, n) if o == COORDINATES || is_read_value(o) => read_value_call(rt, o, n, args)?,
        // A mod's suggestions aren't asked here: a chat box completes from the tree's kinds.
        (SUGGESTIONS, "suggest" | "suggestResource" | "suggestCoordinates" | "suggest2DCoordinates") => crate::jutil::completed_future(rt, V::NULL),
        _ => return None,
    })
}

fn simple(class: &str) -> &str {
    class.rsplit('/').next().unwrap_or(class)
}

fn is_builder(class: &str) -> bool {
    matches!(class, LITERAL_BUILDER | REQUIRED_BUILDER | ARGUMENT_BUILDER)
}

fn is_argument_type(class: &str) -> bool {
    (class.starts_with(BRIGADIER_ARGS) && class.ends_with("ArgumentType")) || (class.starts_with(MC_ARGS) && class.ends_with("Argument"))
}

/// The objects vanilla's argument types read (resolved later with a source).
fn is_read_value(class: &str) -> bool {
    matches!(
        class,
        command_args::SELECTOR
            | command_args::WORLD_COORDINATES
            | command_args::LOCAL_COORDINATES
            | command_args::SINGLE_ANGLE
            | command_args::MESSAGE
            | command_args::PROFILES
            | command_args::ITEM_INPUT
            | command_args::BLOCK_INPUT
    )
}

/// A builder's own calls: each gives the builder back, but `build` (its node) and the getters.
fn builder_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let b = a(0);
    Some(match name {
        "then" => {
            if rt.get_field(b, "$target").r() != 0 {
                return Some(crate::jlang::throw_with(rt, "java/lang/IllegalStateException", "Cannot add children to a redirected node"));
            }
            let node = if is_builder(rt.class_of(a(1))) { build(rt, a(1)) } else { a(1) };
            add_child(rt, b, node);
            b
        }
        "executes" => set(rt, b, "$command", a(1)),
        "requires" => set(rt, b, "$requirement", a(1)),
        "suggests" => set(rt, b, "$suggests", a(1)),
        "redirect" => {
            if !children(rt, b).is_empty() {
                return Some(crate::jlang::throw_with(rt, "java/lang/IllegalStateException", "Cannot forward a node with children"));
            }
            set(rt, b, "$target", a(1));
            // `redirect(target, single)`: Brigadier's modifier giving the one source it makes.
            let modifier = if desc.contains("SingleRedirectModifier") && a(2).r() != 0 {
                let m = rt.new_object(SINGLE_MODIFIER);
                set(rt, m, "$single", a(2))
            } else {
                V::NULL
            };
            set(rt, b, "$modifier", modifier);
            set(rt, b, "$forks", V::I(0))
        }
        "fork" | "forward" => {
            if !children(rt, b).is_empty() {
                return Some(crate::jlang::throw_with(rt, "java/lang/IllegalStateException", "Cannot forward a node with children"));
            }
            set(rt, b, "$target", a(1));
            set(rt, b, "$modifier", a(2));
            set(rt, b, "$forks", if name == "fork" { V::I(1) } else { V::I(a(3).i()) })
        }
        "build" => build(rt, b),
        "getThis" => b,
        "getArguments" => {
            let c = children(rt, b);
            crate::jutil::new_list(rt, c)
        }
        "getCommand" => rt.get_field(b, "$command"),
        "getRequirement" => requirement(rt, b),
        "getRedirect" => rt.get_field(b, "$target"),
        "getRedirectModifier" => rt.get_field(b, "$modifier"),
        "isFork" => V::I(rt.get_field(b, "$forks").i()),
        "getLiteral" | "getName" => rt.get_field(b, "$name"),
        "getType" => rt.get_field(b, "$type"),
        "getSuggestionsProvider" => rt.get_field(b, "$suggests"),
        _ => return None,
    })
}

/// A node's own calls.
fn node_call(rt: &mut Rt, owner: &str, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let n = a(0);
    Some(match name {
        "<init>" if owner == ROOT_NODE => {
            new_children(rt, n);
            V::Void
        }
        "getChild" => {
            let want = rt.str_of(a(1)).unwrap_or("").to_owned();
            child_named(rt, n, &want).unwrap_or(V::NULL)
        }
        "getChildren" => {
            let c = children(rt, n);
            crate::jutil::new_list(rt, c)
        }
        "addChild" => {
            add_child(rt, n, a(1));
            V::Void
        }
        "getName" | "getLiteral" | "getUsageText" => {
            if rt.class_of(n) == ROOT_NODE {
                rt.string("")
            } else {
                rt.get_field(n, "$name")
            }
        }
        "getCommand" => rt.get_field(n, "$command"),
        "getRequirement" => requirement(rt, n),
        "getRedirect" => rt.get_field(n, "$target"),
        "getRedirectModifier" => rt.get_field(n, "$modifier"),
        "isFork" => V::I(rt.get_field(n, "$forks").i()),
        "getType" => rt.get_field(n, "$type"),
        "canUse" => V::I(can_use(rt, n, a(1)) as i32),
        "getCustomSuggestions" => rt.get_field(n, "$suggests"),
        _ => return None,
    })
}

fn set(rt: &mut Rt, obj: V, field: &'static str, v: V) -> V {
    rt.put_field(obj, field, v);
    obj
}

fn new_children(rt: &mut Rt, obj: V) {
    if rt.get_field(obj, "$children").r() == 0 {
        let l = rt.alloc("java/util/ArrayList", Data::List(Vec::new()));
        rt.put_field(obj, "$children", l);
    }
}

fn new_root(rt: &mut Rt) -> V {
    let r = rt.new_object(ROOT_NODE);
    new_children(rt, r);
    r
}

fn new_dispatcher(rt: &mut Rt) -> V {
    let d = rt.new_object(DISPATCHER);
    let root = new_root(rt);
    rt.put_field(d, "$root", root);
    d
}

fn root_of(rt: &mut Rt, dispatcher: V) -> V {
    let r = rt.get_field(dispatcher, "$root");
    if r.r() != 0 {
        return r;
    }
    let r = new_root(rt);
    rt.put_field(dispatcher, "$root", r);
    r
}

/// A node's or builder's children, in the order they were added.
fn children(rt: &Rt, node: V) -> Vec<V> {
    match rt.heap.get(rt.get_field(node, "$children").r() as usize).map(|o| &o.data) {
        Some(Data::List(v)) => v.clone(),
        _ => Vec::new(),
    }
}

fn name_of(rt: &Rt, node: V) -> String {
    rt.str_of(rt.get_field(node, "$name")).unwrap_or("").to_owned()
}

fn is_literal(rt: &Rt, node: V) -> bool {
    matches!(rt.class_of(node), LITERAL_NODE | LITERAL_BUILDER)
}

fn child_named(rt: &Rt, node: V, name: &str) -> Option<V> {
    children(rt, node).into_iter().find(|c| name_of(rt, *c) == name)
}

/// `CommandNode.addChild`: a child named as one already there merges into it (its command, and its
/// own children, one by one); another is added last.
fn add_child(rt: &mut Rt, parent: V, node: V) {
    new_children(rt, parent);
    let name = name_of(rt, node);
    if let Some(existing) = child_named(rt, parent, &name) {
        if existing == node {
            return;
        }
        let command = rt.get_field(node, "$command");
        if command.r() != 0 {
            rt.put_field(existing, "$command", command);
        }
        for grandchild in children(rt, node) {
            add_child(rt, existing, grandchild);
        }
        return;
    }
    let list = rt.get_field(parent, "$children");
    if let Some(Data::List(v)) = rt.heap.get_mut(list.r() as usize).map(|o| &mut o.data) {
        v.push(node);
    }
}

/// `ArgumentBuilder.build()`: a node with the builder's command, requirement and redirect, and its
/// children added.
fn build(rt: &mut Rt, builder: V) -> V {
    let node = rt.new_object(if rt.class_of(builder) == LITERAL_BUILDER { LITERAL_NODE } else { ARGUMENT_NODE });
    for field in ["$name", "$type", "$command", "$requirement", "$target", "$modifier", "$forks", "$suggests"] {
        let v = rt.get_field(builder, field);
        if v.r() != 0 || matches!(v, V::I(_)) {
            rt.put_field(node, field, v);
        }
    }
    new_children(rt, node);
    for child in children(rt, builder) {
        add_child(rt, node, child);
    }
    node
}

/// A node's requirement: its own, or one that passes every source (Brigadier's default).
fn requirement(rt: &mut Rt, node: V) -> V {
    let r = rt.get_field(node, "$requirement");
    if r.r() != 0 {
        return r;
    }
    rt.intern(ALWAYS.into(), |rt| rt.new_object(ALWAYS))
}

/// Whether `source` may use `node`: its requirement passes (one that throws doesn't).
fn can_use(rt: &mut Rt, node: V, source: V) -> bool {
    let r = rt.get_field(node, "$requirement");
    if r.r() == 0 {
        return true;
    }
    let ok = rt.call_functional(TEST, r, &[source]).i() != 0;
    !caught(rt) && ok
}

/// Takes an exception the mod's code left, if any (it was the mod's to catch).
fn caught(rt: &mut Rt) -> bool {
    if rt.thrown.take().is_some() {
        rt.unwinding.clear();
        return true;
    }
    false
}

// ---- The source ----

/// The source of a command the player `player` (their UUID and name) runs at `permission`, where
/// they stand and look; the console's (level 4, no player) for None.
fn new_source(rt: &mut Rt, player: Option<(u128, &str)>, permission: u8, at: [f64; 3], rot: [f32; 2]) -> V {
    let s = rt.new_object(SOURCE);
    let name = match player {
        Some((uuid, name)) => {
            let p = crate::menus::server_player(rt, uuid);
            let n = rt.string(name);
            rt.put_field(p, "$name", n);
            for (field, v) in ["$x", "$y", "$z"].into_iter().zip(at) {
                rt.put_field(p, field, V::D(v));
            }
            rt.put_field(s, "$player", p);
            n
        }
        None => rt.string("Server"),
    };
    rt.put_field(s, "$name", name);
    rt.put_field(s, "$permission", V::I(i32::from(permission)));
    for (field, v) in ["$x", "$y", "$z"].into_iter().zip(at) {
        rt.put_field(s, field, V::D(v));
    }
    rt.put_field(s, "$yaw", V::F(rot[0]));
    rt.put_field(s, "$pitch", V::F(rot[1]));
    s
}

/// A source asked only for its level, to find which level a node needs: a player's, of nobody, saying
/// nothing.
fn probe_source(rt: &mut Rt, level: u8) -> V {
    let s = rt.new_object(SOURCE);
    let p = rt.new_object(SERVER_PLAYER);
    rt.put_field(s, "$player", p);
    let n = rt.string("");
    rt.put_field(s, "$name", n);
    rt.put_field(s, "$permission", V::I(i32::from(level)));
    rt.put_field(s, "$silent", V::I(1));
    s
}

fn copy_source(rt: &mut Rt, s: V) -> V {
    let c = rt.new_object(SOURCE);
    for field in ["$player", "$name", "$display", "$output", "$permission", "$x", "$y", "$z", "$yaw", "$pitch", "$silent"] {
        let v = rt.get_field(s, field);
        if !matches!(v, V::Void) {
            rt.put_field(c, field, v);
        }
    }
    c
}

fn source_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let s = a(0);
    let player = rt.get_field(s, "$player");
    Some(match name {
        // `new CommandSourceStack(output, position, rotation, level, permission, textName,
        // displayName, server, entity)`: a mod's own source (to run a command as the server). What
        // it is told goes to its output when that is the mod's, else nowhere (`CommandSource.NULL`).
        "<init>" if args.len() == 10 => {
            let (pos, rot) = (a(2), a(3));
            for (field, axis) in [("$x", "x"), ("$y", "y"), ("$z", "z")] {
                let v = rt.get_field(pos, axis);
                rt.put_field(s, field, V::D(v.d()));
            }
            let (pitch, yaw) = (rt.get_field(rot, "x").f(), rt.get_field(rot, "y").f());
            rt.put_field(s, "$pitch", V::F(pitch));
            rt.put_field(s, "$yaw", V::F(yaw));
            rt.put_field(s, "$permission", V::I(a(5).i()));
            rt.put_field(s, "$name", a(6));
            rt.put_field(s, "$display", a(7));
            if rt.instance_of(a(9), SERVER_PLAYER) {
                rt.put_field(s, "$player", a(9));
            }
            let output = a(1);
            if output.r() != 0 && rt.supers.contains_key(rt.class_of(output)) {
                rt.put_field(s, "$output", output);
            } else {
                rt.put_field(s, "$silent", V::I(1));
            }
            V::Void
        }
        "hasPermission" => V::I((rt.get_field(s, "$permission").i() >= a(1).i()) as i32),
        "getPlayerOrException" | "getEntityOrException" => {
            if player.r() == 0 {
                return Some(throw_syntax(rt, if name == "getPlayerOrException" { "permissions.requires.player" } else { "permissions.requires.entity" }, Vec::new()));
            }
            player
        }
        "getPlayer" | "getEntity" => player,
        "isPlayer" => V::I((player.r() != 0) as i32),
        "getLevel" | "getUnsidedLevel" => crate::glue::level(rt),
        "getServer" => crate::neo::server(rt),
        "registryAccess" => {
            let server = crate::neo::server(rt);
            api::call(rt, "net/minecraft/server/MinecraftServer.registryAccess.()Lnet/minecraft/core/RegistryAccess;", &[server])
        }
        "getPosition" => {
            let [x, y, z] = [rt.get_field(s, "$x").d(), rt.get_field(s, "$y").d(), rt.get_field(s, "$z").d()];
            crate::api_common::vec3(rt, x, y, z)
        }
        "getRotation" => {
            let (yaw, pitch) = (rt.get_field(s, "$yaw").f(), rt.get_field(s, "$pitch").f());
            crate::api_common::vec2(rt, pitch, yaw)
        }
        "getTextName" => rt.get_field(s, "$name"),
        "getDisplayName" => match rt.get_field(s, "$display") {
            d if d.r() != 0 => d,
            _ => {
                let n = rt.get_field(s, "$name");
                crate::api_common::component(rt, n, false, V::NULL)
            }
        },
        // What the command says: to whoever ran it, unless the source is silenced; a failure in red.
        "sendSuccess" => {
            let c = rt.call_functional(SUPPLY, a(1), &[]);
            if !caught(rt) {
                feedback(rt, s, c, false);
            }
            V::Void
        }
        "sendFailure" => {
            feedback(rt, s, a(1), true);
            V::Void
        }
        "sendSystemMessage" => {
            feedback(rt, s, a(1), false);
            V::Void
        }
        "sendChatMessage" => V::Void,
        "withSuppressedOutput" => {
            let c = copy_source(rt, s);
            set(rt, c, "$silent", V::I(1))
        }
        "withPermission" | "withMaximumPermission" => {
            let c = copy_source(rt, s);
            let now = rt.get_field(s, "$permission").i();
            let level = if name == "withPermission" { a(1).i() } else { now.max(a(1).i()) };
            set(rt, c, "$permission", V::I(level))
        }
        "withPosition" => {
            let c = copy_source(rt, s);
            for field in ["x", "y", "z"] {
                let v = rt.get_field(a(1), field);
                rt.put_field(c, if field == "x" { "$x" } else if field == "y" { "$y" } else { "$z" }, V::D(v.d()));
            }
            c
        }
        "withRotation" => {
            let c = copy_source(rt, s);
            let (pitch, yaw) = (rt.get_field(a(1), "x").f(), rt.get_field(a(1), "y").f());
            rt.put_field(c, "$yaw", V::F(yaw));
            set(rt, c, "$pitch", V::F(pitch))
        }
        "withEntity" => {
            let c = copy_source(rt, s);
            set(rt, c, "$player", a(1))
        }
        "withSource" | "withLevel" | "withAnchor" | "withCallback" | "facing" | "withSigningContext" => copy_source(rt, s),
        "getAnchor" => api::api_enum(rt, "net/minecraft/commands/arguments/EntityAnchorArgument$Anchor", "FEET", 0),
        "shouldFilterMessageTo" => V::I(0),
        "isSilent" => V::I(rt.get_field(s, "$silent").i()),
        _ => return None,
    })
}

/// A line of the command's answer to whoever ran it (not to a probe or a silenced source).
fn feedback(rt: &mut Rt, source: V, component: V, failure: bool) {
    if rt.get_field(source, "$silent").i() != 0 {
        return;
    }
    // A mod's own output (a source it made): its `sendSystemMessage`.
    let output = rt.get_field(source, "$output");
    if output.r() != 0 {
        let c = if rt.str_of(component).is_some() { crate::api_common::component(rt, component, false, V::NULL) } else { component };
        rt.call_functional("net/minecraft/commands/CommandSource.sendSystemMessage.(Lnet/minecraft/network/chat/Component;)V", output, &[c]);
        caught(rt);
        return;
    }
    let mut json = message_json(rt, component);
    if failure && let serde_json::Value::Object(o) = &mut json {
        o.entry("color").or_insert_with(|| "red".into());
    }
    let text = json.to_string();
    if let Some(h) = rt.host_mut() {
        h.command_feedback(&text);
    }
}

/// A Brigadier message's text: a component, Brigadier's `LiteralMessage` or a string.
fn message_json(rt: &Rt, message: V) -> serde_json::Value {
    if let Some(s) = rt.str_of(message) {
        return serde_json::json!({ "text": s });
    }
    if rt.class_of(message) == "com/mojang/brigadier/LiteralMessage" {
        let s = rt.str_of(rt.get_field(message, "$str")).unwrap_or("");
        return serde_json::json!({ "text": s });
    }
    crate::api_common::component_json(rt, message)
}

/// `getArgument(name, class)` of a name the command has no argument of: Brigadier's
/// `IllegalArgumentException`.
fn no_argument(rt: &mut Rt, name: &str) -> V {
    crate::jlang::throw_with(rt, "java/lang/IllegalArgumentException", &format!("No such argument '{name}' exists on this command"))
}

// ---- The argument types ----

/// The vanilla argument types made without their factory (`new UuidArgument()`): the factory's kind.
const CONSTRUCTED: [(&str, &str); 6] = [
    ("UuidArgument", "uuid"),
    ("DimensionArgument", "dimension"),
    ("ColumnPosArgument", "columnPos"),
    ("ItemArgument", "item"),
    ("GameProfileArgument", "gameProfile"),
    ("ResourceLocationArgument", "id"),
];

/// An argument type's factory (`IntegerArgumentType.integer(0, 64)`, `EntityArgument.player()`):
/// an object of the type holding its kind (`IntegerArgumentType.integer`) and the factory's
/// arguments (its bounds). A vanilla type made by its constructor gets its factory's kind.
fn argument_type(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    if name == "<init>" {
        let this = *args.first()?;
        let (_, factory) = CONSTRUCTED.iter().find(|(c, _)| *c == simple(owner) && rt.class_of(this) == owner)?;
        let kind = rt.string(&format!("{}.{factory}", simple(owner)));
        rt.put_field(this, "$kind", kind);
        return Some(V::Void);
    }
    let returns = desc.rsplit(')').next().unwrap_or("");
    let class = returns.strip_prefix('L').and_then(|r| r.strip_suffix(';'))?;
    if name.starts_with("get") || name == "parse" || !(is_argument_type(class) || class.ends_with("/ArgumentType")) {
        return None;
    }
    let class = if is_argument_type(class) { rt.static_key(class) } else { rt.static_key(owner) };
    let t = rt.new_object(class);
    let kind = rt.string(&format!("{}.{name}", simple(owner)));
    rt.put_field(t, "$kind", kind);
    let list = crate::jutil::new_list(rt, args.to_vec());
    rt.put_field(t, "$args", list);
    Some(t)
}

fn kind_of(rt: &Rt, t: V) -> String {
    rt.str_of(rt.get_field(t, "$kind")).unwrap_or("").to_owned()
}

/// The kind of a usage's argument (the game's name for vanilla's parser), which the chat box
/// completes and checks: a vanilla type's own; for a mod's own type, the kind its examples are
/// ([`mod_usage_kind`]). None for a vanilla type not kept here.
fn usage_kind(rt: &mut Rt, t: V) -> Option<&'static str> {
    let kind = kind_of(rt, t);
    if kind.is_empty() && t.r() != 0 {
        return Some(mod_usage_kind(rt, t));
    }
    Some(match kind.as_str() {
        "IntegerArgumentType.integer" => "integer",
        "TimeArgument.time" => "time",
        "LongArgumentType.longArg" => "long",
        "FloatArgumentType.floatArg" | "DoubleArgumentType.doubleArg" => "number",
        "BoolArgumentType.bool" => "boolean",
        "StringArgumentType.word" => "word",
        "StringArgumentType.string" => "string",
        "StringArgumentType.greedyString" => "text",
        "MessageArgument.message" => "message",
        "EntityArgument.player" => "player",
        "EntityArgument.players" => "players",
        "EntityArgument.entity" => "entity",
        "EntityArgument.entities" => "entities",
        "GameProfileArgument.gameProfile" => "game_profile",
        "BlockPosArgument.blockPos" => "block_pos",
        "Vec3Argument.vec3" => "vec3",
        "Vec2Argument.vec2" => "vec2",
        "ColumnPosArgument.columnPos" => "column_pos",
        "RotationArgument.rotation" => "rotation",
        "AngleArgument.angle" => "angle",
        "ResourceLocationArgument.id" => "id",
        "DimensionArgument.dimension" => "dimension",
        "ItemArgument.item" => "item",
        "BlockStateArgument.block" => "block",
        "UuidArgument.uuid" => "uuid",
        "ColorArgument.color" => "color",
        _ => return None,
    })
}

/// The kind a mod's own argument type is checked as in the chat box, from what its examples
/// (`getExamples`) look like: words Brigadier reads unquoted, ids (`create:shaft`), or else
/// strings (a word, or a quoted string). Its own parser reads the line on the server whatever the
/// chat box made of it.
fn mod_usage_kind(rt: &mut Rt, t: V) -> &'static str {
    let given = api::virtual_call(rt, (GET_EXAMPLES_KEY, GET_EXAMPLES), &[t]);
    if caught(rt) {
        return "string";
    }
    let examples: Vec<String> = crate::jutil::elements(rt, given).into_iter().filter_map(|e| rt.str_of(e).map(str::to_owned)).collect();
    let unquoted = |s: &String| !s.is_empty() && s.encode_utf16().all(brigadier::is_allowed_in_unquoted_string);
    if examples.is_empty() {
        "string"
    } else if examples.iter().all(unquoted) {
        "word"
    } else if examples.iter().all(|e| is_location(e)) {
        "id"
    } else {
        "string"
    }
}

/// Whether `s` reads whole as a resource location (`ResourceLocation.read`: an optional namespace
/// of `[a-z0-9_.-]`, a path of those and `/`).
fn is_location(s: &str) -> bool {
    let (namespace, path) = s.split_once(':').unwrap_or(("minecraft", s));
    let ok = |s: &str, slash: bool| s.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-') || (slash && b == b'/'));
    !path.is_empty() && ok(namespace, false) && ok(path, true)
}

// ---- Registration and the usages ----

/// `native_register_commands`: the mods hear `RegisterCommandsEvent` with a new dispatcher, and its
/// tree's runnable paths come back as native usages with the operator level each needs.
pub fn register(rt: &mut Rt) -> Vec<(String, u8)> {
    let dispatcher = new_dispatcher(rt);
    rt.interned.insert(DISPATCHER_KEY.into(), dispatcher);
    let ev = rt.new_object(EVENT);
    rt.put_field(ev, "$dispatcher", dispatcher);
    let all = api::api_enum(rt, SELECTION, "ALL", 0);
    rt.put_field(ev, "$selection", all);
    crate::neo::post(rt, EVENT, ev);
    usages(rt, dispatcher)
}

/// Every runnable path of `dispatcher`'s tree as a usage, with the level it needs.
fn usages(rt: &mut Rt, dispatcher: V) -> Vec<(String, u8)> {
    let root = root_of(rt, dispatcher);
    let mut walk = Usages { root, out: Vec::new(), levels: Vec::new(), left_out: Vec::new() };
    for child in children(rt, root) {
        walk.node(rt, child, &mut Vec::new(), &mut Vec::new(), 0);
    }
    if Rt::tracing() {
        for (usage, why) in &walk.left_out {
            eprintln!("horizon-compat: command path `{usage}` left out: {why}");
        }
    }
    let mut seen = std::collections::HashSet::new();
    walk.out.retain(|(u, _)| seen.insert(u.clone()));
    walk.out
}

struct Usages {
    root: V,
    out: Vec<(String, u8)>,
    /// Each node's level, once found.
    levels: Vec<(V, u8)>,
    left_out: Vec<(String, String)>,
}

impl Usages {
    /// The level `node`'s requirement needs: the lowest of 0 to 4 whose source passes it. A
    /// requirement no level passes asks something else of the source (who it is, the server's
    /// mode): the operators', which the source's own check still guards when it runs.
    fn level(&mut self, rt: &mut Rt, node: V) -> u8 {
        if let Some(&(_, l)) = self.levels.iter().find(|(n, _)| *n == node) {
            return l;
        }
        let r = rt.get_field(node, "$requirement");
        let mut level = 4;
        if r.r() == 0 {
            level = 0;
        } else {
            for l in 0..=4u8 {
                let probe = probe_source(rt, l);
                let ok = rt.call_functional(TEST, r, &[probe]).i() != 0;
                if !caught(rt) && ok {
                    level = l;
                    break;
                }
            }
        }
        self.levels.push((node, level));
        level
    }

    fn node(&mut self, rt: &mut Rt, node: V, words: &mut Vec<String>, path: &mut Vec<V>, level: u8) {
        if path.len() > 32 || path.contains(&node) {
            return;
        }
        let level = level.max(self.level(rt, node));
        let name = name_of(rt, node);
        let word = if is_literal(rt, node) {
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c)) {
                self.left_out.push((format!("{} {name}", words.join(" ")), "not a command word".into()));
                return;
            }
            name
        } else {
            let t = rt.get_field(node, "$type");
            let Some(kind) = usage_kind(rt, t) else {
                let what = match kind_of(rt, t) {
                    k if k.is_empty() => rt.class_of(t).to_owned(),
                    k => k,
                };
                self.left_out.push((format!("{} <{name}>", words.join(" ")), format!("argument type {what}")));
                return;
            };
            let clean: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
            format!("<{clean}:{kind}>")
        };
        let greedy = word.ends_with(":text>") || word.ends_with(":message>");
        words.push(word);
        path.push(node);
        if rt.get_field(node, "$command").r() != 0 {
            self.out.push((words.join(" "), level));
        }
        if !greedy {
            let target = rt.get_field(node, "$target");
            // A redirect to the root (`/execute run`'s kind) can't be written as usages.
            if target.r() != 0 && target == self.root {
                self.left_out.push((words.join(" "), "redirects to every command".into()));
            } else {
                let next = if target.r() != 0 { children(rt, target) } else { children(rt, node) };
                for c in next {
                    self.node(rt, c, words, path, level);
                }
            }
        }
        words.pop();
        path.pop();
    }
}

// ---- Parsing a line ----

/// `native_perform_command`: runs `line` (without its slash) for `player` (their UUID and name; the
/// console for None) at their level and place, as `Commands.performPrefixedCommand` does. False
/// when no root literal of the mods' tree is the line's first word (another bundle's tree, or the
/// game, answers it); else true, whatever the line did: a parse error is answered as vanilla's (its
/// message, then the line up to where it went wrong), and what the command says is its feedback.
pub fn perform(rt: &mut Rt, player: Option<(u128, &str)>, permission: u8, at: [f64; 3], rot: [f32; 2], line: &str) -> bool {
    let Some(&dispatcher) = rt.interned.get(DISPATCHER_KEY) else { return false };
    if !owns(rt, dispatcher, line) {
        return false;
    }
    let source = new_source(rt, player, permission, at, rot);
    let root = root_of(rt, dispatcher);
    let parse = parse(rt, root, source, Reader::new(line));
    let results = results_object(rt, dispatcher, &parse);
    perform_results(rt, results, line);
    true
}

/// Whether the first word of `line` is a root literal of `dispatcher`'s tree.
fn owns(rt: &mut Rt, dispatcher: V, line: &str) -> bool {
    let root = root_of(rt, dispatcher);
    let first = Reader::new(line).word();
    children(rt, root).into_iter().any(|c| is_literal(rt, c) && name_of(rt, c) == first)
}

/// What a parse has read so far (`CommandContextBuilder`).
#[derive(Clone)]
struct Context {
    source: V,
    root: V,
    /// The arguments read, by name in the order first read (`LinkedHashMap.put`), with their range.
    args: Vec<(String, V, (i32, i32))>,
    nodes: Vec<(V, (i32, i32))>,
    command: V,
    range: (i32, i32),
    modifier: V,
    forks: bool,
    child: Option<Box<Context>>,
}

impl Context {
    fn new(source: V, root: V, start: i32) -> Self {
        Context { source, root, args: Vec::new(), nodes: Vec::new(), command: V::NULL, range: (start, start), modifier: V::NULL, forks: false, child: None }
    }

    /// `withNode`: the node read over `range`; its redirect's modifier is now the context's.
    fn with_node(&mut self, rt: &Rt, node: V, range: (i32, i32)) {
        self.nodes.push((node, range));
        self.range = (self.range.0.min(range.0), self.range.1.max(range.1));
        self.modifier = reference(rt.get_field(node, "$modifier"));
        self.forks = rt.get_field(node, "$forks").i() != 0;
    }

    fn with_argument(&mut self, name: String, value: V, range: (i32, i32)) {
        match self.args.iter_mut().find(|a| a.0 == name) {
            Some(a) => (a.1, a.2) = (value, range),
            None => self.args.push((name, value, range)),
        }
    }
}

/// A field holding an object, or null.
fn reference(v: V) -> V {
    match v {
        V::R(_) => v,
        _ => V::NULL,
    }
}

/// What reading a node failed with: a reader's error, or a mod's `CommandSyntaxException`.
#[derive(Clone)]
enum Thrown {
    Syntax(Syntax),
    Java(V),
}

/// `ParseResults`: the context read, the reader where reading stopped, and the errors of the nodes
/// tried there.
struct Parse {
    context: Context,
    reader: Reader,
    errors: Vec<(V, Thrown)>,
}

/// `CommandDispatcher.parse(reader, source)`.
fn parse(rt: &mut Rt, root: V, source: V, reader: Reader) -> Parse {
    let context = Context::new(source, root, reader.cursor);
    parse_nodes(rt, root, &reader, &context, 0)
}

/// The deepest a parse goes (each level reads a separator at least).
const MAX_DEPTH: usize = 512;

/// `parseNodes`: each child the source may use that the reader's next word is relevant to, read on
/// a copy of the reader; one that reads (and is followed by a space or the end) goes on at its own
/// children, or at its redirect's target in a context of its own. Of the paths that read, the one
/// that read the whole line without errors is taken first; when none read, the errors are the
/// children's.
fn parse_nodes(rt: &mut Rt, node: V, reader: &Reader, so_far: &Context, depth: usize) -> Parse {
    let source = so_far.source;
    let mut errors = Vec::new();
    let mut potentials: Vec<Parse> = Vec::new();
    if depth <= MAX_DEPTH {
        for child in relevant_nodes(rt, node, reader) {
            if !can_use(rt, child, source) {
                continue;
            }
            let mut context = so_far.clone();
            let mut r = reader.clone();
            let read = parse_node(rt, child, &mut r, &mut context).and_then(|()| {
                if r.can_read() && r.peek() != unit(b' ') {
                    return Err(Thrown::Syntax(r.error("command.expected.separator", Vec::new())));
                }
                Ok(())
            });
            if let Err(e) = read {
                errors.push((child, e));
                continue;
            }
            context.command = reference(rt.get_field(child, "$command"));
            let redirect = reference(rt.get_field(child, "$target"));
            if r.can_read_n(if redirect.r() == 0 { 2 } else { 1 }) {
                r.skip();
                if redirect.r() != 0 {
                    let child_context = Context::new(source, redirect, r.cursor);
                    let parse = parse_nodes(rt, redirect, &r, &child_context, depth + 1);
                    context.child = Some(Box::new(parse.context));
                    return Parse { context, reader: parse.reader, errors: parse.errors };
                }
                potentials.push(parse_nodes(rt, child, &r, &context, depth + 1));
            } else {
                potentials.push(Parse { context, reader: r, errors: Vec::new() });
            }
        }
    }
    if potentials.is_empty() {
        return Parse { context: so_far.clone(), reader: reader.clone(), errors };
    }
    potentials.sort_by_key(|p| (p.reader.can_read(), !p.errors.is_empty()));
    potentials.swap_remove(0)
}

/// `getRelevantNodes`: the literal the next word names, when there is one; else every argument.
fn relevant_nodes(rt: &Rt, node: V, reader: &Reader) -> Vec<V> {
    let kids = children(rt, node);
    if kids.iter().any(|c| is_literal(rt, *c)) {
        let word = reader.word();
        if let Some(&literal) = kids.iter().find(|c| is_literal(rt, **c) && name_of(rt, **c) == word) {
            return vec![literal];
        }
    }
    kids.into_iter().filter(|c| !is_literal(rt, *c)).collect()
}

/// `CommandNode.parse`: a literal's word (then a space or the end), or an argument its type reads.
fn parse_node(rt: &mut Rt, node: V, r: &mut Reader, context: &mut Context) -> Result<(), Thrown> {
    let start = r.cursor;
    let name = name_of(rt, node);
    if is_literal(rt, node) {
        let literal: Vec<u16> = name.encode_utf16().collect();
        let end = start + literal.len() as i32;
        if r.can_read_n(literal.len() as i32) && r.s[start as usize..end as usize] == literal[..] {
            r.cursor = end;
            if !r.can_read() || r.peek() == unit(b' ') {
                context.with_node(rt, node, (start, end));
                return Ok(());
            }
            r.cursor = start;
        }
        return Err(Thrown::Syntax(r.error("argument.literal.incorrect", vec![Arg::Text(name)])));
    }
    let t = rt.get_field(node, "$type");
    let value = parse_argument(rt, t, context.source, r)?;
    let range = (start, r.cursor);
    context.with_argument(name, value, range);
    context.with_node(rt, node, range);
    Ok(())
}

const PARSE: &str = "parse.(Lcom/mojang/brigadier/StringReader;)Ljava/lang/Object;";
const PARSE_KEY: &str = "com/mojang/brigadier/arguments/ArgumentType.parse.(Lcom/mojang/brigadier/StringReader;)Ljava/lang/Object;";
const PARSE_WITH_SOURCE: &str = "parse.(Lcom/mojang/brigadier/StringReader;Ljava/lang/Object;)Ljava/lang/Object;";
const PARSE_WITH_SOURCE_KEY: &str = "com/mojang/brigadier/arguments/ArgumentType.parse.(Lcom/mojang/brigadier/StringReader;Ljava/lang/Object;)Ljava/lang/Object;";
const GET_EXAMPLES: &str = "getExamples.()Ljava/util/Collection;";
const GET_EXAMPLES_KEY: &str = "com/mojang/brigadier/arguments/ArgumentType.getExamples.()Ljava/util/Collection;";

/// `ArgumentType.parse(reader, source)`: a vanilla type as its parser reads; a mod's own by its
/// bytecode, with a `StringReader` object over the line. A `RuntimeException` the mod's parser
/// throws is Brigadier's `command.exception` with its message.
fn parse_argument(rt: &mut Rt, t: V, source: V, r: &mut Reader) -> Result<V, Thrown> {
    let kind = kind_of(rt, t);
    if !kind.is_empty() {
        let given = factory_arguments(rt, t);
        let selectors = command_args::permission(rt, source) >= 2;
        return match command_args::parse(rt, &kind, &given, r, selectors) {
            Some(read) => read.map_err(Thrown::Syntax),
            None => Err(Thrown::Syntax(r.error("command.exception", vec![Arg::Text(format!("{kind} isn't read here"))]))),
        };
    }
    let reader = brigadier::new_reader(rt, r);
    let class = rt.class_of(t);
    let value = if rt.class_overrides(class, PARSE_WITH_SOURCE) {
        api::virtual_call(rt, (PARSE_WITH_SOURCE_KEY, PARSE_WITH_SOURCE), &[t, reader, source])
    } else {
        api::virtual_call(rt, (PARSE_KEY, PARSE), &[t, reader])
    };
    r.cursor = rt.get_field(reader, "$cursor").i();
    let Some(e) = rt.thrown.take() else { return Ok(value) };
    rt.unwinding.clear();
    if rt.instance_of(e, brigadier::SYNTAX) {
        return Err(Thrown::Java(e));
    }
    let message = rt.str_of(rt.get_field(e, "message")).unwrap_or("null").to_owned();
    if Rt::tracing() {
        eprintln!("horizon-compat: {class}.parse threw {}: {message}", rt.class_of(e));
    }
    Err(Thrown::Syntax(r.error("command.exception", vec![Arg::Text(message)])))
}

/// The arguments a vanilla type's factory was given (its bounds).
fn factory_arguments(rt: &Rt, t: V) -> Vec<V> {
    crate::jutil::elements(rt, rt.get_field(t, "$args"))
}

/// `ArgumentType.parse` called by a mod (a type of its own reading with a vanilla one, or
/// Brigadier's default `parse(reader, source)`): a vanilla type reads natively, moving the reader
/// object's cursor; a failure throws Brigadier's exception.
fn type_parse(rt: &mut Rt, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let (t, reader) = (a(0), a(1));
    let with_source = desc.starts_with("(Lcom/mojang/brigadier/StringReader;Ljava/lang/Object;)");
    let kind = kind_of(rt, t);
    if kind.is_empty() {
        // A mod's type without its own `parse(reader, source)`: Brigadier's default reads alone.
        return with_source.then(|| api::virtual_call(rt, (PARSE_KEY, PARSE), &[t, reader]));
    }
    let source = a(2);
    let selectors = !with_source || (source.r() != 0 && rt.instance_of(source, SOURCE) && command_args::permission(rt, source) >= 2);
    let given = factory_arguments(rt, t);
    let mut r = brigadier::reader_of(rt, reader);
    let read = command_args::parse(rt, &kind, &given, &mut r, selectors)?;
    rt.put_field(reader, "$cursor", V::I(r.cursor));
    Some(match read {
        Ok(v) => v,
        Err(e) => {
            let ex = brigadier::exception(rt, &r.s, &e);
            rt.thrown = Some(ex);
            V::NULL
        }
    })
}

// ---- Parse results and contexts as objects ----

const PARSE_RESULTS: &str = "com/mojang/brigadier/ParseResults";
const CONTEXT_BUILDER: &str = "com/mojang/brigadier/context/CommandContextBuilder";
const PARSED_NODE: &str = "com/mojang/brigadier/context/ParsedCommandNode";
const PARSED_ARGUMENT: &str = "com/mojang/brigadier/context/ParsedArgument";
const STRING_RANGE: &str = "com/mojang/brigadier/context/StringRange";
const COMMAND_EVENT: &str = "net/neoforged/neoforge/event/CommandEvent";
/// What a context and its builder hold alike.
const CONTEXT_FIELDS: [&str; 10] = ["$source", "$root", "$command", "$modifier", "$forks", "$range", "$argNames", "$argValues", "$argRanges", "$nodes"];

/// The `ParseResults` object of `parse`: its context's builder, its reader where reading stopped,
/// and the errors there by node.
fn results_object(rt: &mut Rt, dispatcher: V, parse: &Parse) -> V {
    let o = rt.new_object(PARSE_RESULTS);
    let context = builder_object(rt, dispatcher, &parse.context);
    rt.put_field(o, "$context", context);
    let reader = brigadier::new_reader(rt, &parse.reader);
    rt.put_field(o, "$reader", reader);
    let map = rt.alloc("java/util/LinkedHashMap", Data::Table(Default::default()));
    for (node, e) in &parse.errors {
        let e = match e {
            Thrown::Syntax(s) => brigadier::exception(rt, &parse.reader.s, s),
            Thrown::Java(e) => *e,
        };
        crate::jutil::table_put(rt, map, *node, e);
    }
    rt.put_field(o, "$exceptions", map);
    o
}

/// A `CommandContextBuilder` object of `c` (with its child's).
fn builder_object(rt: &mut Rt, dispatcher: V, c: &Context) -> V {
    let child = match &c.child {
        Some(child) => builder_object(rt, dispatcher, child),
        None => V::NULL,
    };
    let o = rt.new_object(CONTEXT_BUILDER);
    rt.put_field(o, "$dispatcher", dispatcher);
    rt.put_field(o, "$child", child);
    rt.put_field(o, "$source", c.source);
    rt.put_field(o, "$root", c.root);
    rt.put_field(o, "$command", c.command);
    rt.put_field(o, "$modifier", c.modifier);
    rt.put_field(o, "$forks", V::I(i32::from(c.forks)));
    let range = string_range(rt, c.range);
    rt.put_field(o, "$range", range);
    let (mut names, mut values, mut ranges) = (Vec::new(), Vec::new(), Vec::new());
    for (name, value, range) in &c.args {
        names.push(rt.string(name));
        values.push(*value);
        ranges.push(string_range(rt, *range));
    }
    let mut nodes = Vec::new();
    for &(node, range) in &c.nodes {
        let p = rt.new_object(PARSED_NODE);
        rt.put_field(p, "$node", node);
        let range = string_range(rt, range);
        rt.put_field(p, "$range", range);
        nodes.push(p);
    }
    for (field, items) in [("$argNames", names), ("$argValues", values), ("$argRanges", ranges), ("$nodes", nodes)] {
        let list = crate::jutil::new_list(rt, items);
        rt.put_field(o, field, list);
    }
    o
}

fn string_range(rt: &mut Rt, (start, end): (i32, i32)) -> V {
    let r = rt.new_object(STRING_RANGE);
    rt.put_field(r, "$start", V::I(start));
    rt.put_field(r, "$end", V::I(end));
    r
}

fn range_of(rt: &Rt, range: V) -> (i32, i32) {
    (rt.get_field(range, "$start").i(), rt.get_field(range, "$end").i())
}

/// `CommandContextBuilder.build(input)`: the context of `builder` (its child's built too).
fn build_context(rt: &mut Rt, builder: V, input: V) -> V {
    let child = reference(rt.get_field(builder, "$child"));
    let child = if child.r() != 0 { build_context(rt, child, input) } else { V::NULL };
    let o = rt.new_object(CONTEXT);
    for field in CONTEXT_FIELDS {
        let v = rt.get_field(builder, field);
        rt.put_field(o, field, v);
    }
    rt.put_field(o, "$child", child);
    rt.put_field(o, "$input", input);
    o
}

/// `CommandContext.copyFor(source)`: the context with another source (itself for its own).
fn copy_for(rt: &mut Rt, ctx: V, source: V) -> V {
    if rt.get_field(ctx, "$source") == source {
        return ctx;
    }
    let o = rt.new_object(CONTEXT);
    for field in CONTEXT_FIELDS.into_iter().chain(["$child", "$input"]) {
        let v = rt.get_field(ctx, field);
        rt.put_field(o, field, v);
    }
    rt.put_field(o, "$source", source);
    o
}

/// `ContextChain.tryFlatten`: the contexts from the top to the last, whose command runs; None
/// when the last has no command.
fn chain(rt: &Rt, top: V) -> Option<Vec<V>> {
    let mut out = vec![top];
    loop {
        let current = *out.last()?;
        let child = reference(rt.get_field(current, "$child"));
        if child.r() == 0 {
            return (reference(rt.get_field(current, "$command")).r() != 0).then_some(out);
        }
        out.push(child);
    }
}

/// `Commands.getParseException`: none when the whole line was read; else the one error there was,
/// or Brigadier's unknown command (when nothing was read) or unknown argument.
fn parse_exception(rt: &mut Rt, results: V) -> Option<V> {
    let r = brigadier::reader_of(rt, rt.get_field(results, "$reader"));
    if !r.can_read() {
        return None;
    }
    let errors = crate::jutil::table_pairs(rt, rt.get_field(results, "$exceptions"));
    if errors.len() == 1 {
        return Some(errors[0].1);
    }
    let range = rt.get_field(rt.get_field(results, "$context"), "$range");
    let (start, end) = range_of(rt, range);
    let key = if start == end { "command.unknown.command" } else { "command.unknown.argument" };
    Some(brigadier::exception(rt, &r.s, &r.error(key, Vec::new())))
}

/// The top context of `results` and its chain; else the exception Brigadier throws for it (left in
/// `rt.thrown`).
fn finished(rt: &mut Rt, results: V, command: &str) -> Result<(V, Vec<V>), V> {
    if let Some(e) = parse_exception(rt, results) {
        return Err(e);
    }
    let input = rt.string(command);
    let top = build_context(rt, rt.get_field(results, "$context"), input);
    match chain(rt, top) {
        Some(contexts) => Ok((top, contexts)),
        None => {
            let r = brigadier::reader_of(rt, rt.get_field(results, "$reader"));
            Err(brigadier::exception(rt, &r.s, &r.error("command.unknown.command", Vec::new())))
        }
    }
}

// ---- Running a parsed line ----

const COMMAND_FAILED_LIMIT: usize = 65536;

/// `Commands.performCommand(results, command)`: NeoForge's `CommandEvent` first (a listener may
/// cancel the command, or give other results to run), then vanilla's `finishParsing` (a parse
/// error answered with its message and the line's context) and execution.
fn perform_results(rt: &mut Rt, results: V, command: &str) {
    let source = rt.get_field(rt.get_field(results, "$context"), "$source");
    let ev = rt.new_object(COMMAND_EVENT);
    rt.put_field(ev, "$parseResults", results);
    crate::neo::post(rt, COMMAND_EVENT, ev);
    if rt.get_field(ev, "$canceled").i() != 0 {
        let e = reference(rt.get_field(ev, "$exception"));
        if e.r() != 0 {
            let text = describe_error(rt, e);
            let s = rt.string(&text);
            let c = crate::api_common::component(rt, s, false, V::NULL);
            feedback(rt, source, c, true);
        }
        return;
    }
    let results = match reference(rt.get_field(ev, "$parseResults")) {
        r if r.r() != 0 => r,
        _ => results,
    };
    match finished(rt, results, command) {
        Ok((_, contexts)) => run_vanilla(rt, &contexts, source),
        Err(e) => report(rt, source, command, e),
    }
}

/// `Util.describeError`: the innermost cause's message, else its name.
fn describe_error(rt: &Rt, mut e: V) -> String {
    for _ in 0..64 {
        let cause = reference(rt.get_field(e, "cause"));
        if cause.r() == 0 || cause == e {
            break;
        }
        e = cause;
    }
    match rt.str_of(rt.get_field(e, "message")) {
        Some(m) => m.to_owned(),
        None => rt.class_of(e).replace('/', "."),
    }
}

/// `finishParsing`'s answer to a parse error: the message in red, then, where it knows the place,
/// the line's last ten characters before it in grey, the rest red and underlined, and `<--[HERE]`;
/// clicking it puts the command back in the chat box.
fn report(rt: &mut Rt, source: V, command: &str, e: V) {
    let raw = rt.get_field(e, "$rawMessage");
    feedback(rt, source, raw, true);
    let Some(input) = rt.str_of(rt.get_field(e, "$input")).map(str::to_owned) else { return };
    let cursor = match rt.get_field(e, "$cursor") {
        V::I(c) => c,
        _ => -1,
    };
    if cursor >= 0 {
        let line = context_line(command, &input, cursor);
        feedback_json(rt, source, line);
    }
}

/// The context line of a parse error at `cursor` (UTF-16 units) of `input`.
fn context_line(command: &str, input: &str, cursor: i32) -> serde_json::Value {
    use serde_json::json;
    let units: Vec<u16> = input.encode_utf16().collect();
    let i = (cursor.max(0) as usize).min(units.len());
    let mut extra = Vec::new();
    if i > 10 {
        extra.push(json!({ "text": "..." }));
    }
    extra.push(json!({ "text": String::from_utf16_lossy(&units[i.saturating_sub(10)..i]) }));
    if i < units.len() {
        extra.push(json!({ "text": String::from_utf16_lossy(&units[i..]), "color": "red", "underlined": true }));
    }
    extra.push(json!({ "translate": "command.context.here", "color": "red", "italic": true }));
    json!({ "text": "", "color": "gray", "click_event": { "action": "suggest_command", "command": format!("/{command}") }, "extra": extra })
}

/// [`feedback`] of a failure given as JSON: red where it names no colour; a mod's own output hears
/// its text.
fn feedback_json(rt: &mut Rt, source: V, mut json: serde_json::Value) {
    if rt.get_field(source, "$silent").i() != 0 {
        return;
    }
    if rt.get_field(source, "$output").r() != 0 {
        let text = json_text(&json);
        let s = rt.string(&text);
        let c = crate::api_common::component(rt, s, false, V::NULL);
        feedback(rt, source, c, true);
        return;
    }
    if let serde_json::Value::Object(o) = &mut json {
        o.entry("color").or_insert_with(|| "red".into());
    }
    let text = json.to_string();
    if let Some(h) = rt.host_mut() {
        h.command_feedback(&text);
    }
}

/// A component's text from its JSON: each part's text (a key where it is translated).
fn json_text(json: &serde_json::Value) -> String {
    let mut out = json.get("text").or_else(|| json.get("translate")).and_then(|t| t.as_str()).unwrap_or("").to_owned();
    for part in json.get("extra").and_then(|e| e.as_array()).into_iter().flatten() {
        out.push_str(&json_text(part));
    }
    out
}

/// `performCommand`'s answer to an exception that isn't a syntax error: `command.failed`, the
/// exception's message on hover.
fn command_failed(rt: &mut Rt, source: V, e: V) {
    let text = match rt.str_of(rt.get_field(e, "message")) {
        Some(m) => m.to_owned(),
        None => rt.class_of(e).replace('/', "."),
    };
    rt.thrown = Some(e);
    rt.report_thrown("a command");
    rt.thrown = None;
    let json = serde_json::json!({ "translate": "command.failed", "hover_event": { "action": "show_text", "value": { "text": text } } });
    feedback_json(rt, source, json);
}

/// Vanilla's execution of a chain (`ExecutionContext`): each redirect's modifier gives the sources
/// the next stage runs for, and the command runs for each source. A syntax error is said to its
/// source (silently skipped in a fork); a modifier's ends the command, as does any other exception
/// (`command.failed`). Forks stop at vanilla's default limit (`command.forkLimit`).
fn run_vanilla(rt: &mut Rt, contexts: &[V], source: V) {
    let Some((&executable, modifiers)) = contexts.split_last() else { return };
    let mut forked = false;
    let mut sources = vec![source];
    for &stage in modifiers {
        forked |= rt.get_field(stage, "$forks").i() != 0;
        let modifier = reference(rt.get_field(stage, "$modifier"));
        if modifier.r() == 0 {
            continue;
        }
        let mut next = Vec::new();
        for &s in &sources {
            let ctx = copy_for(rt, stage, s);
            let Some(got) = modify(rt, modifier, ctx) else {
                let e = rt.thrown.take().unwrap_or_default();
                rt.unwinding.clear();
                if !rt.instance_of(e, brigadier::SYNTAX) {
                    command_failed(rt, source, e);
                    return;
                }
                if forked {
                    continue;
                }
                let raw = rt.get_field(e, "$rawMessage");
                feedback(rt, s, raw, true);
                return;
            };
            if next.len() + got.len() >= COMMAND_FAILED_LIMIT {
                if !forked {
                    let limit = Syntax { key: "command.forkLimit", args: vec![Arg::Int(COMMAND_FAILED_LIMIT as i32)], cursor: -1 };
                    let message = brigadier::message(rt, &limit);
                    feedback(rt, source, message, true);
                }
                return;
            }
            next.extend(got);
        }
        sources = next;
    }
    let command = rt.get_field(executable, "$command");
    for s in sources {
        let ctx = copy_for(rt, executable, s);
        rt.call_functional(RUN, command, &[ctx]);
        let Some(e) = rt.thrown.take() else { continue };
        rt.unwinding.clear();
        if !rt.instance_of(e, brigadier::SYNTAX) {
            command_failed(rt, source, e);
            return;
        }
        if !forked {
            let raw = rt.get_field(e, "$rawMessage");
            feedback(rt, s, raw, true);
        }
    }
}

/// What a redirect's modifier gives for `ctx`: the sources the rest runs for (the one a
/// `SingleRedirectModifier` makes, as Brigadier's `redirect` wraps it). None when it threw (the
/// exception left in `rt.thrown`).
fn modify(rt: &mut Rt, modifier: V, ctx: V) -> Option<Vec<V>> {
    let single = rt.class_of(modifier) == SINGLE_MODIFIER;
    let given = if single {
        let m = rt.get_field(modifier, "$single");
        rt.call_functional(MODIFY_SINGLE, m, &[ctx])
    } else {
        rt.call_functional(MODIFY, modifier, &[ctx])
    };
    if rt.thrown.is_some() {
        return None;
    }
    Some(if single { vec![given] } else { crate::jutil::elements(rt, given) })
}

/// Brigadier's own execution (`CommandDispatcher.execute`, which a mod may call): the result summed
/// over the sources (one for each in a fork); a syntax error is thrown out of it, but in a fork,
/// where that source is left out. Err leaves the exception in `rt.thrown`.
fn run_brigadier(rt: &mut Rt, contexts: &[V], source: V) -> Result<i32, ()> {
    let Some((&executable, modifiers)) = contexts.split_last() else { return Ok(0) };
    let mut forked = false;
    let mut sources = vec![source];
    let skipped = |rt: &mut Rt, forked: bool| -> Result<bool, ()> {
        let Some(e) = rt.thrown else { return Ok(false) };
        if forked && rt.instance_of(e, brigadier::SYNTAX) {
            rt.thrown = None;
            rt.unwinding.clear();
            return Ok(true);
        }
        Err(())
    };
    for &stage in modifiers {
        forked |= rt.get_field(stage, "$forks").i() != 0;
        let modifier = reference(rt.get_field(stage, "$modifier"));
        let mut next = Vec::new();
        for &s in &sources {
            if modifier.r() == 0 {
                next.push(s);
                continue;
            }
            let ctx = copy_for(rt, stage, s);
            let given = modify(rt, modifier, ctx);
            if !skipped(rt, forked)? {
                next.extend(given.unwrap_or_default());
            }
        }
        if next.is_empty() {
            return Ok(0);
        }
        sources = next;
    }
    let command = rt.get_field(executable, "$command");
    let mut result = 0i32;
    for s in sources {
        let ctx = copy_for(rt, executable, s);
        let r = rt.call_functional(RUN, command, &[ctx]).i();
        if !skipped(rt, forked)? {
            result = result.wrapping_add(if forked { 1 } else { r });
        }
    }
    Ok(result)
}

/// `CommandDispatcher.execute(results)`: the parse error, or the unknown command, thrown; else the
/// chain run as Brigadier runs it.
fn execute_results(rt: &mut Rt, results: V) -> V {
    let r = brigadier::reader_of(rt, rt.get_field(results, "$reader"));
    match finished(rt, results, &r.line()) {
        Err(e) => {
            rt.thrown = Some(e);
            V::I(0)
        }
        Ok((top, contexts)) => {
            let source = rt.get_field(top, "$source");
            V::I(run_brigadier(rt, &contexts, source).unwrap_or(0))
        }
    }
}

/// The dispatcher's `parse` and `execute`, of a line (`String`), a `StringReader` or (`execute`)
/// `ParseResults`.
fn dispatcher_run(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> V {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let results = if desc.starts_with("(Lcom/mojang/brigadier/ParseResults;") {
        a(1)
    } else {
        let reader = if desc.starts_with("(Ljava/lang/String;") { Reader::new(rt.str_of(a(1)).unwrap_or("")) } else { brigadier::reader_of(rt, a(1)) };
        let root = root_of(rt, a(0));
        let parse = parse(rt, root, a(2), reader);
        results_object(rt, a(0), &parse)
    };
    if name == "parse" {
        return results;
    }
    execute_results(rt, results)
}

/// `CommandContext`'s own calls.
fn context_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let c = a(0);
    Some(match name {
        "getSource" => rt.get_field(c, "$source"),
        "getInput" => rt.get_field(c, "$input"),
        "getArgument" => {
            let want = rt.str_of(a(1)).unwrap_or("").to_owned();
            match argument(rt, c, &want) {
                Some(v) => boxed(rt, v),
                None => no_argument(rt, &want),
            }
        }
        "getCommand" => reference(rt.get_field(c, "$command")),
        "getChild" => reference(rt.get_field(c, "$child")),
        "getLastChild" => last_child(rt, c),
        "copyFor" => copy_for(rt, c, a(1)),
        "getRedirectModifier" => reference(rt.get_field(c, "$modifier")),
        "isForked" => V::I(rt.get_field(c, "$forks").i()),
        "getNodes" => list_field(rt, c, "$nodes"),
        "getRange" => rt.get_field(c, "$range"),
        "getRootNode" => rt.get_field(c, "$root"),
        "hasNodes" => V::I(i32::from(!crate::jutil::elements(rt, rt.get_field(c, "$nodes")).is_empty())),
        _ => return None,
    })
}

/// The last of a context's (or builder's) children: itself without one.
fn last_child(rt: &Rt, mut c: V) -> V {
    for _ in 0..MAX_DEPTH {
        let child = reference(rt.get_field(c, "$child"));
        if child.r() == 0 {
            break;
        }
        c = child;
    }
    c
}

/// A list a context holds (an empty one where it holds none).
fn list_field(rt: &mut Rt, c: V, field: &'static str) -> V {
    match reference(rt.get_field(c, field)) {
        l if l.r() != 0 => l,
        _ => crate::jutil::new_list(rt, Vec::new()),
    }
}

/// `CommandContextBuilder`'s own calls.
fn builder_context_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let c = a(0);
    Some(match name {
        "getSource" => rt.get_field(c, "$source"),
        "getRootNode" => rt.get_field(c, "$root"),
        "getCommand" => reference(rt.get_field(c, "$command")),
        "getChild" => reference(rt.get_field(c, "$child")),
        "getLastChild" => last_child(rt, c),
        "getRange" => rt.get_field(c, "$range"),
        "getNodes" => list_field(rt, c, "$nodes"),
        "getDispatcher" => rt.get_field(c, "$dispatcher"),
        "getRedirectModifier" => reference(rt.get_field(c, "$modifier")),
        "isForked" => V::I(rt.get_field(c, "$forks").i()),
        "withSource" => set(rt, c, "$source", a(1)),
        "build" => build_context(rt, c, a(1)),
        // Each argument's result and range (`ParsedArgument`), by name.
        "getArguments" => {
            let names = crate::jutil::elements(rt, rt.get_field(c, "$argNames"));
            let values = crate::jutil::elements(rt, rt.get_field(c, "$argValues"));
            let ranges = crate::jutil::elements(rt, rt.get_field(c, "$argRanges"));
            let map = rt.alloc("java/util/LinkedHashMap", Data::Table(Default::default()));
            for (i, name) in names.into_iter().enumerate() {
                let p = rt.new_object(PARSED_ARGUMENT);
                let value = values.get(i).copied().unwrap_or(V::NULL);
                let value = boxed(rt, value);
                rt.put_field(p, "$result", value);
                rt.put_field(p, "$range", ranges.get(i).copied().unwrap_or(V::NULL));
                crate::jutil::table_put(rt, map, name, p);
            }
            map
        }
        _ => return None,
    })
}

/// `ParseResults`, `ParsedCommandNode`, `ParsedArgument` and `StringRange`.
fn parse_object_call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let o = a(0);
    Some(match (owner, name) {
        (PARSE_RESULTS, "<init>") => {
            rt.put_field(o, "$context", a(1));
            let r = brigadier::reader_of(rt, a(2));
            let reader = brigadier::new_reader(rt, &r);
            rt.put_field(o, "$reader", reader);
            let errors = if desc.contains("Ljava/util/Map;") { a(3) } else { rt.alloc("java/util/LinkedHashMap", Data::Table(Default::default())) };
            rt.put_field(o, "$exceptions", errors);
            V::Void
        }
        (PARSE_RESULTS, "getContext") => rt.get_field(o, "$context"),
        (PARSE_RESULTS, "getReader") => rt.get_field(o, "$reader"),
        (PARSE_RESULTS, "getExceptions") => rt.get_field(o, "$exceptions"),
        (PARSED_NODE, "getNode") => rt.get_field(o, "$node"),
        (PARSED_NODE | PARSED_ARGUMENT, "getRange") => rt.get_field(o, "$range"),
        (PARSED_ARGUMENT, "getResult") => rt.get_field(o, "$result"),
        (PARSED_NODE, "<init>") => {
            rt.put_field(o, "$node", a(1));
            set(rt, o, "$range", a(2));
            V::Void
        }
        (STRING_RANGE, n) => {
            let (start, end) = range_of(rt, o);
            match n {
                "<init>" => {
                    rt.put_field(o, "$start", a(1));
                    rt.put_field(o, "$end", a(2));
                    V::Void
                }
                "at" => string_range(rt, (a(0).i(), a(0).i())),
                "between" => string_range(rt, (a(0).i(), a(1).i())),
                "encompassing" => {
                    let (x, y) = (range_of(rt, a(0)), range_of(rt, a(1)));
                    string_range(rt, (x.0.min(y.0), x.1.max(y.1)))
                }
                "getStart" => V::I(start),
                "getEnd" => V::I(end),
                "isEmpty" => V::I(i32::from(start == end)),
                "getLength" => V::I(end - start),
                // `get(string)` or `get(reader)`: the range's text.
                "get" => {
                    let text = match rt.str_of(a(1)) {
                        Some(s) => s.to_owned(),
                        None => brigadier::reader_of(rt, a(1)).line(),
                    };
                    let r = Reader::new(&text);
                    let part = r.text(start, end);
                    rt.string(&part)
                }
                _ => return None,
            }
        }
        _ => return None,
    })
}

/// NeoForge's `CommandEvent`: the results it carries (a listener may give others), cancelling,
/// and the exception to say when cancelled.
fn command_event_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let ev = a(0);
    Some(match name {
        "getParseResults" => rt.get_field(ev, "$parseResults"),
        "setParseResults" => set(rt, ev, "$parseResults", a(1)),
        "getException" => reference(rt.get_field(ev, "$exception")),
        "setException" => set(rt, ev, "$exception", a(1)),
        "setCanceled" => set(rt, ev, "$canceled", V::I(a(1).i())),
        "isCanceled" => V::I(rt.get_field(ev, "$canceled").i()),
        _ => return None,
    })
    .map(|v| if matches!(name, "setParseResults" | "setException" | "setCanceled") { V::Void } else { v })
}

/// The argument named `name` of a command's context.
fn argument(rt: &Rt, ctx: V, name: &str) -> Option<V> {
    let list = |field: &'static str| crate::jutil::elements(rt, rt.get_field(ctx, field));
    let (names, values) = (list("$argNames"), list("$argValues"));
    names.iter().rposition(|n| rt.str_of(*n) == Some(name)).and_then(|i| values.get(i).copied())
}

/// A primitive argument as `getArgument` gives it, boxed.
fn boxed(rt: &mut Rt, v: V) -> V {
    let class = match v {
        V::I(n) => return crate::api_common::integer(rt, n),
        V::J(_) => "java/lang/Long",
        V::F(_) => "java/lang/Float",
        V::D(_) => "java/lang/Double",
        other => return other,
    };
    let o = rt.new_object(class);
    rt.put_field(o, "value", v);
    o
}

/// The index of a descriptor's first parameter of type `ty`.
fn param_index(desc: &str, ty: &str) -> Option<usize> {
    let params = desc.strip_prefix('(')?.split(')').next()?;
    let mut i = 0;
    let mut rest = params;
    while !rest.is_empty() {
        let len = match rest.as_bytes()[0] {
            b'L' => rest.find(';')? + 1,
            b'[' => {
                let dims = rest.bytes().take_while(|b| *b == b'[').count();
                dims + if rest.as_bytes()[dims] == b'L' { rest[dims..].find(';')? + 1 } else { 1 }
            }
            _ => 1,
        };
        if &rest[..len] == ty {
            return Some(i);
        }
        rest = &rest[len..];
        i += 1;
    }
    None
}

/// Whether a block position is in the world's height and its ±30,000,000 border
/// (`Level.isInWorldBounds`).
fn in_world(rt: &Rt, p: [i32; 3]) -> bool {
    let (min, height) = rt.host().map_or((-64, 384), |h| h.world_height());
    p[1] >= min && i64::from(p[1]) < i64::from(min) + i64::from(height) && horizontal(p)
}

fn horizontal(p: [i32; 3]) -> bool {
    (-30_000_000..30_000_000).contains(&p[0]) && (-30_000_000..30_000_000).contains(&p[2])
}

/// An argument type's getter (`IntegerArgumentType.getInteger(ctx, name)`,
/// `EntityArgument.getPlayer(ctx, name)`...): the parsed value as that getter gives it, resolved
/// with the context's source.
fn get_argument(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let ctx = *args.first()?;
    let want = rt.str_of(*args.get(param_index(desc, "Ljava/lang/String;")?)?).unwrap_or("").to_owned();
    let getter = (simple(owner), name);
    let known = matches!(
        getter,
        ("StringArgumentType", "getString")
            | ("IntegerArgumentType", "getInteger")
            | ("LongArgumentType", "getLong")
            | ("FloatArgumentType", "getFloat")
            | ("DoubleArgumentType", "getDouble")
            | ("BoolArgumentType", "getBool")
            | ("EntityArgument", "getEntity" | "getPlayer" | "getEntities" | "getPlayers" | "getOptionalEntities" | "getOptionalPlayers")
            | ("GameProfileArgument", "getGameProfiles")
            | ("BlockPosArgument", "getBlockPos" | "getLoadedBlockPos" | "getSpawnablePos")
            | ("Vec3Argument", "getVec3" | "getCoordinates")
            | ("Vec2Argument", "getVec2")
            | ("ColumnPosArgument", "getColumnPos")
            | ("RotationArgument", "getRotation")
            | ("AngleArgument", "getAngle")
            | ("ResourceLocationArgument", "getId")
            | ("DimensionArgument", "getDimension")
            | ("ItemArgument", "getItem")
            | ("BlockStateArgument", "getBlock")
            | ("UuidArgument", "getUuid")
            | ("MessageArgument", "getMessage")
            | ("ColorArgument", "getColor")
            | ("TimeArgument", "getTime")
    );
    if !known {
        return None;
    }
    let Some(v) = argument(rt, ctx, &want) else {
        return Some(no_argument(rt, &want));
    };
    let source = rt.get_field(ctx, "$source");
    Some(match getter {
        ("BoolArgumentType", _) => match v {
            V::R(_) => rt.get_field(v, "value"),
            other => other,
        },
        ("EntityArgument", n) => entity_getter(rt, n, v, source),
        ("GameProfileArgument", _) => match command_args::profiles_of(rt, v, source) {
            Ok(found) => crate::jutil::new_list(rt, found),
            Err(e) => throw(rt, &e),
        },
        ("BlockPosArgument", n) => {
            let p = command_args::block_pos(rt, v, source);
            if n == "getLoadedBlockPos" && !in_world(rt, p) {
                return Some(throw_syntax(rt, "argument.pos.outofworld", Vec::new()));
            }
            if n == "getSpawnablePos" && !((-20_000_000..20_000_000).contains(&p[1]) && horizontal(p)) {
                return Some(throw_syntax(rt, "argument.pos.outofbounds", Vec::new()));
            }
            api::block_pos(rt, i64::from(p[0]), i64::from(p[1]), i64::from(p[2]))
        }
        ("Vec3Argument", "getVec3") => {
            let [x, y, z] = command_args::position(rt, v, source);
            crate::api_common::vec3(rt, x, y, z)
        }
        ("Vec2Argument", _) => {
            let p = command_args::position(rt, v, source);
            crate::api_common::vec2(rt, p[0] as f32, p[2] as f32)
        }
        ("ColumnPosArgument", _) => {
            let [x, _, z] = command_args::block_pos(rt, v, source);
            crate::records::make(rt, "net/minecraft/server/level/ColumnPos", &[V::I(x), V::I(z)])
        }
        ("AngleArgument", _) => V::F(command_args::angle_of(rt, v, source)),
        ("DimensionArgument", _) => {
            let here = rt.host().map(|h| h.dimension()).unwrap_or_default();
            let wanted = rt.str_of(v).unwrap_or("").to_owned();
            if !here.is_empty() && wanted != here {
                return Some(throw_syntax(rt, "argument.dimension.invalid", vec![Arg::Text(wanted)]));
            }
            crate::glue::level(rt)
        }
        ("MessageArgument", _) => match command_args::message_component(rt, v, source) {
            Ok(c) => c,
            Err(e) => throw(rt, &e),
        },
        _ => v,
    })
}

/// `EntityArgument`'s getters: what the selector finds for the source, one or (for the plural
/// getters but the optional ones) at least one.
fn entity_getter(rt: &mut Rt, name: &str, selector: V, source: V) -> V {
    let players_only = matches!(name, "getPlayer" | "getPlayers" | "getOptionalPlayers") || rt.get_field(selector, "$entities").i() == 0;
    let found = match command_args::select(rt, selector, source, players_only) {
        Ok(found) => found,
        Err(e) => return throw(rt, &e),
    };
    match name {
        "getEntity" => match found.len() {
            0 => throw_syntax(rt, "argument.entity.notfound.entity", Vec::new()),
            1 => found[0],
            _ => throw_syntax(rt, "argument.entity.toomany", Vec::new()),
        },
        "getPlayer" if found.len() != 1 => throw_syntax(rt, "argument.entity.notfound.player", Vec::new()),
        "getPlayer" => found[0],
        "getEntities" if found.is_empty() => throw_syntax(rt, "argument.entity.notfound.entity", Vec::new()),
        "getPlayers" if found.is_empty() => throw_syntax(rt, "argument.entity.notfound.player", Vec::new()),
        _ => crate::jutil::new_list(rt, found),
    }
}

/// A selector's own calls (a mod's `EntitySelector.findPlayers(source)`...).
fn selector_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let s = a(0);
    let includes = rt.get_field(s, "$entities").i() != 0;
    Some(match name {
        "findEntities" | "findPlayers" => match command_args::select(rt, s, a(1), name == "findPlayers" || !includes) {
            Ok(found) => crate::jutil::new_list(rt, found),
            Err(e) => throw(rt, &e),
        },
        "findSingleEntity" | "findSinglePlayer" => {
            let player = name == "findSinglePlayer";
            match command_args::select(rt, s, a(1), player || !includes) {
                Ok(found) if found.len() == 1 => found[0],
                Ok(found) if found.len() > 1 && !player => throw_syntax(rt, "argument.entity.toomany", Vec::new()),
                Ok(_) => throw_syntax(rt, if player { "argument.entity.notfound.player" } else { "argument.entity.notfound.entity" }, Vec::new()),
                Err(e) => throw(rt, &e),
            }
        }
        "includesEntities" => V::I(i32::from(includes)),
        "getMaxResults" => rt.get_field(s, "$max"),
        "isSelfSelector" => V::I(rt.get_field(s, "$current").i()),
        "usesSelector" => V::I(rt.get_field(s, "$uses").i()),
        _ => return None,
    })
}

/// `EntitySelectorParser`, which a mod's own argument type may read a selector with
/// (`new EntitySelectorParser(reader, allowSelectors).parse()`, FTB Teams' `TeamArgument`):
/// vanilla's selector parser on the mod's reader, its cursor left where vanilla's leaves it.
fn selector_parser_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let p = a(0);
    Some(match name {
        "<init>" => {
            rt.put_field(p, "$reader", a(1));
            rt.put_field(p, "$allow", V::I(a(2).i()));
            V::Void
        }
        "getReader" => rt.get_field(p, "$reader"),
        "parse" => {
            let reader = rt.get_field(p, "$reader");
            let mut r = brigadier::reader_of(rt, reader);
            let start = r.cursor;
            let read = command_args::parse_selector(&mut r, rt.get_field(p, "$allow").i() != 0);
            rt.put_field(reader, "$cursor", V::I(r.cursor));
            match read {
                Ok(s) => {
                    let text = r.text(start, r.cursor);
                    command_args::selector_object(rt, &s, &text)
                }
                Err(e) => {
                    let ex = brigadier::exception(rt, &r.s, &e);
                    rt.thrown = Some(ex);
                    V::NULL
                }
            }
        }
        _ => return None,
    })
}

/// The objects the vanilla types read (coordinates, angles, messages, profiles, items, blocks):
/// what they resolve to for the source a mod gives.
fn read_value_call(rt: &mut Rt, owner: &str, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let o = a(0);
    Some(match (owner, name) {
        (COORDINATES | command_args::WORLD_COORDINATES | command_args::LOCAL_COORDINATES, n) => match n {
            "getPosition" => {
                let [x, y, z] = command_args::position(rt, o, a(1));
                crate::api_common::vec3(rt, x, y, z)
            }
            "getBlockPos" => {
                let [x, y, z] = command_args::block_pos(rt, o, a(1));
                api::block_pos(rt, i64::from(x), i64::from(y), i64::from(z))
            }
            "getRotation" => {
                let [pitch, yaw] = command_args::rotation(rt, o, a(1));
                crate::api_common::vec2(rt, pitch, yaw)
            }
            "isXRelative" | "isYRelative" | "isZRelative" => V::I(i32::from(command_args::is_relative(rt, o, usize::from(n.as_bytes()[2] - b'X')))),
            _ => return None,
        },
        (command_args::SINGLE_ANGLE, "getAngle") => V::F(command_args::angle_of(rt, o, a(1))),
        (command_args::MESSAGE, "toComponent" | "resolveComponent") => match command_args::message_component(rt, o, a(1)) {
            Ok(c) => c,
            Err(e) => throw(rt, &e),
        },
        (command_args::MESSAGE, "getText") => rt.get_field(o, "$text"),
        (command_args::PROFILES, "getNames") => match command_args::profiles_of(rt, o, a(1)) {
            Ok(found) => crate::jutil::new_list(rt, found),
            Err(e) => throw(rt, &e),
        },
        (command_args::ITEM_INPUT, n) => match n {
            "getItem" => rt.get_field(o, "$item"),
            // `createItemStack(count, allowOversizedStacks)`: more than a stack holds is an error.
            "createItemStack" => {
                let id = rt.str_of(rt.get_field(o, "$id")).unwrap_or("minecraft:air").to_owned();
                let limit = rt.host().and_then(|h| h.item_stack_limit(&id)).unwrap_or(64);
                if a(2).i() != 0 && i64::from(a(1).i()) > i64::from(limit) {
                    return Some(throw_syntax(rt, "arguments.item.overstacked", vec![Arg::Text(id), Arg::Int(limit as i32)]));
                }
                crate::api_common::stack(rt, &id, a(1).i())
            }
            "serialize" => {
                let id = rt.str_of(rt.get_field(o, "$id")).unwrap_or("").to_owned();
                let components = rt.str_of(rt.get_field(o, "$components")).unwrap_or("").to_owned();
                rt.string(&format!("{id}{components}"))
            }
            _ => return None,
        },
        (command_args::BLOCK_INPUT, n) => match n {
            "getState" => rt.get_field(o, "$state"),
            // `place(level, pos, flags)`: the state set there.
            "place" => {
                let text = rt.str_of(rt.get_field(o, "$text")).unwrap_or("minecraft:air").to_owned();
                let p = api::pos_of(rt, a(2));
                V::I(rt.host_mut().is_some_and(|h| h.set_block_state(p, &text)) as i32)
            }
            _ => return None,
        },
        _ => return None,
    })
}

/// Throws a `CommandSyntaxException` of 1.21.1's message `key` made without a reader (`create`).
fn throw_syntax(rt: &mut Rt, key: &'static str, args: Vec<Arg>) -> V {
    throw(rt, &Syntax { key, args, cursor: -1 })
}

/// Throws `e` as a `CommandSyntaxException` (with no place in a line).
fn throw(rt: &mut Rt, e: &Syntax) -> V {
    let ex = brigadier::exception_without_context(rt, e);
    rt.thrown = Some(ex);
    V::NULL
}

#[cfg(test)]
mod tests {
    use super::*;

    const LITERAL: &str = "net/minecraft/commands/Commands.literal.(Ljava/lang/String;)Lcom/mojang/brigadier/builder/LiteralArgumentBuilder;";
    const ARGUMENT: &str = "net/minecraft/commands/Commands.argument.(Ljava/lang/String;Lcom/mojang/brigadier/arguments/ArgumentType;)Lcom/mojang/brigadier/builder/RequiredArgumentBuilder;";
    const THEN: &str = "com/mojang/brigadier/builder/LiteralArgumentBuilder.then.(Lcom/mojang/brigadier/builder/ArgumentBuilder;)Lcom/mojang/brigadier/builder/ArgumentBuilder;";
    const THEN_ARG: &str = "com/mojang/brigadier/builder/RequiredArgumentBuilder.then.(Lcom/mojang/brigadier/builder/ArgumentBuilder;)Lcom/mojang/brigadier/builder/ArgumentBuilder;";
    const THEN_NODE: &str = "com/mojang/brigadier/builder/LiteralArgumentBuilder.then.(Lcom/mojang/brigadier/tree/CommandNode;)Lcom/mojang/brigadier/builder/ArgumentBuilder;";
    const EXECUTES: &str = "com/mojang/brigadier/builder/LiteralArgumentBuilder.executes.(Lcom/mojang/brigadier/Command;)Lcom/mojang/brigadier/builder/ArgumentBuilder;";
    const EXECUTES_ARG: &str = "com/mojang/brigadier/builder/RequiredArgumentBuilder.executes.(Lcom/mojang/brigadier/Command;)Lcom/mojang/brigadier/builder/ArgumentBuilder;";
    const REDIRECT_ARG: &str = "com/mojang/brigadier/builder/RequiredArgumentBuilder.redirect.(Lcom/mojang/brigadier/tree/CommandNode;Lcom/mojang/brigadier/SingleRedirectModifier;)Lcom/mojang/brigadier/builder/ArgumentBuilder;";
    const FORK_ARG: &str = "com/mojang/brigadier/builder/RequiredArgumentBuilder.fork.(Lcom/mojang/brigadier/tree/CommandNode;Lcom/mojang/brigadier/RedirectModifier;)Lcom/mojang/brigadier/builder/ArgumentBuilder;";
    const REQUIRES: &str = "com/mojang/brigadier/builder/LiteralArgumentBuilder.requires.(Ljava/util/function/Predicate;)Lcom/mojang/brigadier/builder/ArgumentBuilder;";
    const BUILD: &str = "com/mojang/brigadier/builder/LiteralArgumentBuilder.build.()Lcom/mojang/brigadier/tree/LiteralCommandNode;";
    const REGISTER: &str = "com/mojang/brigadier/CommandDispatcher.register.(Lcom/mojang/brigadier/builder/LiteralArgumentBuilder;)Lcom/mojang/brigadier/tree/LiteralCommandNode;";
    const GET_ROOT: &str = "com/mojang/brigadier/CommandDispatcher.getRoot.()Lcom/mojang/brigadier/tree/RootCommandNode;";
    const ADD_CHILD: &str = "com/mojang/brigadier/tree/RootCommandNode.addChild.(Lcom/mojang/brigadier/tree/CommandNode;)V";
    const GET_CHILDREN: &str = "com/mojang/brigadier/tree/LiteralCommandNode.getChildren.()Ljava/util/Collection;";
    const LITERAL_OF: &str = "com/mojang/brigadier/builder/LiteralArgumentBuilder.literal.(Ljava/lang/String;)Lcom/mojang/brigadier/builder/LiteralArgumentBuilder;";
    const GET_DISPATCHER: &str = "net/neoforged/neoforge/event/RegisterCommandsEvent.getDispatcher.()Lcom/mojang/brigadier/CommandDispatcher;";
    const INTEGER: &str = "com/mojang/brigadier/arguments/IntegerArgumentType.integer.(II)Lcom/mojang/brigadier/arguments/IntegerArgumentType;";
    const GET_INTEGER: &str = "com/mojang/brigadier/arguments/IntegerArgumentType.getInteger.(Lcom/mojang/brigadier/context/CommandContext;Ljava/lang/String;)I";
    const WORD: &str = "com/mojang/brigadier/arguments/StringArgumentType.word.()Lcom/mojang/brigadier/arguments/StringArgumentType;";
    const GET_STRING: &str = "com/mojang/brigadier/arguments/StringArgumentType.getString.(Lcom/mojang/brigadier/context/CommandContext;Ljava/lang/String;)Ljava/lang/String;";
    const PLAYER: &str = "net/minecraft/commands/arguments/EntityArgument.player.()Lnet/minecraft/commands/arguments/EntityArgument;";
    const PLAYERS: &str = "net/minecraft/commands/arguments/EntityArgument.players.()Lnet/minecraft/commands/arguments/EntityArgument;";
    const GET_PLAYER: &str = "net/minecraft/commands/arguments/EntityArgument.getPlayer.(Lcom/mojang/brigadier/context/CommandContext;Ljava/lang/String;)Lnet/minecraft/server/level/ServerPlayer;";
    const GET_PLAYERS: &str = "net/minecraft/commands/arguments/EntityArgument.getPlayers.(Lcom/mojang/brigadier/context/CommandContext;Ljava/lang/String;)Ljava/util/Collection;";
    const BLOCK_POS: &str = "net/minecraft/commands/arguments/coordinates/BlockPosArgument.blockPos.()Lnet/minecraft/commands/arguments/coordinates/BlockPosArgument;";
    const GET_BLOCK_POS: &str = "net/minecraft/commands/arguments/coordinates/BlockPosArgument.getLoadedBlockPos.(Lcom/mojang/brigadier/context/CommandContext;Ljava/lang/String;)Lnet/minecraft/core/BlockPos;";
    const CUSTOM: &str = "test/ColourArgument";
    const HAS_PERMISSION: &str = "net/minecraft/commands/CommandSourceStack.hasPermission.(I)Z";
    const SEND_SUCCESS: &str = "net/minecraft/commands/CommandSourceStack.sendSuccess.(Ljava/util/function/Supplier;Z)V";
    const GET_PLAYER_OR_EXCEPTION: &str = "net/minecraft/commands/CommandSourceStack.getPlayerOrException.()Lnet/minecraft/server/level/ServerPlayer;";
    const WITH_ENTITY: &str = "net/minecraft/commands/CommandSourceStack.withEntity.(Lnet/minecraft/world/entity/Entity;)Lnet/minecraft/commands/CommandSourceStack;";
    const GET_SOURCE: &str = "com/mojang/brigadier/context/CommandContext.getSource.()Ljava/lang/Object;";
    const GET_ARGUMENT: &str = "com/mojang/brigadier/context/CommandContext.getArgument.(Ljava/lang/String;Ljava/lang/Class;)Ljava/lang/Object;";
    const READ_WORD: &str = "com/mojang/brigadier/StringReader.readUnquotedString.()Ljava/lang/String;";
    const GET_CURSOR: &str = "com/mojang/brigadier/StringReader.getCursor.()I";
    const SET_CURSOR: &str = "com/mojang/brigadier/StringReader.setCursor.(I)V";
    const SIMPLE_TYPE: &str = "com/mojang/brigadier/exceptions/SimpleCommandExceptionType";
    const LITERAL_MESSAGE: &str = "com/mojang/brigadier/LiteralMessage";

    fn c(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        crate::api::call(rt, key, args)
    }
    fn s(rt: &mut Rt, t: &str) -> V {
        rt.string(t)
    }

    /// The test mod's own argument type, `test/ColourArgument`: `parse` reads a word with the
    /// `StringReader` object it is given, a colour of three or else its own error back at the
    /// word's start (`SimpleCommandExceptionType` of a `LiteralMessage`); its examples are words.
    fn vcall(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {
        if rt.class_of(args[0]) != CUSTOM {
            return crate::api::call(rt, owner_key, args);
        }
        if key == GET_EXAMPLES {
            let items = vec![rt.string("red"), rt.string("green")];
            return crate::jutil::new_list(rt, items);
        }
        assert_eq!(key, PARSE);
        let reader = args[1];
        let start = c(rt, GET_CURSOR, &[reader]);
        let word = c(rt, READ_WORD, &[reader]);
        if matches!(rt.str_of(word), Some("red" | "green" | "blue")) {
            return word;
        }
        c(rt, SET_CURSOR, &[reader, start]);
        let message = rt.new_object(LITERAL_MESSAGE);
        let text = rt.string("Unknown colour");
        c(rt, "com/mojang/brigadier/LiteralMessage.<init>.(Ljava/lang/String;)V", &[message, text]);
        let ty = rt.new_object(SIMPLE_TYPE);
        c(rt, "com/mojang/brigadier/exceptions/SimpleCommandExceptionType.<init>.(Lcom/mojang/brigadier/Message;)V", &[ty, message]);
        let e = c(
            rt,
            "com/mojang/brigadier/exceptions/SimpleCommandExceptionType.createWithContext.(Lcom/mojang/brigadier/ImmutableStringReader;)Lcom/mojang/brigadier/exceptions/CommandSyntaxException;",
            &[ty, reader],
        );
        rt.thrown = Some(e);
        V::NULL
    }

    fn overrides(class: &str, key: &str) -> bool {
        class == CUSTOM && (key == PARSE || key == GET_EXAMPLES)
    }

    /// The test mod: lambda 1 registers its tree, as Create's `AllCommands` does (a root with
    /// subcommands, a node built apart and added twice, an alias copying the root's children):
    ///
    /// - `/ruby give <count 1..64>` and `/ruby give <count> <player>`,
    /// - `/ruby spot <pos>`, `/ruby say <word>`, an operator's `/ruby wipe`,
    /// - `/ruby tint <colour>`, of a type of the mod's own,
    /// - `/rb ...`, the root's children again.
    ///
    /// 2 is `hasPermission(2)`; 3 to 7 and 9 run the commands, saying what they read; 8 is a
    /// message's supplier.
    fn lambdas(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
        let captured = rt.captured(args[0]);
        Some(match id {
            1 => {
                let dispatcher = c(rt, GET_DISPATCHER, &[args[1]]);
                let lit = |rt: &mut Rt, t: &str| {
                    let n = rt.string(t);
                    c(rt, LITERAL, &[n])
                };
                let arg = |rt: &mut Rt, name: &str, t: V| {
                    let n = rt.string(name);
                    c(rt, ARGUMENT, &[n, t])
                };
                let run = |rt: &mut Rt, id: u32| rt.lambda_id("com/mojang/brigadier/Command", id, vec![]);
                let player_type = c(rt, PLAYER, &[]);
                let to = arg(rt, "player", player_type);
                let r = run(rt, 4);
                let to = c(rt, EXECUTES_ARG, &[to, r]);
                let count_type = c(rt, INTEGER, &[V::I(1), V::I(64)]);
                let count = arg(rt, "count", count_type);
                let r = run(rt, 3);
                let count = c(rt, EXECUTES_ARG, &[count, r]);
                let count = c(rt, THEN_ARG, &[count, to]);
                let give = lit(rt, "give");
                let give = c(rt, THEN, &[give, count]);
                let pos_type = c(rt, BLOCK_POS, &[]);
                let pos = arg(rt, "pos", pos_type);
                let r = run(rt, 5);
                let pos = c(rt, EXECUTES_ARG, &[pos, r]);
                let spot = lit(rt, "spot");
                let spot = c(rt, THEN, &[spot, pos]);
                let word_type = c(rt, WORD, &[]);
                let word = arg(rt, "word", word_type);
                let r = run(rt, 6);
                let word = c(rt, EXECUTES_ARG, &[word, r]);
                let say = lit(rt, "say");
                let say = c(rt, THEN, &[say, word]);
                let say_node = c(rt, BUILD, &[say]);
                let op = rt.lambda_id("java/util/function/Predicate", 2, vec![]);
                let wipe = lit(rt, "wipe");
                let wipe = c(rt, REQUIRES, &[wipe, op]);
                let r = run(rt, 7);
                let wipe = c(rt, EXECUTES, &[wipe, r]);
                let colour = rt.new_object(CUSTOM);
                let tint = arg(rt, "colour", colour);
                let r = run(rt, 9);
                let tint = c(rt, EXECUTES_ARG, &[tint, r]);
                let tint_lit = lit(rt, "tint");
                let tint_lit = c(rt, THEN, &[tint_lit, tint]);
                let root = lit(rt, "ruby");
                for child in [give, spot, wipe, tint_lit] {
                    c(rt, THEN, &[root, child]);
                }
                c(rt, THEN_NODE, &[root, say_node]);
                let ruby = c(rt, REGISTER, &[dispatcher, root]);
                // The alias: a literal with the root's children, added to the dispatcher's root.
                let rb_name = rt.string("rb");
                let rb = c(rt, LITERAL_OF, &[rb_name]);
                let kids = c(rt, GET_CHILDREN, &[ruby]);
                for child in crate::jutil::elements(rt, kids) {
                    c(rt, THEN_NODE, &[rb, child]);
                }
                let rb = c(rt, BUILD, &[rb]);
                let root_node = c(rt, GET_ROOT, &[dispatcher]);
                c(rt, ADD_CHILD, &[root_node, rb]);
                V::Void
            }
            2 => c(rt, HAS_PERMISSION, &[args[1], V::I(2)]),
            3..=7 | 9 => {
                let ctx = args[1];
                let source = c(rt, GET_SOURCE, &[ctx]);
                let said = match id {
                    3 => {
                        let n = s(rt, "count");
                        let count = c(rt, GET_INTEGER, &[ctx, n]).i();
                        format!("gave {count}")
                    }
                    4 => {
                        let (n, p) = (s(rt, "count"), s(rt, "player"));
                        let count = c(rt, GET_INTEGER, &[ctx, n]).i();
                        let player = c(rt, GET_PLAYER, &[ctx, p]);
                        if rt.thrown.is_some() {
                            return Some(V::I(0));
                        }
                        format!("gave {count} to {}", command_args::player_name(rt, player))
                    }
                    5 => {
                        let n = s(rt, "pos");
                        let pos = c(rt, GET_BLOCK_POS, &[ctx, n]);
                        let p = api::pos_of(rt, pos);
                        format!("spot {} {} {}", p[0], p[1], p[2])
                    }
                    6 => {
                        let n = s(rt, "word");
                        let w = c(rt, GET_STRING, &[ctx, n]);
                        let me = c(rt, GET_PLAYER_OR_EXCEPTION, &[source]);
                        if rt.thrown.is_some() {
                            return Some(V::I(0));
                        }
                        format!("{} says {}", command_args::player_name(rt, me), rt.str_of(w).unwrap_or(""))
                    }
                    9 => {
                        let n = s(rt, "colour");
                        let class = rt.alloc("java/lang/Class", Data::Str("java/lang/String".into()));
                        let colour = c(rt, GET_ARGUMENT, &[ctx, n, class]);
                        format!("tint {}", rt.str_of(colour).unwrap_or(""))
                    }
                    _ => "wiped".to_owned(),
                };
                let text = s(rt, &said);
                let supplier = rt.lambda_id("java/util/function/Supplier", 8, vec![text]);
                c(rt, SEND_SUCCESS, &[source, supplier, V::I(0)]);
                V::I(1)
            }
            8 => crate::api_common::component(rt, captured[0], false, V::NULL),
            // A redirect's modifier: the source as the player the context read.
            10 => {
                let ctx = args[1];
                let source = c(rt, GET_SOURCE, &[ctx]);
                let n = s(rt, "who");
                let who = c(rt, GET_PLAYER, &[ctx, n]);
                c(rt, WITH_ENTITY, &[source, who])
            }
            // A fork's modifier: a source for each player the context read.
            11 => {
                let ctx = args[1];
                let source = c(rt, GET_SOURCE, &[ctx]);
                let n = s(rt, "all");
                let all = c(rt, GET_PLAYERS, &[ctx, n]);
                let mut each = Vec::new();
                for p in crate::jutil::elements(rt, all) {
                    each.push(c(rt, WITH_ENTITY, &[source, p]));
                }
                crate::jutil::new_list(rt, each)
            }
            _ => V::Void,
        })
    }

    #[derive(Default)]
    struct Chat {
        lines: Vec<String>,
        players: Vec<(u128, [f64; 3], &'static str)>,
    }
    impl crate::host::Host for Chat {
        fn block_state(&self, _pos: [i32; 3]) -> String {
            "minecraft:air".into()
        }
        fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {
            false
        }
        fn players(&self) -> Vec<(u128, [f64; 3])> {
            self.players.iter().map(|(id, at, _)| (*id, *at)).collect()
        }
        fn player_name(&self, uuid: u128) -> Option<String> {
            self.players.iter().find(|p| p.0 == uuid).map(|p| p.2.to_owned())
        }
        fn command_feedback(&mut self, component: &str) {
            self.lines.push(component.to_owned());
        }
    }

    fn chat_lines(chat: &mut Chat) -> Vec<serde_json::Value> {
        chat.lines.drain(..).map(|l| serde_json::from_str(&l).unwrap()).collect()
    }

    fn said(lines: &[serde_json::Value]) -> Vec<String> {
        lines.iter().map(|j| j["text"].as_str().or_else(|| j["translate"].as_str()).unwrap_or("").to_owned()).collect()
    }

    /// A mod's command tree, built with Brigadier's builders on the dispatcher
    /// `RegisterCommandsEvent` hands it: its runnable paths are the game's usages, each at the level
    /// its requirements ask, an alias's as its target's, an argument of the mod's own type of the
    /// kind its examples are. A player's line runs the command its path reaches, with the arguments
    /// its types read (a count within its bounds, a player by name, a position from where the
    /// player stands, a colour by the mod's own parser), and what the command says comes back to
    /// them. A line that doesn't parse is answered as vanilla answers it: the error (the bound and
    /// what was read, the mod's own message), then the line up to the error's place. A line of a
    /// word no root has isn't the mod's; the console runs a command as no player.
    #[test]
    fn a_mods_commands_register_and_run_as_brigadier_parses_them() {
        let mut rt = Rt::new();
        rt.lambda_call = Some(lambdas);
        rt.vcall = Some(vcall);
        rt.overrides = Some(overrides);
        let listener = rt.lambda_id("java/util/function/Consumer", 1, vec![]);
        rt.listeners.push((EVENT.to_owned(), listener));
        let (me, steve) = (0x11u128, 0x22u128);
        let mut chat = Chat { players: vec![(me, [10.5, 64.0, -3.5], "Alex"), (steve, [0.0, 70.0, 0.0], "Steve")], ..Chat::default() };
        let usages = rt.with_host(&mut chat, register);
        let mut expected = Vec::new();
        for root in ["ruby", "rb"] {
            for (usage, level) in [
                ("give <count:integer>", 0),
                ("give <count:integer> <player:player>", 0),
                ("spot <pos:block_pos>", 0),
                ("wipe", 2),
                ("tint <colour:word>", 0),
                ("say <word:word>", 0),
            ] {
                expected.push((format!("{root} {usage}"), level));
            }
        }
        assert_eq!(usages, expected);
        let mut run = |chat: &mut Chat, level: u8, line: &str| rt.with_host(chat, |rt| perform(rt, Some((me, "Alex")), level, [10.5, 64.0, -3.5], [0.0, 0.0], line));
        for line in ["ruby give 5", "ruby give 7 Steve", "rb spot ~1 ~ ~-2", "ruby spot 3 -60 4", "ruby say hello", "ruby tint red"] {
            assert!(run(&mut chat, 0, line), "{line}");
        }
        assert!(run(&mut chat, 2, "ruby wipe"));
        assert!(!run(&mut chat, 0, "stone 1"), "no root of the mod's");
        let lines = chat_lines(&mut chat);
        assert_eq!(said(&lines), ["gave 5", "gave 7 to Steve", "spot 11 64 -6", "spot 3 -60 4", "Alex says hello", "tint red", "wiped"]);
        // Errors, each with the line's context: the message, then the text before the place.
        let mut error = |chat: &mut Chat, line: &str| {
            assert!(run(chat, 0, line), "{line}");
            let lines = chat_lines(chat);
            assert_eq!(lines.len(), 2, "{line}: {lines:?}");
            assert_eq!(lines[0]["color"], "red", "{line}");
            let context = &lines[1];
            assert_eq!(context["color"], "gray");
            assert_eq!(context["click_event"]["command"], format!("/{line}"));
            let parts: Vec<String> = context["extra"].as_array().unwrap().iter().map(|p| p["text"].as_str().unwrap_or("<here>").to_owned()).collect();
            (lines[0].clone(), parts)
        };
        let (e, parts) = error(&mut chat, "ruby give 65");
        assert_eq!(e, serde_json::json!({"translate": "argument.integer.big", "with": [64, 65], "color": "red"}));
        assert_eq!(parts, ["ruby give ", "65", "<here>"]);
        let (e, parts) = error(&mut chat, "ruby give five");
        assert_eq!(e["translate"], "parsing.int.expected");
        assert_eq!(parts, ["ruby give ", "five", "<here>"]);
        let (e, parts) = error(&mut chat, "ruby give 5 Steve extra");
        assert_eq!(e["translate"], "command.unknown.argument");
        assert_eq!(parts, ["...", "e 5 Steve ", "extra", "<here>"]);
        let (e, parts) = error(&mut chat, "ruby wipe");
        assert_eq!(e["translate"], "command.unknown.argument", "an operator's");
        assert_eq!(parts, ["ruby ", "wipe", "<here>"]);
        let (e, parts) = error(&mut chat, "ruby tint purple");
        assert_eq!(e["text"], "Unknown colour", "the mod's own error");
        assert_eq!(parts, ["ruby tint ", "purple", "<here>"]);
        let (e, parts) = error(&mut chat, "ruby");
        assert_eq!(e["translate"], "command.unknown.command", "no command at the root");
        assert_eq!(parts, ["ruby", "<here>"]);
        // Two spaces: no literal is the empty word, and `ruby` has no argument to try.
        let (e, parts) = error(&mut chat, "ruby  say x");
        assert_eq!(e["translate"], "command.unknown.argument");
        assert_eq!(parts, ["ruby ", " say x", "<here>"]);
        // A command's own error: no place in the line.
        assert!(run(&mut chat, 0, "ruby give 2 Nobody"));
        let lines = chat_lines(&mut chat);
        assert_eq!(said(&lines), ["argument.entity.notfound.player"]);
        let console = rt.with_host(&mut chat, |rt| perform(rt, None, 4, [0.0; 3], [0.0; 2], "ruby say hi"));
        assert!(console);
        let lines = chat_lines(&mut chat);
        assert_eq!(said(&lines), ["permissions.requires.player"]);
        assert_eq!(lines[0]["color"], "red");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Redirects as Brigadier and vanilla run them: the line goes on at the target's children in a
    /// context of its own, and the modifier runs at execution, giving the sources the rest runs
    /// for: `/ruby as <who> say <word>` speaks as that player; `/ruby each <all> say <word>`
    /// forks, once for each player found. A mod's `dispatcher.execute` throws a line's parse
    /// error, with Brigadier's message.
    #[test]
    fn redirects_run_their_modifier_for_each_source() {
        let mut rt = Rt::new();
        rt.lambda_call = Some(lambdas);
        let (me, steve) = (0x11u128, 0x22u128);
        let mut chat = Chat { players: vec![(me, [0.0; 3], "Alex"), (steve, [0.0; 3], "Steve")], ..Chat::default() };
        let dispatcher = new_dispatcher(&mut rt);
        rt.interned.insert(DISPATCHER_KEY.into(), dispatcher);
        let name = |rt: &mut Rt, t: &str| rt.string(t);
        let n = name(&mut rt, "ruby");
        let root = c(&mut rt, LITERAL, &[n]);
        let n = name(&mut rt, "say");
        let say = c(&mut rt, LITERAL, &[n]);
        let word_type = c(&mut rt, WORD, &[]);
        let n = name(&mut rt, "word");
        let word = c(&mut rt, ARGUMENT, &[n, word_type]);
        let speak = rt.lambda_id("com/mojang/brigadier/Command", 6, vec![]);
        let word = c(&mut rt, EXECUTES_ARG, &[word, speak]);
        c(&mut rt, THEN, &[say, word]);
        c(&mut rt, THEN, &[root, say]);
        let ruby = c(&mut rt, REGISTER, &[dispatcher, root]);
        // `as <who>` and `each <all>`, redirecting to `ruby`.
        for (lit, arg, ty, modifier, key) in [("as", "who", PLAYER, 10, REDIRECT_ARG), ("each", "all", PLAYERS, 11, FORK_ARG)] {
            let n = name(&mut rt, lit);
            let l = c(&mut rt, LITERAL, &[n]);
            let t = c(&mut rt, ty, &[]);
            let n = name(&mut rt, arg);
            let a = c(&mut rt, ARGUMENT, &[n, t]);
            let interface = if key == REDIRECT_ARG { "com/mojang/brigadier/SingleRedirectModifier" } else { "com/mojang/brigadier/RedirectModifier" };
            let m = rt.lambda_id(interface, modifier, vec![]);
            c(&mut rt, key, &[a, ruby, m]);
            c(&mut rt, THEN, &[l, a]);
            let n = name(&mut rt, "ruby");
            let again = c(&mut rt, LITERAL, &[n]);
            c(&mut rt, THEN, &[again, l]);
            c(&mut rt, REGISTER, &[dispatcher, again]);
        }
        let mut run = |chat: &mut Chat, level: u8, line: &str| rt.with_host(chat, |rt| perform(rt, Some((me, "Alex")), level, [0.0; 3], [0.0; 2], line));
        assert!(run(&mut chat, 0, "ruby as Steve say hi"));
        assert!(run(&mut chat, 2, "ruby each @a say hey"));
        assert!(run(&mut chat, 0, "ruby each @a say hey"), "a selector at level 0");
        let lines = chat_lines(&mut chat);
        assert_eq!(said(&lines), ["Steve says hi", "Alex says hey", "Steve says hey", "argument.entity.selector.not_allowed", ""]);
        assert_eq!(lines[4]["color"], "gray", "the selector, a parse error: the line's context");
        // `dispatcher.execute(line, source)` by a mod: the parse error thrown.
        let source = new_source(&mut rt, Some((me, "Alex")), 0, [0.0; 3], [0.0; 2]);
        let line = rt.string("ruby say");
        let result = c(&mut rt, "com/mojang/brigadier/CommandDispatcher.execute.(Ljava/lang/String;Ljava/lang/Object;)I", &[dispatcher, line, source]);
        assert_eq!(result, V::I(0));
        let e = rt.thrown.take().expect("a syntax error");
        let message = c(&mut rt, "com/mojang/brigadier/exceptions/CommandSyntaxException.getMessage.()Ljava/lang/String;", &[e]);
        assert_eq!(rt.str_of(message), Some("command.unknown.command at position 8: ruby say<--[HERE]"));
        // `dispatcher.parse`: what it read, and where it stopped.
        let line = rt.string("ruby say a b");
        let results = c(&mut rt, "com/mojang/brigadier/CommandDispatcher.parse.(Ljava/lang/String;Ljava/lang/Object;)Lcom/mojang/brigadier/ParseResults;", &[dispatcher, line, source]);
        let reader = c(&mut rt, "com/mojang/brigadier/ParseResults.getReader.()Lcom/mojang/brigadier/ImmutableStringReader;", &[results]);
        assert_eq!(c(&mut rt, "com/mojang/brigadier/ImmutableStringReader.getCursor.()I", &[reader]), V::I(11));
        let context = c(&mut rt, "com/mojang/brigadier/ParseResults.getContext.()Lcom/mojang/brigadier/context/CommandContextBuilder;", &[results]);
        let nodes = c(&mut rt, "com/mojang/brigadier/context/CommandContextBuilder.getNodes.()Ljava/util/List;", &[context]);
        let names: Vec<String> = crate::jutil::elements(&rt, nodes)
            .into_iter()
            .map(|p| {
                let node = c(&mut rt, "com/mojang/brigadier/context/ParsedCommandNode.getNode.()Lcom/mojang/brigadier/tree/CommandNode;", &[p]);
                name_of(&rt, node)
            })
            .collect();
        assert_eq!(names, ["ruby", "say", "word"]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod's parser reading a selector with vanilla's (`new EntitySelectorParser(reader,
    /// allowSelectors).parse()`, as FTB Teams' `TeamArgument` does): it reads on the mod's reader,
    /// failing as vanilla's where selectors aren't allowed, and what it read finds the players.
    #[test]
    fn a_mods_parser_reads_selectors_with_vanillas() {
        let mut rt = Rt::new();
        let (me, steve) = (0x11u128, 0x22u128);
        let mut chat = Chat { players: vec![(me, [0.0; 3], "Alex"), (steve, [3.0, 0.0, 0.0], "Steve")], ..Chat::default() };
        let parse = |rt: &mut Rt, line: &str, allow: bool| {
            let reader = rt.new_object(brigadier::READER);
            let s = rt.string(line);
            c(rt, "com/mojang/brigadier/StringReader.<init>.(Ljava/lang/String;)V", &[reader, s]);
            let parser = rt.new_object(SELECTOR_PARSER);
            c(rt, "net/minecraft/commands/arguments/selector/EntitySelectorParser.<init>.(Lcom/mojang/brigadier/StringReader;Z)V", &[parser, reader, V::I(i32::from(allow))]);
            let selector = c(rt, "net/minecraft/commands/arguments/selector/EntitySelectorParser.parse.()Lnet/minecraft/commands/arguments/selector/EntitySelector;", &[parser]);
            (selector, c(rt, GET_CURSOR, &[reader]))
        };
        let (_, cursor) = parse(&mut rt, "@p rest", false);
        let e = rt.thrown.take().expect("selectors aren't allowed");
        let message = c(&mut rt, "com/mojang/brigadier/exceptions/CommandSyntaxException.getMessage.()Ljava/lang/String;", &[e]);
        assert_eq!(rt.str_of(message), Some("argument.entity.selector.not_allowed at position 0: <--[HERE]"));
        assert_eq!(cursor, V::I(0));
        let (selector, cursor) = parse(&mut rt, "@p[name=!Alex] rest", true);
        assert!(rt.thrown.is_none());
        assert_eq!(cursor, V::I(14), "up to the selector's end");
        let source = new_source(&mut rt, Some((me, "Alex")), 2, [0.0; 3], [0.0; 2]);
        let found = rt.with_host(&mut chat, |rt| {
            let p = c(rt, "net/minecraft/commands/arguments/selector/EntitySelector.findSinglePlayer.(Lnet/minecraft/commands/CommandSourceStack;)Lnet/minecraft/server/level/ServerPlayer;", &[selector, source]);
            command_args::player_name(rt, p)
        });
        assert!(rt.thrown.is_none());
        assert_eq!(found, "Steve");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Brigadier's merge: registering a root again adds its new children to the one there, and a
    /// child named as one there merges into it; `findNode` walks the names.
    #[test]
    fn registering_a_root_again_merges_its_children() {
        let mut rt = Rt::new();
        let dispatcher = new_dispatcher(&mut rt);
        let build = |rt: &mut Rt, words: &[&str]| {
            let mut node = V::NULL;
            for w in words.iter().rev() {
                let n = rt.string(w);
                let b = c(rt, LITERAL, &[n]);
                let command = rt.lambda_id("com/mojang/brigadier/Command", 1, vec![]);
                c(rt, EXECUTES, &[b, command]);
                if node.r() != 0 {
                    c(rt, THEN, &[b, node]);
                }
                node = b;
            }
            node
        };
        let first = build(&mut rt, &["tool", "a", "x"]);
        let second = build(&mut rt, &["tool", "a", "y"]);
        let third = build(&mut rt, &["tool", "b"]);
        let one = c(&mut rt, REGISTER, &[dispatcher, first]);
        let two = c(&mut rt, REGISTER, &[dispatcher, second]);
        c(&mut rt, REGISTER, &[dispatcher, third]);
        assert_eq!(one, two, "one `tool` node");
        let root = root_of(&mut rt, dispatcher);
        assert_eq!(children(&rt, root).len(), 1);
        let names = |rt: &Rt, n: V| children(rt, n).into_iter().map(|c| name_of(rt, c)).collect::<Vec<_>>();
        assert_eq!(names(&rt, one), ["a", "b"]);
        let a = child_named(&rt, one, "a").unwrap();
        assert_eq!(names(&rt, a), ["x", "y"]);
        let path: Vec<V> = ["tool", "a", "y"].iter().map(|w| rt.string(w)).collect();
        let path = crate::jutil::new_list(&mut rt, path);
        let found = c(&mut rt, "com/mojang/brigadier/CommandDispatcher.findNode.(Ljava/util/Collection;)Lcom/mojang/brigadier/tree/CommandNode;", &[dispatcher, path]);
        assert_eq!(name_of(&rt, found), "y");
        let usages: Vec<String> = usages(&mut rt, dispatcher).into_iter().map(|(u, _)| u).collect();
        assert_eq!(usages, ["tool", "tool a", "tool a x", "tool a y", "tool b"]);
    }
}
