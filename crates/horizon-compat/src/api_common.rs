//! The most used API after the world: block-state properties, NBT
//! compounds, `Vec3`, item stacks, `Mth`, random sources, text components
//! and enums. Java compares many of these by identity (`state.getValue(FACING)
//! == Direction.NORTH`), so directions, axes, booleans, states, blocks and
//! items are interned: the same value is always the same object.
use crate::{Data, Rt, V};
use std::collections::HashMap;

const DIRECTION_NAMES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];
const AXIS_NAMES: [&str; 3] = ["x", "y", "z"];

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

fn name_of(rt: &Rt, v: V) -> String {
    rt.str_of(v).map(str::to_owned).unwrap_or_default()
}

/// A block-state property: its name and kind (`bool`, `int`, `direction`, `axis`, or an enum's class).
fn property(rt: &mut Rt, name: &str, kind: &str) -> V {
    let (name, kind) = (name.to_owned(), kind.to_owned());
    rt.intern(format!("prop/{name}/{kind}"), move |rt| {
        let p = rt.new_object("net/minecraft/world/level/block/state/properties/Property");
        let n = rt.string(&name);
        let k = rt.string(&kind);
        rt.put_field(p, "$name", n);
        rt.put_field(p, "$kind", k);
        p
    })
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
    let name = match field {
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
fn text_of(rt: &Rt, v: V) -> String {
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

fn prop_parts(rt: &Rt, p: V) -> (String, String) {
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

fn map_get(rt: &Rt, tag: V, key: &str) -> Option<V> {
    match rt.heap.get(tag.r() as usize).map(|o| &o.data) {
        Some(Data::Map(m)) => m.get(key).copied(),
        _ => None,
    }
}

fn map_put(rt: &mut Rt, tag: V, key: String, v: V) {
    if let Some(crate::Object { data: Data::Map(m), .. }) = rt.heap.get_mut(tag.r() as usize) {
        m.insert(key, v);
    }
}

fn stack(rt: &mut Rt, item_name: &str, count: i32) -> V {
    let s = rt.new_object("net/minecraft/world/item/ItemStack");
    let i = item(rt, item_name);
    rt.put_field(s, "$item", i);
    rt.put_field(s, "$count", V::I(count));
    s
}

fn stack_empty(rt: &Rt, s: V) -> bool {
    let i = rt.get_field(s, "$item");
    s.r() == 0 || rt.get_field(s, "$count").i() <= 0 || rt.str_of(i).is_none_or(|n| n == "minecraft:air")
}

/// The name an `ItemLike` (an Item, a Block, a holder) stands for.
fn item_like_name(rt: &Rt, v: V) -> String {
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
        // Block-state properties
        (STATE | BASE | HOLDER, "getValue" | "getOptionalValue" | "hasProperty" | "setValue" | "cycle" | "trySetValue") => {
            let text = name_of(rt, a(0));
            let (block, props) = crate::host::split_state(&text);
            let (pname, kind) = prop_parts(rt, a(1));
            let current = props.iter().find(|(k, _)| *k == pname).map(|(_, v)| v.to_string());
            match name {
                "hasProperty" => V::I(current.is_some() as i32),
                "getValue" => value_of(rt, &kind, current.as_deref().unwrap_or("")),
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
            property(rt, &pname, &kind)
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

        // NBT
        ("net/minecraft/nbt/CompoundTag", "<init>") => {
            if let Some(o) = rt.heap.get_mut(a(0).r() as usize) {
                o.data = Data::Map(HashMap::new());
            }
            V::Void
        }
        ("net/minecraft/nbt/CompoundTag", n) if n.starts_with("put") => {
            let key = name_of(rt, a(1));
            let kind = match n {
                "putByte" | "putBoolean" => crate::nbt::BYTE,
                "putShort" => crate::nbt::SHORT,
                "putInt" => crate::nbt::INT,
                "putLong" => crate::nbt::LONG,
                "putFloat" => crate::nbt::FLOAT,
                "putDouble" => crate::nbt::DOUBLE,
                "putString" => crate::nbt::STRING,
                _ => crate::nbt::TAG,
            };
            map_put(rt, a(0), crate::nbt::kind_key(&key), V::I(kind));
            map_put(rt, a(0), key, a(2));
            if n == "put" { a(2) } else { V::Void }
        }
        ("net/minecraft/nbt/CompoundTag", "contains") => {
            let key = name_of(rt, a(1));
            V::I(map_get(rt, a(0), &key).is_some() as i32)
        }
        ("net/minecraft/nbt/CompoundTag", "remove") => {
            let key = name_of(rt, a(1));
            if let Some(crate::Object { data: Data::Map(m), .. }) = rt.heap.get_mut(a(0).r() as usize) {
                m.remove(&key);
            }
            V::Void
        }
        ("net/minecraft/nbt/CompoundTag", n) if n.starts_with("get") => {
            let key = name_of(rt, a(1));
            let found = map_get(rt, a(0), &key);
            match (n, found) {
                (_, Some(v)) => v,
                ("getInt" | "getBoolean" | "getByte" | "getShort", None) => V::I(0),
                ("getLong", None) => V::J(0),
                ("getFloat", None) => V::F(0.0),
                ("getDouble", None) => V::D(0.0),
                ("getString", None) => rt.string(""),
                ("getCompound", None) => compound(rt),
                _ => V::NULL,
            }
        }

        // Players and interactions
        (o, n) if o.starts_with("net/minecraft/") && (o.ends_with("/Player") || o.ends_with("/ServerPlayer") || o.ends_with("/LivingEntity") || o.ends_with("/Entity")) && matches!(n, "isShiftKeyDown" | "isCrouching" | "isSteppingCarefully" | "isCreative" | "isSpectator" | "getItemInHand" | "getMainHandItem" | "level" | "isLocalPlayer") => {
            match n {
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
        ("net/minecraft/world/InteractionResult" | "net/minecraft/world/ItemInteractionResult", "sidedSuccess") => {
            let client = a(0).i() != 0;
            result(rt, owner, if client { "SUCCESS" } else { "CONSUME" })
        }
        ("net/minecraft/world/InteractionResult" | "net/minecraft/world/ItemInteractionResult", "consumesAction") => {
            let n = name_of(rt, a(0));
            V::I(matches!(n.as_str(), "SUCCESS" | "CONSUME" | "SUCCESS_NO_ITEM_USED" | "CONSUME_PARTIAL" | "SKIP_DEFAULT_BLOCK_INTERACTION") as i32)
        }
        ("net/minecraft/world/phys/BlockHitResult", "getBlockPos") => rt.get_field(a(0), "$pos"),
        ("net/minecraft/world/phys/BlockHitResult", "getDirection") => rt.get_field(a(0), "$dir"),
        ("net/minecraft/world/phys/BlockHitResult", "getLocation") => rt.get_field(a(0), "$loc"),
        ("net/minecraft/world/phys/BlockHitResult", "isInside") => V::I(0),
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
        ("net/minecraft/world/item/ItemStack", "<init>") => {
            let n = item_like_name(rt, a(1));
            let count = if desc.starts_with("(Lnet/minecraft/world/level/ItemLike;I)") { a(2).i() } else { 1 };
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
            V::I((rt.get_field(a(0), "$item") == a(1) && !stack_empty(rt, a(0))) as i32)
        }
        ("net/minecraft/world/item/ItemStack", "copy") => {
            let n = name_of(rt, rt.get_field(a(0), "$item"));
            let c = rt.get_field(a(0), "$count").i();
            stack(rt, &n, c)
        }
        ("net/minecraft/world/item/ItemStack", "copyWithCount") => {
            let n = name_of(rt, rt.get_field(a(0), "$item"));
            stack(rt, &n, a(1).i())
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
        ("net/minecraft/util/Mth", "sin") => V::F((a(0).f() as f64).sin() as f32),
        ("net/minecraft/util/Mth", "cos") => V::F((a(0).f() as f64).cos() as f32),
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

/// Static fields this module owns: `BlockStateProperties.*`, `ItemStack.EMPTY`, `Items.*`.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    Some(match owner {
        "net/minecraft/world/level/block/state/properties/BlockStateProperties"
        | "net/minecraft/world/level/block/HorizontalDirectionalBlock"
        | "net/minecraft/world/level/block/DirectionalBlock"
        | "net/minecraft/world/level/block/RotatedPillarBlock" => {
            let (name, kind) = vanilla_property(field)?;
            property(rt, &name, kind)
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
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use crate::{Rt, V, api};

    const GET: &str = "net/minecraft/world/level/block/state/BlockState.getValue.(Lnet/minecraft/world/level/block/state/properties/Property;)Ljava/lang/Comparable;";
    const SET: &str = "net/minecraft/world/level/block/state/BlockState.setValue.(Lnet/minecraft/world/level/block/state/properties/Property;Ljava/lang/Comparable;)Ljava/lang/Object;";

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
