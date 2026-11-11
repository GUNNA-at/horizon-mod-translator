//! `horizon-mod-translator coverage <jar>...`: the game and loader API each
//! jar calls, most used first, and how much of it Horizon's compat layer maps.
use std::{path::Path, process::ExitCode};

/// `translate <jar> <out dir>`: the jar's code as a Rust crate in `<out dir>`,
/// built against horizon-compat.
fn translate(jar: &Path, out: &Path) -> Result<(), String> {
    let classes = horizon_mod_translator::classes(jar)?;
    let mut program = horizon_mod_translator::codegen::Program::new(&classes);
    program.services = horizon_mod_translator::services(jar)?;
    let files = program.emit_files(40);
    let lines: usize = files.iter().map(|(_, f)| f.lines().count()).sum();
    let src = out.join("src");
    std::fs::create_dir_all(&src).map_err(|e| e.to_string())?;
    let compat = Path::new(env!("CARGO_MANIFEST_DIR")).join("../horizon-compat");
    let compat = compat.canonicalize().map_err(|e| e.to_string())?;
    let name = jar.file_stem().unwrap_or_default().to_string_lossy().replace(|c: char| !c.is_ascii_alphanumeric(), "_");
    std::fs::write(
        out.join("Cargo.toml"),
        format!("[package]\nname = \"translated_{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[workspace]\n[dependencies]\nhorizon-compat = {{ path = {compat:?} }}\n"),
    )
    .map_err(|e| e.to_string())?;
    for (name, text) in &files {
        std::fs::write(src.join(name), text).map_err(|e| e.to_string())?;
    }
    println!("{}: {} classes, {lines} lines of Rust in {} files", jar.display(), classes.len(), files.len());
    for (what, n) in &program.unsupported {
        println!("  unsupported: {what} ({n})");
    }
    Ok(())
}

/// The receiver a coverage probe calls an API method with.
#[derive(Clone, Copy)]
enum Receiver {
    /// Null: a static method's, or one answered whatever the receiver.
    Null,
    /// A library class's object as its constructor with no arguments makes it (`new ArrayList()`).
    Init,
    /// A bare object of the owner's class.
    Bare,
    /// The probe's server player, the player in place ([`probe_host`]).
    Player,
    /// The probe's cow, a living entity of the level.
    Mob,
    /// The runtime's level (`glue::level`), the one a mod's code is given.
    Level,
    /// An API class's static field (`owner.NAME`) the mod reads.
    Static(&'static str),
    /// A new object of the class, its constructor (the key, with its argument count) run.
    New(&'static str, usize),
    /// What an API method (the key, with its argument count) returns.
    Returned(&'static str, usize),
    /// A value of the owner's own type as a game gives one ([`sample`]: stone's state for
    /// `BlockState`), else a bare object of it.
    Sample,
    /// The owner's second value where it has one ([`alternate`]: a lever's state for a state's
    /// methods, which holds the property the property sample is).
    Alternate,
}

/// A second value of class `owner` for the probe where its [`sample`] can't take a call: a state
/// with properties (a lever's, `powered` among them) for a state's methods (`cycle`, `setValue`).
fn alternate(rt: &mut horizon_compat::Rt, owner: &str) -> Option<horizon_compat::V> {
    has_alternate(owner).then(|| {
        horizon_compat::api::state(rt, "minecraft:lever[face=floor,facing=north,powered=false]")
    })
}

/// Whether class `owner` has an [`alternate`] value.
fn has_alternate(owner: &str) -> bool {
    matches!(
        owner,
        "net/minecraft/world/level/block/state/BlockState"
            | "net/minecraft/world/level/block/state/BlockBehaviour$BlockStateBase"
            | "net/minecraft/world/level/block/state/StateHolder"
    )
}

/// How a probe call's arguments are filled: all null, or each with a value of its declared type
/// ([`sample`]), read as an instance method's (after the receiver) or a static one's (from the
/// first).
#[derive(Clone, Copy)]
enum Args {
    Null,
    Instance,
    Static,
}

/// The parameter types of method descriptor `desc`, as field descriptors (`I`,
/// `Lnet/minecraft/core/BlockPos;`, `[I`).
fn param_types(desc: &str) -> Vec<&str> {
    let inner = &desc[1..desc.find(')').unwrap_or(1)];
    let (b, mut i, mut out) = (inner.as_bytes(), 0, Vec::new());
    while i < b.len() {
        let start = i;
        while i < b.len() && b[i] == b'[' {
            i += 1;
        }
        if i < b.len() && b[i] == b'L' {
            i += inner[i..].find(';').map_or(inner.len() - i, |e| e + 1);
        } else {
            i += 1;
        }
        out.push(&inner[start..i.min(b.len())]);
    }
    out
}

/// A value of type `ty` (a field descriptor) such as a mod's call gets in a game: stone (its state,
/// block, item, a stack of it, its id), the level's origin, up, the main hand, an empty tag, the
/// probe's level, player and cow, a container of one slot and its slot, an item handler and a
/// fluid tank, NeoForge's item and fluid capabilities, the codec builder's instance and groups of
/// fields, stone's item's registry holder, the `powered` property, a stream codec and the list
/// operation on one, a random source, an empty list; a zero for a primitive. None for a type with
/// no sample (null).
fn sample(rt: &mut horizon_compat::Rt, ty: &str) -> Option<horizon_compat::V> {
    use horizon_compat::{V, api};
    let class = ty.strip_prefix('L').and_then(|t| t.strip_suffix(';'));
    Some(match ty {
        "Z" | "B" | "C" | "S" | "I" => V::I(0),
        "J" => V::J(0),
        "F" => V::F(0.0),
        "D" => V::D(0.0),
        "Ljava/lang/String;" | "Ljava/lang/CharSequence;" => rt.string("stone"),
        "Lnet/minecraft/world/level/block/state/BlockState;" | "Lnet/minecraft/world/level/block/state/BlockBehaviour$BlockStateBase;" => {
            api::state(rt, "minecraft:stone")
        }
        "Lnet/minecraft/world/level/block/Block;" => api::block(rt, "minecraft:stone"),
        "Lnet/minecraft/core/BlockPos;" | "Lnet/minecraft/core/Vec3i;" => api::block_pos(rt, 0, 64, 0),
        "Lnet/minecraft/core/Direction;" => api::direction(rt, 1),
        "Lnet/minecraft/world/item/ItemStack;" => horizon_compat::api_common::stack(rt, "minecraft:stone", 1),
        "Lnet/minecraft/world/level/ItemLike;" | "Lnet/minecraft/world/item/Item;" => api::get_static(rt, "net/minecraft/world/item/Items.STONE"),
        "Lnet/minecraft/resources/ResourceLocation;" => {
            let id = rt.string("minecraft:stone");
            api::call(rt, "net/minecraft/resources/ResourceLocation.parse.(Ljava/lang/String;)Lnet/minecraft/resources/ResourceLocation;", &[id])
        }
        "Lnet/minecraft/network/chat/Component;" => {
            let text = rt.string("stone");
            api::call(rt, "net/minecraft/network/chat/Component.literal.(Ljava/lang/String;)Lnet/minecraft/network/chat/MutableComponent;", &[text])
        }
        "Lnet/minecraft/nbt/CompoundTag;" => {
            let tag = rt.new_object("net/minecraft/nbt/CompoundTag");
            api::call(rt, "net/minecraft/nbt/CompoundTag.<init>.()V", &[tag]);
            tag
        }
        "Lnet/minecraft/world/phys/Vec3;" => {
            let v = rt.new_object("net/minecraft/world/phys/Vec3");
            api::call(rt, "net/minecraft/world/phys/Vec3.<init>.(DDD)V", &[v, V::D(0.5), V::D(64.0), V::D(0.5)]);
            v
        }
        // A registry, as `BuiltInRegistries` gives them: the items'.
        "Lnet/minecraft/core/Registry;"
        | "Lnet/minecraft/core/DefaultedRegistry;"
        | "Lnet/minecraft/core/WritableRegistry;"
        | "Lnet/minecraft/core/MappedRegistry;"
        | "Lnet/minecraft/core/DefaultedMappedRegistry;"
        | "Lnet/minecraft/core/IdMap;" => api::get_static(rt, "net/minecraft/core/registries/BuiltInRegistries.ITEM"),
        // A registry entry's holder, as `Registry.wrapAsHolder` gives one (stone's item).
        "Lnet/minecraft/core/Holder;" | "Lnet/minecraft/core/Holder$Reference;" => {
            let items = api::get_static(rt, "net/minecraft/core/registries/BuiltInRegistries.ITEM");
            let stone = api::get_static(rt, "net/minecraft/world/item/Items.STONE");
            api::call(rt, "net/minecraft/core/Registry.wrapAsHolder.(Ljava/lang/Object;)Lnet/minecraft/core/Holder;", &[items, stone])
        }
        // A state's property: `powered`, which the state's [`alternate`] holds.
        "Lnet/minecraft/world/level/block/state/properties/Property;" | "Lnet/minecraft/world/level/block/state/properties/BooleanProperty;" => {
            api::get_static(rt, "net/minecraft/world/level/block/state/properties/BlockStateProperties.POWERED")
        }
        // A stream codec, and an operation on one (`codec.apply(ByteBufCodecs.list())`).
        "Lnet/minecraft/network/codec/StreamCodec;" => api::get_static(rt, "net/minecraft/network/codec/ByteBufCodecs.VAR_INT"),
        "Lnet/minecraft/network/codec/StreamCodec$CodecOperation;" => {
            api::call(rt, "net/minecraft/network/codec/ByteBufCodecs.list.()Lnet/minecraft/network/codec/StreamCodec$CodecOperation;", &[])
        }
        "Lnet/minecraft/util/RandomSource;" => api::call(rt, "net/minecraft/util/RandomSource.create.()Lnet/minecraft/util/RandomSource;", &[]),
        "Lnet/minecraft/core/NonNullList;" => api::call(rt, "net/minecraft/core/NonNullList.create.()Lnet/minecraft/core/NonNullList;", &[]),
        "Lnet/minecraft/core/Position;" => sample(rt, "Lnet/minecraft/world/phys/Vec3;")?,
        "Lnet/minecraft/world/InteractionHand;" => api::get_static(rt, "net/minecraft/world/InteractionHand.MAIN_HAND"),
        "Lnet/minecraft/world/entity/EquipmentSlot;" => api::get_static(rt, "net/minecraft/world/entity/EquipmentSlot.MAINHAND"),
        "Lnet/minecraft/core/HolderLookup$Provider;" | "Lnet/minecraft/core/RegistryAccess;" => {
            let level = horizon_compat::glue::level(rt);
            api::call(rt, "net/minecraft/world/level/Level.registryAccess.()Lnet/minecraft/core/RegistryAccess;", &[level])
        }
        // Stone's key in the item registry, and that registry as the level's registries look it up.
        "Lnet/minecraft/resources/ResourceKey;" => {
            let items = api::get_static(rt, "net/minecraft/core/registries/Registries.ITEM");
            let id = sample(rt, "Lnet/minecraft/resources/ResourceLocation;")?;
            api::call(rt, "net/minecraft/resources/ResourceKey.create.(Lnet/minecraft/resources/ResourceKey;Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/resources/ResourceKey;", &[items, id])
        }
        "Lnet/minecraft/core/HolderLookup$RegistryLookup;" | "Lnet/minecraft/core/HolderLookup;" | "Lnet/minecraft/core/HolderGetter;" => {
            let registries = sample(rt, "Lnet/minecraft/core/HolderLookup$Provider;")?;
            let items = api::get_static(rt, "net/minecraft/core/registries/Registries.ITEM");
            api::call(rt, "net/minecraft/core/HolderLookup$Provider.lookupOrThrow.(Lnet/minecraft/resources/ResourceKey;)Lnet/minecraft/core/HolderLookup$RegistryLookup;", &[registries, items])
        }
        "Lnet/minecraft/world/entity/player/Player;" | "Lnet/minecraft/server/level/ServerPlayer;" => horizon_compat::menus::server_player(rt, PROBE_PLAYER),
        // A container of one slot and a slot of it, as a menu's are; an item handler and a fluid
        // tank of their own, as a block entity's are; the capabilities a mod asks for.
        "Lnet/minecraft/world/Container;" | "Lnet/minecraft/world/SimpleContainer;" => {
            let c = rt.new_object("net/minecraft/world/SimpleContainer");
            api::call(
                rt,
                "net/minecraft/world/SimpleContainer.<init>.(I)V",
                &[c, V::I(1)],
            );
            c
        }
        "Lnet/minecraft/world/inventory/Slot;" => {
            let c = sample(rt, "Lnet/minecraft/world/Container;")?;
            let s = rt.new_object("net/minecraft/world/inventory/Slot");
            api::call(rt, "net/minecraft/world/inventory/Slot.<init>.(Lnet/minecraft/world/Container;III)V", &[s, c, V::I(0), V::I(0), V::I(0)]);
            s
        }
        "Lnet/neoforged/neoforge/items/IItemHandler;" | "Lnet/neoforged/neoforge/items/IItemHandlerModifiable;" | "Lnet/neoforged/neoforge/items/ItemStackHandler;" => {
            let h = rt.new_object("net/neoforged/neoforge/items/ItemStackHandler");
            api::call(rt, "net/neoforged/neoforge/items/ItemStackHandler.<init>.(I)V", &[h, V::I(1)]);
            h
        }
        "Lnet/neoforged/neoforge/fluids/capability/IFluidHandler;" | "Lnet/neoforged/neoforge/fluids/capability/templates/FluidTank;" => {
            let t = rt.new_object("net/neoforged/neoforge/fluids/capability/templates/FluidTank");
            api::call(rt, "net/neoforged/neoforge/fluids/capability/templates/FluidTank.<init>.(I)V", &[t, V::I(1000)]);
            t
        }
        // NeoForge's energy buffer, as a machine's capability gives one; a water bucket's fluid
        // handler, as its item capability gives it.
        "Lnet/neoforged/neoforge/energy/IEnergyStorage;"
        | "Lnet/neoforged/neoforge/energy/EnergyStorage;" => {
            let s = rt.new_object("net/neoforged/neoforge/energy/EnergyStorage");
            api::call(
                rt,
                "net/neoforged/neoforge/energy/EnergyStorage.<init>.(I)V",
                &[s, V::I(1000)],
            );
            s
        }
        "Lnet/neoforged/neoforge/fluids/capability/IFluidHandlerItem;" => {
            let bucket = horizon_compat::api_common::stack(rt, "minecraft:water_bucket", 1);
            let fluids = api::get_static(
                rt,
                "net/neoforged/neoforge/capabilities/Capabilities$FluidHandler.ITEM",
            );
            api::call(
                rt,
                "net/minecraft/world/item/ItemStack.getCapability.(Lnet/neoforged/neoforge/capabilities/ItemCapability;)Ljava/lang/Object;",
                &[bucket, fluids],
            )
        }
        // A parser of SNBT over a reader of one compound, as a mod makes one to read its own.
        "Lnet/minecraft/nbt/TagParser;" => {
            let reader = rt.new_object("com/mojang/brigadier/StringReader");
            let line = rt.string("{}");
            api::call(rt, "com/mojang/brigadier/StringReader.<init>.(Ljava/lang/String;)V", &[reader, line]);
            let p = rt.new_object("net/minecraft/nbt/TagParser");
            api::call(rt, "net/minecraft/nbt/TagParser.<init>.(Lcom/mojang/brigadier/StringReader;)V", &[p, reader]);
            p
        }
        // A vanilla recipe, as the recipe manager holds one (two stone make two stone), and the
        // crafting grid a recipe is matched against (one stone).
        "Lnet/minecraft/world/item/crafting/Recipe;"
        | "Lnet/minecraft/world/item/crafting/CraftingRecipe;"
        | "Lnet/minecraft/world/item/crafting/ShapelessRecipe;" => {
            let stone = api::get_static(rt, "net/minecraft/world/item/Items.STONE");
            let items = rt.alloc(
                "[Lnet/minecraft/world/level/ItemLike;",
                horizon_compat::Data::Array(vec![stone]),
            );
            let ingredient = api::call(
                rt,
                "net/minecraft/world/item/crafting/Ingredient.of.([Lnet/minecraft/world/level/ItemLike;)Lnet/minecraft/world/item/crafting/Ingredient;",
                &[items],
            );
            let ingredients = sample(rt, "Lnet/minecraft/core/NonNullList;")?;
            for _ in 0..2 {
                api::call(
                    rt,
                    "net/minecraft/core/NonNullList.add.(Ljava/lang/Object;)Z",
                    &[ingredients, ingredient],
                );
            }
            let group = rt.string("");
            let misc = api::get_static(
                rt,
                "net/minecraft/world/item/crafting/CraftingBookCategory.MISC",
            );
            let result = horizon_compat::api_common::stack(rt, "minecraft:stone", 2);
            let r = rt.new_object("net/minecraft/world/item/crafting/ShapelessRecipe");
            api::call(
                rt,
                "net/minecraft/world/item/crafting/ShapelessRecipe.<init>.(Ljava/lang/String;Lnet/minecraft/world/item/crafting/CraftingBookCategory;Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/core/NonNullList;)V",
                &[r, group, misc, result, ingredients],
            );
            r
        }
        "Lnet/minecraft/world/item/crafting/RecipeInput;" | "Lnet/minecraft/world/item/crafting/CraftingInput;" => {
            let stone = sample(rt, "Lnet/minecraft/world/item/ItemStack;")?;
            let list = api::call(rt, "java/util/List.of.(Ljava/lang/Object;)Ljava/util/List;", &[stone]);
            api::call(rt, "net/minecraft/world/item/crafting/CraftingInput.of.(IILjava/util/List;)Lnet/minecraft/world/item/crafting/CraftingInput;", &[V::I(1), V::I(1), list])
        }
        "Lnet/neoforged/neoforge/capabilities/BlockCapability;" => api::get_static(rt, "net/neoforged/neoforge/capabilities/Capabilities$ItemHandler.BLOCK"),
        "Lnet/neoforged/neoforge/capabilities/EntityCapability;" => api::get_static(rt, "net/neoforged/neoforge/capabilities/Capabilities$ItemHandler.ENTITY"),
        "Lnet/neoforged/neoforge/capabilities/ItemCapability;" => api::get_static(rt, "net/neoforged/neoforge/capabilities/Capabilities$FluidHandler.ITEM"),
        // A vanilla enum's constant, as a mod names one (`EquipmentSlot.MAINHAND`), and a pose.
        "Lnet/minecraft/util/StringRepresentable;" => api::get_static(rt, "net/minecraft/world/entity/EquipmentSlot.MAINHAND"),
        "Lnet/minecraft/world/entity/Pose;" => api::get_static(rt, "net/minecraft/world/entity/Pose.STANDING"),
        // The ops the game reads and writes NBT with.
        "Lcom/mojang/serialization/DynamicOps;" => api::get_static(rt, "net/minecraft/nbt/NbtOps.INSTANCE"),
        // A set of stone's item holder (`HolderSet.direct`).
        "Lnet/minecraft/core/HolderSet;" | "Lnet/minecraft/core/HolderSet$Direct;" => {
            let stone = sample(rt, "Lnet/minecraft/core/Holder;")?;
            let holders = rt.alloc(
                "[Lnet/minecraft/core/Holder;",
                horizon_compat::Data::Array(vec![stone]),
            );
            api::call(
                rt,
                "net/minecraft/core/HolderSet.direct.([Lnet/minecraft/core/Holder;)Lnet/minecraft/core/HolderSet$Direct;",
                &[holders],
            )
        }
        // The probe player's click on the top of the block at (0, 64, 0), holding stone.
        "Lnet/minecraft/world/item/context/BlockPlaceContext;"
        | "Lnet/minecraft/world/item/context/UseOnContext;" => {
            let at = sample(rt, "Lnet/minecraft/world/phys/Vec3;")?;
            let up = sample(rt, "Lnet/minecraft/core/Direction;")?;
            let pos = sample(rt, "Lnet/minecraft/core/BlockPos;")?;
            let hit = rt.new_object("net/minecraft/world/phys/BlockHitResult");
            api::call(
                rt,
                "net/minecraft/world/phys/BlockHitResult.<init>.(Lnet/minecraft/world/phys/Vec3;Lnet/minecraft/core/Direction;Lnet/minecraft/core/BlockPos;Z)V",
                &[hit, at, up, pos, V::I(0)],
            );
            let level = horizon_compat::glue::level(rt);
            let player = sample(rt, "Lnet/minecraft/world/entity/player/Player;")?;
            let hand = sample(rt, "Lnet/minecraft/world/InteractionHand;")?;
            let stack = sample(rt, "Lnet/minecraft/world/item/ItemStack;")?;
            let (class, init) = if ty.contains("BlockPlaceContext") {
                (
                    "net/minecraft/world/item/context/BlockPlaceContext",
                    "net/minecraft/world/item/context/BlockPlaceContext.<init>.(Lnet/minecraft/world/level/Level;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/InteractionHand;Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/phys/BlockHitResult;)V",
                )
            } else {
                (
                    "net/minecraft/world/item/context/UseOnContext",
                    "net/minecraft/world/item/context/UseOnContext.<init>.(Lnet/minecraft/world/level/Level;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/InteractionHand;Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/phys/BlockHitResult;)V",
                )
            };
            let ctx = rt.new_object(class);
            api::call(rt, init, &[ctx, level, player, hand, stack, hit]);
            ctx
        }
        _ if class.is_some_and(level_class) => horizon_compat::glue::level(rt),
        _ if class.is_some_and(|c| matches!(c.strip_prefix("net/minecraft/world/entity/"), Some("Entity" | "LivingEntity" | "Mob" | "PathfinderMob" | "AgeableMob" | "animal/Animal"))) => {
            horizon_compat::vanilla_entities::of(rt, PROBE_MOB, "minecraft:cow", None)
        }
        // What a codec's function builds on (`RecordCodecBuilder.Instance`, a group of fields).
        _ => return class.and_then(|c| horizon_compat::codecs::probe_receiver(rt, c)),
    })
}

/// The receivers the mod makes of each API class it calls: the constructors of the class it calls
/// (a mod's own entity is the object `Entity(EntityType, Level)` builds, the receiver of its
/// `super.tick()`), then the API methods it calls that return the class, up to
/// [`STATICS_PER_CLASS`] of each kind per class: the methods returning `Entity` (`getVehicle`,
/// `ServerLevel.getEntity`...) no longer crowd the constructor out.
/// A value of a library's type for the measure's probe of a library call, as a mod has one: an
/// empty `ArrayList` for a `List`, a `HashMap` for a `Map`, an absent Guava `Optional`, a JOML
/// vector... None for a type with no sample here ([`sample`] then answers, or null stays).
fn lib_sample(rt: &mut horizon_compat::Rt, ty: &str) -> Option<horizon_compat::V> {
    use horizon_compat::{V, api};
    let made = |rt: &mut horizon_compat::Rt, class: &'static str, init: &'static str, args: &[V]| {
        let o = rt.new_object(class);
        let mut a = vec![o];
        a.extend_from_slice(args);
        api::call(rt, init, &a);
        o
    };
    Some(match ty {
        "Ljava/util/List;" | "Ljava/util/Collection;" | "Ljava/lang/Iterable;" | "Ljava/util/ArrayList;" | "Ljava/util/AbstractList;" | "Ljava/util/AbstractCollection;" => {
            made(rt, "java/util/ArrayList", "java/util/ArrayList.<init>.()V", &[])
        }
        "Ljava/util/Map;" | "Ljava/util/HashMap;" | "Ljava/util/AbstractMap;" => made(rt, "java/util/HashMap", "java/util/HashMap.<init>.()V", &[]),
        "Ljava/util/LinkedHashMap;" => made(rt, "java/util/LinkedHashMap", "java/util/LinkedHashMap.<init>.()V", &[]),
        "Ljava/util/TreeMap;" | "Ljava/util/SortedMap;" | "Ljava/util/NavigableMap;" => made(rt, "java/util/TreeMap", "java/util/TreeMap.<init>.()V", &[]),
        "Ljava/util/Set;" | "Ljava/util/HashSet;" | "Ljava/util/AbstractSet;" => made(rt, "java/util/HashSet", "java/util/HashSet.<init>.()V", &[]),
        "Ljava/util/LinkedHashSet;" => made(rt, "java/util/LinkedHashSet", "java/util/LinkedHashSet.<init>.()V", &[]),
        "Ljava/util/TreeSet;" | "Ljava/util/SortedSet;" | "Ljava/util/NavigableSet;" => made(rt, "java/util/TreeSet", "java/util/TreeSet.<init>.()V", &[]),
        "Ljava/util/Deque;" | "Ljava/util/Queue;" | "Ljava/util/ArrayDeque;" => made(rt, "java/util/ArrayDeque", "java/util/ArrayDeque.<init>.()V", &[]),
        "Ljava/util/LinkedList;" => made(rt, "java/util/LinkedList", "java/util/LinkedList.<init>.()V", &[]),
        "Ljava/util/Iterator;" => {
            let list = made(rt, "java/util/ArrayList", "java/util/ArrayList.<init>.()V", &[]);
            api::call(rt, "java/util/List.iterator.()Ljava/util/Iterator;", &[list])
        }
        "Ljava/util/ListIterator;" => {
            let list = made(rt, "java/util/ArrayList", "java/util/ArrayList.<init>.()V", &[]);
            api::call(rt, "java/util/List.listIterator.()Ljava/util/ListIterator;", &[list])
        }
        "Ljava/util/Spliterator;" => {
            let list = made(rt, "java/util/ArrayList", "java/util/ArrayList.<init>.()V", &[]);
            api::call(rt, "java/util/List.spliterator.()Ljava/util/Spliterator;", &[list])
        }
        "Ljava/util/stream/IntStream;" => api::call(rt, "java/util/stream/IntStream.range.(II)Ljava/util/stream/IntStream;", &[V::I(0), V::I(3)]),
        "Ljava/util/stream/LongStream;" => api::call(rt, "java/util/stream/LongStream.empty.()Ljava/util/stream/LongStream;", &[]),
        "Ljava/util/stream/DoubleStream;" => api::call(rt, "java/util/stream/DoubleStream.empty.()Ljava/util/stream/DoubleStream;", &[]),
        // fastutil's typed entries are entries of their key's and value's kinds.
        "Lit/unimi/dsi/fastutil/objects/Object2IntMap$Entry;" => {
            let k = rt.string("stone");
            api::call(
                rt,
                "java/util/Map.entry.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/util/Map$Entry;",
                &[k, V::I(0)],
            )
        }
        "Lit/unimi/dsi/fastutil/ints/Int2ObjectMap$Entry;" => {
            let v = rt.string("stone");
            api::call(
                rt,
                "java/util/Map.entry.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/util/Map$Entry;",
                &[V::I(0), v],
            )
        }
        "Lit/unimi/dsi/fastutil/longs/Long2ObjectMap$Entry;" => {
            let v = rt.string("stone");
            api::call(rt, "java/util/Map.entry.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/util/Map$Entry;", &[V::J(0), v])
        }
        "Ljava/util/Optional;" => api::call(rt, "java/util/Optional.empty.()Ljava/util/Optional;", &[]),
        "Ljava/util/stream/Stream;" => {
            let list = made(rt, "java/util/ArrayList", "java/util/ArrayList.<init>.()V", &[]);
            api::call(rt, "java/util/List.stream.()Ljava/util/stream/Stream;", &[list])
        }
        "Ljava/lang/Object;" | "Ljava/lang/Comparable;" | "Ljava/lang/CharSequence;" => rt.string("stone"),
        "Ljava/lang/Integer;" | "Ljava/lang/Number;" => api::call(rt, "java/lang/Integer.valueOf.(I)Ljava/lang/Integer;", &[V::I(0)]),
        "Ljava/lang/StringBuilder;" => made(rt, "java/lang/StringBuilder", "java/lang/StringBuilder.<init>.()V", &[]),
        "Ljava/util/UUID;" => made(rt, "java/util/UUID", "java/util/UUID.<init>.(JJ)V", &[V::J(1), V::J(2)]),
        "Ljava/util/Random;" => made(rt, "java/util/Random", "java/util/Random.<init>.()V", &[]),
        "Ljava/util/concurrent/atomic/AtomicInteger;" => made(rt, "java/util/concurrent/atomic/AtomicInteger", "java/util/concurrent/atomic/AtomicInteger.<init>.()V", &[]),
        "Ljava/util/concurrent/atomic/AtomicBoolean;" => made(rt, "java/util/concurrent/atomic/AtomicBoolean", "java/util/concurrent/atomic/AtomicBoolean.<init>.()V", &[]),
        "Ljava/util/concurrent/atomic/AtomicReference;" => made(rt, "java/util/concurrent/atomic/AtomicReference", "java/util/concurrent/atomic/AtomicReference.<init>.()V", &[]),
        "Lcom/google/common/base/Optional;" => api::call(rt, "com/google/common/base/Optional.absent.()Lcom/google/common/base/Optional;", &[]),
        "Lcom/google/common/collect/ImmutableList;" => api::call(rt, "com/google/common/collect/ImmutableList.of.()Lcom/google/common/collect/ImmutableList;", &[]),
        "Lcom/google/common/collect/ImmutableSet;" => api::call(rt, "com/google/common/collect/ImmutableSet.of.()Lcom/google/common/collect/ImmutableSet;", &[]),
        "Lcom/google/common/collect/ImmutableMap;" => api::call(rt, "com/google/common/collect/ImmutableMap.of.()Lcom/google/common/collect/ImmutableMap;", &[]),
        "Lcom/google/gson/JsonObject;" | "Lcom/google/gson/JsonElement;" => made(rt, "com/google/gson/JsonObject", "com/google/gson/JsonObject.<init>.()V", &[]),
        "Lcom/google/gson/JsonArray;" => made(rt, "com/google/gson/JsonArray", "com/google/gson/JsonArray.<init>.()V", &[]),
        "Lit/unimi/dsi/fastutil/ints/IntList;" | "Lit/unimi/dsi/fastutil/ints/IntArrayList;" | "Lit/unimi/dsi/fastutil/ints/IntCollection;" => {
            made(rt, "it/unimi/dsi/fastutil/ints/IntArrayList", "it/unimi/dsi/fastutil/ints/IntArrayList.<init>.()V", &[])
        }
        "Lit/unimi/dsi/fastutil/objects/ObjectList;" | "Lit/unimi/dsi/fastutil/objects/ObjectArrayList;" | "Lit/unimi/dsi/fastutil/objects/ObjectCollection;" => {
            made(rt, "it/unimi/dsi/fastutil/objects/ObjectArrayList", "it/unimi/dsi/fastutil/objects/ObjectArrayList.<init>.()V", &[])
        }
        "Lit/unimi/dsi/fastutil/ints/Int2ObjectMap;" | "Lit/unimi/dsi/fastutil/ints/Int2ObjectOpenHashMap;" => {
            made(rt, "it/unimi/dsi/fastutil/ints/Int2ObjectOpenHashMap", "it/unimi/dsi/fastutil/ints/Int2ObjectOpenHashMap.<init>.()V", &[])
        }
        "Lit/unimi/dsi/fastutil/longs/Long2ObjectMap;" | "Lit/unimi/dsi/fastutil/longs/Long2ObjectOpenHashMap;" => {
            made(rt, "it/unimi/dsi/fastutil/longs/Long2ObjectOpenHashMap", "it/unimi/dsi/fastutil/longs/Long2ObjectOpenHashMap.<init>.()V", &[])
        }
        "Lit/unimi/dsi/fastutil/objects/Object2IntMap;" | "Lit/unimi/dsi/fastutil/objects/Object2IntOpenHashMap;" => {
            made(rt, "it/unimi/dsi/fastutil/objects/Object2IntOpenHashMap", "it/unimi/dsi/fastutil/objects/Object2IntOpenHashMap.<init>.()V", &[])
        }
        "Ljava/util/Map$Entry;" => {
            let map = made(rt, "java/util/HashMap", "java/util/HashMap.<init>.()V", &[]);
            let k = rt.string("stone");
            api::call(
                rt,
                "java/util/Map.put.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
                &[map, k, k],
            );
            let entries = api::call(rt, "java/util/Map.entrySet.()Ljava/util/Set;", &[map]);
            let it = api::call(
                rt,
                "java/util/Set.iterator.()Ljava/util/Iterator;",
                &[entries],
            );
            api::call(rt, "java/util/Iterator.next.()Ljava/lang/Object;", &[it])
        }
        "Ljava/util/Comparator;" => api::call(
            rt,
            "java/util/Comparator.naturalOrder.()Ljava/util/Comparator;",
            &[],
        ),
        "Ljava/nio/file/Path;" => {
            let name = rt.string("stone");
            let none = rt.new_array(0, V::NULL);
            api::call(
                rt,
                "java/nio/file/Path.of.(Ljava/lang/String;[Ljava/lang/String;)Ljava/nio/file/Path;",
                &[name, none],
            )
        }
        "Ljava/io/File;" => {
            let name = rt.string("stone");
            made(rt, "java/io/File", "java/io/File.<init>.(Ljava/lang/String;)V", &[name])
        }
        "Lcom/google/common/collect/Multimap;" | "Lcom/google/common/collect/ListMultimap;" | "Lcom/google/common/collect/ArrayListMultimap;" => {
            api::call(rt, "com/google/common/collect/ArrayListMultimap.create.()Lcom/google/common/collect/ArrayListMultimap;", &[])
        }
        "Lcom/google/common/collect/SetMultimap;" | "Lcom/google/common/collect/HashMultimap;" => {
            api::call(rt, "com/google/common/collect/HashMultimap.create.()Lcom/google/common/collect/HashMultimap;", &[])
        }
        "Lcom/google/common/collect/Table;" | "Lcom/google/common/collect/HashBasedTable;" => {
            api::call(rt, "com/google/common/collect/HashBasedTable.create.()Lcom/google/common/collect/HashBasedTable;", &[])
        }
        "Lit/unimi/dsi/fastutil/objects/ObjectSet;" | "Lit/unimi/dsi/fastutil/objects/ObjectOpenHashSet;" => {
            made(rt, "it/unimi/dsi/fastutil/objects/ObjectOpenHashSet", "it/unimi/dsi/fastutil/objects/ObjectOpenHashSet.<init>.()V", &[])
        }
        "Lit/unimi/dsi/fastutil/ints/IntSet;" | "Lit/unimi/dsi/fastutil/ints/IntOpenHashSet;" => {
            made(rt, "it/unimi/dsi/fastutil/ints/IntOpenHashSet", "it/unimi/dsi/fastutil/ints/IntOpenHashSet.<init>.()V", &[])
        }
        "Lit/unimi/dsi/fastutil/longs/LongSet;" | "Lit/unimi/dsi/fastutil/longs/LongOpenHashSet;" => {
            made(rt, "it/unimi/dsi/fastutil/longs/LongOpenHashSet", "it/unimi/dsi/fastutil/longs/LongOpenHashSet.<init>.()V", &[])
        }
        "Lit/unimi/dsi/fastutil/longs/LongList;" | "Lit/unimi/dsi/fastutil/longs/LongArrayList;" => {
            made(rt, "it/unimi/dsi/fastutil/longs/LongArrayList", "it/unimi/dsi/fastutil/longs/LongArrayList.<init>.()V", &[])
        }
        "Lit/unimi/dsi/fastutil/longs/Long2IntMap;" | "Lit/unimi/dsi/fastutil/longs/Long2IntOpenHashMap;" => {
            made(rt, "it/unimi/dsi/fastutil/longs/Long2IntOpenHashMap", "it/unimi/dsi/fastutil/longs/Long2IntOpenHashMap.<init>.()V", &[])
        }
        "Lit/unimi/dsi/fastutil/objects/Object2ObjectMap;" | "Lit/unimi/dsi/fastutil/objects/Object2ObjectOpenHashMap;" => {
            made(rt, "it/unimi/dsi/fastutil/objects/Object2ObjectOpenHashMap", "it/unimi/dsi/fastutil/objects/Object2ObjectOpenHashMap.<init>.()V", &[])
        }
        "Lit/unimi/dsi/fastutil/objects/Reference2IntMap;" | "Lit/unimi/dsi/fastutil/objects/Reference2IntOpenHashMap;" => {
            made(rt, "it/unimi/dsi/fastutil/objects/Reference2IntOpenHashMap", "it/unimi/dsi/fastutil/objects/Reference2IntOpenHashMap.<init>.()V", &[])
        }
        "Lit/unimi/dsi/fastutil/objects/Reference2ReferenceMap;" | "Lit/unimi/dsi/fastutil/objects/Reference2ReferenceOpenHashMap;" => {
            made(rt, "it/unimi/dsi/fastutil/objects/Reference2ReferenceOpenHashMap", "it/unimi/dsi/fastutil/objects/Reference2ReferenceOpenHashMap.<init>.()V", &[])
        }
        "Lit/unimi/dsi/fastutil/PriorityQueue;" | "Lit/unimi/dsi/fastutil/objects/ObjectHeapPriorityQueue;" => {
            made(rt, "it/unimi/dsi/fastutil/objects/ObjectHeapPriorityQueue", "it/unimi/dsi/fastutil/objects/ObjectHeapPriorityQueue.<init>.()V", &[])
        }
        "Lit/unimi/dsi/fastutil/objects/ObjectIterator;" => {
            let list = made(rt, "it/unimi/dsi/fastutil/objects/ObjectArrayList", "it/unimi/dsi/fastutil/objects/ObjectArrayList.<init>.()V", &[]);
            api::call(rt, "it/unimi/dsi/fastutil/objects/ObjectArrayList.iterator.()Lit/unimi/dsi/fastutil/objects/ObjectListIterator;", &[list])
        }
        "Lorg/joml/Vector3f;" | "Lorg/joml/Vector3fc;" => made(rt, "org/joml/Vector3f", "org/joml/Vector3f.<init>.()V", &[]),
        "Lorg/joml/Vector4f;" | "Lorg/joml/Vector4fc;" => made(rt, "org/joml/Vector4f", "org/joml/Vector4f.<init>.()V", &[]),
        "Lorg/joml/Matrix4f;" | "Lorg/joml/Matrix4fc;" => made(rt, "org/joml/Matrix4f", "org/joml/Matrix4f.<init>.()V", &[]),
        "Lorg/joml/Matrix3f;" | "Lorg/joml/Matrix3fc;" => made(rt, "org/joml/Matrix3f", "org/joml/Matrix3f.<init>.()V", &[]),
        "Lorg/joml/Quaternionf;" | "Lorg/joml/Quaternionfc;" => made(rt, "org/joml/Quaternionf", "org/joml/Quaternionf.<init>.()V", &[]),
        _ => return None,
    })
}

fn made_receivers(
    calls: &std::collections::BTreeMap<horizon_mod_translator::class::Member, usize>,
    statics: &std::collections::BTreeSet<horizon_mod_translator::class::Member>,
    keep: fn(&str) -> bool,
) -> std::collections::BTreeMap<String, Vec<Receiver>> {
    let mut built: std::collections::BTreeMap<String, Vec<Receiver>> = Default::default();
    let mut returned: std::collections::BTreeMap<String, Vec<Receiver>> = Default::default();
    for m in calls.keys() {
        let key: &'static str = Box::leak(format!("{}.{}.{}", m.owner, m.name, m.desc).into_boxed_str());
        let n = horizon_mod_translator::codegen::arg_kinds(&m.desc).len() + usize::from(!statics.contains(m));
        let (class, r, by) = if m.name == "<init>" {
            (m.owner.as_str(), Receiver::New(key, n), &mut built)
        } else {
            let Some(ret) = m.desc.rsplit(')').next().and_then(|r| r.strip_prefix('L')).and_then(|r| r.strip_suffix(';')) else { continue };
            if !keep(ret) {
                continue;
            }
            (ret, Receiver::Returned(key, n), &mut returned)
        };
        let list = by.entry(class.to_owned()).or_default();
        if list.len() < STATICS_PER_CLASS {
            list.push(r);
        }
    }
    for (class, rs) in returned {
        built.entry(class).or_default().extend(rs);
    }
    built
}

/// The static fields of API classes the mod's classes refer to, by their declared class: up to
/// [`STATICS_PER_CLASS`] of each, the first met.
fn static_receivers(classes: &[horizon_mod_translator::class::ClassFile]) -> std::collections::BTreeMap<String, Vec<&'static str>> {
    let mut by: std::collections::BTreeMap<String, Vec<&'static str>> = Default::default();
    for c in classes {
        for i in 0..c.pool.len() as u16 {
            if !matches!(c.pool[i as usize], horizon_mod_translator::class::Const::FieldRef(..)) {
                continue;
            }
            let Some(m) = c.member(i) else { continue };
            let Some(ty) = m.desc.strip_prefix('L').and_then(|d| d.strip_suffix(';')) else { continue };
            if !horizon_mod_translator::is_api(&m.owner) || !horizon_mod_translator::is_api(ty) {
                continue;
            }
            let list = by.entry(ty.to_owned()).or_default();
            let key = format!("{}.{}", m.owner, m.name);
            if list.len() < STATICS_PER_CLASS && !list.iter().any(|k| *k == key) {
                list.push(Box::leak(key.into_boxed_str()));
            }
        }
    }
    by
}

/// The API classes of data generation: the game's and NeoForge's data providers and builders,
/// and NeoForge's model generators.
const DATAGEN_API: [&str; 4] = [
    "net/minecraft/data/",
    "net/neoforged/neoforge/data/",
    "net/neoforged/neoforge/common/data/",
    "net/neoforged/neoforge/client/model/generators/",
];

/// The jar's classes of data generation, which a player's game never runs: one whose superclasses
/// (the jar's own, up to the API's) reach a class of [`DATAGEN_API`] or implement one of its
/// interfaces (`BlockLootSubProvider`, `RecipeProvider`, `DataProvider`), one in a `datagen`
/// package, and the classes nested in them.
fn datagen_classes(
    classes: &[horizon_mod_translator::class::ClassFile],
) -> std::collections::BTreeSet<&str> {
    let by: std::collections::HashMap<&str, &horizon_mod_translator::class::ClassFile> =
        classes.iter().map(|c| (c.name.as_str(), c)).collect();
    let api = |n: &str| DATAGEN_API.iter().any(|p| n.starts_with(p));
    let mut out: std::collections::BTreeSet<&str> = classes
        .iter()
        .filter(|c| {
            if c.name.contains("/datagen/") {
                return true;
            }
            let mut at = Some(*c);
            for _ in 0..64 {
                let Some(k) = at else { break };
                if k.superclass.as_deref().is_some_and(api) || k.interfaces.iter().any(|i| api(i)) {
                    return true;
                }
                at = k.superclass.as_deref().and_then(|s| by.get(s).copied());
            }
            false
        })
        .map(|c| c.name.as_str())
        .collect();
    let outer: Vec<&str> = out.iter().copied().collect();
    out.extend(classes.iter().map(|c| c.name.as_str()).filter(|n| {
        outer
            .iter()
            .any(|o| n.len() > o.len() && n.starts_with(o) && n.as_bytes()[o.len()] == b'$')
    }));
    out
}

/// How many static fields of one class a method is tried on.
const STATICS_PER_CLASS: usize = 3;

/// Whether `owner` is one of the level's classes or interfaces, whose methods a mod calls on the
/// level it is given.
fn level_class(owner: &str) -> bool {
    matches!(
        owner.strip_prefix("net/minecraft/world/level/"),
        Some(
            "Level" | "LevelAccessor" | "LevelReader" | "BlockGetter" | "LevelWriter" | "CommonLevelAccessor" | "ServerLevelAccessor"
                | "LevelSimulatedReader" | "LevelSimulatedRW" | "EntityGetter" | "CollisionGetter" | "SignalGetter" | "LevelHeightAccessor"
                | "LevelTimeAccess" | "BlockAndTintGetter"
        )
    ) || matches!(
        owner,
        "net/minecraft/server/level/ServerLevel"
            | "net/neoforged/neoforge/common/extensions/ILevelExtension"
            | "net/neoforged/neoforge/common/extensions/ILevelReaderExtension"
            | "net/neoforged/neoforge/common/extensions/IBlockGetterExtension"
    )
}

/// The probe's player's UUID and its cow's entity id.
const PROBE_PLAYER: u128 = 0x5052_4f42_4500_0000_0000_0000_0000_0001;
const PROBE_MOB: i32 = 7;

/// What a hosted probe's calls ask: a player in place at full health, standing at the level's
/// origin, and a cow beside them.
fn probe_host() -> horizon_compat::host::MemoryHost {
    use horizon_compat::host::{EntityView, MemoryHost, VitalsView};
    let place = [0.5, 64.0, 0.5];
    let mut h = MemoryHost {
        player_id: Some(PROBE_PLAYER),
        place: Some((place, 0.0, 0.0)),
        vitals: Some(VitalsView::default()),
        players: vec![(PROBE_PLAYER, place)],
        ..MemoryHost::default()
    };
    h.player_entity_ids.insert(PROBE_PLAYER, 1);
    h.entities.push(EntityView { id: PROBE_MOB, kind: "minecraft:cow".into(), pos: [2.5, 64.0, 0.5], health: Some((10.0, 10.0)), ..EntityView::default() });
    h.effects.insert(None, Vec::new());
    h.effects.insert(Some(PROBE_MOB), Vec::new());
    h
}

/// Whether `owner` is one of vanilla's entity classes (or an interface of theirs), whose methods a
/// mod calls on the game's players and mobs.
fn entity_class(owner: &str) -> bool {
    owner.starts_with("net/minecraft/world/entity/")
        || matches!(owner, "net/minecraft/server/level/ServerPlayer" | "net/minecraft/client/player/LocalPlayer" | "net/minecraft/client/player/AbstractClientPlayer")
        || owner.starts_with("net/neoforged/neoforge/common/extensions/IEntityExtension")
        || owner.starts_with("net/neoforged/neoforge/common/extensions/ILivingEntityExtension")
        || owner.starts_with("net/neoforged/neoforge/common/extensions/IPlayerExtension")
}

/// Whether the runtime answers API method `key` of class `owner`: called with `n` arguments (the
/// receiver first, `receiver` saying what stands there; `fill` what the others are), does it count
/// as unmapped. `hosted`: with a game to ask (`probe_host`). `lib`: a library's method, its
/// samples first.
fn answers_of(
    hosted: bool,
    key: &'static str,
    owner: &'static str,
    n: usize,
    receiver: Receiver,
    fill: Args,
    lib: bool,
    field: Option<(&'static str, &'static str)>,
) -> bool {
    let mut rt = horizon_compat::Rt::new();
    let pick = |rt: &mut horizon_compat::Rt, ty: &str| {
        if lib {
            lib_sample(rt, ty).or_else(|| sample(rt, ty))
        } else {
            sample(rt, ty)
        }
    };
    let call = |rt: &mut horizon_compat::Rt| {
        // Null for each reference, the zero of its kind for a primitive (which is never a
        // reference), after the receiver's place when there is one.
        let mut args = vec![horizon_compat::V::NULL; n];
        let declared = param_types(key.rsplit_once('.').map_or("()V", |(_, d)| d));
        let offset = n - declared.len().min(n);
        for (i, ty) in declared.iter().enumerate() {
            if let Some(slot) = args.get_mut(offset + i) {
                *slot = match *ty {
                    "I" | "Z" | "B" | "C" | "S" => horizon_compat::V::I(0),
                    "J" => horizon_compat::V::J(0),
                    "F" => horizon_compat::V::F(0.0),
                    "D" => horizon_compat::V::D(0.0),
                    _ => horizon_compat::V::NULL,
                };
            }
        }
        match receiver {
            Receiver::Null => {}
            Receiver::Bare => args[0] = rt.new_object(owner),
            Receiver::Init => {
                let o = rt.new_object(owner);
                let init = rt.static_key(&format!("{owner}.<init>.()V"));
                horizon_compat::api::call(rt, init, &[o]);
                if !rt.unmapped.is_empty() || rt.thrown.is_some() {
                    return false;
                }
                args[0] = o;
            }
            Receiver::Sample => {
                args[0] = pick(rt, &format!("L{owner};")).unwrap_or_else(|| rt.new_object(owner))
            }
            Receiver::Alternate => {
                args[0] = alternate(rt, owner).unwrap_or_else(|| rt.new_object(owner))
            }
            Receiver::Player => args[0] = horizon_compat::menus::server_player(rt, PROBE_PLAYER),
            Receiver::Mob => {
                args[0] = horizon_compat::vanilla_entities::of(rt, PROBE_MOB, "minecraft:cow", None)
            }
            Receiver::Level => args[0] = horizon_compat::glue::level(rt),
            Receiver::New(init, n) => {
                let class = init.split_once('.').map_or(owner, |(c, _)| c);
                let o = rt.new_object(class);
                let mut a = vec![horizon_compat::V::NULL; n];
                a[0] = o;
                horizon_compat::api::call(rt, init, &a);
                if !rt.unmapped.is_empty() || rt.thrown.is_some() {
                    return false;
                }
                args[0] = o;
            }
            Receiver::Returned(maker, n) => {
                let v = horizon_compat::api::call(rt, maker, &vec![horizon_compat::V::NULL; n]);
                if v == horizon_compat::V::NULL
                    || v == horizon_compat::V::Void
                    || !rt.unmapped.is_empty()
                    || rt.thrown.is_some()
                {
                    return false;
                }
                args[0] = v;
            }
            Receiver::Static(field) => {
                let v = horizon_compat::api::get_static(rt, field);
                if v == horizon_compat::V::NULL
                    || v == horizon_compat::V::Void
                    || !rt.unmapped.is_empty()
                {
                    return false;
                }
                args[0] = v;
            }
        }
        let params = param_types(key.rsplit_once('.').map_or("()V", |(_, d)| d));
        let first = match fill {
            Args::Null => None,
            Args::Instance => Some(1),
            Args::Static => Some(0),
        };
        if let Some(first) = first {
            for (i, ty) in params.iter().enumerate() {
                if let (Some(v), true) = (pick(rt, ty), first + i < n) {
                    args[first + i] = v;
                }
            }
            // The samples' own making must have gone through.
            if !rt.unmapped.is_empty() || rt.thrown.is_some() {
                return false;
            }
        }
        // A field of the API's object, read as translated code reads it (`field`: its name, and
        // the runtime's own for it): mapped when something is there.
        if let Some((name, own)) = field {
            let v = horizon_compat::api::game_field(rt, args[0], name, own, key);
            return v != horizon_compat::V::Void && rt.unmapped.is_empty();
        }
        horizon_compat::api::call(rt, key, &args);
        rt.unmapped.is_empty()
    };
    if hosted {
        let mut host = probe_host();
        rt.with_host(&mut host, call)
    } else {
        call(&mut rt)
    }
}

/// What a program's calls are asked on besides null and a bare object: the static fields of the
/// API its classes read (`statics`, by the field's class), and what its code makes of each class
/// (`made`).
struct Probe {
    hosted: bool,
    statics: std::collections::BTreeMap<String, Vec<&'static str>>,
    made: std::collections::BTreeMap<String, Vec<Receiver>>,
}

impl Probe {
    fn receivers(&self, owner: &'static str) -> Vec<Receiver> {
        let mut receivers = vec![Receiver::Null, Receiver::Bare];
        if self.hosted && entity_class(owner) {
            receivers.extend([Receiver::Player, Receiver::Mob]);
        }
        if self.hosted && level_class(owner) {
            receivers.push(Receiver::Level);
        }
        if self.hosted && has_alternate(owner) {
            receivers.push(Receiver::Alternate);
        }
        receivers.extend(
            self.statics
                .get(owner)
                .map_or(&[][..], Vec::as_slice)
                .iter()
                .map(|&f| Receiver::Static(f)),
        );
        receivers.extend(
            self.made
                .get(owner)
                .map_or(&[][..], Vec::as_slice)
                .iter()
                .copied(),
        );
        receivers
    }

    /// Whether the runtime answers the method: on each receiver, null arguments and (hosted) ones
    /// of their declared types, and on a value of the owner's type.
    fn asks(&self, m: &horizon_mod_translator::class::Member, is_static: bool) -> bool {
        let key: &'static str =
            Box::leak(format!("{}.{}.{}", m.owner, m.name, m.desc).into_boxed_str());
        let arity =
            horizon_mod_translator::codegen::arg_kinds(&m.desc).len() + usize::from(!is_static);
        let owner: &'static str = Box::leak(m.owner.clone().into_boxed_str());
        let hosted = self.hosted;
        let fills: &[Args] = if hosted {
            &[Args::Null, Args::Instance]
        } else {
            &[Args::Null]
        };
        if is_static {
            return answers_of(
                hosted,
                key,
                owner,
                arity,
                Receiver::Null,
                Args::Null,
                false,
                None,
            ) || (hosted
                && answers_of(
                    hosted,
                    key,
                    owner,
                    arity,
                    Receiver::Null,
                    Args::Static,
                    false,
                    None,
                ));
        }
        let receivers = self.receivers(owner);
        fills.iter().any(|&fill| {
            receivers
                .iter()
                .any(|&r| answers_of(hosted, key, owner, arity, r, fill, false, None))
        }) || (hosted
            && answers_of(
                hosted,
                key,
                owner,
                arity,
                Receiver::Sample,
                Args::Instance,
                false,
                None,
            ))
    }

    /// Whether a read of instance field `name` (of type `desc`) of the API class `owner` finds
    /// something: the object's own state, or the runtime's answer for the field.
    fn reads(&self, owner: &str, name: &str, desc: &str) -> bool {
        let key: &'static str = Box::leak(format!("{owner}.${name}.(){desc}").into_boxed_str());
        let owner: &'static str = Box::leak(owner.to_owned().into_boxed_str());
        let field: (&'static str, &'static str) = (
            Box::leak(name.to_owned().into_boxed_str()),
            Box::leak(format!("${name}").into_boxed_str()),
        );
        let mut receivers = self.receivers(owner);
        receivers.push(Receiver::Sample);
        receivers.iter().any(|&r| {
            answers_of(
                self.hosted,
                key,
                owner,
                1,
                r,
                Args::Null,
                false,
                Some(field),
            )
        })
    }

    /// Whether static field `field` (`owner.NAME`) of the API has a value.
    fn constant(&self, field: &str) -> bool {
        let field: &'static str = Box::leak(field.to_owned().into_boxed_str());
        let mut rt = horizon_compat::Rt::new();
        let mut host = probe_host();
        rt.with_host(&mut host, |rt| {
            let v = horizon_compat::api::get_static(rt, field);
            v != horizon_compat::V::NULL && v != horizon_compat::V::Void && rt.unmapped.is_empty()
        })
    }
}

/// The game's own methods that stand for what the program calls of the API and the runtime
/// doesn't map (`derive`), as classes to translate with it; asked of the runtime with a game
/// (`Probe`, hosted).
fn derived_classes(
    program: &[horizon_mod_translator::class::ClassFile],
    game: &[&Path],
) -> Result<Vec<horizon_mod_translator::class::ClassFile>, String> {
    horizon_compat::install();
    let own: std::collections::HashSet<&str> = program.iter().map(|c| c.name.as_str()).collect();
    // The classes the program names, for the Java runtime's enums among them.
    let mut named = std::collections::HashSet::new();
    for c in program {
        for i in 0..c.pool.len() as u16 {
            if let Ok(name) = c.class_name(i)
                && horizon_mod_translator::derive::is_jdk(&name)
                && !own.contains(name.as_str())
            {
                named.insert(name);
            }
        }
    }
    let game = horizon_mod_translator::derive::Game::open_naming(game, &named)?;
    let all: Vec<&horizon_mod_translator::class::ClassFile> = program.iter().collect();
    // What the program calls of the API, and the static methods it calls of the Java runtime's
    // classes (`Arrays.binarySearch`, `Math.nextUp`): where the runtime has no answer, the JDK's
    // own code may stand as the game's does.
    let mut calls = horizon_mod_translator::code_calls(&all, &|c| {
        (horizon_mod_translator::is_api(c) || horizon_mod_translator::derive::is_library(c))
            && !own.contains(c)
    });
    calls.retain(|m, (_, is_static)| {
        *is_static || !horizon_mod_translator::derive::is_library(&m.owner)
    });
    let statics_called = calls
        .iter()
        .filter(|(_, (_, is_static))| *is_static)
        .map(|(m, _)| m.clone())
        .collect();
    let counts = calls.iter().map(|(m, (n, _))| (m.clone(), *n)).collect();
    let probe = Probe {
        hosted: true,
        statics: static_receivers(program),
        made: made_receivers(&counts, &statics_called, horizon_mod_translator::is_api),
    };
    let (asks, reads, constant, translated) = (
        |m: &horizon_mod_translator::class::Member, is_static: bool| probe.asks(m, is_static),
        |owner: &str, name: &str, desc: &str| probe.reads(owner, name, desc),
        |field: &str| probe.constant(field),
        |c: &str| own.contains(c),
    );
    let mut deriver = horizon_mod_translator::derive::Deriver::new(
        &game,
        horizon_mod_translator::derive::Mapped {
            method: &asks,
            field: &reads,
            constant: &constant,
            translated: &translated,
        },
    );
    for (m, (_, is_static)) in &calls {
        if !probe.asks(m, *is_static) {
            deriver.blockers(&m.owner, &m.name, &m.desc, !is_static);
        }
    }
    // The game's enums the program names (a constant read, a `values()`, a `switch` over one):
    // carried with their constants, for the runtime to answer by where it lists none itself.
    for c in program {
        for i in 0..c.pool.len() as u16 {
            if let Ok(name) = c.class_name(i)
                && (horizon_mod_translator::is_api(&name)
                    || horizon_mod_translator::derive::is_jdk(&name))
                && !own.contains(name.as_str())
                && game.class(&name).is_some_and(|g| g.access & 0x4000 != 0)
            {
                deriver.enums.insert(name);
            }
        }
    }
    // The game's fields the mods' own code reads or writes that a mapped method of the game's
    // only returns or only sets, as for the game's own code above.
    let before = deriver.accessors.len();
    for c in program {
        deriver.field_accessors(c);
    }
    println!(
        "  the game's fields read through a method of the game's: {} ({} of them from the mods' own code)",
        deriver.accessors.len(),
        deriver.accessors.len() - before
    );
    horizon_mod_translator::codegen::set_accessors(deriver.accessors.clone());
    horizon_mod_translator::codegen::set_inherited(
        horizon_mod_translator::derive::inherited_over_defaults(program, &game),
    );
    // The instance fields the game's classes hold above the program's: one of the same name
    // declared below hides it, and is kept under a name of its own.
    horizon_mod_translator::codegen::set_game_fields(horizon_mod_translator::derive::fields_above(
        program, &game,
    ));
    Ok(deriver.classes())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `bundle <jar> <out> [--with <addon jar>]... [--library <jar>]... [--runtime <horizon-compat-core
    // dir>]`: `--with` translates mods built on the first with it, as one engine (see
    // `bundle::write_group`); `--library`, a jar whose code they may call (a language loader's,
    // Kotlin for Forge's), of which what they reach; `--runtime`, a bundle for tools whose engine
    // links the runtime core instead of carrying it.
    if args.first().map(String::as_str) == Some("bundle") && args.len() >= 3 {
        let (mut with, mut libraries, mut runtime, mut rest) = (Vec::new(), Vec::new(), None, args[3..].iter());
        let mut game = Vec::new();
        let mut find_game = true;
        while let Some(a) = rest.next() {
            if a == "--no-game" {
                find_game = false;
                continue;
            }
            match (a.as_str(), rest.next()) {
                ("--with", Some(j)) => with.push(Path::new(j)),
                ("--library", Some(j)) => libraries.push(Path::new(j)),
                ("--game", Some(j)) => game.push(Path::new(j)),
                ("--runtime", Some(dir)) => runtime = Some(Path::new(dir)),
                _ => {
                    eprintln!(
                        "bundle <jar> <out dir> [--with <addon jar>]... [--library <jar>]... [--game <jar of the NeoForge install>]... [--no-game] [--runtime <horizon-compat-core dir>]"
                    );
                    return ExitCode::FAILURE;
                }
            }
        }
        // A jar of the pack that is no mod (a language loader's library, code two mods share) is a
        // library: what the mods reach of it is translated.
        let (with, plain): (Vec<&Path>, Vec<&Path>) = with
            .into_iter()
            .partition(|j| horizon_mod_translator::bundle::mod_id(j).is_some());
        for j in &plain {
            println!("{}: no mod of its own, taken as a library", j.display());
        }
        libraries.extend(plain);
        let jars: Vec<&Path> = std::iter::once(Path::new(&args[1])).chain(with).collect();
        // No `--game`: the player's own NeoForge install, where a launcher keeps one.
        let found = if game.is_empty() && find_game {
            horizon_mod_translator::find_game_jars().unwrap_or_default()
        } else {
            Vec::new()
        };
        if let Some(first) = found.first() {
            println!(
                "the game's own code from {}",
                first
                    .parent()
                    .and_then(Path::parent)
                    .unwrap_or(first)
                    .display()
            );
        }
        game.extend(found.iter().map(|p| p.as_path()));
        // With the game's own code, the Java runtime's: the classes of it that are plain code
        // (`jdk_code`), from the runtime a launcher installed for the game.
        let java = if game.is_empty() {
            None
        } else {
            horizon_mod_translator::find_java_image()
        };
        if let Some(image) = &java {
            println!("the Java runtime's own code from {}", image.display());
            game.push(image);
        }
        let result = horizon_mod_translator::program_with(&jars, &libraries, &game).and_then(|(mut c, library, kept)| {
            println!("{} classes, {kept} of them of the {library} in the libraries the jars bundle", c.len());
            if !game.is_empty() {
                let jdk = c.iter().filter(|k| horizon_mod_translator::jdk_code(&k.name)).count();
                if java.is_some() {
                    println!("  of the Java runtime's own code: {jdk} classes");
                }
                let own = c.iter().filter(|k| horizon_mod_translator::game_code(&k.name)).count();
                let derived = derived_classes(&c, &game)?;
                println!("  of the game's own code: {own} classes the runtime doesn't model, and {} methods of {} it does", derived.iter().map(|k| k.methods.len()).sum::<usize>(), derived.len());
                let of_java = |k: &&horizon_mod_translator::class::ClassFile| {
                    k.name.strip_prefix(horizon_compat::linked::DERIVED).is_some_and(horizon_mod_translator::derive::is_library)
                };
                if java.is_some() {
                    println!("  and of the Java runtime's and the libraries': {} methods of {} classes the runtime models", derived.iter().filter(of_java).map(|k| k.methods.len()).sum::<usize>(), derived.iter().filter(of_java).count());
                }
                if std::env::var_os("HZ_DEBUG_DERIVED_JDK").is_some() {
                    for k in derived.iter().filter(of_java) {
                        for m in &k.methods {
                            eprintln!("jdk-derived {}.{}.{}", k.name, m.name, m.desc);
                        }
                    }
                }
                c.extend(derived);
            }
            // The bundle's tables, each behind a class of its own: the jars' own files and the
            // classes' members as reflection lists them.
            for table in [horizon_compat::FILES, horizon_compat::MEMBERS] {
                c.push(horizon_mod_translator::codegen::table_class(table));
            }
            horizon_mod_translator::bundle::write_group(&jars, &c, Path::new(&args[2]), runtime)
        });
        return match result {
            Ok((id, left)) => {
                println!("{id}: native mod bundle in {}", args[2]);
                for l in left {
                    println!("  not translated: {l}");
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    if args.first().map(String::as_str) == Some("translate") && args.len() == 3 {
        return match translate(Path::new(&args[1]), Path::new(&args[2])) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    // `mixins <jar>...`: what each jar's mixins do to the game (the injections the translator
    // doesn't apply), one line each, then the totals by kind.
    if args.first().map(String::as_str) == Some("mixins") && args.len() >= 2 {
        for jar in &args[1..] {
            let classes = match horizon_mod_translator::classes(Path::new(jar)) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{jar}: {e}");
                    continue;
                }
            };
            let all = horizon_mod_translator::mixin_injections(&classes);
            let mut by: std::collections::BTreeMap<&str, usize> = Default::default();
            for i in &all {
                *by.entry(i.kind.as_str()).or_default() += 1;
                println!("  {} {} {} @ {} ({})", i.kind, i.target, i.method, i.at, i.mixin);
            }
            println!("{jar}: {} mixin members {by:?}", all.len());
        }
        return ExitCode::SUCCESS;
    }
    // What the translator weaves of the mixins among these jars' classes into each other (a mod
    // and its addons, as `bundle --with` translates them), and what it leaves.
    if args.first().map(String::as_str) == Some("weaves") && args.len() >= 2 {
        let jars: Vec<&Path> = args[1..].iter().map(Path::new).collect();
        let classes = match horizon_mod_translator::classes_of(&jars) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        };
        let all: Vec<&horizon_mod_translator::class::ClassFile> = classes.iter().collect();
        let configs = horizon_mod_translator::mixin_configs_of(&jars);
        let plan = horizon_mod_translator::weave::plan_with(&all, &configs);
        for c in &configs {
            for m in &c.off {
                println!("  off {m} (its plugin's, for the mods here)");
            }
        }
        let mut by: std::collections::BTreeMap<String, usize> = Default::default();
        for (target, ws) in &plan.weaves {
            for w in ws {
                let kind = format!("{:?}", w.kind);
                let kind = kind.split([' ', '{']).next().unwrap_or("").to_owned();
                let at = format!("{:?}", w.at);
                let at = at.split([' ', '{']).next().unwrap_or("").to_owned();
                *by.entry(format!("{kind} at {at}")).or_default() += 1;
                println!("  {target} <- {}.{} ({kind} at {at})", w.mixin, w.handler);
            }
        }
        let mixins: usize = plan.mixins.values().map(Vec::len).sum();
        let woven: usize = plan.weaves.values().map(Vec::len).sum();
        let left: usize = plan.unsupported.values().sum();
        println!("{} targets, {mixins} mixins, {woven} handlers woven, {left} left", plan.mixins.len());
        for (k, n) in &by {
            println!("  woven {n:5} {k}");
        }
        for (k, n) in &plan.unsupported {
            println!("  left  {n:5} {k}");
        }
        for (k, n) in &plan.skipped {
            println!("  skipped {n:5} {k} (may find none: Mixin skips them too)");
        }
        return ExitCode::SUCCESS;
    }
    // `game-jars [libraries dir]`: the NeoForge install's jars `bundle` reads by itself, a line each.
    if args.first().map(String::as_str) == Some("game-jars") {
        let found = match args.get(1) {
            Some(dir) => horizon_mod_translator::game_jars_in(Path::new(dir)),
            None => horizon_mod_translator::find_game_jars(),
        };
        for j in found.iter().flatten() {
            println!("{}", j.display());
        }
        return if found.is_some() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    // `game-code <mod jar>... --game <jar>...`: what of the game's own code the mods reach
    // (`program_with`), by package, and the largest classes.
    if args.first().map(String::as_str) == Some("game-code") && args.len() >= 2 {
        let split = args
            .iter()
            .position(|a| a == "--game")
            .unwrap_or(args.len());
        let jars: Vec<&Path> = args[1..split].iter().map(Path::new).collect();
        let game: Vec<&Path> = args[split..]
            .iter()
            .filter(|a| *a != "--game")
            .map(Path::new)
            .collect();
        let program = match horizon_mod_translator::program_with(&jars, &[], &game) {
            Ok((c, _, _)) => c,
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        };
        let own: Vec<_> = program
            .iter()
            .filter(|c| horizon_mod_translator::game_code(&c.name))
            .collect();
        let size = |c: &horizon_mod_translator::class::ClassFile| {
            c.methods
                .iter()
                .filter_map(|m| m.code.as_ref())
                .map(|code| code.bytes.len())
                .sum::<usize>()
        };
        let methods: usize = own
            .iter()
            .map(|c| c.methods.iter().filter(|m| m.code.is_some()).count())
            .sum();
        let mods: usize = program
            .iter()
            .filter(|c| !horizon_mod_translator::game_code(&c.name))
            .map(size)
            .sum();
        println!(
            "game code reached: {} classes, {methods} methods, {} bytes of code (the mods' own: {mods})",
            own.len(),
            own.iter().map(|c| size(c)).sum::<usize>()
        );
        let mut by: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
        for c in &own {
            let e = by
                .entry(c.name.split('/').take(4).collect::<Vec<_>>().join("/"))
                .or_default();
            e.0 += 1;
            e.1 += size(c);
        }
        let mut by: Vec<_> = by.into_iter().collect();
        by.sort_by_key(|e| std::cmp::Reverse(e.1.1));
        for (p, (n, b)) in by.iter().take(40) {
            println!("  {n:5} classes {b:8} bytes  {p}");
        }
        if std::env::var_os("LIST").is_some() {
            for c in &own {
                println!("  class {} {}", size(c), c.name);
            }
        }
        return ExitCode::SUCCESS;
    }
    if args.first().map(String::as_str) != Some("coverage") || args.len() < 2 {
        eprintln!("usage: horizon-mod-translator coverage <jar>... | mixins <jar>... | weaves <mod jar> <addon jar>... | translate <jar> <out dir> | bundle <jar> <out dir> [--with <addon jar>]... [--runtime <horizon-compat-core dir>]");
        return ExitCode::FAILURE;
    }
    // The mapping answers the core's questions (`Rt::is_a` of Minecraft's class families): without
    // it, a handler that checks its receiver's class finds no widget, block or entity.
    horizon_compat::install();
    for jar in &args[1..] {
        let classes = match horizon_mod_translator::classes(Path::new(jar)) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        };
        // A player's game never runs the mod's data generation: its providers are left out.
        let datagen = datagen_classes(&classes);
        let runtime: Vec<_> = classes
            .iter()
            .filter(|c| !datagen.contains(c.name.as_str()))
            .cloned()
            .collect();
        let calls = horizon_mod_translator::api_calls(&runtime);
        // GAME_JARS (the NeoForge install's jars, `:` between them): the game's own code the mod
        // reaches is translated with it (`program_with`), so a call into it is no call of the API:
        // it runs the game's method, where the class (or one below it) has it with code.
        let mut game_jars: Vec<std::path::PathBuf> = std::env::var("GAME_JARS")
            .map(|v| {
                v.split(':')
                    .filter(|p| !p.is_empty())
                    .map(Into::into)
                    .collect()
            })
            .unwrap_or_default();
        // And the Java runtime's own code with it, as `bundle` takes it.
        if !game_jars.is_empty() {
            game_jars.extend(horizon_mod_translator::find_java_image());
        }
        let game_refs: Vec<&Path> = game_jars.iter().map(|p| p.as_path()).collect();
        let with_game = if game_refs.is_empty() {
            Vec::new()
        } else {
            horizon_mod_translator::program_with(&[Path::new(jar)], &[], &game_refs)
                .map(|p| p.0)
                .unwrap_or_default()
        };
        let game_part: std::collections::HashMap<&str, &horizon_mod_translator::class::ClassFile> =
            with_game
                .iter()
                .filter(|c| horizon_mod_translator::game_code(&c.name))
                .map(|c| (c.name.as_str(), c))
                .collect();
        let above = |class: &str, owner: &str| {
            let (mut todo, mut seen) = (vec![class.to_owned()], std::collections::HashSet::new());
            while let Some(c) = todo.pop() {
                if c == owner {
                    return true;
                }
                if let Some(cf) = game_part.get(c.as_str()).filter(|_| seen.insert(c.clone())) {
                    todo.extend(cf.superclass.iter().chain(&cf.interfaces).cloned());
                }
            }
            false
        };
        let through_game = |m: &horizon_mod_translator::class::Member| {
            let has = |c: &horizon_mod_translator::class::ClassFile| {
                c.methods
                    .iter()
                    .any(|x| x.name == m.name && x.desc == m.desc && x.code.is_some())
            };
            game_part.contains_key(m.owner.as_str())
                && game_part
                    .values()
                    .any(|c| has(c) && (above(&m.owner, &c.name) || above(&c.name, &m.owner)))
        };
        // Which of them the runtime maps: call each and see whether it counts as unmapped. Null
        // arguments first; then, as a handler may answer only for an object of its class, with an
        // object of the owner's class as the receiver (or first argument of a static call). With
        // PROBE=hosted, each call also has a game to ask (`probe_host`: a player in place, a cow
        // beside them), and an entity class's method is tried on that player and that cow too, as
        // a mod's calls find them in a game; a level class's on the runtime's level; and any
        // method on the static fields of API classes the mod reads whose declared type is the
        // method's class (`BuiltInRegistries.ITEM` for `DefaultedRegistry.getKey`, `MobEffects.SPEED`
        // for `Holder.value`, `Codec.STRING` for `PrimitiveCodec.fieldOf`), as its calls get them.
        let hosted = std::env::var_os("PROBE").is_some_and(|p| p == "hosted");
        let statics = if hosted {
            static_receivers(if with_game.is_empty() {
                &classes
            } else {
                &with_game
            })
        } else {
            Default::default()
        };
        // What the game's code the mod reaches calls of the API in its turn.
        let own_game: Vec<&horizon_mod_translator::class::ClassFile> =
            game_part.values().copied().collect();
        let theirs = horizon_mod_translator::code_calls(&own_game, &|c| {
            horizon_mod_translator::is_api(c) && !game_part.contains_key(c)
        });
        // And on what the mod makes of the method's class: an object its constructor builds, or
        // what an API method returning that class gives (`NonNullList.withSize`), null arguments.
        // A static call (`invokestatic`) has no receiver: its arguments are all it is given.
        let statics_called = horizon_mod_translator::static_calls(&runtime);
        let made = if hosted {
            let (mut both, mut both_static) = (calls.clone(), statics_called.clone());
            for (m, (n, is_static)) in &theirs {
                *both.entry(m.clone()).or_insert(0) += n;
                if *is_static {
                    both_static.insert(m.clone());
                }
            }
            made_receivers(&both, &both_static, horizon_mod_translator::is_api)
        } else {
            Default::default()
        };
        let (mut mapped, mut sites, mut mapped_sites) = (0usize, 0usize, 0usize);
        let mut missing: Vec<(usize, String)> = Vec::new();
        let answers =
            |key: &'static str,
             owner: &'static str,
             n: usize,
             receiver: Receiver,
             fill: Args,
             lib: bool| answers_of(hosted, key, owner, n, receiver, fill, lib, None);
        let probe = Probe {
            hosted,
            statics: statics.clone(),
            made: made.clone(),
        };
        let asks =
            |m: &horizon_mod_translator::class::Member, is_static: bool| probe.asks(m, is_static);
        let (mut derived, mut derived_sites) = (0usize, 0usize);
        for (m, n) in &calls {
            let key: &'static str = Box::leak(format!("{}.{}.{}", m.owner, m.name, m.desc).into_boxed_str());
            let is_static = statics_called.contains(m);
            let arity = horizon_mod_translator::codegen::arg_kinds(&m.desc).len() + usize::from(!is_static);
            sites += n;
            if through_game(m) {
                derived += 1;
                derived_sites += n;
                continue;
            }
            let owner: &'static str = Box::leak(m.owner.clone().into_boxed_str());
            let entity = hosted && entity_class(owner);
            let level = hosted && level_class(owner);
            let fields = statics.get(owner).map_or(&[][..], Vec::as_slice);
            let mut receivers = vec![Receiver::Null, Receiver::Bare];
            if entity {
                receivers.extend([Receiver::Player, Receiver::Mob]);
            }
            if level {
                receivers.push(Receiver::Level);
            }
            if hosted && has_alternate(owner) {
                receivers.push(Receiver::Alternate);
            }
            receivers.extend(fields.iter().map(|&f| Receiver::Static(f)));
            receivers.extend(made.get(owner).map_or(&[][..], Vec::as_slice).iter().copied());
            // Null arguments first; hosted, also each of its declared type (a state, a position, a
            // stack, the player), on the same receivers and on a value of the owner's type.
            let fills: &[Args] = if hosted { &[Args::Null, Args::Instance] } else { &[Args::Null] };
            let answered = if is_static {
                answers(key, owner, arity, Receiver::Null, Args::Null, false) || (hosted && answers(key, owner, arity, Receiver::Null, Args::Static, false))
            } else {
                fills.iter().any(|&fill| receivers.iter().any(|&r| answers(key, owner, arity, r, fill, false)))
                    || (hosted && answers(key, owner, arity, Receiver::Sample, Args::Instance, false))
            };
            if answered {
                mapped += 1;
                mapped_sites += n;
            } else {
                missing.push((*n, key.to_owned()));
            }
        }
        missing.sort_by(|a, b| b.0.cmp(&a.0));
        // With GAME_JARS: which of the missing methods the game's own code can stand for over
        // what the runtime maps (`derive`): a bundle made with those jars carries them, so they
        // count as mapped; for the rest, what blocks it (listed with DERIVE=1).
        let (mut stands, mut stand_sites) = (0usize, 0usize);
        let mut derive_lines = Vec::new();
        let game = if game_refs.is_empty() {
            None
        } else {
            horizon_mod_translator::derive::Game::open(&game_refs).ok()
        };
        if let Some(game) = &game {
            let constant = |field: &str| probe.constant(field);
            let reads = |owner: &str, name: &str, desc: &str| probe.reads(owner, name, desc);
            let translated = |c: &str| game_part.contains_key(c);
            let mut deriver = horizon_mod_translator::derive::Deriver::new(
                game,
                horizon_mod_translator::derive::Mapped {
                    method: &asks,
                    field: &reads,
                    constant: &constant,
                    translated: &translated,
                },
            );
            missing.retain(|(n, k)| {
                let mut parts = k.splitn(3, '.');
                let (Some(owner), Some(name), Some(desc)) =
                    (parts.next(), parts.next(), parts.next())
                else {
                    return true;
                };
                let m = horizon_mod_translator::class::Member {
                    owner: owner.to_owned(),
                    name: name.to_owned(),
                    desc: desc.to_owned(),
                };
                let blockers = deriver.blockers(owner, name, desc, !statics_called.contains(&m));
                if blockers.is_empty() {
                    stands += 1;
                    stand_sites += n;
                    derive_lines.push(format!("  derivable {n:5} {k}"));
                } else {
                    derive_lines.push(format!(
                        "  blocked {n:5} {k} <- {}",
                        blockers
                            .iter()
                            .map(String::as_str)
                            .collect::<Vec<_>>()
                            .join(" | ")
                    ));
                }
                !blockers.is_empty()
            });
        }
        mapped += derived + stands;
        mapped_sites += derived_sites + stand_sites;
        println!(
            "  mapped: {mapped} of {} API methods ({:.1}%), {mapped_sites} of {sites} class uses ({:.1}%)",
            calls.len(),
            100.0 * mapped as f64 / calls.len().max(1) as f64,
            100.0 * mapped_sites as f64 / sites.max(1) as f64
        );
        if !game_refs.is_empty() {
            println!(
                "  by the game's own code: {derived} methods of classes translated with the mod ({derived_sites} class uses), {stands} standing for unmapped ones ({stand_sites} class uses)"
            );
        }
        // MISSING=1 lists the 60 most called gaps; MISSING=all every one.
        if let Some(m) = std::env::var_os("MISSING") {
            let limit = if m == "all" { usize::MAX } else { 60 };
            for (n, k) in missing.iter().take(limit) {
                println!("  missing {n:5} {k}");
            }
        }
        if std::env::var_os("DERIVE").is_some() {
            for l in &derive_lines {
                println!("{l}");
            }
        }
        if !game_part.is_empty() {
            // What that code calls of the API the runtime maps, asked as the mod's calls are.
            let (mut mapped, mut sites, mut mapped_sites) = (0usize, 0usize, 0usize);
            let mut missing: Vec<(usize, String)> = Vec::new();
            for (m, (n, is_static)) in &theirs {
                sites += n;
                if asks(m, *is_static) {
                    mapped += 1;
                    mapped_sites += n;
                } else {
                    missing.push((*n, format!("{}.{}.{}", m.owner, m.name, m.desc)));
                }
            }
            missing.sort_by_key(|m| std::cmp::Reverse(m.0));
            println!(
                "  game code: {} classes; mapped {mapped} of {} API methods it calls ({:.1}%), {mapped_sites} of {sites} class uses ({:.1}%)",
                own_game.len(),
                theirs.len(),
                100.0 * mapped as f64 / theirs.len().max(1) as f64,
                100.0 * mapped_sites as f64 / sites.max(1) as f64
            );
            if std::env::var_os("MISSING").is_some() {
                for (n, k) in &missing {
                    println!("  game-missing {n:5} {k}");
                }
            }
        }
        // LIBS=1: the calls into the libraries every mod has (the JDK, Guava, fastutil, JOML...),
        // which the runtime answers as it answers the API's, measured the same way: on null, a
        // bare object, what the mod makes of the class (its constructors, and the API's or the
        // libraries' methods returning it), and a sample of the library's type (an `ArrayList`
        // for a `List`). Listed as `lib-missing`.
        if std::env::var_os("LIBS").is_some() {
            let libs = horizon_mod_translator::library_calls(&runtime);
            let made_lib = if hosted {
                let mut both = calls.clone();
                both.extend(libs.iter().map(|(m, n)| (m.clone(), *n)));
                made_receivers(&both, &statics_called, |c| horizon_mod_translator::library_of(c).is_some())
            } else {
                Default::default()
            };
            let (mut mapped, mut sites, mut mapped_sites) = (0usize, 0usize, 0usize);
            let mut missing: Vec<(usize, String)> = Vec::new();
            // What the Java runtime's own code answers, as a bundle made with the install takes
            // it: a class of it translated with the mod (`jdk_code`), and a static method of a
            // class the runtime models whose code stands over what it maps (`derive`).
            let jdk_classes: std::collections::HashSet<&str> = with_game
                .iter()
                .map(|c| c.name.as_str())
                .filter(|n| horizon_mod_translator::jdk_code(n))
                .collect();
            let (constant, reads, translated) = (
                |field: &str| probe.constant(field),
                |owner: &str, name: &str, desc: &str| probe.reads(owner, name, desc),
                |c: &str| game_part.contains_key(c) || jdk_classes.contains(c),
            );
            let mut jdk_deriver = game.as_ref().map(|game| {
                horizon_mod_translator::derive::Deriver::new(
                    game,
                    horizon_mod_translator::derive::Mapped {
                        method: &asks,
                        field: &reads,
                        constant: &constant,
                        translated: &translated,
                    },
                )
            });
            let (mut jdk_code, mut jdk_stands) = (0usize, 0usize);
            for (m, n) in &libs {
                let key: &'static str =
                    Box::leak(format!("{}.{}.{}", m.owner, m.name, m.desc).into_boxed_str());
                let is_static = statics_called.contains(m);
                let arity = horizon_mod_translator::codegen::arg_kinds(&m.desc).len()
                    + usize::from(!is_static);
                sites += n;
                if jdk_classes.contains(m.owner.as_str()) {
                    jdk_code += 1;
                    mapped += 1;
                    mapped_sites += n;
                    continue;
                }
                let owner: &'static str = Box::leak(m.owner.clone().into_boxed_str());
                let mut receivers = vec![Receiver::Null, Receiver::Bare, Receiver::Init];
                receivers.extend(made_lib.get(owner).map_or(&[][..], Vec::as_slice).iter().copied());
                let fills: &[Args] = if hosted { &[Args::Null, Args::Instance] } else { &[Args::Null] };
                let answered = if is_static {
                    answers(key, owner, arity, Receiver::Null, Args::Null, true) || (hosted && answers(key, owner, arity, Receiver::Null, Args::Static, true))
                } else {
                    fills.iter().any(|&fill| receivers.iter().any(|&r| answers(key, owner, arity, r, fill, true)))
                        || (hosted && answers(key, owner, arity, Receiver::Sample, Args::Instance, true))
                };
                let stands = !answered
                    && is_static
                    && horizon_mod_translator::derive::is_library(&m.owner)
                    && jdk_deriver
                        .as_mut()
                        .is_some_and(|d| d.blockers(&m.owner, &m.name, &m.desc, false).is_empty());
                if answered || stands {
                    jdk_stands += usize::from(stands);
                    mapped += 1;
                    mapped_sites += n;
                } else {
                    missing.push((*n, key.to_owned()));
                }
            }
            missing.sort_by(|a, b| b.0.cmp(&a.0));
            println!(
                "  libraries: mapped {mapped} of {} library methods ({:.1}%), {mapped_sites} of {sites} class uses ({:.1}%)",
                libs.len(),
                100.0 * mapped as f64 / libs.len().max(1) as f64,
                100.0 * mapped_sites as f64 / sites.max(1) as f64
            );
            if game.is_some() {
                println!(
                    "  by the Java runtime's own code: {jdk_code} methods of classes translated with the mod, {jdk_stands} static methods standing for unmapped ones"
                );
            }
            if std::env::var_os("MISSING").is_some() {
                for (n, k) in &missing {
                    println!("  lib-missing {n:5} {k}");
                }
            }
        }
        let methods: usize = classes.iter().map(|c| c.methods.len()).sum();
        println!("{jar}: {} classes, {methods} methods, {} distinct API methods called", classes.len(), calls.len());
        let mut top: Vec<_> = calls.into_iter().collect();
        top.sort_by(|a, b| b.1.cmp(&a.1));
        for (m, n) in top.iter().take(15) {
            println!("  {n:5}  {}.{}{}", m.owner, m.name, m.desc);
        }
        let regs = horizon_mod_translator::registrations(&classes);
        let mut by: std::collections::BTreeMap<&str, usize> = Default::default();
        for r in &regs {
            *by.entry(r.registry.as_str()).or_default() += 1;
        }
        println!("  registrations in code: {} {by:?}", regs.len());
        for r in regs.iter().take(5) {
            println!("    {} {}", r.registry, r.name);
        }
    }
    ExitCode::SUCCESS
}
