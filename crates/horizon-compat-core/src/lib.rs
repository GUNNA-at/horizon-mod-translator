//! The object model of mods the translator (`horizon-mod-translator`) turned
//! into Rust. Generated code works on [`V`] values and a [`Rt`] that owns the
//! mod's objects and static fields. Its calls into Minecraft's and NeoForge's
//! API go to `horizon-compat`, which maps them onto Horizon: directly when the
//! runtime is inlined into a mod (the game), or through [`Fns`] when the engine
//! links this crate alone, so the mapping builds as its own small crate.
/// The runtime's maps hash with FxHash (rustc's) instead of SipHash: their keys are the field,
/// class and method names translated code looks up on every access (an object's fields, statics,
/// interned values), never input from outside, and SipHash was the hottest part of a mod's tick.
pub type HashMap<K, V> = std::collections::HashMap<K, V, std::hash::BuildHasherDefault<FxHasher>>;

/// FxHash: a word at a time, rotate, xor, multiply.
#[derive(Default, Clone, Copy)]
pub struct FxHasher {
    hash: u64,
}

const FX_SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl FxHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(FX_SEED);
    }
}

impl std::hash::Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.add(u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut word = [0u8; 8];
            word[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(word));
        }
    }
    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(u64::from(i));
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(u64::from(i));
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

pub mod gc;
pub mod host;

/// A JVM value. References index [`Rt::heap`]; 0 is null.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum V {
    #[default]
    Void,
    I(i32),
    J(i64),
    F(f32),
    D(f64),
    R(u32),
}

impl V {
    pub fn i(self) -> i32 {
        match self {
            V::I(v) => v,
            _ => 0,
        }
    }
    pub fn j(self) -> i64 {
        match self {
            V::J(v) => v,
            _ => 0,
        }
    }
    pub fn f(self) -> f32 {
        match self {
            V::F(v) => v,
            _ => 0.0,
        }
    }
    pub fn d(self) -> f64 {
        match self {
            V::D(v) => v,
            _ => 0.0,
        }
    }
    pub fn r(self) -> u32 {
        match self {
            V::R(v) => v,
            _ => 0,
        }
    }
    pub const NULL: V = V::R(0);
}

/// What an object holds.
#[derive(Debug, Clone)]
pub enum Data {
    /// An instance of a mod class (or an API class the runtime models as fields).
    Fields(HashMap<&'static str, V>),
    Str(String),
    Array(Vec<V>),
    /// A string-keyed map (`CompoundTag`).
    Map(std::collections::HashMap<String, V>),
    /// An API object Horizon owns, by kind and handle (a block state id, a position...).
    Native(&'static str, [i64; 4]),
    /// A `java.util` list (also deques and queues, and a map entry's key and value).
    List(Vec<V>),
    /// A `java.util` map or set ([`Table`]).
    Table(Table),
    /// An iterator: the items it walks and the next position.
    Iter(Vec<V>, usize),
    /// A stream's elements (streams here are eager).
    Stream(Vec<V>),
    /// A byte buffer (`FriendlyByteBuf`, netty's `ByteBuf`): its bytes and where reading is.
    Bytes(Vec<u8>, usize),
}

#[derive(Debug, Clone)]
pub struct Object {
    pub class: &'static str,
    /// The mod class's id (generated `CLASS_IDS`), [`LAMBDA`] plus the implementation's id for a
    /// lambda, 0 for everything else. Virtual calls jump on it.
    pub cid: u32,
    pub data: Data,
}

/// The bit marking a lambda object's id ([`Object::cid`]).
pub const LAMBDA: u32 = 1 << 31;

/// A generated dispatcher: `(rt, receiver's class id, the call's API key, args)`.
pub type Dispatch = fn(&mut Rt, u32, &'static str, &[V]) -> V;

/// The translated mod's runtime state.
pub struct Rt {
    pub heap: Vec<Object>,
    pub statics: HashMap<&'static str, V>,
    /// Interned strings by value, so `ldc` gives the same reference twice.
    strings: HashMap<String, u32>,
    /// API methods the mod called that Horizon doesn't map yet, with counts.
    pub unmapped: HashMap<&'static str, u64>,
    /// The exception being thrown, while it unwinds.
    pub thrown: Option<V>,
    /// Superclasses of classes the runtime knows (the mod's own, registered by
    /// generated code, and the JDK's exceptions).
    pub supers: HashMap<&'static str, &'static str>,
    /// Interfaces each of the mod's classes declares (registered by generated code).
    pub interfaces: HashMap<&'static str, &'static [&'static str]>,
    /// The game, only while a hook runs translated code (see [`Rt::with_host`]).
    host: Option<std::ptr::NonNull<dyn host::Host>>,
    /// The generated `vcall` and `lambda_call`, so API code can call back into the mod.
    pub vcall: Option<fn(&mut Rt, &'static str, &'static str, &[V]) -> V>,
    /// A call on a lambda object by its implementation's id.
    pub lambda_call: Option<fn(&mut Rt, u32, &[V]) -> Option<V>>,
    /// Mod class ids by name (generated `CLASS_IDS`).
    pub class_ids: HashMap<&'static str, u32>,
    /// Each instance method some mod class implements (`name.desc`): its dispatcher, and whether a
    /// class id implements it.
    pub vkeys: HashMap<&'static str, (Dispatch, fn(u32) -> bool)>,
    /// `instanceof` answers by (class, target) names' addresses: both are static strings.
    is_a_cache: std::cell::RefCell<HashMap<(usize, usize, usize, usize), bool>>,
    /// Methods an uncaught exception left, innermost first (with `HORIZON_COMPAT_TRACE`).
    pub unwinding: Vec<&'static str>,
    /// Freed heap slots, reused by allocations ([`gc`]).
    pub free: Vec<u32>,
    /// Live objects at which the next collection runs.
    pub collect_at: usize,
    /// Values the game holds across hook calls: roots for the collector.
    pub pinned: Vec<V>,
    /// `registered` by (object, registry), and the size it was built at.
    registered_index: std::cell::RefCell<(usize, HashMap<(u32, String), String>)>,
    /// Whether a mod class implements a method itself (`name.desc`), so glue calls only real overrides.
    pub overrides: Option<fn(&str, &str) -> bool>,
    /// Reflection over the mod's classes (generated): `(class, what, args)` with `what` one of
    /// `<clinit>` (initialise it), `<init>.()V` (construct it), `values` (an enum's constants) or
    /// `services` (the implementations `META-INF/services` names for an interface, as a String[]).
    /// None when the mod has no such class or member.
    pub reflect: Option<fn(&mut Rt, &str, &str, &[V]) -> Option<V>>,
    /// Registered objects by registry and name (`block`, `create:shaft` → the mod's Block).
    pub registered: HashMap<(String, String), V>,
    /// Suppliers registered and not yet resolved.
    pub suppliers: Vec<(String, String, V)>,
    /// The mod's namespace (its mod id), for names registered without one.
    pub namespace: String,
    /// The mods FML would list (`ModList.isLoaded`): the `@Mod`s this engine carries, the jar's
    /// own and those bundled in it. Set by generated code before the mods are constructed.
    pub mods: Vec<String>,
    /// Classes whose static initialiser ran.
    pub initialized: std::collections::HashSet<&'static str>,
    /// Values Java compares by identity, one object each (see `api_common`).
    pub interned: HashMap<String, V>,
    /// The collection an iterator, key set or map entry came from, for `remove()` and `setValue`.
    pub iter_sources: HashMap<u32, V>,
    /// Fields of objects whose data is not [`Data::Fields`]: a mod class extending a JDK collection
    /// is the collection, and keeps its own fields here (Create's `UniqueLinkedList`, a
    /// `LinkedList` with a `HashSet` of what it holds).
    pub side_fields: HashMap<u32, HashMap<&'static str, V>>,
    /// The mod event bus's listeners: the event class each takes, and the listener.
    pub listeners: Vec<(String, V)>,
    /// [`Rt::listeners`] that hear each event class posted so far (the class's own and its
    /// superclasses' listeners, in the order they listen): what posting an event walks instead of
    /// every listener. Made again once a listener is added or the class tree grows.
    pub listeners_by_event: HashMap<String, Vec<V>>,
    /// The listener and superclass counts [`Rt::listeners_by_event`] was made with.
    pub listeners_indexed: (usize, usize),
    /// `@SubscribeEvent` methods by class (registered by generated code): (event class, method
    /// key, static). [`Rt::subscribe_class`] and [`Rt::subscribe_object`] listen with them.
    pub subscribers: HashMap<&'static str, Vec<(&'static str, &'static str, bool, u32)>>,
    /// Each lambda object's functional interface and its method name.
    pub lambda_info: HashMap<u32, (&'static str, &'static str)>,
    /// The class of the last call into a caller-sensitive method (`StackWalker.getCallerClass`).
    pub caller_class: &'static str,
    /// Registries the mod makes (`ResourceKey.createRegistryKey`), fired after vanilla's.
    pub custom_registries: Vec<String>,
    /// The mod's live block entities, by position.
    pub block_entities: HashMap<[i32; 3], V>,
    /// For each live block entity: its block, and whether its last save wrote anything.
    pub block_entity_meta: HashMap<[i32; 3], (String, bool)>,
    /// Keys made at runtime (`static_key`).
    keys: HashMap<String, &'static str>,
    /// The runtime's random stream (`RandomSource` without the game's own).
    pub random_state: u64,
}

impl Default for Rt {
    fn default() -> Rt {
        Rt {
            heap: Vec::new(),
            statics: HashMap::default(),
            strings: HashMap::default(),
            unmapped: HashMap::default(),
            thrown: None,
            supers: HashMap::default(),
            interfaces: HashMap::default(),
            subscribers: HashMap::default(),
            host: None,
            vcall: None,
            lambda_call: None,
            class_ids: HashMap::default(),
            vkeys: HashMap::default(),
            is_a_cache: Default::default(),
            unwinding: Vec::new(),
            registered_index: Default::default(),
            free: Vec::new(),
            collect_at: 200_000,
            pinned: Vec::new(),
            overrides: None,
            reflect: None,
            registered: HashMap::default(),
            suppliers: Vec::new(),
            namespace: String::new(),
            mods: Vec::new(),
            initialized: Default::default(),
            interned: HashMap::default(),
            iter_sources: HashMap::default(),
            side_fields: HashMap::default(),
            listeners: Vec::new(),
            listeners_by_event: HashMap::default(),
            listeners_indexed: (0, 0),
            lambda_info: HashMap::default(),
            caller_class: "java/lang/Object",
            custom_registries: Vec::new(),
            block_entities: HashMap::default(),
            block_entity_meta: HashMap::default(),
            keys: HashMap::default(),
            random_state: 0x9E37_79B9_7F4A_7C15,
        }
    }
}

impl Rt {
    /// A `&'static str` for a key built at runtime, made once per distinct key.
    pub fn static_key(&mut self, key: &str) -> &'static str {
        if let Some(k) = self.keys.get(key) {
            return k;
        }
        let k: &'static str = Box::leak(key.to_owned().into_boxed_str());
        self.keys.insert(key.to_owned(), k);
        k
    }

    /// Runs `f` with `host` as the game. The host is borrowed for exactly this call.
    pub fn with_host<R>(&mut self, host: &mut dyn host::Host, f: impl FnOnce(&mut Rt) -> R) -> R {
        // SAFETY: the pointer lives in `self.host` only until `f` returns, while `host` is
        // mutably borrowed by this call; nothing else can reach it, and it is cleared after.
        let erased: &mut (dyn host::Host + 'static) = unsafe { std::mem::transmute(host) };
        let previous = self.host.replace(std::ptr::NonNull::from(erased));
        let r = f(self);
        self.host = previous;
        r
    }

    pub fn host(&self) -> Option<&dyn host::Host> {
        // SAFETY: see `with_host`: set only while the borrowed host is alive.
        self.host.map(|p| unsafe { &*p.as_ptr() })
    }

    pub fn host_mut(&mut self) -> Option<&mut dyn host::Host> {
        // SAFETY: as above; `&mut self` makes this the only access.
        self.host.map(|p| unsafe { &mut *p.as_ptr() })
    }

    /// Calls a functional-interface method on `target` (a lambda or a mod object).
    pub fn call_functional(&mut self, key: &'static str, target: V, args: &[V]) -> V {
        let mut all = vec![target];
        all.extend_from_slice(args);
        let class = self.class_of(target);
        if class == "compat:apply" {
            // `Function.apply(captured[1])` on captured[0]: a registerBlock factory with its properties.
            let c = self.captured(target);
            let (f, arg) = (c.first().copied().unwrap_or_default(), c.get(1).copied().unwrap_or_default());
            return self.call_functional("java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;", f, &[arg]);
        }
        let cid = self.cid(target);
        if cid & LAMBDA != 0
            && let Some(lc) = self.lambda_call
            && let Some(r) = lc(self, cid & !LAMBDA, &all)
        {
            return r;
        }
        // A mod object implementing the interface: dispatch on its class by `name.desc`.
        let short: &'static str = key.split_once('.').map_or(key, |(_, r)| r);
        match self.vcall {
            Some(vc) => vc(self, key, short, &all),
            None => V::Void,
        }
    }

    /// Resolves registered suppliers (NeoForge does this when registries fire), in order.
    pub fn resolve_registrations(&mut self) {
        let pending = std::mem::take(&mut self.suppliers);
        for (registry, name, supplier) in pending {
            let obj = self.call_functional("java/util/function/Supplier.get.()Ljava/lang/Object;", supplier, &[]);
            if obj.r() != 0 {
                self.registered.insert((registry, name), obj);
            }
        }
    }

    /// The registered name of a mod object (`create:shaft`), if it is one.
    pub fn registered_name(&self, obj: V, registry: &str) -> Option<&str> {
        // Indexed by object, rebuilt when registrations were added; checked against the map.
        let found = {
            let mut index = self.registered_index.borrow_mut();
            if index.0 != self.registered.len() {
                index.1 = self.registered.iter().map(|((r, n), v)| ((v.r(), r.clone()), n.clone())).collect();
                index.0 = self.registered.len();
            }
            index.1.get(&(obj.r(), registry.to_owned())).cloned()
        };
        if let Some(name) = found
            && let Some(((_, n), v)) = self.registered.get_key_value(&(registry.to_owned(), name))
            && *v == obj
        {
            return Some(n.as_str());
        }
        self.registered.iter().find(|((r, _), v)| r == registry && **v == obj).map(|((_, n), _)| n.as_str())
    }

    pub fn new() -> Rt {
        let mut rt = Rt::default();
        rt.heap.push(Object { class: "null", cid: 0, data: Data::Array(Vec::new()) });
        for (c, sup) in JDK_SUPERS {
            rt.supers.insert(c, sup);
        }
        rt
    }

    pub fn alloc(&mut self, class: &'static str, data: Data) -> V {
        self.alloc_cid(class, 0, data)
    }

    pub fn alloc_cid(&mut self, class: &'static str, cid: u32, data: Data) -> V {
        if let Some(r) = self.free.pop() {
            self.heap[r as usize] = Object { class, cid, data };
            return V::R(r);
        }
        self.heap.push(Object { class, cid, data });
        V::R(self.heap.len() as u32 - 1)
    }

    /// A point where no translated code runs (a hook's outermost entry, before it makes any
    /// value): collects garbage when enough was allocated.
    pub fn safe_point(&mut self) {
        if self.host.is_none() {
            self.maybe_collect();
        }
    }

    /// A new object of `class`, a mod class's carrying its id (looked up by name).
    pub fn new_object(&mut self, class: &'static str) -> V {
        let cid = self.class_ids.get(class).copied().unwrap_or(0);
        self.alloc_cid(class, cid, Data::Fields(HashMap::default()))
    }

    /// A new object of `class` whose id generated code knows.
    pub fn new_object_id(&mut self, class: &'static str, cid: u32) -> V {
        self.alloc_cid(class, cid, Data::Fields(HashMap::default()))
    }

    /// An object's class id ([`Object::cid`]); 0 for null and non-objects.
    pub fn cid(&self, v: V) -> u32 {
        match v {
            V::R(r) if r != 0 => self.heap.get(r as usize).map_or(0, |o| o.cid),
            _ => 0,
        }
    }

    /// The receiver's class id in a call's arguments.
    pub fn recv_cid(&self, args: &[V]) -> u32 {
        self.cid(args.first().copied().unwrap_or_default())
    }

    pub fn string(&mut self, s: &str) -> V {
        if let Some(&r) = self.strings.get(s) {
            return V::R(r);
        }
        let v = self.alloc("java/lang/String", Data::Str(s.to_owned()));
        self.strings.insert(s.to_owned(), v.r());
        v
    }

    pub fn str_of(&self, v: V) -> Option<&str> {
        match &self.heap.get(v.r() as usize)?.data {
            Data::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn class_of(&self, v: V) -> &'static str {
        self.heap.get(v.r() as usize).map_or("null", |o| o.class)
    }

    pub fn get_field(&self, obj: V, name: &'static str) -> V {
        match self.heap.get(obj.r() as usize).map(|o| &o.data) {
            Some(Data::Fields(f)) => f.get(name).copied().unwrap_or_default(),
            Some(_) if obj.r() != 0 => self.side_fields.get(&obj.r()).and_then(|f| f.get(name)).copied().unwrap_or_default(),
            _ => V::Void,
        }
    }

    pub fn put_field(&mut self, obj: V, name: &'static str, v: V) {
        match self.heap.get_mut(obj.r() as usize) {
            Some(Object { data: Data::Fields(f), .. }) => {
                f.insert(name, v);
            }
            Some(o) if obj.r() != 0 && o.class != crate::gc::FREE => {
                self.side_fields.entry(obj.r()).or_default().insert(name, v);
            }
            _ => {}
        }
    }

    pub fn new_array(&mut self, len: i32, zero: V) -> V {
        self.alloc("[", Data::Array(vec![zero; len.max(0) as usize]))
    }

    pub fn array_len(&self, a: V) -> i32 {
        match self.heap.get(a.r() as usize).map(|o| &o.data) {
            Some(Data::Array(v)) => v.len() as i32,
            _ => 0,
        }
    }

    pub fn array_get(&self, a: V, i: i32) -> V {
        match self.heap.get(a.r() as usize).map(|o| &o.data) {
            Some(Data::Array(v)) => v.get(i as usize).copied().unwrap_or_default(),
            _ => V::Void,
        }
    }

    pub fn array_set(&mut self, a: V, i: i32, x: V) {
        if let Some(Object { data: Data::Array(v), .. }) = self.heap.get_mut(a.r() as usize)
            && let Some(slot) = v.get_mut(i as usize)
        {
            *slot = x;
        }
    }
}

/// The JDK's own throwables, with their superclasses.
const JDK_SUPERS: [(&str, &str); 12] = [
    ("java/lang/Throwable", "java/lang/Object"),
    ("java/lang/Exception", "java/lang/Throwable"),
    ("java/lang/Error", "java/lang/Throwable"),
    ("java/lang/RuntimeException", "java/lang/Exception"),
    ("java/lang/ArithmeticException", "java/lang/RuntimeException"),
    ("java/lang/IllegalArgumentException", "java/lang/RuntimeException"),
    ("java/lang/IllegalStateException", "java/lang/RuntimeException"),
    ("java/lang/NullPointerException", "java/lang/RuntimeException"),
    ("java/lang/IndexOutOfBoundsException", "java/lang/RuntimeException"),
    ("java/lang/ArrayIndexOutOfBoundsException", "java/lang/IndexOutOfBoundsException"),
    ("java/lang/UnsupportedOperationException", "java/lang/RuntimeException"),
    ("java/lang/NumberFormatException", "java/lang/IllegalArgumentException"),
];

impl Rt {
    /// Whether `v` is an instance of `class` (walking known superclasses).
    /// The one object for `key`, made by `make` the first time.
    pub fn intern(&mut self, key: String, make: impl FnOnce(&mut Rt) -> V) -> V {
        if let Some(v) = self.interned.get(&key) {
            return *v;
        }
        let v = make(self);
        self.interned.insert(key, v);
        v
    }

    /// Listens with `class`'s static `@SubscribeEvent` methods: an `@EventBusSubscriber` class, or
    /// `bus.register(Foo.class)`.
    pub fn subscribe_class(&mut self, class: &str) {
        for (event, key, is_static, id) in self.subscribers.get(class).cloned().unwrap_or_default() {
            if is_static {
                let l = self.lambda_id(key, id, Vec::new());
                self.listeners.push((event.to_owned(), l));
            }
        }
    }

    /// Listens with `obj`'s instance `@SubscribeEvent` methods, its superclasses' too: `bus.register(obj)`.
    pub fn subscribe_object(&mut self, obj: V) {
        let mut class = Some(self.class_of(obj));
        while let Some(c) = class {
            for (event, key, is_static, id) in self.subscribers.get(c).cloned().unwrap_or_default() {
                if !is_static {
                    let l = self.lambda_id(key, id, vec![obj]);
                    self.listeners.push((event.to_owned(), l));
                }
            }
            class = self.supers.get(c).copied();
        }
    }

    pub fn instance_of(&self, v: V, class: &str) -> bool {
        v.r() != 0 && self.is_a(self.class_of(v), class)
    }

    /// [`Rt::instance_of`] for generated code (`instanceof`, `catch`): both names are static
    /// strings, so answers are kept by their addresses.
    pub fn instance_of_static(&self, v: V, class: &'static str) -> bool {
        if v.r() == 0 || matches!(v, V::Void) {
            return false;
        }
        let c = self.class_of(v);
        let key = (c.as_ptr() as usize, c.len(), class.as_ptr() as usize, class.len());
        if let Some(&hit) = self.is_a_cache.borrow().get(&key) {
            return hit;
        }
        let answer = self.is_a(c, class);
        self.is_a_cache.borrow_mut().insert(key, answer);
        answer
    }

    /// Whether class `c` is `class` or a subtype of it: its superclasses and every interface they
    /// implement, as the JVM's `instanceof` and `Class.isAssignableFrom` answer. Past the mod's own
    /// classes, Minecraft's are known by family (`api_is_a`, horizon-compat's table).
    pub fn is_a(&self, c: &str, class: &str) -> bool {
        if class == "java/lang/Object" {
            return true;
        }
        let mut todo: Vec<&str> = vec![c];
        let mut seen: Vec<&str> = Vec::new();
        while let Some(k) = todo.pop() {
            if k == class || api_is_a(k, class) {
                return true;
            }
            if seen.contains(&k) {
                continue;
            }
            seen.push(k);
            if let Some(&s) = self.supers.get(k) {
                todo.push(s);
            }
            if let Some(is) = self.interfaces.get(k) {
                todo.extend(is.iter().copied());
            }
        }
        false
    }

    /// Starts throwing a new exception of `class`.
    /// Whether `HORIZON_COMPAT_TRACE` is set: report unmapped calls and uncaught exceptions.
    pub fn tracing() -> bool {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ON.get_or_init(|| std::env::var_os("HORIZON_COMPAT_TRACE").is_some())
    }

    /// An exception nothing caught, at a top level (`what`: a static initialiser, the mod's
    /// constructor, an event listener): reported when tracing, then dropped so loading goes on.
    pub fn report_thrown(&mut self, what: &str) {
        let path = std::mem::take(&mut self.unwinding);
        if let Some(e) = self.thrown.take()
            && Self::tracing()
        {
            let msg = self.get_field(e, "message");
            let msg = self.str_of(msg).unwrap_or("").to_owned();
            eprintln!("horizon-compat: uncaught {} in {what}: {msg}", self.class_of(e));
            for m in path.iter().take(24) {
                eprintln!("    at {m}");
            }
        }
    }

    /// An exception left method `m` uncaught (generated code calls this as it unwinds).
    #[inline]
    pub fn unwound(&mut self, m: &'static str) {
        if Self::tracing() {
            if self.unwinding.len() > 256 {
                self.unwinding.clear();
            }
            self.unwinding.push(m);
        }
    }

    /// [`Rt::reflect`]: None without a generated table.
    pub fn reflect(&mut self, class: &str, what: &str, args: &[V]) -> Option<V> {
        self.reflect.and_then(|f| f(self, class, what, args))
    }

    /// Whether `class` is one of the mod's own (translated) classes.
    pub fn is_mod_class(&self, class: &str) -> bool {
        self.supers.contains_key(class)
    }

    pub fn throw_new(&mut self, class: &'static str) {
        let e = self.new_object(class);
        self.thrown = Some(e);
    }

    /// `new T[a][b]...`: nested arrays, innermost filled with `zero`.
    pub fn new_multi(&mut self, dims: &[i32], zero: V) -> V {
        match dims {
            [] => zero,
            [n] => self.new_array(*n, zero),
            [n, rest @ ..] => {
                let outer = self.new_array(*n, V::NULL);
                for i in 0..(*n).max(0) {
                    let inner = self.new_multi(rest, zero);
                    self.array_set(outer, i, inner);
                }
                outer
            }
        }
    }

    /// `Objects.equals` for translated values: strings by content, boxed
    /// integers by value, other references by identity.
    pub fn java_equals(&self, a: V, b: V) -> bool {
        if a == b {
            return true;
        }
        match (self.str_of(a), self.str_of(b)) {
            (Some(x), Some(y)) => x == y,
            _ => {
                matches!((a, b), (V::R(x), V::R(y)) if x != 0 && y != 0
                    && self.class_of(a) == "java/lang/Integer" && self.class_of(b) == "java/lang/Integer"
                    && self.get_field(a, "value") == self.get_field(b, "value"))
            }
        }
    }

    /// `hashCode` for translated values, as the JDK computes it for
    /// primitives and strings; other objects hash by identity.
    pub fn java_hash(&self, v: V) -> i32 {
        match v {
            V::Void => 0,
            V::I(i) => i,
            V::J(j) => (j ^ (j >> 32)) as i32,
            V::F(f) => f.to_bits() as i32,
            V::D(d) => {
                let b = d.to_bits() as i64;
                (b ^ (b >> 32)) as i32
            }
            V::R(0) => 0,
            V::R(r) => match self.str_of(v) {
                Some(s) => s.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32)),
                None if self.class_of(v) == "java/lang/Integer" => self.get_field(v, "value").i(),
                None => r as i32,
            },
        }
    }

    /// A record's generated `equals`, `hashCode` and `toString`
    /// (`ObjectMethods.bootstrap`): `fields` are (name, kind).
    pub fn record_method(&mut self, which: &str, simple_name: &str, fields: &[(&'static str, char)], args: &[V]) -> V {
        let this = args.first().copied().unwrap_or_default();
        match which {
            "equals" => {
                let other = args.get(1).copied().unwrap_or_default();
                let same = other.r() != 0
                    && self.class_of(this) == self.class_of(other)
                    && fields.iter().all(|(f, _)| self.java_equals(self.get_field(this, f), self.get_field(other, f)));
                V::I(same as i32)
            }
            "hashCode" => V::I(fields.iter().fold(0i32, |h, (f, _)| h.wrapping_mul(31).wrapping_add(self.java_hash(self.get_field(this, f))))),
            _ => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(f, k)| format!("{f}={}", self.to_java_string(self.get_field(this, f), *k)))
                    .collect();
                let text = format!("{simple_name}[{}]", parts.join(", "));
                self.alloc("java/lang/String", Data::Str(text))
            }
        }
    }

    /// A lambda object: its class is the implementing method's key, its data the captured values.
    pub fn lambda(&mut self, implementation: &'static str, captured: Vec<V>) -> V {
        self.alloc(implementation, Data::Array(captured))
    }

    /// A lambda the generated `lambda_call` runs by `id`.
    pub fn lambda_id(&mut self, implementation: &'static str, id: u32, captured: Vec<V>) -> V {
        self.alloc_cid(implementation, LAMBDA | id, Data::Array(captured))
    }

    pub fn captured(&self, v: V) -> Vec<V> {
        match self.heap.get(v.r() as usize).map(|o| &o.data) {
            Some(Data::Array(c)) => c.clone(),
            _ => Vec::new(),
        }
    }

    /// `String.valueOf` of a value whose static kind is `kind` ('I', 'Z', 'C', 'J', 'F', 'D', 'L').
    pub fn to_java_string(&self, v: V, kind: char) -> String {
        match kind {
            'Z' => (v.i() != 0).to_string(),
            'C' => char::from_u32(v.i() as u32 & 0xffff).map_or_else(String::new, String::from),
            'J' => v.j().to_string(),
            'F' => java_double(v.f() as f64, true),
            'D' => java_double(v.d(), false),
            'L' => match self.str_of(v) {
                Some(s) => s.to_owned(),
                None if v.r() == 0 => "null".into(),
                None => match self.heap.get(v.r() as usize).map(|o| &o.data) {
                    Some(Data::Fields(f)) if f.contains_key("value") => f["value"].i().to_string(),
                    _ => format!("{}@{:x}", self.class_of(v).replace('/', "."), v.r()),
                },
            },
            _ => v.i().to_string(),
        }
    }

    /// `StringConcatFactory.makeConcatWithConstants`: `\u{1}` takes the next
    /// argument, `\u{2}` the next constant.
    pub fn concat(&mut self, recipe: &str, kinds: &str, args: &[V], constants: &[&str]) -> V {
        let mut out = String::new();
        let (mut a, mut c) = (0, 0);
        let kinds: Vec<char> = kinds.chars().collect();
        for ch in recipe.chars() {
            match ch {
                '\u{1}' => {
                    out.push_str(&self.to_java_string(args.get(a).copied().unwrap_or_default(), kinds.get(a).copied().unwrap_or('L')));
                    a += 1;
                }
                '\u{2}' => {
                    out.push_str(constants.get(c).copied().unwrap_or(""));
                    c += 1;
                }
                ch => out.push(ch),
            }
        }
        self.alloc("java/lang/String", Data::Str(out))
    }
}

/// `Double.toString` / `Float.toString`: shortest round-trip digits, as the
/// JDK prints them since 19, with Java's `1.0`, `1.0E10` and `NaN` forms.
pub fn java_double(d: f64, float: bool) -> String {
    if d.is_nan() {
        return "NaN".into();
    }
    if d.is_infinite() {
        return if d > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if d == 0.0 {
        return if d.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    let a = d.abs();
    if (1e-3..1e7).contains(&a) {
        let s = if float { format!("{}", d as f32) } else { format!("{d}") };
        return if s.contains('.') { s } else { format!("{s}.0") };
    }
    let s = if float { format!("{:e}", d as f32) } else { format!("{d:e}") };
    let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
    let m = if m.contains('.') { m.to_owned() } else { format!("{m}.0") };
    format!("{m}E{e}")
}

/// Java's integer division and remainder: no panic on zero (the JVM would
/// throw; translated code returns 0) and `MIN / -1` wraps.
pub fn idiv(a: i32, b: i32) -> i32 {
    if b == 0 { 0 } else { a.wrapping_div(b) }
}
pub fn irem(a: i32, b: i32) -> i32 {
    if b == 0 { 0 } else { a.wrapping_rem(b) }
}
pub fn ldiv(a: i64, b: i64) -> i64 {
    if b == 0 { 0 } else { a.wrapping_div(b) }
}
pub fn lrem(a: i64, b: i64) -> i64 {
    if b == 0 { 0 } else { a.wrapping_rem(b) }
}

/// `fcmpl`/`dcmpl` (`nan_is` -1) and `fcmpg`/`dcmpg` (+1).
pub fn fcmp(a: f64, b: f64, nan_is: i32) -> i32 {
    if a.is_nan() || b.is_nan() {
        nan_is
    } else if a < b {
        -1
    } else if a > b {
        1
    } else {
        0
    }
}

/// Java's float-to-int conversion: NaN is 0, out of range saturates (Rust's `as` does both).
pub fn d2i(d: f64) -> i32 {
    d as i32
}

/// A map or set: entries in insertion order (sorted, for a `TreeMap`), indexed by hash. The
/// operations live in `horizon-compat`'s `jutil`, which hashes keys as Java does.
#[derive(Debug, Clone, Default)]
pub struct Table {
    pub entries: Vec<(V, V)>,
    pub index: HashMap<i32, Vec<usize>>,
    /// A `TreeMap`/`TreeSet`: entries kept sorted, by `comparator` when it isn't null.
    pub sorted: bool,
    pub comparator: V,
}

// BEGIN ENGINE ENTRY POINTS
// (left out when the runtime is inlined into a mod, where these modules are the mapping itself)

/// The API mapping's entry points, which generated code calls as `api::call`, `api::get_static`,
/// `neo::constructor_arg` and `neo::fire_registrations`. `horizon_compat::install()` sets them.
#[derive(Clone, Copy)]
pub struct Fns {
    pub call: fn(&mut Rt, &'static str, &[V]) -> V,
    pub get_static: fn(&mut Rt, &'static str) -> V,
    pub constructor_arg: fn(&mut Rt, &str) -> V,
    pub fire_registrations: fn(&mut Rt),
    /// Minecraft's class families for `instanceof` (`api_common::api_is_a`).
    pub api_is_a: fn(&str, &str) -> bool,
}

static FNS: std::sync::OnceLock<Fns> = std::sync::OnceLock::new();

/// Installs the API mapping (once per process; later calls are ignored).
pub fn install_fns(f: Fns) {
    let _ = FNS.set(f);
}

fn fns() -> &'static Fns {
    FNS.get().expect("call horizon_compat::install() before running a translated engine")
}

/// Minecraft's class families, from the installed mapping; none known before `install()` (the
/// mod's own classes still answer through their registered hierarchy).
fn api_is_a(c: &str, class: &str) -> bool {
    FNS.get().is_some_and(|f| (f.api_is_a)(c, class))
}

/// The API calls generated code makes, through the installed mapping.
pub mod api {
    use super::{Rt, V, fns};
    pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        (fns().call)(rt, key, args)
    }
    pub fn get_static(rt: &mut Rt, key: &'static str) -> V {
        (fns().get_static)(rt, key)
    }
}

/// Mod loading, through the installed mapping.
pub mod neo {
    use super::{Rt, V, fns};
    pub fn constructor_arg(rt: &mut Rt, class: &str) -> V {
        (fns().constructor_arg)(rt, class)
    }
    pub fn fire_registrations(rt: &mut Rt) {
        (fns().fire_registrations)(rt)
    }
}
// END ENGINE ENTRY POINTS

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_arithmetic_edges() {
        assert_eq!(idiv(i32::MIN, -1), i32::MIN);
        assert_eq!(irem(-7, 3), -1);
        assert_eq!(fcmp(f64::NAN, 1.0, -1), -1);
        assert_eq!(fcmp(f64::NAN, 1.0, 1), 1);
        assert_eq!(d2i(f64::NAN), 0);
        assert_eq!(d2i(1e20), i32::MAX);
    }

    #[test]
    fn strings_are_interned_and_fields_round_trip() {
        let mut rt = Rt::new();
        let a = rt.string("create");
        assert_eq!(a, rt.string("create"));
        let o = rt.new_object("com/example/Thing");
        rt.put_field(o, "speed", V::F(2.5));
        assert_eq!(rt.get_field(o, "speed"), V::F(2.5));
        assert_eq!(rt.get_field(V::NULL, "speed"), V::Void);
    }
}
