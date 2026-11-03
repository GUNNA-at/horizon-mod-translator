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
    let family = |pkg: &str, suffix: &str| c.starts_with(pkg) && c.ends_with(suffix);
    match class {
        "net/minecraft/world/level/block/Block" | "net/minecraft/world/level/block/state/BlockBehaviour" => {
            family("net/minecraft/world/level/block/", "Block") && !c.contains("/entity/")
        }
        "net/minecraft/world/level/block/EntityBlock" => c == "net/minecraft/world/level/block/BaseEntityBlock",
        "net/minecraft/world/phys/shapes/CollisionContext" => c == crate::shapes::ENTITY_COLLISION_CONTEXT,
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
        "net/minecraft/world/entity/Entity" => c.starts_with("net/minecraft/world/entity/") && !c.contains('$'),
        "java/lang/Throwable" => c.starts_with("java/lang/") && (c.ends_with("Exception") || c.ends_with("Error")),
        "java/lang/Exception" => c.starts_with("java/lang/") && c.ends_with("Exception"),
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
        }
        _ => false,
    }
}

pub fn direction(rt: &mut Rt, ordinal: i64) -> V {
    let o = ordinal.clamp(0, 5);
    rt.intern(format!("dir/{o}"), |rt| {
        rt.alloc("net/minecraft/core/Direction", Data::Native("Direction", [o, 0, 0, 0]))
    })
}

pub fn axis(rt: &mut Rt, ordinal: i64) -> V {
    let o = ordinal.clamp(0, 2);
    rt.intern(format!("axis/{o}"), |rt| {
        rt.alloc("net/minecraft/core/Direction$Axis", Data::Native("Axis", [o, 0, 0, 0]))
    })
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
    let text = text.to_owned();
    rt.intern(format!("state/{text}"), move |rt| {
        rt.alloc("net/minecraft/world/level/block/state/BlockState", Data::Str(text))
    })
}

pub fn item(rt: &mut Rt, name: &str) -> V {
    let name = if name.contains(':') { name.to_owned() } else { format!("minecraft:{name}") };
    if let Some(v) = rt.registered.get(&("item".to_owned(), name.clone())) {
        return *v;
    }
    rt.intern(format!("item/{name}"), move |rt| rt.alloc("net/minecraft/world/item/Item", Data::Str(name)))
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

/// A block-state property: its name and kind (`bool`, `int`, `direction`, `axis`, or an enum's class).
fn new_property(rt: &mut Rt, name: &str, kind: &str) -> V {
    let p = rt.new_object("net/minecraft/world/level/block/state/properties/Property");
    let n = rt.string(name);
    let k = rt.string(kind);
    rt.put_field(p, "$name", n);
    rt.put_field(p, "$kind", k);
    p
}

/// Vanilla's property in `BlockStateProperties.<field>`: one object per field, as Java's (`FACING`
/// and `HORIZONTAL_FACING` share a name, not their values).
fn vanilla_property_object(rt: &mut Rt, field: &str) -> Option<V> {
    let (name, kind) = vanilla_property(field)?;
    let f = field.to_owned();
    Some(rt.intern(format!("prop/field/{field}"), move |rt| {
        let p = new_property(rt, &name, kind);
        let fv = rt.string(&f);
        rt.put_field(p, "$field", fv);
        p
    }))
}

/// The `BlockStateProperties` field that vanilla block class `class`'s property field `field` holds:
/// most keep the name (`DoorBlock.OPEN` is `OPEN`); these don't.
fn block_class_property<'a>(class: &str, field: &'a str) -> &'a str {
    let simple = class.rsplit('/').next().unwrap_or(class);
    // Classes whose `FACING` is `HORIZONTAL_FACING` (`DirectionalBlock`'s and the rest are `FACING`).
    const HORIZONTAL: [&str; 33] = [
        "HorizontalDirectionalBlock", "FaceAttachedHorizontalDirectionalBlock", "LeverBlock", "ButtonBlock",
        "GrindstoneBlock", "BellBlock", "ChestBlock", "EnderChestBlock", "TrappedChestBlock", "LadderBlock",
        "TrapDoorBlock", "DoorBlock", "StairBlock", "FenceGateBlock", "AbstractFurnaceBlock", "FurnaceBlock",
        "BlastFurnaceBlock", "SmokerBlock", "CampfireBlock", "LecternBlock", "BedBlock", "AnvilBlock",
        "WallSignBlock", "WallBannerBlock", "StonecutterBlock", "LoomBlock", "BeehiveBlock", "DiodeBlock",
        "RepeaterBlock", "ComparatorBlock", "CocoaBlock", "CarvedPumpkinBlock", "WallTorchBlock",
    ];
    match (simple, field) {
        ("SlabBlock", "TYPE") => "SLAB_TYPE",
        ("ChestBlock" | "TrappedChestBlock", "TYPE") => "CHEST_TYPE",
        ("DoorBlock" | "DoublePlantBlock" | "TallFlowerBlock" | "SmallDripleafBlock", "HALF") => "DOUBLE_BLOCK_HALF",
        ("DoorBlock", "HINGE") => "DOOR_HINGE",
        ("StairBlock", "SHAPE") => "STAIRS_SHAPE",
        ("FaceAttachedHorizontalDirectionalBlock" | "LeverBlock" | "ButtonBlock" | "GrindstoneBlock", "FACE") => "ATTACH_FACE",
        ("CropBlock" | "StemBlock", "AGE") => "AGE_7",
        ("BeetrootBlock" | "NetherWartBlock" | "SweetBerryBushBlock", "AGE") => "AGE_3",
        ("CocoaBlock", "AGE") => "AGE_2",
        ("CactusBlock" | "SugarCaneBlock" | "FireBlock", "AGE") => "AGE_15",
        ("BeehiveBlock", "HONEY_LEVEL") => "LEVEL_HONEY",
        ("RedStoneWireBlock", "NORTH") => "NORTH_REDSTONE",
        ("RedStoneWireBlock", "EAST") => "EAST_REDSTONE",
        ("RedStoneWireBlock", "SOUTH") => "SOUTH_REDSTONE",
        ("RedStoneWireBlock", "WEST") => "WEST_REDSTONE",
        ("ComparatorBlock", "MODE") => "MODE_COMPARATOR",
        (s, "FACING") if HORIZONTAL.contains(&s) => "HORIZONTAL_FACING",
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
                | "STRUCTUREBLOCK_MODE" | "BELL_ATTACHMENT" | "NOTEBLOCK_INSTRUMENT" | "DRIPSTONE_THICKNESS"
                | "BAMBOO_LEAVES" | "NORTH_WALL" | "EAST_WALL" | "SOUTH_WALL" | "WEST_WALL" | "NORTH_REDSTONE"
                | "EAST_REDSTONE" | "SOUTH_REDSTONE" | "WEST_REDSTONE" | "SCULK_SENSOR_PHASE" | "TILT" | "ORIENTATION"
        )
}

/// Vanilla's `BlockStateProperties` fields: (field, property name, kind).
pub fn vanilla_property(field: &str) -> Option<(String, &'static str)> {
    let kind = match field {
        "FACING" | "HORIZONTAL_FACING" | "FACING_HOPPER" | "VERTICAL_DIRECTION" => "direction",
        "AXIS" | "HORIZONTAL_AXIS" => "axis",
        "AGE_1" | "AGE_2" | "AGE_3" | "AGE_4" | "AGE_5" | "AGE_7" | "AGE_15" | "AGE_25" | "POWER" | "LEVEL"
        | "LEVEL_CAULDRON" | "LEVEL_COMPOSTER" | "LEVEL_FLOWING" | "LEVEL_HONEY" | "DISTANCE" | "MOISTURE"
        | "ROTATION_16" | "STAGE" | "DELAY" | "NOTE" | "LAYERS" | "BITES" | "PICKLES" | "EGGS" | "HATCH"
        | "CANDLES" | "RESPAWN_ANCHOR_CHARGES" | "STABILITY_DISTANCE" | "DUSTED" | "FLOWER_AMOUNT"
        | "SEGMENT_AMOUNT" => "int",
        "WATERLOGGED" | "POWERED" | "LIT" | "OPEN" | "OCCUPIED" | "ATTACHED" | "DISARMED" | "EXTENDED"
        | "ENABLED" | "HANGING" | "PERSISTENT" | "SNOWY" | "TRIGGERED" | "UNSTABLE" | "UP" | "DOWN" | "NORTH"
        | "SOUTH" | "EAST" | "WEST" | "BOTTOM" | "CONDITIONAL" | "DRAG" | "HAS_BOOK" | "IN_WALL" | "LOCKED"
        | "SHORT" | "SIGNAL_FIRE" | "INVERTED" | "BERRIES" | "BLOOM" | "SHRIEKING" | "CAN_SUMMON" | "CRACKED"
        | "NATURAL" | "OMINOUS" | "TIP" | "EYE" | "HAS_RECORD" | "IN_WATER" | "ENABLED_POWER" => "bool",
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
        "MODE_COMPARATOR" | "STRUCTUREBLOCK_MODE" => "mode".to_owned(),
        "BELL_ATTACHMENT" => "attachment".to_owned(),
        "NOTEBLOCK_INSTRUMENT" => "instrument".to_owned(),
        "DRIPSTONE_THICKNESS" => "thickness".to_owned(),
        "BAMBOO_LEAVES" => "leaves".to_owned(),
        "NORTH_WALL" | "NORTH_REDSTONE" => "north".to_owned(),
        "EAST_WALL" | "EAST_REDSTONE" => "east".to_owned(),
        "SOUTH_WALL" | "SOUTH_REDSTONE" => "south".to_owned(),
        "WEST_WALL" | "WEST_REDSTONE" => "west".to_owned(),
        "HORIZONTAL_FACING" | "FACING_HOPPER" => "facing".to_owned(),
        "HORIZONTAL_AXIS" => "axis".to_owned(),
        "VERTICAL_DIRECTION" => "vertical_direction".to_owned(),
        f if f.starts_with("AGE_") => "age".to_owned(),
        "LEVEL_CAULDRON" | "LEVEL_COMPOSTER" | "LEVEL_FLOWING" | "LEVEL_HONEY" => "level".to_owned(),
        "ROTATION_16" => "rotation".to_owned(),
        "RESPAWN_ANCHOR_CHARGES" => "charges".to_owned(),
        "STABILITY_DISTANCE" => "distance".to_owned(),
        f => f.to_ascii_lowercase(),
    };
    Some((name, kind))
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

/// A Java property value's text in a state.
pub(crate) fn text_of(rt: &Rt, v: V) -> String {
    match rt.class_of(v) {
        "java/lang/Boolean" => (rt.get_field(v, "value").i() != 0).to_string(),
        "java/lang/Integer" => rt.get_field(v, "value").i().to_string(),
        "net/minecraft/core/Direction" => DIRECTION_NAMES[native(rt, v)[0].clamp(0, 5) as usize].to_owned(),
        "net/minecraft/core/Direction$Axis" => AXIS_NAMES[native(rt, v)[0].clamp(0, 2) as usize].to_owned(),
        _ => match rt.str_of(v) {
            Some(s) => s.to_owned(),
            None => rt.str_of(rt.get_field(v, "$name")).unwrap_or("").to_ascii_lowercase(),
        },
    }
}

pub(crate) fn prop_parts(rt: &Rt, p: V) -> (String, String) {
    (name_of(rt, rt.get_field(p, "$name")), name_of(rt, rt.get_field(p, "$kind")))
}

fn vec3(rt: &mut Rt, x: f64, y: f64, z: f64) -> V {
    let v = rt.new_object("net/minecraft/world/phys/Vec3");
    rt.put_field(v, "x", V::D(x));
    rt.put_field(v, "y", V::D(y));
    rt.put_field(v, "z", V::D(z));
    v
}

fn xyz(rt: &Rt, v: V) -> [f64; 3] {
    [rt.get_field(v, "x").d(), rt.get_field(v, "y").d(), rt.get_field(v, "z").d()]
}

fn compound(rt: &mut Rt) -> V {
    rt.alloc("net/minecraft/nbt/CompoundTag", Data::Map(HashMap::new()))
}


/// Writes `stack` into the runtime compound `tag` as `ItemStack.CODEC` does: `id`, `count`.
pub(crate) fn save_stack(rt: &mut Rt, stack: V, tag: V) {
    let id = item_like_name(rt, rt.get_field(stack, "$item"));
    let count = rt.get_field(stack, "$count").i();
    let (ik, iv, ck) = (rt.string("id"), rt.string(&id), rt.string("count"));
    crate::api::call(rt, "net/minecraft/nbt/CompoundTag.putString.(Ljava/lang/String;Ljava/lang/String;)V", &[tag, ik, iv]);
    crate::api::call(rt, "net/minecraft/nbt/CompoundTag.putInt.(Ljava/lang/String;I)V", &[tag, ck, V::I(count)]);
}

/// A stack from its saved form (`{id, count}`); None for an empty or unknown item, as
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
    Some(stack_of(rt, i, count))
}

pub(crate) fn stack(rt: &mut Rt, item_name: &str, count: i32) -> V {
    let s = rt.new_object("net/minecraft/world/item/ItemStack");
    let i = item(rt, item_name);
    rt.put_field(s, "$item", i);
    rt.put_field(s, "$count", V::I(count));
    s
}

/// Whether a stack is empty: none, no count, or air. A mod's item is its registered object.
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
pub(crate) fn item_like_name(rt: &Rt, v: V) -> String {
    rt.str_of(v)
        .map(str::to_owned)
        .or_else(|| rt.registered_name(v, "item").map(str::to_owned))
        .or_else(|| rt.registered_name(v, "block").map(str::to_owned))
        .unwrap_or_else(|| "minecraft:air".into())
}

/// The next value of the runtime's random stream (xorshift64*).
fn next_random(rt: &mut Rt) -> u64 {
    let mut x = rt.random_state.max(1);
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    rt.random_state = x;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

pub fn call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    const STATE: &str = "net/minecraft/world/level/block/state/BlockState";
    const BASE: &str = "net/minecraft/world/level/block/state/BlockBehaviour$BlockStateBase";
    const HOLDER: &str = "net/minecraft/world/level/block/state/StateHolder";
    Some(match (owner, name) {
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
                    value_of(rt, &kind, current.as_deref().unwrap_or(""))
                }
                "getOptionalValue" => {
                    let v = current.map(|c| value_of(rt, &kind, &c)).unwrap_or(V::NULL);
                    let o = rt.new_object("java/util/Optional");
                    rt.put_field(o, "value", v);
                    o
                }
                "setValue" | "trySetValue" => {
                    if current.is_none() && name == "trySetValue" {
                        return Some(a(0));
                    }
                    let new = text_of(rt, a(2));
                    let mut p: Vec<(&str, &str)> = props.iter().filter(|(k, _)| *k != pname).copied().collect();
                    p.push((&pname, &new));
                    let t = crate::host::join_state(block, p);
                    state(rt, &t)
                }
                _ => {
                    // cycle: booleans flip, ints and enums need their range, which only the game knows.
                    let c = current.unwrap_or_default();
                    let next = match c.as_str() {
                        "true" => "false".to_owned(),
                        "false" => "true".to_owned(),
                        _ => return None,
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
        (_, "getName") if owner.starts_with("net/minecraft/world/level/block/state/properties/") => rt.get_field(a(0), "$name"),
        ("java/lang/Boolean", "valueOf") if desc == "(Z)Ljava/lang/Boolean;" => boolean(rt, a(0).i() != 0),
        ("java/lang/Boolean", "booleanValue") => rt.get_field(a(0), "value"),
        ("java/lang/Integer", "valueOf") if desc == "(I)Ljava/lang/Integer;" => integer(rt, a(0).i()),
        ("java/lang/Enum", "<init>") => {
            let n = a(1);
            rt.put_field(a(0), "$name", n);
            rt.put_field(a(0), "$ordinal", a(2));
            V::Void
        }
        ("java/lang/Enum", "name" | "toString") => rt.get_field(a(0), "$name"),
        ("java/lang/Enum", "ordinal") => V::I(rt.get_field(a(0), "$ordinal").i()),
        ("java/util/Optional", "isPresent") => V::I((rt.get_field(a(0), "value").r() != 0) as i32),
        ("java/util/Optional", "get" | "orElseThrow") => rt.get_field(a(0), "value"),
        ("java/util/Optional", "orElse") => {
            let v = rt.get_field(a(0), "value");
            if v.r() != 0 { v } else { a(1) }
        }

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
        ("net/minecraft/core/Direction", "getName" | "getSerializedName" | "toString") => {
            let n = DIRECTION_NAMES[native(rt, a(0))[0].clamp(0, 5) as usize];
            rt.string(n)
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
        ("net/minecraft/core/Direction", "getNearest") if args.len() >= 3 => {
            let f = |v: V| match v {
                V::I(i) => i as f32,
                V::F(x) => x as f32,
                V::D(x) => x as f32,
                V::J(j) => j as f32,
                _ => 0.0,
            };
            let (x, y, z) = (f(a(0)), f(a(1)), f(a(2)));
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
        (o, n) if o.starts_with("net/minecraft/") && (o.ends_with("/Player") || o.ends_with("/ServerPlayer") || o.ends_with("/LivingEntity") || o.ends_with("/Entity")) && matches!(n, "isShiftKeyDown" | "isCrouching" | "isSteppingCarefully" | "isCreative" | "isSpectator" | "getItemInHand" | "getMainHandItem" | "level" | "isLocalPlayer" | "mayBuild") => {
            match n {
                // Survival and creative players build; the hooks act for one of them.
                "mayBuild" => V::I(1),
                "isShiftKeyDown" | "isCrouching" | "isSteppingCarefully" => V::I(rt.host().is_some_and(|h| h.player_sneaking()) as i32),
                "isCreative" => V::I(rt.host().is_some_and(|h| h.player_creative()) as i32),
                "isSpectator" | "isLocalPlayer" => V::I(0),
                "level" => rt.new_object("net/minecraft/server/level/ServerLevel"),
                _ => {
                    // The main hand (the hook's hand); the off hand is empty here.
                    let off = name == "getItemInHand" && rt.get_field(a(1), "$ordinal").i() == 1;
                    match rt.host().and_then(|h| h.player_held()).filter(|_| !off) {
                        Some((id, count)) => stack(rt, &id, count as i32),
                        None => get_static(rt, "net/minecraft/world/item/ItemStack", "EMPTY").unwrap_or(V::NULL),
                    }
                }
            }
        }
        // The main hand changes: what it held is taken, the new stack given (the off hand is
        // empty here, and stays so).
        (o, "setItemInHand") if is_player(o) => {
            if rt.get_field(a(1), "$ordinal").i() != 1 {
                if let Some((id, n)) = rt.host().and_then(|h| h.player_held())
                    && let Some(h) = rt.host_mut()
                {
                    h.take_player_items(&id, n);
                }
                give_player(rt, a(2));
            }
            V::Void
        }
        (o, "getInventory") if is_player(o) => {
            let inv = rt.new_object(INVENTORY);
            rt.put_field(inv, "player", a(0));
            inv
        }
        (INVENTORY, "player") => rt.get_field(a(0), "player"),
        // Into the player's inventory, the stack shrunk by what went in. What doesn't fit stays
        // in the stack (vanilla's `placeItemBackInInventory` would drop it at the player's feet).
        (INVENTORY, "placeItemBackInInventory" | "add") => {
            let stack = if desc.starts_with("(I") { a(2) } else { a(1) };
            let all = give_player(rt, stack);
            if name == "placeItemBackInInventory" { V::Void } else { V::I(all as i32) }
        }
        (o, "addItem") if is_player(o) => V::I(give_player(rt, a(1)) as i32),
        // An item's `useOn` that reaches vanilla's `Item`: nothing happens.
        ("net/minecraft/world/item/Item", "useOn") => result(rt, "net/minecraft/world/InteractionResult", "PASS"),
        ("net/minecraft/world/InteractionResult" | "net/minecraft/world/ItemInteractionResult", "sidedSuccess") => {
            let client = a(0).i() != 0;
            result(rt, owner, if client { "SUCCESS" } else { "CONSUME" })
        }
        ("net/minecraft/world/InteractionResult" | "net/minecraft/world/ItemInteractionResult", "consumesAction") => {
            let n = name_of(rt, a(0));
            V::I(matches!(n.as_str(), "SUCCESS" | "CONSUME" | "SUCCESS_NO_ITEM_USED" | "CONSUME_PARTIAL" | "SKIP_DEFAULT_BLOCK_INTERACTION") as i32)
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
            // BlockHitResult) or (UseOnContext): the fields the getters read.
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
            } else if desc.starts_with("(Lnet/minecraft/world/entity/player/Player;") {
                let player = a(1);
                let stack = call(rt, "net/minecraft/world/entity/player/Player", "getItemInHand", "(Lnet/minecraft/world/InteractionHand;)Lnet/minecraft/world/item/ItemStack;", &[player, a(2)]).unwrap_or(V::NULL);
                (crate::glue::level(rt), player, a(2), stack, a(3))
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
        ("net/minecraft/core/Direction", "fromYRot") => direction(rt, horizontal_facing(a(0).d() as f32)),
        // The acting player's look (an object `glue` made carries it; others look south, level).
        (o, n) if o.starts_with("net/minecraft/") && (o.ends_with("/Player") || o.ends_with("/ServerPlayer") || o.ends_with("/LivingEntity") || o.ends_with("/Entity")) && matches!(n, "getYRot" | "getXRot" | "getViewYRot" | "getViewXRot" | "getYHeadRot" | "getVisualRotationYInDegrees" | "getDirection" | "getMotionDirection") => {
            let (yaw, pitch) = (rt.get_field(a(0), "$yaw").f(), rt.get_field(a(0), "$pitch").f());
            match n {
                "getXRot" | "getViewXRot" => V::F(pitch),
                "getDirection" | "getMotionDirection" => direction(rt, horizontal_facing(yaw)),
                _ => V::F(yaw),
            }
        }
        ("net/minecraft/util/Mth", "sin") => V::F(mth_sin(a(0))),
        ("net/minecraft/util/Mth", "cos") => V::F(mth_cos(a(0))),
        // Block entities
        // Block entities: the API's state lives in the fields Java names (`level`, `worldPosition`,
        // `blockState`, `remove`), which mod subclasses read directly.
        ("net/minecraft/world/level/block/entity/BlockEntity", "<init>") => {
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
        (o, "setChanged" | "onLoad" | "onChunkUnloaded" | "invalidateCapabilities") if o.ends_with("BlockEntity") => V::Void,
        // The base class saves and loads nothing of its own beyond what the glue writes (id, position).
        (o, "saveAdditional" | "loadAdditional" | "requestModelDataUpdate" | "onDataPacket") if o.ends_with("BlockEntity") => V::Void,
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
        ("net/minecraft/world/level/block/entity/BlockEntityType$Builder", "of") => {
            let b = rt.new_object("net/minecraft/world/level/block/entity/BlockEntityType$Builder");
            rt.put_field(b, "$factory", a(0));
            b
        }
        ("net/minecraft/world/level/block/entity/BlockEntityType$Builder", "build") => {
            let t = rt.new_object("net/minecraft/world/level/block/entity/BlockEntityType");
            let f = rt.get_field(a(0), "$factory");
            rt.put_field(t, "$factory", f);
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
        (o, "createTickerHelper") if o.ends_with("BaseEntityBlock") => {
            if a(0) == a(1) { a(2) } else { V::NULL }
        }
        (o, "getBlockEntity") if o.starts_with("net/minecraft/") && (o.ends_with("Level") || o.ends_with("LevelAccessor") || o.ends_with("BlockGetter") || o.ends_with("LevelReader")) => {
            let h = native(rt, a(1));
            rt.block_entities.get(&[h[0] as i32, h[1] as i32, h[2] as i32]).copied().unwrap_or(V::NULL)
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
                    if l < 1.0e-4 { vec3(rt, 0.0, 0.0, 0.0) } else { vec3(rt, p[0] / l, p[1] / l, p[2] / l) }
                }
                ("dot", _) => {
                    let q = xyz(rt, a(1));
                    V::D(p[0] * q[0] + p[1] * q[1] + p[2] * q[2])
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
        ("net/minecraft/world/item/ItemStack", "getItem") => {
            if stack_empty(rt, a(0)) { item(rt, "minecraft:air") } else { rt.get_field(a(0), "$item") }
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
        ("net/minecraft/world/item/ItemStack", "isStackable") => V::I(1),
        // NBT in `ItemStack.CODEC`'s form, `{id, count}` (components aren't kept yet).
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
        ("net/minecraft/world/item/ItemStack", "getMaxStackSize") => V::I(64),

        // Maths, randomness, text
        ("net/minecraft/util/Mth", "clamp") => match desc {
            "(III)I" => V::I(a(0).i().clamp(a(1).i(), a(2).i().max(a(1).i()))),
            "(FFF)F" => V::F(if a(0).f() < a(1).f() { a(1).f() } else { a(0).f().min(a(2).f()) }),
            "(DDD)D" => V::D(if a(0).d() < a(1).d() { a(1).d() } else { a(0).d().min(a(2).d()) }),
            "(JJJ)J" => V::J(a(0).j().clamp(a(1).j(), a(2).j().max(a(1).j()))),
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
        ("net/minecraft/util/Mth", "sign") => V::I(if a(0).d() > 0.0 { 1 } else if a(0).d() < 0.0 { -1 } else { 0 }),
        ("net/minecraft/util/RandomSource", n) => {
            let r = next_random(rt);
            match (n, desc) {
                ("nextFloat", _) => V::F((r >> 40) as f32 / (1u64 << 24) as f32),
                ("nextDouble", _) => V::D((r >> 11) as f64 / (1u64 << 53) as f64),
                ("nextBoolean", _) => V::I((r >> 63) as i32),
                ("nextInt", "(I)I") => V::I(((r >> 33) % a(1).i().max(1) as u64) as i32),
                ("nextInt", "(II)I") => V::I(a(1).i() + ((r >> 33) % (a(2).i() - a(1).i()).max(1) as u64) as i32),
                ("nextInt", "()I") => V::I((r >> 32) as i32),
                _ => return None,
            }
        }
        ("net/minecraft/network/chat/Component", "literal" | "translatable") => {
            let t = rt.new_object("net/minecraft/network/chat/MutableComponent");
            rt.put_field(t, "$text", a(0));
            t
        }
        ("net/minecraft/network/chat/Component" | "net/minecraft/network/chat/MutableComponent", "getString") => rt.get_field(a(0), "$text"),
        _ => return None,
    })
}

const INVENTORY: &str = "net/minecraft/world/entity/player/Inventory";

/// Whether `owner` is the player or one of the classes a player's methods are called through.
fn is_player(owner: &str) -> bool {
    owner.starts_with("net/minecraft/") && (owner.ends_with("/Player") || owner.ends_with("/ServerPlayer") || owner.ends_with("/LivingEntity") || owner.ends_with("/Entity"))
}

/// Gives the acting player `stack` (where it fits, as `Inventory.add` places it), shrinking it by
/// what went in; whether all of it did.
fn give_player(rt: &mut Rt, stack: V) -> bool {
    if stack.r() == 0 || stack_empty(rt, stack) {
        return true;
    }
    let item = item_like_name(rt, rt.get_field(stack, "$item"));
    let count = rt.get_field(stack, "$count").i().max(0) as u32;
    let gave = rt.host_mut().map_or(0, |h| h.give_player_items(&item, count));
    rt.put_field(stack, "$count", V::I((count - gave.min(count)) as i32));
    gave >= count
}

/// An `InteractionResult` / `ItemInteractionResult` constant, by name (interned: compared by identity).
pub fn result(rt: &mut Rt, class: &str, name: &str) -> V {
    let (class, name) = (class.to_owned(), name.to_owned());
    rt.intern(format!("result/{class}/{name}"), move |rt| {
        let c: &'static str = rt.static_key(&class);
        rt.alloc(c, Data::Str(name))
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

/// Vanilla's `Mth.sin`: a 65536-entry table of `Math.sin`, so angles round as the game rounds them.
fn mth_sin(v: V) -> f32 {
    let x = match v {
        V::D(d) => d,
        other => other.f() as f64,
    };
    let i = ((x * 10430.378350470453) as i64 & 65535) as f64;
    (i * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32
}

fn mth_cos(v: V) -> f32 {
    let x = match v {
        V::D(d) => d,
        other => other.f() as f64,
    };
    let i = ((x * 10430.378350470453 + 16384.0) as i64 & 65535) as f64;
    (i * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32
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

fn context(rt: &mut Rt, p: &crate::glue::Placement, class: &'static str) -> V {
    let ctx = rt.new_object(class);
    let level = crate::glue::level(rt);
    let player = rt.new_object("net/minecraft/server/level/ServerPlayer");
    rt.put_field(player, "$yaw", V::F(p.yaw));
    rt.put_field(player, "$pitch", V::F(p.pitch));
    let hand = get_static(rt, "net/minecraft/world/InteractionHand", "MAIN_HAND").unwrap_or(V::NULL);
    let item = if p.item.is_empty() { get_static(rt, "net/minecraft/world/item/ItemStack", "EMPTY").unwrap_or(V::NULL) } else { stack(rt, &p.item, 1) };
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
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    Some(match owner {
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
        o if o.starts_with("net/minecraft/world/level/block/") && !o[32..].contains('/') && state_property_field(block_class_property(o, field)) => {
            vanilla_property_object(rt, block_class_property(o, field))?
        }
        "net/minecraft/world/item/ItemStack" if field == "EMPTY" => {
            rt.intern("stack/empty".into(), |rt| stack(rt, "minecraft:air", 0))
        }
        "net/minecraft/world/item/Items" => item(rt, &field.to_ascii_lowercase()),
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
    use crate::{Rt, V, api};

    const GET: &str = "net/minecraft/world/level/block/state/BlockState.getValue.(Lnet/minecraft/world/level/block/state/properties/Property;)Ljava/lang/Comparable;";
    const SET: &str = "net/minecraft/world/level/block/state/BlockState.setValue.(Lnet/minecraft/world/level/block/state/properties/Property;Ljava/lang/Comparable;)Ljava/lang/Object;";

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

        let stone = api::get_static(&mut rt, "net/minecraft/world/item/Items.STONE");
        let s = rt.new_object("net/minecraft/world/item/ItemStack");
        api::call(&mut rt, "net/minecraft/world/item/ItemStack.<init>.(Lnet/minecraft/world/level/ItemLike;I)V", &[s, stone, V::I(3)]);
        api::call(&mut rt, "net/minecraft/world/item/ItemStack.shrink.(I)V", &[s, V::I(3)]);
        assert_eq!(api::call(&mut rt, "net/minecraft/world/item/ItemStack.isEmpty.()Z", &[s]), V::I(1));
        let empty = api::get_static(&mut rt, "net/minecraft/world/item/ItemStack.EMPTY");
        assert_eq!(api::call(&mut rt, "net/minecraft/world/item/ItemStack.isEmpty.()Z", &[empty]), V::I(1));
    }
}
