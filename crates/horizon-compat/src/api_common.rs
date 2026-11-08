//! The most used API after the world: block-state properties, NBT
//! compounds, `Vec3`, item stacks, `Mth`, random sources, text components
//! and enums. Java compares many of these by identity (`state.getValue(FACING)
//! == Direction.NORTH`), so directions, axes, booleans, states, blocks and
//! items are interned: the same value is always the same object.
use crate::{Data, Rt, V};
use std::collections::HashMap;

pub(crate) const DIRECTION_NAMES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];
pub(crate) const AXIS_NAMES: [&str; 3] = ["x", "y", "z"];

/// Minecraft's own class families, for `instanceof` past the mod's classes: the API's classes
/// aren't translated, so a mod block extending `RotatedPillarBlock` is a `Block` by this table.
/// The core's `Rt::is_a` asks it (directly when inlined, through `Fns` otherwise).
pub fn api_is_a(c: &str, class: &str) -> bool {
    // 1.21.1's block and item classes by their own supertypes (`CropBlock` a `BonemealableBlock`,
    // `ChestBlock` an `EntityBlock`, `SwordItem` a `TieredItem`); the families below past them.
    if vanilla_is_a(c, class) || crate::crafting::recipe_is_a(c, class) {
        return true;
    }
    // The class trees the runtime climbs for calls too (`ZombieModel` → `HumanoidModel`,
    // `MobRenderer` → `LivingEntityRenderer`), and through one of those to a vanilla class, that
    // class's supertypes (NeoForge's `DeferredSpawnEggItem` is a `SpawnEggItem`, so an `Item`).
    let mut k = c;
    while let Some(sup) = crate::api::api_super(k) {
        if sup == class || vanilla_is_a(sup, class) {
            return true;
        }
        k = sup;
    }
    let family = |pkg: &str, suffix: &str| c.starts_with(pkg) && c.ends_with(suffix);
    match class {
        "net/minecraft/world/level/block/Block" | "net/minecraft/world/level/block/state/BlockBehaviour" => {
            family("net/minecraft/world/level/block/", "Block") && !c.contains("/entity/")
        }
        "net/minecraft/world/level/block/EntityBlock" => c == "net/minecraft/world/level/block/BaseEntityBlock",
        "net/minecraft/core/BlockPos" => c == "net/minecraft/core/BlockPos$MutableBlockPos",
        "net/minecraft/core/Vec3i" => matches!(c, "net/minecraft/core/BlockPos" | "net/minecraft/core/BlockPos$MutableBlockPos"),
        "net/minecraft/world/phys/shapes/CollisionContext" => c == crate::shapes::ENTITY_COLLISION_CONTEXT,
        "net/minecraft/world/phys/HitResult" => matches!(c, "net/minecraft/world/phys/BlockHitResult" | crate::projectiles::ENTITY_HIT),
        "net/minecraft/world/item/Item" => family("net/minecraft/world/item/", "Item"),
        // Fluids: vanilla's (`WaterFluid$Source` is a `FlowingFluid` and a `WaterFluid`) and NeoForge's
        // `BaseFlowingFluid` and its `Flowing` and `Source`.
        "net/minecraft/world/level/material/Fluid" => crate::fluids::is_fluid_class(c),
        "net/minecraft/world/level/material/FlowingFluid" => {
            crate::fluids::is_fluid_class(c) && !matches!(c, "net/minecraft/world/level/material/EmptyFluid" | "net/minecraft/world/level/material/Fluid")
        }
        "net/minecraft/world/level/material/WaterFluid" | "net/minecraft/world/level/material/LavaFluid" | "net/neoforged/neoforge/fluids/BaseFlowingFluid" => {
            c.starts_with(&format!("{class}$")) && !c.ends_with("$Properties")
        }
        "net/minecraft/world/level/block/entity/BlockEntity" => family("net/minecraft/world/level/block/entity/", "BlockEntity"),
        "net/minecraft/world/entity/Entity" => (c.starts_with("net/minecraft/world/entity/") && !c.contains('$')) || is_player_class(c),
        // Players: the one a hook acts for (`ServerPlayer`), the client's own and the others it sees.
        "net/minecraft/world/entity/LivingEntity" | "net/minecraft/world/entity/player/Player" => {
            is_player_class(c) || crate::vanilla_entities::is_a(c, class)
        }
        "net/minecraft/client/player/AbstractClientPlayer" => matches!(c, "net/minecraft/client/player/LocalPlayer" | "net/minecraft/client/player/RemotePlayer"),
        // NeoForge's `FakePlayer` is a server's player.
        "net/minecraft/server/level/ServerPlayer" => c == crate::fake_players::FAKE_PLAYER,
        // The GUI: vanilla's screens are containers of listeners and draw; its widgets
        // (`AbstractWidget` and the buttons, boxes and sliders under it) are listeners that draw.
        "net/minecraft/client/gui/screens/Screen"
        | "net/minecraft/client/gui/components/events/ContainerEventHandler"
        | "net/minecraft/client/gui/components/events/AbstractContainerEventHandler" => is_screen_class(c),
        "net/minecraft/client/gui/components/events/GuiEventListener" | "net/minecraft/client/gui/components/Renderable" => {
            is_screen_class(c) || is_widget_class(c)
        }
        "net/minecraft/client/gui/components/AbstractWidget" | "net/minecraft/client/gui/narration/NarratableEntry" | "net/minecraft/client/gui/layouts/LayoutElement" => {
            is_widget_class(c)
        }
        "net/minecraft/client/gui/components/AbstractButton" => matches!(c, "net/minecraft/client/gui/components/Button" | "net/minecraft/client/gui/components/ImageButton" | "net/minecraft/client/gui/components/Checkbox" | "net/minecraft/client/gui/components/CycleButton" | "net/minecraft/client/gui/components/PlainTextButton"),
        // Menus: NeoForge's slot of an item handler is a `Slot`, the inventory's menu a menu, the
        // player's inventory and `SimpleContainer` containers.
        "net/minecraft/world/inventory/Slot" => c == crate::menus::HANDLER_SLOT,
        "net/minecraft/world/inventory/AbstractContainerMenu" => c == crate::menus::INVENTORY_MENU,
        // ... and vanilla's container block entities (a chest's, a furnace's).
        "net/minecraft/world/Container" | "net/minecraft/world/Clearable" => {
            matches!(c, crate::menus::INVENTORY | crate::menus::SIMPLE_CONTAINER | crate::menus::TRANSIENT_CRAFTING) || crate::item_wrappers::is_a(c, class)
        }
        crate::menus::CRAFTING_CONTAINER | "net/minecraft/world/inventory/StackedContentsCompatible" => c == crate::menus::TRANSIENT_CRAFTING,
        "net/minecraft/world/MenuProvider" | "net/minecraft/world/inventory/MenuConstructor" => c == "net/minecraft/world/SimpleMenuProvider",
        "net/minecraft/world/inventory/ContainerData" => c == "net/minecraft/world/inventory/SimpleContainerData",
        "net/minecraft/client/gui/screens/inventory/MenuAccess" => c == "net/minecraft/client/gui/screens/inventory/AbstractContainerScreen",
        // A mod's container is FML's Java one (`neo::mod_container`).
        "net/neoforged/fml/ModContainer" => c == crate::neo::FML_CONTAINER,
        // A component's contents (`getContents`): a translation or literal text.
        "net/minecraft/network/chat/ComponentContents" => {
            matches!(c, TRANSLATABLE_CONTENTS | LITERAL_CONTENTS)
        }
        PLAIN_TEXT_CONTENTS => c == LITERAL_CONTENTS,
        // DFU's codec interfaces: a `PrimitiveCodec` (a mod's own, Curios') is a `Codec`, and a
        // codec decodes and encodes.
        "com/mojang/serialization/Codec" => c == crate::codecs::PRIMITIVE_CODEC,
        "com/mojang/serialization/Decoder" | "com/mojang/serialization/Encoder" => {
            matches!(c, "com/mojang/serialization/Codec" | crate::codecs::PRIMITIVE_CODEC)
        }
        "java/lang/ReflectiveOperationException" => matches!(
            c,
            "java/lang/ClassNotFoundException"
                | "java/lang/InstantiationException"
                | "java/lang/IllegalAccessException"
                | "java/lang/NoSuchMethodException"
                | "java/lang/NoSuchFieldException"
                | "java/lang/reflect/InvocationTargetException"
        ),
        "java/lang/RuntimeException" => {
            matches!(c, "java/lang/IllegalArgumentException" | "java/lang/IllegalStateException" | "java/lang/NullPointerException" | "java/lang/UnsupportedOperationException" | "java/lang/ClassCastException" | "java/lang/ArithmeticException" | "java/lang/IndexOutOfBoundsException" | "java/lang/ArrayIndexOutOfBoundsException" | "java/lang/NumberFormatException")
                || java_is_a(c, class)
        }
        _ => crate::resources::is_a(c, class) || crate::streams::is_a(c, class) || crate::gson::is_a(c, class) || java_is_a(c, class) || crate::vanilla_entities::is_a(c, class) || crate::fluid_wrappers::is_a(c, class) || crate::item_wrappers::is_a(c, class) || crate::energy::is_a(c, class),
    }
}

/// Whether 1.21.1's block or item class `c` is `class` or a subtype of it, by the generated table
/// of those classes' superclasses and interfaces; false for a class the table doesn't have.
pub(crate) fn vanilla_is_a(c: &str, class: &str) -> bool {
    use crate::vanilla_classes::CLASSES;
    let entry = |k: &str| CLASSES.binary_search_by(|e| e.0.cmp(k)).ok().map(|i| &CLASSES[i]);
    if entry(c).is_none() {
        return false;
    }
    let mut todo = vec![c];
    while let Some(k) = todo.pop() {
        if k == class {
            return true;
        }
        if let Some((_, sup, ifs)) = entry(k) {
            if !sup.is_empty() {
                todo.push(sup);
            }
            todo.extend_from_slice(ifs);
        }
    }
    false
}

/// The class Java has for `v`, an object of the runtime's class `c`, when the runtime stands for a
/// game object by its id: a vanilla block (`Block` naming `minecraft:ice`) is of its 1.21.1 class
/// (`IceBlock`), a vanilla item of its (`SwordItem`, `StandingAndWallBlockItem`). None for any other
/// object, and for the blocks and items 1.21.1 makes plain `Block`s, `Item`s and `BlockItem`s: the
/// runtime's class already is theirs. A block or item 1.21.1 lacks keeps the runtime's class.
pub fn vanilla_class(rt: &Rt, v: V, c: &'static str) -> Option<&'static str> {
    let table: &[(&str, &str)] = match c {
        "net/minecraft/world/level/block/Block" => &crate::vanilla_classes::BLOCKS,
        "net/minecraft/world/item/Item" | "net/minecraft/world/item/BlockItem" => &crate::vanilla_classes::ITEMS,
        _ => return None,
    };
    let id = rt.str_of(v)?;
    table.binary_search_by(|(i, _)| (*i).cmp(id)).ok().map(|i| table[i].1)
}

/// Java's own class families, for the runtime's objects (a `new HashMap<>()` is a `Map`, a
/// `Collection` and `Iterable`): collections by kind, boxed numbers, strings, and the exceptions
/// the runtime throws, each under its supertypes as the JDK declares them.
fn java_is_a(c: &str, class: &str) -> bool {
    match class {
        "java/util/Map" => is_map_class(c),
        "java/util/SortedMap" | "java/util/NavigableMap" => {
            matches!(c, "java/util/TreeMap" | "java/util/concurrent/ConcurrentSkipListMap" | "com/google/common/collect/ImmutableSortedMap")
        }
        "java/util/concurrent/ConcurrentMap" => matches!(c, "java/util/concurrent/ConcurrentHashMap" | "java/util/concurrent/ConcurrentSkipListMap"),
        "java/util/AbstractMap" => is_map_class(c) && c.starts_with("java/util/") && c != "java/util/Map",
        "java/util/HashMap" => c == "java/util/LinkedHashMap",
        "java/util/List" => is_list_class(c),
        "java/util/RandomAccess" => is_list_class(c) && !c.ends_with("LinkedList"),
        "java/util/Set" => is_set_class(c),
        "java/util/SortedSet" | "java/util/NavigableSet" => {
            matches!(c, "java/util/TreeSet" | "java/util/concurrent/ConcurrentSkipListSet" | "com/google/common/collect/ImmutableSortedSet")
        }
        "java/util/HashSet" => c == "java/util/LinkedHashSet",
        "java/util/Queue" => is_queue_class(c),
        "java/util/Deque" => matches!(c, "java/util/ArrayDeque" | "java/util/LinkedList" | "java/util/concurrent/ConcurrentLinkedDeque" | "java/util/concurrent/LinkedBlockingDeque"),
        "java/util/Collection" | "java/lang/Iterable" | "java/util/SequencedCollection" => {
            is_list_class(c) || is_set_class(c) || is_queue_class(c) || c.ends_with("Multiset") || c == "java/util/Collection"
        }
        "java/lang/Number" => matches!(
            c,
            "java/lang/Integer" | "java/lang/Long" | "java/lang/Float" | "java/lang/Double" | "java/lang/Short" | "java/lang/Byte"
                | "java/util/concurrent/atomic/AtomicInteger" | "java/util/concurrent/atomic/AtomicLong" | "java/math/BigInteger" | "java/math/BigDecimal"
        ),
        "java/lang/CharSequence" => matches!(c, "java/lang/String" | "java/lang/StringBuilder" | "java/lang/StringBuffer"),
        "java/lang/Comparable" => matches!(
            c,
            "java/lang/String" | "java/lang/Integer" | "java/lang/Long" | "java/lang/Float" | "java/lang/Double" | "java/lang/Short" | "java/lang/Byte"
                | "java/lang/Character" | "java/lang/Boolean" | "java/lang/Enum" | "java/math/BigInteger" | "java/math/BigDecimal" | "java/util/UUID"
        ),
        "java/lang/Throwable" => library_class(c) && (c.ends_with("Exception") || c.ends_with("Error")),
        "java/lang/Exception" => library_class(c) && c.ends_with("Exception"),
        "java/lang/Error" => library_class(c) && c.ends_with("Error"),
        "java/lang/RuntimeException" => matches!(
            c,
            "java/util/NoSuchElementException" | "java/util/ConcurrentModificationException" | "java/util/MissingResourceException"
                | "java/lang/ArrayStoreException" | "java/lang/NegativeArraySizeException" | "java/lang/StringIndexOutOfBoundsException"
                | "java/lang/IllegalMonitorStateException" | "java/io/UncheckedIOException" | "java/util/InputMismatchException"
                | "java/util/concurrent/CompletionException" | "java/util/concurrent/RejectedExecutionException"
                | "com/google/gson/JsonParseException" | "com/google/gson/JsonSyntaxException" | "com/google/gson/JsonIOException"
                | "net/minecraft/ResourceLocationException" | "net/minecraft/ReportedException"
        ),
        "com/google/gson/JsonParseException" => matches!(c, "com/google/gson/JsonSyntaxException" | "com/google/gson/JsonIOException"),
        "java/lang/IllegalArgumentException" => matches!(c, "java/lang/NumberFormatException" | "java/util/IllegalFormatException"),
        "java/lang/IndexOutOfBoundsException" => matches!(c, "java/lang/ArrayIndexOutOfBoundsException" | "java/lang/StringIndexOutOfBoundsException"),
        "java/lang/IllegalStateException" => c == "java/util/concurrent/CancellationException",
        "java/util/NoSuchElementException" => c == "java/util/InputMismatchException",
        "java/io/IOException" => {
            (c.starts_with("java/") && c.ends_with("IOException") && c != class)
                || matches!(
                    c,
                    "java/io/FileNotFoundException"
                        | "java/io/EOFException"
                        | "java/io/UTFDataFormatException"
                        | "java/util/zip/ZipException"
                        | "java/nio/file/FileSystemException"
                )
                || api_is_a(c, "java/nio/file/FileSystemException")
        }
        "java/nio/file/FileSystemException" => matches!(
            c,
            "java/nio/file/NoSuchFileException"
                | "java/nio/file/FileAlreadyExistsException"
                | "java/nio/file/DirectoryNotEmptyException"
                | "java/nio/file/NotDirectoryException"
                | "java/nio/file/AccessDeniedException"
                | "java/nio/file/AtomicMoveNotSupportedException"
                | "java/nio/file/FileSystemLoopException"
                | "java/nio/file/NotLinkException"
        ),
        _ => false,
    }
}

/// A class of Java's or a library's the runtime throws (a mod's own classes answer through their
/// hierarchy).
fn library_class(c: &str) -> bool {
    ["java/", "javax/", "com/google/", "com/mojang/", "net/minecraft/", "net/neoforged/", "io/netty/", "org/apache/"].iter().any(|p| c.starts_with(p))
}

/// The packages whose classes the runtime keeps as its own collections.
fn collection_package(c: &str) -> bool {
    c.starts_with("java/util/") || c.starts_with("com/google/common/collect/") || c.starts_with("it/unimi/dsi/fastutil/")
}

/// A map class (`HashMap`, `ImmutableMap`, fastutil's `Object2IntOpenHashMap`): Guava's multimaps and
/// range maps aren't maps.
fn is_map_class(c: &str) -> bool {
    collection_package(c) && c.ends_with("Map") && !c.ends_with("Multimap") && !c.ends_with("RangeMap") && !c.contains('$')
}

/// A list class (`ArrayList`, `ImmutableList`, fastutil's `ObjectArrayList`, Minecraft's `NonNullList`).
fn is_list_class(c: &str) -> bool {
    (collection_package(c) && c.ends_with("List") && !c.contains('$')) || c == "net/minecraft/core/NonNullList" || c == "java/util/Arrays$ArrayList"
}

/// A set class (`HashSet`, `ImmutableSet`, `EnumSet`): `BitSet`, Guava's multisets and range sets aren't sets.
fn is_set_class(c: &str) -> bool {
    collection_package(c) && c.ends_with("Set") && !matches!(c, "java/util/BitSet") && !c.ends_with("Multiset") && !c.ends_with("RangeSet") && !c.contains('$')
}

/// A queue class of `java.util` (`ArrayDeque`, `PriorityQueue`, `LinkedList`); fastutil's queues aren't Java's.
fn is_queue_class(c: &str) -> bool {
    (c.starts_with("java/util/") && (c.ends_with("Queue") || c.ends_with("Deque"))) || c == "java/util/LinkedList"
}

/// Vanilla's player classes: a server's, the client's own and the others it sees.
/// Vanilla's screens (`TitleScreen`, `ConfirmScreen`, `Screen` itself).
fn is_screen_class(c: &str) -> bool {
    c.starts_with("net/minecraft/client/gui/screens/") && c.ends_with("Screen")
}

/// Vanilla's widgets: `AbstractWidget` and the buttons, boxes and sliders under it.
fn is_widget_class(c: &str) -> bool {
    let Some(name) = c.strip_prefix("net/minecraft/client/gui/components/") else { return false };
    !name.contains('/') && (name.ends_with("Widget") || name.ends_with("Button") || matches!(name, "EditBox" | "Checkbox" | "MultiLineEditBox"))
}

/// Where player object `p` stands and looks (feet, yaw, pitch in degrees): the client's own player
/// as the host places it, a server player as the hook that made it said (`$x`, `$yaw`); None when
/// it isn't known.
fn player_place(rt: &Rt, p: V) -> Option<([f64; 3], f32, f32)> {
    match rt.class_of(p) {
        LOCAL_PLAYER => rt.host()?.player_place(),
        SERVER_PLAYER if matches!(rt.get_field(p, "$x"), V::D(_)) => Some((
            [rt.get_field(p, "$x").d(), rt.get_field(p, "$y").d(), rt.get_field(p, "$z").d()],
            rt.get_field(p, "$yaw").f(),
            rt.get_field(p, "$pitch").f(),
        )),
        _ => None,
    }
}

/// Where entity `e` is: a player's place, the game's entity's, a mod entity's or an item's; None
/// when it isn't known.
pub(crate) fn entity_place(rt: &Rt, e: V) -> Option<[f64; 3]> {
    player_place(rt, e).map(|(p, _, _)| p).or_else(|| crate::vanilla_entities::position(rt, e))
}

/// The player's eye height, vanilla's float (1.62, 1.27 crouching) as `getEyeY` adds it.
pub(crate) fn player_eye_height(rt: &Rt) -> f64 {
    f64::from(if rt.host().is_some_and(|h| h.player_sneaking()) { 1.27f32 } else { 1.62f32 })
}

/// The player's width and height (`Player.STANDING_DIMENSIONS` 0.6 by 1.8, crouching 1.5).
fn player_size(rt: &Rt) -> (f32, f32) {
    (0.6, if rt.host().is_some_and(|h| h.player_sneaking()) { 1.5 } else { 1.8 })
}

/// Vanilla enums whose constants are plain values, one object each (`api_enum`): their statics,
/// `values()` and `valueOf`.
const POSE: &str = "net/minecraft/world/entity/Pose";
const PLAIN_ENUMS: [(&str, &[&str]); 23] = [
    (MONTH, &["JANUARY", "FEBRUARY", "MARCH", "APRIL", "MAY", "JUNE", "JULY", "AUGUST", "SEPTEMBER", "OCTOBER", "NOVEMBER", "DECEMBER"]),
    // FML's config files, in its order: what `ModConfig.Type.values()` and an enum codec give.
    ("net/neoforged/fml/config/ModConfig$Type", &["COMMON", "CLIENT", "SERVER", "STARTUP"]),
    // A creative tab's kind (a mod's tab is a `CATEGORY`).
    ("net/minecraft/world/item/CreativeModeTab$Type", &["CATEGORY", "INVENTORY", "HOTBAR", "SEARCH"]),
    ("net/minecraft/world/phys/HitResult$Type", &["MISS", "BLOCK", "ENTITY"]),
    ("net/minecraft/world/entity/vehicle/AbstractMinecart$Type", &["RIDEABLE", "CHEST", "FURNACE", "TNT", "SPAWNER", "HOPPER", "COMMAND_BLOCK"]),
    ("net/minecraft/world/level/pathfinder/PathComputationType", &["LAND", "WATER", "AIR"]),
    (
        "net/minecraft/world/level/pathfinder/PathType",
        &[
            "BLOCKED", "OPEN", "WALKABLE", "WALKABLE_DOOR", "TRAPDOOR", "POWDER_SNOW", "DANGER_POWDER_SNOW", "FENCE", "LAVA", "WATER", "WATER_BORDER", "RAIL",
            "UNPASSABLE_RAIL", "DANGER_FIRE", "DAMAGE_FIRE", "DANGER_OTHER", "DAMAGE_OTHER", "DOOR_OPEN", "DOOR_WOOD_CLOSED", "DOOR_IRON_CLOSED", "BREACH",
            "LEAVES", "STICKY_HONEY", "COCOA", "DAMAGE_CAUTIOUS", "DANGER_TRAPDOOR",
        ],
    ),
    ("net/minecraft/util/ByIdMap$OutOfBoundsStrategy", &["ZERO", "WRAP", "CLAMP"]),
    // What an entity wears or holds, in `EquipmentSlot`'s order (`getItemBySlot` reads it).
    ("net/minecraft/world/entity/EquipmentSlot", &["MAINHAND", "OFFHAND", "FEET", "LEGS", "CHEST", "HEAD", "BODY"]),
    (
        "net/minecraft/ChatFormatting",
        &[
            "BLACK", "DARK_BLUE", "DARK_GREEN", "DARK_AQUA", "DARK_RED", "DARK_PURPLE", "GOLD", "GRAY", "DARK_GRAY", "BLUE", "GREEN", "AQUA", "RED",
            "LIGHT_PURPLE", "YELLOW", "WHITE", "OBFUSCATED", "BOLD", "STRIKETHROUGH", "UNDERLINE", "ITALIC", "RESET",
        ],
    ),
    (
        CHRONO_UNIT,
        &[
            "NANOS", "MICROS", "MILLIS", "SECONDS", "MINUTES", "HOURS", "HALF_DAYS", "DAYS", "WEEKS", "MONTHS", "YEARS", "DECADES", "CENTURIES", "MILLENNIA",
            "ERAS", "FOREVER",
        ],
    ),
    ("java/math/RoundingMode", &["UP", "DOWN", "CEILING", "FLOOR", "HALF_UP", "HALF_DOWN", "HALF_EVEN", "UNNECESSARY"]),
    // Log4j's levels, most severe first (a class of constants, not an enum, but as plain).
    ("org/apache/logging/log4j/Level", &["OFF", "FATAL", "ERROR", "WARN", "INFO", "DEBUG", "TRACE", "ALL"]),
    ("java/util/concurrent/TimeUnit", &["NANOSECONDS", "MICROSECONDS", "MILLISECONDS", "SECONDS", "MINUTES", "HOURS", "DAYS"]),
    ("java/lang/StackWalker$Option", &["RETAIN_CLASS_REFERENCE", "SHOW_REFLECT_FRAMES", "SHOW_HIDDEN_FRAMES"]),
    ("net/minecraft/world/entity/ai/attributes/AttributeModifier$Operation", &["ADD_VALUE", "ADD_MULTIPLIED_BASE", "ADD_MULTIPLIED_TOTAL"]),
    ("net/minecraft/world/effect/MobEffectCategory", &["BENEFICIAL", "HARMFUL", "NEUTRAL"]),
    (
        "net/minecraft/world/entity/MobCategory",
        &["MONSTER", "CREATURE", "AMBIENT", "AXOLOTLS", "UNDERGROUND_WATER_CREATURE", "WATER_CREATURE", "WATER_AMBIENT", "MISC"],
    ),
    ("net/minecraft/util/Unit", &["INSTANCE"]),
    ("net/minecraft/world/level/block/state/properties/BlockSetType$PressurePlateSensitivity", &["EVERYTHING", "MOBS"]),
    (crate::menus::CLICK_TYPE, &crate::menus::CLICK_TYPES),
    ("net/minecraft/world/inventory/ClickAction", &["PRIMARY", "SECONDARY"]),
    (
        POSE,
        &[
            "STANDING", "FALL_FLYING", "SLEEPING", "SWIMMING", "SPIN_ATTACK", "CROUCHING", "LONG_JUMPING", "DYING", "CROAKING", "USING_TONGUE",
            "SITTING", "ROARING", "SNIFFING", "EMERGING", "DIGGING", "SLIDING", "SHOOTING", "INHALING",
        ],
    ),
];

/// Plain vanilla enum `class`'s constant `name`; None for another class or name.
const MONTH: &str = "java/time/Month";
const CHRONO_UNIT: &str = "java/time/temporal/ChronoUnit";
const LOCAL_TIME: &str = "java/time/LocalTime";
const DURATION: &str = "java/time/Duration";
const NANOS_PER_DAY: i64 = 86_400_000_000_000;
/// `ChronoUnit`'s estimated durations in nanoseconds, in its order (a month is a twelfth of the
/// mean Gregorian year, as Java estimates it; `FOREVER` is `Long.MAX_VALUE` seconds).
const CHRONO_NANOS: [i128; 16] = [
    1,
    1_000,
    1_000_000,
    1_000_000_000,
    60_000_000_000,
    3_600_000_000_000,
    43_200_000_000_000,
    86_400_000_000_000,
    604_800_000_000_000,
    2_629_746_000_000_000,
    31_556_952_000_000_000,
    315_569_520_000_000_000,
    3_155_695_200_000_000_000,
    31_556_952_000_000_000_000,
    31_556_952_000_000_000_000_000_000,
    i64::MAX as i128 * 1_000_000_000 + 999_999_999,
];
const THREAD_POOL: &str = "java/util/concurrent/ThreadPoolExecutor";
const SCHEDULED_POOL: &str = "java/util/concurrent/ScheduledThreadPoolExecutor";
const SCHEDULED_FUTURE: &str = "java/util/concurrent/ScheduledFuture";
const THREAD_FACTORY_BUILDER: &str = "com/google/common/util/concurrent/ThreadFactoryBuilder";
const TICKET_TYPE: &str = "net/minecraft/server/level/TicketType";
const LOCAL_DATE: &str = "java/time/LocalDate";
const BOUNDS_INTS: &str = "net/minecraft/advancements/critereon/MinMaxBounds$Ints";
const BOUNDS_DOUBLES: &str = "net/minecraft/advancements/critereon/MinMaxBounds$Doubles";
const LOOT_PARAMS: &str = "net/minecraft/world/level/storage/loot/parameters/LootContextParams";
const LOOT_PARAM: &str = "net/minecraft/world/level/storage/loot/parameters/LootContextParam";
const LOOT_PARAM_SET: &str = "net/minecraft/world/level/storage/loot/parameters/LootContextParamSet";
const LOOT_PARAM_SET_BUILDER: &str = "net/minecraft/world/level/storage/loot/parameters/LootContextParamSet$Builder";

/// A new `LootContextParamSet.Builder`'s state: the required and the optional parameters.
fn loot_param_builder(rt: &mut Rt, b: V) {
    let (required, optional) = (crate::jutil::new_list(rt, Vec::new()), crate::jutil::new_list(rt, Vec::new()));
    rt.put_field(b, "$required", required);
    rt.put_field(b, "$optional", optional);
}
const STOPWATCH: &str = "com/google/common/base/Stopwatch";
/// `TimeUnit`'s constants (ordinal order) in nanoseconds.
const TIME_UNIT_NANOS: [i128; 7] = [1, 1_000, 1_000_000, 1_000_000_000, 60_000_000_000, 3_600_000_000_000, 86_400_000_000_000];

/// `MinMaxBounds.Ints(min, max)` or `Doubles`' (`class`): the record with the squares its
/// constructor keeps (an int's as a long).
fn bounds(rt: &mut Rt, class: &str, min: Option<V>, max: Option<V>) -> V {
    let ints = class == BOUNDS_INTS;
    let value = |v: V| if ints { V::I(v.i()) } else { V::D(v.d()) };
    let square = |v: V| if ints { V::J(i64::from(v.i()) * i64::from(v.i())) } else { V::D(v.d() * v.d()) };
    let parts: Vec<V> = [min.map(value), max.map(value), min.map(square), max.map(square)]
        .into_iter()
        .map(|p| optional(rt, p.unwrap_or(V::NULL)))
        .collect();
    crate::records::make(rt, class, &parts)
}

/// The proleptic Gregorian date `days` after 1970-01-01, as `(year, month, day)` (`LocalDate`'s
/// `ofEpochDay`).
fn civil_date(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year as i32, month, day)
}

fn local_date(rt: &mut Rt, (year, month, day): (i32, u32, u32)) -> V {
    let d = rt.new_object(LOCAL_DATE);
    rt.put_field(d, "year", V::I(year));
    rt.put_field(d, "month", V::I(month as i32));
    rt.put_field(d, "day", V::I(day as i32));
    d
}

/// Now, in nanoseconds since 1970 (`Stopwatch`'s ticker).
fn now_nanos() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i64)
}

/// A `LocalTime` `nanos` into the day.
fn local_time(rt: &mut Rt, nanos: i64) -> V {
    let t = rt.new_object(LOCAL_TIME);
    rt.put_field(t, "$nanoOfDay", V::J(nanos.rem_euclid(NANOS_PER_DAY)));
    t
}

/// A `Duration` of `nanos` (seconds and the nanoseconds past them, as Java keeps it).
fn duration(rt: &mut Rt, nanos: i128) -> V {
    let d = rt.new_object(DURATION);
    rt.put_field(d, "$seconds", V::J(nanos.div_euclid(1_000_000_000) as i64));
    rt.put_field(d, "$nanos", V::I(nanos.rem_euclid(1_000_000_000) as i32));
    d
}

fn duration_nanos(rt: &Rt, d: V) -> i128 {
    i128::from(rt.get_field(d, "$seconds").j()) * 1_000_000_000 + i128::from(rt.get_field(d, "$nanos").i())
}

/// An executor service: one runtime runs one thread, so what it is given to run runs at once.
fn executor(rt: &mut Rt, class: &'static str) -> V {
    rt.new_object(class)
}

fn is_executor(class: &str) -> bool {
    matches!(
        class,
        "java/util/concurrent/Executor"
            | "java/util/concurrent/ExecutorService"
            | "java/util/concurrent/ScheduledExecutorService"
            | "java/util/concurrent/AbstractExecutorService"
            | THREAD_POOL
            | SCHEDULED_POOL
    )
}

/// `d` of `TimeUnit` `from` (an ordinal) in `to`, saturating as `TimeUnit.convert` does.
fn time_convert(d: i64, from: usize, to: usize) -> i64 {
    let n = i128::from(d) * TIME_UNIT_NANOS[from.min(6)] / TIME_UNIT_NANOS[to.min(6)];
    n.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

/// The constants of a vanilla enum the runtime makes (its own table's, else the records'), in
/// ordinal order, with the class name as the runtime keeps it.
pub(crate) fn enum_names(class: &str) -> Option<(&'static str, Vec<&'static str>)> {
    match PLAIN_ENUMS.iter().find(|(c, _)| *c == class) {
        Some((c, names)) => Some((c, names.to_vec())),
        None => crate::records::enum_names(class),
    }
}

pub(crate) fn plain_enum(rt: &mut Rt, class: &str, name: &str) -> Option<V> {
    let (class, names) = PLAIN_ENUMS.iter().find(|(c, _)| *c == class)?;
    let i = names.iter().position(|n| *n == name)?;
    Some(crate::api::api_enum(rt, class, names[i], i as i32))
}

/// `values()` of a plain vanilla enum, or of `InteractionHand` (its constants are the ones its
/// statics give).
fn plain_enum_values(rt: &mut Rt, class: &str) -> Option<V> {
    let names: Vec<&'static str> = match PLAIN_ENUMS.iter().find(|(c, _)| *c == class) {
        Some((_, names)) => names.to_vec(),
        None if class == "net/minecraft/world/InteractionHand" => vec!["MAIN_HAND", "OFF_HAND"],
        None if !result_names(class).is_empty() => result_names(class).to_vec(),
        None => return None,
    };
    let values: Vec<V> = names.iter().filter_map(|n| get_static(rt, class, n)).collect();
    let arr = rt.new_array(values.len() as i32, V::NULL);
    for (i, v) in values.into_iter().enumerate() {
        rt.array_set(arr, i as i32, v);
    }
    Some(arr)
}

const BLOCK_ITEM_GET_BLOCK: (&str, &str) = (
    "net/minecraft/world/item/BlockItem.getBlock.()Lnet/minecraft/world/level/block/Block;",
    "getBlock.()Lnet/minecraft/world/level/block/Block;",
);
const STATE_FOR_PLACEMENT: (&str, &str) = (
    "net/minecraft/world/level/block/Block.getStateForPlacement.(Lnet/minecraft/world/item/context/BlockPlaceContext;)Lnet/minecraft/world/level/block/state/BlockState;",
    "getStateForPlacement.(Lnet/minecraft/world/item/context/BlockPlaceContext;)Lnet/minecraft/world/level/block/state/BlockState;",
);
const BLOCK_ITEM_CAN_PLACE: (&str, &str) = (
    "net/minecraft/world/item/BlockItem.canPlace.(Lnet/minecraft/world/item/context/BlockPlaceContext;Lnet/minecraft/world/level/block/state/BlockState;)Z",
    "canPlace.(Lnet/minecraft/world/item/context/BlockPlaceContext;Lnet/minecraft/world/level/block/state/BlockState;)Z",
);
const BLOCK_ITEM_MUST_SURVIVE: (&str, &str) = (
    "net/minecraft/world/item/BlockItem.mustSurvive.()Z",
    "mustSurvive.()Z",
);
const STATE_CAN_SURVIVE: &str = "net/minecraft/world/level/block/state/BlockBehaviour$BlockStateBase.canSurvive.(Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;)Z";

/// Whether `v` is a player object: the game's (a server's player, the client's own, a fake one)
/// or a mod's subclass of `Player`.
fn player_object(rt: &Rt, v: V) -> bool {
    v.r() != 0
        && (is_player_class(rt.class_of(v))
            || rt.instance_of(v, "net/minecraft/world/entity/player/Player"))
}

pub(crate) fn is_player_class(c: &str) -> bool {
    matches!(
        c,
        "net/minecraft/server/level/ServerPlayer"
            | "net/minecraft/client/player/LocalPlayer"
            | "net/minecraft/client/player/AbstractClientPlayer"
            | "net/minecraft/client/player/RemotePlayer"
            | crate::fake_players::FAKE_PLAYER
    )
}

pub fn direction(rt: &mut Rt, ordinal: i64) -> V {
    let o = ordinal.clamp(0, 5);
    rt.intern(format!("dir/{o}"), |rt| {
        rt.alloc("net/minecraft/core/Direction", Data::Native("Direction", [o, 0, 0, 0]))
    })
}

/// A `Direction` object's data value (0 down, 1 up, 2 north, 3 south, 4 west, 5 east).
pub fn direction_ordinal(rt: &Rt, d: V) -> u8 {
    match rt.heap.get(d.r() as usize).map(|o| &o.data) {
        Some(Data::Native("Direction", h)) => h[0].clamp(0, 5) as u8,
        _ => 0,
    }
}

pub fn axis(rt: &mut Rt, ordinal: i64) -> V {
    let o = ordinal.clamp(0, 2);
    rt.intern(format!("axis/{o}"), |rt| {
        rt.alloc("net/minecraft/core/Direction$Axis", Data::Native("Axis", [o, 0, 0, 0]))
    })
}

/// `Enum.name()` (or `toString()`, `to_string`) of a constant: a mod's own and the ones made by
/// their constructor keep their name; vanilla's directions, axes and planes, kept as natives,
/// answer vanilla's: `NORTH`, `X`, and as `toString` the name each was built with (`north`, `x`,
/// `Towards positive`), as 26.3's classes override it.
pub(crate) fn enum_name(rt: &mut Rt, v: V, to_string: bool) -> V {
    let named = rt.get_field(v, "$name");
    if !matches!(named, V::Void) {
        return named;
    }
    let o = native(rt, v)[0].max(0) as usize;
    let text = match (rt.class_of(v), to_string) {
        ("net/minecraft/core/Direction", false) => DIRECTION_NAMES[o.min(5)].to_ascii_uppercase(),
        ("net/minecraft/core/Direction", true) => DIRECTION_NAMES[o.min(5)].to_owned(),
        ("net/minecraft/core/Direction$Axis", false) => AXIS_NAMES[o.min(2)].to_ascii_uppercase(),
        ("net/minecraft/core/Direction$Axis", true) => AXIS_NAMES[o.min(2)].to_owned(),
        // Kept as 1 for positive, 0 for negative.
        ("net/minecraft/core/Direction$AxisDirection", false) => ["NEGATIVE", "POSITIVE"][o.min(1)].to_owned(),
        ("net/minecraft/core/Direction$AxisDirection", true) => ["Towards negative", "Towards positive"][o.min(1)].to_owned(),
        ("net/minecraft/core/Direction$Plane", _) => ["HORIZONTAL", "VERTICAL"][o.min(1)].to_owned(),
        _ => return named,
    };
    rt.string(&text)
}

/// `Enum.ordinal()`: a direction's is its data value (`DOWN` 0 ... `EAST` 5), an axis's `X` 0 to `Z` 2.
pub(crate) fn enum_ordinal(rt: &Rt, v: V) -> i32 {
    match rt.get_field(v, "$ordinal") {
        V::Void if matches!(rt.class_of(v), "net/minecraft/core/Direction" | "net/minecraft/core/Direction$Axis") => native(rt, v)[0] as i32,
        o => o.i(),
    }
}

pub fn boolean(rt: &mut Rt, b: bool) -> V {
    rt.intern(format!("bool/{b}"), |rt| {
        let o = rt.new_object("java/lang/Boolean");
        rt.put_field(o, "value", V::I(b as i32));
        o
    })
}

pub fn integer(rt: &mut Rt, i: i32) -> V {
    let o = rt.new_object("java/lang/Integer");
    rt.put_field(o, "value", V::I(i));
    o
}

pub fn state(rt: &mut Rt, text: &str) -> V {
    let key = format!("state/{text}");
    if let Some(v) = rt.interned.get(&key) {
        return *v;
    }
    let v = rt.alloc("net/minecraft/world/level/block/state/BlockState", Data::Str(text.to_owned()));
    rt.interned.insert(key, v);
    if text.starts_with(crate::api::UNREGISTERED) {
        crate::api::note_unregistered(rt, text, v);
    }
    v
}

pub fn item(rt: &mut Rt, name: &str) -> V {
    let name = if name.contains(':') { name.to_owned() } else { format!("minecraft:{name}") };
    if let Some(v) = rt.registered.get(&("item".to_owned(), name.clone())) {
        return *v;
    }
    // A vanilla block's item is a `BlockItem` where the game says so (a mod checks `instanceof
    // BlockItem`: Create's deployer won't place against a block it can't replace); without a host to
    // ask, an `Item`.
    rt.intern(format!("item/{name}"), move |rt| {
        let class = if rt.host().is_some_and(|h| h.item_is_block(&name)) { "net/minecraft/world/item/BlockItem" } else { "net/minecraft/world/item/Item" };
        rt.alloc(class, Data::Str(name))
    })
}

fn native(rt: &Rt, v: V) -> [i64; 4] {
    match rt.heap.get(v.r() as usize).map(|o| &o.data) {
        Some(Data::Native(_, h)) => *h,
        _ => [0; 4],
    }
}

pub(crate) fn name_of(rt: &Rt, v: V) -> String {
    rt.str_of(v).map(str::to_owned).unwrap_or_default()
}

/// The name of an API enum's constant (`ChatFormatting.RED` is `RED`), empty for anything else.
fn constant(rt: &Rt, v: V) -> String {
    if v.r() == 0 {
        return String::new();
    }
    rt.str_of(rt.get_field(v, "$name")).map(str::to_owned).unwrap_or_default()
}

const PROPERTIES: &str = "net/minecraft/world/level/block/state/properties/";
const PROPERTY: &str = "net/minecraft/world/level/block/state/properties/Property";

/// A block-state property: its name and kind (`bool`, `int`, `direction`, `axis`, or an enum's class).
fn new_property(rt: &mut Rt, name: &str, kind: &str) -> V {
    let p = rt.new_object(PROPERTY);
    let n = rt.string(name);
    let k = rt.string(kind);
    rt.put_field(p, "$name", n);
    rt.put_field(p, "$kind", k);
    p
}

/// Vanilla's `BlockStateProperties` fields a state's property may be, in the order a name several
/// share is matched by (`FACING` before `HORIZONTAL_FACING`).
const PROPERTY_FIELDS: [&str; 111] = [
    "FACING", "HORIZONTAL_FACING", "FACING_HOPPER", "VERTICAL_DIRECTION", "AXIS", "HORIZONTAL_AXIS", "AGE_1", "AGE_2", "AGE_3", "AGE_4",
    "AGE_5", "AGE_7", "AGE_15", "AGE_25", "POWER", "LEVEL", "LEVEL_CAULDRON", "LEVEL_COMPOSTER", "LEVEL_FLOWING", "LEVEL_HONEY",
    "DISTANCE", "MOISTURE", "ROTATION_16", "STAGE", "DELAY", "NOTE", "LAYERS", "BITES", "PICKLES", "EGGS", "HATCH", "CANDLES",
    "RESPAWN_ANCHOR_CHARGES", "STABILITY_DISTANCE", "DUSTED", "FLOWER_AMOUNT", "SEGMENT_AMOUNT", "WATERLOGGED", "POWERED", "LIT", "OPEN",
    "OCCUPIED", "ATTACHED", "DISARMED", "EXTENDED", "ENABLED", "HANGING", "PERSISTENT", "SNOWY", "TRIGGERED", "UNSTABLE", "UP", "DOWN",
    "NORTH", "SOUTH", "EAST", "WEST", "BOTTOM", "CONDITIONAL", "DRAG", "HAS_BOOK", "IN_WALL", "LOCKED", "SHORT", "SIGNAL_FIRE", "INVERTED",
    "BERRIES", "BLOOM", "SHRIEKING", "CAN_SUMMON", "CRACKED", "NATURAL", "OMINOUS", "TIP", "EYE", "HAS_RECORD", "IN_WATER", "FALLING",
    "HAS_BOTTLE_0", "HAS_BOTTLE_1", "HAS_BOTTLE_2", "CRAFTING", "MAP", "HALF", "DOUBLE_BLOCK_HALF", "DOOR_HINGE", "SLAB_TYPE",
    "CHEST_TYPE", "PISTON_TYPE", "STAIRS_SHAPE", "RAIL_SHAPE", "RAIL_SHAPE_STRAIGHT", "ATTACH_FACE", "BED_PART", "MODE_COMPARATOR",
    "BELL_ATTACHMENT", "TILT", "NORTH_WALL", "EAST_WALL", "SOUTH_WALL", "WEST_WALL", "NORTH_REDSTONE", "EAST_REDSTONE", "SOUTH_REDSTONE",
    "WEST_REDSTONE", "SLOT_0_OCCUPIED", "SLOT_1_OCCUPIED", "SLOT_2_OCCUPIED", "SLOT_3_OCCUPIED", "SLOT_4_OCCUPIED", "SLOT_5_OCCUPIED",
];

/// The vanilla property a state spelling `name=value` holds: the first `BlockStateProperties`
/// field of that name taking that value; else one of that name whose kind the value spells.
fn vanilla_property_named(rt: &mut Rt, name: &str, value: &str) -> V {
    for field in PROPERTY_FIELDS {
        if vanilla_property(field).is_some_and(|(n, _)| n == name)
            && let Some(p) = vanilla_property_object(rt, field)
            && crate::blocks::property_values(rt, p).iter().any(|v| v == value)
        {
            return p;
        }
    }
    let kind = match value {
        "true" | "false" => "bool",
        v if !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()) => "int",
        _ => "string",
    };
    let (n, k) = (name.to_owned(), kind.to_owned());
    rt.intern(format!("prop/named/{name}/{kind}"), move |rt| new_property(rt, &n, &k))
}

/// The properties of block state `state`, in its definition's order (by name): a mod block's own
/// objects, else vanilla's `BlockStateProperties` of each name and value.
pub(crate) fn state_properties(rt: &mut Rt, state: V) -> Vec<V> {
    let text = name_of(rt, state);
    let (block, props) = crate::host::split_state(&text);
    let own = rt
        .registered
        .get(&("block".to_owned(), block.to_owned()))
        .map(|b| rt.get_field(*b, "$state-props"))
        .filter(|l| l.r() != 0);
    match own {
        Some(list) => crate::jutil::elements(rt, list)
            .into_iter()
            .filter(|p| props.iter().any(|(k, _)| *k == prop_parts(rt, *p).0))
            .collect(),
        None => props
            .iter()
            .map(|(k, v)| vanilla_property_named(rt, k, v))
            .collect(),
    }
}

/// Vanilla's property in `BlockStateProperties.<field>`: one object per field, as Java's (`FACING`
/// and `HORIZONTAL_FACING` share a name, not their values).
pub(crate) fn vanilla_property_object(rt: &mut Rt, field: &str) -> Option<V> {
    let (name, kind) = vanilla_property(field)?;
    let f = field.to_owned();
    Some(rt.intern(format!("prop/field/{field}"), move |rt| {
        let p = new_property(rt, &name, kind);
        let fv = rt.string(&f);
        rt.put_field(p, "$field", fv);
        p
    }))
}

/// The `BlockStateProperties` field that vanilla block class `class`'s property field `field` holds,
/// its own or inherited: most keep the name (`DoorBlock.OPEN` is `OPEN`); these don't. Read from
/// 26.3's block classes (`LayeredCauldronBlock.LEVEL` is `LEVEL_CAULDRON`, 1 to 3).
fn block_class_property<'a>(class: &str, field: &'a str) -> &'a str {
    let simple = class.rsplit('/').next().unwrap_or(class);
    // Classes whose `FACING` is `HORIZONTAL_FACING` (`DirectionalBlock`'s and the rest are `FACING`).
    const HORIZONTAL: [&str; 65] = [
        "AbstractBedBlock", "AbstractFurnaceBlock", "AnvilBlock", "AttachedStemBlock", "BaseCoralWallFanBlock", "BedBlock",
        "BeehiveBlock", "BellBlock", "BigDripleafBlock", "BigDripleafStemBlock", "BlastFurnaceBlock", "ButtonBlock",
        "CalibratedSculkSensorBlock", "CampfireBlock", "CarvedPumpkinBlock", "ChestBlock", "ChiseledBookShelfBlock", "CocoaBlock",
        "ComparatorBlock", "CopperChestBlock", "CopperGolemStatueBlock", "CoralWallFanBlock", "DiodeBlock", "DoorBlock",
        "DriedGhastBlock", "EndPortalFrameBlock", "EnderChestBlock", "FaceAttachedHorizontalDirectionalBlock", "FenceGateBlock",
        "FlowerBedBlock", "FurnaceBlock", "GlazedTerracottaBlock", "GrindstoneBlock", "HorizontalDirectionalBlock", "LadderBlock",
        "LeafLitterBlock", "LecternBlock", "LeverBlock", "LoomBlock", "PiglinWallSkullBlock", "PlayerWallHeadBlock",
        "RedstoneWallTorchBlock", "RepeaterBlock", "ShelfBlock", "ShelfMushroomBlock", "SmallDripleafBlock", "SmokerBlock",
        "StairBlock", "StonecutterBlock", "StrawBedBlock", "TrapDoorBlock", "TrappedChestBlock", "TripWireHookBlock",
        "VaultBlock", "WallBannerBlock", "WallHangingSignBlock", "WallSignBlock", "WallSkullBlock", "WallTorchBlock",
        "WeatheringCopperChestBlock", "WeatheringCopperDoorBlock", "WeatheringCopperGolemStatueBlock",
        "WeatheringCopperStairBlock", "WeatheringCopperTrapDoorBlock", "WitherWallSkullBlock",
    ];
    const DRIPSTONE: [&str; 3] = ["PointedDripstoneBlock", "SpeleothemBlock", "SulfurSpikeBlock"];
    match (simple, field) {
        ("BambooStalkBlock" | "ShelfMushroomBlock" | "TorchflowerCropBlock", "AGE") => "AGE_1",
        ("CocoaBlock", "AGE") => "AGE_2",
        ("BeetrootBlock" | "FrostedIceBlock" | "NetherWartBlock" | "SweetBerryBushBlock", "AGE") => "AGE_3",
        ("MangrovePropaguleBlock" | "PitcherCropBlock", "AGE") => "AGE_4",
        ("ChorusFlowerBlock", "AGE") => "AGE_5",
        ("CarrotBlock" | "CropBlock" | "PotatoBlock" | "StemBlock", "AGE") => "AGE_7",
        ("CactusBlock" | "FireBlock" | "SugarCaneBlock", "AGE") => "AGE_15",
        ("CaveVinesBlock" | "GrowingPlantHeadBlock" | "KelpBlock" | "TwistingVinesBlock" | "WeepingVinesBlock", "AGE") => "AGE_25",
        ("FlowerBedBlock", "AMOUNT") => "FLOWER_AMOUNT",
        ("SegmentableBlock", "AMOUNT") => "SEGMENT_AMOUNT",
        ("BellBlock", "ATTACHMENT") => "BELL_ATTACHMENT",
        ("NetherPortalBlock", "AXIS") => "HORIZONTAL_AXIS",
        ("MossyCarpetBlock", "BASE") => "BOTTOM",
        ("RespawnAnchorBlock", "CHARGE") => "RESPAWN_ANCHOR_CHARGES",
        ("ScaffoldingBlock", "DISTANCE") => "STABILITY_DISTANCE",
        ("BubbleColumnBlock", "DRAG_DOWN") => "DRAG",
        ("RedstoneWireBlock", "NORTH") => "NORTH_REDSTONE",
        ("RedstoneWireBlock", "EAST") => "EAST_REDSTONE",
        ("RedstoneWireBlock", "SOUTH") => "SOUTH_REDSTONE",
        ("RedstoneWireBlock", "WEST") => "WEST_REDSTONE",
        ("MossyCarpetBlock" | "WallBlock", "NORTH") => "NORTH_WALL",
        ("MossyCarpetBlock" | "WallBlock", "EAST") => "EAST_WALL",
        ("MossyCarpetBlock" | "WallBlock", "SOUTH") => "SOUTH_WALL",
        ("MossyCarpetBlock" | "WallBlock", "WEST") => "WEST_WALL",
        ("ButtonBlock" | "FaceAttachedHorizontalDirectionalBlock" | "GrindstoneBlock" | "LeverBlock", "FACE") => "ATTACH_FACE",
        ("HopperBlock", "FACING") => "FACING_HOPPER",
        (s, "FACING") if HORIZONTAL.contains(&s) => "HORIZONTAL_FACING",
        (
            "DoorBlock" | "DoublePlantBlock" | "PitcherCropBlock" | "SmallDripleafBlock" | "TallFlowerBlock" | "TallSeagrassBlock"
            | "WeatheringCopperDoorBlock",
            "HALF",
        ) => "DOUBLE_BLOCK_HALF",
        ("EndPortalFrameBlock", "HAS_EYE") => "EYE",
        ("DoorBlock" | "WeatheringCopperDoorBlock", "HINGE") => "DOOR_HINGE",
        ("BeehiveBlock", "HONEY_LEVEL") => "LEVEL_HONEY",
        ("DriedGhastBlock", "HYDRATION_LEVEL") => "DRIED_GHAST_HYDRATION_LEVELS",
        ("NoteBlock", "INSTRUMENT") => "NOTEBLOCK_INSTRUMENT",
        ("BambooStalkBlock", "LEAVES") => "BAMBOO_LEAVES",
        ("LayeredCauldronBlock", "LEVEL") => "LEVEL_CAULDRON",
        ("ComposterBlock", "LEVEL") => "LEVEL_COMPOSTER",
        ("ComparatorBlock", "MODE") => "MODE_COMPARATOR",
        ("StructureBlock", "MODE") => "STRUCTUREBLOCK_MODE",
        ("TestBlock", "MODE") => "TEST_BLOCK_MODE",
        ("TargetBlock", "OUTPUT_POWER") => "POWER",
        ("AbstractBedBlock" | "BedBlock" | "StrawBedBlock", "PART") => "BED_PART",
        ("CalibratedSculkSensorBlock" | "SculkSensorBlock", "PHASE") => "SCULK_SENSOR_PHASE",
        ("CopperGolemStatueBlock" | "WeatheringCopperGolemStatueBlock", "POSE") => "COPPER_GOLEM_POSE",
        ("SculkCatalystBlock", "PULSE") => "BLOOM",
        (
            "BannerBlock" | "CeilingHangingSignBlock" | "PlayerHeadBlock" | "SkullBlock" | "StandingSignBlock" | "WitherSkullBlock",
            "ROTATION",
        ) => "ROTATION_16",
        ("RailBlock", "SHAPE") => "RAIL_SHAPE",
        ("DetectorRailBlock" | "PoweredRailBlock", "SHAPE") => "RAIL_SHAPE_STRAIGHT",
        ("StairBlock" | "WeatheringCopperStairBlock", "SHAPE") => "STAIRS_SHAPE",
        ("CreakingHeartBlock", "STATE") => "CREAKING_HEART_STATE",
        ("PotentSulfurBlock", "STATE") => "POTENT_SULFUR_STATE",
        ("TrialSpawnerBlock", "STATE") => "TRIAL_SPAWNER_STATE",
        ("VaultBlock", "STATE") => "VAULT_STATE",
        (s, "THICKNESS") if DRIPSTONE.contains(&s) => "SPELEOTHEM_THICKNESS",
        (s, "TIP_DIRECTION") if DRIPSTONE.contains(&s) => "VERTICAL_DIRECTION",
        ("ChestBlock" | "CopperChestBlock" | "TrappedChestBlock" | "WeatheringCopperChestBlock", "TYPE") => "CHEST_TYPE",
        ("MovingPistonBlock" | "PistonHeadBlock", "TYPE") => "PISTON_TYPE",
        ("SlabBlock" | "WeatheringCopperSlabBlock", "TYPE") => "SLAB_TYPE",
        _ => field,
    }
}

/// Whether `BlockStateProperties` has field `f`: the enum-valued ones by name, the rest by kind
/// (`Blocks.ANDESITE` or `SoundType.METAL` are not properties).
fn state_property_field(f: &str) -> bool {
    vanilla_property(f).is_some_and(|(_, kind)| kind != "string")
        || matches!(
            f,
            "HALF" | "DOUBLE_BLOCK_HALF" | "DOOR_HINGE" | "SLAB_TYPE" | "CHEST_TYPE" | "PISTON_TYPE" | "STAIRS_SHAPE"
                | "RAIL_SHAPE" | "RAIL_SHAPE_STRAIGHT" | "ATTACH_FACE" | "BED_PART" | "MODE_COMPARATOR"
                | "STRUCTUREBLOCK_MODE" | "BELL_ATTACHMENT" | "NOTEBLOCK_INSTRUMENT" | "SPELEOTHEM_THICKNESS"
                | "BAMBOO_LEAVES" | "NORTH_WALL" | "EAST_WALL" | "SOUTH_WALL" | "WEST_WALL" | "NORTH_REDSTONE"
                | "EAST_REDSTONE" | "SOUTH_REDSTONE" | "WEST_REDSTONE" | "SCULK_SENSOR_PHASE" | "TILT" | "ORIENTATION"
                | "SIDE_CHAIN_PART" | "TRIAL_SPAWNER_STATE" | "VAULT_STATE" | "CREAKING_HEART_STATE" | "TEST_BLOCK_MODE"
                | "COPPER_GOLEM_POSE" | "POTENT_SULFUR_STATE"
        )
}

/// The vanilla enum whose constants a `BlockStateProperties` field's values are (`BELL_ATTACHMENT`:
/// `BellAttachType`), for the enums the runtime knows the values of.
fn vanilla_property_enum(field: &str) -> Option<&'static str> {
    Some(match field {
        "HALF" => "net/minecraft/world/level/block/state/properties/Half",
        "DOUBLE_BLOCK_HALF" => "net/minecraft/world/level/block/state/properties/DoubleBlockHalf",
        "DOOR_HINGE" => "net/minecraft/world/level/block/state/properties/DoorHingeSide",
        "SLAB_TYPE" => "net/minecraft/world/level/block/state/properties/SlabType",
        "CHEST_TYPE" => "net/minecraft/world/level/block/state/properties/ChestType",
        "PISTON_TYPE" => "net/minecraft/world/level/block/state/properties/PistonType",
        "STAIRS_SHAPE" => "net/minecraft/world/level/block/state/properties/StairsShape",
        "RAIL_SHAPE" | "RAIL_SHAPE_STRAIGHT" => "net/minecraft/world/level/block/state/properties/RailShape",
        "ATTACH_FACE" => "net/minecraft/world/level/block/state/properties/AttachFace",
        "BED_PART" => "net/minecraft/world/level/block/state/properties/BedPart",
        "MODE_COMPARATOR" => "net/minecraft/world/level/block/state/properties/ComparatorMode",
        "BELL_ATTACHMENT" => "net/minecraft/world/level/block/state/properties/BellAttachType",
        "TILT" => "net/minecraft/world/level/block/state/properties/Tilt",
        "NORTH_WALL" | "EAST_WALL" | "SOUTH_WALL" | "WEST_WALL" => "net/minecraft/world/level/block/state/properties/WallSide",
        "NORTH_REDSTONE" | "EAST_REDSTONE" | "SOUTH_REDSTONE" | "WEST_REDSTONE" => "net/minecraft/world/level/block/state/properties/RedstoneSide",
        _ => return None,
    })
}

/// Vanilla's `BlockStateProperties` fields: (field, property name, kind). An enum-valued property's
/// kind is its enum's class.
pub fn vanilla_property(field: &str) -> Option<(String, &'static str)> {
    let kind = match field {
        f if vanilla_property_enum(f).is_some() => vanilla_property_enum(f).unwrap_or("string"),
        "FACING" | "HORIZONTAL_FACING" | "FACING_HOPPER" | "VERTICAL_DIRECTION" => "direction",
        "AXIS" | "HORIZONTAL_AXIS" => "axis",
        "AGE_1" | "AGE_2" | "AGE_3" | "AGE_4" | "AGE_5" | "AGE_7" | "AGE_15" | "AGE_25" | "POWER" | "LEVEL"
        | "LEVEL_CAULDRON" | "LEVEL_COMPOSTER" | "LEVEL_FLOWING" | "LEVEL_HONEY" | "DISTANCE" | "MOISTURE"
        | "ROTATION_16" | "STAGE" | "DELAY" | "NOTE" | "LAYERS" | "BITES" | "PICKLES" | "EGGS" | "HATCH"
        | "CANDLES" | "RESPAWN_ANCHOR_CHARGES" | "STABILITY_DISTANCE" | "DUSTED" | "FLOWER_AMOUNT"
        | "SEGMENT_AMOUNT" | "DRIED_GHAST_HYDRATION_LEVELS" => "int",
        "WATERLOGGED" | "POWERED" | "LIT" | "OPEN" | "OCCUPIED" | "ATTACHED" | "DISARMED" | "EXTENDED"
        | "ENABLED" | "HANGING" | "PERSISTENT" | "SNOWY" | "TRIGGERED" | "UNSTABLE" | "UP" | "DOWN" | "NORTH"
        | "SOUTH" | "EAST" | "WEST" | "BOTTOM" | "CONDITIONAL" | "DRAG" | "HAS_BOOK" | "IN_WALL" | "LOCKED"
        | "SHORT" | "SIGNAL_FIRE" | "INVERTED" | "BERRIES" | "BLOOM" | "SHRIEKING" | "CAN_SUMMON" | "CRACKED"
        | "NATURAL" | "OMINOUS" | "TIP" | "EYE" | "HAS_RECORD" | "IN_WATER" | "ENABLED_POWER" | "FALLING" | "HAS_BOTTLE_0"
        | "HAS_BOTTLE_1" | "HAS_BOTTLE_2" | "SLOT_0_OCCUPIED" | "SLOT_1_OCCUPIED" | "SLOT_2_OCCUPIED" | "SLOT_3_OCCUPIED"
        | "SLOT_4_OCCUPIED" | "SLOT_5_OCCUPIED" | "CRAFTING" | "MAP" => "bool",
        // 1.21.1's names of the bookshelf's slots (`CHISELED_BOOKSHELF_SLOT_0_OCCUPIED`).
        f if f.starts_with("CHISELED_BOOKSHELF_SLOT_") => "bool",
        _ => "string",
    };
    // The property's own name where it isn't the field's (`DOOR_HINGE` is `hinge`).
    let name = match field {
        "DOUBLE_BLOCK_HALF" => "half".to_owned(),
        "DOOR_HINGE" => "hinge".to_owned(),
        "SLAB_TYPE" | "CHEST_TYPE" | "PISTON_TYPE" => "type".to_owned(),
        "STAIRS_SHAPE" | "RAIL_SHAPE" | "RAIL_SHAPE_STRAIGHT" => "shape".to_owned(),
        "ATTACH_FACE" => "face".to_owned(),
        "BED_PART" => "part".to_owned(),
        "MODE_COMPARATOR" | "STRUCTUREBLOCK_MODE" | "TEST_BLOCK_MODE" => "mode".to_owned(),
        "BELL_ATTACHMENT" => "attachment".to_owned(),
        "NOTEBLOCK_INSTRUMENT" => "instrument".to_owned(),
        "SPELEOTHEM_THICKNESS" => "thickness".to_owned(),
        "SIDE_CHAIN_PART" => "side_chain".to_owned(),
        "DRIED_GHAST_HYDRATION_LEVELS" => "hydration".to_owned(),
        "LEVEL_HONEY" => "honey_level".to_owned(),
        "BAMBOO_LEAVES" => "leaves".to_owned(),
        "NORTH_WALL" | "NORTH_REDSTONE" => "north".to_owned(),
        "EAST_WALL" | "EAST_REDSTONE" => "east".to_owned(),
        "SOUTH_WALL" | "SOUTH_REDSTONE" => "south".to_owned(),
        "WEST_WALL" | "WEST_REDSTONE" => "west".to_owned(),
        "HORIZONTAL_FACING" | "FACING_HOPPER" => "facing".to_owned(),
        "HORIZONTAL_AXIS" => "axis".to_owned(),
        "VERTICAL_DIRECTION" => "vertical_direction".to_owned(),
        f if f.starts_with("AGE_") => "age".to_owned(),
        "LEVEL_CAULDRON" | "LEVEL_COMPOSTER" | "LEVEL_FLOWING" => "level".to_owned(),
        "ROTATION_16" => "rotation".to_owned(),
        "RESPAWN_ANCHOR_CHARGES" => "charges".to_owned(),
        "STABILITY_DISTANCE" => "distance".to_owned(),
        f if f.starts_with("CHISELED_BOOKSHELF_") => f["CHISELED_BOOKSHELF_".len()..].to_ascii_lowercase(),
        f => f.to_ascii_lowercase(),
    };
    Some((name, kind))
}

/// The value of property `p` (of `kind`) that a state spelling `text` holds: by the property's own
/// `getValue` when a mod's class has one.
fn prop_value(rt: &mut Rt, p: V, kind: &str, text: &str) -> V {
    match crate::blocks::parsed_value(rt, p, text) {
        Some(v) => v,
        None => value_of(rt, kind, text),
    }
}

/// A property value as Java sees it, from its text in a state.
fn value_of(rt: &mut Rt, kind: &str, text: &str) -> V {
    match kind {
        "bool" => boolean(rt, text == "true"),
        "int" => integer(rt, text.parse().unwrap_or(0)),
        "direction" => match DIRECTION_NAMES.iter().position(|d| *d == text) {
            Some(o) => direction(rt, o as i64),
            None => V::NULL,
        },
        "axis" => match AXIS_NAMES.iter().position(|a| *a == text) {
            Some(o) => axis(rt, o as i64),
            None => V::NULL,
        },
        class => {
            // A vanilla enum's constant (`BellAttachType.FLOOR` for `floor`), one object each.
            if let Some(v) = crate::blocks::vanilla_enum_constant(rt, class, text) {
                return v;
            }
            // A mod enum: the constant whose name, lower-cased, is the text.
            let prefix = format!("{class}.");
            let found = rt
                .statics
                .iter()
                .filter(|(k, _)| k.starts_with(&prefix))
                .map(|(_, v)| *v)
                .find(|v| rt.str_of(rt.get_field(*v, "$name")).is_some_and(|n| n.eq_ignore_ascii_case(text)));
            match found {
                Some(v) => v,
                None => rt.string(text),
            }
        }
    }
}

/// What the acting player looks at, as `Item.getPlayerPOVHitResult` finds it (`fluids` 1: sources
/// too, 2: any fluid): the block within reach, else a miss where their reach ends
/// (`BlockHitResult.miss`). The client's `Minecraft.hitResult` for a block.
pub(crate) fn pov_hit_result(rt: &mut Rt, fluids: u8) -> V {
    let hit = rt.new_object("net/minecraft/world/phys/BlockHitResult");
    match rt.host().and_then(|h| h.player_pov_hit(fluids)) {
        Some((pos, face, at)) => {
            let loc = vec3(rt, at[0], at[1], at[2]);
            let dir = direction(rt, i64::from(face));
            let p = crate::api::block_pos(rt, i64::from(pos[0]), i64::from(pos[1]), i64::from(pos[2]));
            rt.put_field(hit, "$loc", loc);
            rt.put_field(hit, "$dir", dir);
            rt.put_field(hit, "$pos", p);
            rt.put_field(hit, "$inside", V::I(0));
        }
        None => {
            // From the eyes along the look, 4.5 blocks (`blockInteractionRange`).
            let ([x, y, z], yaw, pitch) = rt.host().and_then(|h| h.player_place()).unwrap_or(([0.0; 3], 0.0, 0.0));
            let eye = player_eye_height(rt);
            let (yr, pr) = (f64::from(-yaw).to_radians(), f64::from(-pitch).to_radians());
            let look = [yr.sin() * pr.cos(), pr.sin(), yr.cos() * pr.cos()];
            let end = [x + look[0] * 4.5, y + eye + look[1] * 4.5, z + look[2] * 4.5];
            let loc = vec3(rt, end[0], end[1], end[2]);
            // `Direction.getNearest` of the way back: the first of the largest dot product.
            let normals = [[0.0, -1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0], [0.0, 0.0, 1.0], [-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
            let (mut face, mut best) = (2, f64::from(f32::from_bits(1)));
            for (i, n) in normals.iter().enumerate() {
                let d = -look[0] * n[0] - look[1] * n[1] - look[2] * n[2];
                if d > best {
                    (face, best) = (i as i64, d);
                }
            }
            let dir = direction(rt, face);
            let p = crate::api::block_pos(rt, end[0].floor() as i64, end[1].floor() as i64, end[2].floor() as i64);
            rt.put_field(hit, "$loc", loc);
            rt.put_field(hit, "$dir", dir);
            rt.put_field(hit, "$pos", p);
            rt.put_field(hit, "$miss", V::I(1));
        }
    }
    hit
}

/// A Java property value's text in a state.
pub(crate) fn text_of(rt: &Rt, v: V) -> String {
    match rt.class_of(v) {
        "java/lang/Boolean" => (rt.get_field(v, "value").i() != 0).to_string(),
        "java/lang/Integer" => rt.get_field(v, "value").i().to_string(),
        "net/minecraft/core/Direction" => DIRECTION_NAMES[native(rt, v)[0].clamp(0, 5) as usize].to_owned(),
        "net/minecraft/core/Direction$Axis" => AXIS_NAMES[native(rt, v)[0].clamp(0, 2) as usize].to_owned(),
        class => match rt.str_of(v) {
            Some(s) => s.to_owned(),
            None => {
                let name = rt.str_of(rt.get_field(v, "$name")).unwrap_or("");
                // A vanilla enum's constant by its serialized name, which isn't always its own
                // name in lower case (`PistonType.DEFAULT` is `normal`).
                if class.starts_with("net/minecraft/") {
                    crate::blocks::enum_text(class, name)
                } else {
                    name.to_ascii_lowercase()
                }
            }
        },
    }
}

pub(crate) fn prop_parts(rt: &Rt, p: V) -> (String, String) {
    (name_of(rt, rt.get_field(p, "$name")), name_of(rt, rt.get_field(p, "$kind")))
}

/// `Vec3.ZERO`: one object, as vanilla's (`normalize()` of a tiny vector gives it back).
fn vec3_zero(rt: &mut Rt) -> V {
    rt.intern("compat:vec3-zero".into(), |rt| vec3(rt, 0.0, 0.0, 0.0))
}

/// Vanilla's `Mth.floor(double)`: the int below, saturating as Java's `(int)` cast.
fn mth_floor(d: f64) -> i32 {
    let i = d as i32;
    if d < i as f64 { i.wrapping_sub(1) } else { i }
}

/// The block an entity at `p` is in (`blockPosition`) or stands on, as `Entity.getOnPos(float)`
/// finds it with no supporting block known: the one an offset below its feet (`getOnPos` 1.0E-5,
/// `getOnPosLegacy` 0.2, `getBlockPosBelowThatAffectsMyMovement` 0.500001, as floats).
pub(crate) fn feet_block(rt: &mut Rt, name: &str, p: [f64; 3]) -> V {
    let below = match name {
        "getOnPos" => 1.0e-5_f32,
        "getOnPosLegacy" => 0.2,
        "getBlockPosBelowThatAffectsMyMovement" => 0.500_001,
        _ => 0.0,
    };
    let y = mth_floor(p[1] - f64::from(below));
    crate::api::block_pos(
        rt,
        i64::from(mth_floor(p[0])),
        i64::from(y),
        i64::from(mth_floor(p[2])),
    )
}

const VEC2: &str = "net/minecraft/world/phys/Vec2";

pub(crate) fn vec2(rt: &mut Rt, x: f32, y: f32) -> V {
    let v = rt.new_object(VEC2);
    rt.put_field(v, "x", V::F(x));
    rt.put_field(v, "y", V::F(y));
    v
}

pub(crate) fn vec3(rt: &mut Rt, x: f64, y: f64, z: f64) -> V {
    let v = rt.new_object("net/minecraft/world/phys/Vec3");
    rt.put_field(v, "x", V::D(x));
    rt.put_field(v, "y", V::D(y));
    rt.put_field(v, "z", V::D(z));
    v
}

pub(crate) fn xyz(rt: &Rt, v: V) -> [f64; 3] {
    [rt.get_field(v, "x").d(), rt.get_field(v, "y").d(), rt.get_field(v, "z").d()]
}

fn compound(rt: &mut Rt) -> V {
    rt.alloc("net/minecraft/nbt/CompoundTag", Data::Map(HashMap::new()))
}


/// Writes `stack` into the runtime compound `tag` as `ItemStack.CODEC` does: `id`, `count`, and its
/// mod components (`components`, when it has any).
pub(crate) fn save_stack(rt: &mut Rt, stack: V, tag: V) {
    let id = item_like_name(rt, rt.get_field(stack, "$item"));
    let count = rt.get_field(stack, "$count").i();
    let (ik, iv, ck) = (rt.string("id"), rt.string(&id), rt.string("count"));
    crate::api::call(rt, "net/minecraft/nbt/CompoundTag.putString.(Ljava/lang/String;Ljava/lang/String;)V", &[tag, ik, iv]);
    crate::api::call(rt, "net/minecraft/nbt/CompoundTag.putInt.(Ljava/lang/String;I)V", &[tag, ck, V::I(count)]);
    let carried = crate::components::patch(rt, stack);
    if !carried.is_empty() {
        let c = crate::nbt::to_runtime(rt, &carried);
        let k = rt.string("components");
        crate::api::call(rt, "net/minecraft/nbt/CompoundTag.put.(Ljava/lang/String;Lnet/minecraft/nbt/Tag;)Lnet/minecraft/nbt/Tag;", &[tag, k, c]);
    }
}

/// A stack from its saved form (`{id, count, components}`); None for an empty or unknown item, as
/// `ItemStack.parse` refuses it.
pub(crate) fn parse_stack(rt: &mut Rt, tag: &std::collections::BTreeMap<String, crate::nbt::Nbt>) -> Option<V> {
    use crate::nbt::Nbt;
    let Some(Nbt::String(id)) = tag.get("id") else { return None };
    let id = if id.contains(':') { id.clone() } else { format!("minecraft:{id}") };
    let count = match tag.get("count").or_else(|| tag.get("Count")) {
        Some(Nbt::Int(n)) => *n,
        Some(Nbt::Byte(n)) => i32::from(*n),
        Some(Nbt::Short(n)) => i32::from(*n),
        _ => 1,
    };
    if id == "minecraft:air" || count <= 0 || !crate::recipes::item_known(rt, &id) {
        return None;
    }
    let i = item(rt, &id);
    let s = stack_of(rt, i, count);
    if let Some(Nbt::Compound(patch)) = tag.get("components") {
        crate::components::load_saved_nbt(rt, s, patch);
    }
    Some(s)
}

pub fn stack(rt: &mut Rt, item_name: &str, count: i32) -> V {
    let s = rt.new_object("net/minecraft/world/item/ItemStack");
    let i = item(rt, item_name);
    rt.put_field(s, "$item", i);
    rt.put_field(s, "$count", V::I(count));
    s
}

/// Whether a stack is empty: none, no count, or air. A mod's item is its registered object.
/// Vanilla's hash strategy for item stacks (`ItemStackLinkedSet.TYPE_AND_TAG`: same item, same
/// components, the count aside) and NeoForge's for fluid stacks
/// (`FluidStackLinkedSet.TYPE_AND_COMPONENTS`), as the anonymous classes they are.
const ITEM_STRATEGY: &str = "net/minecraft/world/item/ItemStackLinkedSet$1";
const FLUID_STRATEGY: &str = "net/neoforged/neoforge/fluids/FluidStackLinkedSet$1";

/// A stack strategy's `hashCode(stack)` and `equals(a, b)`; None for a strategy that isn't one.
pub(crate) fn stack_strategy(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let fluid = match rt.class_of(a(0)) {
        ITEM_STRATEGY => false,
        FLUID_STRATEGY => true,
        _ => return None,
    };
    Some(match name {
        "hashCode" => V::I(stack_hash(rt, a(1), fluid)),
        "equals" => {
            let (x, y) = (a(1), a(2));
            if x == y || x.r() == 0 || y.r() == 0 {
                return Some(V::I((x == y) as i32));
            }
            let same = if fluid {
                crate::api::call(rt, "net/neoforged/neoforge/fluids/FluidStack.isSameFluidSameComponents.(Lnet/neoforged/neoforge/fluids/FluidStack;Lnet/neoforged/neoforge/fluids/FluidStack;)Z", &[x, y]).i() != 0
            } else {
                stack_empty(rt, x) == stack_empty(rt, y)
                    && crate::api::call(rt, "net/minecraft/world/item/ItemStack.isSameItemSameComponents.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/ItemStack;)Z", &[x, y]).i() != 0
            };
            V::I(same as i32)
        }
        _ => return None,
    })
}

/// A stack's hash as far as stacks a strategy calls equal go (`ItemStack.hashItemAndComponents`):
/// its item's name, or its fluid's identity; 0 for null. Components only make equal ones rarer.
fn stack_hash(rt: &Rt, s: V, fluid: bool) -> i32 {
    if s.r() == 0 {
        0
    } else if fluid {
        rt.get_field(s, "$fluid").r() as i32
    } else {
        crate::jlang::string_hash(&item_like_name(rt, rt.get_field(s, "$item")))
    }
}

/// A linked set keyed by `strategy` (`ItemStackLinkedSet.createTypeAndComponentsSet()`).
fn strategy_set(rt: &mut Rt, strategy: &'static str) -> V {
    let s = intern_strategy(rt, strategy);
    rt.alloc("it/unimi/dsi/fastutil/objects/ObjectLinkedOpenCustomHashSet", Data::Table(crate::Table { strategy: s, ..Default::default() }))
}

fn intern_strategy(rt: &mut Rt, class: &'static str) -> V {
    rt.intern(format!("compat:strategy/{class}"), |rt| rt.new_object(class))
}

pub(crate) fn stack_empty(rt: &Rt, s: V) -> bool {
    if s.r() == 0 || rt.get_field(s, "$count").i() <= 0 {
        return true;
    }
    let i = rt.get_field(s, "$item");
    i.r() == 0 || item_like_name(rt, i) == "minecraft:air"
}

/// A stack of the item `$item` holds (a mod's registered object or a named item), `count` of it.
fn stack_of(rt: &mut Rt, item: V, count: i32) -> V {
    let s = rt.new_object("net/minecraft/world/item/ItemStack");
    rt.put_field(s, "$item", item);
    rt.put_field(s, "$count", V::I(count));
    s
}

/// The name an `ItemLike` (an Item, a Block, a holder) stands for.
/// Vanilla's `Item.Properties.craftRemainder`s (`Items`): a bucket of anything leaves the bucket,
/// honey and dragon's breath the bottle.
const VANILLA_CRAFT_REMAINDERS: [(&str, &str); 5] = [
    ("minecraft:water_bucket", "minecraft:bucket"),
    ("minecraft:lava_bucket", "minecraft:bucket"),
    ("minecraft:milk_bucket", "minecraft:bucket"),
    ("minecraft:dragon_breath", "minecraft:glass_bottle"),
    ("minecraft:honey_bottle", "minecraft:glass_bottle"),
];

/// What crafting with `item` leaves in the grid (`Item.getCraftingRemainingItem`): the mod item's
/// own `craftRemainder`, else vanilla's; None for nothing.
pub(crate) fn crafting_remainder(rt: &Rt, item: V) -> Option<String> {
    if item.r() == 0 {
        return None;
    }
    let props = rt.get_field(item, "$properties");
    let own = if props.r() != 0 { rt.get_field(props, "$craftRemainder") } else { V::NULL };
    if own.r() != 0 {
        return Some(item_like_name(rt, own));
    }
    let name = item_like_name(rt, item);
    VANILLA_CRAFT_REMAINDERS.iter().find(|(i, _)| *i == name).map(|(_, r)| (*r).to_owned())
}

const ITEM_DESCRIPTION_ID: &str = "net/minecraft/world/item/Item.getDescriptionId.()Ljava/lang/String;";
const ITEM_STACK_DESCRIPTION_ID: &str = "net/minecraft/world/item/Item.getDescriptionId.(Lnet/minecraft/world/item/ItemStack;)Ljava/lang/String;";
const ITEM_GET_NAME: &str = "net/minecraft/world/item/Item.getName.(Lnet/minecraft/world/item/ItemStack;)Lnet/minecraft/network/chat/Component;";
const STACK_GET: &str = "net/minecraft/world/item/ItemStack.get.(Lnet/minecraft/core/component/DataComponentType;)Ljava/lang/Object;";

/// Vanilla's item classes (`Item`, `BlockItem`, `SwordItem`...), the owner a mod item's inherited
/// calls carry.
pub(crate) fn is_item_class(owner: &str) -> bool {
    owner.strip_prefix("net/minecraft/world/item/").is_some_and(|n| n.ends_with("Item") && !n.contains('/'))
}

/// `item.canFitInsideContainerItems(stack)` (NeoForge's, with the stack) or vanilla's
/// `item.canFitInsideContainerItems()` (without), called virtually: a mod item's override answers,
/// else the default (here again).
fn fits_virtually(rt: &mut Rt, item: V, stack: Option<V>) -> V {
    const WITH_STACK: (&str, &str) = (
        "net/minecraft/world/item/Item.canFitInsideContainerItems.(Lnet/minecraft/world/item/ItemStack;)Z",
        "canFitInsideContainerItems.(Lnet/minecraft/world/item/ItemStack;)Z",
    );
    const PLAIN: (&str, &str) = ("net/minecraft/world/item/Item.canFitInsideContainerItems.()Z", "canFitInsideContainerItems.()Z");
    let ((owner_key, key), args) = match stack {
        Some(s) => (WITH_STACK, vec![item, s]),
        None => (PLAIN, vec![item]),
    };
    match rt.vcall {
        Some(vcall) => vcall(rt, owner_key, key, &args),
        None => crate::api::call(rt, owner_key, &args),
    }
}

/// Vanilla's `Item.canFitInsideContainerItems()`: true, but for a shulker box's item
/// (`BlockItem`'s: its block a `ShulkerBoxBlock`, vanilla's 17 or a mod's subclass).
fn fits_inside_container_items(rt: &Rt, item: V) -> bool {
    // A vanilla item names its block (`minecraft:red_shulker_box`); a mod's `BlockItem` holds it.
    let block = if rt.str_of(item).is_some() { item } else { rt.get_field(item, "$block") };
    if block.r() == 0 {
        return true;
    }
    match rt.str_of(block) {
        Some(name) => !name.strip_prefix("minecraft:").is_some_and(|n| n == "shulker_box" || n.ends_with("_shulker_box")),
        None => !rt.instance_of(block, "net/minecraft/world/level/block/ShulkerBoxBlock"),
    }
}

/// `ItemStack.getItem`: air for an empty stack.
fn stack_item(rt: &mut Rt, stack: V) -> V {
    if stack_empty(rt, stack) { item(rt, "minecraft:air") } else { rt.get_field(stack, "$item") }
}

/// The stack's component of vanilla type `key` (`DataComponents.CUSTOM_NAME`), or null.
fn stack_component(rt: &mut Rt, stack: V, key: &'static str) -> V {
    let ty = crate::api::get_static(rt, key);
    crate::api::call(rt, STACK_GET, &[stack, ty])
}

/// `ItemStack.getHoverName`: its custom name, else its item name, else `Item.getName(stack)`.
fn hover_name(rt: &mut Rt, stack: V) -> V {
    for ty in ["net/minecraft/core/component/DataComponents.CUSTOM_NAME", "net/minecraft/core/component/DataComponents.ITEM_NAME"] {
        let name = stack_component(rt, stack, ty);
        if name.r() != 0 {
            return name;
        }
    }
    let item = stack_item(rt, stack);
    crate::menus::virt(rt, ITEM_GET_NAME, item, &[stack])
}

/// `Item.getDescriptionId`: `item.<namespace>.<path>` (`Util.makeDescriptionId`), or for a block
/// item its block's `block.<namespace>.<path>`. A mod's block item names itself after its block
/// unless it is an `ItemNameBlockItem`; vanilla's own block items that keep the item's name (seeds,
/// redstone, string) are told by the language having the item's key, as the game's names are.
pub(crate) fn item_description_id(rt: &Rt, item: V) -> String {
    let key = |kind: &str, name: &str| {
        let (ns, path) = name.split_once(':').unwrap_or(("minecraft", name));
        format!("{kind}.{ns}.{}", path.replace('/', "."))
    };
    let name = item_like_name(rt, item);
    if rt.str_of(item).is_none() {
        let block = rt.get_field(item, "$block");
        if block.r() != 0 && !rt.is_a(rt.class_of(item), "net/minecraft/world/item/ItemNameBlockItem") {
            let placed = rt.str_of(block).map(str::to_owned).or_else(|| rt.registered_name(block, "block").map(str::to_owned));
            if let Some(b) = placed {
                return key("block", &b);
            }
        }
        return key("item", &name);
    }
    let own = key("item", &name);
    if rt.host().is_some_and(|h| h.item_is_block(&name) && h.translate(&own).is_none()) { key("block", &name) } else { own }
}

pub(crate) fn item_like_name(rt: &Rt, v: V) -> String {
    rt.str_of(v)
        .map(str::to_owned)
        .or_else(|| rt.registered_name(v, "item").map(str::to_owned))
        .or_else(|| rt.registered_name(v, "block").map(str::to_owned))
        .unwrap_or_else(|| "minecraft:air".into())
}

/// The next value of the runtime's random stream (xorshift64*).
pub(crate) fn next_random(rt: &mut Rt) -> u64 {
    let mut x = rt.random_state.max(1);
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    rt.random_state = x;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

const CRASH_REPORT: &str = "net/minecraft/CrashReport";
const CRASH_REPORT_CATEGORY: &str = "net/minecraft/CrashReportCategory";
const REPORTED_EXCEPTION: &str = "net/minecraft/ReportedException";
const RANDOM_SOURCE: &str = "net/minecraft/util/RandomSource";
/// `forkPositional()`'s factory of sources by position or name (`LegacyPositionalRandomFactory`).
const POSITIONAL_RANDOM: &str = "net/minecraft/world/level/levelgen/PositionalRandomFactory";
const LEGACY_POSITIONAL_RANDOM: &str =
    "net/minecraft/world/level/levelgen/LegacyRandomSource$LegacyPositionalRandomFactory";
/// `java.util.Random`'s generator, which vanilla's legacy sources are.
const LCG_MULTIPLIER: i64 = 0x5_DEEC_E66D;
const LCG_MASK: i64 = (1 << 48) - 1;

/// Vanilla's random sources by class: the interface, and the legacy generator's classes a mod makes
/// with `new` (`new LegacyRandomSource(seed)`; the single-threaded and thread-safe ones draw the
/// same numbers).
fn is_random_class(class: &str) -> bool {
    matches!(
        class,
        RANDOM_SOURCE
            | "net/minecraft/world/level/levelgen/BitRandomSource"
            | "net/minecraft/world/level/levelgen/LegacyRandomSource"
            | "net/minecraft/world/level/levelgen/SingleThreadedRandomSource"
            | "net/minecraft/world/level/levelgen/ThreadSafeLegacyRandomSource"
    )
}

/// A source of its own from `seed`, as `new LegacyRandomSource(seed)`: the same seed draws the same
/// numbers (a block's look picked by its position).
pub(crate) fn seeded_random(rt: &mut Rt, seed: i64) -> V {
    let r = rt.new_object(RANDOM_SOURCE);
    set_random_seed(rt, r, seed);
    r
}

/// `setSeed(seed)`: the generator restarted from `seed`, its kept gaussian dropped.
fn set_random_seed(rt: &mut Rt, r: V, seed: i64) {
    rt.put_field(r, "$seed", V::J((seed ^ LCG_MULTIPLIER) & LCG_MASK));
    rt.put_field(r, "$gaussian", V::Void);
}

/// `next(bits)`: the source's own generator (`LegacyRandomSource.next`), or the runtime's stream for
/// a source without one (the level's, a tick's).
fn random_bits(rt: &mut Rt, r: V, bits: u32) -> i32 {
    match rt.get_field(r, "$seed") {
        V::J(s) => {
            let s = s.wrapping_mul(LCG_MULTIPLIER).wrapping_add(11) & LCG_MASK;
            rt.put_field(r, "$seed", V::J(s));
            (s >> (48 - bits)) as i32
        }
        _ => (next_random(rt) >> (64 - bits)) as i32,
    }
}

/// `nextInt(bound)` as `BitRandomSource` draws it; 0 for a bound below 1.
fn random_int_bound(rt: &mut Rt, r: V, bound: i32) -> i32 {
    if bound <= 0 {
        return 0;
    }
    if bound & (bound - 1) == 0 {
        return ((i64::from(bound) * i64::from(random_bits(rt, r, 31))) >> 31) as i32;
    }
    loop {
        let bits = random_bits(rt, r, 31);
        let value = bits % bound;
        // Java's signed overflow is part of the rejection rule.
        if bits.wrapping_sub(value).wrapping_add(bound - 1) >= 0 {
            return value;
        }
    }
}

fn random_long(rt: &mut Rt, r: V) -> i64 {
    let high = i64::from(random_bits(rt, r, 32)) << 32;
    high.wrapping_add(i64::from(random_bits(rt, r, 32)))
}

fn random_float(rt: &mut Rt, r: V) -> f32 {
    random_bits(rt, r, 24) as f32 * (1.0 / 16_777_216.0)
}

fn random_double(rt: &mut Rt, r: V) -> f64 {
    let value = (i64::from(random_bits(rt, r, 26)) << 27) + i64::from(random_bits(rt, r, 27));
    value as f64 * (1.0 / 9_007_199_254_740_992.0)
}

/// `nextGaussian()`: Marsaglia's polar method, as vanilla's legacy source (`MarsagliaPolarGaussian`),
/// the second value of a pair kept for the next call.
fn random_gaussian(rt: &mut Rt, r: V) -> f64 {
    if let V::D(kept) = rt.get_field(r, "$gaussian") {
        rt.put_field(r, "$gaussian", V::Void);
        return kept;
    }
    loop {
        let x = 2.0 * random_double(rt, r) - 1.0;
        let y = 2.0 * random_double(rt, r) - 1.0;
        let radius = x * x + y * y;
        if radius >= 1.0 || radius == 0.0 {
            continue;
        }
        let multiplier = (-2.0 * radius.ln() / radius).sqrt();
        rt.put_field(r, "$gaussian", V::D(y * multiplier));
        return x * multiplier;
    }
}

/// A random source's methods (`RandomSource`'s and the defaults vanilla composes from `next(bits)`),
/// on receiver `args[0]`.
fn random_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let r = a(0);
    Some(match (name, desc) {
        ("nextInt", "()I") => V::I(random_bits(rt, r, 32)),
        ("nextInt", "(I)I") => V::I(random_int_bound(rt, r, a(1).i())),
        // `origin + nextInt(bound - origin)`, the origin itself for an empty range.
        ("nextInt", "(II)I") if a(1).i() >= a(2).i() => a(1),
        ("nextInt", "(II)I") => V::I(a(1).i().wrapping_add(random_int_bound(
            rt,
            r,
            a(2).i().wrapping_sub(a(1).i()),
        ))),
        ("nextIntBetweenInclusive", "(II)I") => V::I(a(1).i().wrapping_add(random_int_bound(
            rt,
            r,
            a(2).i().wrapping_sub(a(1).i()).wrapping_add(1),
        ))),
        ("nextLong", "()J") => V::J(random_long(rt, r)),
        ("nextBoolean", "()Z") => V::I(i32::from(random_bits(rt, r, 1) != 0)),
        ("nextFloat", "()F") => V::F(random_float(rt, r)),
        ("nextDouble", "()D") => V::D(random_double(rt, r)),
        ("nextGaussian", "()D") => V::D(random_gaussian(rt, r)),
        // `mean + deviation * (next() - next())`.
        ("triangle", "(DD)D") => {
            let (one, two) = (random_double(rt, r), random_double(rt, r));
            V::D(a(1).d() + a(2).d() * (one - two))
        }
        ("triangle", "(FF)F") => {
            let (one, two) = (random_float(rt, r), random_float(rt, r));
            V::F(a(1).f() + a(2).f() * (one - two))
        }
        ("consumeCount", "(I)V") => {
            for _ in 0..a(1).i() {
                random_bits(rt, r, 32);
            }
            V::Void
        }
        ("setSeed", "(J)V") => {
            set_random_seed(rt, r, a(1).j());
            V::Void
        }
        // `new LegacyRandomSource(nextLong())`, and the positional factory of `nextLong()`.
        ("fork", _) => {
            let seed = random_long(rt, r);
            seeded_random(rt, seed)
        }
        ("forkPositional", _) => {
            let seed = random_long(rt, r);
            let f = rt.new_object(LEGACY_POSITIONAL_RANDOM);
            rt.put_field(f, "seed", V::J(seed));
            f
        }
        _ => return None,
    })
}

/// The source `holder` keeps in its field `random`, made (from the runtime's stream) on first use.
fn own_random(rt: &mut Rt, holder: V) -> V {
    match rt.get_field(holder, "random") {
        r if r.r() != 0 => r,
        _ => {
            let seed = next_random(rt) as i64;
            let r = seeded_random(rt, seed);
            if holder.r() != 0 {
                rt.put_field(holder, "random", r);
            }
            r
        }
    }
}

/// `PositionalRandomFactory`'s sources (`LegacyPositionalRandomFactory`'s): one of `Mth.getSeed(x, y,
/// z) ^ seed` at a position, of `name.hashCode() ^ seed` by name, or of a seed given.
fn positional_random_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let seed = rt.get_field(a(0), "seed").j();
    let at = |x: i32, y: i32, z: i32| {
        let mut l = i64::from(x.wrapping_mul(3_129_871))
            ^ i64::from(z).wrapping_mul(116_129_781)
            ^ i64::from(y);
        l = l
            .wrapping_mul(l)
            .wrapping_mul(42_317_861)
            .wrapping_add(l.wrapping_mul(11));
        (l >> 16) ^ seed
    };
    let s = match (name, desc) {
        ("at", "(III)Lnet/minecraft/util/RandomSource;") => at(a(1).i(), a(2).i(), a(3).i()),
        ("at", "(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/util/RandomSource;") => {
            let p = crate::api::pos_of(rt, a(1));
            at(p[0], p[1], p[2])
        }
        ("fromHashOf", _) => {
            let text = text_of(rt, a(1));
            i64::from(crate::jlang::string_hash(&text)) ^ seed
        }
        ("fromSeed", "(J)Lnet/minecraft/util/RandomSource;") => a(1).j(),
        _ => return None,
    };
    Some(seeded_random(rt, s))
}

/// A `java.util.Optional` holding `v` (null: empty), in the JDK's own field.
pub fn optional(rt: &mut Rt, v: V) -> V {
    let o = rt.new_object("java/util/Optional");
    rt.put_field(o, "value", held(v));
    o
}

/// What an `Optional` holds of `v`: null for nothing, else `v`, also when it is a primitive that
/// Java would have boxed (`Optional.map(BeltBlockEntity::hasPulley)` gets the method's raw
/// boolean back, as collections hold their numbers raw).
fn held(v: V) -> V {
    match v {
        V::Void => V::NULL,
        other => other,
    }
}

/// `java.util.Optional`, whole: the factories, the tests and the lambda forms (`ifPresent`, `map`,
/// `filter`...), which run the mod's lambdas the way the JDK does.
fn optional_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    const ACCEPT: &str = "java/util/function/Consumer.accept.(Ljava/lang/Object;)V";
    const RUN: &str = "java/lang/Runnable.run.()V";
    const SUPPLY: &str = "java/util/function/Supplier.get.()Ljava/lang/Object;";
    const APPLY: &str = "java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;";
    const TEST: &str = "java/util/function/Predicate.test.(Ljava/lang/Object;)Z";
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    // Static factories have no receiver; everything else reads the receiver's value.
    match name {
        "of" | "ofNullable" => return Some(optional(rt, a(0))),
        "empty" => return Some(rt.intern("compat:optional-empty".into(), |rt| optional(rt, V::NULL))),
        _ => {}
    }
    let v = held(rt.get_field(a(0), "value"));
    let present = v != V::NULL;
    Some(match name {
        "isPresent" => V::I(present as i32),
        "isEmpty" => V::I(!present as i32),
        "get" | "orElseThrow" => {
            if !present {
                // `orElseThrow(Supplier)` throws what the mod's supplier makes.
                if desc.starts_with("(Ljava/util/function/Supplier;") {
                    let e = rt.call_functional(SUPPLY, a(1), &[]);
                    if rt.thrown.is_none() {
                        rt.thrown = Some(e);
                    }
                } else {
                    rt.throw_new("java/util/NoSuchElementException");
                }
                return Some(V::NULL);
            }
            v
        }
        "orElse" => {
            if present {
                v
            } else {
                a(1)
            }
        }
        "orElseGet" => {
            if present {
                v
            } else {
                rt.call_functional(SUPPLY, a(1), &[])
            }
        }
        "ifPresent" => {
            if present {
                rt.call_functional(ACCEPT, a(1), &[v]);
            }
            V::Void
        }
        "ifPresentOrElse" => {
            if present {
                rt.call_functional(ACCEPT, a(1), &[v]);
            } else {
                rt.call_functional(RUN, a(2), &[]);
            }
            V::Void
        }
        "map" => {
            if !present {
                return Some(a(0));
            }
            let r = rt.call_functional(APPLY, a(1), &[v]);
            optional(rt, r)
        }
        "flatMap" => {
            if !present {
                return Some(a(0));
            }
            rt.call_functional(APPLY, a(1), &[v])
        }
        "filter" => {
            if present && rt.call_functional(TEST, a(1), &[v]).i() != 0 {
                a(0)
            } else {
                optional(rt, V::NULL)
            }
        }
        "or" => {
            if present {
                a(0)
            } else {
                rt.call_functional(SUPPLY, a(1), &[])
            }
        }
        "stream" => rt.alloc("java/util/stream/Stream", Data::Stream(if present { vec![v] } else { Vec::new() })),
        "equals" => {
            let other = held(rt.get_field(a(1), "value"));
            V::I((rt.class_of(a(1)) == "java/util/Optional" && (v == other || (present && crate::jutil::equals(rt, v, other)))) as i32)
        }
        "hashCode" => V::I(if present { crate::jutil::hash(rt, v) } else { 0 }),
        _ => return None,
    })
}

pub fn call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    if owner == "net/minecraft/util/Mth"
        && let Some(v) = mth_call(rt, name, desc, args)
    {
        return Some(v);
    }
    if owner == "net/minecraft/util/FastColor$ARGB32"
        && let Some(c) = argb32(name, desc, args)
    {
        return Some(V::I(c));
    }
    // `FastColor.as8BitChannel(f)`: `Mth.floor(f * 255)`.
    if owner == "net/minecraft/util/FastColor" && name == "as8BitChannel" {
        let f = args.first().copied().unwrap_or_default().f() * 255.0;
        return Some(V::I(f.floor() as i32));
    }
    // `DyeItem.byColor(color)`: vanilla's dye of that colour.
    if owner == "net/minecraft/world/item/DyeItem" && name == "byColor" {
        let i = rt.get_field(args.first().copied().unwrap_or_default(), "$ordinal").i().clamp(0, 15) as usize;
        let n = format!("minecraft:{}_dye", crate::blocks::DYES[i].0.to_ascii_lowercase());
        return Some(item(rt, &n));
    }
    // `DyeItem.getDyeColor()`: the colour its constructor got (a mod's dye), else the one a vanilla
    // dye's id names.
    if owner == "net/minecraft/world/item/DyeItem" && name == "getDyeColor" {
        let dye = args.first().copied().unwrap_or_default();
        let own = rt.get_field(dye, "$dyeColor");
        if own.r() != 0 {
            return Some(own);
        }
        let id = item_like_name(rt, dye);
        let colour = crate::blocks::DYES
            .iter()
            .position(|(d, _)| id == format!("minecraft:{}_dye", d.to_ascii_lowercase()));
        return Some(match colour {
            Some(i) => crate::api::api_enum(
                rt,
                "net/minecraft/world/item/DyeColor",
                crate::blocks::DYES[i].0,
                i as i32,
            ),
            None => V::NULL,
        });
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    const STATE: &str = "net/minecraft/world/level/block/state/BlockState";
    const BASE: &str = "net/minecraft/world/level/block/state/BlockBehaviour$BlockStateBase";
    const HOLDER: &str = "net/minecraft/world/level/block/state/StateHolder";
    Some(match (owner, name) {
        // What a translation may take as an argument as it is (a number, a boolean, a string);
        // anything else becomes a component.
        ("net/minecraft/network/chat/contents/TranslatableContents", "isAllowedPrimitiveArgument") => {
            let c = rt.class_of(a(0));
            let number = matches!(
                c,
                "java/lang/Integer" | "java/lang/Long" | "java/lang/Short" | "java/lang/Byte" | "java/lang/Float" | "java/lang/Double" | "java/math/BigInteger" | "java/math/BigDecimal"
            );
            V::I((number || matches!(c, "java/lang/Boolean" | "java/lang/String")) as i32)
        }
        // A chunk ticket's type: its name, comparator and lifespan (a long), as `create(name,
        // comparator[, lifespan])` or the constructor gives them.
        (TICKET_TYPE, "create" | "<init>") => {
            let (t, at) = if name == "create" { (rt.new_object(TICKET_TYPE), 0) } else { (a(0), 1) };
            let lifespan = match a(at + 2) {
                V::J(j) => j,
                V::I(i) => i64::from(i),
                _ => 0,
            };
            rt.put_field(t, "name", a(at));
            rt.put_field(t, "comparator", a(at + 1));
            rt.put_field(t, "timeout", V::J(lifespan));
            if name == "create" { t } else { V::Void }
        }
        (TICKET_TYPE, "toString") => rt.get_field(a(0), "name"),
        (TICKET_TYPE, "getComparator") => rt.get_field(a(0), "comparator"),
        (TICKET_TYPE, "timeout") => V::J(rt.get_field(a(0), "timeout").j()),
        // `MinMaxBounds.Ints.between(min, max)` and the other factories, `Doubles`' too.
        (BOUNDS_INTS | BOUNDS_DOUBLES, "exactly" | "between" | "atLeast" | "atMost") => {
            let (min, max) = match name {
                "exactly" => (Some(a(0)), Some(a(0))),
                "between" => (Some(a(0)), Some(a(1))),
                "atLeast" => (Some(a(0)), None),
                _ => (None, Some(a(0))),
            };
            bounds(rt, owner, min, max)
        }
        // `matches(value)`: not under its min, not over its max; `isAny()`: neither set.
        (BOUNDS_INTS | BOUNDS_DOUBLES, "matches" | "isAny") => {
            let num = |v: V| match v {
                V::I(i) => f64::from(i),
                V::J(j) => j as f64,
                V::F(f) => f64::from(f),
                V::D(d) => d,
                _ => 0.0,
            };
            // An optional's number, held raw or boxed.
            let bound = |rt: &Rt, f: &'static str| match rt.get_field(rt.get_field(a(0), f), "value") {
                V::Void | V::R(0) => None,
                V::R(boxed) => Some(num(rt.get_field(V::R(boxed), "value"))),
                v => Some(num(v)),
            };
            let (min, max) = (bound(rt, "min"), bound(rt, "max"));
            if name == "isAny" {
                V::I((min.is_none() && max.is_none()) as i32)
            } else {
                let v = num(a(1));
                V::I((min.is_none_or(|m| m <= v) && max.is_none_or(|m| m >= v)) as i32)
            }
        }
        // `LocalDate.now()`: today, in UTC (the game holds no time zone); `of(y, m, d)`; its parts.
        (LOCAL_DATE, "now") => {
            let days = now_nanos().div_euclid(86_400_000_000_000);
            local_date(rt, civil_date(days))
        }
        (LOCAL_DATE, "of") => {
            let month = if desc.contains("Ljava/time/Month;") { rt.get_field(a(1), "$ordinal").i() + 1 } else { a(1).i() };
            local_date(rt, (a(0).i(), month as u32, a(2).i() as u32))
        }
        (LOCAL_DATE, "getYear") => rt.get_field(a(0), "year"),
        (LOCAL_DATE, "getMonthValue") => rt.get_field(a(0), "month"),
        (LOCAL_DATE, "getDayOfMonth") => rt.get_field(a(0), "day"),
        (LOCAL_DATE, "getMonth") => {
            let i = (rt.get_field(a(0), "month").i() - 1).clamp(0, 11) as usize;
            let (_, names) = PLAIN_ENUMS.iter().find(|(c, _)| *c == MONTH)?;
            plain_enum(rt, MONTH, names[i])?
        }
        (MONTH, "getValue") => V::I(rt.get_field(a(0), "$ordinal").i() + 1),
        // `TimeUnit`'s conversions (`MILLISECONDS.toSeconds(d)`, `convert(d, unit)`).
        ("java/util/concurrent/TimeUnit", n) if n.starts_with("to") || n == "convert" => {
            let this = rt.get_field(a(0), "$ordinal").i().max(0) as usize;
            let (from, to) = match n {
                "convert" => (rt.get_field(a(2), "$ordinal").i().max(0) as usize, this),
                "toNanos" => (this, 0),
                "toMicros" => (this, 1),
                "toMillis" => (this, 2),
                "toSeconds" => (this, 3),
                "toMinutes" => (this, 4),
                "toHours" => (this, 5),
                "toDays" => (this, 6),
                _ => return None,
            };
            V::J(time_convert(a(1).j(), from, to))
        }
        // Guava's `Stopwatch`: the time between `start` and `stop`, in the unit asked.
        (STOPWATCH, n) => {
            let running = |rt: &Rt| matches!(rt.get_field(a(0), "$start"), V::J(_));
            let elapsed = |rt: &Rt| {
                let before = rt.get_field(a(0), "$elapsed").j();
                match rt.get_field(a(0), "$start") {
                    V::J(start) => before + (now_nanos() - start),
                    _ => before,
                }
            };
            match n {
                "createStarted" | "createUnstarted" => {
                    let w = rt.new_object(STOPWATCH);
                    rt.put_field(w, "$elapsed", V::J(0));
                    if n == "createStarted" {
                        rt.put_field(w, "$start", V::J(now_nanos()));
                    }
                    w
                }
                "start" => {
                    rt.put_field(a(0), "$start", V::J(now_nanos()));
                    a(0)
                }
                "stop" => {
                    let total = elapsed(rt);
                    rt.put_field(a(0), "$elapsed", V::J(total));
                    rt.put_field(a(0), "$start", V::NULL);
                    a(0)
                }
                "reset" => {
                    rt.put_field(a(0), "$elapsed", V::J(0));
                    rt.put_field(a(0), "$start", V::NULL);
                    a(0)
                }
                "isRunning" => V::I(running(rt) as i32),
                "elapsed" if desc.starts_with("(Ljava/util/concurrent/TimeUnit;)") => {
                    let unit = rt.get_field(a(1), "$ordinal").i().max(0) as usize;
                    V::J(time_convert(elapsed(rt), 0, unit))
                }
                "toString" => {
                    let ms = elapsed(rt) as f64 / 1e6;
                    rt.string(&format!("{ms:.4} ms"))
                }
                _ => return None,
            }
        }
        // `LocalTime`: a time of day, now in UTC (as `LocalDate.now()`); `until` counts the whole
        // units of a `ChronoUnit` from one to another, signed (Mekanism's time to midnight).
        (LOCAL_TIME, "now") => local_time(rt, now_nanos()),
        (LOCAL_TIME, "of") => {
            let parts = [a(0).i(), a(1).i(), if desc.starts_with("(III") { a(2).i() } else { 0 }, if desc.starts_with("(IIII)") { a(3).i() } else { 0 }];
            local_time(rt, i64::from(parts[0]) * 3_600_000_000_000 + i64::from(parts[1]) * 60_000_000_000 + i64::from(parts[2]) * 1_000_000_000 + i64::from(parts[3]))
        }
        (LOCAL_TIME, n) => {
            let nanos = rt.get_field(a(0), "$nanoOfDay").j();
            let other = |rt: &Rt| rt.get_field(a(1), "$nanoOfDay").j();
            match n {
                "until" => {
                    let unit = CHRONO_NANOS[rt.get_field(a(2), "$ordinal").i().clamp(0, 15) as usize];
                    V::J((i128::from(other(rt) - nanos) / unit) as i64)
                }
                "getHour" => V::I((nanos / 3_600_000_000_000) as i32),
                "getMinute" => V::I((nanos / 60_000_000_000 % 60) as i32),
                "getSecond" => V::I((nanos / 1_000_000_000 % 60) as i32),
                "getNano" => V::I((nanos % 1_000_000_000) as i32),
                "toNanoOfDay" => V::J(nanos),
                "toSecondOfDay" => V::I((nanos / 1_000_000_000) as i32),
                "isBefore" => V::I((nanos < other(rt)) as i32),
                "isAfter" => V::I((nanos > other(rt)) as i32),
                "compareTo" => V::I(nanos.cmp(&other(rt)) as i32),
                "equals" => V::I((rt.class_of(a(1)) == LOCAL_TIME && nanos == other(rt)) as i32),
                "plusSeconds" | "plusMinutes" | "plusHours" | "plusNanos" => {
                    let unit: i64 = match n {
                        "plusNanos" => 1,
                        "plusSeconds" => 1_000_000_000,
                        "plusMinutes" => 60_000_000_000,
                        _ => 3_600_000_000_000,
                    };
                    local_time(rt, nanos.wrapping_add(a(1).j().rem_euclid(NANOS_PER_DAY / unit.min(NANOS_PER_DAY)) * unit))
                }
                _ => return None,
            }
        }
        (CHRONO_UNIT, "getDuration") => {
            let unit = CHRONO_NANOS[rt.get_field(a(0), "$ordinal").i().clamp(0, 15) as usize];
            duration(rt, unit)
        }
        // `Duration`: seconds and nanoseconds; made from an amount of a unit, read back in one.
        (DURATION, n) if n.starts_with("of") => {
            let unit: i128 = match n {
                "ofNanos" => 1,
                "ofMillis" => 1_000_000,
                "ofSeconds" => 1_000_000_000,
                "ofMinutes" => 60_000_000_000,
                "ofHours" => 3_600_000_000_000,
                "ofDays" => NANOS_PER_DAY as i128,
                "of" => CHRONO_NANOS[rt.get_field(a(1), "$ordinal").i().clamp(0, 15) as usize],
                _ => return None,
            };
            let extra = if n == "ofSeconds" && desc == "(JJ)Ljava/time/Duration;" { i128::from(a(1).j()) } else { 0 };
            duration(rt, i128::from(a(0).j()) * unit + extra)
        }
        (DURATION, n) => {
            let d = duration_nanos(rt, a(0));
            match n {
                // As Java's: from the whole seconds (rounded down) and the nanoseconds past them.
                "toNanos" => V::J(d as i64),
                "toMillis" => V::J((d.div_euclid(1_000_000_000) * 1000 + d.rem_euclid(1_000_000_000) / 1_000_000) as i64),
                "getSeconds" | "toSeconds" => V::J(d.div_euclid(1_000_000_000) as i64),
                "toMinutes" => V::J((d.div_euclid(1_000_000_000) / 60) as i64),
                "toHours" => V::J((d.div_euclid(1_000_000_000) / 3600) as i64),
                "toDays" => V::J((d.div_euclid(1_000_000_000) / 86_400) as i64),
                "getNano" => rt.get_field(a(0), "$nanos"),
                "isZero" => V::I((d == 0) as i32),
                "isNegative" => V::I((d < 0) as i32),
                "plus" | "minus" => {
                    let e = duration_nanos(rt, a(1));
                    duration(rt, if n == "plus" { d + e } else { d - e })
                }
                "compareTo" => V::I(d.cmp(&duration_nanos(rt, a(1))) as i32),
                "equals" => V::I((rt.class_of(a(1)) == DURATION && d == duration_nanos(rt, a(1))) as i32),
                _ => return None,
            }
        }
        // Executors: what they are given to run runs at once (one runtime, one thread); a task with
        // a delay or a period (a scheduled executor's) doesn't run here, its future never done.
        ("java/util/concurrent/Executors", n) => match n {
            "newSingleThreadExecutor" | "newFixedThreadPool" | "newCachedThreadPool" | "newWorkStealingPool" | "newVirtualThreadPerTaskExecutor" => executor(rt, THREAD_POOL),
            "newSingleThreadScheduledExecutor" | "newScheduledThreadPool" => executor(rt, SCHEDULED_POOL),
            "defaultThreadFactory" => rt.new_object("java/util/concurrent/ThreadFactory"),
            _ => return None,
        },
        (THREAD_POOL | SCHEDULED_POOL, "<init>") => V::Void,
        (o, n) if is_executor(o) && is_executor(rt.class_of(a(0))) => match n {
            "execute" => {
                rt.call_functional("java/lang/Runnable.run.()V", a(1), &[]);
                V::Void
            }
            "submit" => {
                let value = if desc.starts_with("(Ljava/util/concurrent/Callable;)") {
                    rt.call_functional("java/util/concurrent/Callable.call.()Ljava/lang/Object;", a(1), &[])
                } else {
                    rt.call_functional("java/lang/Runnable.run.()V", a(1), &[]);
                    if desc.starts_with("(Ljava/lang/Runnable;Ljava/lang/Object;)") { a(2) } else { V::NULL }
                };
                crate::jutil::completed_future(rt, value)
            }
            "schedule" | "scheduleAtFixedRate" | "scheduleWithFixedDelay" => rt.new_object(SCHEDULED_FUTURE),
            "shutdownNow" => crate::jutil::new_list(rt, Vec::new()),
            "isShutdown" | "isTerminated" => V::I(rt.get_field(a(0), "$shutdown").i()),
            "awaitTermination" => V::I(1),
            "shutdown" | "close" => {
                rt.put_field(a(0), "$shutdown", V::I(1));
                V::Void
            }
            n if n.starts_with("set") || n == "allowCoreThreadTimeOut" || n == "prestartAllCoreThreads" => V::Void,
            _ => return None,
        },
        // A task that never runs here: not done, not cancelled until asked.
        ("java/util/concurrent/ScheduledFuture" | "java/util/concurrent/Future" | "java/util/concurrent/RunnableScheduledFuture", n) if rt.class_of(a(0)) == SCHEDULED_FUTURE => match n {
            "cancel" => {
                rt.put_field(a(0), "$cancelled", V::I(1));
                V::I(1)
            }
            "isCancelled" => V::I(rt.get_field(a(0), "$cancelled").i()),
            "isDone" => V::I(rt.get_field(a(0), "$cancelled").i()),
            _ => return None,
        },
        // Guava's `ThreadFactoryBuilder`: the factory it builds names threads no one starts here.
        (THREAD_FACTORY_BUILDER, n) => match n {
            "<init>" => V::Void,
            "build" => rt.new_object("java/util/concurrent/ThreadFactory"),
            _ if n.starts_with("set") => a(0),
            _ => return None,
        },
        ("java/util/concurrent/ConcurrentHashMap", "newKeySet") => rt.alloc("java/util/HashSet", Data::Table(Default::default())),
        // Log4j's configuration: levels are set where its loggers write, which here is the game's log.
        ("org/apache/logging/log4j/core/config/Configurator", n) if n.starts_with("set") || n == "initialize" || n == "reconfigure" => V::Void,
        ("org/apache/logging/log4j/Level", "toString" | "name") => rt.get_field(a(0), "$name"),
        // An SLF4J marker, by name.
        ("org/slf4j/MarkerFactory", "getMarker" | "getDetachedMarker") => {
            let m = rt.new_object("org/slf4j/Marker");
            rt.put_field(m, "$name", a(0));
            m
        }
        ("org/slf4j/Marker", "getName") => rt.get_field(a(0), "$name"),
        // A mod's own loot parameters and their sets (`new LootContextParamSet.Builder()
        // .required(LootContextParams.THIS_ENTITY).build()`): what each holds, by identity.
        (LOOT_PARAM, "<init>") => {
            rt.put_field(a(0), "$name", a(1));
            V::Void
        }
        (LOOT_PARAM, "getName" | "toString") => rt.get_field(a(0), "$name"),
        (LOOT_PARAM_SET, "builder") => {
            let b = rt.new_object(LOOT_PARAM_SET_BUILDER);
            loot_param_builder(rt, b);
            b
        }
        (LOOT_PARAM_SET_BUILDER, "<init>") => {
            loot_param_builder(rt, a(0));
            V::Void
        }
        (LOOT_PARAM_SET_BUILDER, "required" | "optional") => {
            let list = rt.get_field(a(0), if name == "required" { "$required" } else { "$optional" });
            crate::api::call(rt, "java/util/List.add.(Ljava/lang/Object;)Z", &[list, a(1)]);
            a(0)
        }
        (LOOT_PARAM_SET_BUILDER, "build") => {
            let required = crate::jutil::elements(rt, rt.get_field(a(0), "$required"));
            let mut allowed = required.clone();
            allowed.extend(crate::jutil::elements(rt, rt.get_field(a(0), "$optional")));
            let set = rt.new_object(LOOT_PARAM_SET);
            let (r, al) = (crate::jutil::new_set_of(rt, &required), crate::jutil::new_set_of(rt, &allowed));
            rt.put_field(set, "$required", r);
            rt.put_field(set, "$allowed", al);
            set
        }
        (LOOT_PARAM_SET, "getRequired" | "getAllowed") => match rt.get_field(a(0), if name == "getRequired" { "$required" } else { "$allowed" }) {
            s if s.r() != 0 => s,
            _ => crate::jutil::new_set_of(rt, &[]),
        },
        // A vanilla set (`LootContextParamSets.BLOCK`) allows what the game gives it.
        (LOOT_PARAM_SET, "isAllowed") => match rt.get_field(a(0), "$allowed") {
            s if s.r() != 0 => crate::api::call(rt, "java/util/Set.contains.(Ljava/lang/Object;)Z", &[s, a(1)]),
            _ => V::I(1),
        },
        (LOOT_PARAM_SET, "validateUser") => V::Void,
        // `PotionContents.withEffectAdded(effect)`: the same contents with the effect after its own.
        ("net/minecraft/world/item/alchemy/PotionContents", "withEffectAdded") => {
            let mut effects = crate::jutil::elements(rt, rt.get_field(a(0), "customEffects"));
            effects.push(a(1));
            let list = crate::jutil::new_list(rt, effects);
            let (potion, color) = (rt.get_field(a(0), "potion"), rt.get_field(a(0), "customColor"));
            crate::records::make(rt, "net/minecraft/world/item/alchemy/PotionContents", &[potion, color, list])
        }
        // `Tool.Rule`'s factories: the blocks (a tag's key, or a list), the speed and whether they
        // drop, as each sets them.
        ("net/minecraft/world/item/component/Tool$Rule", "deniesDrops" | "minesAndDrops" | "overrideSpeed") => {
            let speed = if name == "deniesDrops" { V::NULL } else { V::F(a(1).f()) };
            let drops = match name {
                "deniesDrops" => V::I(0),
                "minesAndDrops" => V::I(1),
                _ => V::NULL,
            };
            let (speed, drops) = (optional(rt, speed), optional(rt, drops));
            crate::records::make(rt, "net/minecraft/world/item/component/Tool$Rule", &[a(0), speed, drops])
        }
        // NeoForge's `ConcatenatedListView.of(lists...)`: their elements in order.
        ("net/neoforged/neoforge/common/util/ConcatenatedListView", "of") => {
            let mut items = Vec::new();
            for l in crate::jutil::elements(rt, a(0)) {
                items.extend(crate::jutil::elements(rt, l));
            }
            crate::jutil::new_list(rt, items)
        }
        // NeoForge's `new IngredientType(mapCodec)`: its stream codec the map codec's over the wire
        // (`ByteBufCodecs.fromCodecWithRegistries(mapCodec.codec())`).
        ("net/neoforged/neoforge/common/crafting/IngredientType" | "net/neoforged/neoforge/fluids/crafting/FluidIngredientType", "<init>")
            if desc == "(Lcom/mojang/serialization/MapCodec;)V" =>
        {
            let codec = crate::api::call(rt, "com/mojang/serialization/MapCodec.codec.()Lcom/mojang/serialization/Codec;", &[a(1)]);
            let stream = crate::network::from_codec(rt, codec);
            rt.put_field(a(0), "codec", a(1));
            rt.put_field(a(0), "streamCodec", stream);
            V::Void
        }
        // Sets of stacks by type and components, and the hash they keep them by.
        ("net/minecraft/world/item/ItemStackLinkedSet", "createTypeAndComponentsSet") => strategy_set(rt, ITEM_STRATEGY),
        ("net/neoforged/neoforge/fluids/FluidStackLinkedSet", "createTypeAndComponentsSet") => strategy_set(rt, FLUID_STRATEGY),
        ("net/minecraft/world/item/ItemStackLinkedSet", "hashStackAndComponents") | ("net/minecraft/world/item/ItemStack", "hashItemAndComponents") => {
            V::I(stack_hash(rt, a(0), false))
        }
        ("net/neoforged/neoforge/fluids/FluidStack", "hashFluidAndComponents") => V::I(stack_hash(rt, a(0), true)),
        // The missing texture's sprite name (`minecraft:missingno`), which a mod compares a sprite with.
        ("net/minecraft/client/renderer/texture/MissingTextureAtlasSprite", "getLocation") => rt.string("minecraft:missingno"),
        // `BuiltInLootTables.register(key)` adds a table's key to vanilla's set and answers it.
        ("net/minecraft/world/level/storage/loot/BuiltInLootTables", "register") => a(0),
        // `LazyLoadedValue(supplier)`: the supplier asked once, at the first `get()`.
        ("net/minecraft/util/LazyLoadedValue", "<init>") => {
            rt.put_field(a(0), "$supplier", a(1));
            V::Void
        }
        ("net/minecraft/util/LazyLoadedValue", "get") => match rt.get_field(a(0), "$value") {
            V::Void => {
                let s = rt.get_field(a(0), "$supplier");
                let v = rt.call_functional("java/util/function/Supplier.get.()Ljava/lang/Object;", s, &[]);
                if rt.thrown.is_none() {
                    rt.put_field(a(0), "$value", v);
                }
                v
            }
            v => v,
        },
        // Flywheel's mixins onto vanilla types (`flywheel$setVisualizer`...): its instancing is off
        // (`VisualizationManager.supportsVisualization` is false), so they keep nothing.
        // Flywheel's `TransformStack.of(poseStack)`: its own wrapper over the stack.
        (_, "flywheel$transformStack") => {
            let t = rt.new_object("dev/engine_room/flywheel/lib/transform/PoseTransformStack");
            rt.put_field(t, "stack", a(0));
            t
        }
        (_, n) if n.starts_with("flywheel$") => {
            if desc.ends_with(")V") {
                V::Void
            } else if desc.ends_with(")Z") {
                V::I(0)
            } else {
                V::NULL
            }
        }
        // `getProperties()`: the state's properties, in its definition's order (by name): a mod
        // block's own objects, else vanilla's `BlockStateProperties` of each name and value.
        (STATE | BASE | HOLDER, "getProperties") => {
            let items = state_properties(rt, a(0));
            crate::jutil::new_list(rt, items)
        }
        // `getValues()`: each property to the value the state holds, in the same order.
        (STATE | BASE | HOLDER, "getValues") => {
            let text = name_of(rt, a(0));
            let (_, props) = crate::host::split_state(&text);
            let props: Vec<(String, String)> = props
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect();
            let m = rt.alloc("java/util/LinkedHashMap", Data::Table(Default::default()));
            for p in state_properties(rt, a(0)) {
                let (pname, kind) = prop_parts(rt, p);
                if let Some((_, text)) = props.iter().find(|(k, _)| *k == pname) {
                    let v = prop_value(rt, p, &kind, text);
                    crate::jutil::table_put(rt, m, p, v);
                }
            }
            m
        }
        // Block-state properties
        (STATE | BASE | HOLDER, "getValue" | "getOptionalValue" | "hasProperty" | "setValue" | "cycle" | "trySetValue") => {
            let text = name_of(rt, a(0));
            let (block, props) = crate::host::split_state(&text);
            let (pname, kind) = prop_parts(rt, a(1));
            let current = props.iter().find(|(k, _)| *k == pname).map(|(_, v)| v.to_string());
            match name {
                "hasProperty" => V::I(current.is_some() as i32),
                "getValue" => {
                    // Read back by `glue::render_shape`: an answer that rests on a property the
                    // game's state doesn't have isn't the block's.
                    if current.is_none() {
                        rt.interned.insert("compat:missing-property".into(), V::I(1));
                    }
                    prop_value(rt, a(1), &kind, current.as_deref().unwrap_or(""))
                }
                "getOptionalValue" => {
                    let v = current.map(|c| prop_value(rt, a(1), &kind, &c)).unwrap_or(V::NULL);
                    let o = rt.new_object("java/util/Optional");
                    rt.put_field(o, "value", v);
                    o
                }
                "setValue" | "trySetValue" => {
                    if current.is_none() && name == "trySetValue" {
                        return Some(a(0));
                    }
                    let new = crate::blocks::property_text(rt, a(1), a(2));
                    let mut p: Vec<(&str, &str)> = props.iter().filter(|(k, _)| *k != pname).copied().collect();
                    p.push((&pname, &new));
                    let t = crate::host::join_state(block, p);
                    state(rt, &t)
                }
                _ => {
                    // cycle: the property's next value, the first after the last.
                    let c = current.unwrap_or_default();
                    let values = crate::blocks::property_values(rt, a(1));
                    let next = match values.iter().position(|v| *v == c) {
                        Some(i) => values[(i + 1) % values.len()].clone(),
                        None => return None,
                    };
                    let mut p: Vec<(&str, &str)> = props.iter().filter(|(k, _)| *k != pname).copied().collect();
                    p.push((&pname, &next));
                    let t = crate::host::join_state(block, p);
                    state(rt, &t)
                }
            }
        }
        (o, "create")
            if o.starts_with("net/minecraft/world/level/block/state/properties/")
                && (o.ends_with("BooleanProperty") || o.ends_with("IntegerProperty") || o.ends_with("EnumProperty") || o.ends_with("DirectionProperty")) =>
        {
            let pname = name_of(rt, a(0));
            let kind = if o.ends_with("BooleanProperty") {
                "bool".to_owned()
            } else if o.ends_with("IntegerProperty") {
                "int".to_owned()
            } else if o.ends_with("DirectionProperty") {
                "direction".to_owned()
            } else {
                // EnumProperty.create(name, Class): the class literal is a String object holding its name.
                let class = name_of(rt, a(1));
                if class == "net/minecraft/core/Direction$Axis" { "axis".to_owned() } else if class == "net/minecraft/core/Direction" { "direction".to_owned() } else { class }
            };
            // Each `create` makes its own property, as Java's: two mods' `power` may differ.
            let p = new_property(rt, &pname, &kind);
            crate::blocks::created_property(rt, p, o, desc, args);
            p
        }
        // A mod's own property class (`extends Property<T>`, or one of vanilla's kinds): its name
        // and kind as `create` gives them. Its values are its `getPossibleValues`, spelled by its
        // `getName` (`blocks::property_values`).
        (o, "<init>") if o.starts_with(PROPERTIES) && o.ends_with("Property") && args.len() >= 2 => {
            let this = a(0);
            let kind = match o.rsplit('/').next().unwrap_or("") {
                "BooleanProperty" => "bool".to_owned(),
                "IntegerProperty" => "int".to_owned(),
                "DirectionProperty" => "direction".to_owned(),
                // `Property(name, Class)`, `EnumProperty(name, Class, values)`.
                _ => match name_of(rt, a(2)).as_str() {
                    "java/lang/Boolean" => "bool".to_owned(),
                    "java/lang/Integer" => "int".to_owned(),
                    "net/minecraft/core/Direction" => "direction".to_owned(),
                    "net/minecraft/core/Direction$Axis" => "axis".to_owned(),
                    class => class.to_owned(),
                },
            };
            rt.put_field(this, "$name", a(1));
            let k = rt.string(&kind);
            rt.put_field(this, "$kind", k);
            crate::blocks::created_property(rt, this, o, desc, &args[1..]);
            V::Void
        }
        (_, "getName") if owner.starts_with(PROPERTIES) && desc.starts_with("()") => rt.get_field(a(0), "$name"),
        // `getName(value)`: the value as a state spells it.
        (_, "getName") if owner.starts_with(PROPERTIES) && owner.ends_with("Property") => {
            let t = text_of(rt, a(1));
            rt.string(&t)
        }
        // `getValue(text)`: the value a state spelling `text` holds, if the property takes it.
        (_, "getValue") if owner.starts_with(PROPERTIES) && owner.ends_with("Property") && desc == "(Ljava/lang/String;)Ljava/util/Optional;" => {
            let text = name_of(rt, a(1));
            let v = if crate::blocks::property_values(rt, a(0)).contains(&text) {
                let (_, kind) = prop_parts(rt, a(0));
                value_of(rt, &kind, &text)
            } else {
                V::NULL
            };
            optional(rt, v)
        }
        // `Property.equals`: the same name and value class (what a mod's override calls first).
        (PROPERTY, "equals") => V::I((a(0) == a(1) || (a(1).r() != 0 && prop_parts(rt, a(0)) == prop_parts(rt, a(1)))) as i32),
        (PROPERTY, "generateHashCode" | "hashCode") => {
            let (n, k) = prop_parts(rt, a(0));
            let h = |s: &str| s.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32));
            V::I(h(&k).wrapping_mul(31).wrapping_add(h(&n)))
        }
        // The values in Java's order, as Java holds them: `Integer`s, `Boolean`s, constants.
        (_, "getPossibleValues") if owner.starts_with("net/minecraft/world/level/block/state/properties/") => {
            let (_, kind) = prop_parts(rt, a(0));
            let texts = crate::blocks::property_values(rt, a(0));
            let values: Vec<V> = texts.iter().map(|t| value_of(rt, &kind, t)).collect();
            crate::jutil::new_list(rt, values)
        }
        ("java/lang/Boolean", "valueOf") if desc == "(Z)Ljava/lang/Boolean;" => boolean(rt, a(0).i() != 0),
        // A boolean a method reference gave back raw (`Optional.map(BeltBlockEntity::hasPulley)`,
        // where Java boxes it) is its own value, as raw ints are in `number`.
        ("java/lang/Boolean", "booleanValue") => match a(0) {
            V::R(_) => rt.get_field(a(0), "value"),
            raw => raw,
        },
        ("java/lang/Integer", "valueOf") if desc == "(I)Ljava/lang/Integer;" => integer(rt, a(0).i()),
        ("java/lang/Enum", "<init>") => {
            let n = a(1);
            rt.put_field(a(0), "$name", n);
            rt.put_field(a(0), "$ordinal", a(2));
            V::Void
        }
        ("java/lang/Enum", "name" | "toString") => enum_name(rt, a(0), name == "toString"),
        ("java/lang/Enum", "ordinal") => V::I(enum_ordinal(rt, a(0))),
        ("java/util/Optional", _) => return optional_call(rt, name, desc, args),

        // Directions and axes
        ("net/minecraft/core/Direction", "getAxisDirection") => {
            let o = native(rt, a(0))[0];
            let positive = matches!(o, 1 | 3 | 5);
            rt.intern(format!("axisdir/{positive}"), |rt| {
                let v = rt.alloc("net/minecraft/core/Direction$AxisDirection", Data::Native("AxisDirection", [positive as i64, 0, 0, 0]));
                rt.put_field(v, "$ordinal", V::I(if positive { 0 } else { 1 }));
                v
            })
        }
        ("net/minecraft/core/Direction", "getNormal") => {
            let d = crate::api::DIRECTIONS[native(rt, a(0))[0].clamp(0, 5) as usize];
            crate::api::block_pos(rt, d[0], d[1], d[2])
        }
        // `step()`: the normal as a new `Vector3f`.
        ("net/minecraft/core/Direction", "step") => {
            let d = crate::api::DIRECTIONS[native(rt, a(0))[0].clamp(0, 5) as usize];
            let v = rt.new_object("org/joml/Vector3f");
            for (f, c) in ["x", "y", "z"].into_iter().zip(d) {
                rt.put_field(v, f, V::F(c as f32));
            }
            v
        }
        // `getRotation()`: a new quaternion turning up to the direction, as vanilla builds it.
        ("net/minecraft/core/Direction", "getRotation") => {
            let q = direction_rotation(native(rt, a(0))[0].clamp(0, 5) as usize);
            crate::render::new_quat(rt, q)
        }
        // `fromDelta(x, y, z)`: the direction whose normal points that way along one axis, null for
        // a delta on none or more than one.
        ("net/minecraft/core/Direction", "fromDelta") => {
            let o = match (a(0).i().signum(), a(1).i().signum(), a(2).i().signum()) {
                (0, -1, 0) => 0,
                (0, 1, 0) => 1,
                (0, 0, -1) => 2,
                (0, 0, 1) => 3,
                (-1, 0, 0) => 4,
                (1, 0, 0) => 5,
                _ => return Some(V::NULL),
            };
            direction(rt, o)
        }
        // `byName(name)`: the direction its serialized name names (`north`), else null.
        ("net/minecraft/core/Direction", "byName") => {
            let found = rt
                .str_of(a(0))
                .and_then(|n| DIRECTION_NAMES.iter().position(|d| *d == n));
            found.map_or(V::NULL, |o| direction(rt, o as i64))
        }
        ("net/minecraft/core/Direction", "name") => {
            let n = DIRECTION_NAMES[native(rt, a(0))[0].clamp(0, 5) as usize].to_ascii_uppercase();
            rt.string(&n)
        }
        ("net/minecraft/core/Direction", "getName" | "getSerializedName" | "toString") => {
            let n = DIRECTION_NAMES[native(rt, a(0))[0].clamp(0, 5) as usize];
            rt.string(n)
        }
        ("net/minecraft/core/Direction", "stream") => {
            let all: Vec<V> = (0..6).map(|i| direction(rt, i)).collect();
            rt.alloc("java/util/stream/Stream", Data::Stream(all))
        }
        ("net/minecraft/core/Direction", "values") => {
            let arr = rt.new_array(6, V::NULL);
            for i in 0..6 {
                let d = direction(rt, i);
                rt.array_set(arr, i as i32, d);
            }
            arr
        }
        ("net/minecraft/core/Direction", "fromAxisAndDirection") => {
            let ax = native(rt, a(0))[0];
            let positive = native(rt, a(1))[0] != 0;
            let o = match ax {
                0 => if positive { 5 } else { 4 },
                1 => if positive { 1 } else { 0 },
                _ => if positive { 3 } else { 2 },
            };
            direction(rt, o)
        }
        ("net/minecraft/core/Direction", "get") if desc.starts_with("(Lnet/minecraft/core/Direction$AxisDirection;Lnet/minecraft/core/Direction$Axis;") => {
            let positive = native(rt, a(0))[0] != 0;
            let o = match native(rt, a(1))[0] {
                0 => if positive { 5 } else { 4 },
                1 => if positive { 1 } else { 0 },
                _ => if positive { 3 } else { 2 },
            };
            direction(rt, o)
        }
        // `Direction.getNearest(x, y, z)`: the largest dot product with a direction's normal, the first
        // on ties, in `Direction`'s order; north unless one is above `Float.MIN_VALUE`.
        ("net/minecraft/core/Direction", "getNearest")
            if args.len() >= 3 || desc.starts_with("(Lnet/minecraft/world/phys/Vec3;") =>
        {
            let f = |v: V| match v {
                V::I(i) => i as f32,
                V::F(x) => x as f32,
                V::D(x) => x as f32,
                V::J(j) => j as f32,
                _ => 0.0,
            };
            let (x, y, z) = if args.len() >= 3 {
                (f(a(0)), f(a(1)), f(a(2)))
            } else {
                let p = xyz(rt, a(0));
                (p[0] as f32, p[1] as f32, p[2] as f32)
            };
            let (mut best, mut at) = (f32::from_bits(1), 2);
            for (i, n) in crate::api::DIRECTIONS.iter().enumerate() {
                let dot = x * n[0] as f32 + y * n[1] as f32 + z * n[2] as f32;
                if dot > best {
                    best = dot;
                    at = i as i64;
                }
            }
            direction(rt, at)
        }
        ("net/minecraft/core/Direction$Axis", "choose") => args.get(1 + native(rt, a(0))[0].clamp(0, 2) as usize).copied().unwrap_or_default(),
        ("net/minecraft/core/Direction$Axis", "getName" | "getSerializedName" | "toString") => {
            let n = AXIS_NAMES[native(rt, a(0))[0].clamp(0, 2) as usize];
            rt.string(n)
        }
        ("net/minecraft/core/Direction$Axis", "test") => {
            let d = native(rt, a(1))[0];
            V::I((a(1).r() != 0 && [1, 1, 2, 2, 0, 0][d.clamp(0, 5) as usize] == native(rt, a(0))[0]) as i32)
        }
        // `Axis.getPlane()`: y is vertical, x and z horizontal.
        ("net/minecraft/core/Direction$Axis", "getPlane") => {
            let key = if native(rt, a(0))[0] == 1 {
                "net/minecraft/core/Direction$Plane.VERTICAL"
            } else {
                "net/minecraft/core/Direction$Plane.HORIZONTAL"
            };
            crate::api::get_static(rt, key)
        }
        // `Axis.byName(name)`: `x`, `y` or `z`, else null.
        ("net/minecraft/core/Direction$Axis", "byName") => {
            let found = rt
                .str_of(a(0))
                .and_then(|n| AXIS_NAMES.iter().position(|x| *x == n));
            found.map_or(V::NULL, |o| axis(rt, o as i64))
        }
        // `AxisDirection.values()`: positive, then negative.
        ("net/minecraft/core/Direction$AxisDirection", "values") => {
            let arr = rt.new_array(2, V::NULL);
            for (i, key) in [
                "net/minecraft/core/Direction$AxisDirection.POSITIVE",
                "net/minecraft/core/Direction$AxisDirection.NEGATIVE",
            ]
            .into_iter()
            .enumerate()
            {
                let v = crate::api::get_static(rt, key);
                rt.array_set(arr, i as i32, v);
            }
            arr
        }
        ("net/minecraft/core/Direction$AxisDirection", "opposite") => {
            let positive = native(rt, a(0))[0] == 0;
            rt.intern(format!("axisdir/{positive}"), |rt| {
                let v = rt.alloc("net/minecraft/core/Direction$AxisDirection", Data::Native("AxisDirection", [positive as i64, 0, 0, 0]));
                rt.put_field(v, "$ordinal", V::I(if positive { 0 } else { 1 }));
                v
            })
        }
        // Planes: the horizontal one holds the four sides, the vertical one up and down.
        ("net/minecraft/core/Direction$Plane", "test") => {
            let horizontal = native(rt, a(0))[0] == 0;
            V::I((a(1).r() != 0 && (native(rt, a(1))[0] >= 2) == horizontal) as i32)
        }
        // Walked in vanilla's order: north, east, south, west; up, down.
        ("net/minecraft/core/Direction$Plane", "iterator" | "stream" | "forEach" | "shuffledCopy" | "length" | "getRandomDirection") => {
            let faces: &[i64] = if native(rt, a(0))[0] == 0 { &[2, 5, 3, 4] } else { &[1, 0] };
            let mut ds: Vec<V> = faces.iter().map(|&o| direction(rt, o)).collect();
            match name {
                "iterator" => crate::jutil::new_iter(rt, ds),
                "stream" => rt.alloc("java/util/stream/Stream", Data::Stream(ds)),
                "length" => V::I(ds.len() as i32),
                "getRandomDirection" => {
                    let i = (next_random(rt) >> 33) as usize % ds.len();
                    ds[i]
                }
                "shuffledCopy" => {
                    for i in (1..ds.len()).rev() {
                        let j = (next_random(rt) >> 33) as usize % (i + 1);
                        ds.swap(i, j);
                    }
                    crate::jutil::new_list(rt, ds)
                }
                _ => {
                    for d in ds {
                        rt.call_functional("java/util/function/Consumer.accept.(Ljava/lang/Object;)V", a(1), &[d]);
                        if rt.thrown.is_some() {
                            break;
                        }
                    }
                    V::Void
                }
            }
        }
        ("net/minecraft/core/Direction$Axis", "isVertical") => V::I((native(rt, a(0))[0] == 1) as i32),
        ("net/minecraft/core/Direction$Axis", "isHorizontal") => V::I((native(rt, a(0))[0] != 1) as i32),
        ("net/minecraft/core/Direction$Axis", "ordinal") => V::I(native(rt, a(0))[0] as i32),
        ("net/minecraft/core/Direction$Axis", "values") => {
            let arr = rt.new_array(3, V::NULL);
            for i in 0..3 {
                let x = axis(rt, i);
                rt.array_set(arr, i as i32, x);
            }
            arr
        }
        ("net/minecraft/core/Direction$AxisDirection", "getStep") => V::I(if native(rt, a(0))[0] != 0 { 1 } else { -1 }),

        // Players and interactions
        // What the player holds or wears in a slot: the hands' stacks as `getItemInHand`'s, the
        // armor's read from the game.
        // NeoForge's data a mod keeps with the player, on its object.
        // A player's identity: their UUID (a server's player's own, else the one the host acts
        // for), their name as the game gave it at their login, and their profile of both.
        // A server's player's entity id: the game's now, by their UUID (Create names the player it
        // syncs a client with so), else as the game last told it (`damage.rs`); 0 before. A
        // client's own player's, as the server gave it.
        (o, "getId") if desc == "()I" && is_player(o) && rt.class_of(a(0)) == SERVER_PLAYER => {
            let uuid = crate::network::player_uuid(rt, a(0));
            let now = uuid.and_then(|u| rt.host()?.player_entities().into_iter().find(|p| p.1 == u));
            V::I(now.map_or_else(|| rt.get_field(a(0), "$entityId").i(), |p| p.0))
        }
        (o, "getId") if desc == "()I" && is_player(o) && rt.class_of(a(0)) == LOCAL_PLAYER => V::I(rt.host().and_then(|h| h.client_player_id()).unwrap_or(0)),
        // Whether the window's own player stands on something, as the client's game knows it
        // (Create's turntable turns only a player standing on it); false before their world loads.
        (o, "onGround" | "$onGround") if desc == "()Z" && is_player(o) && rt.class_of(a(0)) == LOCAL_PLAYER => {
            V::I(i32::from(rt.host().and_then(|h| h.client_player_on_ground()).unwrap_or(false)))
        }
        // A player's type is `EntityType.PLAYER` (`BuiltInRegistries.ENTITY_TYPE.getKey` names it).
        (o, "getType") if desc == "()Lnet/minecraft/world/entity/EntityType;" && is_player(o) && is_player_class(rt.class_of(a(0))) => {
            crate::mod_entities::vanilla_type(rt, "minecraft:player")
        }
        (o, "getUUID" | "getStringUUID" | "getGameProfile" | "getScoreboardName") if is_player(o) && is_player_class(rt.class_of(a(0))) => {
            let (uuid, player_name) = player_identity(rt, a(0));
            match name {
                "getUUID" => uuid,
                "getStringUUID" => {
                    let (msb, lsb) = crate::tags::uuid_bits(rt, uuid);
                    rt.string(&crate::tags::uuid_text(msb, lsb))
                }
                "getScoreboardName" => player_name,
                _ => {
                    let g = rt.new_object("com/mojang/authlib/GameProfile");
                    rt.put_field(g, "$id", uuid);
                    rt.put_field(g, "$name", player_name);
                    g
                }
            }
        }
        (o, "getName" | "getDisplayName") if is_player(o) && is_player_class(rt.class_of(a(0))) && desc == "()Lnet/minecraft/network/chat/Component;" => {
            let (_, player_name) = player_identity(rt, a(0));
            crate::api::call(rt, "net/minecraft/network/chat/Component.literal.(Ljava/lang/String;)Lnet/minecraft/network/chat/MutableComponent;", &[player_name])
        }
        // The server a server's player plays on (a client's player has none).
        (o, "getServer") if is_player(o) && is_player_class(rt.class_of(a(0))) => {
            if crate::neo::is_client(rt) {
                V::NULL
            } else {
                rt.intern("compat:server".into(), |rt| rt.new_object("net/minecraft/server/MinecraftServer"))
            }
        }
        // `SectionPos`: a 16-block cube's coordinates (the block's shifted right 4), packed into a
        // long as vanilla packs them; its static helpers between blocks, sections and chunks.
        (SECTION_POS, n) => {
            let at = |rt: &Rt, v: V| native(rt, v).map(|c| c as i32);
            let section_of = |b: [i32; 3]| b.map(|c| c >> 4);
            let floor = |d: f64| d.floor() as i32;
            match (n, desc) {
                ("of", "(III)Lnet/minecraft/core/SectionPos;") => section_pos(rt, [a(0).i(), a(1).i(), a(2).i()]),
                ("of", "(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/core/SectionPos;") => {
                    let b = at(rt, a(0));
                    section_pos(rt, section_of([b[0], b[1], b[2]]))
                }
                ("of", "(Lnet/minecraft/world/level/ChunkPos;I)Lnet/minecraft/core/SectionPos;") => {
                    let (x, z) = (rt.get_field(a(0), "x").i(), rt.get_field(a(0), "z").i());
                    section_pos(rt, [x, a(1).i(), z])
                }
                ("of", "(Lnet/minecraft/core/Position;)Lnet/minecraft/core/SectionPos;") => {
                    let p = ["x", "y", "z"].map(|f| floor(rt.get_field(a(0), f).d()));
                    section_pos(rt, section_of(p))
                }
                ("of", "(Lnet/minecraft/world/level/entity/EntityAccess;)Lnet/minecraft/core/SectionPos;") => {
                    let b = crate::api::call(rt, "net/minecraft/world/entity/Entity.blockPosition.()Lnet/minecraft/core/BlockPos;", &[a(0)]);
                    let b = at(rt, b);
                    section_pos(rt, section_of([b[0], b[1], b[2]]))
                }
                ("of", "(J)Lnet/minecraft/core/SectionPos;") => section_pos(rt, section_of_long(a(0).j())),
                ("asLong", "(III)J") => V::J(section_long([a(0).i(), a(1).i(), a(2).i()])),
                ("asLong", "(Lnet/minecraft/core/BlockPos;)J") => {
                    let b = at(rt, a(0));
                    V::J(section_long(section_of([b[0], b[1], b[2]])))
                }
                ("asLong", "()J") => {
                    let s = at(rt, a(0));
                    V::J(section_long([s[0], s[1], s[2]]))
                }
                ("x" | "y" | "z", "(J)I") => V::I(section_of_long(a(0).j())[usize::from(n.as_bytes()[0] - b'x')]),
                ("x" | "y" | "z" | "getX" | "getY" | "getZ", "()I") => V::I(at(rt, a(0))[usize::from(n.as_bytes()[n.len() - 1].to_ascii_lowercase() - b'x')]),
                ("minBlockX" | "minBlockY" | "minBlockZ" | "maxBlockX" | "maxBlockY" | "maxBlockZ", "()I") => {
                    let axis = usize::from(n.as_bytes()[n.len() - 1] - b'X');
                    let edge = if n.starts_with("max") { 15 } else { 0 };
                    V::I((at(rt, a(0))[axis] << 4) + edge)
                }
                ("origin" | "center", _) => {
                    let s = at(rt, a(0));
                    let edge = if n == "center" { 8 } else { 0 };
                    let b: [i64; 3] = std::array::from_fn(|i| i64::from((s[i] << 4) + edge));
                    crate::api::block_pos(rt, b[0], b[1], b[2])
                }
                ("chunk", _) => {
                    let s = at(rt, a(0));
                    chunk_pos(rt, s[0], s[2])
                }
                ("offset", "(III)Lnet/minecraft/core/SectionPos;" | "(III)Lnet/minecraft/core/Vec3i;") => {
                    let s = at(rt, a(0));
                    section_pos(rt, [s[0].wrapping_add(a(1).i()), s[1].wrapping_add(a(2).i()), s[2].wrapping_add(a(3).i())])
                }
                ("offset", "(JIII)J" | "(JLnet/minecraft/core/Direction;)J") => {
                    let s = section_of_long(a(0).j());
                    let d = if desc == "(JIII)J" { [a(1).i(), a(2).i(), a(3).i()] } else { DIRECTION_STEPS[native(rt, a(1))[0].clamp(0, 5) as usize].map(|c| c as i32) };
                    V::J(section_long([s[0].wrapping_add(d[0]), s[1].wrapping_add(d[1]), s[2].wrapping_add(d[2])]))
                }
                ("blockToSectionCoord", "(I)I") => V::I(a(0).i() >> 4),
                ("blockToSectionCoord" | "posToSectionCoord", "(D)I") => V::I(floor(a(0).d()) >> 4),
                ("sectionToBlockCoord", "(I)I") => V::I(a(0).i() << 4),
                ("sectionToBlockCoord", "(II)I") => V::I((a(0).i() << 4).wrapping_add(a(1).i())),
                ("sectionRelative", "(I)I") => V::I(a(0).i() & 15),
                // A block's place in its section as a short: x in bits 8-11, z in 4-7, y in 0-3.
                ("sectionRelativePos", _) => {
                    let b = at(rt, a(0));
                    V::I(i32::from((((b[0] & 15) << 8) | ((b[2] & 15) << 4) | (b[1] & 15)) as i16))
                }
                ("sectionRelativeX" | "sectionRelativeY" | "sectionRelativeZ", "(S)I") => {
                    let shift = match n.as_bytes()[n.len() - 1] {
                        b'X' => 8,
                        b'Z' => 4,
                        _ => 0,
                    };
                    V::I(((a(0).i() as u32) >> shift) as i32 & 15)
                }
                ("relativeToBlockX" | "relativeToBlockY" | "relativeToBlockZ" | "relativeToBlockPos", "(S)I" | "(S)Lnet/minecraft/core/BlockPos;") => {
                    let (s, r) = (at(rt, a(0)), a(1).i() as u32);
                    let b = [(s[0] << 4) + ((r >> 8) & 15) as i32, (s[1] << 4) + (r & 15) as i32, (s[2] << 4) + ((r >> 4) & 15) as i32];
                    match n.as_bytes()[n.len() - 1] {
                        b'X' => V::I(b[0]),
                        b'Y' => V::I(b[1]),
                        b'Z' => V::I(b[2]),
                        _ => crate::api::block_pos(rt, i64::from(b[0]), i64::from(b[1]), i64::from(b[2])),
                    }
                }
                ("getZeroNode", "(II)J") => V::J(section_long([a(0).i(), 0, a(1).i()]) & -1_048_576),
                ("getZeroNode", "(J)J") => V::J(a(0).j() & -1_048_576),
                // From a packed block position (`BlockPos.asLong`) to its section's.
                ("blockToSection", "(J)J") => {
                    let l = a(0).j();
                    let b = [(l >> 38) as i32, ((l << 52) >> 52) as i32, ((l << 26) >> 38) as i32];
                    V::J(section_long(section_of(b)))
                }
                ("sectionToChunk", "(J)J") => {
                    let s = section_of_long(a(0).j());
                    V::J(chunk_long(s[0], s[2]))
                }
                _ => return None,
            }
        }
        // `ChunkPos`: a chunk's x and z in its public fields, packed into a long as vanilla packs
        // them; its blocks span 16 from x * 16.
        (CHUNK_POS, "asLong") if desc == "(II)J" => V::J(chunk_long(a(0).i(), a(1).i())),
        (CHUNK_POS, "asLong") if desc == "(Lnet/minecraft/core/BlockPos;)J" => {
            let [x, _, z] = crate::api::pos_of(rt, a(0));
            V::J(chunk_long(x >> 4, z >> 4))
        }
        (CHUNK_POS, "getX") if desc == "(J)I" => V::I(a(0).j() as i32),
        (CHUNK_POS, "getZ") if desc == "(J)I" => V::I((a(0).j() >> 32) as i32),
        (CHUNK_POS, "hash") => V::I(chunk_hash(a(0).i(), a(1).i())),
        (CHUNK_POS, "minFromRegion" | "maxFromRegion") => {
            let edge = if name == "minFromRegion" { 0 } else { 31 };
            chunk_pos(rt, (a(0).i() << 5) + edge, (a(1).i() << 5) + edge)
        }
        (CHUNK_POS, "<init>") => {
            let (x, z) = match desc {
                "(II)V" => (a(1).i(), a(2).i()),
                "(J)V" => (a(1).j() as i32, (a(1).j() >> 32) as i32),
                _ => {
                    let [x, _, z] = crate::api::pos_of(rt, a(1));
                    (x >> 4, z >> 4)
                }
            };
            rt.put_field(a(0), "x", V::I(x));
            rt.put_field(a(0), "z", V::I(z));
            V::Void
        }
        (CHUNK_POS, n) => {
            let (x, z) = (rt.get_field(a(0), "x").i(), rt.get_field(a(0), "z").i());
            let block = |at: i32, local: i32| (at << 4) + local;
            let other = |rt: &Rt, v: V| (rt.get_field(v, "x").i(), rt.get_field(v, "z").i());
            match n {
                "toLong" => V::J(chunk_long(x, z)),
                "hashCode" => V::I(chunk_hash(x, z)),
                "equals" => V::I(i32::from(rt.class_of(a(1)) == CHUNK_POS && other(rt, a(1)) == (x, z))),
                "toString" => rt.string(&format!("[{x}, {z}]")),
                "getMinBlockX" => V::I(block(x, 0)),
                "getMinBlockZ" => V::I(block(z, 0)),
                "getMaxBlockX" => V::I(block(x, 15)),
                "getMaxBlockZ" => V::I(block(z, 15)),
                "getMiddleBlockX" => V::I(block(x, 8)),
                "getMiddleBlockZ" => V::I(block(z, 8)),
                "getBlockX" => V::I(block(x, a(1).i())),
                "getBlockZ" => V::I(block(z, a(1).i())),
                "getRegionX" => V::I(x >> 5),
                "getRegionZ" => V::I(z >> 5),
                "getRegionLocalX" => V::I(x & 31),
                "getRegionLocalZ" => V::I(z & 31),
                "getWorldPosition" => crate::api::block_pos(rt, i64::from(block(x, 0)), 0, i64::from(block(z, 0))),
                "getMiddleBlockPosition" => crate::api::block_pos(rt, i64::from(block(x, 8)), i64::from(a(1).i()), i64::from(block(z, 8))),
                "getBlockAt" => crate::api::block_pos(rt, i64::from(block(x, a(1).i())), i64::from(a(2).i()), i64::from(block(z, a(3).i()))),
                "getChessboardDistance" | "distanceSquared" => {
                    let (ox, oz) = match desc {
                        "(II)I" => (a(1).i(), a(2).i()),
                        "(J)I" => (a(1).j() as i32, (a(1).j() >> 32) as i32),
                        _ => other(rt, a(1)),
                    };
                    let (dx, dz) = (ox.wrapping_sub(x), oz.wrapping_sub(z));
                    if n == "distanceSquared" {
                        V::I(dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz)))
                    } else {
                        V::I(dx.wrapping_abs().max(dz.wrapping_abs()))
                    }
                }
                _ => return None,
            }
        }
        (o, "getPersistentData") if is_player(o) && is_player_class(rt.class_of(a(0))) => crate::entities::persistent_data(rt, a(0)),
        (o, "getItemBySlot") if is_player(o) => match constant(rt, a(1)).as_str() {
            "MAINHAND" => crate::hand::stack(rt, crate::hand::MAIN),
            "OFFHAND" => crate::hand::stack(rt, crate::hand::OFF),
            slot => {
                let i = ["HEAD", "CHEST", "LEGS", "FEET"].iter().position(|s| *s == slot).unwrap_or(4) as u8;
                crate::hand::worn(rt, i)
            }
        },
        // A player's motion, in blocks a tick, as this side holds it: a client's own player moves by
        // it; a server's copy changes, as vanilla's does without `hurtMarked`. Another entity's push
        // (`push(Entity)`) is the game's own.
        (o, n @ ("getDeltaMovement" | "setDeltaMovement" | "addDeltaMovement" | "push"))
            if is_player(o)
                && is_player_class(rt.class_of(a(0)))
                && desc != "(Lnet/minecraft/world/entity/Entity;)V" =>
        {
            // A client holds its own player's motion alone (another's is the server's to send): any
            // UUID names them there.
            let own = rt.class_of(a(0)) == LOCAL_PLAYER;
            let client = rt.host().is_some_and(|h| h.client_player_id().is_some());
            let uuid = crate::network::player_uuid(rt, a(0))
                .or_else(|| rt.host().and_then(|h| h.player_id()))
                .or(own.then_some(0))
                .filter(|_| own || !client);
            let now = uuid.and_then(|u| rt.host_call(|h| h.player_motion(u)).flatten());
            if n == "getDeltaMovement" {
                let m = now.unwrap_or_default();
                vec3(rt, m[0], m[1], m[2])
            } else {
                let v = if desc.starts_with("(DDD)") {
                    [a(1).d(), a(2).d(), a(3).d()]
                } else {
                    ["x", "y", "z"].map(|k| rt.get_field(a(1), k).d())
                };
                // A push on a motion the host can't read (a hit's) is lost, as a set isn't.
                let to = if n == "setDeltaMovement" {
                    Some(v)
                } else {
                    now.map(|m| [m[0] + v[0], m[1] + v[1], m[2] + v[2]])
                };
                if let (Some(u), Some(to)) = (uuid, to) {
                    rt.host_call(|h| h.set_player_motion(u, to));
                }
                V::Void
            }
        }
        // `hurtMarked = true` on a server's player: their motion as the server holds it is sent
        // to their client, which moves them by it (a belt carries them), and the mark is spent,
        // as `ServerEntity.sendChanges` spends it. A client's own mark tells nobody.
        (o, "$hurtMarked")
            if is_player(o) && is_player_class(rt.class_of(a(0))) && desc == "(Z)V" =>
        {
            let client = rt.class_of(a(0)) == LOCAL_PLAYER
                || rt.host().is_some_and(|h| h.client_player_id().is_some());
            if a(1).i() != 0 && !client {
                let uuid = crate::network::player_uuid(rt, a(0))
                    .or_else(|| rt.host().and_then(|h| h.player_id()));
                if let Some(u) = uuid
                    && rt.host_call(|h| h.mark_player_motion(u)).unwrap_or(false)
                {
                    rt.put_field(a(0), "hurtMarked", V::I(0));
                }
            }
            V::Void
        }
        // Any other entity's mark: a mob's motion is sent to the players who see it as it
        // changes, and a mod's own entity keeps the field as written.
        (_, "$hurtMarked") if desc == "(Z)V" => V::Void,
        // A server's player moves where their client takes them: the motion a mod sets and marks
        // (`hurtMarked`) moves them there, and the server's copy follows what the client tells.
        (o, "move")
            if is_player(o)
                && is_player_class(rt.class_of(a(0)))
                && rt.class_of(a(0)) != LOCAL_PLAYER
                && desc
                    == "(Lnet/minecraft/world/entity/MoverType;Lnet/minecraft/world/phys/Vec3;)V" =>
        {
            V::Void
        }
        // A client moves its own player: a mod's move of them (Create's belt carries them on their
        // client) is a step of theirs, blocks stopping it.
        (o, "move")
            if is_player(o)
                && rt.class_of(a(0)) == LOCAL_PLAYER
                && desc
                    == "(Lnet/minecraft/world/entity/MoverType;Lnet/minecraft/world/phys/Vec3;)V" =>
        {
            let movement = ["x", "y", "z"].map(|k| rt.get_field(a(2), k).d());
            rt.host_call(|h| h.move_client_player(movement));
            V::Void
        }
        // A client's own player walks by its keys (`LocalPlayer.aiStep` sets `xxa` and `zza`): a
        // belt holds to its middle a player who doesn't walk.
        (o, n @ ("$xxa" | "$yya" | "$zza"))
            if is_player(o) && rt.class_of(a(0)) == LOCAL_PLAYER =>
        {
            let input = rt
                .host()
                .and_then(|h| h.client_player_input())
                .unwrap_or_default();
            V::F(match n {
                "$xxa" => input[0],
                "$zza" => input[1],
                _ => 0.0,
            })
        }
        // Where a player stands is their physics' (`setOnGround`); a walking player's inputs reach a
        // server only while riding (`$xxa`, `$zza`); they step as `STEP_HEIGHT`'s default says, carry
        // no passengers here, and a piston (or a belt) pushes them.
        (o, "setOnGround")
            if is_player(o) && is_player_class(rt.class_of(a(0))) && desc == "(Z)V" =>
        {
            V::Void
        }
        (o, "$xxa" | "$yya" | "$zza")
            if is_player(o)
                && is_player_class(rt.class_of(a(0)))
                && rt.class_of(a(0)) != LOCAL_PLAYER =>
        {
            V::F(0.0)
        }
        (o, "maxUpStep") if is_player(o) && is_player_class(rt.class_of(a(0))) => V::F(0.6),
        (o, "getPassengers") if is_player(o) && is_player_class(rt.class_of(a(0))) => {
            crate::jutil::new_list(rt, Vec::new())
        }
        (o, "getPistonPushReaction") if is_player(o) && is_player_class(rt.class_of(a(0))) => {
            crate::blocks::push_reaction_named(rt, "normal")
        }
        // Invisible by the effect, as `Player.updateInvisibilityStatus` makes a player (a spectator
        // is too, and none acts here).
        (o, "isInvisible") if is_player(o) && is_player_class(rt.class_of(a(0))) => {
            let invisible = crate::mob_effects::has(rt, a(0), "minecraft:invisibility");
            V::I(i32::from(invisible))
        }
        // `hurt(source, amount)` on a server's player, through the game: nothing in creative, else
        // their frames, a raised shield, armor and effects, the knockback and the sound. The game's
        // damage hooks hear the mod's own source.
        (o, "hurt" | "hurtServer")
            if is_player(o) && is_player_class(rt.class_of(a(0))) && desc.ends_with("F)Z") =>
        {
            let (source, amount) = (a(args.len() - 2), a(args.len() - 1).f());
            let blow = crate::damage::blow_of(rt, source);
            let uuid = crate::network::player_uuid(rt, a(0))
                .or_else(|| rt.host().and_then(|h| h.player_id()));
            let deal = move |rt: &mut Rt| {
                uuid.is_some_and(|u| {
                    rt.host_call(|h| h.hurt_player_by(u, amount, &blow))
                        .unwrap_or(false)
                })
            };
            let hurt = match rt.get_field(a(0), "$entityId") {
                V::I(id) => crate::damage::dealing(rt, id, source, deal),
                _ => deal(rt),
            };
            V::I(i32::from(hurt))
        }
        (o, n)
            if is_player(o)
                && matches!(
                    n,
                    "isShiftKeyDown"
                        | "isCrouching"
                        | "isSteppingCarefully"
                        | "isSecondaryUseActive"
                        | "isCreative"
                        | "hasInfiniteMaterials"
                        | "isSpectator"
                        | "getItemInHand"
                        | "getMainHandItem"
                        | "getOffhandItem"
                        | "getWeaponItem"
                        | "level"
                        | "getCommandSenderWorld"
                        | "serverLevel"
                        | "isLocalPlayer"
                        | "mayBuild"
                        | "$clientLevel"
                ) =>
        {
            match n {
                // Survival and creative players build; the hooks act for one of them.
                "mayBuild" => V::I(1),
                // `Player.isSecondaryUseActive` is its shift key; `hasInfiniteMaterials`, creative's
                // `instabuild` ability.
                "isShiftKeyDown"
                | "isCrouching"
                | "isSteppingCarefully"
                | "isSecondaryUseActive" => {
                    V::I(rt.host().is_some_and(|h| h.player_sneaking()) as i32)
                }
                "isCreative" => V::I(rt.host().is_some_and(|h| h.player_creative()) as i32),
                // Only a player builds from infinite materials: other living entities answer false.
                "hasInfiniteMaterials" => V::I(
                    (is_player_class(rt.class_of(a(0)))
                        && rt.host().is_some_and(|h| h.player_creative()))
                        as i32,
                ),
                "isSpectator" => V::I(0),
                // The client's own player (`Minecraft.player`) is in the client's level.
                "isLocalPlayer" => V::I(i32::from(rt.class_of(a(0)) == LOCAL_PLAYER)),
                // `LocalPlayer.clientLevel`, through a mixin accessor: the client's level.
                "$clientLevel" => crate::glue::level(rt),
                // A server player's level is the hooks' one level (what mods compare and key their
                // data by), but on a client's runtime, where that one is the client's.
                "level" | "getCommandSenderWorld" | "serverLevel" if rt.class_of(a(0)) == LOCAL_PLAYER || !crate::neo::is_client(rt) => crate::glue::level(rt),
                "level" | "getCommandSenderWorld" | "serverLevel" => rt.intern("compat:server-level".into(), |rt| {
                    let l = rt.new_object("net/minecraft/server/level/ServerLevel");
                    rt.put_field(l, "isClientSide", V::I(0));
                    l
                }),
                _ => {
                    // One stack object for each of the player's hands through the hook, as
                    // vanilla's: what a mod changes in it, it finds there again, and the game's
                    // stack gets it.
                    let off = name == "getOffhandItem" || (name == "getItemInHand" && rt.get_field(a(1), "$ordinal").i() == 1);
                    crate::hand::stack(rt, if off { crate::hand::OFF } else { crate::hand::MAIN })
                }
            }
        }
        // Messages for a player, in their chat or over the hotbar: the acting player's, and another
        // server player's by their UUID (a mob's killer told of its death, with nobody acting).
        (o, "displayClientMessage" | "sendSystemMessage") if is_player(o) => {
            let json = component_json(rt, a(1)).to_string();
            let overlay = name == "displayClientMessage" && a(2).i() != 0;
            let to = crate::network::player_uuid(rt, a(0));
            if let Some(h) = rt.host_mut() {
                let other = to.filter(|uuid| h.player_id() != Some(*uuid));
                if !other.is_some_and(|uuid| h.tell_player(uuid, &json, overlay)) {
                    h.player_message(&json, overlay);
                }
            }
            V::Void
        }
        // `Item.getPlayerPOVHitResult(level, player, fluid)`: the block the player looks at within
        // reach, else a miss where their reach ends (`BlockHitResult.miss`).
        ("net/minecraft/world/item/Item", "getPlayerPOVHitResult") => {
            let fluids = match constant(rt, a(2)).as_str() {
                "SOURCE_ONLY" => 1,
                "ANY" | "WATER" => 2,
                _ => 0,
            };
            pov_hit_result(rt, fluids)
        }
        // Food: the acting player's, through the host (work by hand, as Create's hand crank, tires
        // the player); `FoodData`'s methods are `vitals`'.
        (o, "getFoodData") if is_player(o) => rt.intern("compat:food-data".into(), |rt| rt.new_object(FOOD_DATA)),
        (o, "causeFoodExhaustion") if is_player(o) => {
            let amount = a(1).f();
            if let Some(h) = rt.host_mut() {
                h.exhaust_player(amount);
            }
            V::Void
        }
        // The hand holds the new stack.
        (o, "setItemInHand") if is_player(o) => {
            let hand = if rt.get_field(a(1), "$ordinal").i() == 1 { crate::hand::OFF } else { crate::hand::MAIN };
            crate::hand::replace(rt, hand, a(2));
            V::Void
        }
        // `ItemInHandRenderer`: the stacks the first person hands show (`mainHandItem`,
        // `offHandItem`: the hands' own; the game draws them itself).
        ("net/minecraft/client/renderer/entity/EntityRenderDispatcher", "getItemInHandRenderer") => {
            let r = rt.new_object("net/minecraft/client/renderer/ItemInHandRenderer");
            let (main, off) = (crate::hand::stack(rt, crate::hand::MAIN), crate::hand::stack(rt, crate::hand::OFF));
            rt.put_field(r, "mainHandItem", main);
            rt.put_field(r, "offHandItem", off);
            r
        }
        (o, "addItem") if is_player(o) => V::I(give_player(rt, a(1)) as i32),
        // An item's `useOn` that reaches vanilla's `Item`: nothing happens.
        ("net/minecraft/world/item/Item", "useOn") => result(rt, "net/minecraft/world/InteractionResult", "PASS"),
        // A mod's block item calling `super.useOn(context)`, or `place(context)` (vanilla's
        // `useOn` is `place(new BlockPlaceContext(context))`; Create's deployer and schematic
        // cannon place through it): the stack in the context's hand placed, as the game places it.
        ("net/minecraft/world/item/BlockItem", "useOn" | "place") => {
            let stack = rt.get_field(a(1), "$item");
            crate::item_use::block_item_use_on(rt, stack, a(1))
        }
        // `BlockItem.getPlacementState(context)`: its block's state for the click
        // (`getStateForPlacement`, a mod block's own), kept only where `canPlace` lets it.
        ("net/minecraft/world/item/BlockItem", "getPlacementState") => {
            let block = crate::api::virtual_call(rt, BLOCK_ITEM_GET_BLOCK, &[a(0)]);
            let state = if block.r() != 0 {
                crate::api::virtual_call(rt, STATE_FOR_PLACEMENT, &[block, a(1)])
            } else {
                V::NULL
            };
            if state.r() != 0
                && crate::api::virtual_call(rt, BLOCK_ITEM_CAN_PLACE, &[a(0), a(1), state]).i() != 0
            {
                state
            } else {
                V::NULL
            }
        }
        // `BlockItem.canPlace(context, state)`: the state survives at the clicked place where the
        // item says it must (`mustSurvive`, vanilla's yes), and nothing's in its way
        // (`isUnobstructed`: the entities there aren't asked, so nothing is).
        ("net/minecraft/world/item/BlockItem", "canPlace")
            if desc
                == "(Lnet/minecraft/world/item/context/BlockPlaceContext;Lnet/minecraft/world/level/block/state/BlockState;)Z" =>
        {
            let must = crate::api::virtual_call(rt, BLOCK_ITEM_MUST_SURVIVE, &[a(0)]).i() != 0;
            let (level, pos) = (rt.get_field(a(1), "$level"), rt.get_field(a(1), "$pos"));
            V::I(i32::from(
                !must || crate::api::call(rt, STATE_CAN_SURVIVE, &[a(2), level, pos]).i() != 0,
            ))
        }
        ("net/minecraft/world/item/BlockItem", "mustSurvive") => V::I(1),
        // `BlockItem.placeBlock(context, state)`: the state set at the clicked place (flags 11:
        // neighbours told, clients told, re-rendered at once).
        ("net/minecraft/world/item/BlockItem", "placeBlock") => {
            let (level, pos) = (rt.get_field(a(1), "$level"), rt.get_field(a(1), "$pos"));
            crate::api::call(
                rt,
                "net/minecraft/world/level/Level.setBlock.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;I)Z",
                &[level, pos, a(2), V::I(11)],
            )
        }
        // `Item.use` reaching vanilla's: the game's own use of the stack goes on (eating a food,
        // placing a block item's block).
        ("net/minecraft/world/item/Item" | "net/minecraft/world/item/BlockItem", "use") => {
            // Through the whole chain: a fake player's hand is its own.
            let stack = crate::api::call(rt, "net/minecraft/world/entity/player/Player.getItemInHand.(Lnet/minecraft/world/InteractionHand;)Lnet/minecraft/world/item/ItemStack;", &[a(2), a(3)]);
            result_holder(rt, "PASS", stack)
        }
        // `InteractionResultHolder`: what `Item.use` answers, a result and the stack the hand keeps.
        (RESULT_HOLDER, "<init>") => {
            rt.put_field(a(0), "$result", a(1));
            rt.put_field(a(0), "$object", a(2));
            V::Void
        }
        (RESULT_HOLDER, "success" | "consume" | "pass" | "fail") => {
            let r = match name {
                "success" => "SUCCESS",
                "consume" => "CONSUME",
                "pass" => "PASS",
                _ => "FAIL",
            };
            result_holder(rt, r, a(0))
        }
        (RESULT_HOLDER, "sidedSuccess") => result_holder(rt, if a(1).i() != 0 { "SUCCESS" } else { "CONSUME" }, a(0)),
        (RESULT_HOLDER, "getResult") => rt.get_field(a(0), "$result"),
        (RESULT_HOLDER, "getObject") => rt.get_field(a(0), "$object"),
        ("net/minecraft/world/InteractionResult" | "net/minecraft/world/ItemInteractionResult", "sidedSuccess") => {
            let client = a(0).i() != 0;
            result(rt, owner, if client { "SUCCESS" } else { "CONSUME" })
        }
        // An item result consumes the action when the plain result it stands for does (both
        // block-interaction passes are a pass).
        ("net/minecraft/world/InteractionResult" | "net/minecraft/world/ItemInteractionResult", "consumesAction") => {
            let n = name_of(rt, a(0));
            V::I(matches!(n.as_str(), "SUCCESS" | "CONSUME" | "SUCCESS_NO_ITEM_USED" | "CONSUME_PARTIAL") as i32)
        }
        ("net/minecraft/world/InteractionResult", "shouldSwing") => V::I(matches!(name_of(rt, a(0)).as_str(), "SUCCESS" | "SUCCESS_NO_ITEM_USED") as i32),
        ("net/minecraft/world/InteractionResult", "indicateItemUse") => V::I(matches!(name_of(rt, a(0)).as_str(), "SUCCESS" | "CONSUME") as i32),
        // `ItemInteractionResult.result()`: the plain result it stands for.
        ("net/minecraft/world/ItemInteractionResult", "result") if desc == "()Lnet/minecraft/world/InteractionResult;" => {
            let n = name_of(rt, a(0));
            let plain = match n.as_str() {
                "SUCCESS" => "SUCCESS",
                "CONSUME" => "CONSUME",
                "CONSUME_PARTIAL" => "CONSUME_PARTIAL",
                "FAIL" => "FAIL",
                _ => "PASS",
            };
            result(rt, "net/minecraft/world/InteractionResult", plain)
        }
        ("net/minecraft/world/phys/BlockHitResult", "<init>") => {
            // (Vec3 location, Direction, BlockPos, boolean inside[, boolean worldBorder])
            rt.put_field(a(0), "$loc", a(1));
            rt.put_field(a(0), "$dir", a(2));
            rt.put_field(a(0), "$pos", a(3));
            rt.put_field(a(0), "$inside", V::I(a(4).i()));
            V::Void
        }
        ("net/minecraft/world/phys/BlockHitResult", "miss") => {
            let h = rt.new_object("net/minecraft/world/phys/BlockHitResult");
            rt.put_field(h, "$loc", a(0));
            rt.put_field(h, "$dir", a(1));
            rt.put_field(h, "$pos", a(2));
            rt.put_field(h, "$miss", V::I(1));
            h
        }
        ("net/minecraft/world/phys/BlockHitResult", "withDirection" | "withPosition") => {
            let h = rt.new_object("net/minecraft/world/phys/BlockHitResult");
            for f in ["$loc", "$dir", "$pos", "$inside", "$miss"] {
                let v = rt.get_field(a(0), f);
                rt.put_field(h, f, v);
            }
            rt.put_field(h, if name == "withDirection" { "$dir" } else { "$pos" }, a(1));
            h
        }
        ("net/minecraft/world/phys/BlockHitResult", "getBlockPos") => rt.get_field(a(0), "$pos"),
        ("net/minecraft/world/phys/BlockHitResult", "getDirection") => rt.get_field(a(0), "$dir"),
        ("net/minecraft/world/phys/BlockHitResult" | "net/minecraft/world/phys/HitResult", "getLocation") => rt.get_field(a(0), "$loc"),
        ("net/minecraft/world/phys/BlockHitResult", "isInside") => V::I(rt.get_field(a(0), "$inside").i()),
        ("net/minecraft/world/phys/BlockHitResult" | "net/minecraft/world/phys/HitResult", "getType") => {
            let miss = rt.get_field(a(0), "$miss").i() != 0;
            crate::api::api_enum(rt, "net/minecraft/world/phys/HitResult$Type", if miss { "MISS" } else { "BLOCK" }, if miss { 0 } else { 1 })
        }
        // Placing a block: vanilla's `BlockPlaceContext` (`glue::state_for_placement` makes one from
        // the game's). `UseOnContext` calls on one reach it too: its class decides, as Java dispatches.
        (o, n) if o.starts_with("net/minecraft/world/item/context/") && matches!(rt.class_of(a(0)), PLACE_CONTEXT | USE_ON_CONTEXT) && n != "<init>" && n != "at" => {
            return place_context_call(rt, n, args);
        }
        ("net/minecraft/world/item/context/BlockPlaceContext", "at") => {
            // A context for placing at `pos` against its face `direction` (vanilla: a hit at that face's centre).
            let (c, p, d) = (a(0), a(1), a(2));
            let n = native(rt, p);
            let step = DIRECTION_STEPS[native(rt, d)[0].clamp(0, 5) as usize];
            let hit = vec3(
                rt,
                n[0] as f64 + 0.5 + step[0] as f64 * 0.5,
                n[1] as f64 + 0.5 + step[1] as f64 * 0.5,
                n[2] as f64 + 0.5 + step[2] as f64 * 0.5,
            );
            let h = rt.new_object("net/minecraft/world/phys/BlockHitResult");
            rt.put_field(h, "$loc", hit);
            rt.put_field(h, "$dir", d);
            rt.put_field(h, "$pos", p);
            let ctx = rt.new_object(PLACE_CONTEXT);
            for f in ["$level", "$player", "$item", "$hand"] {
                let v = rt.get_field(c, f);
                rt.put_field(ctx, f, v);
            }
            place_context_from_hit(rt, ctx, h);
            ctx
        }
        ("net/minecraft/world/item/context/BlockPlaceContext" | "net/minecraft/world/item/context/UseOnContext", "<init>") => {
            // (Level, Player, InteractionHand, ItemStack, BlockHitResult), (Player, InteractionHand,
            // ItemStack, BlockHitResult) (a block place context's: the player's level), (Player,
            // InteractionHand, BlockHitResult) (also the player's stack in that hand) or
            // (UseOnContext): the fields the getters read.
            let ctx = a(0);
            if desc.starts_with("(Lnet/minecraft/world/item/context/UseOnContext;") {
                for f in ["$level", "$player", "$item", "$hand", "$hitResult"] {
                    let v = rt.get_field(a(1), f);
                    rt.put_field(ctx, f, v);
                }
                let h = rt.get_field(a(1), "$hitResult");
                place_context_from_hit(rt, ctx, h);
                return Some(V::Void);
            }
            let (level, player, hand, item, h) = if desc.starts_with("(Lnet/minecraft/world/level/Level;Lnet/minecraft/world/entity/player/Player;") {
                (a(1), a(2), a(3), a(4), a(5))
            } else if desc.starts_with("(Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/InteractionHand;Lnet/minecraft/world/item/ItemStack;") {
                let level = crate::api::call(rt, "net/minecraft/world/entity/player/Player.level.()Lnet/minecraft/world/level/Level;", &[a(1)]);
                (level, a(1), a(2), a(3), a(4))
            } else if desc.starts_with("(Lnet/minecraft/world/entity/player/Player;") {
                let player = a(1);
                // `player.level()` and `player.getItemInHand(hand)` through the whole chain: a fake
                // player's are its own.
                let stack = crate::api::call(rt, "net/minecraft/world/entity/player/Player.getItemInHand.(Lnet/minecraft/world/InteractionHand;)Lnet/minecraft/world/item/ItemStack;", &[player, a(2)]);
                let level = crate::api::call(rt, "net/minecraft/world/entity/player/Player.level.()Lnet/minecraft/world/level/Level;", &[player]);
                (level, player, a(2), stack, a(3))
            } else {
                return Some(V::Void);
            };
            rt.put_field(ctx, "$level", level);
            rt.put_field(ctx, "$player", player);
            rt.put_field(ctx, "$hand", hand);
            rt.put_field(ctx, "$item", item);
            place_context_from_hit(rt, ctx, h);
            V::Void
        }
        ("net/minecraft/core/Direction", "orderedByNearest") => {
            let (yaw, pitch) = (rt.get_field(a(0), "$yaw").f(), rt.get_field(a(0), "$pitch").f());
            directions(rt, &ordered_by_nearest(yaw, pitch))
        }
        // Vanilla's debug renderers' packets: a released game's do nothing, but for game tests'
        // markers (Create's fluid pipes tell of their neighbours' updates).
        ("net/minecraft/network/protocol/game/DebugPackets", n) if !n.starts_with("sendGameTest") => V::Void,
        ("net/minecraft/core/Direction", "fromYRot") => direction(rt, horizontal_facing(a(0).d() as f32)),
        // The player's place: the acting server player's when the game told it (`acting_player`),
        // the client's own as the host places it; a player the game hasn't placed (the client's
        // before its world loads: a resource reload at start, a probe drawing a block) is where a
        // new entity is, at the origin with no rotation, so looking south.
        (o, n) if is_player(o)
            && matches!(
                n,
                "getX" | "getY" | "getZ" | "position" | "getPosition" | "blockPosition" | "getEyeY" | "getEyePosition" | "getEyeHeight" | "getLookAngle" | "getViewVector" | "getXRot" | "getYRot" | "getYHeadRot" | "getBoundingBox" | "getBbWidth" | "getBbHeight"
                    | "getRotationVector" | "getVisualRotationYInDegrees" | "getPreciseBodyRotation" | "$yHeadRot" | "$yBodyRot"
                    | "getOnPos" | "getOnPosLegacy" | "getBlockPosBelowThatAffectsMyMovement"
            )
            && (!n.starts_with('$') || desc == "()F")
            && (player_place(rt, a(0)).is_some() || is_player_class(rt.class_of(a(0)))) =>
        {
            let (p, yaw, pitch) = player_place(rt, a(0)).unwrap_or_default();
            let eye = p[1] + player_eye_height(rt);
            match n {
                "getX" => V::D(p[0]),
                "getY" => V::D(p[1]),
                "getZ" => V::D(p[2]),
                "position" | "getPosition" => vec3(rt, p[0], p[1], p[2]),
                "blockPosition"
                | "getOnPos"
                | "getOnPosLegacy"
                | "getBlockPosBelowThatAffectsMyMovement" => feet_block(rt, n, p),
                "getEyeY" => V::D(eye),
                "getEyePosition" => vec3(rt, p[0], eye, p[2]),
                "getEyeHeight" => V::F(player_eye_height(rt) as f32),
                // `EntityDimensions.makeBoundingBox`: half the width and the height, as floats.
                "getBoundingBox" => {
                    let (w, h) = player_size(rt);
                    let half = f64::from(w / 2.0);
                    crate::shapes::aabb(rt, [p[0] - half, p[1], p[2] - half, p[0] + half, p[1] + f64::from(h), p[2] + half])
                }
                "getBbWidth" => V::F(player_size(rt).0),
                "getBbHeight" => V::F(player_size(rt).1),
                "getXRot" => V::F(pitch),
                // A player's head and body turn with their view, in the game.
                "getYRot"
                | "getYHeadRot"
                | "getVisualRotationYInDegrees"
                | "getPreciseBodyRotation"
                | "$yHeadRot"
                | "$yBodyRot" => V::F(yaw),
                "getRotationVector" => vec2(rt, pitch, yaw),
                _ => view_vector(rt, yaw, pitch),
            }
        }
        // Turning a player. The window's own (`LocalPlayer`, on its client) turns as a client mod
        // turns them (Create's turntable, in a frame): their view turns, and their client tells
        // the server as it tells it their own turns. A server's player turns for the rest of the
        // hook, as vanilla's copy does until their client next tells where they look; their
        // client isn't told, as vanilla's isn't. A head and body follow the view in the game, and
        // the pose between ticks is the game's own. An angle that isn't finite is dropped, as
        // vanilla's `setYRot` drops it; a pitch is kept between -90 and 90, as 26.3's `setXRot`
        // keeps it.
        (
            o,
            n @ ("setYRot" | "setXRot" | "setYHeadRot" | "setYBodyRot" | "$yHeadRot" | "$yBodyRot"
            | "setOldPosAndRot"),
        ) if is_player(o)
            && is_player_class(rt.class_of(a(0)))
            && (desc == "(F)V" || (n == "setOldPosAndRot" && desc == "()V")) =>
        {
            let to = a(1).f();
            let local = rt.class_of(a(0)) == LOCAL_PLAYER;
            match n {
                _ if !to.is_finite() => {}
                "setYRot" | "setXRot" if local => {
                    let (yaw, pitch) = if n == "setYRot" {
                        (Some(to), None)
                    } else {
                        (None, Some(to))
                    };
                    rt.host_call(|h| h.turn_client_player(yaw, pitch));
                }
                "setYRot" => rt.put_field(a(0), "$yaw", V::F(to)),
                "setXRot" => rt.put_field(a(0), "$pitch", V::F((to % 360.0).clamp(-90.0, 90.0))),
                _ => {}
            }
            V::Void
        }
        // Any other entity's head and body written: a mob's are turned by `vanilla_entities`, and
        // a mod's own entity keeps the field as written.
        (_, "$yHeadRot" | "$yBodyRot") if desc == "(F)V" => V::Void,
        // How far the player is from another entity (`Entity.distanceTo`, a float, and
        // `distanceToSqr`), a point or a `Vec3`; an entity whose place isn't known is where they are.
        (o, n @ ("distanceTo" | "distanceToSqr")) if is_player(o) && player_place(rt, a(0)).is_some() => {
            let (p, _, _) = player_place(rt, a(0)).unwrap_or_default();
            let q = match desc {
                "(DDD)D" => [a(1).d(), a(2).d(), a(3).d()],
                "(Lnet/minecraft/world/phys/Vec3;)D" => xyz(rt, a(1)),
                _ => entity_place(rt, a(1)).unwrap_or(p),
            };
            let d2 = (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2);
            if n == "distanceTo" { V::F(d2.sqrt() as f32) } else { V::D(d2) }
        }
        // Reach (`Player.blockInteractionRange`, `entityInteractionRange`: the attributes' bases,
        // creative's modifiers added), and whether a block is in it with `margin` more
        // (`canInteractWithBlock`: the block's box from the eyes; a server checks a mod's packet
        // with it). A player whose place isn't known reaches it.
        (o, "blockInteractionRange" | "entityInteractionRange") if is_player(o) => {
            let creative = rt.host().is_some_and(|h| h.player_creative());
            V::D(match (name, creative) {
                ("blockInteractionRange", false) => 4.5,
                ("blockInteractionRange", true) => 5.0,
                (_, false) => 3.0,
                (_, true) => 5.0,
            })
        }
        (o, "canInteractWithBlock") if is_player(o) => {
            let Some((p, _, _)) = player_place(rt, a(0)) else { return Some(V::I(1)) };
            let creative = rt.host().is_some_and(|h| h.player_creative());
            let range = if creative { 5.0 } else { 4.5 } + a(2).d();
            let eye = [p[0], p[1] + player_eye_height(rt), p[2]];
            let b = native(rt, a(1));
            // `AABB.distanceToSqr(Vec3)`: per axis, how far outside the box.
            let d2: f64 = (0..3)
                .map(|i| {
                    let (lo, hi) = (b[i] as f64, b[i] as f64 + 1.0);
                    let d = (lo - eye[i]).max(eye[i] - hi).max(0.0);
                    d * d
                })
                .sum();
            V::I(i32::from(d2 < range * range))
        }
        // `Player.isHolding(Predicate<ItemStack>)` (either hand's stack passes) and `isHolding(Item)`.
        (o, "isHolding") if is_player(o) => {
            let by_item = desc.starts_with("(Lnet/minecraft/world/item/Item;");
            let mut holding = false;
            for hand in [crate::hand::MAIN, crate::hand::OFF] {
                let stack = crate::hand::stack(rt, hand);
                holding = if by_item {
                    let item = call(rt, "net/minecraft/world/item/ItemStack", "getItem", "()Lnet/minecraft/world/item/Item;", &[stack]).unwrap_or(V::NULL);
                    item == a(1)
                } else {
                    rt.call_functional("java/util/function/Predicate.test.(Ljava/lang/Object;)Z", a(1), &[stack]).i() != 0
                };
                if holding {
                    break;
                }
            }
            V::I(i32::from(holding))
        }
        // Players ride nothing here, and stand or crouch (`Entity.getPose`).
        (o, "isPassenger") if is_player(o) => V::I(0),
        (o, "getPose") if is_player(o) && player_object(rt, a(0)) => {
            let crouching = rt.host().is_some_and(|h| h.player_sneaking());
            plain_enum(rt, POSE, if crouching { "CROUCHING" } else { "STANDING" }).unwrap_or(V::NULL)
        }
        // A player's size in a pose (`Player.getDefaultDimensions`: its `POSES`, standing for any
        // other; `LivingEntity.getDimensions` adds its scale attribute, whose base the game's
        // players keep here, and asleep is `SLEEPING_DIMENSIONS`, as `POSES` has it too).
        (o, "getDimensions" | "getDefaultDimensions")
            if is_player(o)
                && desc == crate::vanilla_entities::POSE_DIMENSIONS
                && player_object(rt, a(0)) =>
        {
            let pose = constant(rt, a(1));
            let (w, h, eye, fixed, vehicle) = match pose.as_str() {
                "SLEEPING" => (0.2, 0.2, 0.2, true, 0.0),
                "FALL_FLYING" | "SWIMMING" | "SPIN_ATTACK" => (0.6, 0.6, 0.4, false, 0.0),
                "CROUCHING" => (0.6, 1.5, 1.27, false, 0.6),
                "DYING" => (0.2, 0.2, 1.62, true, 0.0),
                _ => (0.6, 1.8, 1.62, false, 0.6),
            };
            crate::mod_entities::sized(rt, w, h, eye, fixed, vehicle)
        }
        ("net/minecraft/world/entity/player/Inventory", "getSelectionSize") => V::I(9),
        // Item cooldowns aren't kept for a mod's uses yet: none is ever cooling down.
        (o, "getCooldowns") if is_player(o) => rt.intern("compat:cooldowns".into(), |rt| rt.new_object("net/minecraft/world/item/ItemCooldowns")),
        // What the player last hurt (`setLastHurtMob`: a mod's own blow, Create's potato), a living
        // entity only, and when (its `tickCount` then), asked back by `getLastHurtMob` and
        // `getLastHurtMobTimestamp`. The game's tamed wolves don't hear of it.
        (o, "setLastHurtMob") if is_player(o) => {
            let target = if a(1).r() != 0 && rt.instance_of(a(1), "net/minecraft/world/entity/LivingEntity") { a(1) } else { V::NULL };
            let now = V::I(rt.get_field(a(0), "tickCount").i());
            rt.put_field(a(0), "$lastHurtMob", target);
            rt.put_field(a(0), "$lastHurtMobTimestamp", now);
            V::Void
        }
        (o, "getLastHurtMob") if is_player(o) => rt.get_field(a(0), "$lastHurtMob"),
        (o, "getLastHurtMobTimestamp") if is_player(o) => V::I(rt.get_field(a(0), "$lastHurtMobTimestamp").i()),
        // An entity's main arm (`getMainArm`), a mod class's own aside: the right one, vanilla's
        // default (a player's comes from its client's options, a mob is left-handed when born so:
        // the game tells neither).
        (o, "getMainArm") if is_player(o) => crate::api::get_static(rt, "net/minecraft/world/entity/HumanoidArm.RIGHT"),
        ("net/minecraft/world/item/ItemCooldowns", "addCooldown" | "removeCooldown") => V::Void,
        ("net/minecraft/world/item/ItemCooldowns", "isOnCooldown") => V::I(0),
        ("net/minecraft/world/item/ItemCooldowns", "getCooldownPercent") => V::F(0.0),
        // What the acting player may do: in creative they build at once, may fly and take no harm.
        (o, "getAbilities") if is_player(o) => {
            let creative = rt.host().is_some_and(|h| h.player_creative());
            let abilities = rt.new_object("net/minecraft/world/entity/player/Abilities");
            for f in ["instabuild", "mayfly", "invulnerable"] {
                rt.put_field(abilities, f, V::I(creative as i32));
            }
            rt.put_field(abilities, "mayBuild", V::I(1));
            rt.put_field(abilities, "flying", V::I(0));
            rt.put_field(abilities, "flyingSpeed", V::F(0.05));
            rt.put_field(abilities, "walkingSpeed", V::F(0.1));
            abilities
        }
        // The acting player's look (an object `glue` made carries it; others look south, level).
        (o, n) if is_player(o) && matches!(n, "getYRot" | "getXRot" | "getViewYRot" | "getViewXRot" | "getYHeadRot" | "getVisualRotationYInDegrees" | "getDirection" | "getMotionDirection") => {
            let (yaw, pitch) = (rt.get_field(a(0), "$yaw").f(), rt.get_field(a(0), "$pitch").f());
            match n {
                "getXRot" | "getViewXRot" => V::F(pitch),
                "getDirection" | "getMotionDirection" => direction(rt, horizontal_facing(yaw)),
                _ => V::F(yaw),
            }
        }
        ("net/minecraft/util/Mth", "sin") => V::F(mth_sin(a(0))),
        ("net/minecraft/util/Mth", "cos") => V::F(mth_cos(a(0))),
        // The game's version, as mods made for it read it: 1.21.1's `version.json`.
        ("net/minecraft/SharedConstants", "getCurrentVersion") => rt.intern("compat:world-version".into(), |rt| rt.new_object(WORLD_VERSION)),
        (WORLD_VERSION | "net/minecraft/DetectedVersion", n) => match n {
            "getName" | "getId" => rt.string("1.21.1"),
            "getProtocolVersion" => V::I(767),
            "isStable" => V::I(1),
            // `PackType`: client resources (its first constant) 34, server data 48.
            "getPackVersion" => V::I(if rt.get_field(a(1), "$ordinal").i() == 0 { 34 } else { 48 }),
            "getDataVersion" => {
                let series = rt.string("main");
                crate::records::make_data(rt, "net/minecraft/world/level/storage/DataVersion", "(ILjava/lang/String;)V", &[V::I(3955), series])
            }
            _ => return None,
        },
        // Block entities
        // Block entities: the API's state lives in the fields Java names (`level`, `worldPosition`,
        // `blockState`, `remove`), which mod subclasses read directly. Until it joins a level its
        // `level` is null and it isn't removed, as Java's fields start: a constructor that asks
        // (Create's `sendData` from a value's default, `level != null && ...`) reads them so.
        ("net/minecraft/world/level/block/entity/BlockEntity", "<init>") => {
            rt.put_field(a(0), "level", V::NULL);
            rt.put_field(a(0), "remove", V::I(0));
            rt.put_field(a(0), "$type", a(1));
            rt.put_field(a(0), "type", a(1));
            rt.put_field(a(0), "$pos", a(2));
            rt.put_field(a(0), "worldPosition", a(2));
            rt.put_field(a(0), "$state", a(3));
            rt.put_field(a(0), "blockState", a(3));
            V::Void
        }
        (o, "getBlockPos") if o.ends_with("BlockEntity") => rt.get_field(a(0), "$pos"),
        (o, "getBlockState") if o.ends_with("BlockEntity") => rt.get_field(a(0), "$state"),
        (o, "setBlockState") if o.ends_with("BlockEntity") => {
            rt.put_field(a(0), "$state", a(1));
            rt.put_field(a(0), "blockState", a(1));
            V::Void
        }
        (o, "getType") if o.ends_with("BlockEntity") => rt.get_field(a(0), "$type"),
        (o, "getLevel") if o.ends_with("BlockEntity") => rt.get_field(a(0), "level"),
        (o, "hasLevel") if o.ends_with("BlockEntity") => V::I((rt.get_field(a(0), "level").r() != 0) as i32),
        (o, "setLevel") if o.ends_with("BlockEntity") => {
            rt.put_field(a(0), "level", a(1));
            V::Void
        }
        (o, "isRemoved") if o.ends_with("BlockEntity") => V::I((rt.get_field(a(0), "remove").i() != 0) as i32),
        (o, "setRemoved" | "clearRemoved") if o.ends_with("BlockEntity") => {
            rt.put_field(a(0), "remove", V::I((name == "setRemoved") as i32));
            V::Void
        }
        (o, "setChanged") if o.ends_with("BlockEntity") && desc == "()V" => {
            block_entity_changed(rt, a(0));
            V::Void
        }
        (o, "setChanged" | "onLoad" | "onChunkUnloaded" | "invalidateCapabilities") if o.ends_with("BlockEntity") => V::Void,
        // The base class saves and loads NeoForge's own data of it (`getPersistentData()`); the glue
        // writes its id and position.
        (o, "saveAdditional") if o.ends_with("BlockEntity") => {
            crate::block_entity_data::base_save(rt, a(0), a(1));
            V::Void
        }
        (o, "loadAdditional") if o.ends_with("BlockEntity") => {
            crate::block_entity_data::base_load(rt, a(0), a(1));
            V::Void
        }
        (o, "requestModelDataUpdate" | "onDataPacket") if o.ends_with("BlockEntity") => V::Void,
        // Vanilla's `handleUpdateTag`: `loadWithComponents`, which loads through the block entity's
        // own `loadAdditional`.
        (o, "handleUpdateTag") if o.ends_with("BlockEntity") => {
            const LOAD: &str = "loadAdditional.(Lnet/minecraft/nbt/CompoundTag;Lnet/minecraft/core/HolderLookup$Provider;)V";
            if let Some(vc) = rt.vcall {
                vc(rt, "net/minecraft/world/level/block/entity/BlockEntity.loadAdditional.(Lnet/minecraft/nbt/CompoundTag;Lnet/minecraft/core/HolderLookup$Provider;)V", LOAD, &[a(0), a(1), a(2)]);
            }
            V::Void
        }
        (o, "getUpdateTag") if o.ends_with("BlockEntity") => compound(rt),
        (o, "getUpdatePacket") if o.ends_with("BlockEntity") => V::NULL,
        // `ClientboundBlockEntityDataPacket.create(blockEntity[, saver])`, what a mod's
        // `getUpdatePacket` returns: the block entity's position and type, and its update tag, made
        // by `saver` from it and the level's registries, or its own `getUpdateTag`. The game sends
        // players the update tag itself (`glue::block_entity_update_tag`): the packet is only read
        // back, by its getters, which the data-class table answers, as it built it.
        (o, "create") if o.ends_with("/protocol/game/ClientboundBlockEntityDataPacket") => {
            const INIT: &str = "(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/entity/BlockEntityType;Lnet/minecraft/nbt/CompoundTag;)V";
            const SAVER: &str = "java/util/function/BiFunction.apply.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
            const UPDATE_TAG: &str = "getUpdateTag.(Lnet/minecraft/core/HolderLookup$Provider;)Lnet/minecraft/nbt/CompoundTag;";
            const UPDATE_TAG_KEY: &str = "net/minecraft/world/level/block/entity/BlockEntity.getUpdateTag.(Lnet/minecraft/core/HolderLookup$Provider;)Lnet/minecraft/nbt/CompoundTag;";
            let be = a(0);
            let access = rt.intern("compat:registry-access".into(), |rt| rt.new_object("net/minecraft/core/RegistryAccess"));
            let tag = if args.len() > 1 {
                rt.call_functional(SAVER, a(1), &[be, access])
            } else if let Some(vc) = rt.vcall {
                vc(rt, UPDATE_TAG_KEY, UPDATE_TAG, &[be, access])
            } else {
                compound(rt)
            };
            let (pos, kind) = (rt.get_field(be, "$pos"), rt.get_field(be, "$type"));
            crate::records::make_data(rt, "net/minecraft/network/protocol/game/ClientboundBlockEntityDataPacket", INIT, &[pos, kind, tag])
        }
        // A type keeps its factory and the blocks it is valid for (an array, or a set).
        ("net/minecraft/world/level/block/entity/BlockEntityType$Builder", "of") => {
            let b = rt.new_object("net/minecraft/world/level/block/entity/BlockEntityType$Builder");
            rt.put_field(b, "$factory", a(0));
            rt.put_field(b, "$blocks", a(1));
            b
        }
        // `new BlockEntityType(factory, validBlocks, dataType)` (Refined Storage makes its types
        // so): what `Builder.of(factory, blocks).build(type)` makes.
        ("net/minecraft/world/level/block/entity/BlockEntityType", "<init>") if desc.starts_with("(Lnet/minecraft/world/level/block/entity/BlockEntityType$BlockEntitySupplier;") => {
            rt.put_field(a(0), "$factory", a(1));
            rt.put_field(a(0), "$blocks", a(2));
            V::Void
        }
        ("net/minecraft/world/level/block/entity/BlockEntityType$Builder", "build") => {
            let t = rt.new_object("net/minecraft/world/level/block/entity/BlockEntityType");
            for f in ["$factory", "$blocks"] {
                let v = rt.get_field(a(0), f);
                rt.put_field(t, f, v);
            }
            t
        }
        // `type.create(pos, state)`: the factory the type was built with.
        ("net/minecraft/world/level/block/entity/BlockEntityType", "create") => {
            let f = rt.get_field(a(0), "$factory");
            if f.r() == 0 {
                V::NULL
            } else {
                rt.call_functional("net/minecraft/world/level/block/entity/BlockEntityType$BlockEntitySupplier.create.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/world/level/block/entity/BlockEntity;", f, &[a(1), a(2)])
            }
        }
        ("net/minecraft/world/level/block/entity/BlockEntityType", "isValid") => V::I(1),
        // `EntityBlock`'s defaults, for a block without its own: no ticker, no game-event listener.
        ("net/minecraft/world/level/block/EntityBlock", "getTicker" | "getListener") => V::NULL,
        (o, "createTickerHelper") if o.ends_with("BaseEntityBlock") => {
            if a(0) == a(1) { a(2) } else { V::NULL }
        }
        (o, "getBlockEntity") if o.starts_with("net/minecraft/") && (o.ends_with("Level") || o.ends_with("LevelAccessor") || o.ends_with("BlockGetter") || o.ends_with("LevelReader")) => {
            let h = native(rt, a(1));
            crate::glue::block_entity_at(rt, [h[0] as i32, h[1] as i32, h[2] as i32])
        }
        // Vec2: two floats (`x`, `y`), as Create's value settings lay their board out with.
        (VEC2, "<init>") if desc == "(FF)V" => {
            rt.put_field(a(0), "x", V::F(a(1).f()));
            rt.put_field(a(0), "y", V::F(a(2).f()));
            V::Void
        }
        (VEC2, n) => {
            let xy = |rt: &Rt, v: V| [rt.get_field(v, "x").f(), rt.get_field(v, "y").f()];
            let [x, y] = xy(rt, a(0));
            match (n, desc) {
                ("scale", _) => vec2(rt, x * a(1).f(), y * a(1).f()),
                ("dot", _) => {
                    let [ox, oy] = xy(rt, a(1));
                    V::F(x * ox + y * oy)
                }
                ("add", "(Lnet/minecraft/world/phys/Vec2;)Lnet/minecraft/world/phys/Vec2;") => {
                    let [ox, oy] = xy(rt, a(1));
                    vec2(rt, x + ox, y + oy)
                }
                ("add", _) => vec2(rt, x + a(1).f(), y + a(1).f()),
                ("equals", _) => {
                    let [ox, oy] = xy(rt, a(1));
                    V::I(i32::from(rt.class_of(a(1)) == VEC2 && x == ox && y == oy))
                }
                // `normalized`: `Mth.sqrt`'s float root; a zero vector stays zero.
                ("normalized", _) => {
                    let l = (x * x + y * y).sqrt();
                    if l < 1.0e-4 { vec2(rt, 0.0, 0.0) } else { vec2(rt, x / l, y / l) }
                }
                ("length", _) => V::F((x * x + y * y).sqrt()),
                ("lengthSquared", _) => V::F(x * x + y * y),
                ("distanceToSqr", _) => {
                    let [ox, oy] = xy(rt, a(1));
                    V::F((ox - x) * (ox - x) + (oy - y) * (oy - y))
                }
                ("negated", _) => vec2(rt, -x, -y),
                _ => return None,
            }
        }
        // Plain vanilla enums' `values()` and `valueOf(String)`.
        (o, "values") if desc.starts_with("()[") => return plain_enum_values(rt, o),
        (o, "valueOf") if desc.starts_with("(Ljava/lang/String;)") && PLAIN_ENUMS.iter().any(|(c, _)| *c == o) => {
            let n = rt.str_of(a(0)).unwrap_or_default().to_owned();
            return plain_enum(rt, o, &n);
        }
        // Vec3
        ("net/minecraft/world/phys/Vec3", "<init>") if desc == "(DDD)V" => {
            rt.put_field(a(0), "x", a(1));
            rt.put_field(a(0), "y", a(2));
            rt.put_field(a(0), "z", a(3));
            V::Void
        }
        ("net/minecraft/world/phys/Vec3", n) => {
            let p = xyz(rt, a(0));
            match (n, desc) {
                ("x", _) => V::D(p[0]),
                ("y", _) => V::D(p[1]),
                ("z", _) => V::D(p[2]),
                ("atCenterOf", _) => {
                    let h = native(rt, a(0));
                    vec3(rt, h[0] as f64 + 0.5, h[1] as f64 + 0.5, h[2] as f64 + 0.5)
                }
                ("atLowerCornerOf", _) => {
                    let h = native(rt, a(0));
                    vec3(rt, h[0] as f64, h[1] as f64, h[2] as f64)
                }
                ("add" | "subtract", "(Lnet/minecraft/world/phys/Vec3;)Lnet/minecraft/world/phys/Vec3;") => {
                    let q = xyz(rt, a(1));
                    let s = if n == "add" { 1.0 } else { -1.0 };
                    vec3(rt, p[0] + s * q[0], p[1] + s * q[1], p[2] + s * q[2])
                }
                ("add" | "subtract", "(DDD)Lnet/minecraft/world/phys/Vec3;") => {
                    let s = if n == "add" { 1.0 } else { -1.0 };
                    vec3(rt, p[0] + s * a(1).d(), p[1] + s * a(2).d(), p[2] + s * a(3).d())
                }
                ("scale", _) => vec3(rt, p[0] * a(1).d(), p[1] * a(1).d(), p[2] * a(1).d()),
                ("multiply", "(DDD)Lnet/minecraft/world/phys/Vec3;") => vec3(rt, p[0] * a(1).d(), p[1] * a(2).d(), p[2] * a(3).d()),
                ("length", _) => V::D((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt()),
                ("lengthSqr", _) => V::D(p[0] * p[0] + p[1] * p[1] + p[2] * p[2]),
                ("distanceTo" | "distanceToSqr", "(Lnet/minecraft/world/phys/Vec3;)D") => {
                    let q = xyz(rt, a(1));
                    let d2 = (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2);
                    V::D(if n == "distanceTo" { d2.sqrt() } else { d2 })
                }
                ("normalize", _) => {
                    let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
                    if l < 1.0e-4 { vec3_zero(rt) } else { vec3(rt, p[0] / l, p[1] / l, p[2] / l) }
                }
                ("<init>", "(Lorg/joml/Vector3f;)V") => {
                    for f in ["x", "y", "z"] {
                        let c = rt.get_field(a(1), f).f() as f64;
                        rt.put_field(a(0), f, V::D(c));
                    }
                    V::Void
                }
                ("fromRGB24", _) => {
                    let c = a(0).i();
                    vec3(rt, ((c >> 16) & 255) as f64 / 255.0, ((c >> 8) & 255) as f64 / 255.0, (c & 255) as f64 / 255.0)
                }
                ("atLowerCornerWithOffset", _) => {
                    let h = native(rt, a(0));
                    vec3(rt, h[0] as f64 + a(1).d(), h[1] as f64 + a(2).d(), h[2] as f64 + a(3).d())
                }
                ("atBottomCenterOf", _) => {
                    let h = native(rt, a(0));
                    vec3(rt, h[0] as f64 + 0.5, h[1] as f64, h[2] as f64 + 0.5)
                }
                ("upFromBottomCenterOf", _) => {
                    let h = native(rt, a(0));
                    vec3(rt, h[0] as f64 + 0.5, h[1] as f64 + a(1).d(), h[2] as f64 + 0.5)
                }
                ("vectorTo", _) => {
                    let q = xyz(rt, a(1));
                    vec3(rt, q[0] - p[0], q[1] - p[1], q[2] - p[2])
                }
                ("cross", _) => {
                    let q = xyz(rt, a(1));
                    vec3(rt, p[1] * q[2] - p[2] * q[1], p[2] * q[0] - p[0] * q[2], p[0] * q[1] - p[1] * q[0])
                }
                ("distanceToSqr", "(DDD)D") => {
                    let (dx, dy, dz) = (a(1).d() - p[0], a(2).d() - p[1], a(3).d() - p[2]);
                    V::D(dx * dx + dy * dy + dz * dz)
                }
                ("closerThan", "(Lnet/minecraft/core/Position;D)Z") => {
                    let q = xyz(rt, a(1));
                    let (dx, dy, dz) = (q[0] - p[0], q[1] - p[1], q[2] - p[2]);
                    V::I((dx * dx + dy * dy + dz * dz < a(2).d() * a(2).d()) as i32)
                }
                // Within `h` horizontally and `v` vertically.
                ("closerThan", "(Lnet/minecraft/world/phys/Vec3;DD)Z") => {
                    let q = xyz(rt, a(1));
                    let (dx, dy, dz) = (q[0] - p[0], q[1] - p[1], q[2] - p[2]);
                    let h = a(2).d();
                    V::I((dx * dx + dz * dz < h * h && dy.abs() < a(3).d()) as i32)
                }
                ("reverse", _) => vec3(rt, p[0] * -1.0, p[1] * -1.0, p[2] * -1.0),
                ("multiply", "(Lnet/minecraft/world/phys/Vec3;)Lnet/minecraft/world/phys/Vec3;") => {
                    let q = xyz(rt, a(1));
                    vec3(rt, p[0] * q[0], p[1] * q[1], p[2] * q[2])
                }
                // Three `nextFloat`s, x first.
                ("offsetRandom", _) => {
                    let spread = a(2).f();
                    let mut o = [0.0f64; 3];
                    for c in &mut o {
                        let r = crate::api::call(rt, "net/minecraft/util/RandomSource.nextFloat.()F", &[a(1)]).f();
                        *c = ((r - 0.5) * spread) as f64;
                    }
                    vec3(rt, p[0] + o[0], p[1] + o[1], p[2] + o[2])
                }
                ("horizontalDistance", _) => V::D((p[0] * p[0] + p[2] * p[2]).sqrt()),
                ("horizontalDistanceSqr", _) => V::D(p[0] * p[0] + p[2] * p[2]),
                ("toString", _) => {
                    let text = format!("({}, {}, {})", crate::java_double(p[0], false), crate::java_double(p[1], false), crate::java_double(p[2], false));
                    rt.string(&text)
                }
                // `Mth.lerp(delta, start, end)` per axis.
                ("lerp", _) => {
                    let (q, t) = (xyz(rt, a(1)), a(2).d());
                    vec3(rt, p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1]), p[2] + t * (q[2] - p[2]))
                }
                ("xRot" | "yRot" | "zRot", _) => {
                    let (c, s) = (mth_cos(a(1)) as f64, mth_sin(a(1)) as f64);
                    let [x, y, z] = p;
                    match n {
                        "xRot" => vec3(rt, x, y * c + z * s, z * c - y * s),
                        "yRot" => vec3(rt, x * c + z * s, y, z * c - x * s),
                        _ => vec3(rt, x * c + y * s, y * c - x * s, z),
                    }
                }
                ("directionFromRotation", _) => {
                    let (x_rot, y_rot) = if desc.starts_with("(Lnet/minecraft/world/phys/Vec2;") {
                        (rt.get_field(a(0), "x").f(), rt.get_field(a(0), "y").f())
                    } else {
                        (a(0).f(), a(1).f())
                    };
                    let c = mth_cos(V::F(-y_rot * 0.017453292 - std::f32::consts::PI));
                    let s = mth_sin(V::F(-y_rot * 0.017453292 - std::f32::consts::PI));
                    let c2 = -mth_cos(V::F(-x_rot * 0.017453292));
                    let s2 = mth_sin(V::F(-x_rot * 0.017453292));
                    vec3(rt, (s * c2) as f64, s2 as f64, (c * c2) as f64)
                }
                // The axes in the set floored.
                ("align", _) => {
                    let axes: Vec<i32> = crate::jutil::elements(rt, a(1)).into_iter().map(|e| enum_ordinal(rt, e)).collect();
                    let f = |i: usize| if axes.contains(&(i as i32)) { mth_floor(p[i]) as f64 } else { p[i] };
                    vec3(rt, f(0), f(1), f(2))
                }
                ("get", "(Lnet/minecraft/core/Direction$Axis;)D") => V::D(p[native(rt, a(1))[0].clamp(0, 2) as usize]),
                ("with", _) => {
                    let mut q = p;
                    q[native(rt, a(1))[0].clamp(0, 2) as usize] = a(2).d();
                    vec3(rt, q[0], q[1], q[2])
                }
                ("relative", _) => {
                    let d = DIRECTION_STEPS[native(rt, a(1))[0].clamp(0, 5) as usize];
                    let t = a(2).d();
                    vec3(rt, p[0] + t * d[0] as f64, p[1] + t * d[1] as f64, p[2] + t * d[2] as f64)
                }
                ("toVector3f", _) => {
                    let v = rt.new_object("org/joml/Vector3f");
                    for (f, c) in ["x", "y", "z"].into_iter().zip(p) {
                        rt.put_field(v, f, V::F(c as f32));
                    }
                    v
                }
                ("dot", _) => {
                    let q = xyz(rt, a(1));
                    V::D(p[0] * q[0] + p[1] * q[1] + p[2] * q[2])
                }
                // `Double.compare(...) == 0` per axis: NaN equals NaN, 0.0 doesn't equal -0.0.
                ("equals", _) => {
                    let q = xyz(rt, a(1));
                    let same = |x: f64, y: f64| x.to_bits() == y.to_bits() || (x.is_nan() && y.is_nan());
                    V::I((rt.class_of(a(1)) == "net/minecraft/world/phys/Vec3" && (0..3).all(|i| same(p[i], q[i]))) as i32)
                }
                ("hashCode", _) => {
                    // `Double.doubleToLongBits` folded as vanilla's `Vec3.hashCode`.
                    let bits = |d: f64| {
                        let l = if d.is_nan() { 0x7ff8_0000_0000_0000u64 } else { d.to_bits() };
                        (l ^ (l >> 32)) as i32
                    };
                    let h = bits(p[0]).wrapping_mul(31).wrapping_add(bits(p[1])).wrapping_mul(31).wrapping_add(bits(p[2]));
                    V::I(h)
                }
                _ => return None,
            }
        }

        // Items
        // `new ItemStack(itemLike)` holds `itemLike.asItem()`: a block's item, not its name.
        ("net/minecraft/world/item/ItemStack", "<init>") => {
            // An `ItemLike` or a `Holder<Item>`, then the count when one is given.
            let like = if desc.starts_with("(Lnet/minecraft/core/Holder;") { rt.get_field(a(1), "value") } else { V::NULL };
            let like = if like.r() == 0 { a(1) } else { like };
            let it = crate::creative::as_item(rt, like);
            let n = item_like_name(rt, it);
            let count = if desc.split(';').nth(1).is_some_and(|rest| rest.starts_with('I')) { a(2).i() } else { 1 };
            let i = item(rt, &n);
            rt.put_field(a(0), "$item", i);
            rt.put_field(a(0), "$count", V::I(count));
            V::Void
        }
        ("net/minecraft/world/item/ItemStack", "isEmpty") => V::I(stack_empty(rt, a(0)) as i32),
        // A tooltip's picture (a bundle's contents): none, as for most items.
        ("net/minecraft/world/item/ItemStack", "getTooltipImage") => optional(rt, V::NULL),
        ("net/minecraft/world/item/ItemStack", "getItem") => {
            if stack_empty(rt, a(0)) { item(rt, "minecraft:air") } else { rt.get_field(a(0), "$item") }
        }
        // `getItemHolder()`: its item's registry entry (`stack.getItemHolder().is(tag)`).
        ("net/minecraft/world/item/ItemStack", "getItemHolder") => {
            let it = if stack_empty(rt, a(0)) {
                item(rt, "minecraft:air")
            } else {
                rt.get_field(a(0), "$item")
            };
            let n = item_like_name(rt, it);
            crate::neo::registry_holder(rt, "item", &n, it)
        }
        ("net/minecraft/world/item/ItemStack", "getCount") => V::I(if stack_empty(rt, a(0)) { 0 } else { rt.get_field(a(0), "$count").i() }),
        ("net/minecraft/world/item/ItemStack", "setCount") => {
            rt.put_field(a(0), "$count", a(1));
            V::Void
        }
        ("net/minecraft/world/item/ItemStack", "shrink" | "grow") => {
            let c = rt.get_field(a(0), "$count").i();
            let d = if name == "shrink" { -a(1).i() } else { a(1).i() };
            rt.put_field(a(0), "$count", V::I(c + d));
            V::Void
        }
        ("net/minecraft/world/item/ItemStack", "is") if desc == "(Lnet/minecraft/world/item/Item;)Z" => {
            let (held, other) = (rt.get_field(a(0), "$item"), a(1));
            let same = held == other || item_like_name(rt, held) == item_like_name(rt, other);
            V::I((same && !stack_empty(rt, a(0))) as i32)
        }
        ("net/minecraft/world/item/ItemStack", "copy") => {
            if stack_empty(rt, a(0)) {
                return get_static(rt, "net/minecraft/world/item/ItemStack", "EMPTY");
            }
            let i = rt.get_field(a(0), "$item");
            let c = rt.get_field(a(0), "$count").i();
            let s = stack_of(rt, i, c);
            crate::components::copy_patch(rt, a(0), s);
            s
        }
        ("net/minecraft/world/item/ItemStack", "copyWithCount") => {
            if stack_empty(rt, a(0)) {
                return get_static(rt, "net/minecraft/world/item/ItemStack", "EMPTY");
            }
            let i = rt.get_field(a(0), "$item");
            let s = stack_of(rt, i, a(1).i());
            crate::components::copy_patch(rt, a(0), s);
            s
        }
        // `consume(n, entity)`: the stack shrinks by n unless the entity builds from infinite
        // materials (a creative player); `consumeAndReturn` first copies n of it, to give back.
        ("net/minecraft/world/item/ItemStack", "consume" | "consumeAndReturn")
            if desc.starts_with("(ILnet/minecraft/world/entity/LivingEntity;)") =>
        {
            let back = if name == "consumeAndReturn" {
                if stack_empty(rt, a(0)) {
                    get_static(rt, "net/minecraft/world/item/ItemStack", "EMPTY").unwrap_or(V::NULL)
                } else {
                    let i = rt.get_field(a(0), "$item");
                    let s = stack_of(rt, i, a(1).i());
                    crate::components::copy_patch(rt, a(0), s);
                    s
                }
            } else {
                V::Void
            };
            let infinite =
                a(2).r() != 0 && crate::tanks::virt(rt, HAS_INFINITE_MATERIALS, &[a(2)]).i() != 0;
            if !infinite {
                let c = rt.get_field(a(0), "$count").i();
                rt.put_field(a(0), "$count", V::I(c - a(1).i()));
            }
            back
        }
        // `split(n)`: a stack of up to n taken off this one.
        ("net/minecraft/world/item/ItemStack", "split") => {
            let have = if stack_empty(rt, a(0)) { 0 } else { rt.get_field(a(0), "$count").i() };
            let take = a(1).i().min(have).max(0);
            let i = rt.get_field(a(0), "$item");
            let out = if take == 0 { get_static(rt, "net/minecraft/world/item/ItemStack", "EMPTY").unwrap_or(V::NULL) } else { stack_of(rt, i, take) };
            if take != 0 {
                crate::components::copy_patch(rt, a(0), out);
            }
            rt.put_field(a(0), "$count", V::I(have - take));
            out
        }
        ("net/minecraft/world/item/ItemStack", "isSameItem" | "isSameItemSameComponents" | "matches") => {
            let (x, y) = (a(0), a(1));
            let same = match (stack_empty(rt, x), stack_empty(rt, y)) {
                (true, true) => true,
                (false, false) => item_like_name(rt, rt.get_field(x, "$item")) == item_like_name(rt, rt.get_field(y, "$item")),
                _ => false,
            };
            let counts = name != "matches" || rt.get_field(x, "$count").i() == rt.get_field(y, "$count").i() || stack_empty(rt, x);
            let components = name == "isSameItem" || stack_empty(rt, x) || crate::components::same_patch(rt, x, y);
            V::I((same && counts && components) as i32)
        }
        ("net/minecraft/world/item/ItemStack", "is") if desc.starts_with("(Lnet/minecraft/tags/TagKey;") => {
            let item = item_like_name(rt, rt.get_field(a(0), "$item"));
            let tag = rt.str_of(rt.get_field(a(1), "$location")).unwrap_or("").to_owned();
            V::I((!stack_empty(rt, a(0)) && crate::recipes::item_in_tag(rt, &item, &tag)) as i32)
        }
        // `getMaxStackSize() > 1`, and not a damaged one.
        ("net/minecraft/world/item/ItemStack", "isStackable") => {
            let item = item_like_name(rt, rt.get_field(a(0), "$item"));
            let limit = rt.host().and_then(|h| h.item_stack_limit(&item)).unwrap_or(64);
            let damaged = crate::components::max_damage(rt, a(0)) > 0 && crate::components::damage(rt, a(0)) > 0;
            V::I((limit > 1 && !damaged) as i32)
        }
        // Whether the stack may go inside an item that holds items (a bundle, Create's toolbox,
        // NeoForge's `ComponentItemHandler`): NeoForge's `IItemStackExtension` asks the item's
        // `canFitInsideContainerItems(stack)`, which asks vanilla's `canFitInsideContainerItems()`;
        // a mod item's override of either answers (Create's packages and minecart contraptions say
        // no).
        ("net/minecraft/world/item/ItemStack", "canFitInsideContainerItems") => {
            let item = stack_item(rt, a(0));
            fits_virtually(rt, item, Some(a(0)))
        }
        (_, "canFitInsideContainerItems") if is_item_class(owner) || owner == "net/neoforged/neoforge/common/extensions/IItemExtension" => {
            if desc.starts_with("(Lnet/minecraft/world/item/ItemStack;)") {
                fits_virtually(rt, a(0), None)
            } else {
                V::I(fits_inside_container_items(rt, a(0)) as i32)
            }
        }
        // What crafting with the stack's item leaves in the grid (NeoForge's stack-aware forms ask
        // the item the same).
        ("net/minecraft/world/item/ItemStack", "hasCraftingRemainingItem") => {
            let left = !stack_empty(rt, a(0)) && crafting_remainder(rt, rt.get_field(a(0), "$item")).is_some();
            V::I(left as i32)
        }
        ("net/minecraft/world/item/ItemStack", "getCraftingRemainingItem") => {
            let left = if stack_empty(rt, a(0)) { None } else { crafting_remainder(rt, rt.get_field(a(0), "$item")) };
            match left {
                Some(r) => stack(rt, &r, 1),
                None => get_static(rt, "net/minecraft/world/item/ItemStack", "EMPTY").unwrap_or(V::NULL),
            }
        }
        ("net/minecraft/world/item/Item", "hasCraftingRemainingItem") => V::I(crafting_remainder(rt, a(0)).is_some() as i32),
        ("net/minecraft/world/item/Item", "getCraftingRemainingItem") => match (crafting_remainder(rt, a(0)), desc.ends_with(")Lnet/minecraft/world/item/Item;")) {
            (Some(r), true) => item(rt, &r),
            (Some(r), false) => stack(rt, &r, 1),
            (None, true) => V::NULL,
            (None, false) => get_static(rt, "net/minecraft/world/item/ItemStack", "EMPTY").unwrap_or(V::NULL),
        },
        // NBT in `ItemStack.CODEC`'s form, `{id, count, components}`.
        ("net/minecraft/world/item/ItemStack", "save" | "saveOptional") => {
            let into = desc.contains(";Lnet/minecraft/nbt/Tag;)");
            let tag = if into { a(2) } else { crate::tags::new_compound(rt) };
            if stack_empty(rt, a(0)) {
                if name == "save" {
                    let e = rt.new_object("java/lang/IllegalStateException");
                    let m = rt.string("Cannot encode empty ItemStack");
                    rt.put_field(e, "message", m);
                    rt.thrown = Some(e);
                }
                return Some(tag);
            }
            save_stack(rt, a(0), tag);
            tag
        }
        ("net/minecraft/world/item/ItemStack", "parseOptional" | "parse") => {
            let map = if rt.class_of(a(1)) == "net/minecraft/nbt/CompoundTag" { crate::nbt::from_runtime(rt, a(1)) } else { crate::nbt::from_runtime(rt, a(2)) };
            let parsed = parse_stack(rt, &map);
            if name == "parse" {
                let o = rt.new_object("java/util/Optional");
                rt.put_field(o, "value", parsed.unwrap_or(V::NULL));
                return Some(o);
            }
            match parsed {
                Some(s) => s,
                None => get_static(rt, "net/minecraft/world/item/ItemStack", "EMPTY").unwrap_or(V::NULL),
            }
        }
        ("net/minecraft/world/item/ItemStack", "useOn") => crate::item_use::stack_use_on(rt, a(0), a(1)),
        ("net/minecraft/world/item/ItemStack", "onItemUseFirst") => crate::item_use::stack_use_first(rt, a(0), a(1)),
        ("net/minecraft/world/item/ItemStack", "doesSneakBypassUse") => crate::item_use::sneak_bypasses(rt, a(0), a(1), a(2), a(3)),
        // The item's `max_stack_size`, as the game knows it.
        ("net/minecraft/world/item/ItemStack", "getMaxStackSize") => {
            let item = item_like_name(rt, rt.get_field(a(0), "$item"));
            V::I(rt.host().and_then(|h| h.item_stack_limit(&item)).unwrap_or(64) as i32)
        }
        // Names, as 1.21.1's `Item` and `ItemStack` give them: the description id, its translatable
        // text (`getDescription`, `getName(stack)`), and a stack's name (its custom name, else its
        // item name, else the item's). Each step is a virtual call, so a mod's override is the one
        // that answers.
        (o, "getDescriptionId" | "getOrCreateDescriptionId") if is_item_class(o) && desc == "()Ljava/lang/String;" => {
            let id = item_description_id(rt, a(0));
            rt.string(&id)
        }
        (o, "getDescriptionId") if is_item_class(o) && desc == "(Lnet/minecraft/world/item/ItemStack;)Ljava/lang/String;" => crate::menus::virt(rt, ITEM_DESCRIPTION_ID, a(0), &[]),
        (o, "getDescription") if is_item_class(o) && desc == "()Lnet/minecraft/network/chat/Component;" => {
            let id = crate::menus::virt(rt, ITEM_DESCRIPTION_ID, a(0), &[]);
            component(rt, id, true, V::NULL)
        }
        (o, "getName") if is_item_class(o) && desc == "(Lnet/minecraft/world/item/ItemStack;)Lnet/minecraft/network/chat/Component;" => {
            let id = crate::menus::virt(rt, ITEM_STACK_DESCRIPTION_ID, a(0), &[a(1)]);
            component(rt, id, true, V::NULL)
        }
        ("net/minecraft/world/item/ItemStack", "getDescriptionId") => {
            let item = stack_item(rt, a(0));
            crate::menus::virt(rt, ITEM_STACK_DESCRIPTION_ID, item, &[a(0)])
        }
        ("net/minecraft/world/item/ItemStack", "getHoverName") => hover_name(rt, a(0)),
        // `Component.empty().append(getHoverName())`, italic for a custom name, in square brackets.
        // (Vanilla also colours it by rarity and shows the stack on hover.)
        ("net/minecraft/world/item/ItemStack", "getDisplayName") => {
            let name = hover_name(rt, a(0));
            let empty = crate::api::call(rt, "net/minecraft/network/chat/Component.empty.()Lnet/minecraft/network/chat/MutableComponent;", &[]);
            let mut c = crate::api::call(rt, "net/minecraft/network/chat/MutableComponent.append.(Lnet/minecraft/network/chat/Component;)Lnet/minecraft/network/chat/MutableComponent;", &[empty, name]);
            if stack_component(rt, a(0), "net/minecraft/core/component/DataComponents.CUSTOM_NAME").r() != 0 {
                let italic = crate::api::get_static(rt, "net/minecraft/ChatFormatting.ITALIC");
                c = crate::api::call(rt, "net/minecraft/network/chat/MutableComponent.withStyle.(Lnet/minecraft/ChatFormatting;)Lnet/minecraft/network/chat/MutableComponent;", &[c, italic]);
            }
            crate::api::call(rt, "net/minecraft/network/chat/ComponentUtils.wrapInSquareBrackets.(Lnet/minecraft/network/chat/Component;)Lnet/minecraft/network/chat/MutableComponent;", &[c])
        }

        // Maths, randomness, text
        ("net/minecraft/util/Mth", "clamp") => match desc {
            "(III)I" => V::I(a(0).i().max(a(1).i()).min(a(2).i())),
            "(FFF)F" => V::F(if a(0).f() < a(1).f() { a(1).f() } else { java_min(f64::from(a(0).f()), f64::from(a(2).f())) as f32 }),
            "(DDD)D" => V::D(if a(0).d() < a(1).d() { a(1).d() } else { java_min(a(0).d(), a(2).d()) }),
            "(JJJ)J" => V::J(a(0).j().max(a(1).j()).min(a(2).j())),
            _ => return None,
        },
        ("net/minecraft/util/Mth", "floor") => V::I(if desc.starts_with("(D") { a(0).d().floor() as i32 } else { a(0).f().floor() as i32 }),
        ("net/minecraft/util/Mth", "ceil") => V::I(if desc.starts_with("(D") { a(0).d().ceil() as i32 } else { a(0).f().ceil() as i32 }),
        ("net/minecraft/util/Mth", "abs") => if desc == "(I)I" { V::I(a(0).i().wrapping_abs()) } else { V::F(a(0).f().abs()) },
        ("net/minecraft/util/Mth", "lerp") => match desc {
            "(FFF)F" => V::F(a(1).f() + a(0).f() * (a(2).f() - a(1).f())),
            "(DDD)D" => V::D(a(1).d() + a(0).d() * (a(2).d() - a(1).d())),
            _ => return None,
        },
        ("net/minecraft/util/Mth", "sqrt") => V::F(a(0).f().sqrt()),
        ("net/minecraft/util/Mth", "positiveModulo") => match desc {
            "(II)I" => V::I(a(0).i().rem_euclid(a(1).i().max(1))),
            "(FF)F" => V::F(a(0).f().rem_euclid(a(1).f())),
            "(DD)D" => V::D(a(0).d().rem_euclid(a(1).d())),
            _ => return None,
        },
        // `Math.abs(b - a) < 1.0E-5F`, for floats and doubles alike.
        ("net/minecraft/util/Mth", "equal") => V::I(if desc == "(FF)Z" { (a(1).f() - a(0).f()).abs() < 1.0e-5 } else { (a(1).d() - a(0).d()).abs() < f64::from(1.0e-5f32) } as i32),
        ("net/minecraft/util/Mth", "sign") => V::I(if a(0).d() > 0.0 { 1 } else if a(0).d() < 0.0 { -1 } else { 0 }),
        // `RandomSource.create(seed)` is vanilla's legacy source of that seed; `create()` and the
        // thread kinds are seeded from the runtime's stream, as vanilla's from a unique seed.
        (RANDOM_SOURCE, "create") if desc.starts_with("(J)") => seeded_random(rt, a(0).j()),
        (RANDOM_SOURCE, "create" | "createNewThreadLocalInstance" | "createThreadSafe") => {
            let seed = next_random(rt) as i64;
            seeded_random(rt, seed)
        }
        // `new LegacyRandomSource(seed)` and its kinds.
        (o, "<init>") if is_random_class(o) && desc == "(J)V" => {
            set_random_seed(rt, a(0), a(1).j());
            V::Void
        }
        (o, n) if is_random_class(o) => return random_call(rt, n, desc, args),
        (POSITIONAL_RANDOM | LEGACY_POSITIONAL_RANDOM, n) => {
            return positional_random_call(rt, n, desc, args);
        }
        // An entity's own source (vanilla's field `random`, made with it): a mod's entity keeps its
        // own (`mod_entities`); the game's entities and players, and a loot roll, one each.
        (o, "getRandom")
            if desc == "()Lnet/minecraft/util/RandomSource;"
                && (crate::vanilla_entities::is_a(o, "net/minecraft/world/entity/Entity")
                    || is_player(o)
                    || o == "net/minecraft/world/level/storage/loot/LootContext") =>
        {
            own_random(rt, a(0))
        }
        // `Util.getRandom(Direction.values(), random)`.
        ("net/minecraft/core/Direction", "getRandom")
            if desc == "(Lnet/minecraft/util/RandomSource;)Lnet/minecraft/core/Direction;" =>
        {
            let i = crate::api::call(
                rt,
                "net/minecraft/util/RandomSource.nextInt.(I)I",
                &[a(0), V::I(6)],
            )
            .i();
            direction(rt, i64::from(i))
        }
        // A crash report a mod makes to throw (`throw new ReportedException(CrashReport.forThrowable(e,
        // "Ticking block entity"))`): its title and exception, which the exception carries as its
        // message and cause, as vanilla's `getMessage` and `getCause` give them (the hook logs both).
        // Its categories' details are for the report's text, which nobody writes here.
        (CRASH_REPORT, "<init>") => {
            rt.put_field(a(0), "$title", a(1));
            rt.put_field(a(0), "$exception", a(2));
            V::Void
        }
        // A `ReportedException`'s own report, else a new one.
        (CRASH_REPORT, "forThrowable") if rt.class_of(a(0)) == REPORTED_EXCEPTION => {
            rt.get_field(a(0), "$report")
        }
        (CRASH_REPORT, "forThrowable") => {
            let r = rt.new_object(CRASH_REPORT);
            rt.put_field(r, "$title", a(1));
            rt.put_field(r, "$exception", a(0));
            r
        }
        (CRASH_REPORT, "getTitle") => rt.get_field(a(0), "$title"),
        (CRASH_REPORT, "getException") => rt.get_field(a(0), "$exception"),
        (CRASH_REPORT, "addCategory") => {
            let c = rt.new_object(CRASH_REPORT_CATEGORY);
            rt.put_field(c, "$title", a(1));
            c
        }
        (CRASH_REPORT_CATEGORY, "setDetail") => a(0),
        (CRASH_REPORT_CATEGORY, "setDetailError") => V::Void,
        (REPORTED_EXCEPTION, "<init>") => {
            let (title, cause) = (
                rt.get_field(a(1), "$title"),
                rt.get_field(a(1), "$exception"),
            );
            rt.put_field(a(0), "$report", a(1));
            rt.put_field(a(0), "message", title);
            rt.put_field(a(0), "cause", cause);
            V::Void
        }
        (REPORTED_EXCEPTION, "getReport") => rt.get_field(a(0), "$report"),
        (REPORTED_EXCEPTION, "getMessage") => rt.get_field(a(0), "message"),
        (REPORTED_EXCEPTION, "getCause") => rt.get_field(a(0), "cause"),
        // Nothing here caps how much NBT a read may take.
        ("net/minecraft/nbt/NbtAccounter", "unlimitedHeap" | "create" | "defaultQuota") => rt.intern("compat:nbt-accounter".into(), |rt| rt.new_object("net/minecraft/nbt/NbtAccounter")),
        ("net/minecraft/nbt/NbtAccounter", "accountBytes" | "pushDepth" | "popDepth") => V::Void,
        ("net/minecraft/network/chat/Component", "literal" | "translatable") => {
            let with = if desc.contains("[Ljava/lang/Object;") { a(1) } else { V::NULL };
            component(rt, a(0), name == "translatable", with)
        }
        // A component's contents (`getContents()`): a translation's key and arguments
        // (`TranslatableContents`) or literal text (`PlainTextContents.LiteralContents`), the same
        // object each time, as vanilla's component holds one.
        ("net/minecraft/network/chat/Component" | MUTABLE_COMPONENT, "getContents") => {
            component_contents(rt, a(0))
        }
        // `MutableComponent.create(contents)`: the component of those contents.
        (MUTABLE_COMPONENT, "create") => component_of_contents(rt, a(0)),
        // `new TranslatableContents(key, fallback, args)`.
        (TRANSLATABLE_CONTENTS, "<init>") => {
            rt.put_field(a(0), "$key", a(1));
            rt.put_field(a(0), "$fallback", a(2));
            rt.put_field(a(0), "$args", a(3));
            V::Void
        }
        (TRANSLATABLE_CONTENTS, "getKey") => rt.get_field(a(0), "$key"),
        (TRANSLATABLE_CONTENTS, "getFallback") => rt.get_field(a(0), "$fallback"),
        // `getArgs()`: the arguments, an empty array where there are none (vanilla's `NO_ARGS`).
        (TRANSLATABLE_CONTENTS, "getArgs") => {
            let args = rt.get_field(a(0), "$args");
            if args.r() != 0 {
                args
            } else {
                rt.alloc("[Ljava/lang/Object;", Data::Array(Vec::new()))
            }
        }
        (PLAIN_TEXT_CONTENTS | LITERAL_CONTENTS, "text") => rt.get_field(a(0), "text"),
        (PLAIN_TEXT_CONTENTS, "create") => crate::records::make(rt, LITERAL_CONTENTS, &[a(0)]),
        // `WidgetSprites`, a record: its shorter constructors repeat sprites as vanilla's do (two:
        // one for enabled, one for focused; three: the disabled one serves unfocused and focused),
        // and `get(enabled, focused)` is the sprite of that state.
        (WIDGET_SPRITES, "<init>")
            if desc == WIDGET_SPRITES_TWO || desc == WIDGET_SPRITES_THREE =>
        {
            let [enabled, disabled, focused] = if desc == WIDGET_SPRITES_TWO {
                [a(1), a(1), a(2)]
            } else {
                [a(1), a(2), a(3)]
            };
            // Disabled and focused is the second sprite either way: the focused one of two, the
            // disabled one of three.
            for (field, v) in [
                ("enabled", enabled),
                ("disabled", disabled),
                ("enabledFocused", focused),
                ("disabledFocused", a(2)),
            ] {
                rt.put_field(a(0), field, v);
            }
            V::Void
        }
        // `new HoverEvent(action, value)`: what a style shows on hover (kept with it, as its events
        // are); `getValue(action)` is the value when the event is of that action, else null.
        (HOVER_EVENT, "<init>")
            if desc == "(Lnet/minecraft/network/chat/HoverEvent$Action;Ljava/lang/Object;)V" =>
        {
            rt.put_field(a(0), "$action", a(1));
            rt.put_field(a(0), "$value", a(2));
            V::Void
        }
        (HOVER_EVENT, "getAction") => rt.get_field(a(0), "$action"),
        (HOVER_EVENT, "getValue") => {
            if rt.get_field(a(0), "$action") == a(1) {
                rt.get_field(a(0), "$value")
            } else {
                V::NULL
            }
        }
        // Equal for the same action and an equal value, hashed as its `TypedHoverEvent` record.
        (HOVER_EVENT, "equals") => {
            let other = a(1);
            let same = a(0) == other
                || (other.r() != 0
                    && rt.class_of(other) == HOVER_EVENT
                    && rt.get_field(a(0), "$action") == rt.get_field(other, "$action")
                    && {
                        let (x, y) = (rt.get_field(a(0), "$value"), rt.get_field(other, "$value"));
                        x == y || (x.r() != 0 && crate::jutil::equals(rt, x, y))
                    });
            V::I(i32::from(same))
        }
        (HOVER_EVENT, "hashCode") => {
            let (action, value) = (rt.get_field(a(0), "$action"), rt.get_field(a(0), "$value"));
            let action = crate::jutil::hash(rt, action);
            let value = if value.r() == 0 {
                0
            } else {
                crate::jutil::hash(rt, value)
            };
            V::I(action.wrapping_mul(31).wrapping_add(value))
        }
        // Vanilla's three actions, each allowed from a server.
        (HOVER_ACTION, "getSerializedName") => rt.get_field(a(0), "$name"),
        (HOVER_ACTION, "isAllowedFromServer") => V::I(1),
        (HOVER_ACTION, "cast") => a(1),
        (HOVER_ACTION, "toString") => {
            let named = format!(
                "<action {}>",
                rt.str_of(rt.get_field(a(0), "$name")).unwrap_or("")
            );
            rt.string(&named)
        }
        // `new ItemStackInfo(stack)`: the stack as it was then (its item, count and components),
        // which `getItemStack()` gives, the same object each time.
        (ITEM_STACK_INFO, "<init>") if desc == "(Lnet/minecraft/world/item/ItemStack;)V" => {
            let copy = crate::api::call(
                rt,
                "net/minecraft/world/item/ItemStack.copy.()Lnet/minecraft/world/item/ItemStack;",
                &[a(1)],
            );
            rt.put_field(a(0), "$stack", copy);
            V::Void
        }
        (ITEM_STACK_INFO, "getItemStack") => rt.get_field(a(0), "$stack"),
        // `new EntityTooltipInfo(type, id, name)`: its public fields, the name optional; its tooltip
        // the name if any, the type (`gui.entity_tooltip.type`) and the UUID, made once.
        (ENTITY_TOOLTIP_INFO, "<init>") => {
            let named = if desc.ends_with("Ljava/util/Optional;)V") {
                a(3)
            } else {
                optional(rt, a(3))
            };
            rt.put_field(a(0), "type", a(1));
            rt.put_field(a(0), "id", a(2));
            rt.put_field(a(0), "name", named);
            V::Void
        }
        (ENTITY_TOOLTIP_INFO, "getTooltipLines") => {
            let cached = rt.get_field(a(0), "$lines");
            if cached.r() != 0 {
                return Some(cached);
            }
            let mut lines = Vec::new();
            let named = rt.get_field(rt.get_field(a(0), "name"), "value");
            if named.r() != 0 {
                lines.push(named);
            }
            let ty = rt.get_field(a(0), "type");
            let kind = crate::api::call(
                rt,
                "net/minecraft/world/entity/EntityType.getDescription.()Lnet/minecraft/network/chat/Component;",
                &[ty],
            );
            let key = rt.string("gui.entity_tooltip.type");
            let with = rt.alloc("[Ljava/lang/Object;", Data::Array(vec![kind]));
            lines.push(component(rt, key, true, with));
            let uuid = rt.get_field(a(0), "id");
            let id = crate::api::call(rt, "java/util/UUID.toString.()Ljava/lang/String;", &[uuid]);
            lines.push(component(rt, id, false, V::NULL));
            let list = crate::jutil::new_list(rt, lines);
            rt.put_field(a(0), "$lines", list);
            list
        }
        (WIDGET_SPRITES, "get") if desc == "(ZZ)Lnet/minecraft/resources/ResourceLocation;" => {
            let field = match (a(1).i() != 0, a(2).i() != 0) {
                (true, true) => "enabledFocused",
                (true, false) => "enabled",
                (false, true) => "disabledFocused",
                (false, false) => "disabled",
            };
            rt.get_field(a(0), field)
        }
        // `CriterionTrigger.createCriterion(instance)`, its default method: the criterion record
        // of the trigger and the instance (Create's advancements, built in code).
        ("net/minecraft/advancements/CriterionTrigger", "createCriterion") => crate::records::make(rt, "net/minecraft/advancements/Criterion", &[a(0), a(1)]),
        // Advancements a mod builds in code (for its data generator; the game reads the files the
        // mod ships): the builders keep nothing, each step gives the builder back and `build` an
        // advancement holder of the id; a criterion the vanilla shorthands make has no trigger.
        (ADVANCEMENT_BUILDER | ITEM_PREDICATE_BUILDER, _) if desc.ends_with(&format!(")L{owner};")) => {
            let class = if owner == ADVANCEMENT_BUILDER { ADVANCEMENT_BUILDER } else { ITEM_PREDICATE_BUILDER };
            if a(0).r() != 0 && rt.class_of(a(0)) == class { a(0) } else { rt.new_object(class) }
        }
        (ADVANCEMENT_BUILDER, "build" | "save") => {
            let id = if name == "build" { a(1) } else { a(2) };
            crate::records::make(rt, "net/minecraft/advancements/AdvancementHolder", &[id, V::NULL])
        }
        (ITEM_PREDICATE_BUILDER, "build") => crate::records::make(rt, "net/minecraft/advancements/critereon/ItemPredicate", &[]),
        (o, "hasItems") if o == "net/minecraft/advancements/critereon/InventoryChangeTrigger$TriggerInstance" => {
            crate::records::make(rt, "net/minecraft/advancements/Criterion", &[V::NULL, V::NULL])
        }
        // A player's profile: its id and name.
        ("com/mojang/authlib/GameProfile", "<init>") if desc == "(Ljava/util/UUID;Ljava/lang/String;)V" => {
            rt.put_field(a(0), "$id", a(1));
            rt.put_field(a(0), "$name", a(2));
            V::Void
        }
        ("com/mojang/authlib/GameProfile", "getId") => rt.get_field(a(0), "$id"),
        ("com/mojang/authlib/GameProfile", "getName") => rt.get_field(a(0), "$name"),
        // A mod's worldgen feature (`Feature<C>` subclass): its configuration's codec.
        ("net/minecraft/world/level/levelgen/feature/Feature", "<init>") if desc == "(Lcom/mojang/serialization/Codec;)V" => {
            rt.put_field(a(0), "$configuredCodec", a(1));
            V::Void
        }
        // A command argument over a `StringRepresentable` enum (`StringRepresentableArgument(codec, values)`).
        ("net/minecraft/commands/arguments/StringRepresentableArgument", "<init>") => {
            rt.put_field(a(0), "$codec", a(1));
            rt.put_field(a(0), "$values", a(2));
            V::Void
        }
        ("net/minecraft/network/chat/Component" | "net/minecraft/network/chat/MutableComponent", "getString") if desc == "()Ljava/lang/String;" => {
            // The text and its siblings', translated where the game has the player's language (a
            // translation's key stands for it elsewhere).
            let text = crate::gui::plain(rt, a(0));
            rt.string(&text)
        }
        ("net/minecraft/network/chat/Component", "empty") => {
            let t = rt.string("");
            component(rt, t, false, V::NULL)
        }
        // Styles (`withStyle` of `ChatFormatting`s: colours and formats) and siblings (`append`);
        // `plainCopy` keeps neither, `copy` both.
        ("net/minecraft/network/chat/Component" | "net/minecraft/network/chat/MutableComponent", "withStyle") if desc.contains("ChatFormatting;") => {
            let formats: Vec<V> = match desc {
                "(Lnet/minecraft/ChatFormatting;)Lnet/minecraft/network/chat/MutableComponent;" => vec![a(1)],
                "([Lnet/minecraft/ChatFormatting;)Lnet/minecraft/network/chat/MutableComponent;" => crate::jutil::elements(rt, a(1)),
                _ => Vec::new(),
            };
            let mut style = rt.str_of(rt.get_field(a(0), "$style")).unwrap_or("").to_owned();
            for f in formats {
                match constant(rt, f).as_str() {
                    "RESET" => style.clear(),
                    n => {
                        style.push_str(n);
                        style.push(',');
                    }
                }
            }
            let style = rt.string(&style);
            rt.put_field(a(0), "$style", style);
            a(0)
        }
        ("net/minecraft/network/chat/Component" | "net/minecraft/network/chat/MutableComponent", "append") => {
            let sibling = if desc.starts_with("(Ljava/lang/String;)") { component(rt, a(1), false, V::NULL) } else { a(1) };
            let mut extra = crate::jutil::elements(rt, rt.get_field(a(0), "$extra"));
            extra.push(sibling);
            let list = crate::jutil::new_list(rt, extra);
            rt.put_field(a(0), "$extra", list);
            a(0)
        }
        ("net/minecraft/network/chat/Component" | "net/minecraft/network/chat/MutableComponent", "plainCopy" | "copy") => {
            let (text, translatable, with) = (rt.get_field(a(0), "$text"), rt.get_field(a(0), "$translate").i() != 0, rt.get_field(a(0), "$with"));
            let c = component(rt, text, translatable, with);
            if name == "copy" {
                for k in ["$style", "$extra"] {
                    let v = rt.get_field(a(0), k);
                    rt.put_field(c, k, v);
                }
            }
            c
        }
        // A component as JSON text and back (`Component.Serializer`), as a block entity saves one.
        ("net/minecraft/network/chat/Component$Serializer", "toJson") => {
            let j = component_json(rt, a(0)).to_string();
            rt.string(&j)
        }
        ("net/minecraft/network/chat/Component$Serializer", "fromJson" | "fromJsonLenient") if desc.starts_with("(Ljava/lang/String;") => {
            match rt.str_of(a(0)).and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok()) {
                Some(j) => component_from_json(rt, &j),
                None => {
                    let t = rt.str_of(a(0)).unwrap_or("").to_owned();
                    crate::glue::trace_some(rt, "unread-json", &t, 8, |_| {
                        format!("component JSON unread: {t:?}")
                    });
                    V::NULL
                }
            }
        }
        // A vanilla enum's name in data (`StringRepresentable.getSerializedName`, named on the
        // interface or on its class): its constant's, as vanilla spells it.
        (_, "getSerializedName") if desc == "()Ljava/lang/String;" && api_constant(rt, a(0)) => {
            let class = rt.class_of(a(0));
            let constant = constant(rt, a(0));
            let text = serialized_name(class, &constant);
            rt.string(&text)
        }
        _ => return None,
    })
}

/// Whether `v` is a constant of one of vanilla's enums as the runtime makes them (`api_enum`: a
/// name and an ordinal).
fn api_constant(rt: &Rt, v: V) -> bool {
    v.r() != 0
        && matches!(rt.get_field(v, "$ordinal"), V::I(_))
        && rt.str_of(rt.get_field(v, "$name")).is_some()
        && rt.class_of(v).starts_with("net/minecraft/")
}

/// Vanilla's serialized names that aren't their constant's name in lower case, by class and
/// constant.
const SERIALIZED_NAMES: [(&str, &str, &str); 23] = [
    (
        "net/minecraft/world/level/levelgen/DensityFunctions$Marker$Type",
        "FlatCache",
        "flat_cache",
    ),
    (
        "net/minecraft/world/level/levelgen/DensityFunctions$Marker$Type",
        "Cache2D",
        "cache_2d",
    ),
    (
        "net/minecraft/world/level/levelgen/DensityFunctions$Marker$Type",
        "CacheOnce",
        "cache_once",
    ),
    (
        "net/minecraft/world/level/levelgen/DensityFunctions$Marker$Type",
        "CacheAllInCell",
        "cache_all_in_cell",
    ),
    (
        "net/minecraft/world/item/ItemDisplayContext",
        "THIRD_PERSON_LEFT_HAND",
        "thirdperson_lefthand",
    ),
    (
        "net/minecraft/world/item/ItemDisplayContext",
        "THIRD_PERSON_RIGHT_HAND",
        "thirdperson_righthand",
    ),
    (
        "net/minecraft/world/item/ItemDisplayContext",
        "FIRST_PERSON_LEFT_HAND",
        "firstperson_lefthand",
    ),
    (
        "net/minecraft/world/item/ItemDisplayContext",
        "FIRST_PERSON_RIGHT_HAND",
        "firstperson_righthand",
    ),
    (
        "net/minecraft/world/level/block/state/properties/PistonType",
        "DEFAULT",
        "normal",
    ),
    (
        "net/minecraft/world/level/block/Rotation",
        "CLOCKWISE_180",
        "180",
    ),
    (
        "net/minecraft/world/level/block/Rotation",
        "CLOCKWISE_90",
        "clockwise_90",
    ),
    (
        "net/minecraft/server/packs/PackType",
        "CLIENT_RESOURCES",
        "assets",
    ),
    ("net/minecraft/server/packs/PackType", "SERVER_DATA", "data"),
    ("net/minecraft/client/CloudStatus", "OFF", "false"),
    ("net/minecraft/client/CloudStatus", "FANCY", "true"),
    (
        "net/minecraft/client/gui/font/FontOption",
        "JAPANESE_VARIANTS",
        "jp",
    ),
    (
        "net/minecraft/client/telemetry/TelemetryProperty$ServerType",
        "OTHER",
        "server",
    ),
    (
        "net/minecraft/world/level/levelgen/structure/StructureSpawnOverride$BoundingBoxType",
        "STRUCTURE",
        "full",
    ),
    (
        "net/minecraft/world/level/levelgen/structure/StructureSpawnOverride$BoundingBoxType",
        "PIECE",
        "piece",
    ),
    (
        "net/minecraft/world/level/levelgen/DensityFunctions$WeirdScaledSampler$RarityValueMapper",
        "TYPE1",
        "type_1",
    ),
    (
        "net/minecraft/world/level/levelgen/DensityFunctions$WeirdScaledSampler$RarityValueMapper",
        "TYPE2",
        "type_2",
    ),
    (
        "net/minecraft/world/item/enchantment/effects/SpawnParticlesEffect$PositionSourceType",
        "BOUNDING_BOX",
        "in_bounding_box",
    ),
    (
        "net/minecraft/world/item/enchantment/effects/SpawnParticlesEffect$PositionSourceType",
        "ENTITY_POSITION",
        "entity_position",
    ),
];

/// `getSerializedName` of vanilla's enum constant `constant` of `class`: its name in lower case,
/// but a heightmap's (as it is), a display slot's for a team colour (`sidebar.team.<colour>`) and
/// the other names vanilla gives ([`SERIALIZED_NAMES`]).
pub(crate) fn serialized_name(class: &str, constant: &str) -> String {
    if let Some((_, _, n)) = SERIALIZED_NAMES
        .iter()
        .find(|(c, k, _)| *c == class && *k == constant)
    {
        return (*n).to_owned();
    }
    match class {
        "net/minecraft/world/level/levelgen/Heightmap$Types" => constant.to_owned(),
        "net/minecraft/world/scores/DisplaySlot" if constant.starts_with("TEAM_") => {
            format!(
                "sidebar.team.{}",
                constant["TEAM_".len()..].to_ascii_lowercase()
            )
        }
        _ => constant.to_ascii_lowercase(),
    }
}

const MUTABLE_COMPONENT: &str = "net/minecraft/network/chat/MutableComponent";
const TRANSLATABLE_CONTENTS: &str = "net/minecraft/network/chat/contents/TranslatableContents";
const WIDGET_SPRITES: &str = "net/minecraft/client/gui/components/WidgetSprites";
const HOVER_EVENT: &str = "net/minecraft/network/chat/HoverEvent";
const HOVER_ACTION: &str = "net/minecraft/network/chat/HoverEvent$Action";
const ITEM_STACK_INFO: &str = "net/minecraft/network/chat/HoverEvent$ItemStackInfo";
const ENTITY_TOOLTIP_INFO: &str = "net/minecraft/network/chat/HoverEvent$EntityTooltipInfo";
/// `HoverEvent.Action`'s constants: their fields and serialized names.
const HOVER_ACTIONS: [(&str, &str); 3] = [
    ("SHOW_TEXT", "show_text"),
    ("SHOW_ITEM", "show_item"),
    ("SHOW_ENTITY", "show_entity"),
];
const WIDGET_SPRITES_TWO: &str =
    "(Lnet/minecraft/resources/ResourceLocation;Lnet/minecraft/resources/ResourceLocation;)V";
const WIDGET_SPRITES_THREE: &str = "(Lnet/minecraft/resources/ResourceLocation;Lnet/minecraft/resources/ResourceLocation;Lnet/minecraft/resources/ResourceLocation;)V";
const HAS_INFINITE_MATERIALS: (&str, &str) = (
    "net/minecraft/world/entity/LivingEntity.hasInfiniteMaterials.()Z",
    "hasInfiniteMaterials.()Z",
);
const PLAIN_TEXT_CONTENTS: &str = "net/minecraft/network/chat/contents/PlainTextContents";
const LITERAL_CONTENTS: &str =
    "net/minecraft/network/chat/contents/PlainTextContents$LiteralContents";

/// `Component.getContents()` of component `c`: its text as vanilla's contents classes hold it,
/// made the first time it is asked.
fn component_contents(rt: &mut Rt, c: V) -> V {
    let made = rt.get_field(c, "$contents");
    if made.r() != 0 {
        return made;
    }
    let text = rt.get_field(c, "$text");
    let contents = if rt.get_field(c, "$translate").i() != 0 {
        let t = rt.new_object(TRANSLATABLE_CONTENTS);
        let with = rt.get_field(c, "$with");
        rt.put_field(t, "$key", text);
        rt.put_field(t, "$args", with);
        t
    } else {
        crate::records::make(rt, LITERAL_CONTENTS, &[text])
    };
    rt.put_field(c, "$contents", contents);
    contents
}

/// `MutableComponent.create(contents)`: a translation's or literal text's component; contents of
/// another kind (a key bind's, a score's) make a component of no text that gives them back.
fn component_of_contents(rt: &mut Rt, contents: V) -> V {
    let c = match rt.class_of(contents) {
        TRANSLATABLE_CONTENTS => {
            let (key, args) = (
                rt.get_field(contents, "$key"),
                rt.get_field(contents, "$args"),
            );
            component(rt, key, true, args)
        }
        LITERAL_CONTENTS => {
            let text = rt.get_field(contents, "text");
            component(rt, text, false, V::NULL)
        }
        _ => {
            let none = rt.string("");
            component(rt, none, false, V::NULL)
        }
    };
    rt.put_field(c, "$contents", contents);
    c
}

/// A text component: `text` itself (`Component.literal`) or the translation key it is
/// (`Component.translatable`, with the arguments `with`, an `Object[]` or null).
/// `BlockEntity.setChanged()` of the block entity `be` once it is in a level: as vanilla's, the
/// comparators beside it read it again (`updateNeighbourForOutputSignal`) unless its block is air.
fn block_entity_changed(rt: &mut Rt, be: V) {
    let (level, pos, state) = (rt.get_field(be, "level"), rt.get_field(be, "worldPosition"), rt.get_field(be, "blockState"));
    if level.r() == 0 || pos.r() == 0 {
        return;
    }
    let text = rt.str_of(state).unwrap_or("minecraft:air").to_owned();
    let (block, _) = crate::host::split_state(&text);
    if matches!(block, "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air") {
        return;
    }
    let pos = crate::api::pos_of(rt, pos);
    rt.host_call(|h| h.update_neighbors(crate::host::NeighborUpdate::Output { pos }, block));
}

pub(crate) fn component(rt: &mut Rt, text: V, translatable: bool, with: V) -> V {
    let t = rt.new_object(MUTABLE_COMPONENT);
    rt.put_field(t, "$text", text);
    if translatable {
        rt.put_field(t, "$translate", V::I(1));
        rt.put_field(t, "$with", with);
    }
    t
}

/// A component as vanilla's serializer writes it: `{"text": ...}` or `{"translate": ..., "with": [...]}`,
/// with its style (a colour, `bold`...) and siblings (`extra`).
pub(crate) fn component_json(rt: &Rt, c: V) -> serde_json::Value {
    let mut j = component_core_json(rt, c);
    if let serde_json::Value::Object(o) = &mut j {
        // The style's tokens in order (`gui`'s: `RED`, `#RRGGBB`, `#` for no colour, `!BOLD` off).
        for f in rt.str_of(rt.get_field(c, "$style")).unwrap_or("").split(',').filter(|f| !f.is_empty()) {
            let (on, name) = f.strip_prefix('!').map_or((true, f), |n| (false, n));
            let format = match name {
                "OBFUSCATED" => "obfuscated",
                "BOLD" => "bold",
                "STRIKETHROUGH" => "strikethrough",
                "UNDERLINE" => "underlined",
                "ITALIC" => "italic",
                "RESET" => {
                    o.retain(|k, _| !matches!(k.as_str(), "color" | "bold" | "italic" | "underlined" | "strikethrough" | "obfuscated"));
                    continue;
                }
                "#" => {
                    o.remove("color");
                    continue;
                }
                colour => {
                    o.insert("color".into(), colour.to_ascii_lowercase().into());
                    continue;
                }
            };
            o.insert(format.into(), on.into());
        }
        let extra: Vec<serde_json::Value> = crate::jutil::elements(rt, rt.get_field(c, "$extra")).into_iter().map(|s| component_json(rt, s)).collect();
        if !extra.is_empty() {
            o.insert("extra".into(), extra.into());
        }
    }
    j
}

/// [`component_json`] without style and siblings.
fn component_core_json(rt: &Rt, c: V) -> serde_json::Value {
    use serde_json::{json, Value};
    let text = rt.str_of(rt.get_field(c, "$text")).unwrap_or("").to_owned();
    if rt.get_field(c, "$translate").i() == 0 {
        return json!({ "text": text });
    }
    let with: Vec<Value> = match rt.heap.get(rt.get_field(c, "$with").r() as usize).map(|o| &o.data) {
        Some(Data::Array(items)) if rt.get_field(c, "$with").r() != 0 => items
            .iter()
            .map(|&v| {
                let number = |n: V| match n {
                    V::I(i) => json!(i),
                    V::J(l) => json!(l),
                    V::F(f) => json!(f),
                    V::D(d) => json!(d),
                    _ => Value::Null,
                };
                match (v, rt.class_of(v)) {
                    (V::R(_), MUTABLE_COMPONENT) => component_json(rt, v),
                    (V::R(_), "java/lang/Boolean") => json!(rt.get_field(v, "value").i() != 0),
                    (V::R(_), c) if c.starts_with("java/lang/") && c != "java/lang/String" && rt.str_of(v).is_none() => number(rt.get_field(v, "value")),
                    (V::R(_), _) => json!(rt.str_of(v).unwrap_or("null")),
                    _ => number(v),
                }
            })
            .collect(),
        _ => Vec::new(),
    };
    let mut o = serde_json::Map::new();
    o.insert("translate".into(), Value::String(text));
    if !with.is_empty() {
        o.insert("with".into(), Value::Array(with));
    }
    Value::Object(o)
}

/// A component vanilla's serializer reads from `j`: a string is literal text, an array its first
/// part (its siblings left out), an object its `translate` (with `with`) or `text`.
pub(crate) fn component_from_json(rt: &mut Rt, j: &serde_json::Value) -> V {
    use serde_json::Value;
    match j {
        Value::Object(o) => match o.get("translate").and_then(Value::as_str) {
            Some(key) => {
                let key = rt.string(key);
                let args: Vec<V> = match o.get("with") {
                    Some(Value::Array(a)) => a
                        .iter()
                        .map(|v| match v {
                            Value::String(s) => rt.string(s),
                            Value::Number(n) if n.is_i64() => integer(rt, n.as_i64().unwrap_or(0) as i32),
                            Value::Bool(b) => boolean(rt, *b),
                            other => component_from_json(rt, other),
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                let with = if args.is_empty() { V::NULL } else { rt.alloc("[Ljava/lang/Object;", Data::Array(args)) };
                component(rt, key, true, with)
            }
            None => {
                let text = o.get("text").and_then(Value::as_str).unwrap_or("").to_owned();
                let t = rt.string(&text);
                component(rt, t, false, V::NULL)
            }
        },
        Value::Array(a) => match a.first() {
            Some(first) => component_from_json(rt, first),
            None => {
                let t = rt.string("");
                component(rt, t, false, V::NULL)
            }
        },
        Value::String(s) => {
            let t = rt.string(s);
            component(rt, t, false, V::NULL)
        }
        other => {
            let t = rt.string(&other.to_string());
            component(rt, t, false, V::NULL)
        }
    }
}

const LOCAL_PLAYER: &str = "net/minecraft/client/player/LocalPlayer";
const FOOD_DATA: &str = "net/minecraft/world/food/FoodData";

/// Whether `owner` is the player or one of the classes a player's methods are called through (the
/// client's own: `LocalPlayer`, `AbstractClientPlayer`).
pub(crate) fn is_player(owner: &str) -> bool {
    owner.starts_with("net/minecraft/")
        && [ "/Player", "/ServerPlayer", "/LocalPlayer", "/AbstractClientPlayer", "/LivingEntity", "/Entity"].iter().any(|s| owner.ends_with(s))
}

/// A player object's UUID and name: a server's player's own (set when the game told of them),
/// else the acting player's UUID and the name "Player".
fn player_identity(rt: &mut Rt, p: V) -> (V, V) {
    let uuid = match rt.get_field(p, "$uuid") {
        u if u.r() != 0 => u,
        _ => {
            let id = rt.host().and_then(|h| h.player_id()).unwrap_or(0);
            let u = crate::tags::uuid(rt, (id >> 64) as i64, id as i64);
            if p.r() != 0 {
                rt.put_field(p, "$uuid", u);
            }
            u
        }
    };
    let name = match rt.get_field(p, "$name") {
        n if n.r() != 0 => n,
        _ => rt.string("Player"),
    };
    (uuid, name)
}

const CHUNK_POS: &str = "net/minecraft/world/level/ChunkPos";
const SECTION_POS: &str = "net/minecraft/core/SectionPos";
const WORLD_VERSION: &str = "net/minecraft/WorldVersion";
const ADVANCEMENT_BUILDER: &str = "net/minecraft/advancements/Advancement$Builder";
const ITEM_PREDICATE_BUILDER: &str = "net/minecraft/advancements/critereon/ItemPredicate$Builder";

/// `ChunkPos.asLong(x, z)`: x in the low 32 bits, z in the high.
fn chunk_long(x: i32, z: i32) -> i64 {
    (i64::from(x) & 0xffff_ffff) | ((i64::from(z) & 0xffff_ffff) << 32)
}

/// `ChunkPos.hash(x, z)`, its `hashCode`.
fn chunk_hash(x: i32, z: i32) -> i32 {
    let i = 1_664_525i32.wrapping_mul(x).wrapping_add(1_013_904_223);
    let j = 1_664_525i32.wrapping_mul(z ^ -559_038_737).wrapping_add(1_013_904_223);
    i ^ j
}

/// `SectionPos.asLong(x, y, z)`: x in 22 bits from bit 42, z in 22 from bit 20, y in the low 20.
fn section_long(s: [i32; 3]) -> i64 {
    ((i64::from(s[0]) & 0x3f_ffff) << 42) | (i64::from(s[1]) & 0xf_ffff) | ((i64::from(s[2]) & 0x3f_ffff) << 20)
}

/// The section a packed section position holds, as `SectionPos.x(l)`, `y(l)` and `z(l)` unpack it.
fn section_of_long(l: i64) -> [i32; 3] {
    [(l >> 42) as i32, ((l << 44) >> 44) as i32, ((l << 22) >> 42) as i32]
}

/// A new `SectionPos` of section `s`: a `Vec3i` as a `BlockPos` is, which `Vec3i`'s methods read.
fn section_pos(rt: &mut Rt, s: [i32; 3]) -> V {
    rt.alloc(SECTION_POS, Data::Native("BlockPos", [i64::from(s[0]), i64::from(s[1]), i64::from(s[2]), 0]))
}

/// A new `ChunkPos` of chunk `x`, `z`.
pub(crate) fn chunk_pos(rt: &mut Rt, x: i32, z: i32) -> V {
    let c = rt.new_object(CHUNK_POS);
    rt.put_field(c, "x", V::I(x));
    rt.put_field(c, "z", V::I(z));
    c
}

/// Gives the acting player `stack` (where it fits, as `Inventory.add` places it), shrinking it by
/// what went in; whether all of it did.
pub(crate) fn give_player(rt: &mut Rt, stack: V) -> bool {
    if stack.r() == 0 || stack_empty(rt, stack) {
        return true;
    }
    let item = item_like_name(rt, rt.get_field(stack, "$item"));
    let count = rt.get_field(stack, "$count").i().max(0) as u32;
    let gave = rt.host_call(|h| h.give_player_items(&item, count)).unwrap_or(0);
    rt.put_field(stack, "$count", V::I((count - gave.min(count)) as i32));
    gave >= count
}

/// `InteractionResult`'s constants and `ItemInteractionResult`'s, in their declaration order.
const RESULTS: [&str; 6] = [
    "SUCCESS",
    "SUCCESS_NO_ITEM_USED",
    "CONSUME",
    "CONSUME_PARTIAL",
    "PASS",
    "FAIL",
];
const ITEM_RESULTS: [&str; 6] = [
    "SUCCESS",
    "CONSUME",
    "CONSUME_PARTIAL",
    "PASS_TO_DEFAULT_BLOCK_INTERACTION",
    "SKIP_DEFAULT_BLOCK_INTERACTION",
    "FAIL",
];

fn result_names(class: &str) -> &'static [&'static str] {
    match class {
        "net/minecraft/world/InteractionResult" => &RESULTS,
        "net/minecraft/world/ItemInteractionResult" => &ITEM_RESULTS,
        _ => &[],
    }
}

/// An `InteractionResult` / `ItemInteractionResult` constant, by name (interned: compared by
/// identity), with its ordinal and name (a mod's `switch` on one reads the ordinal).
pub fn result(rt: &mut Rt, class: &str, name: &str) -> V {
    let (class, name) = (class.to_owned(), name.to_owned());
    rt.intern(format!("result/{class}/{name}"), move |rt| {
        let c: &'static str = rt.static_key(&class);
        let ordinal = result_names(c).iter().position(|n| *n == name).unwrap_or(0) as i32;
        let label = rt.string(&name);
        let o = rt.alloc(c, Data::Str(name));
        rt.put_field(o, "$ordinal", V::I(ordinal));
        rt.put_field(o, "$name", label);
        o
    })
}

/// A `BlockHitResult` for a click on `pos`'s face `face` (a direction ordinal).
pub fn hit(rt: &mut Rt, pos: [i32; 3], face: i64) -> V {
    let h = rt.new_object("net/minecraft/world/phys/BlockHitResult");
    let p = crate::api::block_pos(rt, pos[0] as i64, pos[1] as i64, pos[2] as i64);
    let d = direction(rt, face);
    let l = vec3(rt, pos[0] as f64 + 0.5, pos[1] as f64 + 0.5, pos[2] as f64 + 0.5);
    rt.put_field(h, "$pos", p);
    rt.put_field(h, "$dir", d);
    rt.put_field(h, "$loc", l);
    h
}

const PLACE_CONTEXT: &str = "net/minecraft/world/item/context/BlockPlaceContext";
const USE_ON_CONTEXT: &str = "net/minecraft/world/item/context/UseOnContext";
/// `Direction.getNormal()` by ordinal (down, up, north, south, west, east).
const DIRECTION_STEPS: [[i64; 3]; 6] = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];

/// `Direction.getRotation()` by ordinal: vanilla's quaternions (x, y, z, w), as JOML's
/// `rotationX` and `rotationXYZ` compute them (`Math.sin` of the half angle, the cosine from it).
fn direction_rotation(direction: usize) -> [f32; 4] {
    use std::f32::consts::{FRAC_PI_2, PI};
    // JOML's `Math.sin` and `Math.cosFromSin` without fast math.
    let sin_cos = |angle: f32| {
        let sin = (f64::from(angle)).sin() as f32;
        let cos = (1.0 - sin * sin).sqrt();
        let a = angle + FRAC_PI_2;
        let mut b = a - ((a / (2.0 * PI)) as i32) as f32 * (2.0 * PI);
        if b < 0.0 {
            b += 2.0 * PI;
        }
        (sin, if b >= PI { -cos } else { cos })
    };
    let rotation_x = |angle: f32| {
        let (s, c) = sin_cos(angle * 0.5);
        [s, 0.0, 0.0, c]
    };
    let rotation_xyz = |x: f32, y: f32, z: f32| {
        let ((sx, cx), (sy, cy), (sz, cz)) = (sin_cos(x * 0.5), sin_cos(y * 0.5), sin_cos(z * 0.5));
        let (cycz, sysz, sycz, cysz) = (cy * cz, sy * sz, sy * cz, cy * sz);
        [
            sx * cycz + cx * sysz,
            cx * sycz - sx * cysz,
            cx * cysz + sx * sycz,
            cx * cycz - sx * sysz,
        ]
    };
    match direction {
        0 => rotation_x(PI),
        1 => [0.0, 0.0, 0.0, 1.0],
        2 => rotation_xyz(FRAC_PI_2, 0.0, PI),
        3 => rotation_x(FRAC_PI_2),
        4 => rotation_xyz(FRAC_PI_2, 0.0, FRAC_PI_2),
        _ => rotation_xyz(FRAC_PI_2, 0.0, -FRAC_PI_2),
    }
}

/// Java's `Math.min` on floating point: NaN if either is, -0.0 below 0.0.
fn java_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0.0 && b == 0.0 {
        if b.is_sign_negative() { b } else { a }
    } else if a <= b {
        a
    } else {
        b
    }
}

/// Java's `Math.max` on floating point: NaN if either is, 0.0 above -0.0.
fn java_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_negative() { b } else { a }
    } else if a >= b {
        a
    } else {
        b
    }
}

fn mth_floor_f(x: f32) -> i32 {
    f64::from(x).floor() as i32
}

fn mth_wrap_f(v: f32) -> f32 {
    let mut f = v % 360.0;
    if f >= 180.0 {
        f -= 360.0;
    }
    if f < -180.0 {
        f += 360.0;
    }
    f
}

fn mth_wrap_d(v: f64) -> f64 {
    let mut f = v % 360.0;
    if f >= 180.0 {
        f -= 360.0;
    }
    if f < -180.0 {
        f += 360.0;
    }
    f
}

fn mth_is_pow2(v: i32) -> bool {
    v != 0 && v & v.wrapping_sub(1) == 0
}

fn mth_smallest_pow2(v: i32) -> i32 {
    let mut i = v.wrapping_sub(1);
    i |= i >> 1;
    i |= i >> 2;
    i |= i >> 4;
    i |= i >> 8;
    i |= i >> 16;
    i.wrapping_add(1)
}

fn mth_ceillog2(v: i32) -> i32 {
    const DE_BRUIJN: [i32; 32] = [0, 1, 28, 2, 29, 14, 24, 3, 30, 22, 20, 15, 25, 17, 4, 8, 31, 27, 13, 23, 21, 19, 16, 7, 26, 12, 18, 6, 11, 5, 10, 9];
    let v = if mth_is_pow2(v) { v } else { mth_smallest_pow2(v) };
    DE_BRUIJN[(((i64::from(v) * 125_613_361) >> 27) as i32 & 31) as usize]
}

/// 1.21.1's `FastColor.ARGB32`: a colour's channels, and colours made of them.
fn argb32(name: &str, desc: &str, args: &[V]) -> Option<i32> {
    let i = |k: usize| args.get(k).copied().unwrap_or_default().i();
    let (alpha, red, green, blue) = (|c: i32| ((c as u32) >> 24) as i32, |c: i32| (c >> 16) & 0xFF, |c: i32| (c >> 8) & 0xFF, |c: i32| c & 0xFF);
    let color = |a: i32, r: i32, g: i32, b: i32| (a << 24) | (r << 16) | (g << 8) | b;
    // `Mth.lerpInt`: `start + floor(delta * (end - start))`.
    let lerp = |t: f32, x: i32, y: i32| x + (t * (y - x) as f32).floor() as i32;
    let chan = |f: f32| (f * 255.0).floor() as i32;
    Some(match (name, desc) {
        ("alpha", "(I)I") => alpha(i(0)),
        ("red", "(I)I") => red(i(0)),
        ("green", "(I)I") => green(i(0)),
        ("blue", "(I)I") => blue(i(0)),
        ("color", "(IIII)I") => color(i(0), i(1), i(2), i(3)),
        ("color", "(III)I") => color(255, i(0), i(1), i(2)),
        ("color", "(II)I") => (i(0) << 24) | (i(1) & 0xFF_FFFF),
        ("opaque", "(I)I") => i(0) | (0xFF00_0000_u32 as i32),
        ("multiply", "(II)I") => {
            let (x, y) = (i(0), i(1));
            color(alpha(x) * alpha(y) / 255, red(x) * red(y) / 255, green(x) * green(y) / 255, blue(x) * blue(y) / 255)
        }
        ("lerp", "(FII)I") => {
            let (t, x, y) = (args.first().copied().unwrap_or_default().f(), i(1), i(2));
            color(lerp(t, alpha(x), alpha(y)), lerp(t, red(x), red(y)), lerp(t, green(x), green(y)), lerp(t, blue(x), blue(y)))
        }
        ("colorFromFloat", "(FFFF)I") => {
            let f = |k: usize| args.get(k).copied().unwrap_or_default().f();
            color(chan(f(0)), chan(f(1)), chan(f(2)), chan(f(3)))
        }
        _ => return None,
    })
}

/// `Math.floorDiv`; None for a zero divisor (Java's ArithmeticException).
fn floor_div(x: i64, y: i64) -> Option<i64> {
    if y == 0 {
        return None;
    }
    let q = x.wrapping_div(y);
    Some(if (x ^ y) < 0 && q.wrapping_mul(y) != x { q - 1 } else { q })
}

/// `Mth.fastInvSqrt`: the bit trick and one Newton step, as vanilla does it.
fn mth_fast_inv_sqrt(x: f64) -> f64 {
    let half = 0.5 * x;
    let i = 6_910_469_410_427_058_090_i64.wrapping_sub((x.to_bits() as i64) >> 1);
    let y = f64::from_bits(i as u64);
    y * (1.5 - half * y * y)
}

/// `Mth.atan2`: vanilla's table approximation (its `ASIN_TAB`/`COS_TAB` of 257 entries).
pub(crate) fn mth_atan2(mut y: f64, mut x: f64) -> f64 {
    use std::sync::OnceLock;
    static TABLES: OnceLock<(Vec<f64>, Vec<f64>)> = OnceLock::new();
    let (asin_tab, cos_tab) = TABLES.get_or_init(|| {
        let (mut asin, mut cos) = (vec![0.0; 257], vec![0.0; 257]);
        for i in 0..257 {
            let v = (f64::from(i) / 256.0).asin();
            cos[i as usize] = v.cos();
            asin[i as usize] = v;
        }
        (asin, cos)
    });
    let frac_bias = f64::from_bits(4_805_340_802_404_319_232);
    let d = x * x + y * y;
    if d.is_nan() {
        return f64::NAN;
    }
    let neg_y = y < 0.0;
    if neg_y {
        y = -y;
    }
    let neg_x = x < 0.0;
    if neg_x {
        x = -x;
    }
    let steep = y > x;
    if steep {
        std::mem::swap(&mut x, &mut y);
    }
    let inv = mth_fast_inv_sqrt(d);
    x *= inv;
    y *= inv;
    let biased = frac_bias + y;
    let i = biased.to_bits() as i32;
    let at = |t: &Vec<f64>| t.get(i as usize).copied().unwrap_or(0.0);
    let (asin, cos) = (at(asin_tab), at(cos_tab));
    let delta = biased - frac_bias;
    let e = y * cos - x * delta;
    let t = (6.0 + e * e) * e * 0.16666666666666666;
    let mut angle = asin + t;
    if steep {
        angle = std::f64::consts::FRAC_PI_2 - angle;
    }
    if neg_x {
        angle = std::f64::consts::PI - angle;
    }
    if neg_y {
        angle = -angle;
    }
    angle
}

/// Vanilla's `Mth` (26.3), ported from its bytecode: the pure functions mods call (Create's
/// depot renderer takes `log2` of a stack's count). None for what isn't here (the arms in
/// [`call`] answer `sin`, `cos`, `floor`, `clamp`, `lerp`...).
fn mth_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |k: usize| args.get(k).copied().unwrap_or_default();
    let (i, f, d, j) = (|k: usize| a(k).i(), |k: usize| a(k).f(), |k: usize| a(k).d(), |k: usize| a(k).j());
    let arithmetic = |rt: &mut Rt| {
        rt.throw_new("java/lang/ArithmeticException");
        V::I(0)
    };
    let clamp_f = |v: f32, lo: f32, hi: f32| if v < lo { lo } else { java_min(f64::from(v), f64::from(hi)) as f32 };
    let approach = |from: f32, to: f32, step: f32| {
        let step = step.abs();
        if from < to { clamp_f(from + step, from, to) } else { clamp_f(from - step, to, from) }
    };
    let lerp_f = |t: f32, a: f32, b: f32| a + t * (b - a);
    let lerp_d = |t: f64, a: f64, b: f64| a + t * (b - a);
    let draw = |rt: &mut Rt, key: &'static str, bound: Option<i32>| {
        let mut call = vec![a(0)];
        call.extend(bound.map(V::I));
        crate::api::call(rt, key, &call)
    };
    const NEXT_INT: &str = "net/minecraft/util/RandomSource.nextInt.(I)I";
    const NEXT_FLOAT: &str = "net/minecraft/util/RandomSource.nextFloat.()F";
    const NEXT_DOUBLE: &str = "net/minecraft/util/RandomSource.nextDouble.()D";
    Some(match (name, desc) {
        // Draws in a range from a `RandomSource`, as `Mth` composes them.
        ("randomBetween", "(Lnet/minecraft/util/RandomSource;FF)F") => V::F(draw(rt, NEXT_FLOAT, None).f() * (f(2) - f(1)) + f(1)),
        ("randomBetweenInclusive", "(Lnet/minecraft/util/RandomSource;II)I") => {
            V::I(draw(rt, NEXT_INT, Some(i(2).wrapping_sub(i(1)).wrapping_add(1))).i().wrapping_add(i(1)))
        }
        ("nextInt", "(Lnet/minecraft/util/RandomSource;II)I") => {
            V::I(if i(1) >= i(2) { i(1) } else { draw(rt, NEXT_INT, Some(i(2).wrapping_sub(i(1)).wrapping_add(1))).i().wrapping_add(i(1)) })
        }
        ("nextFloat", "(Lnet/minecraft/util/RandomSource;FF)F") => V::F(if f(1) >= f(2) { f(1) } else { draw(rt, NEXT_FLOAT, None).f() * (f(2) - f(1)) + f(1) }),
        ("nextDouble", "(Lnet/minecraft/util/RandomSource;DD)D") => V::D(if d(1) >= d(2) { d(1) } else { draw(rt, NEXT_DOUBLE, None).d() * (d(2) - d(1)) + d(1) }),
        // `FastColor.ARGB32.color(0, floor(r * 255), floor(g * 255), floor(b * 255))`.
        ("color", "(FFF)I") => V::I((mth_floor_f(f(0) * 255.0) << 16) | (mth_floor_f(f(1) * 255.0) << 8) | mth_floor_f(f(2) * 255.0)),
        ("isPowerOfTwo", "(I)Z") => V::I(mth_is_pow2(i(0)) as i32),
        ("isPowerOfTwo", "(J)Z") => V::I((j(0) != 0 && j(0) & j(0).wrapping_sub(1) == 0) as i32),
        ("smallestEncompassingPowerOfTwo", "(I)I") => V::I(mth_smallest_pow2(i(0))),
        ("ceillog2", "(I)I") => V::I(mth_ceillog2(i(0))),
        ("log2", "(I)I") => V::I(mth_ceillog2(i(0)) - if mth_is_pow2(i(0)) { 0 } else { 1 }),
        ("frac", "(F)F") => V::F(f(0) - mth_floor_f(f(0)) as f32),
        ("frac", "(D)D") => V::D(d(0) - d(0).floor() as i64 as f64),
        ("lfloor", "(D)J") => V::J(d(0).floor() as i64),
        ("ceilLong", "(D)J") => V::J(d(0).ceil() as i64),
        ("floorDiv", "(II)I") => match floor_div(i64::from(i(0)), i64::from(i(1))) {
            Some(q) => V::I(q as i32),
            None => arithmetic(rt),
        },
        ("positiveCeilDiv", "(II)I") => match floor_div(i64::from(i(0).wrapping_neg()), i64::from(i(1))) {
            Some(q) => V::I((q as i32).wrapping_neg()),
            None => arithmetic(rt),
        },
        ("positiveCeilDiv", "(JJ)J") => match floor_div(j(0).wrapping_neg(), j(1)) {
            Some(q) => V::J(q.wrapping_neg()),
            None => {
                arithmetic(rt);
                V::J(0)
            }
        },
        ("roundToward", "(II)I") => match floor_div(i64::from(i(0).wrapping_neg()), i64::from(i(1))) {
            Some(q) => V::I((q as i32).wrapping_neg().wrapping_mul(i(1))),
            None => arithmetic(rt),
        },
        ("roundToward", "(JJ)J") => match floor_div(j(0).wrapping_neg(), j(1)) {
            Some(q) => V::J(q.wrapping_neg().wrapping_mul(j(1))),
            None => {
                arithmetic(rt);
                V::J(0)
            }
        },
        ("isMultipleOf", "(II)Z") => match i(1) {
            0 => arithmetic(rt),
            m => V::I((i(0).wrapping_rem(m) == 0) as i32),
        },
        ("quantize", "(DI)I") => V::I(((d(0) / f64::from(i(1))).floor() as i32).wrapping_mul(i(1))),
        ("wrapDegrees", "(I)I") => {
            let mut w = i(0) % 360;
            if w >= 180 {
                w -= 360;
            }
            if w < -180 {
                w += 360;
            }
            V::I(w)
        }
        ("wrapDegrees", "(J)F") => {
            let mut w = (j(0) % 360) as f32;
            if w >= 180.0 {
                w -= 360.0;
            }
            if w < -180.0 {
                w += 360.0;
            }
            V::F(w)
        }
        ("wrapDegrees", "(F)F") => V::F(mth_wrap_f(f(0))),
        ("wrapDegrees", "(D)D") => V::D(mth_wrap_d(d(0))),
        ("wrapDegrees90", "(F)F") => {
            let mut w = f(0) % 90.0;
            if w >= 45.0 {
                w -= 90.0;
            }
            if w < -45.0 {
                w += 90.0;
            }
            V::F(w)
        }
        ("degreesDifference", "(FF)F") => V::F(mth_wrap_f(f(1) - f(0))),
        ("degreesDifferenceAbs", "(FF)F") => V::F(mth_wrap_f(f(1) - f(0)).abs()),
        ("rotateIfNecessary", "(FFF)F") => {
            let diff = mth_wrap_f(f(1) - f(0));
            V::F(f(1) - clamp_f(diff, -f(2), f(2)))
        }
        ("approach", "(FFF)F") => V::F(approach(f(0), f(1), f(2))),
        ("approachDegrees", "(FFF)F") => {
            let diff = mth_wrap_f(f(1) - f(0));
            V::F(approach(f(0), f(0) + diff, f(2)))
        }
        ("rotLerp", "(FFF)F") => V::F(f(1) + f(0) * mth_wrap_f(f(2) - f(1))),
        ("rotLerp", "(DDD)D") => V::D(d(1) + d(0) * mth_wrap_d(d(2) - d(1))),
        ("rotLerpRad", "(FFF)F") => {
            let mut diff = f(2) - f(1);
            while diff < -std::f32::consts::PI {
                diff += std::f32::consts::TAU;
            }
            while diff >= std::f32::consts::PI {
                diff -= std::f32::consts::TAU;
            }
            V::F(f(1) + f(0) * diff)
        }
        ("packDegrees", "(F)B") => V::I(i32::from(mth_floor_f(f(0) * 256.0 / 360.0) as i8)),
        ("unpackDegrees", "(B)F") => V::F(i(0).wrapping_mul(360) as f32 / 256.0),
        ("square", "(F)F") => V::F(f(0) * f(0)),
        ("square", "(D)D") => V::D(d(0) * d(0)),
        ("square", "(I)I") => V::I(i(0).wrapping_mul(i(0))),
        ("square", "(J)J") => V::J(j(0).wrapping_mul(j(0))),
        ("cube", "(F)F") => V::F(f(0) * f(0) * f(0)),
        ("lerpInt", "(FII)I") => V::I(i(1).wrapping_add(mth_floor_f(f(0) * i(2).wrapping_sub(i(1)) as f32))),
        ("lerpDiscrete", "(FII)I") => {
            let delta = i(2).wrapping_sub(i(1));
            V::I(i(1).wrapping_add(mth_floor_f(f(0) * delta.wrapping_sub(1) as f32)).wrapping_add((f(0) > 0.0) as i32))
        }
        ("inverseLerp", "(FFF)F") => V::F((f(0) - f(1)) / (f(2) - f(1))),
        ("inverseLerp", "(DDD)D") => V::D((d(0) - d(1)) / (d(2) - d(1))),
        ("clampedLerp", "(FFF)F") => V::F(if f(0) < 0.0 { f(1) } else if f(0) > 1.0 { f(2) } else { lerp_f(f(0), f(1), f(2)) }),
        ("clampedLerp", "(DDD)D") => V::D(if d(0) < 0.0 { d(1) } else if d(0) > 1.0 { d(2) } else { lerp_d(d(0), d(1), d(2)) }),
        ("map", "(FFFFF)F") => V::F(lerp_f((f(0) - f(1)) / (f(2) - f(1)), f(3), f(4))),
        ("map", "(DDDDD)D") => V::D(lerp_d((d(0) - d(1)) / (d(2) - d(1)), d(3), d(4))),
        ("clampedMap", "(FFFFF)F") => {
            let t = (f(0) - f(1)) / (f(2) - f(1));
            V::F(if t < 0.0 { f(3) } else if t > 1.0 { f(4) } else { lerp_f(t, f(3), f(4)) })
        }
        ("clampedMap", "(DDDDD)D") => {
            let t = (d(0) - d(1)) / (d(2) - d(1));
            V::D(if t < 0.0 { d(3) } else if t > 1.0 { d(4) } else { lerp_d(t, d(3), d(4)) })
        }
        ("smoothstep", "(F)F") => V::F(f(0) * f(0) * f(0) * (f(0) * (f(0) * 6.0 - 15.0) + 10.0)),
        ("smoothstepDerivative", "(F)F") => V::F(30.0 * f(0) * f(0) * (f(0) - 1.0) * (f(0) - 1.0)),
        ("triangleWave", "(FF)F") => V::F((((f(0) % f(1)) - f(1) * 0.5).abs() - f(1) * 0.25) / (f(1) * 0.25)),
        ("catmullrom", "(FFFFF)F") => {
            let (t, p0, p1, p2, p3) = (f(0), f(1), f(2), f(3), f(4));
            V::F(0.5 * (2.0 * p1 + (p2 - p0) * t + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t * t * t))
        }
        ("murmurHash3Mixer", "(I)I") => {
            let mut x = i(0);
            x ^= ((x as u32) >> 16) as i32;
            x = x.wrapping_mul(-2_048_144_789);
            x ^= ((x as u32) >> 13) as i32;
            x = x.wrapping_mul(-1_028_477_387);
            x ^= ((x as u32) >> 16) as i32;
            V::I(x)
        }
        ("getSeed", "(III)J") => {
            let mut seed = i64::from(i(0).wrapping_mul(3_129_871)) ^ i64::from(i(2)).wrapping_mul(116_129_781) ^ i64::from(i(1));
            seed = seed.wrapping_mul(seed).wrapping_mul(42_317_861).wrapping_add(seed.wrapping_mul(11));
            V::J(seed >> 16)
        }
        ("absMax", "(II)I") => V::I(i(0).wrapping_abs().max(i(1).wrapping_abs())),
        ("absMax", "(FF)F") => V::F(java_max(f64::from(f(0).abs()), f64::from(f(1).abs())) as f32),
        ("absMax", "(DD)D") => V::D(java_max(d(0).abs(), d(1).abs())),
        ("chessboardDistance", "(IIII)I") => V::I(i(2).wrapping_sub(i(0)).wrapping_abs().max(i(3).wrapping_sub(i(1)).wrapping_abs())),
        ("lengthSquared", "(DD)D") => V::D(d(0) * d(0) + d(1) * d(1)),
        ("lengthSquared", "(FF)F") => V::F(f(0) * f(0) + f(1) * f(1)),
        ("lengthSquared", "(DDD)D") => V::D(d(0) * d(0) + d(1) * d(1) + d(2) * d(2)),
        ("lengthSquared", "(FFF)F") => V::F(f(0) * f(0) + f(1) * f(1) + f(2) * f(2)),
        ("length", "(DD)D") => V::D((d(0) * d(0) + d(1) * d(1)).sqrt()),
        ("length", "(FF)F") => V::F((f(0) * f(0) + f(1) * f(1)).sqrt()),
        ("length", "(DDD)D") => V::D((d(0) * d(0) + d(1) * d(1) + d(2) * d(2)).sqrt()),
        ("length", "(FFF)F") => V::F((f(0) * f(0) + f(1) * f(1) + f(2) * f(2)).sqrt()),
        // joml's `Math.invsqrt`: 1 / sqrt.
        ("invSqrt", "(F)F") => V::F(1.0 / f(0).sqrt()),
        ("invSqrt", "(D)D") => V::D(1.0 / d(0).sqrt()),
        ("fastInvSqrt", "(D)D") => V::D(mth_fast_inv_sqrt(d(0))),
        ("atan2", "(DD)D") => V::D(mth_atan2(d(0), d(1))),
        ("hsvToRgb", "(FFF)I") | ("hsvToArgb", "(FFFI)I") => {
            let alpha = if args.len() > 3 { i(3) } else { 0 };
            match hsv_to_argb(f(0), f(1), f(2), alpha) {
                Some(c) => V::I(c),
                None => {
                    rt.throw_new("java/lang/RuntimeException");
                    V::I(0)
                }
            }
        }
        _ => return None,
    })
}

/// `Mth.hsvToArgb(hue, saturation, value, alpha)` (`hsvToRgb`: alpha 0); None where vanilla throws
/// (a negative hue's sector).
pub(crate) fn hsv_to_argb(h: f32, sat: f32, val: f32, alpha: i32) -> Option<i32> {
    let sector = ((h * 6.0) as i32) % 6;
    let frac = h * 6.0 - sector as f32;
    let p = val * (1.0 - sat);
    let q = val * (1.0 - frac * sat);
    let t = val * (1.0 - (1.0 - frac) * sat);
    let (r, g, b) = match sector {
        0 => (val, t, p),
        1 => (q, val, p),
        2 => (p, val, t),
        3 => (p, q, val),
        4 => (t, p, val),
        5 => (val, p, q),
        _ => return None,
    };
    let c = |x: f32| ((x * 255.0) as i32).clamp(0, 255);
    Some((alpha & 0xFF) << 24 | (c(r) & 0xFF) << 16 | (c(g) & 0xFF) << 8 | (c(b) & 0xFF))
}

/// `Entity.calculateViewVector(xRot, yRot)` of a look (`yaw`, `pitch` in degrees), with `Mth`'s table:
/// the unit vector it points along.
pub(crate) fn view_vector(rt: &mut Rt, yaw: f32, pitch: f32) -> V {
    let (x_rot, y_rot) = (V::F(pitch * (std::f32::consts::PI / 180.0)), V::F(-yaw * (std::f32::consts::PI / 180.0)));
    let (c1, s1, c, s) = (mth_cos(y_rot), mth_sin(y_rot), mth_cos(x_rot), mth_sin(x_rot));
    vec3(rt, f64::from(s1 * c), f64::from(-s), f64::from(c1 * c))
}

/// Vanilla's `Mth.sin`: a 65536-entry table of `Math.sin`, so angles round as the game rounds them.
/// A float angle indexes it as 1.21.1's `sin(float)` (`(int)(a * 10430.378F) & 65535`, in float);
/// a double one as the later `sin(double)`.
pub(crate) fn mth_sin(v: V) -> f32 {
    sin_table(match v {
        V::D(d) => (d * 10430.378350470453) as i64 & 65535,
        other => ((other.f() * 10430.378f32) as i32 & 65535) as i64,
    })
}

pub(crate) fn mth_cos(v: V) -> f32 {
    sin_table(match v {
        V::D(d) => (d * 10430.378350470453 + 16384.0) as i64 & 65535,
        other => ((other.f() * 10430.378f32 + 16384.0f32) as i32 & 65535) as i64,
    })
}

/// `Mth.SIN[i]`: `(float) Math.sin(i * PI * 2 / 65536)`.
fn sin_table(i: i64) -> f32 {
    (i as f64 * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32
}

/// `Direction.fromYRot`: the horizontal direction a yaw faces, as an ordinal.
pub(crate) fn horizontal_facing(yaw: f32) -> i64 {
    // 2D data values 0..3 are south, west, north, east.
    [3, 4, 2, 5][((yaw as f64 / 90.0 + 0.5).floor() as i64 & 3) as usize]
}

/// `Direction.orderedByNearest(entity)` for a look of `yaw`, `pitch` (degrees), as ordinals.
pub(crate) fn ordered_by_nearest(yaw: f32, pitch: f32) -> [i64; 6] {
    let x_rot = pitch * (std::f32::consts::PI / 180.0);
    let y_rot = -yaw * (std::f32::consts::PI / 180.0);
    let (ps, pc) = (mth_sin(V::F(x_rot)), mth_cos(V::F(x_rot)));
    let (ys, yc) = (mth_sin(V::F(y_rot)), mth_cos(V::F(y_rot)));
    let (east, up, south) = (ys > 0.0, ps < 0.0, yc > 0.0);
    let x = if east { ys } else { -ys };
    let y = if up { -ps } else { ps };
    let z = if south { yc } else { -yc };
    let (xm, zm) = (x * pc, z * pc);
    let dx = if east { 5 } else { 4 };
    let dy = if up { 1 } else { 0 };
    let dz = if south { 3 } else { 2 };
    let opposite = |d: i64| d ^ 1;
    let three = |a: i64, b: i64, c: i64| [a, b, c, opposite(c), opposite(b), opposite(a)];
    if x > z {
        if y > xm {
            three(dy, dx, dz)
        } else if zm > y {
            three(dx, dz, dy)
        } else {
            three(dx, dy, dz)
        }
    } else if y > zm {
        three(dy, dz, dx)
    } else if xm > y {
        three(dz, dx, dy)
    } else {
        three(dz, dy, dx)
    }
}

fn directions(rt: &mut Rt, order: &[i64]) -> V {
    let arr = rt.new_array(order.len() as i32, V::NULL);
    for (i, d) in order.iter().enumerate() {
        let v = direction(rt, *d);
        rt.array_set(arr, i as i32, v);
    }
    arr
}

/// Whether a block the game reports can be replaced by a placement (`canBeReplaced`), for contexts
/// mod code makes itself: air, fluids and the plants and snow a placement overwrites.
fn replaceable(state: &str) -> bool {
    let (name, props) = crate::host::split_state(state);
    matches!(
        name,
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air" | "minecraft:water" | "minecraft:lava"
            | "minecraft:short_grass" | "minecraft:tall_grass" | "minecraft:fern" | "minecraft:large_fern"
            | "minecraft:dead_bush" | "minecraft:vine" | "minecraft:glow_lichen" | "minecraft:seagrass"
            | "minecraft:tall_seagrass" | "minecraft:structure_void" | "minecraft:fire" | "minecraft:soul_fire"
            | "minecraft:light" | "minecraft:bubble_column" | "minecraft:hanging_roots" | "minecraft:crimson_roots"
            | "minecraft:warped_roots" | "minecraft:nether_sprouts"
    ) || (name == "minecraft:snow" && props.iter().any(|(k, v)| *k == "layers" && *v == "1"))
}

/// Fills a context's click from a `BlockHitResult`, as vanilla's constructor does: the block goes into
/// the clicked block when that can be replaced, else beside it.
fn place_context_from_hit(rt: &mut Rt, ctx: V, h: V) {
    let p = rt.get_field(h, "$pos");
    let d = rt.get_field(h, "$dir");
    let pos = native(rt, p);
    let face = native(rt, d)[0].clamp(0, 5);
    let clicked = rt.host().map(|host| host.block_state([pos[0] as i32, pos[1] as i32, pos[2] as i32])).unwrap_or_default();
    let replace = replaceable(&clicked);
    let s = DIRECTION_STEPS[face as usize];
    let place = if replace { p } else { crate::api::block_pos(rt, pos[0] + s[0], pos[1] + s[1], pos[2] + s[2]) };
    let loc = rt.get_field(h, "$loc");
    let inside = rt.get_field(h, "$inside");
    rt.put_field(ctx, "$hitResult", h);
    rt.put_field(ctx, "$hitPos", p);
    rt.put_field(ctx, "$pos", place);
    rt.put_field(ctx, "$face", d);
    rt.put_field(ctx, "$hit", loc);
    rt.put_field(ctx, "$inside", V::I(inside.i()));
    rt.put_field(ctx, "$replace", V::I(replace as i32));
    let player = rt.get_field(ctx, "$player");
    let yaw = rt.get_field(player, "$yaw");
    let pitch = rt.get_field(player, "$pitch");
    rt.put_field(ctx, "$yaw", V::F(yaw.f()));
    rt.put_field(ctx, "$pitch", V::F(pitch.f()));
}

/// The context a placement hands `getStateForPlacement`, from the game's.
pub fn place_context(rt: &mut Rt, p: &crate::glue::Placement) -> V {
    context(rt, p, PLACE_CONTEXT)
}

/// The context `Item.useOn` gets for a right click on a block: its `getClickedPos` is the block
/// clicked, not the one next to it.
pub fn use_on_context(rt: &mut Rt, p: &crate::glue::Placement) -> V {
    let clicked = crate::glue::Placement { pos: p.hit_pos, replace_clicked: true, ..p.clone() };
    context(rt, &clicked, USE_ON_CONTEXT)
}

/// The player a server hook acts for (`ServerPlayer`), where the game says they stand and look
/// (`Host::player_place`), so their position, eyes and look answer.
pub(crate) fn acting_player(rt: &mut Rt) -> V {
    // A server's player is one object for as long as the runtime runs (the menu it has open stays on
    // it); a hook with no player to name gets one of its own.
    let player = match rt.host().and_then(|h| h.player_id()) {
        Some(uuid) if !crate::neo::is_client(rt) => crate::menus::server_player(rt, uuid),
        _ => {
            let p = rt.new_object(SERVER_PLAYER);
            crate::menus::init_player(rt, p);
            p
        }
    };
    if let Some(([x, y, z], yaw, pitch)) = rt.host().and_then(|h| h.player_place()) {
        rt.put_field(player, "$x", V::D(x));
        rt.put_field(player, "$y", V::D(y));
        rt.put_field(player, "$z", V::D(z));
        rt.put_field(player, "$yaw", V::F(yaw));
        rt.put_field(player, "$pitch", V::F(pitch));
    }
    player
}

const SERVER_PLAYER: &str = "net/minecraft/server/level/ServerPlayer";
const RESULT_HOLDER: &str = "net/minecraft/world/InteractionResultHolder";

/// An `InteractionResultHolder` of the result named `result` and `object`.
fn result_holder(rt: &mut Rt, result: &str, object: V) -> V {
    let h = rt.new_object(RESULT_HOLDER);
    let r = self::result(rt, "net/minecraft/world/InteractionResult", result);
    rt.put_field(h, "$result", r);
    rt.put_field(h, "$object", object);
    h
}

fn context(rt: &mut Rt, p: &crate::glue::Placement, class: &'static str) -> V {
    let ctx = rt.new_object(class);
    let level = crate::glue::level(rt);
    let player = acting_player(rt);
    rt.put_field(player, "$yaw", V::F(p.yaw));
    rt.put_field(player, "$pitch", V::F(p.pitch));
    let hand = crate::hand::constant(rt, p.hand);
    // The stack in hand, as the player's own (`getItemInHand`), when it holds the item.
    let item = if p.item.is_empty() {
        get_static(rt, "net/minecraft/world/item/ItemStack", "EMPTY").unwrap_or(V::NULL)
    } else if rt.host().and_then(|h| h.player_hand(p.hand)).is_some_and(|s| s.item == p.item) {
        crate::hand::stack(rt, p.hand)
    } else {
        stack(rt, &p.item, 1)
    };
    let pos = crate::api::block_pos(rt, p.pos[0] as i64, p.pos[1] as i64, p.pos[2] as i64);
    let hit_pos = crate::api::block_pos(rt, p.hit_pos[0] as i64, p.hit_pos[1] as i64, p.hit_pos[2] as i64);
    let face = direction(rt, p.face);
    let hit = vec3(rt, p.hit[0], p.hit[1], p.hit[2]);
    let h = rt.new_object("net/minecraft/world/phys/BlockHitResult");
    rt.put_field(h, "$loc", hit);
    rt.put_field(h, "$dir", face);
    rt.put_field(h, "$pos", hit_pos);
    rt.put_field(h, "$inside", V::I(p.inside as i32));
    for (f, v) in [
        ("$level", level),
        ("$player", player),
        ("$hand", hand),
        ("$item", item),
        ("$hitResult", h),
        ("$hitPos", hit_pos),
        ("$pos", pos),
        ("$face", face),
        ("$hit", hit),
        ("$inside", V::I(p.inside as i32)),
        ("$replace", V::I(p.replace_clicked as i32)),
        ("$yaw", V::F(p.yaw)),
        ("$pitch", V::F(p.pitch)),
    ] {
        rt.put_field(ctx, f, v);
    }
    ctx
}

/// `BlockPlaceContext`'s getters (and `UseOnContext`'s, which it extends).
fn place_context_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let c = args.first().copied().unwrap_or_default();
    let (yaw, pitch) = (rt.get_field(c, "$yaw").f(), rt.get_field(c, "$pitch").f());
    Some(match name {
        "getLevel" => rt.get_field(c, "$level"),
        "getClickedPos" => rt.get_field(c, "$pos"),
        "getPlayer" => rt.get_field(c, "$player"),
        "getClickedFace" => rt.get_field(c, "$face"),
        "getClickLocation" => rt.get_field(c, "$hit"),
        "getHitResult" => rt.get_field(c, "$hitResult"),
        "isInside" => V::I(rt.get_field(c, "$inside").i()),
        "getItemInHand" => rt.get_field(c, "$item"),
        "getHand" => rt.get_field(c, "$hand"),
        "isSecondaryUseActive" => V::I(rt.host().is_some_and(|h| h.player_sneaking()) as i32),
        "replacingClickedOnBlock" => V::I(rt.get_field(c, "$replace").i()),
        // Asked while the game places: the position takes the block.
        "canPlace" => V::I(1),
        "getRotation" => V::F(yaw),
        "getHorizontalDirection" => direction(rt, horizontal_facing(yaw)),
        "getNearestLookingDirection" => direction(rt, ordered_by_nearest(yaw, pitch)[0]),
        "getNearestLookingVerticalDirection" => direction(rt, if pitch < 0.0 { 1 } else { 0 }),
        "getNearestLookingDirections" => {
            let mut order = ordered_by_nearest(yaw, pitch).to_vec();
            if rt.get_field(c, "$replace").i() == 0 {
                // The face clicked against comes first: its opposite leads, the rest keep their order.
                let face = native(rt, rt.get_field(c, "$face"))[0].clamp(0, 5) ^ 1;
                if let Some(i) = order.iter().position(|d| *d == face) {
                    let d = order.remove(i);
                    order.insert(0, d);
                }
            }
            directions(rt, &order)
        }
        _ => return None,
    })
}

/// Static fields this module owns: `BlockStateProperties.*`, `ItemStack.EMPTY`, `Items.*`.
/// 26.3's block entity types; each one's id is its field's name, lower-cased.
const VANILLA_BLOCK_ENTITY_TYPES: [&str; 49] = [
    "FURNACE", "CHEST", "TRAPPED_CHEST", "ENDER_CHEST", "JUKEBOX", "DISPENSER", "DROPPER", "SIGN", "HANGING_SIGN", "MOB_SPAWNER",
    "CREAKING_HEART", "PISTON", "BREWING_STAND", "ENCHANTING_TABLE", "END_PORTAL", "BEACON", "SKULL", "DAYLIGHT_DETECTOR", "HOPPER",
    "COMPARATOR", "BANNER", "STRUCTURE_BLOCK", "END_GATEWAY", "COMMAND_BLOCK", "SHULKER_BOX", "CONDUIT", "BARREL", "SMOKER",
    "BLAST_FURNACE", "LECTERN", "BELL", "JIGSAW", "CAMPFIRE", "BEEHIVE", "SCULK_SENSOR", "CALIBRATED_SCULK_SENSOR", "SCULK_CATALYST",
    "SCULK_SHRIEKER", "CHISELED_BOOKSHELF", "SHELF", "BRUSHABLE_BLOCK", "DECORATED_POT", "CRAFTER", "TRIAL_SPAWNER", "VAULT",
    "TEST_BLOCK", "TEST_INSTANCE_BLOCK", "COPPER_GOLEM_STATUE", "POTENT_SULFUR",
];

/// Vanilla block entity type `field` (`LECTERN`): registered as `minecraft:lectern`, so the same
/// object every time and its id found from it.
pub(crate) fn vanilla_block_entity_type(rt: &mut Rt, field: &str) -> V {
    let id = format!("minecraft:{}", field.to_ascii_lowercase());
    if let Some(v) = rt.registered.get(&("block_entity_type".to_owned(), id.clone())) {
        return *v;
    }
    let v = rt.new_object("net/minecraft/world/level/block/entity/BlockEntityType");
    rt.registered.insert(("block_entity_type".to_owned(), id), v);
    v
}

pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    Some(match owner {
        "net/minecraft/world/item/ItemStackLinkedSet" if field == "TYPE_AND_TAG" => intern_strategy(rt, ITEM_STRATEGY),
        "net/neoforged/neoforge/fluids/FluidStackLinkedSet" if field == "TYPE_AND_COMPONENTS" => intern_strategy(rt, FLUID_STRATEGY),
        "net/minecraft/world/phys/Vec3" if field == "ZERO" => vec3_zero(rt),
        HOVER_ACTION => {
            let &(_, named) = HOVER_ACTIONS.iter().find(|(f, _)| *f == field)?;
            rt.intern(format!("compat:hover-action/{named}"), |rt| {
                let action = rt.new_object(HOVER_ACTION);
                let s = rt.string(named);
                rt.put_field(action, "$name", s);
                action
            })
        }
        BOUNDS_INTS | BOUNDS_DOUBLES if field == "ANY" => {
            let class: &'static str = if owner == BOUNDS_INTS { BOUNDS_INTS } else { BOUNDS_DOUBLES };
            rt.intern(format!("const/{class}.ANY"), |rt| bounds(rt, class, None, None))
        }
        // A loot context's parameters (`LootContextParams.THIS_ENTITY`), one per id: each of
        // 1.21.1's 12 is named by its field.
        LOOT_PARAMS if !field.is_empty() && field.bytes().all(|b| b.is_ascii_uppercase() || b == b'_') => {
            let id = format!("minecraft:{}", field.to_ascii_lowercase());
            rt.intern(format!("compat:loot-param/{id}"), move |rt| {
                let p = rt.new_object(LOOT_PARAM);
                let n = rt.string(&id);
                rt.put_field(p, "$name", n);
                p
            })
        }
        // A loot context's parameter sets (`LootContextParamSets.BLOCK`), by name.
        "net/minecraft/world/level/storage/loot/parameters/LootContextParamSets" => rt.intern(format!("compat:loot-param-set/{field}"), |rt| {
            let set = rt.new_object("net/minecraft/world/level/storage/loot/parameters/LootContextParamSet");
            let n = rt.string(&field.to_ascii_lowercase());
            rt.put_field(set, "$name", n);
            set
        }),
        "net/minecraft/world/item/component/BlockItemStateProperties" if field == "EMPTY" => rt.intern("const/BlockItemStateProperties.EMPTY".into(), |rt| {
            let m = rt.alloc("java/util/LinkedHashMap", Data::Table(Default::default()));
            crate::records::make(rt, "net/minecraft/world/item/component/BlockItemStateProperties", &[m])
        }),
        CHUNK_POS if field == "ZERO" => rt.intern("compat:chunk-pos-zero".into(), |rt| chunk_pos(rt, 0, 0)),
        CHUNK_POS if field == "INVALID_CHUNK_POS" => V::J(chunk_long(1_875_066, 1_875_066)),
        // `Integer.TYPE`: the primitive's own class (`int.class`), as reflection names it.
        "java/lang/Integer" | "java/lang/Long" | "java/lang/Short" | "java/lang/Byte" | "java/lang/Float" | "java/lang/Double" | "java/lang/Boolean"
        | "java/lang/Character" | "java/lang/Void"
            if field == "TYPE" =>
        {
            let primitive = match owner.trim_start_matches("java/lang/") {
                "Integer" => "int".to_owned(),
                "Character" => "char".to_owned(),
                other => other.to_ascii_lowercase(),
            };
            crate::jlang::class_object(rt, &primitive)
        }
        // `StandardCharsets.UTF_8` and the others: one object each, named as Java names them.
        "java/nio/charset/StandardCharsets" => {
            let name = match field {
                "UTF_8" => "UTF-8",
                "US_ASCII" => "US-ASCII",
                "ISO_8859_1" => "ISO-8859-1",
                "UTF_16" => "UTF-16",
                "UTF_16BE" => "UTF-16BE",
                "UTF_16LE" => "UTF-16LE",
                _ => return None,
            };
            rt.intern(format!("compat:charset/{name}"), |rt| {
                let c = rt.new_object("java/nio/charset/Charset");
                let n = rt.string(name);
                rt.put_field(c, "$name", n);
                c
            })
        }
        o if PLAIN_ENUMS.iter().any(|(c, _)| *c == o) => plain_enum(rt, o, field)?,
        "net/minecraft/util/Mth" if field == "SQRT_OF_TWO" => V::F(std::f32::consts::SQRT_2),
        // The build limits any dimension type may have (`DimensionType.MIN_Y`, `MAX_Y`).
        "net/minecraft/world/level/dimension/DimensionType" => match field {
            "BITS_FOR_Y" => V::I(12),
            "MIN_HEIGHT" => V::I(16),
            "Y_SIZE" => V::I(4064),
            "MAX_Y" => V::I(2031),
            "MIN_Y" => V::I(-2032),
            _ => return None,
        },
        // The six slots' `occupied` properties of a chiseled bookshelf, vanilla's own.
        "net/minecraft/world/level/block/ChiseledBookShelfBlock" if field == "SLOT_OCCUPIED_PROPERTIES" => {
            let props: Vec<V> = (0..6).filter_map(|i| vanilla_property_object(rt, &format!("CHISELED_BOOKSHELF_SLOT_{i}_OCCUPIED"))).collect();
            crate::jutil::new_list(rt, props)
        }
        LOCAL_TIME => match field {
            "MIDNIGHT" | "MIN" => local_time(rt, 0),
            "NOON" => local_time(rt, NANOS_PER_DAY / 2),
            "MAX" => local_time(rt, NANOS_PER_DAY - 1),
            _ => return None,
        },
        DURATION if field == "ZERO" => duration(rt, 0),
        VEC2 => {
            let (x, y) = match field {
                "ZERO" => (0.0, 0.0),
                "ONE" => (1.0, 1.0),
                "UNIT_X" => (1.0, 0.0),
                "NEG_UNIT_X" => (-1.0, 0.0),
                "UNIT_Y" => (0.0, 1.0),
                "NEG_UNIT_Y" => (0.0, -1.0),
                "MAX" => (f32::MAX, f32::MAX),
                "MIN" => (f32::MIN, f32::MIN),
                _ => return None,
            };
            let f = field.to_owned();
            rt.intern(format!("vec2/{f}"), move |rt| vec2(rt, x, y))
        }
        "net/minecraft/world/level/block/state/properties/BlockStateProperties" => vanilla_property_object(rt, field)?,
        "net/minecraft/world/level/block/PipeBlock" if field == "PROPERTY_BY_DIRECTION" => {
            rt.intern("prop/pipe-by-direction".into(), |rt| {
                let m = rt.alloc("java/util/HashMap", Data::Table(Default::default()));
                for (o, f) in ["DOWN", "UP", "NORTH", "SOUTH", "WEST", "EAST"].into_iter().enumerate() {
                    let d = direction(rt, o as i64);
                    if let Some(p) = vanilla_property_object(rt, f) {
                        crate::jutil::table_put(rt, m, d, p);
                    }
                }
                m
            })
        }
        // A vanilla block class's own property field (`SlabBlock.TYPE`, `DoorBlock.OPEN`): the
        // `BlockStateProperties` field it holds.
        o if o.starts_with("net/minecraft/world/level/block/")
            && (!o[32..].contains('/') || o[32..].starts_with("piston/"))
            && state_property_field(block_class_property(o, field)) =>
        {
            vanilla_property_object(rt, block_class_property(o, field))?
        }
        "net/minecraft/world/item/ItemStack" if field == "EMPTY" => {
            rt.intern("stack/empty".into(), |rt| stack(rt, "minecraft:air", 0))
        }
        // Vanilla's block entity types (`BlockEntityType.LECTERN`; 26.3 keeps them in
        // `BlockEntityTypes`), one object each under its id: Create keys display targets by them.
        "net/minecraft/world/level/block/entity/BlockEntityType" | "net/minecraft/world/level/block/entity/BlockEntityTypes"
            if VANILLA_BLOCK_ENTITY_TYPES.contains(&field) =>
        {
            vanilla_block_entity_type(rt, field)
        }
        "net/minecraft/world/item/Items" => item(rt, &crate::api::field_id(field)),
        // What a villager eats and picks up (1.21.1's `Villager.FOOD_POINTS` and `WANTED_ITEMS`, read
        // from its initialiser): FarmersDelight adds its crops to them.
        "net/minecraft/world/entity/npc/Villager" if field == "FOOD_POINTS" => rt.intern("compat:villager-food-points".into(), |rt| {
            let m = rt.alloc("java/util/LinkedHashMap", Data::Table(Default::default()));
            for (id, points) in [("bread", 4), ("potato", 1), ("carrot", 1), ("beetroot", 1)] {
                let (it, n) = (item(rt, id), crate::api::call(rt, "java/lang/Integer.valueOf.(I)Ljava/lang/Integer;", &[V::I(points)]));
                crate::jutil::table_put(rt, m, it, n);
            }
            m
        }),
        "net/minecraft/world/entity/npc/Villager" if field == "WANTED_ITEMS" => rt.intern("compat:villager-wanted-items".into(), |rt| {
            let ids = ["bread", "potato", "carrot", "wheat", "wheat_seeds", "beetroot", "beetroot_seeds", "torchflower_seeds", "pitcher_pod"];
            let items: Vec<V> = ids.iter().map(|id| item(rt, id)).collect();
            crate::jutil::new_set_of(rt, &items)
        }),
        // Vanilla's tag constants: the key of the tag each names in 26.3.
        "net/minecraft/tags/BlockTags" | "net/minecraft/tags/ItemTags" => {
            let kind = if owner == "net/minecraft/tags/BlockTags" { "block" } else { "item" };
            let tag = crate::vanilla_tags::named(kind, field)?;
            crate::fluids::tag_key(rt, kind, &tag)
        }
        "net/minecraft/world/InteractionResult" | "net/minecraft/world/ItemInteractionResult" => result(rt, owner, field),
        "net/minecraft/world/InteractionHand" => {
            let ordinal = if field == "OFF_HAND" { 1 } else { 0 };
            let f = field.to_owned();
            rt.intern(format!("hand/{f}"), move |rt| {
                let o = rt.new_object("net/minecraft/world/InteractionHand");
                rt.put_field(o, "$ordinal", V::I(ordinal));
                let n = rt.string(&f);
                rt.put_field(o, "$name", n);
                o
            })
        }
        "java/lang/Boolean" if field == "TRUE" || field == "FALSE" => boolean(rt, field == "TRUE"),
        // The primitives' classes (`int.class` is `Integer.TYPE`), one object each.
        "java/lang/Integer" | "java/lang/Long" | "java/lang/Boolean" | "java/lang/Byte" | "java/lang/Character" | "java/lang/Short" | "java/lang/Float" | "java/lang/Double" | "java/lang/Void"
            if field == "TYPE" =>
        {
            let primitive = match owner {
                "java/lang/Integer" => "int",
                "java/lang/Character" => "char",
                o => &o["java/lang/".len()..],
            };
            rt.class_literal(&primitive.to_ascii_lowercase())
        }
        "net/minecraft/core/Direction$Axis" => axis(rt, AXIS_NAMES.iter().position(|a| a.eq_ignore_ascii_case(field))? as i64),
        "net/minecraft/core/Direction$AxisDirection" if field == "POSITIVE" || field == "NEGATIVE" => {
            let positive = field == "POSITIVE";
            rt.intern(format!("axisdir/{positive}"), |rt| {
                let v = rt.alloc("net/minecraft/core/Direction$AxisDirection", Data::Native("AxisDirection", [positive as i64, 0, 0, 0]));
                rt.put_field(v, "$ordinal", V::I(if positive { 0 } else { 1 }));
                v
            })
        }
        "net/minecraft/core/Direction$Plane" if field == "HORIZONTAL" || field == "VERTICAL" => {
            let vertical = field == "VERTICAL";
            rt.intern(format!("plane/{vertical}"), |rt| {
                let v = rt.alloc("net/minecraft/core/Direction$Plane", Data::Native("Plane", [vertical as i64, 0, 0, 0]));
                rt.put_field(v, "$ordinal", V::I(vertical as i32));
                v
            })
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    /// A client mod turns the window's own player (Create's turntable, in a frame): their view
    /// turns in the game, and reads back so; a pitch past straight up is kept at it, and an angle
    /// that isn't finite is dropped. On a server, turning the acting player turns the server's
    /// copy for the rest of the hook, and the game, whose player's client holds their look, is not
    /// told. Their head and body read as their view.
    #[test]
    fn a_mod_turns_the_window_player_and_a_servers_copy() {
        const LOCAL: &str = "net/minecraft/client/player/LocalPlayer";
        const PLAYER: &str = "net/minecraft/world/entity/player/Player";
        let get = |rt: &mut Rt, key: &str, p: V| -> f32 {
            crate::api::call(rt, Box::leak(key.to_owned().into_boxed_str()), &[p]).f()
        };
        let set = |rt: &mut Rt, key: &str, p: V, to: f32| {
            crate::api::call(
                rt,
                Box::leak(key.to_owned().into_boxed_str()),
                &[p, V::F(to)],
            );
        };
        let mut rt = Rt::new();
        crate::install();
        rt.interned.insert("compat:client".into(), V::I(1));
        let mut host = crate::host::MemoryHost {
            place: Some(([0.5, 64.0, 0.5], 10.0, 0.0)),
            client: true,
            client_on_ground: true,
            ..Default::default()
        };
        // Where they stand: on the block below their feet, a player on a block's top.
        let stands = |rt: &mut Rt, me: V| {
            let on = crate::api::call(
                rt,
                "net/minecraft/client/player/LocalPlayer.onGround.()Z",
                &[me],
            )
            .i() != 0;
            let below = crate::api::call(
                rt,
                "net/minecraft/client/player/LocalPlayer.getOnPos.()Lnet/minecraft/core/BlockPos;",
                &[me],
            );
            (on, crate::api::pos_of(rt, below))
        };
        rt.with_host(&mut host, |rt| {
            let me = crate::network::local_player(rt);
            assert_eq!(stands(rt, me), (true, [0, 63, 0]));
            set(rt, &format!("{LOCAL}.setYRot.(F)V"), me, 45.0);
            set(
                rt,
                "net/minecraft/world/entity/Entity.setXRot.(F)V",
                me,
                120.0,
            );
            set(rt, &format!("{LOCAL}.setYRot.(F)V"), me, f32::INFINITY);
            assert_eq!(get(rt, &format!("{LOCAL}.getYRot.()F"), me), 45.0);
            assert_eq!(get(rt, &format!("{LOCAL}.getXRot.()F"), me), 90.0);
            let body = crate::api::game_field(
                rt,
                me,
                "yBodyRot",
                "$yBodyRot",
                "net/minecraft/client/player/LocalPlayer.$yBodyRot.()F",
            );
            assert_eq!(body.f(), 45.0);
            assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        });
        assert_eq!(host.place, Some(([0.5, 64.0, 0.5], 45.0, 90.0)));
        // Half a block up, in the air: over the block their feet are in.
        host.place = Some(([-0.5, 64.5, 0.5], 45.0, 0.0));
        host.client_on_ground = false;
        rt.with_host(&mut host, |rt| {
            let me = crate::network::local_player(rt);
            assert_eq!(stands(rt, me), (false, [-1, 64, 0]));
        });
        let mut rt = Rt::new();
        let mut host = crate::host::MemoryHost {
            place: Some(([0.5, 64.0, 0.5], 10.0, 0.0)),
            player_id: Some(42),
            ..Default::default()
        };
        rt.with_host(&mut host, |rt| {
            let player = super::acting_player(rt);
            set(rt, &format!("{PLAYER}.setYRot.(F)V"), player, 45.0);
            set(rt, &format!("{PLAYER}.setXRot.(F)V"), player, -30.0);
            set(rt, &format!("{PLAYER}.setYHeadRot.(F)V"), player, 70.0);
            assert_eq!(get(rt, &format!("{PLAYER}.getYRot.()F"), player), 45.0);
            assert_eq!(get(rt, &format!("{PLAYER}.getXRot.()F"), player), -30.0);
            assert_eq!(get(rt, &format!("{PLAYER}.getYHeadRot.()F"), player), 45.0);
            assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        });
        assert_eq!(host.place, Some(([0.5, 64.0, 0.5], 10.0, 0.0)));
    }

    /// A block item's state for a click (`BlockItem.getPlacementState`): its block's
    /// `getStateForPlacement`, asked through `Block` as vanilla's code asks it, runs the rule of
    /// the block's own class or the one above it that has one (a lever's is
    /// `FaceAttachedHorizontalDirectionalBlock`'s: looking down, on the floor facing the way the
    /// player faces), kept where it survives. The click is a context made as a mod makes one, of a
    /// player, a hand, a stack and a hit (`BlockPlaceContext`'s own constructor), which places
    /// where the hit says (in the air it clicked, which it replaces).
    #[test]
    fn a_block_items_placement_state_follows_its_blocks_rule() {
        const HIT_INIT: &str = "net/minecraft/world/phys/BlockHitResult.<init>.(Lnet/minecraft/world/phys/Vec3;Lnet/minecraft/core/Direction;Lnet/minecraft/core/BlockPos;Z)V";
        const CONTEXT_INIT: &str = "net/minecraft/world/item/context/BlockPlaceContext.<init>.(Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/InteractionHand;Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/phys/BlockHitResult;)V";
        const PLACEMENT_STATE: &str = "net/minecraft/world/item/BlockItem.getPlacementState.(Lnet/minecraft/world/item/context/BlockPlaceContext;)Lnet/minecraft/world/level/block/state/BlockState;";
        const CLICKED_POS: &str = "net/minecraft/world/item/context/BlockPlaceContext.getClickedPos.()Lnet/minecraft/core/BlockPos;";
        let mut rt = Rt::new();
        crate::install();
        let mut host = crate::host::MemoryHost::default();
        rt.with_host(&mut host, |rt| {
            let player = crate::menus::server_player(rt, 42);
            rt.put_field(player, "$yaw", V::F(0.0));
            rt.put_field(player, "$pitch", V::F(90.0));
            let hand = crate::api::get_static(rt, "net/minecraft/world/InteractionHand.MAIN_HAND");
            let stack = super::stack(rt, "minecraft:lever", 1);
            let at = super::vec3(rt, 0.5, 65.0, 0.5);
            let up = crate::api::direction(rt, 1);
            let pos = crate::api::block_pos(rt, 0, 64, 0);
            let hit = rt.new_object("net/minecraft/world/phys/BlockHitResult");
            crate::api::call(rt, HIT_INIT, &[hit, at, up, pos, V::I(0)]);
            let ctx = rt.new_object(super::PLACE_CONTEXT);
            crate::api::call(rt, CONTEXT_INIT, &[ctx, player, hand, stack, hit]);
            let clicked = crate::api::call(rt, CLICKED_POS, &[ctx]);
            assert_eq!(
                crate::api::pos_of(rt, clicked),
                [0, 64, 0],
                "the air clicked is replaced"
            );
            let lever = super::item(rt, "minecraft:lever");
            let state = crate::api::call(rt, PLACEMENT_STATE, &[lever, ctx]);
            let text = rt.str_of(state).unwrap_or("").to_owned();
            assert!(
                text.contains("face=floor") && text.contains("facing=south"),
                "{text}"
            );
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A vanilla enum's name in data (`getSerializedName`), asked through the interface or its
    /// class: its constant's in lower case (`EquipmentSlot.MAINHAND` is `mainhand`) but where
    /// vanilla's differs (`ItemDisplayContext.FIRST_PERSON_RIGHT_HAND` is `firstperson_righthand`,
    /// a heightmap keeps its name, a team's sidebar is `sidebar.team.<colour>`).
    #[test]
    fn vanilla_enums_give_their_serialized_names() {
        const NAME: &str =
            "net/minecraft/util/StringRepresentable.getSerializedName.()Ljava/lang/String;";
        const CONTEXT_NAME: &str =
            "net/minecraft/world/item/ItemDisplayContext.getSerializedName.()Ljava/lang/String;";
        let mut rt = Rt::new();
        crate::install();
        let named = |rt: &mut Rt, key: &'static str, constant: &'static str| {
            let v = crate::api::get_static(rt, constant);
            let s = crate::api::call(rt, key, &[v]);
            rt.str_of(s).map(str::to_owned)
        };
        assert_eq!(
            named(
                &mut rt,
                NAME,
                "net/minecraft/world/entity/EquipmentSlot.MAINHAND"
            )
            .as_deref(),
            Some("mainhand")
        );
        assert_eq!(
            named(
                &mut rt,
                CONTEXT_NAME,
                "net/minecraft/world/item/ItemDisplayContext.FIRST_PERSON_RIGHT_HAND"
            )
            .as_deref(),
            Some("firstperson_righthand")
        );
        assert_eq!(
            named(
                &mut rt,
                NAME,
                "net/minecraft/world/item/ItemDisplayContext.GUI"
            )
            .as_deref(),
            Some("gui")
        );
        assert_eq!(
            named(
                &mut rt,
                NAME,
                "net/minecraft/world/level/levelgen/Heightmap$Types.MOTION_BLOCKING"
            )
            .as_deref(),
            Some("MOTION_BLOCKING")
        );
        assert_eq!(
            named(
                &mut rt,
                NAME,
                "net/minecraft/world/scores/DisplaySlot.TEAM_DARK_AQUA"
            )
            .as_deref(),
            Some("sidebar.team.dark_aqua")
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `ItemInteractionResult` as 1.21.1's: `result()` is the plain result it stands for (both
    /// block-interaction passes are a pass), and it consumes the action when that does; the plain
    /// results' `values()` and ordinals are in declaration order (a mod's `switch` reads them).
    #[test]
    fn item_results_stand_for_plain_results() {
        use super::*;
        const ITEM_RESULT: &str = "net/minecraft/world/ItemInteractionResult.result.()Lnet/minecraft/world/InteractionResult;";
        const CONSUMES: &str = "net/minecraft/world/ItemInteractionResult.consumesAction.()Z";
        const VALUES: &str = "net/minecraft/world/InteractionResult.values.()[Lnet/minecraft/world/InteractionResult;";
        const ORDINAL: &str = "net/minecraft/world/InteractionResult.ordinal.()I";
        let mut rt = Rt::new();
        crate::install();
        let names = [
            "SUCCESS",
            "CONSUME",
            "CONSUME_PARTIAL",
            "PASS_TO_DEFAULT_BLOCK_INTERACTION",
            "SKIP_DEFAULT_BLOCK_INTERACTION",
            "FAIL",
        ];
        let (mut plain, mut consumes) = (Vec::new(), Vec::new());
        for n in names {
            let r = crate::api::get_static(
                &mut rt,
                format!("net/minecraft/world/ItemInteractionResult.{n}").leak(),
            );
            let p = crate::api::call(&mut rt, ITEM_RESULT, &[r]);
            plain.push(name_of(&rt, p));
            consumes.push(crate::api::call(&mut rt, CONSUMES, &[r]).i());
        }
        assert_eq!(
            plain,
            [
                "SUCCESS",
                "CONSUME",
                "CONSUME_PARTIAL",
                "PASS",
                "PASS",
                "FAIL"
            ]
        );
        assert_eq!(consumes, [1, 1, 1, 0, 0, 0]);
        let pass = crate::api::get_static(&mut rt, "net/minecraft/world/InteractionResult.PASS");
        let skip = crate::api::get_static(
            &mut rt,
            "net/minecraft/world/ItemInteractionResult.SKIP_DEFAULT_BLOCK_INTERACTION",
        );
        assert_eq!(
            crate::api::call(&mut rt, ITEM_RESULT, &[skip]),
            pass,
            "the one constant"
        );
        let values = crate::api::call(&mut rt, VALUES, &[]);
        let all = crate::jutil::elements(&rt, values);
        assert_eq!(all.len(), 6);
        assert_eq!(all[4], pass);
        let ordinals: Vec<i32> = all
            .iter()
            .map(|&v| crate::api::call(&mut rt, ORDINAL, &[v]).i())
            .collect();
        assert_eq!(ordinals, [0, 1, 2, 3, 4, 5]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// The game's version as a mod made for 1.21.1 reads it: that version's `version.json` (name,
    /// protocol, pack formats, data version).
    #[test]
    fn the_game_is_1_21_1_to_its_mods() {
        use super::*;
        let mut rt = Rt::new();
        crate::install();
        let c = |rt: &mut Rt, k: &'static str, args: &[V]| crate::api::call(rt, k, args);
        let v = c(&mut rt, "net/minecraft/SharedConstants.getCurrentVersion.()Lnet/minecraft/WorldVersion;", &[]);
        let name = c(&mut rt, "net/minecraft/WorldVersion.getName.()Ljava/lang/String;", &[v]);
        assert_eq!(rt.str_of(name), Some("1.21.1"));
        assert_eq!(c(&mut rt, "net/minecraft/WorldVersion.getProtocolVersion.()I", &[v]), V::I(767));
        let pack = "net/minecraft/WorldVersion.getPackVersion.(Lnet/minecraft/server/packs/PackType;)I";
        let data = crate::api::get_static(&mut rt, "net/minecraft/server/packs/PackType.SERVER_DATA");
        let resources = crate::api::get_static(&mut rt, "net/minecraft/server/packs/PackType.CLIENT_RESOURCES");
        assert_eq!((c(&mut rt, pack, &[v, resources]), c(&mut rt, pack, &[v, data])), (V::I(34), V::I(48)));
        let dv = c(&mut rt, "net/minecraft/WorldVersion.getDataVersion.()Lnet/minecraft/world/level/storage/DataVersion;", &[v]);
        assert_eq!(c(&mut rt, "net/minecraft/world/level/storage/DataVersion.getVersion.()I", &[dv]), V::I(3955));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// The client's player before the game has placed it (a resource reload at start, the probe
    /// runtime drawing a block entity's renderer) is where a new entity is: at the origin, with no
    /// rotation, so looking south.
    #[test]
    fn a_player_the_game_has_not_placed_is_at_the_origin() {
        use super::*;
        let mut rt = Rt::new();
        crate::install();
        let p = rt.new_object(LOCAL_PLAYER);
        let c = |rt: &mut Rt, k: &'static str| crate::api::call(rt, k, &[p]);
        let at = c(
            &mut rt,
            "net/minecraft/world/entity/Entity.position.()Lnet/minecraft/world/phys/Vec3;",
        );
        assert_eq!(xyz(&rt, at), [0.0, 0.0, 0.0]);
        assert_eq!(
            c(&mut rt, "net/minecraft/world/entity/Entity.getZ.()D"),
            V::D(0.0)
        );
        let block = c(
            &mut rt,
            "net/minecraft/world/entity/Entity.blockPosition.()Lnet/minecraft/core/BlockPos;",
        );
        assert_eq!(crate::api::pos_of(&rt, block), [0, 0, 0]);
        let look = c(
            &mut rt,
            "net/minecraft/world/entity/Entity.getLookAngle.()Lnet/minecraft/world/phys/Vec3;",
        );
        assert_eq!(xyz(&rt, look), [0.0, 0.0, 1.0]);
        let eye = c(
            &mut rt,
            "net/minecraft/world/entity/Entity.getEyePosition.()Lnet/minecraft/world/phys/Vec3;",
        );
        assert_eq!(xyz(&rt, eye), [0.0, f64::from(1.62f32), 0.0]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Only a shulker box can't go inside an item that holds items: vanilla's (any colour) or a
    /// mod's block item whose block is one. A mod item saying no (Create's package) is heard from
    /// the stack's question, through NeoForge's stack-aware default.
    #[test]
    fn only_a_shulker_box_or_what_says_so_cannot_go_inside_an_item() {
        use super::*;
        fn package(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {
            if key == "canFitInsideContainerItems.()Z" && rt.class_of(args[0]) == "test/Package" {
                return V::I(0);
            }
            crate::api::call(rt, owner_key, args)
        }
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(package);
        let fits = |rt: &mut Rt, name: &str| {
            let s = stack(rt, name, 1);
            crate::api::call(rt, "net/minecraft/world/item/ItemStack.canFitInsideContainerItems.()Z", &[s]) != V::I(0)
        };
        assert!(fits(&mut rt, "minecraft:iron_pickaxe"));
        assert!(!fits(&mut rt, "minecraft:shulker_box"));
        assert!(!fits(&mut rt, "minecraft:red_shulker_box"));
        rt.supers.insert("test/Package", "net/minecraft/world/item/Item");
        let package_item = rt.new_object("test/Package");
        rt.registered.insert(("item".into(), "test:package".into()), package_item);
        assert!(!fits(&mut rt, "test:package"));
        rt.supers.insert("test/Crate", "net/minecraft/world/level/block/ShulkerBoxBlock");
        rt.supers.insert("test/CrateItem", "net/minecraft/world/item/BlockItem");
        let crate_block = rt.new_object("test/Crate");
        let crate_item = rt.new_object("test/CrateItem");
        rt.put_field(crate_item, "$block", crate_block);
        rt.registered.insert(("item".into(), "test:crate".into()), crate_item);
        assert!(!fits(&mut rt, "test:crate"));
        let plain = crate::api::call(&mut rt, "net/minecraft/world/item/Item.canFitInsideContainerItems.()Z", &[crate_item]);
        assert_eq!(plain, V::I(0));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `BlockState.getProperties()`: a vanilla state's are `BlockStateProperties`' of its names
    /// and values, so Copycats' `tryCopyProperties` (each property `to` has takes `from`'s value)
    /// carries a door's state onto another door; a mod block's are its own objects.
    #[test]
    fn a_states_properties_carry_its_values_onto_another() {
        use super::*;
        let mut rt = Rt::new();
        crate::install();
        const GET_PROPERTIES: &str = "net/minecraft/world/level/block/state/BlockState.getProperties.()Ljava/util/Collection;";
        let from = state(&mut rt, "minecraft:oak_door[facing=east,half=upper,hinge=right,open=true,powered=false]");
        let props = crate::api::call(&mut rt, GET_PROPERTIES, &[from]);
        let props = crate::jutil::elements(&rt, props);
        let names: Vec<String> = props.iter().map(|p| prop_parts(&rt, *p).0).collect();
        assert_eq!(names, ["facing", "half", "hinge", "open", "powered"]);
        let facing = crate::api::get_static(&mut rt, "net/minecraft/world/level/block/state/properties/BlockStateProperties.FACING");
        assert_eq!(props[0], facing);
        let mut to = state(&mut rt, "ruby:door[facing=north,half=lower,hinge=left,open=false,powered=true]");
        for p in props {
            let has = crate::api::call(&mut rt, "net/minecraft/world/level/block/state/BlockState.hasProperty.(Lnet/minecraft/world/level/block/state/properties/Property;)Z", &[to, p]);
            if has.i() != 0 {
                let v = crate::api::call(&mut rt, "net/minecraft/world/level/block/state/BlockState.getValue.(Lnet/minecraft/world/level/block/state/properties/Property;)Ljava/lang/Comparable;", &[from, p]);
                to = crate::api::call(&mut rt, "net/minecraft/world/level/block/state/BlockState.setValue.(Lnet/minecraft/world/level/block/state/properties/Property;Ljava/lang/Comparable;)Ljava/lang/Object;", &[to, p, v]);
            }
        }
        assert_eq!(rt.str_of(to), Some("ruby:door[facing=east,half=upper,hinge=right,open=true,powered=false]"));

        let block = rt.new_object("test/Drawer");
        let (n, k) = (rt.string("slots"), rt.string("int"));
        let slots = rt.new_object("test/SlotsProperty");
        rt.put_field(slots, "$name", n);
        rt.put_field(slots, "$kind", k);
        let list = crate::jutil::new_list(&mut rt, vec![slots]);
        rt.put_field(block, "$state-props", list);
        rt.registered.insert(("block".into(), "ruby:drawer".into()), block);
        let drawer = state(&mut rt, "ruby:drawer[slots=2]");
        let own = crate::api::call(&mut rt, GET_PROPERTIES, &[drawer]);
        assert_eq!(crate::jutil::elements(&rt, own), vec![slots]);
    }

    /// `MinMaxBounds.Ints.between(1, 3)` matches 1 to 3 and keeps the squares; `ANY` matches all;
    /// `Doubles.atLeast(0.5)` matches from 0.5 up (vanilla's records and their `matches`).
    #[test]
    fn bounds_match_as_vanillas_records_do() {
        use super::*;
        let mut rt = Rt::new();
        crate::install();
        let between = crate::api::call(&mut rt, "net/minecraft/advancements/critereon/MinMaxBounds$Ints.between.(II)Lnet/minecraft/advancements/critereon/MinMaxBounds$Ints;", &[V::I(1), V::I(3)]);
        let matches = |rt: &mut Rt, b: V, v: i32| crate::api::call(rt, "net/minecraft/advancements/critereon/MinMaxBounds$Ints.matches.(I)Z", &[b, V::I(v)]).i();
        assert_eq!([0, 1, 2, 3, 4].map(|v| matches(&mut rt, between, v)), [0, 1, 1, 1, 0]);
        let max_sq = rt.get_field(rt.get_field(between, "maxSq"), "value");
        assert_eq!(max_sq, V::J(9));
        let any = crate::api::get_static(&mut rt, "net/minecraft/advancements/critereon/MinMaxBounds$Ints.ANY");
        assert_eq!(matches(&mut rt, any, i32::MIN), 1);
        assert_eq!(crate::api::call(&mut rt, "net/minecraft/advancements/critereon/MinMaxBounds$Ints.isAny.()Z", &[any]), V::I(1));
        let at_least = crate::api::call(&mut rt, "net/minecraft/advancements/critereon/MinMaxBounds$Doubles.atLeast.(D)Lnet/minecraft/advancements/critereon/MinMaxBounds$Doubles;", &[V::D(0.5)]);
        let d = |rt: &mut Rt, v: f64| crate::api::call(rt, "net/minecraft/advancements/critereon/MinMaxBounds$Doubles.matches.(D)Z", &[at_least, V::D(v)]).i();
        assert_eq!([d(&mut rt, 0.25), d(&mut rt, 0.5), d(&mut rt, 1e9)], [0, 1, 1]);
    }

    /// Days since 1970 as `LocalDate.ofEpochDay` gives them (a leap day, the day before 1970), and
    /// `LocalTime.until` counts whole units, signed (Mekanism's time to midnight is negative
    /// until it adds a day); a `Duration` reads back as Java's, from whole seconds rounded down;
    /// an executor runs what it is given at once, and a task with a period never runs.
    #[test]
    fn times_of_day_durations_and_executors_are_javas() {
        let mut rt = Rt::new();
        let c = |rt: &mut Rt, key: &str, args: &[V]| {
            let key = rt.static_key(key);
            crate::api::call(rt, key, args)
        };
        let half_past = c(&mut rt, "java/time/LocalTime.of.(II)Ljava/time/LocalTime;", &[V::I(23), V::I(30)]);
        let midnight = crate::api::get_static(&mut rt, "java/time/LocalTime.MIDNIGHT");
        let millis = crate::api::get_static(&mut rt, "java/time/temporal/ChronoUnit.MILLIS");
        let until = "java/time/LocalTime.until.(Ljava/time/temporal/Temporal;Ljava/time/temporal/TemporalUnit;)J";
        assert_eq!(c(&mut rt, until, &[half_past, midnight, millis]), V::J(-84_600_000));
        let minutes = crate::api::get_static(&mut rt, "java/time/temporal/ChronoUnit.MINUTES");
        assert_eq!(c(&mut rt, until, &[midnight, half_past, minutes]), V::J(1410));
        let d = c(&mut rt, "java/time/Duration.ofMillis.(J)Ljava/time/Duration;", &[V::J(-1500)]);
        assert_eq!(c(&mut rt, "java/time/Duration.getSeconds.()J", &[d]), V::J(-2));
        assert_eq!(c(&mut rt, "java/time/Duration.toMillis.()J", &[d]), V::J(-1500));
        let hours = crate::api::get_static(&mut rt, "java/time/temporal/ChronoUnit.HOURS");
        let hour = c(&mut rt, "java/time/temporal/ChronoUnit.getDuration.()Ljava/time/Duration;", &[hours]);
        assert_eq!(c(&mut rt, "java/time/Duration.toMinutes.()J", &[hour]), V::J(60));

        fn call(rt: &mut Rt, _id: u32, args: &[V]) -> Option<V> {
            let n = rt.get_field(args[0], "runs").i();
            rt.put_field(args[0], "runs", V::I(n + 1));
            Some(V::I(7))
        }
        rt.lambda_call = Some(call);
        let task = rt.lambda_id("java/util/concurrent/Callable", 1, vec![]);
        let pool = c(&mut rt, "java/util/concurrent/Executors.newSingleThreadExecutor.()Ljava/util/concurrent/ExecutorService;", &[]);
        let done = c(&mut rt, "java/util/concurrent/ExecutorService.submit.(Ljava/util/concurrent/Callable;)Ljava/util/concurrent/Future;", &[pool, task]);
        assert_eq!(c(&mut rt, "java/util/concurrent/Future.get.()Ljava/lang/Object;", &[done]), V::I(7));
        let timer = c(&mut rt, "java/util/concurrent/Executors.newSingleThreadScheduledExecutor.()Ljava/util/concurrent/ScheduledExecutorService;", &[]);
        let seconds = crate::api::get_static(&mut rt, "java/util/concurrent/TimeUnit.SECONDS");
        let periodic = rt.lambda_id("java/lang/Runnable", 1, vec![]);
        let f = c(
            &mut rt,
            "java/util/concurrent/ScheduledExecutorService.scheduleAtFixedRate.(Ljava/lang/Runnable;JJLjava/util/concurrent/TimeUnit;)Ljava/util/concurrent/ScheduledFuture;",
            &[timer, periodic, V::J(0), V::J(1), seconds],
        );
        assert_eq!(rt.get_field(periodic, "runs"), V::Void, "a periodic task doesn't run here");
        assert_eq!(c(&mut rt, "java/util/concurrent/ScheduledFuture.isDone.()Z", &[f]), V::I(0));
        assert_eq!(rt.get_field(task, "runs"), V::I(1));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `TimeUnit`'s conversions, which round down and saturate.
    #[test]
    fn dates_and_time_units_are_javas() {
        use super::*;
        assert_eq!(civil_date(0), (1970, 1, 1));
        assert_eq!(civil_date(-1), (1969, 12, 31));
        assert_eq!(civil_date(11_016), (2000, 2, 29));
        assert_eq!(civil_date(20_000), (2024, 10, 4));
        let mut rt = Rt::new();
        crate::install();
        let date = crate::api::call(&mut rt, "java/time/LocalDate.of.(III)Ljava/time/LocalDate;", &[V::I(2024), V::I(12), V::I(25)]);
        let month = crate::api::call(&mut rt, "java/time/LocalDate.getMonth.()Ljava/time/Month;", &[date]);
        assert_eq!(month, crate::api::get_static(&mut rt, "java/time/Month.DECEMBER"));
        assert_eq!(crate::api::call(&mut rt, "java/time/LocalDate.getDayOfMonth.()I", &[date]), V::I(25));
        let unit = |rt: &mut Rt, n: &str| {
            let k = rt.static_key(&format!("java/util/concurrent/TimeUnit.{n}"));
            crate::api::get_static(rt, k)
        };
        let (ms, s) = (unit(&mut rt, "MILLISECONDS"), unit(&mut rt, "SECONDS"));
        assert_eq!(crate::api::call(&mut rt, "java/util/concurrent/TimeUnit.toSeconds.(J)J", &[ms, V::J(1_999)]), V::J(1));
        assert_eq!(crate::api::call(&mut rt, "java/util/concurrent/TimeUnit.toMillis.(J)J", &[s, V::J(-2)]), V::J(-2_000));
        assert_eq!(crate::api::call(&mut rt, "java/util/concurrent/TimeUnit.toNanos.(J)J", &[s, V::J(i64::MAX)]), V::J(i64::MAX));
        assert_eq!(crate::api::call(&mut rt, "java/util/concurrent/TimeUnit.convert.(JLjava/util/concurrent/TimeUnit;)J", &[ms, V::J(3), s]), V::J(3_000));
    }

    /// A block position's centres, text, order, distances and turns, and the section it is in, as
    /// vanilla's `BlockPos`, `Vec3i` and `SectionPos` give them.
    #[test]
    fn positions_and_sections_answer_as_vanillas() {
        use super::*;
        const POS: &str = "net/minecraft/core/BlockPos";
        const VEC3I: &str = "net/minecraft/core/Vec3i";
        const SECTION: &str = "net/minecraft/core/SectionPos";
        const VEC: &str = "Lnet/minecraft/world/phys/Vec3;";
        fn c(rt: &mut Rt, key: &str, args: &[V]) -> V {
            let key = rt.static_key(key);
            crate::api::call(rt, key, args)
        }
        fn at(rt: &Rt, v: V) -> [i64; 3] {
            let n = native(rt, v);
            [n[0], n[1], n[2]]
        }
        let mut rt = Rt::new();
        crate::install();
        let pos = crate::api::block_pos(&mut rt, 1, 64, -3);
        let center = c(&mut rt, &format!("{POS}.getCenter.(){VEC}"), &[pos]);
        assert_eq!(xyz(&rt, center), [1.5, 64.5, -2.5]);
        let bottom = c(&mut rt, &format!("{POS}.getBottomCenter.(){VEC}"), &[pos]);
        assert_eq!(xyz(&rt, bottom), [1.5, 64.0, -2.5]);
        // A `Position` is read as the `Vec3` it is.
        assert_eq!(c(&mut rt, "net/minecraft/core/Position.y.()D", &[center]), V::D(64.5));
        let low = c(&mut rt, &format!("{POS}.atY.(I)L{POS};"), &[pos, V::I(-60)]);
        assert_eq!(at(&rt, low), [1, -60, -3]);
        // Text: `toShortString` (asked of a BlockPos, answered as `Vec3i`'s), and `toString` as
        // concatenation writes it, with the class's simple name.
        let short = c(&mut rt, &format!("{POS}.toShortString.()Ljava/lang/String;"), &[pos]);
        assert_eq!(rt.str_of(short), Some("1, 64, -3"));
        let text = rt.concat("at \u{1}", "L", &[pos], &[]);
        assert_eq!(rt.str_of(text), Some("at BlockPos{x=1, y=64, z=-3}"));
        // A direction and an axis concatenate as their own `toString` writes them.
        let (up, x) = (direction(&mut rt, 1), axis(&mut rt, 0));
        let text = rt.concat("\u{1} along \u{1}", "LL", &[up, x], &[]);
        assert_eq!(rt.str_of(text), Some("up along x"));
        let moving = c(&mut rt, &format!("{POS}.mutable.()L{POS}$MutableBlockPos;"), &[pos]);
        let text = c(&mut rt, &format!("{VEC3I}.toString.()Ljava/lang/String;"), &[moving]);
        assert_eq!(rt.str_of(text), Some("MutableBlockPos{x=1, y=64, z=-3}"));
        // Order: y first, then z, then x.
        for (other, want) in [([5, 64, -3], -4), ([1, 63, 9], 1), ([1, 64, -1], -2), ([1, 64, -3], 0)] {
            let o = crate::api::block_pos(&mut rt, other[0], other[1], other[2]);
            assert_eq!(c(&mut rt, &format!("{POS}.compareTo.(L{VEC3I};)I"), &[pos, o]), V::I(want), "{other:?}");
        }
        // From the centre (1.5, 64.5, -2.5) to (0, 64, 0): 1.5² + 0.5² + 2.5² = 8.75.
        let d = c(&mut rt, &format!("{POS}.distToCenterSqr.(DDD)D"), &[pos, V::D(0.0), V::D(64.0), V::D(0.0)]);
        assert_eq!(d, V::D(8.75));
        let origin = vec3(&mut rt, 0.0, 64.0, 0.0);
        let closer = |rt: &mut Rt, distance: f64| c(rt, &format!("{POS}.closerToCenterThan.(Lnet/minecraft/core/Position;D)Z"), &[pos, origin, V::D(distance)]);
        assert_eq!((closer(&mut rt, 3.0), closer(&mut rt, 2.9)), (V::I(1), V::I(0)));
        // Turns about the origin, seen from above: north (z -1) goes east (x +1) a quarter clockwise.
        let rotate = format!("{POS}.rotate.(Lnet/minecraft/world/level/block/Rotation;)L{POS};");
        for (ordinal, name, want) in [(1, "CLOCKWISE_90", [3, 64, 1]), (2, "CLOCKWISE_180", [-1, 64, 3]), (3, "COUNTERCLOCKWISE_90", [-3, 64, -1])] {
            let r = crate::api::api_enum(&mut rt, crate::rotation::ROTATION, name, ordinal);
            let turned = c(&mut rt, &rotate, &[pos, r]);
            assert_eq!(at(&rt, turned), want, "{name}");
        }
        let none = crate::api::api_enum(&mut rt, crate::rotation::ROTATION, "NONE", 0);
        assert_eq!(c(&mut rt, &rotate, &[pos, none]), pos);
        // A point kept within the block: x up to its low side plus 1e-5 (summed in floats), z down to
        // its high side less vanilla's ~1e-5.
        let outside = vec3(&mut rt, 0.0, 64.25, -1.0);
        let kept = c(&mut rt, &format!("{POS}.clampLocationWithin.({VEC}){VEC}"), &[pos, outside]);
        assert_eq!(xyz(&rt, kept), [f64::from(1.00001f32), 64.25, -2.0 + -9.999999747378752e-6]);

        // The section of block (-17, 70, 33): (-2, 4, 2), spanning x -32..-17, y 64..79, z 32..47.
        let block = crate::api::block_pos(&mut rt, -17, 70, 33);
        let section = c(&mut rt, &format!("{SECTION}.of.(L{POS};)L{SECTION};"), &[block]);
        assert_eq!(at(&rt, section), [-2, 4, 2]);
        assert_eq!(c(&mut rt, &format!("{SECTION}.x.()I"), &[section]), V::I(-2));
        assert_eq!(c(&mut rt, &format!("{SECTION}.getZ.()I"), &[section]), V::I(2));
        assert_eq!(c(&mut rt, &format!("{SECTION}.minBlockX.()I"), &[section]), V::I(-32));
        assert_eq!(c(&mut rt, &format!("{SECTION}.maxBlockY.()I"), &[section]), V::I(79));
        let origin = c(&mut rt, &format!("{SECTION}.origin.()L{POS};"), &[section]);
        assert_eq!(at(&rt, origin), [-32, 64, 32]);
        let middle = c(&mut rt, &format!("{SECTION}.center.()L{POS};"), &[section]);
        assert_eq!(at(&rt, middle), [-24, 72, 40]);
        let chunk = c(&mut rt, &format!("{SECTION}.chunk.()Lnet/minecraft/world/level/ChunkPos;"), &[section]);
        assert_eq!((rt.get_field(chunk, "x"), rt.get_field(chunk, "z")), (V::I(-2), V::I(2)));
        // Packed: x's 22 bits from bit 42, z's from bit 20, y in the low 20 (worked out by hand).
        let packed = c(&mut rt, &format!("{SECTION}.asLong.()J"), &[section]);
        assert_eq!(packed, V::J(-8_796_090_925_052));
        assert_eq!(c(&mut rt, &format!("{SECTION}.asLong.(L{POS};)J"), &[block]), packed);
        for (axis, want) in [("x", -2), ("y", 4), ("z", 2)] {
            assert_eq!(c(&mut rt, &format!("{SECTION}.{axis}.(J)I"), &[packed]), V::I(want), "{axis}");
        }
        let back = c(&mut rt, &format!("{SECTION}.of.(J)L{SECTION};"), &[packed]);
        assert_eq!(c(&mut rt, &format!("{VEC3I}.equals.(Ljava/lang/Object;)Z"), &[back, section]), V::I(1));
        let up = c(&mut rt, &format!("{SECTION}.offset.(JIII)J"), &[packed, V::I(0), V::I(1), V::I(0)]);
        assert_eq!(c(&mut rt, &format!("{SECTION}.y.(J)I"), &[up]), V::I(5));
        // A block's place in its section: x 15 in bits 8-11, z 1 in 4-7, y 6 in 0-3; and back.
        let local = c(&mut rt, &format!("{SECTION}.sectionRelativePos.(L{POS};)S"), &[block]);
        assert_eq!(local, V::I(15 << 8 | 1 << 4 | 6));
        let again = c(&mut rt, &format!("{SECTION}.relativeToBlockPos.(S)L{POS};"), &[section, local]);
        assert_eq!(at(&rt, again), [-17, 70, 33]);
        assert_eq!(c(&mut rt, &format!("{SECTION}.blockToSectionCoord.(I)I"), &[V::I(-17)]), V::I(-2));
        assert_eq!(c(&mut rt, &format!("{SECTION}.posToSectionCoord.(D)I"), &[V::D(-0.5)]), V::I(-1));
        let text = rt.concat("\u{1}", "L", &[section], &[]);
        assert_eq!(rt.str_of(text), Some("SectionPos{x=-2, y=4, z=2}"));
        // A `Vec3` as vanilla writes it: "(x, y, z)", in `Double.toString`'s digits.
        let text = rt.concat("\u{1}", "L", &[center], &[]);
        assert_eq!(rt.str_of(text), Some("(1.5, 64.5, -2.5)"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `ChunkPos` as vanilla packs it: x in a long's low half, z in its high; a block's chunk is its
    /// coordinates shifted by 4 (floor), and a chunk spans 16 blocks from x * 16.
    #[test]
    fn a_chunk_pos_packs_and_spans_as_vanillas() {
        use super::*;
        let mut rt = Rt::new();
        crate::install();
        let pos = crate::api::block_pos(&mut rt, -5, 70, 33);
        let packed = crate::api::call(&mut rt, "net/minecraft/world/level/ChunkPos.asLong.(Lnet/minecraft/core/BlockPos;)J", &[pos]);
        // Chunk (-1, 2): 0x0000_0002_ffff_ffff.
        assert_eq!(packed, V::J(12_884_901_887));
        assert_eq!(crate::api::call(&mut rt, "net/minecraft/world/level/ChunkPos.asLong.(II)J", &[V::I(-1), V::I(2)]), packed);
        assert_eq!(crate::api::call(&mut rt, "net/minecraft/world/level/ChunkPos.getX.(J)I", &[packed]), V::I(-1));
        assert_eq!(crate::api::call(&mut rt, "net/minecraft/world/level/ChunkPos.getZ.(J)I", &[packed]), V::I(2));
        let chunk = rt.new_object(CHUNK_POS);
        crate::api::call(&mut rt, "net/minecraft/world/level/ChunkPos.<init>.(Lnet/minecraft/core/BlockPos;)V", &[chunk, pos]);
        let get = |rt: &mut Rt, key: &'static str| crate::api::call(rt, key, &[chunk]);
        assert_eq!(get(&mut rt, "net/minecraft/world/level/ChunkPos.toLong.()J"), packed);
        assert_eq!(get(&mut rt, "net/minecraft/world/level/ChunkPos.getMinBlockX.()I"), V::I(-16));
        assert_eq!(get(&mut rt, "net/minecraft/world/level/ChunkPos.getMaxBlockX.()I"), V::I(-1));
        assert_eq!(get(&mut rt, "net/minecraft/world/level/ChunkPos.getMiddleBlockX.()I"), V::I(-8));
        assert_eq!(get(&mut rt, "net/minecraft/world/level/ChunkPos.getMinBlockZ.()I"), V::I(32));
        assert_eq!(get(&mut rt, "net/minecraft/world/level/ChunkPos.getMaxBlockZ.()I"), V::I(47));
        let from_long = rt.new_object(CHUNK_POS);
        crate::api::call(&mut rt, "net/minecraft/world/level/ChunkPos.<init>.(J)V", &[from_long, packed]);
        assert_eq!(crate::api::call(&mut rt, "net/minecraft/world/level/ChunkPos.equals.(Ljava/lang/Object;)Z", &[from_long, chunk]), V::I(1));
        // `hash(-1, 2)`: (1664525 * -1 + 1013904223) ^ (1664525 * (2 ^ 0xdeadbeef) + 1013904223), in ints.
        assert_eq!(get(&mut rt, "net/minecraft/world/level/ChunkPos.hashCode.()I"), V::I(1_445_764_666));
        let text = get(&mut rt, "net/minecraft/world/level/ChunkPos.toString.()Ljava/lang/String;");
        assert_eq!(rt.str_of(text), Some("[-1, 2]"));
        let zero = crate::api::get_static(&mut rt, "net/minecraft/world/level/ChunkPos.ZERO");
        assert_eq!(crate::api::call(&mut rt, "net/minecraft/world/level/ChunkPos.distanceSquared.(Lnet/minecraft/world/level/ChunkPos;)I", &[chunk, zero]), V::I(5));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `ModConfig.Type.values()`, as a config library's enum codec reads it (Copycats' config sync):
    /// FML's four types in order, the same objects as the statics.
    #[test]
    fn config_types_are_listed_as_flms() {
        use super::*;
        let mut rt = Rt::new();
        crate::install();
        let values = crate::api::call(&mut rt, "net/neoforged/fml/config/ModConfig$Type.values.()[Lnet/neoforged/fml/config/ModConfig$Type;", &[]);
        let names: Vec<String> = (0..4)
            .map(|i| {
                let v = rt.array_get(values, i);
                let n = crate::api::call(&mut rt, "java/lang/Enum.name.()Ljava/lang/String;", &[v]);
                rt.str_of(n).unwrap_or_default().to_owned()
            })
            .collect();
        assert_eq!(names, ["COMMON", "CLIENT", "SERVER", "STARTUP"]);
        let server = crate::api::get_static(&mut rt, "net/neoforged/fml/config/ModConfig$Type.SERVER");
        assert_eq!(rt.array_get(values, 2), server);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Mekanism counts stacks in `new ObjectOpenCustomHashSet<>(ItemStackLinkedSet.TYPE_AND_TAG)`:
    /// stacks of one item are one entry whatever their counts; another item is another.
    #[test]
    fn a_custom_hash_set_keeps_stacks_by_their_type() {
        use super::*;
        let mut rt = Rt::new();
        crate::install();
        fn vcall(rt: &mut Rt, key: &'static str, _short: &'static str, args: &[V]) -> V {
            crate::api::call(rt, key, args)
        }
        rt.vcall = Some(vcall);
        let strategy = crate::api::get_static(&mut rt, "net/minecraft/world/item/ItemStackLinkedSet.TYPE_AND_TAG");
        let set = rt.new_object("it/unimi/dsi/fastutil/objects/ObjectOpenCustomHashSet");
        crate::api::call(&mut rt, "it/unimi/dsi/fastutil/objects/ObjectOpenCustomHashSet.<init>.(Lit/unimi/dsi/fastutil/Hash$Strategy;)V", &[set, strategy]);
        let (stone, dirt) = (item(&mut rt, "minecraft:stone"), item(&mut rt, "minecraft:dirt"));
        let (one, five, other) = (stack_of(&mut rt, stone, 1), stack_of(&mut rt, stone, 5), stack_of(&mut rt, dirt, 1));
        let add = "it/unimi/dsi/fastutil/objects/ObjectOpenCustomHashSet.add.(Ljava/lang/Object;)Z";
        assert_eq!(crate::api::call(&mut rt, add, &[set, one]), V::I(1));
        assert_eq!(crate::api::call(&mut rt, add, &[set, five]), V::I(0), "the same item is in already");
        assert_eq!(crate::api::call(&mut rt, add, &[set, other]), V::I(1));
        assert_eq!(crate::api::call(&mut rt, "java/util/Set.size.()I", &[set]), V::I(2));
        // Cleared, the set still compares by the strategy.
        crate::api::call(&mut rt, "java/util/Set.clear.()V", &[set]);
        let again = stack_of(&mut rt, stone, 3);
        assert_eq!(crate::api::call(&mut rt, add, &[set, again]), V::I(1));
        assert_eq!(crate::api::call(&mut rt, add, &[set, one]), V::I(0), "the strategy outlives clear()");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn javas_own_classes_are_what_the_jdk_declares_them() {
        // Copycats' shape tables test `instanceof Map` on the `HashMap` they made; mods catch
        // `IllegalArgumentException` around `Integer.parseInt`.
        for (c, class, is) in [
            ("java/util/HashMap", "java/util/Map", true),
            ("java/util/LinkedHashMap", "java/util/HashMap", true),
            ("it/unimi/dsi/fastutil/objects/Object2ObjectOpenHashMap", "java/util/Map", true),
            ("com/google/common/collect/ArrayListMultimap", "java/util/Map", false),
            ("java/util/ArrayList", "java/util/Collection", true),
            ("java/util/ArrayList", "java/lang/Iterable", true),
            ("java/util/ArrayList", "java/util/Map", false),
            ("net/minecraft/core/NonNullList", "java/util/List", true),
            ("java/util/BitSet", "java/util/Set", false),
            ("java/util/ArrayDeque", "java/util/Deque", true),
            ("java/lang/Integer", "java/lang/Number", true),
            ("java/lang/String", "java/lang/CharSequence", true),
            ("java/lang/NumberFormatException", "java/lang/IllegalArgumentException", true),
            ("java/util/NoSuchElementException", "java/lang/RuntimeException", true),
            ("java/util/NoSuchElementException", "java/lang/Exception", true),
            ("com/google/gson/JsonSyntaxException", "java/lang/RuntimeException", true),
            ("com/example/RenderError", "java/lang/Throwable", false),
        ] {
            assert_eq!(super::api_is_a(c, class), is, "{c} instanceof {class}");
        }
    }

    #[test]
    fn vanilla_blocks_and_items_are_their_1_21_1_classes() {
        // A mod asks what kind of block it breaks or grows (`instanceof IceBlock`, Create's drill
        // melting ice; `BonemealableBlock`, a fertilizer) of the runtime's block named by its id.
        // Expected classes from 1.21.1's own declarations.
        crate::install();
        let mut rt = Rt::new();
        let b = |c: &str| format!("net/minecraft/world/level/block/{c}");
        let ice = crate::api::block(&mut rt, "minecraft:ice");
        let stone = crate::api::block(&mut rt, "minecraft:stone");
        let leaves = crate::api::block(&mut rt, "minecraft:oak_leaves");
        let wheat = crate::api::block(&mut rt, "minecraft:wheat");
        let chest = crate::api::block(&mut rt, "minecraft:chest");
        for (v, class, is) in [
            (ice, b("IceBlock"), true),
            (ice, b("HalfTransparentBlock"), true),
            (ice, b("Block"), true),
            (ice, b("LeavesBlock"), false),
            (stone, b("Block"), true),
            (stone, b("IceBlock"), false),
            (stone, b("BonemealableBlock"), false),
            (leaves, b("LeavesBlock"), true),
            (leaves, b("SimpleWaterloggedBlock"), true),
            (leaves, "net/neoforged/neoforge/common/IShearable".to_owned(), true),
            (wheat, b("CropBlock"), true),
            (wheat, b("BushBlock"), true),
            (wheat, b("BonemealableBlock"), true),
            (chest, b("EntityBlock"), true),
            (chest, b("BaseEntityBlock"), true),
            (chest, b("CropBlock"), false),
        ] {
            assert_eq!(rt.instance_of(v, &class), is, "{} instanceof {class}", super::name_of(&rt, v));
        }
        // Generated code's `instanceof`, whose answers are kept: one block's doesn't stand for another's.
        assert!(rt.instance_of_static(ice, "net/minecraft/world/level/block/IceBlock"));
        assert!(!rt.instance_of_static(stone, "net/minecraft/world/level/block/IceBlock"));
        let sword = super::item(&mut rt, "minecraft:diamond_sword");
        let diamond = super::item(&mut rt, "minecraft:diamond");
        let torch = super::item(&mut rt, "minecraft:torch");
        assert!(rt.instance_of(sword, "net/minecraft/world/item/SwordItem"));
        assert!(rt.instance_of(sword, "net/minecraft/world/item/TieredItem"));
        assert!(!rt.instance_of(diamond, "net/minecraft/world/item/TieredItem"));
        assert!(rt.instance_of(diamond, "net/minecraft/world/item/Item"));
        assert!(rt.instance_of(torch, "net/minecraft/world/item/BlockItem"), "a StandingAndWallBlockItem");
        // `getClass()` as `instanceof` answers.
        let class = crate::api::call(&mut rt, "java/lang/Object.getClass.()Ljava/lang/Class;", &[ice]);
        assert_eq!(rt.str_of(class), Some("net/minecraft/world/level/block/IceBlock"));
        // A mod's crop, extending vanilla's, is bonemealable through it.
        rt.supers.insert("com/example/RiceCrop", "net/minecraft/world/level/block/CropBlock");
        assert!(rt.is_a("com/example/RiceCrop", "net/minecraft/world/level/block/BonemealableBlock"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn the_vanilla_class_tables_are_sorted_for_their_searches() {
        use crate::vanilla_classes::{BLOCKS, CLASSES, ITEMS};
        assert!(BLOCKS.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(ITEMS.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(CLASSES.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn colours_and_dyes_answer_as_1_21_1_does() {
        let mut rt = Rt::new();
        let k = |s: &str| -> &'static str { Box::leak(format!("net/minecraft/util/FastColor$ARGB32.{s}").into_boxed_str()) };
        // Values by hand from 1.21.1's `FastColor.ARGB32`.
        assert_eq!(crate::api::call(&mut rt, k("opaque.(I)I"), &[V::I(0x123456)]), V::I(0xFF12_3456_u32 as i32));
        assert_eq!(crate::api::call(&mut rt, k("red.(I)I"), &[V::I(0x80FF_4020_u32 as i32)]), V::I(0xFF));
        assert_eq!(crate::api::call(&mut rt, k("alpha.(I)I"), &[V::I(0x80FF_4020_u32 as i32)]), V::I(0x80));
        assert_eq!(crate::api::call(&mut rt, k("multiply.(II)I"), &[V::I(0xFF80_8080_u32 as i32), V::I(0xFFFF_0000_u32 as i32)]), V::I(0xFF80_0000_u32 as i32));
        assert_eq!(crate::api::call(&mut rt, k("lerp.(FII)I"), &[V::F(0.5), V::I(0xFF00_0000_u32 as i32), V::I(0xFFFF_FFFF_u32 as i32)]), V::I(0xFF7F_7F7F_u32 as i32));
        let red = crate::api::get_static(&mut rt, "net/minecraft/world/item/DyeColor.RED");
        let dye = crate::api::call(&mut rt, "net/minecraft/world/item/DyeItem.byColor.(Lnet/minecraft/world/item/DyeColor;)Lnet/minecraft/world/item/DyeItem;", &[red]);
        assert_eq!(rt.str_of(dye), Some("minecraft:red_dye"));
        assert_eq!(crate::api::call(&mut rt, "java/lang/Math.ceilDiv.(II)I", &[V::I(7), V::I(2)]), V::I(4));
        assert_eq!(crate::api::call(&mut rt, "java/lang/Math.ceilDiv.(II)I", &[V::I(-7), V::I(2)]), V::I(-3));
    }

    /// Reach as Create's packets check it (`canInteractWithBlock(pos, 20)`): a block's box from the
    /// player's eyes, within the player's reach plus the margin.
    #[test]
    fn a_block_is_within_reach_by_its_box_from_the_eyes_plus_the_margin() {
        crate::install();
        let mut rt = Rt::new();
        let mut h = crate::host::MemoryHost { client: true, place: Some(([0.5, 64.0, 0.5], 0.0, 0.0)), ..Default::default() };
        let key = "net/minecraft/client/player/LocalPlayer.canInteractWithBlock.(Lnet/minecraft/core/BlockPos;D)Z";
        let reach = |rt: &mut Rt, z: i64, margin: f64| {
            let player = crate::network::local_player(rt);
            let pos = crate::api::block_pos(rt, 0, 64, z);
            crate::api::call(rt, key, &[player, pos, V::D(margin)]).i() != 0
        };
        rt.with_host(&mut h, |rt| {
            // Ten blocks south: 9.5 out along z, the eyes 0.62 above the box.
            assert!(!reach(rt, 10, 1.0));
            assert!(reach(rt, 10, 20.0));
            assert!(!reach(rt, 30, 20.0));
            // Four blocks: within survival's 4.5 with no margin.
            assert!(reach(rt, 4, 0.0));
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Item names as 1.21.1 gives them: `item.<id>` for an item, its block's `block.<id>` for a
    /// block item (a mod's, and vanilla's unless the language names the item itself, as redstone's),
    /// translatable text of it, and a stack's hover name: its custom name first. The display name
    /// is that in square brackets, italic for a custom name.
    #[test]
    fn items_and_stacks_are_named_by_their_description_ids() {
        crate::install();
        let mut rt = Rt::new();
        let mut h = crate::host::MemoryHost { client: true, ..Default::default() };
        h.block_items = vec!["minecraft:stone".into(), "minecraft:redstone".into()];
        h.lang.insert("item.minecraft.redstone".into(), "Redstone Dust".into());
        let filter = rt.new_object("net/minecraft/world/item/Item");
        rt.registered.insert(("item".into(), "create:filter".into()), filter);
        let shaft_block = rt.new_object("net/minecraft/world/level/block/Block");
        rt.registered.insert(("block".into(), "create:shaft".into()), shaft_block);
        let shaft = rt.new_object("net/minecraft/world/item/BlockItem");
        rt.put_field(shaft, "$block", shaft_block);
        rt.registered.insert(("item".into(), "create:shaft_item".into()), shaft);
        const ID: &str = "net/minecraft/world/item/Item.getDescriptionId.()Ljava/lang/String;";
        const DESCRIPTION: &str = "net/minecraft/world/item/Item.getDescription.()Lnet/minecraft/network/chat/Component;";
        const HOVER: &str = "net/minecraft/world/item/ItemStack.getHoverName.()Lnet/minecraft/network/chat/Component;";
        const DISPLAY: &str = "net/minecraft/world/item/ItemStack.getDisplayName.()Lnet/minecraft/network/chat/Component;";
        const SET: &str = "net/minecraft/world/item/ItemStack.set.(Lnet/minecraft/core/component/DataComponentType;Ljava/lang/Object;)Ljava/lang/Object;";
        rt.with_host(&mut h, |rt| {
            let id = |rt: &mut Rt, item: V| {
                let v = api::call(rt, ID, &[item]);
                rt.str_of(v).unwrap_or("").to_owned()
            };
            let diamond = super::item(rt, "minecraft:diamond");
            let stone = super::item(rt, "minecraft:stone");
            let redstone = super::item(rt, "minecraft:redstone");
            assert_eq!(id(rt, diamond), "item.minecraft.diamond");
            assert_eq!(id(rt, stone), "block.minecraft.stone");
            assert_eq!(id(rt, redstone), "item.minecraft.redstone");
            assert_eq!(id(rt, filter), "item.create.filter");
            assert_eq!(id(rt, shaft), "block.create.shaft");
            let d = api::call(rt, DESCRIPTION, &[filter]);
            assert_eq!(super::component_json(rt, d), serde_json::json!({"translate": "item.create.filter"}));
            let s = super::stack(rt, "create:filter", 1);
            let hover = api::call(rt, HOVER, &[s]);
            assert_eq!(super::component_json(rt, hover), serde_json::json!({"translate": "item.create.filter"}));
            let display = api::call(rt, DISPLAY, &[s]);
            assert_eq!(
                super::component_json(rt, display),
                serde_json::json!({"translate": "chat.square_brackets", "with": [{"text": "", "extra": [{"translate": "item.create.filter"}]}]})
            );
            let custom = api::get_static(rt, "net/minecraft/core/component/DataComponents.CUSTOM_NAME");
            let text = rt.string("Sorter");
            let name = super::component(rt, text, false, V::NULL);
            api::call(rt, SET, &[s, custom, name]);
            let hover = api::call(rt, HOVER, &[s]);
            assert_eq!(super::component_json(rt, hover), serde_json::json!({"text": "Sorter"}));
            let display = api::call(rt, DISPLAY, &[s]);
            assert_eq!(
                super::component_json(rt, display),
                serde_json::json!({"translate": "chat.square_brackets", "with": [{"text": "", "italic": true, "extra": [{"text": "Sorter"}]}]})
            );
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
    use crate::{Rt, V, api};

    const GET: &str = "net/minecraft/world/level/block/state/BlockState.getValue.(Lnet/minecraft/world/level/block/state/properties/Property;)Ljava/lang/Comparable;";
    const SET: &str = "net/minecraft/world/level/block/state/BlockState.setValue.(Lnet/minecraft/world/level/block/state/properties/Property;Ljava/lang/Comparable;)Ljava/lang/Object;";

    /// `BeltBlockEntity::hasPulley` run as a `Function`: the method's own raw `true`, which Java
    /// would box.
    fn raw_true(_rt: &mut Rt, _id: u32, _args: &[V]) -> Option<V> {
        Some(V::I(1))
    }

    /// Vanilla 1.21.1's `Vec3` and `Mth.sin`/`cos`, against values a JDK printed running the merged
    /// jar's own classes (`scratchpad/jdkcheck/vec/Vec3Check.java`).
    #[test]
    fn vec3_turns_crosses_and_aligns_as_vanilla_does() {
        let mut rt = Rt::new();
        crate::install();
        const VEC3: &str = "net/minecraft/world/phys/Vec3";
        let new = |rt: &mut Rt, [x, y, z]: [f64; 3]| {
            let v = rt.new_object(VEC3);
            api::call(rt, "net/minecraft/world/phys/Vec3.<init>.(DDD)V", &[v, V::D(x), V::D(y), V::D(z)]);
            v
        };
        let xyz = |rt: &Rt, v: V| [rt.get_field(v, "x").d(), rt.get_field(v, "y").d(), rt.get_field(v, "z").d()];
        let k = |s: &str| -> &'static str { Box::leak(format!("{VEC3}.{s}").into_boxed_str()) };
        let v = new(&mut rt, [1.5, -2.25, 3.0]);
        let turned = |rt: &mut Rt, m: &str, a: f32| {
            let r = api::call(rt, k(&format!("{m}.(F)Lnet/minecraft/world/phys/Vec3;")), &[v, V::F(a)]);
            xyz(rt, r)
        };
        assert_eq!(turned(&mut rt, "yRot", 0.7), [3.0798825323581696, -2.25, 1.32827827334404]);
        assert_eq!(turned(&mut rt, "xRot", -1.3), [1.5, -3.492403417825699, -1.3656218647956848]);
        assert_eq!(turned(&mut rt, "zRot", 2.9), [-1.9947652146220207, 1.82576322555542, 3.0]);
        let dir = api::call(&mut rt, k("directionFromRotation.(FF)Lnet/minecraft/world/phys/Vec3;"), &[V::F(30.5), V::F(-45.25)]);
        assert_eq!(xyz(&rt, dir), [0.611922562122345, -0.5075089931488037, 0.6065488457679749]);
        let (a, b) = (new(&mut rt, [0.3, 0.4, 0.5]), new(&mut rt, [-1.25, 2.0, 0.75]));
        let cross = api::call(&mut rt, k("cross.(Lnet/minecraft/world/phys/Vec3;)Lnet/minecraft/world/phys/Vec3;"), &[a, b]);
        assert_eq!(xyz(&rt, cross), [-0.7, -0.85, 1.1]);
        let to = new(&mut rt, [-4.0, 0.1, 9.5]);
        let lerp = api::call(&mut rt, k("lerp.(Lnet/minecraft/world/phys/Vec3;D)Lnet/minecraft/world/phys/Vec3;"), &[v, to, V::D(0.35)]);
        assert_eq!(xyz(&rt, lerp), [-0.4249999999999998, -1.4275, 5.275]);
        // A vector too short to normalize is `Vec3.ZERO` itself.
        let tiny = new(&mut rt, [1e-5, 0.0, 0.0]);
        let n = api::call(&mut rt, k("normalize.()Lnet/minecraft/world/phys/Vec3;"), &[tiny]);
        assert_eq!(n, api::get_static(&mut rt, "net/minecraft/world/phys/Vec3.ZERO"));
        // `align(EnumSet.of(Z, X))`: those axes floored.
        let (z, x) = (api::get_static(&mut rt, "net/minecraft/core/Direction$Axis.Z"), api::get_static(&mut rt, "net/minecraft/core/Direction$Axis.X"));
        let axes = api::call(&mut rt, "java/util/EnumSet.of.(Ljava/lang/Enum;Ljava/lang/Enum;)Ljava/util/EnumSet;", &[z, x]);
        let p = new(&mut rt, [-1.5, 2.7, 3.2]);
        let aligned = api::call(&mut rt, k("align.(Ljava/util/EnumSet;)Lnet/minecraft/world/phys/Vec3;"), &[p, axes]);
        assert_eq!(xyz(&rt, aligned), [-2.0, 2.7, 3.0]);
        let shown = new(&mut rt, [1.0e-5, -0.0, 123456789.125]);
        let text = api::call(&mut rt, k("toString.()Ljava/lang/String;"), &[shown]);
        assert_eq!(rt.str_of(text), Some("(1.0E-5, -0.0, 1.23456789125E8)"));
        let near = new(&mut rt, [2.0, 5.0, 3.5]);
        let closer = |rt: &mut Rt, v_range: f64| api::call(rt, k("closerThan.(Lnet/minecraft/world/phys/Vec3;DD)Z"), &[v, near, V::D(0.8), V::D(v_range)]);
        assert_eq!((closer(&mut rt, 7.3), closer(&mut rt, 7.2)), (V::I(1), V::I(0)));
        let rgb = api::call(&mut rt, k("fromRGB24.(I)Lnet/minecraft/world/phys/Vec3;"), &[V::I(0x7F3A10)]);
        assert_eq!(xyz(&rt, rgb), [0.4980392156862745, 0.22745098039215686, 0.06274509803921569]);
        let west = api::get_static(&mut rt, "net/minecraft/core/Direction.WEST");
        let rel = api::call(&mut rt, k("relative.(Lnet/minecraft/core/Direction;D)Lnet/minecraft/world/phys/Vec3;"), &[v, west, V::D(2.5)]);
        assert_eq!(xyz(&rt, rel), [-1.0, -2.25, 3.0]);
        assert_eq!(api::call(&mut rt, "net/minecraft/util/Mth.sin.(F)F", &[V::F(1234.567)]), V::F(0.078822285));
        assert_eq!(api::call(&mut rt, "net/minecraft/util/Mth.cos.(F)F", &[V::F(-0.001)]), V::F(0.99999946));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `EnumSet` keeps its enum's order and knows its type: `range` and `complementOf` as a JDK gives
    /// them for `Direction` (`range(NORTH, WEST)` = north, south, west; the complement of {up, east}
    /// = down, north, south, west).
    #[test]
    fn an_enum_set_ranges_and_complements_in_its_enums_order() {
        let mut rt = Rt::new();
        crate::install();
        let d = |rt: &mut Rt, n: &str| api::get_static(rt, Box::leak(format!("net/minecraft/core/Direction.{n}").into_boxed_str()));
        let names = |rt: &mut Rt, set: V| -> Vec<String> {
            crate::jutil::elements(rt, set).into_iter().map(|e| {
                let n = crate::api_common::enum_name(rt, e, true);
                rt.str_of(n).unwrap_or("").to_owned()
            }).collect()
        };
        let (north, west) = (d(&mut rt, "NORTH"), d(&mut rt, "WEST"));
        let range = api::call(&mut rt, "java/util/EnumSet.range.(Ljava/lang/Enum;Ljava/lang/Enum;)Ljava/util/EnumSet;", &[north, west]);
        assert_eq!(names(&mut rt, range), ["north", "south", "west"]);
        let (east, up) = (d(&mut rt, "EAST"), d(&mut rt, "UP"));
        let some = api::call(&mut rt, "java/util/EnumSet.of.(Ljava/lang/Enum;Ljava/lang/Enum;)Ljava/util/EnumSet;", &[east, up]);
        // Added east first, kept in order: up, east.
        assert_eq!(names(&mut rt, some), ["up", "east"]);
        let rest = api::call(&mut rt, "java/util/EnumSet.complementOf.(Ljava/util/EnumSet;)Ljava/util/EnumSet;", &[some]);
        assert_eq!(names(&mut rt, rest), ["down", "north", "south", "west"]);
        assert_eq!(api::call(&mut rt, "java/util/Set.contains.(Ljava/lang/Object;)Z", &[rest, north]), V::I(1));
        assert_eq!(api::call(&mut rt, "java/util/Set.contains.(Ljava/lang/Object;)Z", &[rest, up]), V::I(0));
        // An empty set still knows its type.
        let class = rt.string("net.minecraft.core.Direction");
        let none = api::call(&mut rt, "java/util/EnumSet.noneOf.(Ljava/lang/Class;)Ljava/util/EnumSet;", &[class]);
        let all = api::call(&mut rt, "java/util/EnumSet.complementOf.(Ljava/util/EnumSet;)Ljava/util/EnumSet;", &[none]);
        assert_eq!(names(&mut rt, all).len(), 6);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn an_optional_mapped_to_a_raw_boolean_holds_it() {
        // Create's `BeltBlock.hasShaftTowards`:
        // `getBlockEntityOptional(...).map(BeltBlockEntity::hasPulley).orElse(false)`, unboxed.
        const MAP: &str = "java/util/Optional.map.(Ljava/util/function/Function;)Ljava/util/Optional;";
        const OR_ELSE: &str = "java/util/Optional.orElse.(Ljava/lang/Object;)Ljava/lang/Object;";
        const UNBOX: &str = "java/lang/Boolean.booleanValue.()Z";
        let mut rt = Rt::new();
        rt.lambda_call = Some(raw_true);
        let has_pulley = rt.lambda_id("java/util/function/Function", 1, Vec::new());
        let no = api::call(&mut rt, "java/lang/Boolean.valueOf.(Z)Ljava/lang/Boolean;", &[V::I(0)]);
        let be = rt.new_object("test/BeltBlockEntity");
        let found = api::call(&mut rt, "java/util/Optional.of.(Ljava/lang/Object;)Ljava/util/Optional;", &[be]);
        let mapped = api::call(&mut rt, MAP, &[found, has_pulley]);
        assert_eq!(api::call(&mut rt, "java/util/Optional.isPresent.()Z", &[mapped]), V::I(1));
        let got = api::call(&mut rt, OR_ELSE, &[mapped, no]);
        assert_eq!(api::call(&mut rt, UNBOX, &[got]), V::I(1));
        // No block entity: the fallback.
        let empty = api::call(&mut rt, "java/util/Optional.empty.()Ljava/util/Optional;", &[]);
        let mapped = api::call(&mut rt, MAP, &[empty, has_pulley]);
        let got = api::call(&mut rt, OR_ELSE, &[mapped, no]);
        assert_eq!(api::call(&mut rt, UNBOX, &[got]), V::I(0));
    }

    #[test]
    fn a_block_state_is_in_the_block_tags_the_game_holds() {
        // Create's `BlockHelper.hasBlockSolidSide`: `!state.is(BlockTags.LEAVES) && ...`.
        const IS: &str = "net/minecraft/world/level/block/state/BlockState.is.(Lnet/minecraft/tags/TagKey;)Z";
        let mut rt = Rt::new();
        let mut host = crate::host::MemoryHost::default();
        host.block_tags.insert("minecraft:leaves".into(), vec!["minecraft:oak_leaves".into()]);
        let leaves = api::get_static(&mut rt, "net/minecraft/tags/BlockTags.LEAVES");
        let item_leaves = api::get_static(&mut rt, "net/minecraft/tags/ItemTags.LEAVES");
        assert_ne!(leaves, item_leaves);
        let oak = api::state(&mut rt, "minecraft:oak_leaves[distance=7,persistent=false,waterlogged=false]");
        let stone = api::state(&mut rt, "minecraft:stone");
        rt.with_host(&mut host, |rt| {
            assert_eq!(api::call(rt, IS, &[oak, leaves]), V::I(1));
            assert_eq!(api::call(rt, IS, &[stone, leaves]), V::I(0));
            // An item tag of the same name holds no block.
            assert_eq!(api::call(rt, IS, &[oak, item_leaves]), V::I(0));
        });
    }

    #[test]
    fn vanilla_block_entity_types_are_one_object_each_with_their_id() {
        let mut rt = Rt::new();
        let lectern = api::get_static(&mut rt, "net/minecraft/world/level/block/entity/BlockEntityType.LECTERN");
        let sign = api::get_static(&mut rt, "net/minecraft/world/level/block/entity/BlockEntityType.SIGN");
        assert_ne!(lectern, V::NULL);
        assert_ne!(lectern, sign);
        assert_eq!(api::get_static(&mut rt, "net/minecraft/world/level/block/entity/BlockEntityTypes.LECTERN"), lectern);
        assert_eq!(rt.registered_name(lectern, "block_entity_type"), Some("minecraft:lectern"));
        assert_eq!(rt.registered_name(sign, "block_entity_type"), Some("minecraft:sign"));
    }

    #[test]
    fn mth_answers_as_vanilla_26_3() {
        // Expected values printed by vanilla's own `Mth` (the 26.3 jar, run on the JDK).
        crate::install();
        let mut rt = Rt::new();
        let mut m = |name: &'static str, args: &[V]| -> V {
            let key: &'static str = Box::leak(name.to_owned().into_boxed_str());
            crate::api::call(&mut rt, key, args)
        };
        let logs: Vec<(i32, i32, i32)> = [0, 1, 2, 3, 64, 65, 1000, -5, i32::MIN]
            .iter()
            .map(|&v| {
                let a = [V::I(v)];
                (m("net/minecraft/util/Mth.log2.(I)I", &a).i(), m("net/minecraft/util/Mth.ceillog2.(I)I", &a).i(), m("net/minecraft/util/Mth.smallestEncompassingPowerOfTwo.(I)I", &a).i())
            })
            .collect();
        assert_eq!(logs, [(-1, 0, 0), (0, 0, 1), (1, 1, 2), (1, 2, 4), (6, 6, 64), (6, 7, 128), (9, 10, 1024), (-1, 0, 0), (31, 31, i32::MIN)]);
        assert_eq!(m("net/minecraft/util/Mth.wrapDegrees.(F)F", &[V::F(370.5)]), V::F(10.5));
        assert_eq!(m("net/minecraft/util/Mth.wrapDegrees.(D)D", &[V::D(-190.0)]), V::D(170.0));
        assert_eq!(m("net/minecraft/util/Mth.wrapDegrees.(I)I", &[V::I(540)]), V::I(-180));
        assert_eq!(m("net/minecraft/util/Mth.wrapDegrees.(J)F", &[V::J(725)]), V::F(5.0));
        assert_eq!(m("net/minecraft/util/Mth.wrapDegrees90.(F)F", &[V::F(100.0)]), V::F(10.0));
        assert_eq!(m("net/minecraft/util/Mth.degreesDifference.(FF)F", &[V::F(350.0), V::F(10.0)]), V::F(20.0));
        assert_eq!(m("net/minecraft/util/Mth.rotLerp.(FFF)F", &[V::F(0.25), V::F(350.0), V::F(10.0)]), V::F(355.0));
        assert_eq!(m("net/minecraft/util/Mth.approachDegrees.(FFF)F", &[V::F(350.0), V::F(10.0), V::F(5.0)]), V::F(355.0));
        assert_eq!(m("net/minecraft/util/Mth.rotateIfNecessary.(FFF)F", &[V::F(10.0), V::F(100.0), V::F(30.0)]), V::F(70.0));
        assert_eq!(m("net/minecraft/util/Mth.atan2.(DD)D", &[V::D(1.0), V::D(2.0)]), V::D(0.4636472634610012));
        assert_eq!(m("net/minecraft/util/Mth.atan2.(DD)D", &[V::D(-3.0), V::D(-0.5)]), V::D(-1.735944546704755));
        assert_eq!(m("net/minecraft/util/Mth.atan2.(DD)D", &[V::D(0.0), V::D(-1.0)]), V::D(std::f64::consts::PI));
        assert_eq!(m("net/minecraft/util/Mth.fastInvSqrt.(D)D", &[V::D(2.0)]), V::D(0.7069296507954639));
        assert_eq!(m("net/minecraft/util/Mth.hsvToRgb.(FFF)I", &[V::F(0.3), V::F(0.5), V::F(0.8)]), V::I(8047718));
        assert_eq!(m("net/minecraft/util/Mth.hsvToArgb.(FFFI)I", &[V::F(0.95), V::F(1.0), V::F(1.0), V::I(255)]), V::I(-65460));
        assert_eq!(m("net/minecraft/util/Mth.murmurHash3Mixer.(I)I", &[V::I(12345)]), V::I(1011272156));
        assert_eq!(m("net/minecraft/util/Mth.getSeed.(III)J", &[V::I(10), V::I(64), V::I(-20)]), V::J(-62212282568415));
        assert_eq!(m("net/minecraft/util/Mth.frac.(F)F", &[V::F(-1.25)]), V::F(0.75));
        assert_eq!(m("net/minecraft/util/Mth.frac.(D)D", &[V::D(2.75)]), V::D(0.75));
        assert_eq!(m("net/minecraft/util/Mth.positiveCeilDiv.(II)I", &[V::I(7), V::I(2)]), V::I(4));
        assert_eq!(m("net/minecraft/util/Mth.roundToward.(II)I", &[V::I(7), V::I(4)]), V::I(8));
        assert_eq!(m("net/minecraft/util/Mth.floorDiv.(II)I", &[V::I(-7), V::I(2)]), V::I(-4));
        assert_eq!(m("net/minecraft/util/Mth.lerpInt.(FII)I", &[V::F(0.5), V::I(2), V::I(9)]), V::I(5));
        assert_eq!(m("net/minecraft/util/Mth.lerpDiscrete.(FII)I", &[V::F(0.5), V::I(2), V::I(9)]), V::I(6));
        assert_eq!(m("net/minecraft/util/Mth.packDegrees.(F)B", &[V::F(200.0)]), V::I(-114));
        assert_eq!(m("net/minecraft/util/Mth.unpackDegrees.(B)F", &[V::I(-100)]), V::F(-140.625));
        assert_eq!(m("net/minecraft/util/Mth.triangleWave.(FF)F", &[V::F(7.5), V::F(4.0)]), V::F(0.5));
        assert_eq!(m("net/minecraft/util/Mth.catmullrom.(FFFFF)F", &[V::F(0.3), V::F(1.0), V::F(2.0), V::F(4.0), V::F(3.0)]), V::F(2.621));
        assert_eq!(m("net/minecraft/util/Mth.clampedMap.(DDDDD)D", &[V::D(5.0), V::D(0.0), V::D(10.0), V::D(100.0), V::D(200.0)]), V::D(150.0));
        assert_eq!(m("net/minecraft/util/Mth.smoothstep.(F)F", &[V::F(0.3)]), V::F(0.16308002));
        assert_eq!(m("net/minecraft/util/Mth.quantize.(DI)I", &[V::D(17.9), V::I(4)]), V::I(16));
        assert_eq!(m("net/minecraft/util/Mth.absMax.(FF)F", &[V::F(-3.5), V::F(2.0)]), V::F(3.5));
        assert_eq!(m("net/minecraft/util/Mth.length.(FF)F", &[V::F(3.0), V::F(4.0)]), V::F(5.0));
    }

    #[test]
    fn positions_pack_and_directions_turn_as_vanilla_does() {
        let mut rt = Rt::new();
        let c = |rt: &mut Rt, key: &'static str, args: &[V]| api::call(rt, key, args);
        let p = api::block_pos(&mut rt, 1, 2, 3);
        // x << 38 | z << 12 | y, by vanilla's layout: 274877906944 + 12288 + 2.
        assert_eq!(c(&mut rt, "net/minecraft/core/BlockPos.asLong.()J", &[p]), V::J(274_877_919_234));
        let neg = api::block_pos(&mut rt, -1, -1, -1);
        assert_eq!(c(&mut rt, "net/minecraft/core/BlockPos.asLong.()J", &[neg]), V::J(-1));
        let back = c(&mut rt, "net/minecraft/core/BlockPos.of.(J)Lnet/minecraft/core/BlockPos;", &[V::J(-1)]);
        assert_eq!(c(&mut rt, "net/minecraft/core/BlockPos.equals.(Ljava/lang/Object;)Z", &[back, neg]), V::I(1));
        // Vec3i.hashCode of (1, 2, 3): (2 + 3 * 31) * 31 + 1.
        assert_eq!(c(&mut rt, "net/minecraft/core/BlockPos.hashCode.()I", &[p]), V::I(2946));
        let north = api::direction(&mut rt, 2);
        let east = c(&mut rt, "net/minecraft/core/Direction.getClockWise.()Lnet/minecraft/core/Direction;", &[north]);
        assert_eq!(east, api::direction(&mut rt, 5));
        assert_eq!(c(&mut rt, "net/minecraft/core/Direction.get2DDataValue.()I", &[east]), V::I(3));
        let west = c(&mut rt, "net/minecraft/core/Direction.getCounterClockWise.()Lnet/minecraft/core/Direction;", &[north]);
        assert_eq!(west, api::direction(&mut rt, 4));
        let horizontal = api::get_static(&mut rt, "net/minecraft/core/Direction$Plane.HORIZONTAL");
        let up = api::direction(&mut rt, 1);
        assert_eq!(c(&mut rt, "net/minecraft/core/Direction$Plane.test.(Lnet/minecraft/core/Direction;)Z", &[horizontal, east]), V::I(1));
        assert_eq!(c(&mut rt, "net/minecraft/core/Direction$Plane.test.(Lnet/minecraft/core/Direction;)Z", &[horizontal, up]), V::I(0));
        // `for (Direction d : Plane.HORIZONTAL)`, through `Iterable`: north, east, south, west.
        let it = c(&mut rt, "java/lang/Iterable.iterator.()Ljava/util/Iterator;", &[horizontal]);
        let mut walked = Vec::new();
        while c(&mut rt, "java/util/Iterator.hasNext.()Z", &[it]).i() != 0 {
            let d = c(&mut rt, "java/util/Iterator.next.()Ljava/lang/Object;", &[it]);
            walked.push(super::native(&rt, d)[0]);
        }
        assert_eq!(walked, [2, 5, 3, 4]);
        // Nearest: (1, 0, 0) is east; (0, 0, 0) falls back to north; a tie goes to the first (down before up).
        let nearest = "net/minecraft/core/Direction.getNearest.(FFF)Lnet/minecraft/core/Direction;";
        assert_eq!(c(&mut rt, nearest, &[V::F(1.0), V::F(0.0), V::F(0.0)]), api::direction(&mut rt, 5));
        assert_eq!(c(&mut rt, nearest, &[V::F(0.0), V::F(0.0), V::F(0.0)]), north);
        assert_eq!(c(&mut rt, nearest, &[V::F(0.0), V::F(-2.0), V::F(-2.0)]), api::direction(&mut rt, 0));
    }

    #[test]
    fn state_properties_round_trip_with_java_identity() {
        let mut rt = Rt::new();
        let s = api::state(&mut rt, "minecraft:furnace[facing=north,lit=false]");
        let facing = api::get_static(&mut rt, "net/minecraft/world/level/block/state/properties/BlockStateProperties.HORIZONTAL_FACING");
        let lit = api::get_static(&mut rt, "net/minecraft/world/level/block/state/properties/BlockStateProperties.LIT");
        let north = api::get_static(&mut rt, "net/minecraft/core/Direction.NORTH");
        // `state.getValue(FACING) == Direction.NORTH` holds by identity, as in Java.
        assert_eq!(api::call(&mut rt, GET, &[s, facing]), north);
        let east = api::get_static(&mut rt, "net/minecraft/core/Direction.EAST");
        let t = api::call(&mut rt, SET, &[s, facing, east]);
        let on = api::get_static(&mut rt, "java/lang/Boolean.TRUE");
        let t = api::call(&mut rt, SET, &[t, lit, on]);
        assert_eq!(rt.str_of(t), Some("minecraft:furnace[facing=east,lit=true]"));
        // Equal states are the same object.
        assert_eq!(t, api::state(&mut rt, "minecraft:furnace[facing=east,lit=true]"));
    }

    /// A vanilla enum value is spelled in a state by its serialized name, not always its constant's
    /// own: Create's piston sets its head's `type` to `PistonType.DEFAULT`, which is `normal`.
    #[test]
    fn a_vanilla_enum_value_is_set_by_its_serialized_name() {
        let mut rt = Rt::new();
        let kind = api::get_static(
            &mut rt,
            "net/minecraft/world/level/block/state/properties/BlockStateProperties.PISTON_TYPE",
        );
        let default = api::get_static(
            &mut rt,
            "net/minecraft/world/level/block/state/properties/PistonType.DEFAULT",
        );
        let sticky = api::get_static(
            &mut rt,
            "net/minecraft/world/level/block/state/properties/PistonType.STICKY",
        );
        let s = api::state(
            &mut rt,
            "minecraft:piston_head[facing=east,short=false,type=sticky]",
        );
        let t = api::call(&mut rt, SET, &[s, kind, default]);
        assert_eq!(
            rt.str_of(t),
            Some("minecraft:piston_head[facing=east,short=false,type=normal]")
        );
        assert_eq!(api::call(&mut rt, GET, &[t, kind]), default);
        let u = api::call(&mut rt, SET, &[t, kind, sticky]);
        assert_eq!(u, s);
    }

    #[test]
    fn mod_enum_property_values_are_the_mods_constants() {
        let mut rt = Rt::new();
        // A mod enum constant as its <clinit> makes it: Enum.<init>(this, "SPEED", 1), then a static field.
        let speed = rt.new_object("com/example/Mode");
        let n = rt.string("SPEED");
        api::call(&mut rt, "java/lang/Enum.<init>.(Ljava/lang/String;I)V", &[speed, n, V::I(1)]);
        rt.statics.insert("com/example/Mode.SPEED", speed);
        let pname = rt.string("mode");
        let class = rt.string("com/example/Mode");
        let prop = api::call(&mut rt, "net/minecraft/world/level/block/state/properties/EnumProperty.create.(Ljava/lang/String;Ljava/lang/Class;)Lnet/minecraft/world/level/block/state/properties/EnumProperty;", &[pname, class]);
        let s = api::state(&mut rt, "example:gearbox[mode=speed]");
        assert_eq!(api::call(&mut rt, GET, &[s, prop]), speed);
    }

    #[test]
    fn nbt_vec3_and_stacks() {
        let mut rt = Rt::new();
        let tag = rt.new_object("net/minecraft/nbt/CompoundTag");
        api::call(&mut rt, "net/minecraft/nbt/CompoundTag.<init>.()V", &[tag]);
        let k = rt.string("Speed");
        api::call(&mut rt, "net/minecraft/nbt/CompoundTag.putInt.(Ljava/lang/String;I)V", &[tag, k, V::I(256)]);
        assert_eq!(api::call(&mut rt, "net/minecraft/nbt/CompoundTag.getInt.(Ljava/lang/String;)I", &[tag, k]), V::I(256));
        let missing = rt.string("Nope");
        assert_eq!(api::call(&mut rt, "net/minecraft/nbt/CompoundTag.getInt.(Ljava/lang/String;)I", &[tag, missing]), V::I(0));

        let a = rt.new_object("net/minecraft/world/phys/Vec3");
        api::call(&mut rt, "net/minecraft/world/phys/Vec3.<init>.(DDD)V", &[a, V::D(3.0), V::D(0.0), V::D(4.0)]);
        assert_eq!(api::call(&mut rt, "net/minecraft/world/phys/Vec3.length.()D", &[a]), V::D(5.0));
        assert_eq!(rt.get_field(a, "z"), V::D(4.0)); // `vec.z` field reads work

        // A field names its 26.3 id: the chain 26.3 renamed, 1.21.1's own odd names.
        let id = |rt: &mut Rt, key: &'static str| {
            let v = api::get_static(rt, key);
            rt.str_of(v).map(str::to_owned)
        };
        for (key, want) in [
            ("net/minecraft/world/level/block/Blocks.CHAIN", "minecraft:iron_chain"),
            ("net/minecraft/world/level/block/Blocks.POTTED_AZALEA", "minecraft:potted_azalea_bush"),
            ("net/minecraft/world/level/block/Blocks.STONE", "minecraft:stone"),
            ("net/minecraft/world/item/Items.CHAIN", "minecraft:iron_chain"),
            ("net/minecraft/world/item/Items.CUT_STANDSTONE_SLAB", "minecraft:cut_sandstone_slab"),
            ("net/minecraft/world/item/Items.STONE", "minecraft:stone"),
        ] {
            assert_eq!(id(&mut rt, key).as_deref(), Some(want), "{key}");
        }
        let stone = api::get_static(&mut rt, "net/minecraft/world/item/Items.STONE");
        let s = rt.new_object("net/minecraft/world/item/ItemStack");
        api::call(&mut rt, "net/minecraft/world/item/ItemStack.<init>.(Lnet/minecraft/world/level/ItemLike;I)V", &[s, stone, V::I(3)]);
        api::call(&mut rt, "net/minecraft/world/item/ItemStack.shrink.(I)V", &[s, V::I(3)]);
        assert_eq!(api::call(&mut rt, "net/minecraft/world/item/ItemStack.isEmpty.()Z", &[s]), V::I(1));
        let empty = api::get_static(&mut rt, "net/minecraft/world/item/ItemStack.EMPTY");
        assert_eq!(api::call(&mut rt, "net/minecraft/world/item/ItemStack.isEmpty.()Z", &[empty]), V::I(1));
    }

    /// What crafting with an item leaves: vanilla's water bucket the bucket, stone nothing, a mod's
    /// item the remainder its properties name (`craftRemainder`).
    #[test]
    fn crafting_with_an_item_leaves_its_remainder() {
        let mut rt = Rt::new();
        crate::install();
        let has = "net/minecraft/world/item/ItemStack.hasCraftingRemainingItem.()Z";
        let get = "net/minecraft/world/item/ItemStack.getCraftingRemainingItem.()Lnet/minecraft/world/item/ItemStack;";
        let left = |rt: &mut Rt, s: V| {
            let r = api::call(rt, get, &[s]);
            (api::call(rt, has, &[s]).i(), super::stack_empty(rt, r), super::item_like_name(rt, rt.get_field(r, "$item")))
        };
        let water = super::stack(&mut rt, "minecraft:water_bucket", 1);
        assert_eq!(left(&mut rt, water), (1, false, "minecraft:bucket".to_owned()));
        let stone = super::stack(&mut rt, "minecraft:stone", 1);
        assert_eq!(left(&mut rt, stone).0, 0);
        assert!(left(&mut rt, stone).1);

        let props = rt.new_object("net/minecraft/world/item/Item$Properties");
        api::call(&mut rt, "net/minecraft/world/item/Item$Properties.<init>.()V", &[props]);
        let bowl = api::get_static(&mut rt, "net/minecraft/world/item/Items.BOWL");
        api::call(&mut rt, "net/minecraft/world/item/Item$Properties.craftRemainder.(Lnet/minecraft/world/item/Item;)Lnet/minecraft/world/item/Item$Properties;", &[props, bowl]);
        let it = rt.new_object("net/minecraft/world/item/Item");
        api::call(&mut rt, "net/minecraft/world/item/Item.<init>.(Lnet/minecraft/world/item/Item$Properties;)V", &[it, props]);
        let item_left = api::call(&mut rt, "net/minecraft/world/item/Item.getCraftingRemainingItem.()Lnet/minecraft/world/item/Item;", &[it]);
        assert_eq!(super::item_like_name(&rt, item_left), "minecraft:bowl");
    }

    /// A block entity saves a component as JSON text and loads it back (Create's assembly errors):
    /// the translation key and its arguments, a literal's text.
    #[test]
    fn a_component_written_as_json_reads_back_as_the_same_text() {
        let mut rt = Rt::new();
        let c = |rt: &mut Rt, k: &str, args: &[V]| crate::api::call(rt, Box::leak(k.to_owned().into_boxed_str()), args);
        let key = rt.string("create.gui.assembly.exception.too_many_pistons");
        let five = super::integer(&mut rt, 5);
        let name = rt.string("Piston");
        let args = rt.alloc("[Ljava/lang/Object;", crate::Data::Array(vec![five, name]));
        let t = c(&mut rt, "net/minecraft/network/chat/Component.translatable.(Ljava/lang/String;[Ljava/lang/Object;)Lnet/minecraft/network/chat/MutableComponent;", &[key, args]);
        let json = c(&mut rt, "net/minecraft/network/chat/Component$Serializer.toJson.(Lnet/minecraft/network/chat/Component;Lnet/minecraft/core/HolderLookup$Provider;)Ljava/lang/String;", &[t, V::NULL]);
        let text = rt.str_of(json).unwrap().to_owned();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&text).unwrap(), serde_json::json!({"translate": "create.gui.assembly.exception.too_many_pistons", "with": [5, "Piston"]}));
        let back = c(&mut rt, "net/minecraft/network/chat/Component$Serializer.fromJson.(Ljava/lang/String;Lnet/minecraft/core/HolderLookup$Provider;)Lnet/minecraft/network/chat/MutableComponent;", &[json, V::NULL]);
        let again = c(&mut rt, "net/minecraft/network/chat/Component$Serializer.toJson.(Lnet/minecraft/network/chat/Component;Lnet/minecraft/core/HolderLookup$Provider;)Ljava/lang/String;", &[back, V::NULL]);
        assert_eq!(rt.str_of(again), Some(text.as_str()));
        let hello = rt.string("hello");
        let lit = c(&mut rt, "net/minecraft/network/chat/Component.literal.(Ljava/lang/String;)Lnet/minecraft/network/chat/MutableComponent;", &[hello]);
        let j = c(&mut rt, "net/minecraft/network/chat/Component$Serializer.toJson.(Lnet/minecraft/network/chat/Component;Lnet/minecraft/core/HolderLookup$Provider;)Ljava/lang/String;", &[lit, V::NULL]);
        assert_eq!(rt.str_of(j), Some(r#"{"text":"hello"}"#));
    }

    /// What a villager eats and picks up, as 1.21.1 lists them, and then what a mod writes over them
    /// (FarmersDelight's `Villager.FOOD_POINTS = ImmutableMap.copyOf(theirs)`): later reads see it.
    #[test]
    fn villager_food_is_vanillas_until_a_mod_replaces_it() {
        let mut rt = Rt::new();
        crate::install();
        const FOOD: &str = "net/minecraft/world/entity/npc/Villager.FOOD_POINTS";
        let points = api::get_static(&mut rt, FOOD);
        let bread = super::item(&mut rt, "minecraft:bread");
        let n = api::call(&mut rt, "java/util/Map.get.(Ljava/lang/Object;)Ljava/lang/Object;", &[points, bread]);
        assert_eq!(api::call(&mut rt, "java/lang/Integer.intValue.()I", &[n]), V::I(4));
        assert_eq!(api::call(&mut rt, "java/util/Map.size.()I", &[points]), V::I(4));
        let wanted = api::get_static(&mut rt, "net/minecraft/world/entity/npc/Villager.WANTED_ITEMS");
        let seeds = super::item(&mut rt, "minecraft:wheat_seeds");
        assert_eq!(api::call(&mut rt, "java/util/Set.contains.(Ljava/lang/Object;)Z", &[wanted, seeds]), V::I(1));
        assert_eq!(api::call(&mut rt, "java/util/Set.size.()I", &[wanted]), V::I(9));

        let theirs = rt.alloc("java/util/HashMap", crate::Data::Table(Default::default()));
        let cabbage = super::item(&mut rt, "farmersdelight:cabbage");
        let one = api::call(&mut rt, "java/lang/Integer.valueOf.(I)Ljava/lang/Integer;", &[V::I(1)]);
        api::call(&mut rt, "java/util/HashMap.put.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;", &[theirs, cabbage, one]);
        api::call(&mut rt, "java/util/HashMap.putAll.(Ljava/util/Map;)V", &[theirs, points]);
        let copy = api::call(&mut rt, "com/google/common/collect/ImmutableMap.copyOf.(Ljava/util/Map;)Lcom/google/common/collect/ImmutableMap;", &[theirs]);
        rt.statics.insert(FOOD, copy);
        let now = api::get_static(&mut rt, FOOD);
        assert_eq!(api::call(&mut rt, "java/util/Map.size.()I", &[now]), V::I(5));
        let n = api::call(&mut rt, "java/util/Map.get.(Ljava/lang/Object;)Ljava/lang/Object;", &[now, cabbage]);
        assert_eq!(api::call(&mut rt, "java/lang/Integer.intValue.()I", &[n]), V::I(1));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod's own loot parameter set (guardvillagers' `new LootContextParamSet.Builder()
    /// .required(THIS_ENTITY)...build()`): it requires and allows what it was given, one parameter
    /// object per id.
    #[test]
    fn a_mods_loot_parameter_set_holds_its_parameters() {
        let mut rt = Rt::new();
        crate::install();
        let this_entity = api::get_static(&mut rt, "net/minecraft/world/level/storage/loot/parameters/LootContextParams.THIS_ENTITY");
        assert_eq!(api::get_static(&mut rt, "net/minecraft/world/level/storage/loot/parameters/LootContextParams.THIS_ENTITY"), this_entity);
        let origin = api::get_static(&mut rt, "net/minecraft/world/level/storage/loot/parameters/LootContextParams.ORIGIN");
        let tool = api::get_static(&mut rt, "net/minecraft/world/level/storage/loot/parameters/LootContextParams.TOOL");
        let b = rt.new_object("net/minecraft/world/level/storage/loot/parameters/LootContextParamSet$Builder");
        api::call(&mut rt, "net/minecraft/world/level/storage/loot/parameters/LootContextParamSet$Builder.<init>.()V", &[b]);
        let required = "net/minecraft/world/level/storage/loot/parameters/LootContextParamSet$Builder.required.(Lnet/minecraft/world/level/storage/loot/parameters/LootContextParam;)Lnet/minecraft/world/level/storage/loot/parameters/LootContextParamSet$Builder;";
        let optional = "net/minecraft/world/level/storage/loot/parameters/LootContextParamSet$Builder.optional.(Lnet/minecraft/world/level/storage/loot/parameters/LootContextParam;)Lnet/minecraft/world/level/storage/loot/parameters/LootContextParamSet$Builder;";
        assert_eq!(api::call(&mut rt, required, &[b, this_entity]), b);
        api::call(&mut rt, optional, &[b, origin]);
        let set = api::call(&mut rt, "net/minecraft/world/level/storage/loot/parameters/LootContextParamSet$Builder.build.()Lnet/minecraft/world/level/storage/loot/parameters/LootContextParamSet;", &[b]);
        let allowed = |rt: &mut Rt, p: V| api::call(rt, "net/minecraft/world/level/storage/loot/parameters/LootContextParamSet.isAllowed.(Lnet/minecraft/world/level/storage/loot/parameters/LootContextParam;)Z", &[set, p]);
        assert_eq!((allowed(&mut rt, this_entity), allowed(&mut rt, origin), allowed(&mut rt, tool)), (V::I(1), V::I(1), V::I(0)));
        let needed = api::call(&mut rt, "net/minecraft/world/level/storage/loot/parameters/LootContextParamSet.getRequired.()Ljava/util/Set;", &[set]);
        assert_eq!(crate::jutil::elements(&rt, needed), [this_entity]);
        let name = api::call(&mut rt, "net/minecraft/world/level/storage/loot/parameters/LootContextParam.getName.()Lnet/minecraft/resources/ResourceLocation;", &[this_entity]);
        assert_eq!(rt.str_of(name), Some("minecraft:this_entity"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A mod's `getUpdatePacket`, `ClientboundBlockEntityDataPacket.create(this)`: the packet holds
    /// the block entity's position and type and the update tag its own `getUpdateTag` makes;
    /// `create(this, saver)` holds what `saver` makes of the block entity and the registries.
    #[test]
    fn a_block_entitys_update_packet_holds_its_update_tag() {
        const PACKET: &str = "net/minecraft/network/protocol/game/ClientboundBlockEntityDataPacket";
        fn tag_with(rt: &mut Rt, key: &str, value: i32) -> V {
            let tag = rt.new_object("net/minecraft/nbt/CompoundTag");
            api::call(rt, "net/minecraft/nbt/CompoundTag.<init>.()V", &[tag]);
            let k = rt.string(key);
            api::call(rt, "net/minecraft/nbt/CompoundTag.putInt.(Ljava/lang/String;I)V", &[tag, k, V::I(value)]);
            tag
        }
        fn int_of(rt: &mut Rt, tag: V, key: &str) -> V {
            let k = rt.string(key);
            api::call(rt, "net/minecraft/nbt/CompoundTag.getInt.(Ljava/lang/String;)I", &[tag, k])
        }
        // The mod's block entity overrides `getUpdateTag`: its fuel.
        fn vcall(rt: &mut Rt, _key: &'static str, short: &'static str, _args: &[V]) -> V {
            assert_eq!(short, "getUpdateTag.(Lnet/minecraft/core/HolderLookup$Provider;)Lnet/minecraft/nbt/CompoundTag;");
            tag_with(rt, "Fuel", 7)
        }
        // The saver `(be, registries) -> be.saveCustomOnly(registries)`: 1 when it was given the
        // block entity and the registries.
        fn lambdas(rt: &mut Rt, _id: u32, args: &[V]) -> Option<V> {
            let given = rt.class_of(args[1]) == "test/Furnace" && rt.class_of(args[2]) == "net/minecraft/core/RegistryAccess";
            Some(tag_with(rt, "Saved", given as i32))
        }
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(vcall);
        rt.lambda_call = Some(lambdas);
        let furnace = api::get_static(&mut rt, "net/minecraft/world/level/block/entity/BlockEntityType.FURNACE");
        let pos = api::block_pos(&mut rt, 4, 64, -2);
        let state = api::state(&mut rt, "minecraft:furnace[facing=north,lit=false]");
        let be = rt.new_object("test/Furnace");
        api::call(&mut rt, "net/minecraft/world/level/block/entity/BlockEntity.<init>.(Lnet/minecraft/world/level/block/entity/BlockEntityType;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)V", &[be, furnace, pos, state]);

        let packet = api::call(&mut rt, "net/minecraft/network/protocol/game/ClientboundBlockEntityDataPacket.create.(Lnet/minecraft/world/level/block/entity/BlockEntity;)Lnet/minecraft/network/protocol/game/ClientboundBlockEntityDataPacket;", &[be]);
        assert_eq!(rt.class_of(packet), PACKET);
        assert_eq!(api::call(&mut rt, "net/minecraft/network/protocol/game/ClientboundBlockEntityDataPacket.getPos.()Lnet/minecraft/core/BlockPos;", &[packet]), pos);
        assert_eq!(api::call(&mut rt, "net/minecraft/network/protocol/game/ClientboundBlockEntityDataPacket.getType.()Lnet/minecraft/world/level/block/entity/BlockEntityType;", &[packet]), furnace);
        let tag = api::call(&mut rt, "net/minecraft/network/protocol/game/ClientboundBlockEntityDataPacket.getTag.()Lnet/minecraft/nbt/CompoundTag;", &[packet]);
        assert_eq!(int_of(&mut rt, tag, "Fuel"), V::I(7));

        let saver = rt.lambda_id("test:saver", 1, Vec::new());
        let packet = api::call(&mut rt, "net/minecraft/network/protocol/game/ClientboundBlockEntityDataPacket.create.(Lnet/minecraft/world/level/block/entity/BlockEntity;Ljava/util/function/BiFunction;)Lnet/minecraft/network/protocol/game/ClientboundBlockEntityDataPacket;", &[be, saver]);
        let tag = api::call(&mut rt, "net/minecraft/network/protocol/game/ClientboundBlockEntityDataPacket.getTag.()Lnet/minecraft/nbt/CompoundTag;", &[packet]);
        assert_eq!(int_of(&mut rt, tag, "Saved"), V::I(1));
        assert_eq!(int_of(&mut rt, tag, "Fuel"), V::I(0));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A block entity's constructor that reads its own `level` (Create's `sendData` when a value
    /// takes its default) finds null and not removed, as Java's fields start; once in a level it
    /// finds that one.
    #[test]
    fn a_new_block_entity_has_no_level_until_it_joins_one() {
        const INIT: &str = "net/minecraft/world/level/block/entity/BlockEntity.<init>.(Lnet/minecraft/world/level/block/entity/BlockEntityType;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)V";
        const LEVEL: &str = "net/minecraft/world/level/block/entity/BlockEntity.$level.()Lnet/minecraft/world/level/Level;";
        const REMOVED: &str = "net/minecraft/world/level/block/entity/BlockEntity.$remove.()Z";
        const HAS_LEVEL: &str = "net/minecraft/world/level/block/entity/BlockEntity.hasLevel.()Z";
        const SET_LEVEL: &str = "net/minecraft/world/level/block/entity/BlockEntity.setLevel.(Lnet/minecraft/world/level/Level;)V";
        let mut rt = Rt::new();
        crate::install();
        let ty = api::get_static(&mut rt, "net/minecraft/world/level/block/entity/BlockEntityType.FURNACE");
        let pos = api::block_pos(&mut rt, 4, 64, -2);
        let state = api::state(&mut rt, "minecraft:furnace[facing=north,lit=false]");
        let be = rt.new_object("test/Diode");
        api::call(&mut rt, INIT, &[be, ty, pos, state]);
        // What the generated code reads for `this.level` and `this.remove`.
        assert_eq!(api::game_field(&mut rt, be, "level", "$level", LEVEL), V::NULL);
        assert_eq!(api::game_field(&mut rt, be, "remove", "$remove", REMOVED).i(), 0);
        assert_eq!(api::call(&mut rt, HAS_LEVEL, &[be]).i(), 0);
        let level = rt.new_object("net/minecraft/server/level/ServerLevel");
        api::call(&mut rt, SET_LEVEL, &[be, level]);
        assert_eq!(api::game_field(&mut rt, be, "level", "$level", LEVEL), level);
        assert_eq!(api::call(&mut rt, HAS_LEVEL, &[be]).i(), 1);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A source made from a seed draws as vanilla's legacy one (`java.util.Random`): the numbers a
    /// JDK's `new Random(42)` gives, worked out apart from the runtime. A fork and a positional
    /// factory make sources of their own seeds, as vanilla's.
    #[test]
    fn a_seeded_random_source_draws_as_vanillas_legacy_one() {
        use super::*;
        const CREATE: &str =
            "net/minecraft/util/RandomSource.create.(J)Lnet/minecraft/util/RandomSource;";
        const NEXT_INT: &str = "net/minecraft/util/RandomSource.nextInt.()I";
        const NEXT_BOUND: &str = "net/minecraft/util/RandomSource.nextInt.(I)I";
        let mut rt = Rt::new();
        crate::install();
        let c = |rt: &mut Rt, k: &'static str, args: &[V]| crate::api::call(rt, k, args);
        let r = c(&mut rt, CREATE, &[V::J(42)]);
        assert_eq!(
            (c(&mut rt, NEXT_INT, &[r]), c(&mut rt, NEXT_INT, &[r])),
            (V::I(-1_170_105_035), V::I(234_785_527))
        );
        // A bound that isn't a power of two, and one that is (drawn otherwise).
        let r = c(&mut rt, CREATE, &[V::J(42)]);
        let tens: Vec<i32> = (0..5)
            .map(|_| c(&mut rt, NEXT_BOUND, &[r, V::I(10)]).i())
            .collect();
        assert_eq!(tens, [0, 3, 8, 4, 0]);
        let r = c(&mut rt, CREATE, &[V::J(42)]);
        let sixteens: Vec<i32> = (0..5)
            .map(|_| c(&mut rt, NEXT_BOUND, &[r, V::I(16)]).i())
            .collect();
        assert_eq!(sixteens, [11, 0, 10, 0, 4]);
        let r = c(&mut rt, CREATE, &[V::J(42)]);
        assert_eq!(
            c(
                &mut rt,
                "net/minecraft/util/RandomSource.nextLong.()J",
                &[r]
            ),
            V::J(-5_025_562_857_975_149_833)
        );
        let r = c(&mut rt, CREATE, &[V::J(42)]);
        assert_eq!(
            c(
                &mut rt,
                "net/minecraft/util/RandomSource.nextDouble.()D",
                &[r]
            ),
            V::D(0.727_563_680_032_868_1)
        );
        let r = c(&mut rt, CREATE, &[V::J(42)]);
        assert_eq!(
            c(
                &mut rt,
                "net/minecraft/util/RandomSource.nextFloat.()F",
                &[r]
            ),
            V::F(12_206_493.0 / 16_777_216.0)
        );
        // Gaussians in pairs, the second kept for the next call.
        let r = c(&mut rt, CREATE, &[V::J(42)]);
        let gauss: Vec<f64> = (0..3)
            .map(|_| {
                c(
                    &mut rt,
                    "net/minecraft/util/RandomSource.nextGaussian.()D",
                    &[r],
                )
                .d()
            })
            .collect();
        for (got, want) in gauss.iter().zip([
            1.141_905_315_473_054_7,
            0.919_407_948_982_788,
            -0.949_866_636_890_895_9,
        ]) {
            assert!((got - want).abs() < 1e-12, "{gauss:?}");
        }
        let r = c(&mut rt, CREATE, &[V::J(42)]);
        let between: Vec<i32> = (0..6)
            .map(|_| {
                c(
                    &mut rt,
                    "net/minecraft/util/RandomSource.nextIntBetweenInclusive.(II)I",
                    &[r, V::I(3), V::I(7)],
                )
                .i()
            })
            .collect();
        assert_eq!(between, [3, 6, 6, 7, 3, 3]);
        let r = c(&mut rt, CREATE, &[V::J(42)]);
        let t = c(
            &mut rt,
            "net/minecraft/util/RandomSource.triangle.(DD)D",
            &[r, V::D(10.0), V::D(2.0)],
        )
        .d();
        assert!((t - 10.088_680_416_546_046).abs() < 1e-12, "{t}");
        // `fork()`: a source seeded with the next long.
        let r = c(&mut rt, CREATE, &[V::J(42)]);
        let fork = c(
            &mut rt,
            "net/minecraft/util/RandomSource.fork.()Lnet/minecraft/util/RandomSource;",
            &[r],
        );
        assert_eq!(c(&mut rt, NEXT_INT, &[fork]), V::I(2_040_010_338));
        // `forkPositional()`: by position (`Mth.getSeed`) and by a name's hash.
        let r = c(&mut rt, CREATE, &[V::J(42)]);
        let f = c(
            &mut rt,
            "net/minecraft/util/RandomSource.forkPositional.()Lnet/minecraft/world/level/levelgen/PositionalRandomFactory;",
            &[r],
        );
        let at = c(
            &mut rt,
            "net/minecraft/world/level/levelgen/PositionalRandomFactory.at.(III)Lnet/minecraft/util/RandomSource;",
            &[f, V::I(1), V::I(2), V::I(3)],
        );
        assert_eq!(c(&mut rt, NEXT_BOUND, &[at, V::I(100)]), V::I(38));
        let stone = rt.string("minecraft:stone");
        let named = c(
            &mut rt,
            "net/minecraft/world/level/levelgen/PositionalRandomFactory.fromHashOf.(Ljava/lang/String;)Lnet/minecraft/util/RandomSource;",
            &[f, stone],
        );
        assert_eq!(c(&mut rt, NEXT_BOUND, &[named, V::I(100)]), V::I(99));
        // `new LegacyRandomSource(42)`, and a source without a seed of its own given one.
        let legacy = rt.new_object("net/minecraft/world/level/levelgen/LegacyRandomSource");
        c(
            &mut rt,
            "net/minecraft/world/level/levelgen/LegacyRandomSource.<init>.(J)V",
            &[legacy, V::J(42)],
        );
        assert_eq!(
            c(
                &mut rt,
                "net/minecraft/world/level/levelgen/LegacyRandomSource.nextInt.()I",
                &[legacy]
            ),
            V::I(-1_170_105_035)
        );
        let level_random = rt.new_object("net/minecraft/util/RandomSource");
        c(
            &mut rt,
            "net/minecraft/util/RandomSource.setSeed.(J)V",
            &[level_random, V::J(42)],
        );
        assert_eq!(c(&mut rt, NEXT_INT, &[level_random]), V::I(-1_170_105_035));
        // `Direction.getRandom`, `Util.getRandom` and `Util.shuffle` draw from the source given.
        let r = c(&mut rt, CREATE, &[V::J(42)]);
        let d = c(
            &mut rt,
            "net/minecraft/core/Direction.getRandom.(Lnet/minecraft/util/RandomSource;)Lnet/minecraft/core/Direction;",
            &[r],
        );
        assert_eq!(d, direction(&mut rt, 2), "north");
        let items: Vec<V> = (1..=5).map(V::I).collect();
        let r = c(&mut rt, CREATE, &[V::J(42)]);
        let three = crate::jutil::new_list(&mut rt, items[..3].to_vec());
        assert_eq!(
            c(
                &mut rt,
                "net/minecraft/Util.getRandom.(Ljava/util/List;Lnet/minecraft/util/RandomSource;)Ljava/lang/Object;",
                &[three, r]
            ),
            V::I(3)
        );
        let r = c(&mut rt, CREATE, &[V::J(42)]);
        let five = crate::jutil::new_list(&mut rt, items.clone());
        c(
            &mut rt,
            "net/minecraft/Util.shuffle.(Ljava/util/List;Lnet/minecraft/util/RandomSource;)V",
            &[five, r],
        );
        assert_eq!(
            crate::jutil::elements(&rt, five),
            [V::I(2), V::I(3), V::I(4), V::I(5), V::I(1)]
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// An entity's random source is its own and stays the same (a mod's particles and drops draw
    /// from it); a player's too, whichever class names the call.
    #[test]
    fn an_entity_keeps_its_own_random_source() {
        let mut rt = Rt::new();
        crate::install();
        let player = rt.new_object("net/minecraft/server/level/ServerPlayer");
        let first = api::call(
            &mut rt,
            "net/minecraft/world/entity/LivingEntity.getRandom.()Lnet/minecraft/util/RandomSource;",
            &[player],
        );
        let again = api::call(
            &mut rt,
            "net/minecraft/world/entity/player/Player.getRandom.()Lnet/minecraft/util/RandomSource;",
            &[player],
        );
        assert!(first.r() != 0 && first == again);
        let other = rt.new_object("net/minecraft/server/level/ServerPlayer");
        assert_ne!(
            api::call(
                &mut rt,
                "net/minecraft/world/entity/Entity.getRandom.()Lnet/minecraft/util/RandomSource;",
                &[other]
            ),
            first
        );
        let roll = api::call(
            &mut rt,
            "net/minecraft/util/RandomSource.nextInt.(I)I",
            &[first, V::I(4)],
        )
        .i();
        assert!((0..4).contains(&roll));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Vanilla's `Util`: the platform the game runs on, one shared pool that runs what it is given,
    /// and a cursor moved by code points (an emoji's surrogate pair is one).
    #[test]
    fn util_answers_the_platform_its_pools_and_code_points() {
        let mut rt = Rt::new();
        crate::install();
        let os = api::call(
            &mut rt,
            "net/minecraft/Util.getPlatform.()Lnet/minecraft/Util$OS;",
            &[],
        );
        let want = if cfg!(target_os = "macos") {
            "net/minecraft/Util$OS.OSX"
        } else if cfg!(target_os = "windows") {
            "net/minecraft/Util$OS.WINDOWS"
        } else if cfg!(target_os = "linux") {
            "net/minecraft/Util$OS.LINUX"
        } else {
            "net/minecraft/Util$OS.UNKNOWN"
        };
        assert!(os.r() != 0);
        assert_eq!(os, api::get_static(&mut rt, want));
        let pool = api::call(
            &mut rt,
            "net/minecraft/Util.backgroundExecutor.()Ljava/util/concurrent/ExecutorService;",
            &[],
        );
        assert!(pool.r() != 0);
        assert_eq!(
            api::call(
                &mut rt,
                "net/minecraft/Util.backgroundExecutor.()Ljava/util/concurrent/ExecutorService;",
                &[]
            ),
            pool
        );
        let text = rt.string("a\u{1F600}b");
        let offset = |rt: &mut Rt, cursor: i32, by: i32| {
            api::call(
                rt,
                "net/minecraft/Util.offsetByCodepoints.(Ljava/lang/String;II)I",
                &[text, V::I(cursor), V::I(by)],
            )
            .i()
        };
        assert_eq!(offset(&mut rt, 0, 2), 3, "past the pair at once");
        assert_eq!(offset(&mut rt, 4, -2), 1);
        assert_eq!(offset(&mut rt, 1, 9), 4, "stops at the end");
        assert_eq!(offset(&mut rt, 3, -9), 0, "and at the start");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `throw new ReportedException(CrashReport.forThrowable(e, title))`: the exception says the
    /// report's title and has `e` as its cause; a report made again from it is the same report.
    #[test]
    fn a_reported_exception_carries_its_crash_report() {
        let mut rt = Rt::new();
        crate::install();
        let cause = rt.new_object("java/lang/IllegalStateException");
        let why = rt.string("bad state");
        api::call(
            &mut rt,
            "java/lang/IllegalStateException.<init>.(Ljava/lang/String;)V",
            &[cause, why],
        );
        let title = rt.string("Ticking block entity");
        let report = api::call(
            &mut rt,
            "net/minecraft/CrashReport.forThrowable.(Ljava/lang/Throwable;Ljava/lang/String;)Lnet/minecraft/CrashReport;",
            &[cause, title],
        );
        let block = rt.string("Block entity details");
        let category = api::call(
            &mut rt,
            "net/minecraft/CrashReport.addCategory.(Ljava/lang/String;)Lnet/minecraft/CrashReportCategory;",
            &[report, block],
        );
        let (key, value) = (rt.string("Name"), rt.string("create:belt"));
        let detail = "net/minecraft/CrashReportCategory.setDetail.(Ljava/lang/String;Ljava/lang/Object;)Lnet/minecraft/CrashReportCategory;";
        assert_eq!(
            api::call(&mut rt, detail, &[category, key, value]),
            category
        );
        let thrown = rt.new_object("net/minecraft/ReportedException");
        api::call(
            &mut rt,
            "net/minecraft/ReportedException.<init>.(Lnet/minecraft/CrashReport;)V",
            &[thrown, report],
        );
        let message = api::call(
            &mut rt,
            "net/minecraft/ReportedException.getMessage.()Ljava/lang/String;",
            &[thrown],
        );
        assert_eq!(rt.str_of(message), Some("Ticking block entity"));
        assert_eq!(
            api::call(
                &mut rt,
                "net/minecraft/ReportedException.getCause.()Ljava/lang/Throwable;",
                &[thrown]
            ),
            cause
        );
        assert_eq!(
            api::call(
                &mut rt,
                "net/minecraft/ReportedException.getReport.()Lnet/minecraft/CrashReport;",
                &[thrown]
            ),
            report
        );
        let again = api::call(
            &mut rt,
            "net/minecraft/CrashReport.forThrowable.(Ljava/lang/Throwable;Ljava/lang/String;)Lnet/minecraft/CrashReport;",
            &[thrown, block],
        );
        assert_eq!(again, report);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A chunk the level gives a mod: where it is, the world's height in sections, its fluids and
    /// the mods' block entities in it; the server's levels are the one the hook runs in.
    #[test]
    fn a_chunk_knows_its_place_and_the_worlds_height() {
        let mut rt = Rt::new();
        crate::install();
        let level = crate::glue::level(&mut rt);
        let pos = api::block_pos(&mut rt, -20, 70, 35);
        let chunk = api::call(
            &mut rt,
            "net/minecraft/world/level/Level.getChunkAt.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/chunk/LevelChunk;",
            &[level, pos],
        );
        let at = api::call(
            &mut rt,
            "net/minecraft/world/level/chunk/LevelChunk.getPos.()Lnet/minecraft/world/level/ChunkPos;",
            &[chunk],
        );
        assert_eq!(
            (rt.get_field(at, "x"), rt.get_field(at, "z")),
            (V::I(-2), V::I(2))
        );
        let ask = |rt: &mut Rt, k: &'static str, arg: Option<i32>| {
            let mut args = vec![chunk];
            args.extend(arg.map(V::I));
            api::call(rt, k, &args).i()
        };
        // Vanilla's overworld, the default where no host tells: y from -64, 384 high.
        assert_eq!(
            ask(
                &mut rt,
                "net/minecraft/world/level/chunk/ChunkAccess.getSectionsCount.()I",
                None
            ),
            24
        );
        assert_eq!(
            ask(
                &mut rt,
                "net/minecraft/world/level/chunk/LevelChunk.getMinSection.()I",
                None
            ),
            -4
        );
        assert_eq!(
            ask(
                &mut rt,
                "net/minecraft/world/level/chunk/LevelChunk.getSectionIndex.(I)I",
                Some(0)
            ),
            4
        );
        assert_eq!(
            ask(
                &mut rt,
                "net/minecraft/world/level/chunk/LevelChunk.getSectionIndex.(I)I",
                Some(-64)
            ),
            0
        );
        assert_eq!(
            ask(
                &mut rt,
                "net/minecraft/world/level/chunk/LevelChunk.getSectionYFromSectionIndex.(I)I",
                Some(23)
            ),
            19
        );
        assert_eq!(
            ask(
                &mut rt,
                "net/minecraft/world/level/chunk/LevelChunk.isOutsideBuildHeight.(I)Z",
                Some(320)
            ),
            1
        );
        assert_eq!(
            ask(
                &mut rt,
                "net/minecraft/world/level/chunk/LevelChunk.isOutsideBuildHeight.(I)Z",
                Some(319)
            ),
            0
        );
        api::call(
            &mut rt,
            "net/minecraft/world/level/chunk/LevelChunk.setUnsaved.(Z)V",
            &[chunk, V::I(1)],
        );
        let levels = api::call(
            &mut rt,
            "net/minecraft/server/MinecraftServer.getAllLevels.()Ljava/lang/Iterable;",
            &[V::NULL],
        );
        assert_eq!(crate::jutil::elements(&rt, levels), [level]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A direction's step and rotation, and the lookups by delta, name and nearest vector, as
    /// vanilla's `Direction` gives them; an axis's plane and name.
    #[test]
    fn directions_step_turn_and_are_found_as_vanilla_does() {
        use super::*;
        let mut rt = Rt::new();
        crate::install();
        let c = |rt: &mut Rt, k: &'static str, args: &[V]| crate::api::call(rt, k, args);
        let floats = |rt: &Rt, v: V, fields: &[&'static str]| {
            fields
                .iter()
                .map(|f| rt.get_field(v, f).f())
                .collect::<Vec<f32>>()
        };
        let west = direction(&mut rt, 4);
        let step = c(
            &mut rt,
            "net/minecraft/core/Direction.step.()Lorg/joml/Vector3f;",
            &[west],
        );
        assert_eq!(
            (rt.class_of(step), floats(&rt, step, &["x", "y", "z"])),
            ("org/joml/Vector3f", vec![-1.0, 0.0, 0.0])
        );

        // `getRotation()` turns up onto each direction's normal: v + 2w(q × v) + 2q × (q × v).
        let cross = |a: [f32; 3], b: [f32; 3]| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        for (o, normal) in DIRECTION_STEPS.iter().enumerate() {
            let d = direction(&mut rt, o as i64);
            let q = c(
                &mut rt,
                "net/minecraft/core/Direction.getRotation.()Lorg/joml/Quaternionf;",
                &[d],
            );
            let [x, y, z, w] = floats(&rt, q, &["x", "y", "z", "w"])[..] else {
                panic!("four floats")
            };
            let up = [0.0, 1.0, 0.0];
            let t = cross([x, y, z], up).map(|v| 2.0 * v);
            let u = cross([x, y, z], t);
            let turned = [0, 1, 2].map(|i| up[i] + w * t[i] + u[i]);
            for i in 0..3 {
                assert!(
                    (turned[i] - normal[i] as f32).abs() < 1e-6,
                    "{o}: {turned:?}"
                );
            }
        }
        // JOML's own numbers: up is no turn, south a quarter turn about x.
        let (up, south) = (direction(&mut rt, 1), direction(&mut rt, 3));
        let q = |rt: &mut Rt, d: V| {
            let q = c(
                rt,
                "net/minecraft/core/Direction.getRotation.()Lorg/joml/Quaternionf;",
                &[d],
            );
            floats(rt, q, &["x", "y", "z", "w"])
        };
        assert_eq!(q(&mut rt, up), vec![0.0, 0.0, 0.0, 1.0]);
        assert_eq!(q(&mut rt, south), vec![0.70710677, 0.0, 0.0, 0.70710677]);

        // `fromDelta`: one axis only, by its sign.
        let from = |rt: &mut Rt, [x, y, z]: [i32; 3]| {
            c(
                rt,
                "net/minecraft/core/Direction.fromDelta.(III)Lnet/minecraft/core/Direction;",
                &[V::I(x), V::I(y), V::I(z)],
            )
        };
        for (delta, o) in [
            ([0, 0, 5], 3),
            ([0, -3, 0], 0),
            ([2, 0, 0], 5),
            ([-1, 0, 0], 4),
        ] {
            let want = direction(&mut rt, o);
            assert_eq!(from(&mut rt, delta), want, "{delta:?}");
        }
        assert_eq!(
            (from(&mut rt, [1, 1, 0]), from(&mut rt, [0, 0, 0])),
            (V::NULL, V::NULL)
        );

        // `byName`: the serialized name, nothing else.
        let by_name = |rt: &mut Rt, n: Option<&str>| {
            let n = n.map_or(V::NULL, |n| rt.string(n));
            c(
                rt,
                "net/minecraft/core/Direction.byName.(Ljava/lang/String;)Lnet/minecraft/core/Direction;",
                &[n],
            )
        };
        let north = direction(&mut rt, 2);
        assert_eq!(by_name(&mut rt, Some("north")), north);
        assert_eq!(
            (by_name(&mut rt, Some("NORTH")), by_name(&mut rt, None)),
            (V::NULL, V::NULL)
        );

        // `getNearest(vec)`: the largest dot product.
        let v = vec3(&mut rt, 0.2, -0.9, 0.1);
        let nearest = c(
            &mut rt,
            "net/minecraft/core/Direction.getNearest.(Lnet/minecraft/world/phys/Vec3;)Lnet/minecraft/core/Direction;",
            &[v],
        );
        assert_eq!(nearest, direction(&mut rt, 0));

        // An axis's plane and name.
        let (x_axis, y_axis) = (axis(&mut rt, 0), axis(&mut rt, 1));
        let plane = |rt: &mut Rt, a: V| {
            c(
                rt,
                "net/minecraft/core/Direction$Axis.getPlane.()Lnet/minecraft/core/Direction$Plane;",
                &[a],
            )
        };
        let (horizontal, vertical) = (
            crate::api::get_static(&mut rt, "net/minecraft/core/Direction$Plane.HORIZONTAL"),
            crate::api::get_static(&mut rt, "net/minecraft/core/Direction$Plane.VERTICAL"),
        );
        assert_eq!(
            (plane(&mut rt, x_axis), plane(&mut rt, y_axis)),
            (horizontal, vertical)
        );
        let z = rt.string("z");
        let z_axis = c(
            &mut rt,
            "net/minecraft/core/Direction$Axis.byName.(Ljava/lang/String;)Lnet/minecraft/core/Direction$Axis;",
            &[z],
        );
        assert_eq!(z_axis, axis(&mut rt, 2));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A dye item's colour: a vanilla dye's by its id, a mod's own by its constructor, none for an
    /// item that isn't one; a colour's `c:dyes` item tag.
    #[test]
    fn a_dye_item_knows_its_colour_and_a_colour_its_tag() {
        use super::*;
        let mut rt = Rt::new();
        crate::install();
        let c = |rt: &mut Rt, k: &'static str, args: &[V]| crate::api::call(rt, k, args);
        const COLOUR: &str =
            "net/minecraft/world/item/DyeItem.getDyeColor.()Lnet/minecraft/world/item/DyeColor;";
        let (red, blue) = (
            crate::api::get_static(&mut rt, "net/minecraft/world/item/DyeColor.RED"),
            crate::api::get_static(&mut rt, "net/minecraft/world/item/DyeColor.BLUE"),
        );
        let red_dye = item(&mut rt, "minecraft:red_dye");
        assert_eq!(c(&mut rt, COLOUR, &[red_dye]), red);
        let glow = rt.new_object("t/GlowDyeItem");
        c(
            &mut rt,
            "net/minecraft/world/item/DyeItem.<init>.(Lnet/minecraft/world/item/DyeColor;Lnet/minecraft/world/item/Item$Properties;)V",
            &[glow, blue, V::NULL],
        );
        assert_eq!(c(&mut rt, COLOUR, &[glow]), blue);
        let stick = item(&mut rt, "minecraft:stick");
        assert_eq!(c(&mut rt, COLOUR, &[stick]), V::NULL);

        let light_blue =
            crate::api::get_static(&mut rt, "net/minecraft/world/item/DyeColor.LIGHT_BLUE");
        let tag = c(
            &mut rt,
            "net/minecraft/world/item/DyeColor.getTag.()Lnet/minecraft/tags/TagKey;",
            &[light_blue],
        );
        let id = rt.string("c:dyes/light_blue");
        let location = c(
            &mut rt,
            "net/minecraft/resources/ResourceLocation.parse.(Ljava/lang/String;)Lnet/minecraft/resources/ResourceLocation;",
            &[id],
        );
        let same = c(
            &mut rt,
            "net/minecraft/tags/ItemTags.create.(Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/tags/TagKey;",
            &[location],
        );
        assert_eq!(tag, same);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A component's contents: a translation's key and arguments, or literal text, the same object
    /// each time and of the classes a mod tests for; a component made from contents.
    #[test]
    fn a_components_contents_are_its_translation_or_text() {
        use super::*;
        let mut rt = Rt::new();
        crate::install();
        let c = |rt: &mut Rt, k: &'static str, args: &[V]| crate::api::call(rt, k, args);
        const CONTENTS: &str = "net/minecraft/network/chat/Component.getContents.()Lnet/minecraft/network/chat/ComponentContents;";
        const GET_KEY: &str =
            "net/minecraft/network/chat/contents/TranslatableContents.getKey.()Ljava/lang/String;";
        const GET_ARGS: &str = "net/minecraft/network/chat/contents/TranslatableContents.getArgs.()[Ljava/lang/Object;";
        const TEXT: &str =
            "net/minecraft/network/chat/contents/PlainTextContents.text.()Ljava/lang/String;";
        const CREATE: &str = "net/minecraft/network/chat/MutableComponent.create.(Lnet/minecraft/network/chat/ComponentContents;)Lnet/minecraft/network/chat/MutableComponent;";
        let is = |rt: &Rt, v: V| {
            [
                "net/minecraft/network/chat/ComponentContents",
                TRANSLATABLE_CONTENTS,
                PLAIN_TEXT_CONTENTS,
            ]
            .map(|class| rt.instance_of(v, class))
        };

        let key = rt.string("block.minecraft.stone");
        let five = integer(&mut rt, 5);
        let args = rt.alloc("[Ljava/lang/Object;", Data::Array(vec![five]));
        let t = c(
            &mut rt,
            "net/minecraft/network/chat/Component.translatable.(Ljava/lang/String;[Ljava/lang/Object;)Lnet/minecraft/network/chat/MutableComponent;",
            &[key, args],
        );
        let contents = c(&mut rt, CONTENTS, &[t]);
        assert_eq!(is(&rt, contents), [true, true, false]);
        let got = c(&mut rt, GET_KEY, &[contents]);
        assert_eq!(rt.str_of(got), Some("block.minecraft.stone"));
        let got = c(&mut rt, GET_ARGS, &[contents]);
        assert_eq!((rt.array_len(got), rt.array_get(got, 0)), (1, five));
        assert_eq!(c(&mut rt, CONTENTS, &[t]), contents);

        let hi = rt.string("Hi");
        let literal = c(
            &mut rt,
            "net/minecraft/network/chat/Component.literal.(Ljava/lang/String;)Lnet/minecraft/network/chat/MutableComponent;",
            &[hi],
        );
        let contents = c(&mut rt, CONTENTS, &[literal]);
        assert_eq!(is(&rt, contents), [true, false, true]);
        let got = c(&mut rt, TEXT, &[contents]);
        assert_eq!(rt.str_of(got), Some("Hi"));

        // `MutableComponent.create(new TranslatableContents("gui.done", null, NO_ARGS))`: that
        // translation, with no arguments, and those very contents.
        let done = rt.string("gui.done");
        let no_args = rt.alloc("[Ljava/lang/Object;", Data::Array(Vec::new()));
        let tc = rt.new_object(TRANSLATABLE_CONTENTS);
        c(
            &mut rt,
            "net/minecraft/network/chat/contents/TranslatableContents.<init>.(Ljava/lang/String;Ljava/lang/String;[Ljava/lang/Object;)V",
            &[tc, done, V::NULL, no_args],
        );
        let made = c(&mut rt, CREATE, &[tc]);
        assert_eq!(
            component_json(&rt, made),
            serde_json::json!({ "translate": "gui.done" })
        );
        assert_eq!(c(&mut rt, CONTENTS, &[made]), tc);
        let got = c(&mut rt, GET_ARGS, &[tc]);
        assert_eq!(rt.array_len(got), 0);
        let text = c(
            &mut rt,
            "net/minecraft/network/chat/contents/PlainTextContents.create.(Ljava/lang/String;)Lnet/minecraft/network/chat/contents/PlainTextContents;",
            &[hi],
        );
        let made = c(&mut rt, CREATE, &[text]);
        assert_eq!(
            component_json(&rt, made),
            serde_json::json!({ "text": "Hi" })
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `WidgetSprites`' shorter constructors repeat sprites as vanilla's do, and `get` picks the
    /// sprite of a button's state.
    #[test]
    fn a_widgets_sprites_follow_its_state() {
        use super::*;
        let mut rt = Rt::new();
        crate::install();
        const GET: &str = "net/minecraft/client/gui/components/WidgetSprites.get.(ZZ)Lnet/minecraft/resources/ResourceLocation;";
        let [plain, off, lit] = ["test:plain", "test:off", "test:lit"].map(|s| rt.string(s));
        let make = |rt: &mut Rt, desc: &str, sprites: &[V]| {
            let w = rt.new_object(WIDGET_SPRITES);
            let key = rt.static_key(&format!("{WIDGET_SPRITES}.<init>.{desc}"));
            crate::api::call(rt, key, &[&[w], sprites].concat());
            w
        };
        let states = |rt: &mut Rt, w: V| {
            [(1, 0), (1, 1), (0, 0), (0, 1)].map(|(enabled, focused)| {
                crate::api::call(rt, GET, &[w, V::I(enabled), V::I(focused)])
            })
        };
        let two = make(&mut rt, WIDGET_SPRITES_TWO, &[plain, lit]);
        assert_eq!(states(&mut rt, two), [plain, lit, plain, lit]);
        let three = make(&mut rt, WIDGET_SPRITES_THREE, &[plain, off, lit]);
        assert_eq!(states(&mut rt, three), [plain, lit, off, off]);
        // The record's own accessor reads the component the constructor filled.
        let got = crate::api::call(
            &mut rt,
            "net/minecraft/client/gui/components/WidgetSprites.disabledFocused.()Lnet/minecraft/resources/ResourceLocation;",
            &[three],
        );
        assert_eq!(got, off);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// An item use consumes from the stack, except a creative player's (infinite materials); a
    /// mob's or no one's always does. A player's secondary use is its shift key.
    #[test]
    fn consuming_spares_only_a_creative_players_stack() {
        use super::*;
        let mut rt = Rt::new();
        crate::install();
        const CONSUME: &str = "net/minecraft/world/item/ItemStack.consume.(ILnet/minecraft/world/entity/LivingEntity;)V";
        const AND_RETURN: &str = "net/minecraft/world/item/ItemStack.consumeAndReturn.(ILnet/minecraft/world/entity/LivingEntity;)Lnet/minecraft/world/item/ItemStack;";
        const SECONDARY: &str = "net/minecraft/world/entity/player/Player.isSecondaryUseActive.()Z";
        let count = |rt: &Rt, s: V| rt.get_field(s, "$count").i();
        let stone = item(&mut rt, "minecraft:stone");
        let stack = stack_of(&mut rt, stone, 8);
        let mob = rt.new_object("net/minecraft/world/entity/animal/Pig");
        let mut host = crate::host::MemoryHost {
            creative: true,
            sneaking: true,
            ..Default::default()
        };
        let player = rt.with_host(&mut host, |rt| {
            let player = crate::menus::server_player(rt, 7);
            crate::api::call(rt, CONSUME, &[stack, V::I(2), player]);
            assert_eq!(count(rt, stack), 8);
            crate::api::call(rt, CONSUME, &[stack, V::I(2), mob]);
            assert_eq!(count(rt, stack), 6);
            crate::api::call(rt, CONSUME, &[stack, V::I(1), V::NULL]);
            assert_eq!(count(rt, stack), 5);
            let back = crate::api::call(rt, AND_RETURN, &[stack, V::I(3), player]);
            assert_eq!((count(rt, back), count(rt, stack)), (3, 5));
            assert_eq!(crate::api::call(rt, SECONDARY, &[player]), V::I(1));
            player
        });
        host.creative = false;
        host.sneaking = false;
        rt.with_host(&mut host, |rt| {
            let back = crate::api::call(rt, AND_RETURN, &[stack, V::I(3), player]);
            assert_eq!((count(rt, back), count(rt, stack)), (3, 2));
            assert_eq!(rt.get_field(back, "$item"), stone);
            assert_eq!(crate::api::call(rt, SECONDARY, &[player]), V::I(0));
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A hover event keeps its action and value, gives the value only for its own action, and
    /// equals another of both; an item's info is the stack as it was; an entity's tooltip is its
    /// name, its type and its UUID.
    #[test]
    fn hover_events_keep_what_they_show() {
        use super::*;
        let mut rt = Rt::new();
        crate::install();
        const NEW: &str = "net/minecraft/network/chat/HoverEvent.<init>.(Lnet/minecraft/network/chat/HoverEvent$Action;Ljava/lang/Object;)V";
        const VALUE: &str = "net/minecraft/network/chat/HoverEvent.getValue.(Lnet/minecraft/network/chat/HoverEvent$Action;)Ljava/lang/Object;";
        const EQUALS: &str = "net/minecraft/network/chat/HoverEvent.equals.(Ljava/lang/Object;)Z";
        let [text, item, entity] = ["SHOW_TEXT", "SHOW_ITEM", "SHOW_ENTITY"]
            .map(|f| get_static(&mut rt, HOVER_ACTION, f).unwrap());
        let hi = rt.string("Hi");
        let shown = component(&mut rt, hi, false, V::NULL);
        let event = |rt: &mut Rt, action: V, value: V| {
            let e = rt.new_object(HOVER_EVENT);
            crate::api::call(rt, NEW, &[e, action, value]);
            e
        };
        let e = event(&mut rt, text, shown);
        assert_eq!(crate::api::call(&mut rt, VALUE, &[e, text]), shown);
        assert_eq!(crate::api::call(&mut rt, VALUE, &[e, item]), V::NULL);
        assert_eq!(
            crate::api::call(
                &mut rt,
                "net/minecraft/network/chat/HoverEvent.getAction.()Lnet/minecraft/network/chat/HoverEvent$Action;",
                &[e]
            ),
            text
        );
        let name = crate::api::call(
            &mut rt,
            "net/minecraft/network/chat/HoverEvent$Action.getSerializedName.()Ljava/lang/String;",
            &[entity],
        );
        assert_eq!(rt.str_of(name), Some("show_entity"));
        let same = event(&mut rt, text, shown);
        let other = event(&mut rt, item, shown);
        assert_eq!(
            [same, other].map(|o| crate::api::call(&mut rt, EQUALS, &[e, o])),
            [V::I(1), V::I(0)]
        );

        // An item's info is the stack when it was made, not what the stack becomes.
        let stone = stack(&mut rt, "minecraft:stone", 5);
        let info = rt.new_object(ITEM_STACK_INFO);
        crate::api::call(
            &mut rt,
            "net/minecraft/network/chat/HoverEvent$ItemStackInfo.<init>.(Lnet/minecraft/world/item/ItemStack;)V",
            &[info, stone],
        );
        rt.put_field(stone, "$count", V::I(1));
        let got = crate::api::call(
            &mut rt,
            "net/minecraft/network/chat/HoverEvent$ItemStackInfo.getItemStack.()Lnet/minecraft/world/item/ItemStack;",
            &[info],
        );
        assert_eq!(rt.get_field(got, "$count"), V::I(5));
        assert_eq!(
            item_like_name(&rt, rt.get_field(got, "$item")),
            "minecraft:stone"
        );

        // An entity's: its name, its type's description, its UUID.
        let zombie =
            crate::api::get_static(&mut rt, "net/minecraft/world/entity/EntityType.ZOMBIE");
        let uuid = crate::tags::uuid(&mut rt, 1, 2);
        let named = rt.string("Bob");
        let bob = component(&mut rt, named, false, V::NULL);
        let tip = rt.new_object(ENTITY_TOOLTIP_INFO);
        crate::api::call(
            &mut rt,
            "net/minecraft/network/chat/HoverEvent$EntityTooltipInfo.<init>.(Lnet/minecraft/world/entity/EntityType;Ljava/util/UUID;Lnet/minecraft/network/chat/Component;)V",
            &[tip, zombie, uuid, bob],
        );
        let lines = crate::api::call(
            &mut rt,
            "net/minecraft/network/chat/HoverEvent$EntityTooltipInfo.getTooltipLines.()Ljava/util/List;",
            &[tip],
        );
        let json: Vec<serde_json::Value> = crate::jutil::elements(&rt, lines)
            .into_iter()
            .map(|c| component_json(&rt, c))
            .collect();
        assert_eq!(json[0], serde_json::json!({ "text": "Bob" }));
        assert_eq!(json[1]["translate"], "gui.entity_tooltip.type");
        assert_eq!(json[1]["with"][0]["translate"], "entity.minecraft.zombie");
        assert_eq!(
            json[2],
            serde_json::json!({ "text": "00000000-0000-0001-0000-000000000002" })
        );
        assert_eq!(json.len(), 3);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
