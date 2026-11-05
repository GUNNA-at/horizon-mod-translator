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
//! - [`perform`]: a player's command line walks the tree as Brigadier parses it (the literal the
//!   next word names, or else each argument in turn, read by its type; a redirect goes on at its
//!   target), and the mod's `executes` runs with a `CommandContext` whose source is that player.
//!   What the command says (`sendSuccess`, `sendFailure`, a `CommandSyntaxException`) is its
//!   feedback.
//!
//! The vanilla argument types read as their parsers do: Brigadier's numbers (within their bounds),
//! booleans and strings; entity selectors (`@s`, `@p`, `@a`, `@r`, `@e`, a player's name or UUID)
//! over the server's players, the entities kept here; block positions and vectors (`~` from the
//! source); resource locations, dimensions, items, block states, messages, times, angles, colours
//! and UUIDs. A path through an argument of another type (a mod's own `ArgumentType`) is left out.
use crate::{api, Data, Rt, V};

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
const SYNTAX: &str = "com/mojang/brigadier/exceptions/CommandSyntaxException";
const BRIGADIER_ARGS: &str = "com/mojang/brigadier/arguments/";
const MC_ARGS: &str = "net/minecraft/commands/arguments/";
const SELECTOR: &str = "net/minecraft/commands/arguments/selector/EntitySelector";
const COORDINATES: &str = "net/minecraft/commands/arguments/coordinates/Coordinates";
const WORLD_COORDINATES: &str = "net/minecraft/commands/arguments/coordinates/WorldCoordinates";
const ITEM_INPUT: &str = "net/minecraft/commands/arguments/item/ItemInput";
const BLOCK_INPUT: &str = "net/minecraft/commands/arguments/blocks/BlockInput";
const SUGGESTIONS: &str = "net/minecraft/commands/SharedSuggestionProvider";
const SERVER_PLAYER: &str = "net/minecraft/server/level/ServerPlayer";
const RUN: &str = "com/mojang/brigadier/Command.run.(Lcom/mojang/brigadier/context/CommandContext;)I";
const TEST: &str = "java/util/function/Predicate.test.(Ljava/lang/Object;)Z";
const SUPPLY: &str = "java/util/function/Supplier.get.()Ljava/lang/Object;";
const MODIFY: &str = "com/mojang/brigadier/RedirectModifier.apply.(Lcom/mojang/brigadier/context/CommandContext;)Ljava/util/Collection;";
/// The server's dispatcher, once the mods registered to it.
const DISPATCHER_KEY: &str = "compat:commands";
/// Brigadier's default requirement, which every source passes (`getRequirement` of a node without one).
const ALWAYS: &str = "compat:command-always";

/// The keys [`call`] may answer, by owner, name and descriptor alone.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    owner.starts_with("com/mojang/brigadier/builder/")
        || owner.starts_with(TREE)
        || matches!(owner, DISPATCHER | CONTEXT | SOURCE | EVENT | SELECTOR | COORDINATES | WORLD_COORDINATES | ITEM_INPUT | BLOCK_INPUT | SUGGESTIONS)
        || (owner == COMMANDS && matches!(name, "literal" | "argument" | "getDispatcher"))
        || is_argument_type(owner)
        || (owner == "java/util/function/Predicate" && name == "test")
}

/// A Brigadier or Minecraft command call, or None when this module doesn't model it.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        ("java/util/function/Predicate", "test") if rt.class_of(a(0)) == ALWAYS => V::I(1),
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
        // `execute(line, source)`: the command a line names, run for that source.
        (DISPATCHER, "execute") if desc.starts_with("(Ljava/lang/String;") => {
            let line = rt.str_of(a(1)).unwrap_or("").to_owned();
            let root = root_of(rt, a(0));
            match run_line(rt, root, a(2), &line) {
                Some(result) => V::I(result),
                None => syntax_error(rt, "command.unknown.command"),
            }
        }
        // The event: the dispatcher the mods register to, and what it was made for.
        (EVENT, "getDispatcher") => rt.get_field(a(0), "$dispatcher"),
        (EVENT, "getBuildContext") => rt.intern("compat:command-build-context".into(), |rt| rt.new_object(BUILD_CONTEXT)),
        (EVENT, "getCommandSelection") => api::api_enum(rt, SELECTION, "ALL", 0),
        // A command's context: its source, its arguments, its line.
        (CONTEXT, n) => match n {
            "getSource" => rt.get_field(a(0), "$source"),
            "getInput" => rt.get_field(a(0), "$input"),
            "getArgument" => {
                let want = rt.str_of(a(1)).unwrap_or("").to_owned();
                match argument(rt, a(0), &want) {
                    Some(v) => boxed(rt, v),
                    None => no_argument(rt, &want),
                }
            }
            "getCommand" => rt.get_field(a(0), "$command"),
            "getChild" => V::NULL,
            "getLastChild" => a(0),
            "isForked" => V::I(0),
            _ => return None,
        },
        (SOURCE, _) => source_call(rt, name, args)?,
        // The argument types: one object per use, its kind and bounds; and their getters.
        (o, n) if is_argument_type(o) => {
            if n.starts_with("get") && desc.starts_with("(Lcom/mojang/brigadier/context/CommandContext;") {
                return get_argument(rt, o, n, desc, args);
            }
            argument_type(rt, o, n, desc, args)?
        }
        // What a selector, coordinates, an item or a block read gives.
        (SELECTOR, n) => {
            let found = select(rt, a(0), a(1));
            match n {
                "findEntities" | "findPlayers" => crate::jutil::new_list(rt, found),
                "findSingleEntity" | "findSinglePlayer" => match found.first() {
                    Some(&p) => p,
                    None => syntax_error(rt, if n == "findSinglePlayer" { "argument.entity.notfound.player" } else { "argument.entity.notfound.entity" }),
                },
                "includesEntities" => V::I(rt.get_field(a(0), "$players").i() ^ 1),
                _ => return None,
            }
        }
        (COORDINATES | WORLD_COORDINATES, n) => match n {
            "getBlockPos" => rt.get_field(a(0), "$block"),
            "getPosition" => rt.get_field(a(0), "$vec"),
            "getRotation" => rt.get_field(a(0), "$rot"),
            "isXRelative" | "isYRelative" | "isZRelative" => {
                let i = (n.as_bytes()[2] - b'X') as i32;
                V::I(rt.get_field(a(0), "$relative").i() >> i & 1)
            }
            _ => return None,
        },
        (ITEM_INPUT, n) => match n {
            "getItem" => rt.get_field(a(0), "$item"),
            "createItemStack" => {
                let id = rt.str_of(rt.get_field(a(0), "$id")).unwrap_or("minecraft:air").to_owned();
                crate::api_common::stack(rt, &id, a(1).i())
            }
            "serialize" => rt.get_field(a(0), "$id"),
            "getItemName" => {
                let id = rt.get_field(a(0), "$id");
                crate::api_common::component(rt, id, false, V::NULL)
            }
            _ => return None,
        },
        (BLOCK_INPUT, n) => match n {
            "getState" => rt.get_field(a(0), "$state"),
            // `place(level, pos, flags)`: the state set there.
            "place" => {
                let text = rt.str_of(rt.get_field(a(0), "$text")).unwrap_or("minecraft:air").to_owned();
                let p = api::pos_of(rt, a(2));
                V::I(rt.host_mut().is_some_and(|h| h.set_block_state(p, &text)) as i32)
            }
            _ => return None,
        },
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
            set(rt, b, "$modifier", if desc.contains("RedirectModifier") { a(2) } else { V::NULL });
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
                return Some(syntax_error(rt, if name == "getPlayerOrException" { "permissions.requires.player" } else { "permissions.requires.entity" }));
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

/// Throws Brigadier's `CommandSyntaxException` with the translated message `key`.
fn syntax_error(rt: &mut Rt, key: &str) -> V {
    let k = rt.string(key);
    let message = crate::api_common::component(rt, k, true, V::NULL);
    let e = rt.new_object(SYNTAX);
    rt.put_field(e, "$rawMessage", message);
    rt.thrown = Some(e);
    V::NULL
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

/// A factory argument of an argument type (`IntegerArgumentType.integer(min, max)`'s), if given.
fn bound(rt: &Rt, t: V, i: usize) -> Option<V> {
    match rt.heap.get(rt.get_field(t, "$args").r() as usize).map(|o| &o.data) {
        Some(Data::List(v)) => v.get(i).copied(),
        _ => None,
    }
}

/// The kind of a usage's argument (the game's name for vanilla's parser): None for a type not kept
/// here.
fn usage_kind(rt: &Rt, t: V) -> Option<&'static str> {
    Some(match kind_of(rt, t).as_str() {
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

// ---- Running a command ----

/// `native_perform_command`: runs `line` (without its slash) for `player` (their UUID and name; the
/// console for None) at their level and place. False when the tree has no path for it (the game
/// answers as vanilla does an unknown command, or another mod's tree takes it); true once a mod's
/// command ran, whatever it answered.
pub fn perform(rt: &mut Rt, player: Option<(u128, &str)>, permission: u8, at: [f64; 3], rot: [f32; 2], line: &str) -> bool {
    let Some(&dispatcher) = rt.interned.get(DISPATCHER_KEY) else { return false };
    let source = new_source(rt, player, permission, at, rot);
    let root = root_of(rt, dispatcher);
    run_line(rt, root, source, line).is_some()
}

/// A parse of a line: the arguments read, in order, and the node whose command runs.
struct Parse {
    args: Vec<(String, V)>,
    /// The sources the command runs for (a redirect's modifier may give several); None for the
    /// line's own.
    sources: Option<Vec<V>>,
    command: V,
}

/// Parses `line` from `root` for `source` and runs the command it reaches: its result (the sum of
/// each source's), or None when no path reads the whole line. What the command throws is its
/// answer: a `CommandSyntaxException` in red, anything else as vanilla's `command.failed`.
fn run_line(rt: &mut Rt, root: V, source: V, line: &str) -> Option<i32> {
    let parse = walk(rt, source, root, line, 0, Vec::new(), 0)?;
    let sources = parse.sources.clone().unwrap_or_else(|| vec![source]);
    let mut total = 0;
    for s in sources {
        let ctx = context(rt, s, line, &parse);
        let r = rt.call_functional(RUN, parse.command, &[ctx]).i();
        match rt.thrown.take() {
            None => total += r,
            Some(e) => {
                rt.unwinding.clear();
                if rt.instance_of(e, SYNTAX) {
                    let message = rt.get_field(e, "$rawMessage");
                    feedback(rt, s, message, true);
                } else {
                    rt.thrown = Some(e);
                    rt.report_thrown("a command");
                    let k = rt.string("command.failed");
                    let c = crate::api_common::component(rt, k, true, V::NULL);
                    feedback(rt, s, c, true);
                }
            }
        }
    }
    Some(total)
}

/// The `CommandContext` a command runs with.
fn context(rt: &mut Rt, source: V, line: &str, parse: &Parse) -> V {
    let ctx = rt.new_object(CONTEXT);
    rt.put_field(ctx, "$source", source);
    let input = rt.string(line);
    rt.put_field(ctx, "$input", input);
    rt.put_field(ctx, "$command", parse.command);
    let names: Vec<V> = parse.args.iter().map(|(n, _)| rt.string(n)).collect();
    let values: Vec<V> = parse.args.iter().map(|(_, v)| *v).collect();
    let (n, v) = (crate::jutil::new_list(rt, names), crate::jutil::new_list(rt, values));
    rt.put_field(ctx, "$argNames", n);
    rt.put_field(ctx, "$argValues", v);
    ctx
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

/// Brigadier's parse from `node` at byte `at` of `line`: the command of the path that reads the
/// whole line. A literal the next word names is the only way on when there is one; else each
/// argument the source may use, in order, until one leads through. A redirect goes on at its
/// target's children, with a context of its own (its modifier, if any, gives the sources).
fn walk(rt: &mut Rt, source: V, node: V, line: &str, at: usize, args: Vec<(String, V)>, depth: usize) -> Option<Parse> {
    if depth > 64 {
        return None;
    }
    let is_root = rt.class_of(node) == ROOT_NODE;
    if at >= line.len() && !is_root {
        let command = rt.get_field(node, "$command");
        return (command.r() != 0).then_some(Parse { args, sources: None, command });
    }
    let start = if is_root {
        at
    } else {
        if line.as_bytes().get(at) != Some(&b' ') || at + 1 >= line.len() {
            return None;
        }
        at + 1
    };
    let target = if is_root { V::NULL } else { rt.get_field(node, "$target") };
    if target.r() != 0 {
        let modifier = rt.get_field(node, "$modifier");
        let mut sources = None;
        if modifier.r() != 0 {
            let parse = Parse { args: args.clone(), sources: None, command: V::NULL };
            let ctx = context(rt, source, line, &parse);
            let given = rt.call_functional(MODIFY, modifier, &[ctx]);
            if caught(rt) {
                return None;
            }
            sources = Some(crate::jutil::elements(rt, given));
        }
        let first = sources.as_ref().and_then(|s| s.first().copied()).unwrap_or(source);
        let mut parse = walk_children(rt, first, target, line, start, Vec::new(), depth)?;
        if parse.sources.is_none() {
            parse.sources = sources;
        }
        return Some(parse);
    }
    walk_children(rt, source, node, line, start, args, depth)
}

fn walk_children(rt: &mut Rt, source: V, node: V, line: &str, start: usize, args: Vec<(String, V)>, depth: usize) -> Option<Parse> {
    let next = children(rt, node);
    let word_end = line[start..].find(' ').map_or(line.len(), |i| start + i);
    let word = &line[start..word_end];
    if let Some(lit) = next.iter().copied().find(|c| is_literal(rt, *c) && name_of(rt, *c) == word) {
        if !can_use(rt, lit, source) {
            return None;
        }
        return walk(rt, source, lit, line, word_end, args, depth + 1);
    }
    let arguments: Vec<V> = next.into_iter().filter(|c| !is_literal(rt, *c)).collect();
    for c in arguments {
        if !can_use(rt, c, source) {
            continue;
        }
        let t = rt.get_field(c, "$type");
        let Some((value, end)) = parse_argument(rt, source, t, line, start) else { continue };
        if end < line.len() && line.as_bytes()[end] != b' ' {
            continue;
        }
        let mut args = args.clone();
        args.push((name_of(rt, c), value));
        if let Some(parse) = walk(rt, source, c, line, end, args, depth + 1) {
            return Some(parse);
        }
    }
    None
}

/// Whether Brigadier reads `c` in an unquoted string.
fn unquoted(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+')
}

fn token_end(line: &str, start: usize) -> usize {
    line[start..].find(' ').map_or(line.len(), |i| start + i)
}

/// A number as Brigadier's reader takes it: digits, points and minus signs.
fn number_end(line: &str, start: usize) -> usize {
    line[start..].find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).map_or(line.len(), |i| start + i)
}

/// A quoted string's text and the byte after its closing quote (`\` escapes the quote and itself).
fn quoted(line: &str, start: usize) -> Option<(String, usize)> {
    let quote = line[start..].chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let mut out = String::new();
    let mut escaped = false;
    for (i, c) in line[start + 1..].char_indices() {
        if escaped {
            if c != quote && c != '\\' {
                return None;
            }
            out.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == quote {
            return Some((out, start + 1 + i + 1));
        } else {
            out.push(c);
        }
    }
    None
}

/// One world coordinate: `~`, `~1.5`, or a number (a whole one is a block's centre when `center`).
/// Its value from `origin`, whether it was relative, and the byte after it.
fn coordinate(line: &str, start: usize, origin: f64, center: bool, whole: bool) -> Option<(f64, bool, usize)> {
    let end = token_end(line, start);
    let tok = &line[start..end];
    let (relative, rest) = match tok.strip_prefix('~') {
        Some(r) => (true, r),
        None => (false, tok),
    };
    if rest.is_empty() {
        return relative.then_some((origin, true, end));
    }
    if whole && !relative && rest.contains('.') {
        return None;
    }
    let n: f64 = rest.parse().ok()?;
    let v = if relative {
        origin + n
    } else if center && !rest.contains('.') {
        n + 0.5
    } else {
        n
    };
    Some((v, relative, end))
}

/// `^left ^up ^forward` from the source's place and look, as vanilla's `LocalCoordinates`.
fn local(line: &str, start: usize, origin: [f64; 3], rot: [f32; 2]) -> Option<([f64; 3], usize)> {
    let mut at = start;
    let mut v = [0.0; 3];
    for (i, slot) in v.iter_mut().enumerate() {
        if i > 0 {
            if line.as_bytes().get(at) != Some(&b' ') {
                return None;
            }
            at += 1;
        }
        let end = token_end(line, at);
        let rest = line[at..end].strip_prefix('^')?;
        *slot = if rest.is_empty() { 0.0 } else { rest.parse().ok()? };
        at = end;
    }
    let [left, up, forwards] = v;
    let (yaw, pitch) = (rot[0], rot[1]);
    let rad = std::f32::consts::PI / 180.0;
    let (f, f1) = (((yaw + 90.0) * rad).cos(), ((yaw + 90.0) * rad).sin());
    let (f2, f3) = ((-pitch * rad).cos(), (-pitch * rad).sin());
    let (f4, f5) = (((-pitch + 90.0) * rad).cos(), ((-pitch + 90.0) * rad).sin());
    let a = [f64::from(f * f2), f64::from(f3), f64::from(f1 * f2)];
    let b = [f64::from(f * f4), f64::from(f5), f64::from(f1 * f4)];
    let c = [-(a[1] * b[2] - a[2] * b[1]), -(a[2] * b[0] - a[0] * b[2]), -(a[0] * b[1] - a[1] * b[0])];
    let pos = [0, 1, 2].map(|i| origin[i] + a[i] * forwards + b[i] * up + c[i] * left);
    Some((pos, at))
}

/// Several world coordinates separated by spaces, each from its origin; which were relative (bits).
fn coordinates(line: &str, start: usize, origin: &[f64], center: &[bool], whole: bool) -> Option<(Vec<f64>, i32, usize)> {
    let mut at = start;
    let mut out = Vec::new();
    let mut relative = 0;
    for (i, o) in origin.iter().enumerate() {
        if i > 0 {
            if line.as_bytes().get(at) != Some(&b' ') {
                return None;
            }
            at += 1;
        }
        let (v, rel, end) = coordinate(line, at, *o, center[i], whole)?;
        out.push(v);
        relative |= i32::from(rel) << i;
        at = end;
    }
    Some((out, relative, at))
}

/// A resource location token (`create:brass_ingot`, `the_nether`): its id with the namespace, and
/// the byte after it.
fn location(line: &str, start: usize) -> Option<(String, usize)> {
    let end = line[start..]
        .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-' | '.' | ':' | '/')))
        .map_or(line.len(), |i| start + i);
    let id = &line[start..end];
    if id.is_empty() || id.matches(':').count() > 1 {
        return None;
    }
    Some((if id.contains(':') { id.to_owned() } else { format!("minecraft:{id}") }, end))
}

/// The end of a bracketed part (`[...]`, `{...}`) at `start`, nested ones and quotes inside; `start`
/// itself when there is none there.
fn bracketed_end(line: &str, start: usize) -> Option<usize> {
    let open = match line[start..].chars().next() {
        Some(c @ ('[' | '{')) => c,
        _ => return Some(start),
    };
    let close = if open == '[' { ']' } else { '}' };
    let mut depth = 0;
    let mut quote: Option<char> = None;
    for (i, c) in line[start..].char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == open => depth += 1,
            None if c == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + i + 1);
                }
            }
            None => {}
        }
    }
    None
}

/// The coordinates object a position argument reads: its block, its point, its rotation.
fn coordinates_object(rt: &mut Rt, v: [f64; 3], relative: i32) -> V {
    let c = rt.new_object(WORLD_COORDINATES);
    let block = api::block_pos(rt, v[0].floor() as i64, v[1].floor() as i64, v[2].floor() as i64);
    let vec = crate::api_common::vec3(rt, v[0], v[1], v[2]);
    rt.put_field(c, "$block", block);
    rt.put_field(c, "$vec", vec);
    rt.put_field(c, "$relative", V::I(relative));
    c
}

/// Vanilla's chat colours, `ChatFormatting`'s first sixteen in order, and `reset`.
const COLOURS: [&str; 16] = [
    "black", "dark_blue", "dark_green", "dark_aqua", "dark_red", "dark_purple", "gold", "gray", "dark_gray", "blue", "green", "aqua", "red", "light_purple",
    "yellow", "white",
];

/// Reads one argument of type `t` at `start`: its value as the type's parser gives it, and the
/// byte after it. None when the text there isn't one.
fn parse_argument(rt: &mut Rt, source: V, t: V, line: &str, start: usize) -> Option<(V, usize)> {
    let kind = kind_of(rt, t);
    let origin = [rt.get_field(source, "$x").d(), rt.get_field(source, "$y").d(), rt.get_field(source, "$z").d()];
    let rot = [rt.get_field(source, "$yaw").f(), rt.get_field(source, "$pitch").f()];
    // A number token here, as Brigadier's reader takes one.
    let number_at = number_end(line, start);
    let number = (number_at > start).then(|| (&line[start..number_at], number_at));
    Some(match kind.as_str() {
        "IntegerArgumentType.integer" => {
            let (tok, end) = number?;
            let n: i32 = tok.parse().ok()?;
            let (min, max) = (bound(rt, t, 0).map_or(i32::MIN, V::i), bound(rt, t, 1).map_or(i32::MAX, V::i));
            ((min..=max).contains(&n).then_some(V::I(n))?, end)
        }
        "LongArgumentType.longArg" => {
            let (tok, end) = number?;
            let n: i64 = tok.parse().ok()?;
            let (min, max) = (bound(rt, t, 0).map_or(i64::MIN, V::j), bound(rt, t, 1).map_or(i64::MAX, V::j));
            ((min..=max).contains(&n).then_some(V::J(n))?, end)
        }
        "FloatArgumentType.floatArg" => {
            let (tok, end) = number?;
            let n: f32 = tok.parse().ok()?;
            let (min, max) = (bound(rt, t, 0).map_or(-f32::MAX, V::f), bound(rt, t, 1).map_or(f32::MAX, V::f));
            ((min..=max).contains(&n).then_some(V::F(n))?, end)
        }
        "DoubleArgumentType.doubleArg" => {
            let (tok, end) = number?;
            let n: f64 = tok.parse().ok()?;
            let (min, max) = (bound(rt, t, 0).map_or(-f64::MAX, V::d), bound(rt, t, 1).map_or(f64::MAX, V::d));
            ((min..=max).contains(&n).then_some(V::D(n))?, end)
        }
        "BoolArgumentType.bool" => {
            let end = token_end(line, start);
            let b = match &line[start..end] {
                "true" => 1,
                "false" => 0,
                _ => return None,
            };
            (V::I(b), end)
        }
        "StringArgumentType.word" => {
            let end = line[start..].find(|c: char| !unquoted(c)).map_or(line.len(), |i| start + i);
            (rt.string(&line[start..end]), end)
        }
        "StringArgumentType.string" => match quoted(line, start) {
            Some((s, end)) => (rt.string(&s), end),
            None if line[start..].starts_with(['"', '\'']) => return None,
            None => {
                let end = line[start..].find(|c: char| !unquoted(c)).map_or(line.len(), |i| start + i);
                (rt.string(&line[start..end]), end)
            }
        },
        "StringArgumentType.greedyString" => (rt.string(&line[start..]), line.len()),
        "MessageArgument.message" => {
            let s = rt.string(&line[start..]);
            (crate::api_common::component(rt, s, false, V::NULL), line.len())
        }
        "EntityArgument.entity" | "EntityArgument.entities" | "EntityArgument.player" | "EntityArgument.players" | "GameProfileArgument.gameProfile" => {
            let end = if line[start..].starts_with('@') { bracketed_end(line, (start + 2).min(line.len()))? } else { token_end(line, start) };
            let text = &line[start..end];
            if text.is_empty() || (text.starts_with('@') && !matches!(text.get(..2), Some("@s" | "@p" | "@a" | "@r" | "@e" | "@n"))) {
                return None;
            }
            let single = matches!(kind.as_str(), "EntityArgument.entity" | "EntityArgument.player");
            let players = kind != "EntityArgument.entity" && kind != "EntityArgument.entities";
            // A single target can't be a selector of several (`@a`, `@e` without `limit=1`).
            if single && matches!(text.get(..2), Some("@a" | "@e")) && !text.contains("limit=1") {
                return None;
            }
            let sel = rt.new_object(SELECTOR);
            let s = rt.string(text);
            rt.put_field(sel, "$text", s);
            rt.put_field(sel, "$single", V::I(single as i32));
            rt.put_field(sel, "$players", V::I(players as i32));
            (sel, end)
        }
        "BlockPosArgument.blockPos" | "ColumnPosArgument.columnPos" => {
            let three = kind.starts_with("BlockPos");
            if three && line[start..].starts_with('^') {
                let (v, end) = local(line, start, origin, rot)?;
                return Some((coordinates_object(rt, v, 7), end));
            }
            let o: Vec<f64> = if three { origin.iter().map(|v| v.floor()).collect() } else { vec![origin[0].floor(), origin[2].floor()] };
            let (v, relative, end) = coordinates(line, start, &o, &vec![false; o.len()], true)?;
            let v = if three { [v[0], v[1], v[2]] } else { [v[0], 0.0, v[1]] };
            (coordinates_object(rt, v, relative), end)
        }
        "Vec3Argument.vec3" => {
            if line[start..].starts_with('^') {
                let (v, end) = local(line, start, origin, rot)?;
                return Some((coordinates_object(rt, v, 7), end));
            }
            let center = bound(rt, t, 0).is_none_or(|v| v.i() != 0);
            let (v, relative, end) = coordinates(line, start, &origin, &[center, false, center], false)?;
            (coordinates_object(rt, [v[0], v[1], v[2]], relative), end)
        }
        "Vec2Argument.vec2" => {
            let center = bound(rt, t, 0).is_none_or(|v| v.i() != 0);
            let (v, _, end) = coordinates(line, start, &[origin[0], origin[2]], &[center, center], false)?;
            (crate::api_common::vec2(rt, v[0] as f32, v[1] as f32), end)
        }
        "RotationArgument.rotation" => {
            let (v, relative, end) = coordinates(line, start, &[f64::from(rot[0]), f64::from(rot[1])], &[false, false], false)?;
            let c = rt.new_object(WORLD_COORDINATES);
            let r = crate::api_common::vec2(rt, v[1] as f32, v[0] as f32);
            rt.put_field(c, "$rot", r);
            rt.put_field(c, "$relative", V::I(relative));
            (c, end)
        }
        "AngleArgument.angle" => {
            let (v, _, end) = coordinate(line, start, f64::from(rot[0]), false, false)?;
            (V::F(v as f32), end)
        }
        "ResourceLocationArgument.id" | "DimensionArgument.dimension" => {
            let (id, end) = location(line, start)?;
            (rt.string(&id), end)
        }
        "ItemArgument.item" | "BlockStateArgument.block" => {
            let (id, id_end) = location(line, start)?;
            let props_end = bracketed_end(line, id_end)?;
            let end = bracketed_end(line, props_end)?;
            if kind.starts_with("Item") {
                let input = rt.new_object(ITEM_INPUT);
                let item = crate::api_common::item(rt, &id);
                rt.put_field(input, "$item", item);
                let s = rt.string(&id);
                rt.put_field(input, "$id", s);
                (input, end)
            } else {
                let text = format!("{id}{}", &line[id_end..props_end]);
                let input = rt.new_object(BLOCK_INPUT);
                let state = api::state(rt, &text);
                rt.put_field(input, "$state", state);
                let s = rt.string(&text);
                rt.put_field(input, "$text", s);
                (input, end)
            }
        }
        "TimeArgument.time" => {
            let end = token_end(line, start);
            let tok = &line[start..end];
            let (n, unit) = match tok.as_bytes().last()? {
                b'd' => (&tok[..tok.len() - 1], 24000.0),
                b's' => (&tok[..tok.len() - 1], 20.0),
                b't' => (&tok[..tok.len() - 1], 1.0),
                _ => (tok, 1.0),
            };
            let ticks = (n.parse::<f32>().ok()? * unit).round() as i32;
            if ticks < bound(rt, t, 0).map_or(0, V::i) {
                return None;
            }
            (V::I(ticks), end)
        }
        "UuidArgument.uuid" => {
            let end = token_end(line, start);
            let (msb, lsb) = crate::tags::uuid_parse(&line[start..end])?;
            (crate::tags::uuid(rt, msb, lsb), end)
        }
        "ColorArgument.color" => {
            let end = token_end(line, start);
            let name = &line[start..end];
            let (constant, ordinal) = match COLOURS.iter().position(|c| *c == name) {
                Some(i) => (name.to_ascii_uppercase(), i as i32),
                None if name == "reset" => ("RESET".to_owned(), 21),
                None => return None,
            };
            let constant = rt.static_key(&constant);
            (api::api_enum(rt, "net/minecraft/ChatFormatting", constant, ordinal), end)
        }
        _ => return None,
    })
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

/// An argument type's getter (`IntegerArgumentType.getInteger(ctx, name)`,
/// `EntityArgument.getPlayer(ctx, name)`...): the parsed value as that getter gives it.
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
    );
    if !known {
        return None;
    }
    let Some(v) = argument(rt, ctx, &want) else {
        return Some(no_argument(rt, &want));
    };
    let source = rt.get_field(ctx, "$source");
    Some(match getter {
        ("EntityArgument", "getPlayer" | "getEntity") => match select(rt, v, source).first() {
            Some(&p) => p,
            None => syntax_error(rt, if name == "getPlayer" { "argument.entity.notfound.player" } else { "argument.entity.notfound.entity" }),
        },
        ("EntityArgument", _) => {
            let found = select(rt, v, source);
            if found.is_empty() && !name.starts_with("getOptional") {
                return Some(syntax_error(rt, if name == "getPlayers" { "argument.entity.notfound.player" } else { "argument.entity.notfound.entity" }));
            }
            crate::jutil::new_list(rt, found)
        }
        ("GameProfileArgument", _) => {
            let found = select(rt, v, source);
            if found.is_empty() {
                return Some(syntax_error(rt, "argument.player.unknown"));
            }
            let profiles: Vec<V> = found.into_iter().map(|p| api::call(rt, "net/minecraft/server/level/ServerPlayer.getGameProfile.()Lcom/mojang/authlib/GameProfile;", &[p])).collect();
            crate::jutil::new_list(rt, profiles)
        }
        ("BlockPosArgument", _) => rt.get_field(v, "$block"),
        ("ColumnPosArgument", _) => {
            let b = rt.get_field(v, "$block");
            let [x, _, z] = api::pos_of(rt, b);
            crate::records::make(rt, "net/minecraft/server/level/ColumnPos", &[V::I(x), V::I(z)])
        }
        ("Vec3Argument", "getVec3") => rt.get_field(v, "$vec"),
        ("DimensionArgument", _) => {
            let here = rt.host().map(|h| h.dimension()).unwrap_or_default();
            let wanted = rt.str_of(v).unwrap_or("").to_owned();
            if !here.is_empty() && wanted != here {
                return Some(syntax_error(rt, "argument.dimension.invalid"));
            }
            crate::glue::level(rt)
        }
        _ => v,
    })
}

/// The players a selector picks for `source` (the entities kept here are players): `@s` the
/// source's own, `@p` the nearest, `@r` one at random, `@a`, `@e` and `@n` every one (nearest
/// first), a name or UUID that player. Of its options, `name=` and `limit=` are kept.
fn select(rt: &mut Rt, selector: V, source: V) -> Vec<V> {
    let text = rt.str_of(rt.get_field(selector, "$text")).unwrap_or("").to_owned();
    let me = rt.get_field(source, "$player");
    let mut all = crate::events::server_players(rt);
    if me.r() != 0 && !all.contains(&me) && rt.class_of(me) == SERVER_PLAYER {
        all.insert(0, me);
    }
    let origin = [rt.get_field(source, "$x").d(), rt.get_field(source, "$y").d(), rt.get_field(source, "$z").d()];
    let distance = |rt: &Rt, p: V| {
        let d = [rt.get_field(p, "$x").d() - origin[0], rt.get_field(p, "$y").d() - origin[1], rt.get_field(p, "$z").d() - origin[2]];
        d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
    };
    let options = text.find('[').map_or("", |i| text[i + 1..].trim_end_matches(']'));
    let option = |key: &str| options.split(',').find_map(|o| o.trim().strip_prefix(key)?.trim_start().strip_prefix('=').map(str::trim));
    let mut found: Vec<V> = match text.get(..2) {
        Some("@s") => (me.r() != 0).then_some(me).into_iter().collect(),
        Some("@p" | "@a" | "@e" | "@n" | "@r") => {
            let mut v = all;
            v.sort_by(|a, b| distance(rt, *a).total_cmp(&distance(rt, *b)));
            if text.starts_with("@r") && v.len() > 1 {
                let i = rt.host_mut().and_then(|h| h.random()).map_or(0, |r| r.next_int_bound(v.len() as i32)) as usize;
                v = vec![v[i]];
            }
            if text.starts_with("@p") || text.starts_with("@n") {
                v.truncate(1);
            }
            v
        }
        _ => {
            let by_uuid = crate::tags::uuid_parse(&text);
            all.into_iter()
                .filter(|p| match by_uuid {
                    Some(id) => crate::tags::uuid_bits(rt, rt.get_field(*p, "$uuid")) == id,
                    None => player_name(rt, *p).eq_ignore_ascii_case(&text),
                })
                .take(1)
                .collect()
        }
    };
    if let Some(name) = option("name") {
        let (negate, name) = name.strip_prefix('!').map_or((false, name), |n| (true, n));
        let name = name.trim_matches('"');
        found.retain(|p| (player_name(rt, *p) == name) != negate);
    }
    if let Some(limit) = option("limit").and_then(|l| l.parse::<usize>().ok()) {
        found.truncate(limit);
    }
    if rt.get_field(selector, "$single").i() != 0 {
        found.truncate(1);
    }
    found
}

/// A server player's name: the one the game gave them, else the host's.
fn player_name(rt: &Rt, p: V) -> String {
    if let Some(n) = rt.str_of(rt.get_field(p, "$name")) {
        return n.to_owned();
    }
    let (msb, lsb) = crate::tags::uuid_bits(rt, rt.get_field(p, "$uuid"));
    let uuid = (u128::from(msb as u64) << 64) | u128::from(lsb as u64);
    rt.host().and_then(|h| h.player_name(uuid)).unwrap_or_default()
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
    const GET_PLAYER: &str = "net/minecraft/commands/arguments/EntityArgument.getPlayer.(Lcom/mojang/brigadier/context/CommandContext;Ljava/lang/String;)Lnet/minecraft/server/level/ServerPlayer;";
    const BLOCK_POS: &str = "net/minecraft/commands/arguments/coordinates/BlockPosArgument.blockPos.()Lnet/minecraft/commands/arguments/coordinates/BlockPosArgument;";
    const GET_BLOCK_POS: &str = "net/minecraft/commands/arguments/coordinates/BlockPosArgument.getLoadedBlockPos.(Lcom/mojang/brigadier/context/CommandContext;Ljava/lang/String;)Lnet/minecraft/core/BlockPos;";
    const CUSTOM: &str = "test/ColourArgument";
    const HAS_PERMISSION: &str = "net/minecraft/commands/CommandSourceStack.hasPermission.(I)Z";
    const SEND_SUCCESS: &str = "net/minecraft/commands/CommandSourceStack.sendSuccess.(Ljava/util/function/Supplier;Z)V";
    const GET_PLAYER_OR_EXCEPTION: &str = "net/minecraft/commands/CommandSourceStack.getPlayerOrException.()Lnet/minecraft/server/level/ServerPlayer;";
    const GET_SOURCE: &str = "com/mojang/brigadier/context/CommandContext.getSource.()Ljava/lang/Object;";

    fn c(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        crate::api::call(rt, key, args)
    }
    fn s(rt: &mut Rt, t: &str) -> V {
        rt.string(t)
    }

    /// The test mod: lambda 1 registers its tree, as Create's `AllCommands` does (a root with
    /// subcommands, a node built apart and added twice, an alias copying the root's children):
    ///
    /// - `/ruby give <count 1..64>` and `/ruby give <count> <player>`,
    /// - `/ruby spot <pos>`, `/ruby say <word>`, an operator's `/ruby wipe`,
    /// - `/ruby tint <colour>` of a type of the mod's own (left out),
    /// - `/rb ...`, the root's children again.
    ///
    /// 2 is `hasPermission(2)`; 3 to 7 run the commands, saying what they read; 8 is a message's
    /// supplier.
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
                let r = run(rt, 3);
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
            3..=7 => {
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
                        format!("gave {count} to {}", player_name(rt, player))
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
                        format!("{} says {}", player_name(rt, me), rt.str_of(w).unwrap_or(""))
                    }
                    _ => "wiped".to_owned(),
                };
                let text = s(rt, &said);
                let supplier = rt.lambda_id("java/util/function/Supplier", 8, vec![text]);
                c(rt, SEND_SUCCESS, &[source, supplier, V::I(0)]);
                V::I(1)
            }
            8 => crate::api_common::component(rt, captured[0], false, V::NULL),
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

    /// A mod's command tree, built with Brigadier's builders on the dispatcher
    /// `RegisterCommandsEvent` hands it: its runnable paths are the game's usages, each at the level
    /// its requirements ask, an alias's as its target's, a path through an argument type of the
    /// mod's own left out. A player's line runs the command its path reaches, with the arguments
    /// its types read (a count within its bounds, a player by name, a position from where the
    /// player stands), and what the command says comes back to them; an error the command throws
    /// comes back in red. A line no path reads isn't the mod's; one the player's level forbids
    /// neither; the console runs a command as no player.
    #[test]
    fn a_mods_commands_register_and_run_as_brigadier_parses_them() {
        let mut rt = Rt::new();
        rt.lambda_call = Some(lambdas);
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
                ("say <word:word>", 0),
            ] {
                expected.push((format!("{root} {usage}"), level));
            }
        }
        assert_eq!(usages, expected);
        let mut run = |level: u8, line: &str| rt.with_host(&mut chat, |rt| perform(rt, Some((me, "Alex")), level, [10.5, 64.0, -3.5], [0.0, 0.0], line));
        assert!(run(0, "ruby give 5"));
        assert!(run(0, "ruby give 7 Steve"));
        assert!(run(0, "rb spot ~1 ~ ~-2"));
        assert!(run(0, "ruby spot 3 -60 4"));
        assert!(run(0, "ruby say hello"));
        assert!(!run(0, "ruby give 65"), "beyond the count's bound");
        assert!(!run(0, "ruby give five"));
        assert!(!run(0, "ruby give 5 Steve extra"));
        assert!(!run(0, "ruby wipe"), "an operator's");
        assert!(!run(0, "ruby tint red"), "a type of the mod's own");
        assert!(run(2, "ruby wipe"));
        assert!(run(0, "ruby give 2 Nobody"));
        let console = rt.with_host(&mut chat, |rt| perform(rt, None, 4, [0.0; 3], [0.0; 2], "ruby say hi"));
        assert!(console);
        let said: Vec<serde_json::Value> = chat.lines.iter().map(|l| serde_json::from_str(l).unwrap()).collect();
        let texts: Vec<&str> = said.iter().map(|j| j["text"].as_str().or_else(|| j["translate"].as_str()).unwrap_or("")).collect();
        assert_eq!(
            texts,
            ["gave 5", "gave 7 to Steve", "spot 11 64 -6", "spot 3 -60 4", "Alex says hello", "wiped", "argument.entity.notfound.player", "permissions.requires.player"]
        );
        assert_eq!(said[6]["color"], "red");
        assert_eq!(said[7]["color"], "red");
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

    /// The vanilla argument types read as their parsers: bounds, quoted strings, relative and local
    /// coordinates, ids with their namespace, block states with properties, times in units.
    #[test]
    fn arguments_read_as_vanillas_parsers() {
        fn read<'a>(rt: &mut Rt, factory: &'static str, args: &[V], line: &'a str) -> Option<(V, &'a str)> {
            let source = new_source(rt, None, 4, [10.5, 64.0, -3.5], [90.0, 0.0]);
            let t = c(rt, factory, args);
            parse_argument(rt, source, t, line, 0).map(|(v, end)| (v, &line[end..]))
        }
        let mut rt = Rt::new();
        let string = "com/mojang/brigadier/arguments/StringArgumentType.string.()Lcom/mojang/brigadier/arguments/StringArgumentType;";
        let (v, rest) = read(&mut rt, string, &[], r#""a \"b\" c" d"#).unwrap();
        assert_eq!((rt.str_of(v), rest), (Some(r#"a "b" c"#), " d"));
        assert!(read(&mut rt, INTEGER, &[V::I(0), V::I(10)], "11").is_none());
        assert_eq!(read(&mut rt, INTEGER, &[V::I(0), V::I(10)], "-0").map(|r| r.0), Some(V::I(0)));
        let (v, _) = read(&mut rt, BLOCK_POS, &[], "~ ~1 ~-1").unwrap();
        let b = rt.get_field(v, "$block");
        assert_eq!(api::pos_of(&rt, b), [10, 65, -5]);
        assert!(read(&mut rt, BLOCK_POS, &[], "1.5 2 3").is_none(), "a block position is whole");
        // Looking along +x (yaw 90 faces -x in vanilla): `^ ^ ^2` is two blocks ahead.
        let (v, _) = read(&mut rt, BLOCK_POS, &[], "^ ^ ^2").unwrap();
        let b = rt.get_field(v, "$block");
        assert_eq!(api::pos_of(&rt, b), [8, 64, -4]);
        let vec3 = "net/minecraft/commands/arguments/coordinates/Vec3Argument.vec3.()Lnet/minecraft/commands/arguments/coordinates/Vec3Argument;";
        let (v, _) = read(&mut rt, vec3, &[], "1 2.5 ~").unwrap();
        let p = rt.get_field(v, "$vec");
        let xyz = [rt.get_field(p, "x").d(), rt.get_field(p, "y").d(), rt.get_field(p, "z").d()];
        assert_eq!(xyz, [1.5, 2.5, -3.5], "a whole x and z are the block's centre");
        let id = "net/minecraft/commands/arguments/ResourceLocationArgument.id.()Lnet/minecraft/commands/arguments/ResourceLocationArgument;";
        let (v, _) = read(&mut rt, id, &[], "stone").unwrap();
        assert_eq!(rt.str_of(v), Some("minecraft:stone"));
        let block = "net/minecraft/commands/arguments/blocks/BlockStateArgument.block.(Lnet/minecraft/commands/CommandBuildContext;)Lnet/minecraft/commands/arguments/blocks/BlockStateArgument;";
        let (v, rest) = read(&mut rt, block, &[V::NULL], "create:shaft[axis=x]{x:1} 2").unwrap();
        assert_eq!((rt.str_of(rt.get_field(v, "$text")), rest), (Some("create:shaft[axis=x]"), " 2"));
        let time = "net/minecraft/commands/arguments/TimeArgument.time.()Lnet/minecraft/commands/arguments/TimeArgument;";
        assert_eq!(read(&mut rt, time, &[], "1.5d").map(|r| r.0), Some(V::I(36000)));
        assert_eq!(read(&mut rt, time, &[], "3s").map(|r| r.0), Some(V::I(60)));
    }
}
