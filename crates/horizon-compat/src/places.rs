//! The places of the game's own code where mixins' instance handlers run.
//!
//! A mixin into one of the mods' classes is woven into that class's translated code. A method
//! of the game's has no code here to weave into: the game is Horizon's own. Where the runtime
//! stands for such a method (the server's tick, a level's, the end of the constructor of an
//! object it makes or the mods' code makes), it runs the handlers mixins inject there, on the
//! game's object it has in hand: `@Inject` handlers at the method's `HEAD`, `RETURN` or `TAIL`
//! that take the method's own arguments, or none, and a `CallbackInfo`. The translator lists
//! each among its mixin class's subscribers by the place it names
//! (`hz/inject:<owner>.<method>@<place>`), with the lambda that runs it. Where the runtime has
//! a method's locals in hand too (the builders `RecipeManager.apply` fills), a handler may
//! capture them after its `CallbackInfo` ([`at_locals`]).
//!
//! A static handler listens for its place as for an event ([`crate::neo`]'s `at_place`); these
//! run on an object, so they are kept here by place, and called with it.
use crate::{Rt, V};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

/// What the name of a place starts with.
pub(crate) const PLACE: &str = "hz/inject:";
const CALLBACK_INFO: &str = "Lorg/spongepowered/asm/mixin/injection/callback/CallbackInfo;";

/// An instance handler a mixin injects at a place of the game's code.
struct Handler {
    /// Its key (`<mixin>.<name>.<descriptor>`).
    key: &'static str,
    /// The lambda the generated code runs it by.
    lid: u32,
    /// The types of its parameters before the `CallbackInfo`: the target method's own, or none.
    params: Vec<String>,
    /// The types of those after it: the locals of the target method it captures.
    locals: Vec<String>,
}

impl Handler {
    /// What the handler takes for its parameters at the end of a constructor called with
    /// `given` (each argument by its type): the arguments themselves where they are the
    /// constructor's the handler was written for (`own`: the same types in the same order).
    /// Else the handler was written for another constructor of the class, or of a class above
    /// it, that this one ran through: an object the one argument of its type (a subclass hands
    /// its `Properties` on), and nothing known for the rest (no object, zero).
    fn arguments(&self, given: &[(&str, V)], own: bool) -> Vec<V> {
        if own
            && self.params.len() == given.len()
            && self.params.iter().zip(given).all(|(p, (g, _))| p == g)
        {
            return given.iter().map(|(_, v)| *v).collect();
        }
        self.params
            .iter()
            .map(|p| {
                let mut same = given.iter().filter(|(g, _)| g == p);
                match (p.as_bytes().first().copied(), same.next(), same.next()) {
                    (Some(b'L' | b'['), Some((_, v)), None) => *v,
                    (Some(b'L' | b'['), ..) => V::Void,
                    (Some(b'J'), ..) => V::J(0),
                    (Some(b'F'), ..) => V::F(0.0),
                    (Some(b'D'), ..) => V::D(0.0),
                    _ => V::I(0),
                }
            })
            .collect()
    }
}

/// A runtime's instance handlers, by where they run.
#[derive(Default)]
struct Places {
    /// By place, other than the end of a constructor.
    at: HashMap<&'static str, Vec<Rc<Handler>>>,
    /// At the end of the constructors of the game's classes, by class.
    ends: Vec<(&'static str, Vec<Rc<Handler>>)>,
    /// The same handlers, by mixin.
    of_mixin: HashMap<&'static str, Vec<Rc<Handler>>>,
    /// By the class a constructor is of, what [`Places::ends`] holds for it and for the classes
    /// above it, found once.
    chains: RefCell<HashMap<String, Chain>>,
}

/// The handlers at the end of the constructors one object is made by, each with the class it
/// is injected into, the topmost class's first.
type Chain = Rc<Vec<(&'static str, Rc<Handler>)>>;

/// The class whose constructors' end `place` is, if it is one
/// (`hz/inject:<class>.<init>@RETURN`, or `@TAIL`: both are the constructor's end).
fn constructed_class(place: &'static str) -> Option<&'static str> {
    let rest = place.strip_prefix(PLACE)?;
    rest.strip_suffix(".<init>@RETURN")
        .or_else(|| rest.strip_suffix(".<init>@TAIL"))
}

impl Places {
    /// The instance handlers among the subscribers the translator listed for `rt`'s classes.
    fn of(rt: &Rt) -> Places {
        // Each handler's places, by its key: the order is the keys', whatever the classes'.
        let mut by_key: BTreeMap<&'static str, (&'static str, u32, Vec<&'static str>)> =
            BTreeMap::new();
        for (&mixin, all) in &rt.subscribers {
            for &(event, key, is_static, lid) in all {
                if !is_static && event.starts_with(PLACE) {
                    by_key
                        .entry(key)
                        .or_insert((mixin, lid, Vec::new()))
                        .2
                        .push(event);
                }
            }
        }
        let mut out = Places::default();
        let mut ends: BTreeMap<&'static str, Vec<Rc<Handler>>> = BTreeMap::new();
        for (key, (mixin, lid, mut events)) in by_key {
            events.sort_unstable();
            let desc = key.find(".(").map_or("", |at| &key[at + 1..]);
            let mut params = crate::neo::desc_params(desc);
            // The `CallbackInfo` (the last parameter, or the one the captured locals follow).
            let info = params
                .iter()
                .rposition(|p| p == CALLBACK_INFO)
                .map_or(params.len(), |at| at + 1);
            let locals = params.split_off(info.min(params.len()));
            params.pop();
            let handler = Rc::new(Handler {
                key,
                lid,
                params,
                locals,
            });
            let classes: Vec<Option<&'static str>> =
                events.iter().copied().map(constructed_class).collect();
            if classes.iter().all(Option::is_some) {
                // At constructors' ends only: it runs once on each object (the generated
                // code sees to that), whichever way its turn comes.
                let mut seen: Vec<&'static str> = Vec::new();
                for class in classes.into_iter().flatten() {
                    if !seen.contains(&class) {
                        seen.push(class);
                        ends.entry(class).or_default().push(handler.clone());
                    }
                }
                out.of_mixin.entry(mixin).or_default().push(handler);
            } else {
                // (One at a constructor's end and elsewhere too runs elsewhere only: it
                // would run again at each of its places.)
                for (event, class) in events.iter().zip(&classes) {
                    if class.is_none() {
                        out.at.entry(*event).or_default().push(handler.clone());
                    }
                }
            }
        }
        out.ends = ends.into_iter().collect();
        out
    }

    /// The handlers at the end of the constructors an object of the game's class `class` is
    /// made by: the class's own and those of the classes above it the runtime knows of.
    fn chain(&self, rt: &Rt, class: &str) -> Chain {
        if let Some(found) = self.chains.borrow().get(class) {
            return found.clone();
        }
        let mut classes: Vec<&'static str> = Vec::new();
        for &(target, _) in &self.ends {
            if target == class || rt.is_a(class, target) {
                // A class before the first one below it: constructors end from the top down.
                let at = classes
                    .iter()
                    .position(|below| rt.is_a(below, target))
                    .unwrap_or(classes.len());
                classes.insert(at, target);
            }
        }
        let mut chain = Vec::new();
        for target in classes {
            let handlers = self.ends.iter().find(|(c, _)| *c == target).map(|(_, h)| h);
            for h in handlers.into_iter().flatten() {
                chain.push((target, h.clone()));
            }
        }
        let chain: Chain = Rc::new(chain);
        self.chains
            .borrow_mut()
            .insert(class.to_owned(), chain.clone());
        chain
    }
}

/// Under which a runtime keeps the number its handlers are filed under on each thread.
const INDEX: &str = "compat:instance-places";

thread_local! {
    /// The handlers of the runtimes this thread has run, by the number each keeps ([`INDEX`]).
    /// A copy of a runtime ([`Rt::fork`]) keeps the number: its classes are the same.
    static INDEXED: RefCell<HashMap<i32, Rc<Places>>> = RefCell::new(HashMap::new());
    /// The game's objects whose constructors' end is being run now ([`ended`]).
    static CONSTRUCTING: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
}

/// The instance handlers of `rt`'s classes, found once for the runtime on each thread.
fn places(rt: &mut Rt) -> Rc<Places> {
    static NEXT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(1);
    let id = match rt.interned.get(INDEX) {
        Some(&V::I(id)) => id,
        _ => {
            let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            rt.interned.insert(INDEX.into(), V::I(id));
            id
        }
    };
    if let Some(found) = INDEXED.with(|all| all.borrow().get(&id).cloned()) {
        return found;
    }
    let found = Rc::new(Places::of(rt));
    INDEXED.with(|all| all.borrow_mut().insert(id, found.clone()));
    found
}

/// Runs handler `h` on the game's object `object` with `arguments`, the `CallbackInfo` of the
/// method `method` and the locals it captures. What it throws is left thrown.
fn run(rt: &mut Rt, h: &Handler, object: V, arguments: &[V], locals: &[V], method: &str) {
    let Some(call) = rt.lambda_call else { return };
    let info = rt.new_object(crate::mixin::CALLBACK_INFO);
    let name = rt.string(method);
    // Cancellable: a handler that cancels was written for a method that can be, and the
    // `CallbackInfo` of one that cannot throws at `cancel()`. The runtime's own code for the
    // method goes on either way.
    crate::api::call(
        rt,
        "org/spongepowered/asm/mixin/injection/callback/CallbackInfo.<init>.(Ljava/lang/String;Z)V",
        &[info, name, V::I(1)],
    );
    // No lambda object before them: nothing is captured, the handler runs on exactly these.
    let mut full = Vec::with_capacity(arguments.len() + locals.len() + 3);
    full.push(V::NULL);
    full.push(object);
    full.extend_from_slice(arguments);
    full.push(info);
    full.extend_from_slice(locals);
    call(rt, h.lid, &full);
}

/// Runs the instance handlers mixins inject at `place` (`hz/inject:<owner>.<method>@HEAD`,
/// `@RETURN` or `@TAIL`) on `object`, the game's object the method runs on. `given` is the
/// method's arguments, each by its type: a handler that takes them gets them, one that takes
/// none gets none, and one that takes others was written for another method of the name, so it
/// is not this place's. A handler that throws is told of, and the next one runs.
pub(crate) fn at(rt: &mut Rt, place: &str, object: V, given: &[(&str, V)]) {
    at_locals(rt, place, object, given, &[]);
}

/// [`at`], at a place where the runtime has locals of the method in hand (`locals`, each by its
/// type, in the method's order): a handler that captures locals after its `CallbackInfo`
/// (`LocalCapture`'s, MixinExtras' `@Local` ones) gets for each the first one of its type not
/// yet taken, and one that captures a local the runtime has none for is not run.
pub(crate) fn at_locals(
    rt: &mut Rt,
    place: &str,
    object: V,
    given: &[(&str, V)],
    locals: &[(&str, V)],
) {
    let all = places(rt);
    let Some(handlers) = all.at.get(place) else {
        return;
    };
    let method = place.rsplit_once('@').map_or(place, |(m, _)| m);
    let method = method.rsplit('.').next().unwrap_or(method);
    for h in handlers {
        let arguments: Vec<V> = if h.params.is_empty() {
            Vec::new()
        } else if h.params.len() == given.len()
            && h.params.iter().zip(given).all(|(p, (g, _))| p == g)
        {
            given.iter().map(|(_, v)| *v).collect()
        } else {
            continue;
        };
        let mut left: Vec<(&str, V)> = locals.to_vec();
        let mut captured = Vec::with_capacity(h.locals.len());
        for wanted in &h.locals {
            let Some(found) = left.iter().position(|(ty, _)| ty == wanted) else {
                break;
            };
            captured.push(left.remove(found).1);
        }
        if captured.len() != h.locals.len() {
            continue;
        }
        run(rt, h, object, &arguments, &captured, method);
        if rt.thrown.is_some() {
            rt.report_thrown(&format!("{} at {}", h.key, &place[PLACE.len()..]));
        }
    }
}

/// Whether any instance handler runs at `place` ([`at`]): for a caller that would make the
/// method's arguments only for them.
pub(crate) fn heard(rt: &mut Rt, place: &str) -> bool {
    places(rt).at.contains_key(place)
}

/// Runs `chain`, the handlers at the end of the constructors `object` was made by, an object
/// of the game's class `class` whose constructor was called with `given`. A handler that
/// throws ends it: when `thrown_on`, what it threw is left thrown, as the constructor's caller
/// would have it; else it is told of and the next handler runs.
fn ended(rt: &mut Rt, object: V, class: &str, chain: &Chain, given: &[(&str, V)], thrown_on: bool) {
    CONSTRUCTING.with(|c| c.borrow_mut().push(object.r()));
    for (target, h) in chain.iter() {
        let arguments = h.arguments(given, *target == class);
        run(rt, h, object, &arguments, &[], "<init>");
        if rt.thrown.is_some() {
            if thrown_on {
                break;
            }
            rt.report_thrown(&format!("{} at the end of {target}'s constructor", h.key));
        }
    }
    CONSTRUCTING.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(at) = c.iter().rposition(|&r| r == object.r()) {
            c.remove(at);
        }
    });
}

/// The end of the constructors of `object`, an object of the game's class `class` the runtime
/// made itself (a level, a player, the server): the handlers mixins inject there run on it,
/// those of the classes above `class` first. `given` is what of the constructor's arguments
/// the runtime has, each by its type (`Lnet/minecraft/server/MinecraftServer;`): a handler
/// that takes the constructor's arguments gets those, and nothing for the rest.
pub(crate) fn constructed(rt: &mut Rt, object: V, class: &str, given: &[(&str, V)]) {
    if object.r() == 0 {
        return;
    }
    let all = places(rt);
    if all.ends.is_empty() {
        return;
    }
    let chain = all.chain(rt, class);
    if !chain.is_empty() {
        ended(rt, object, class, &chain, given, false);
    }
}

/// Whether any handler runs at the end of the constructors of the game's class `class`
/// ([`constructed`]): for a caller that would make the object only for them.
pub(crate) fn constructors_heard(rt: &mut Rt, class: &str) -> bool {
    let all = places(rt);
    !all.ends.is_empty() && !all.chain(rt, class).is_empty()
}

/// The end of a constructor of the game's that the mods' code called (API call `key`,
/// `<class>.<init>.<descriptor>`, with `args`: the object, then the arguments): `new
/// Item(properties)`, or `super(properties)` from a mod's own item. The handlers mixins
/// inject there run on the object, with the arguments the constructor was called with. What
/// one throws is the constructor's to throw.
pub(crate) fn constructor_ended(rt: &mut Rt, key: &'static str, args: &[V]) {
    let all = places(rt);
    if all.ends.is_empty() || rt.thrown.is_some() {
        return;
    }
    let Some((class, desc)) = key.split_once(".<init>.") else {
        return;
    };
    let Some(&object) = args.first().filter(|o| o.r() != 0) else {
        return;
    };
    let chain = all.chain(rt, class);
    if chain.is_empty() {
        return;
    }
    let types = crate::neo::desc_params(desc);
    let given: Vec<(&str, V)> = types
        .iter()
        .map(String::as_str)
        .zip(args[1..].iter().copied())
        .collect();
    ended(rt, object, class, &chain, &given, true);
}

/// The owner of the API call the code generated for a mixin makes for [`constructed_late`]:
/// this and the mixin's class (`hz/mixin-made:<mixin>.late.(Ljava/lang/Object;)V`). The code's
/// crates link the runtime's core alone, so what they ask of this crate is an API call
/// ([`crate::mixin`] answers it).
pub(crate) const MADE: &str = "hz/mixin-made:";

/// What the code generated for mixin `mixin` asks for before the first of the mixin's methods
/// runs on a game object: the mixin's handlers at the end of the object's constructors, should
/// nothing have told of that end ([`constructed`], [`constructor_ended`]): an object the
/// runtime made without saying so. Each handler is handed nothing of the constructor's
/// arguments: only the object is there. While the object's constructors' end is being run,
/// nothing: the handlers' turn comes there, with the arguments.
pub(crate) fn constructed_late(rt: &mut Rt, object: V, mixin: &str) {
    if object.r() == 0
        || rt.thrown.is_some()
        || CONSTRUCTING.with(|c| c.borrow().contains(&object.r()))
    {
        return;
    }
    let all = places(rt);
    let Some(handlers) = all.of_mixin.get(mixin) else {
        return;
    };
    for h in handlers {
        let arguments = h.arguments(&[], false);
        run(rt, h, object, &arguments, &[], "<init>");
        if rt.thrown.is_some() {
            rt.report_thrown(&format!("{} at the end of a constructor", h.key));
        }
    }
}

/// A method of the game's the runtime stands for, with the places instance handlers run at:
/// its head, and its end (its returns, then its tail).
pub(crate) struct Method {
    head: &'static str,
    returns: &'static str,
    tail: &'static str,
    /// The types of its parameters.
    types: &'static [&'static str],
}

/// The [`Method`] `name` of the game's class `owner`, taking parameters of the types after it.
macro_rules! method {
    ($owner:literal, $name:literal $(, $ty:literal)*) => {
        Method {
            head: concat!("hz/inject:", $owner, ".", $name, "@HEAD"),
            returns: concat!("hz/inject:", $owner, ".", $name, "@RETURN"),
            tail: concat!("hz/inject:", $owner, ".", $name, "@TAIL"),
            types: &[$($ty),*],
        }
    };
}

impl Method {
    /// Runs the handlers at the method's head on `object`, the method called with `arguments`.
    pub(crate) fn head(&self, rt: &mut Rt, object: V, arguments: &[V]) {
        self.run(rt, self.head, object, arguments);
    }

    /// Runs the handlers at the method's end on `object`: those at its returns, then those at
    /// its tail.
    pub(crate) fn end(&self, rt: &mut Rt, object: V, arguments: &[V]) {
        self.run(rt, self.returns, object, arguments);
        self.run(rt, self.tail, object, arguments);
    }

    /// Whether any handler runs at the method's head or its end: for a caller that would make
    /// the object or the arguments only for them.
    pub(crate) fn heard(&self, rt: &mut Rt) -> bool {
        let all = places(rt);
        [self.head, self.returns, self.tail]
            .iter()
            .any(|place| all.at.contains_key(place))
    }

    fn run(&self, rt: &mut Rt, place: &str, object: V, arguments: &[V]) {
        if object.r() == 0 || !heard(rt, place) {
            return;
        }
        let given: Vec<(&str, V)> = self
            .types
            .iter()
            .enumerate()
            .map(|(i, ty)| (*ty, arguments.get(i).copied().unwrap_or(V::Void)))
            .collect();
        at(rt, place, object, &given);
    }
}

/// `MinecraftServer.runServer()`: from before the server loads anything until it has stopped.
pub(crate) const RUN_SERVER: Method = method!("net/minecraft/server/MinecraftServer", "runServer");
/// `MinecraftServer.loadLevel()`, around [`CREATE_LEVELS`].
pub(crate) const LOAD_LEVEL: Method = method!("net/minecraft/server/MinecraftServer", "loadLevel");
/// `MinecraftServer.createLevels(ChunkProgressListener)`: the server's levels are made.
pub(crate) const CREATE_LEVELS: Method = method!(
    "net/minecraft/server/MinecraftServer",
    "createLevels",
    "Lnet/minecraft/server/level/progress/ChunkProgressListener;"
);
/// `MinecraftServer.tickServer(BooleanSupplier)`: one tick of the server.
pub(crate) const TICK_SERVER: Method = method!(
    "net/minecraft/server/MinecraftServer",
    "tickServer",
    "Ljava/util/function/BooleanSupplier;"
);
/// `MinecraftServer.tickChildren(BooleanSupplier)`: the levels' ticks within the server's.
pub(crate) const TICK_CHILDREN: Method = method!(
    "net/minecraft/server/MinecraftServer",
    "tickChildren",
    "Ljava/util/function/BooleanSupplier;"
);
/// `MinecraftServer.stopServer()`.
pub(crate) const STOP_SERVER: Method =
    method!("net/minecraft/server/MinecraftServer", "stopServer");
/// `ServerLevel.tick(BooleanSupplier)`.
pub(crate) const LEVEL_TICK: Method = method!(
    "net/minecraft/server/level/ServerLevel",
    "tick",
    "Ljava/util/function/BooleanSupplier;"
);
/// `ServerLevel.close()`.
pub(crate) const LEVEL_CLOSE: Method = method!("net/minecraft/server/level/ServerLevel", "close");
/// `PlayerList.placeNewPlayer(Connection, ServerPlayer, CommonListenerCookie)`: a player joins.
pub(crate) const PLACE_NEW_PLAYER: Method = method!(
    "net/minecraft/server/players/PlayerList",
    "placeNewPlayer",
    "Lnet/minecraft/network/Connection;",
    "Lnet/minecraft/server/level/ServerPlayer;",
    "Lnet/minecraft/server/network/CommonListenerCookie;"
);
/// `PlayerList.remove(ServerPlayer)`: a player leaves.
pub(crate) const REMOVE_PLAYER: Method = method!(
    "net/minecraft/server/players/PlayerList",
    "remove",
    "Lnet/minecraft/server/level/ServerPlayer;"
);
/// `ServerPlayer.restoreFrom(ServerPlayer, boolean)`: a player made anew takes what the old
/// one had (after dying, or coming back from the End).
pub(crate) const RESTORE_FROM: Method = method!(
    "net/minecraft/server/level/ServerPlayer",
    "restoreFrom",
    "Lnet/minecraft/server/level/ServerPlayer;",
    "Z"
);
/// `RecipeManager.apply(files, resources, profiler)`: the server's recipes load
/// ([`crate::recipes::applied`]).
pub(crate) const RECIPES_APPLY: Method = method!(
    "net/minecraft/world/item/crafting/RecipeManager",
    "apply",
    "Ljava/util/Map;",
    "Lnet/minecraft/server/packs/resources/ResourceManager;",
    "Lnet/minecraft/util/profiling/ProfilerFiller;"
);
/// `ReloadableServerResources.updateRegistryTags()`: the loaded tags bind to the registries.
pub(crate) const UPDATE_REGISTRY_TAGS: Method = method!(
    "net/minecraft/server/ReloadableServerResources",
    "updateRegistryTags"
);
/// `Minecraft.tick()`: one tick of the client.
pub(crate) const CLIENT_TICK: Method = method!("net/minecraft/client/Minecraft", "tick");
/// `Minecraft.setLevel(ClientLevel, ReceivingLevelScreen.Reason)`: the client takes a level.
pub(crate) const SET_LEVEL: Method = method!(
    "net/minecraft/client/Minecraft",
    "setLevel",
    "Lnet/minecraft/client/multiplayer/ClientLevel;",
    "Lnet/minecraft/client/gui/screens/ReceivingLevelScreen$Reason;"
);
/// `Minecraft.updateLevelInEngines(ClientLevel)`: its renderers take the level, or none.
pub(crate) const UPDATE_LEVEL_IN_ENGINES: Method = method!(
    "net/minecraft/client/Minecraft",
    "updateLevelInEngines",
    "Lnet/minecraft/client/multiplayer/ClientLevel;"
);
/// `Minecraft.disconnect(Screen, boolean)`: the client leaves its server.
pub(crate) const DISCONNECT: Method = method!(
    "net/minecraft/client/Minecraft",
    "disconnect",
    "Lnet/minecraft/client/gui/screens/Screen;",
    "Z"
);
/// `ClientPacketListener.handleLogin(ClientboundLoginPacket)`: the client joins a server.
pub(crate) const HANDLE_LOGIN: Method = method!(
    "net/minecraft/client/multiplayer/ClientPacketListener",
    "handleLogin",
    "Lnet/minecraft/network/protocol/game/ClientboundLoginPacket;"
);
/// `ClientLevel.tick(BooleanSupplier)`.
pub(crate) const CLIENT_LEVEL_TICK: Method = method!(
    "net/minecraft/client/multiplayer/ClientLevel",
    "tick",
    "Ljava/util/function/BooleanSupplier;"
);
/// `ClientLevel.tickEntities()`.
pub(crate) const CLIENT_LEVEL_ENTITIES: Method = method!(
    "net/minecraft/client/multiplayer/ClientLevel",
    "tickEntities"
);

/// A player's `tick()`, as the classes it is an object of run it one inside the other:
/// `Player`'s calls `LivingEntity`'s, which calls `Entity`'s.
const PLAYER_TICK: Method = method!("net/minecraft/world/entity/player/Player", "tick");
const INNER_TICKS: [Method; 2] = [
    method!("net/minecraft/world/entity/LivingEntity", "tick"),
    method!("net/minecraft/world/entity/Entity", "tick"),
];
/// A server's player's own `tick()` (among its level's entities), and `doTick()` (from its
/// connection), which runs [`PLAYER_TICK`].
const SERVER_PLAYER_TICKS: [Method; 2] = [
    method!("net/minecraft/server/level/ServerPlayer", "tick"),
    method!("net/minecraft/server/level/ServerPlayer", "doTick"),
];
/// The client's own player's `tick()`, which runs `AbstractClientPlayer`'s, then [`PLAYER_TICK`].
const LOCAL_PLAYER_TICKS: [Method; 2] = [
    method!("net/minecraft/client/player/LocalPlayer", "tick"),
    method!("net/minecraft/client/player/AbstractClientPlayer", "tick"),
];

/// Whether any handler runs in a player's tick ([`player_tick`]): the client's own (`local`),
/// or a server's.
pub(crate) fn player_ticks_heard(rt: &mut Rt, local: bool) -> bool {
    let own = if local {
        &LOCAL_PLAYER_TICKS
    } else {
        &SERVER_PLAYER_TICKS
    };
    own.iter()
        .chain(&INNER_TICKS)
        .chain([&PLAYER_TICK])
        .any(|m| m.heard(rt))
}

/// A player's tick, in the order the game runs it: the handlers at the head of `Player.tick()`
/// (the client's own player's classes' first: `local`), then `before` (the tick's start, as
/// the mods hear it), the `tick()` of the classes above `Player` from head to end, then
/// `after`, and the handlers at the ends. A server's player's own `tick()`, run among its
/// level's entities, comes before all that, and its `doTick()` is around it.
pub(crate) fn player_tick(
    rt: &mut Rt,
    player: V,
    local: bool,
    before: impl FnOnce(&mut Rt),
    after: impl FnOnce(&mut Rt),
) {
    if local {
        for m in &LOCAL_PLAYER_TICKS {
            m.head(rt, player, &[]);
        }
    } else {
        SERVER_PLAYER_TICKS[0].head(rt, player, &[]);
        SERVER_PLAYER_TICKS[0].end(rt, player, &[]);
        SERVER_PLAYER_TICKS[1].head(rt, player, &[]);
    }
    PLAYER_TICK.head(rt, player, &[]);
    before(rt);
    for m in &INNER_TICKS {
        m.head(rt, player, &[]);
    }
    for m in INNER_TICKS.iter().rev() {
        m.end(rt, player, &[]);
    }
    after(rt);
    PLAYER_TICK.end(rt, player, &[]);
    if local {
        for m in LOCAL_PLAYER_TICKS.iter().rev() {
            m.end(rt, player, &[]);
        }
    } else {
        SERVER_PLAYER_TICKS[1].end(rt, player, &[]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK_ITEM: &str = "net/minecraft/world/item/BlockItem";
    const PROPERTIES: &str = "net/minecraft/world/item/Item$Properties";
    const INFO: &str = "Lorg/spongepowered/asm/mixin/injection/callback/CallbackInfo;";

    /// What each handler was called with: `[lid, object, argument count, first argument]`.
    fn log(rt: &mut Rt) -> Vec<[V; 4]> {
        let list = rt.interned["test:log"];
        crate::jutil::elements(rt, list)
            .chunks(4)
            .map(|c| [c[0], c[1], c[2], c[3]])
            .collect()
    }

    /// A handler's lambda, as the generated code runs it: `args` are the lambda (none here),
    /// the object, the handler's arguments, the `CallbackInfo`.
    fn handler(rt: &mut Rt, lid: u32, args: &[V]) -> Option<V> {
        let list = rt.interned["test:log"];
        let info = *args.last()?;
        assert_eq!(rt.class_of(info), crate::mixin::CALLBACK_INFO);
        let first = if args.len() > 3 { args[2] } else { V::Void };
        for v in [
            V::I(lid as i32),
            args[1],
            V::I(args.len() as i32 - 3),
            first,
        ] {
            crate::jutil::list_add(rt, list, v);
        }
        if lid == 9 {
            rt.throw_new("java/lang/IllegalStateException");
        }
        Some(V::Void)
    }

    fn runtime(subscribers: &[(&'static str, Vec<(&'static str, &'static str, bool, u32)>)]) -> Rt {
        crate::install();
        let mut rt = Rt::new();
        rt.lambda_call = Some(handler);
        let list = crate::jutil::new_list(&mut rt, Vec::new());
        rt.interned.insert("test:log".into(), list);
        for (class, all) in subscribers {
            rt.subscribers.insert(*class, all.clone());
        }
        rt
    }

    /// A handler that captures locals of the method gets them where the runtime has them in
    /// hand, each by its type, after its `CallbackInfo`; one that captures a local the runtime
    /// has none for is not run, and where the runtime hands no locals neither is.
    #[test]
    fn a_handler_captures_the_locals_the_runtime_has() {
        const BUILT: &str =
            "hz/inject:net/minecraft/world/item/crafting/RecipeManager.apply@INVOKE:builder:AFTER";
        fn captures(rt: &mut Rt, lid: u32, args: &[V]) -> Option<V> {
            let list = rt.interned["test:log"];
            crate::jutil::list_add(rt, list, V::I(lid as i32));
            for &v in &args[1..] {
                crate::jutil::list_add(rt, list, v);
            }
            Some(V::Void)
        }
        let mut rt = runtime(&[(
            "t/RecipesMixin",
            vec![
                (
                    BUILT,
                    "t/RecipesMixin.adds.(Lorg/spongepowered/asm/mixin/injection/callback/CallbackInfo;Ljava/util/Map;Ljava/util/List;)V",
                    false,
                    1,
                ),
                (
                    BUILT,
                    "t/RecipesMixin.wants.(Lorg/spongepowered/asm/mixin/injection/callback/CallbackInfo;Ljava/util/Set;)V",
                    false,
                    2,
                ),
            ],
        )]);
        rt.lambda_call = Some(captures);
        let manager = rt.new_object("net/minecraft/world/item/crafting/RecipeManager");
        let (list, map) = (
            rt.new_object("java/util/ArrayList"),
            rt.new_object("java/util/HashMap"),
        );
        // The locals in the method's order; the handler takes them in its own.
        at_locals(
            &mut rt,
            BUILT,
            manager,
            &[],
            &[("Ljava/util/List;", list), ("Ljava/util/Map;", map)],
        );
        let log = rt.interned["test:log"];
        let seen = crate::jutil::elements(&rt, log);
        assert_eq!(seen.len(), 5);
        assert_eq!(
            (seen[0], seen[1], seen[3], seen[4]),
            (V::I(1), manager, map, list)
        );
        assert_eq!(rt.class_of(seen[2]), crate::mixin::CALLBACK_INFO);
        at(&mut rt, BUILT, manager, &[]);
        assert_eq!(crate::jutil::elements(&rt, log).len(), 5);
    }

    /// A handler at a method's head runs on the object with the method's arguments, one that
    /// takes none with none, one written for other arguments not at all; a static one is not
    /// an instance handler's business; a place nothing is injected at runs nothing.
    #[test]
    fn a_handler_runs_at_its_place_on_the_object() {
        const HEAD: &str = "hz/inject:net/minecraft/server/level/ServerLevel.tick@HEAD";
        const TAIL: &str = "hz/inject:net/minecraft/server/level/ServerLevel.tick@TAIL";
        let mut rt = runtime(&[(
            "t/LevelMixin",
            vec![
                (
                    HEAD,
                    "t/LevelMixin.before.(Ljava/util/function/BooleanSupplier;Lorg/spongepowered/asm/mixin/injection/callback/CallbackInfo;)V",
                    false,
                    1,
                ),
                (
                    HEAD,
                    "t/LevelMixin.plain.(Lorg/spongepowered/asm/mixin/injection/callback/CallbackInfo;)V",
                    false,
                    2,
                ),
                (
                    HEAD,
                    "t/LevelMixin.shared.(Lorg/spongepowered/asm/mixin/injection/callback/CallbackInfo;)V",
                    true,
                    3,
                ),
                (
                    HEAD,
                    "t/LevelMixin.other.(ILorg/spongepowered/asm/mixin/injection/callback/CallbackInfo;)V",
                    false,
                    4,
                ),
            ],
        )]);
        let level = rt.new_object("net/minecraft/server/level/ServerLevel");
        let more = rt.new_object("java/util/function/BooleanSupplier");
        let given = [("Ljava/util/function/BooleanSupplier;", more)];
        assert!(heard(&mut rt, HEAD) && !heard(&mut rt, TAIL));
        at(&mut rt, HEAD, level, &given);
        at(&mut rt, TAIL, level, &given);
        assert_eq!(
            log(&mut rt),
            [
                [V::I(1), level, V::I(1), more],
                [V::I(2), level, V::I(0), V::Void]
            ]
        );
        assert!(rt.thrown.is_none());
    }

    /// The end of a constructor the mods' code called: the handlers of the class and of the
    /// class above it run, the upper class's first, each once whatever both places name it;
    /// one written for the constructor called gets its arguments, one for the class above the
    /// argument of its type.
    #[test]
    fn a_constructor_called_ends_with_its_handlers() {
        let block_item = format!(
            "t/BlockItemMixin.made.(Lnet/minecraft/world/level/block/Block;L{PROPERTIES};{INFO})V"
        );
        let item = format!("t/ItemMixin.init.(L{PROPERTIES};{INFO})V");
        let block_item: &'static str = Box::leak(block_item.into_boxed_str());
        let item: &'static str = Box::leak(item.into_boxed_str());
        let mut rt = runtime(&[
            (
                "t/BlockItemMixin",
                vec![(
                    "hz/inject:net/minecraft/world/item/BlockItem.<init>@TAIL",
                    block_item,
                    false,
                    1,
                )],
            ),
            (
                "t/ItemMixin",
                vec![
                    (
                        "hz/inject:net/minecraft/world/item/Item.<init>@RETURN",
                        item,
                        false,
                        2,
                    ),
                    (
                        "hz/inject:net/minecraft/world/item/Item.<init>@TAIL",
                        item,
                        false,
                        2,
                    ),
                ],
            ),
        ]);
        let made = rt.new_object(BLOCK_ITEM);
        let block = rt.new_object("net/minecraft/world/level/block/Block");
        let properties = rt.new_object(PROPERTIES);
        constructor_ended(
            &mut rt,
            "net/minecraft/world/item/BlockItem.<init>.(Lnet/minecraft/world/level/block/Block;Lnet/minecraft/world/item/Item$Properties;)V",
            &[made, block, properties],
        );
        assert_eq!(
            log(&mut rt),
            [
                [V::I(2), made, V::I(1), properties],
                [V::I(1), made, V::I(2), block]
            ]
        );
        // A class no mixin is injected into, nor any above it: nothing.
        let other = rt.new_object("net/minecraft/core/BlockPos");
        constructor_ended(
            &mut rt,
            "net/minecraft/core/BlockPos.<init>.(III)V",
            &[other, V::I(1), V::I(2), V::I(3)],
        );
        assert_eq!(log(&mut rt).len(), 2);
    }

    /// An object the runtime made: its constructors' handlers run with what the runtime has of
    /// the arguments, by type, and one that throws is told of without stopping the next. A
    /// mixin's late call does the same with nothing, but not while the end is being run.
    #[test]
    fn an_object_the_runtime_made_ends_its_constructors() {
        const SERVER: &str = "Lnet/minecraft/server/MinecraftServer;";
        let throws = format!("t/AMixin.fails.({INFO})V");
        let takes = format!("t/BMixin.takes.({SERVER}Ljava/util/concurrent/Executor;J{INFO})V");
        let throws: &'static str = Box::leak(throws.into_boxed_str());
        let takes: &'static str = Box::leak(takes.into_boxed_str());
        let above = format!("t/ZMixin.above.({INFO})V");
        let above: &'static str = Box::leak(above.into_boxed_str());
        let end = "hz/inject:net/minecraft/server/level/ServerLevel.<init>@RETURN";
        let mut rt = runtime(&[
            ("t/AMixin", vec![(end, throws, false, 9)]),
            ("t/BMixin", vec![(end, takes, false, 4)]),
            // The class above a server's level, as the game's own classes say it is: first.
            (
                "t/ZMixin",
                vec![(
                    "hz/inject:net/minecraft/world/level/Level.<init>@TAIL",
                    above,
                    false,
                    7,
                )],
            ),
        ]);
        let level = rt.new_object("net/minecraft/server/level/ServerLevel");
        let server = rt.new_object("net/minecraft/server/MinecraftServer");
        assert!(constructors_heard(
            &mut rt,
            "net/minecraft/client/multiplayer/ClientLevel"
        ));
        assert!(!constructors_heard(
            &mut rt,
            "net/minecraft/server/MinecraftServer"
        ));
        constructed(
            &mut rt,
            level,
            "net/minecraft/server/level/ServerLevel",
            &[(SERVER, server)],
        );
        assert!(rt.thrown.is_none());
        assert_eq!(
            log(&mut rt),
            [
                [V::I(7), level, V::I(0), V::Void],
                [V::I(9), level, V::I(0), V::Void],
                [V::I(4), level, V::I(3), server]
            ]
        );
        let other = rt.new_object("net/minecraft/server/level/ServerLevel");
        constructed_late(&mut rt, other, "t/BMixin");
        assert_eq!(log(&mut rt)[3], [V::I(4), other, V::I(3), V::Void]);
        CONSTRUCTING.with(|c| c.borrow_mut().push(other.r()));
        constructed_late(&mut rt, other, "t/BMixin");
        CONSTRUCTING.with(|c| c.borrow_mut().clear());
        assert_eq!(log(&mut rt).len(), 4);
    }
}
