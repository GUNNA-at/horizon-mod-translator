//! Fluids: vanilla's (`Fluids.WATER`), a mod's own (NeoForge's `BaseFlowingFluid`, as Registrate
//! builds them), their `FluidState`s, the fluid a block state holds (`getFluidState`), and fluid
//! `TagKey`s.
//!
//! A vanilla fluid is a named object (`minecraft:water`), one per name as Java's are. A mod's fluid
//! is its own object: `BaseFlowingFluid`'s constructor copies onto it the suppliers and values of
//! its `Properties`, and its getters run those suppliers, so `getSource()` is the source fluid the
//! mod registered. A `FluidState` is one object per fluid, level and falling flag: Java compares
//! fluids and their states by identity.
//!
//! A call on a `FluidState` goes to its fluid as Java's does (`state.isSource()` is
//! `getType().isSource(state)`): the mod's own method when its fluid class has one, else the API's.
use crate::{api, Data, Rt, V};

const FLUID_STATE: &str = "net/minecraft/world/level/material/FluidState";
const BASE: &str = "net/neoforged/neoforge/fluids/BaseFlowingFluid";
const BASE_FLOWING: &str = "net/neoforged/neoforge/fluids/BaseFlowingFluid$Flowing";
const BASE_SOURCE: &str = "net/neoforged/neoforge/fluids/BaseFlowingFluid$Source";
const PROPERTIES: &str = "net/neoforged/neoforge/fluids/BaseFlowingFluid$Properties";
const FLUID_TYPE: &str = "net/neoforged/neoforge/fluids/FluidType";
const TYPE_PROPERTIES: &str = "net/neoforged/neoforge/fluids/FluidType$Properties";
const FLUID_INTERACTIONS: &str = "net/neoforged/neoforge/fluids/FluidInteractionRegistry";
const INTERACTION_INFO: &str = "net/neoforged/neoforge/fluids/FluidInteractionRegistry$InteractionInformation";

/// `FluidInteractionRegistry`'s interactions: for each fluid type, what it does next to others
/// (lava beside water makes cobblestone), in the order mods added them.
pub(crate) fn fluid_interactions(rt: &mut Rt) -> V {
    rt.intern("compat:fluid-interactions".into(), |rt| rt.alloc("java/util/IdentityHashMap", Data::Table(Default::default())))
}
const SOUND_ACTION: &str = "net/neoforged/neoforge/common/SoundAction";
const TAG_KEY: &str = "net/minecraft/tags/TagKey";
const FLUID_STACK: &str = "net/neoforged/neoforge/fluids/FluidStack";
const SIMPLE_FLUID_CONTENT: &str = "net/neoforged/neoforge/fluids/SimpleFluidContent";
const LIQUID_BLOCK: &str = "net/minecraft/world/level/block/LiquidBlock";
const STATE: &str = "net/minecraft/world/level/block/state/BlockState";
const STATE_BASE: &str = "net/minecraft/world/level/block/state/BlockBehaviour$BlockStateBase";

/// Vanilla's fluids: their names and classes.
const VANILLA: [(&str, &str); 5] = [
    ("minecraft:empty", "net/minecraft/world/level/material/EmptyFluid"),
    ("minecraft:flowing_water", "net/minecraft/world/level/material/WaterFluid$Flowing"),
    ("minecraft:water", "net/minecraft/world/level/material/WaterFluid$Source"),
    ("minecraft:flowing_lava", "net/minecraft/world/level/material/LavaFluid$Flowing"),
    ("minecraft:lava", "net/minecraft/world/level/material/LavaFluid$Source"),
];

/// What `BaseFlowingFluid.Properties` holds, copied onto each fluid made from it.
const PROPERTY_FIELDS: [&str; 9] = ["$type", "$still", "$flowing", "$bucket", "$block", "$slope", "$drop", "$resistance", "$tickRate"];

// The fluid methods a `FluidState` calls, as keys a mod's fluid class can implement.
const IS_SOURCE: &str = "net/minecraft/world/level/material/Fluid.isSource.(Lnet/minecraft/world/level/material/FluidState;)Z";
const GET_AMOUNT: &str = "net/minecraft/world/level/material/Fluid.getAmount.(Lnet/minecraft/world/level/material/FluidState;)I";
const IS_EMPTY: &str = "net/minecraft/world/level/material/Fluid.isEmpty.()Z";
const IS_SAME: &str = "net/minecraft/world/level/material/Fluid.isSame.(Lnet/minecraft/world/level/material/Fluid;)Z";
const OWN_HEIGHT: &str = "net/minecraft/world/level/material/Fluid.getOwnHeight.(Lnet/minecraft/world/level/material/FluidState;)F";
const HEIGHT: &str = "net/minecraft/world/level/material/Fluid.getHeight.(Lnet/minecraft/world/level/material/FluidState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)F";
const LEGACY_BLOCK: &str = "net/minecraft/world/level/material/Fluid.createLegacyBlock.(Lnet/minecraft/world/level/material/FluidState;)Lnet/minecraft/world/level/block/state/BlockState;";
const TYPE_OF: &str = "net/minecraft/world/level/material/Fluid.getFluidType.()Lnet/neoforged/neoforge/fluids/FluidType;";
const TYPE_DENSITY: &str = "net/neoforged/neoforge/fluids/FluidType.getDensity.()I";
const TYPE_VISCOSITY: &str = "net/neoforged/neoforge/fluids/FluidType.getViscosity.()I";
const TYPE_TEMPERATURE: &str = "net/neoforged/neoforge/fluids/FluidType.getTemperature.()I";
const TYPE_LIGHT: &str = "net/neoforged/neoforge/fluids/FluidType.getLightLevel.()I";
const TYPE_RARITY: &str = "net/neoforged/neoforge/fluids/FluidType.getRarity.()Lnet/minecraft/world/item/Rarity;";
const TYPE_DESCRIPTION_ID: &str = "net/neoforged/neoforge/fluids/FluidType.getDescriptionId.()Ljava/lang/String;";
const FLUID_BUCKET: (&str, &str) = ("net/minecraft/world/level/material/Fluid.getBucket.()Lnet/minecraft/world/item/Item;", "getBucket.()Lnet/minecraft/world/item/Item;");
const RESISTANCE: &str = "net/minecraft/world/level/material/Fluid.getExplosionResistance.()F";
const SOURCE: &str = "net/minecraft/world/level/material/FlowingFluid.getSource.()Lnet/minecraft/world/level/material/Fluid;";
const FLOWING: &str = "net/minecraft/world/level/material/FlowingFluid.getFlowing.()Lnet/minecraft/world/level/material/Fluid;";
const SOURCE_STATE: &str = "net/minecraft/world/level/material/FlowingFluid.getSource.(Z)Lnet/minecraft/world/level/material/FluidState;";
const FLOWING_STATE: &str = "net/minecraft/world/level/material/FlowingFluid.getFlowing.(IZ)Lnet/minecraft/world/level/material/FluidState;";

/// `Block.getFluidState(BlockState)`, which a mod's block can implement.
const GET_FLUID_STATE: &str = "getFluidState.(Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/world/level/material/FluidState;";
const GET_FLUID_STATE_KEY: &str =
    "net/minecraft/world/level/block/state/BlockBehaviour.getFluidState.(Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/world/level/material/FluidState;";

/// Vanilla block classes whose `getFluidState` is `BlockBehaviour`'s (no fluid): a block of the
/// mod's that extends one of them without a `getFluidState` of its own holds no fluid, whatever
/// its `waterlogged`. Every other vanilla class with a `waterlogged` property reads it (26.3).
const NO_FLUID: [&str; 7] = [
    "net/minecraft/world/level/block/Block",
    "net/minecraft/world/level/block/state/BlockBehaviour",
    "net/minecraft/world/level/block/BaseEntityBlock",
    "net/minecraft/world/level/block/DirectionalBlock",
    "net/minecraft/world/level/block/HorizontalDirectionalBlock",
    "net/minecraft/world/level/block/RotatedPillarBlock",
    "net/minecraft/world/level/block/FaceAttachedHorizontalDirectionalBlock",
];

/// What a fluid is: vanilla's by name; a mod's by the NeoForge class it extends.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Empty,
    /// A source, or a fluid without a level (a mod's own direct `BaseFlowingFluid` subclass).
    Source,
    /// A flowing fluid: its states have a level.
    Flowing,
}

/// Whether `c` is one of the API's fluid classes (vanilla's and NeoForge's `BaseFlowingFluid`s).
pub fn is_fluid_class(c: &str) -> bool {
    (c.starts_with("net/minecraft/world/level/material/") && (c.ends_with("Fluid") || c.contains("Fluid$"))) || (c.starts_with(BASE) && c != PROPERTIES)
}

/// A fluid by registered name: vanilla's (`minecraft:water`, one object each) or the one a mod
/// registered under it; null when there is none.
pub fn fluid(rt: &mut Rt, name: &str) -> V {
    if let Some(v) = rt.registered.get(&("fluid".to_owned(), name.to_owned())) {
        return *v;
    }
    let Some(&(_, class)) = VANILLA.iter().find(|(n, _)| *n == name) else { return V::NULL };
    let v = rt.alloc(class, Data::Str(name.to_owned()));
    rt.registered.insert(("fluid".to_owned(), name.to_owned()), v);
    v
}

/// A fluid's registered name (`minecraft:water`, `create:honey`); empty when it has none.
pub fn name_of(rt: &Rt, f: V) -> String {
    rt.str_of(f).map(str::to_owned).or_else(|| rt.registered_name(f, "fluid").map(str::to_owned)).unwrap_or_default()
}

fn kind(rt: &Rt, f: V) -> Kind {
    if let Some(n) = rt.str_of(f) {
        return match n {
            "minecraft:empty" => Kind::Empty,
            n if n.starts_with("minecraft:flowing_") => Kind::Flowing,
            _ => Kind::Source,
        };
    }
    let mut c = rt.class_of(f);
    loop {
        match c {
            BASE_FLOWING => return Kind::Flowing,
            BASE_SOURCE => return Kind::Source,
            _ => match rt.supers.get(c) {
                Some(&s) => c = s,
                None => return Kind::Source,
            },
        }
    }
}

/// Vanilla's water and lava by family: (source, flowing).
fn family(name: &str) -> Option<(&'static str, &'static str)> {
    match name {
        "minecraft:water" | "minecraft:flowing_water" => Some(("minecraft:water", "minecraft:flowing_water")),
        "minecraft:lava" | "minecraft:flowing_lava" => Some(("minecraft:lava", "minecraft:flowing_lava")),
        _ => None,
    }
}

/// The state of `fluid` with `level` (a flowing fluid's `LEVEL`; 0 for one without) and `falling`.
pub fn fluid_state(rt: &mut Rt, fluid: V, level: i32, falling: bool) -> V {
    rt.intern(format!("fluid-state/{}/{level}/{}", fluid.r(), falling as u8), |rt| {
        let s = rt.new_object(FLUID_STATE);
        rt.put_field(s, "$fluid", fluid);
        rt.put_field(s, "$level", V::I(level));
        rt.put_field(s, "$falling", V::I(falling as i32));
        s
    })
}

fn level_of_state(rt: &Rt, s: V) -> i32 {
    rt.get_field(s, "$level").i()
}

fn falling(rt: &Rt, s: V) -> bool {
    rt.get_field(s, "$falling").i() != 0
}

/// `defaultFluidState()`: the state definition's first state (`StateDefinition.any()`, where
/// `falling` lists true first and `level` 1 first), except that NeoForge's
/// `BaseFlowingFluid.Flowing` registers level 7.
fn default_state(rt: &mut Rt, f: V) -> V {
    let (level, falls) = match kind(rt, f) {
        Kind::Empty => (0, false),
        Kind::Source => (0, true),
        Kind::Flowing if rt.str_of(f).is_some() => (1, true),
        Kind::Flowing => (7, true),
    };
    fluid_state(rt, f, level, falls)
}

/// Runs one of the `Supplier`s a mod's fluid got from its `Properties`: null when unset.
fn supplied(rt: &mut Rt, f: V, field: &'static str) -> V {
    let s = rt.get_field(f, field);
    if s.r() == 0 {
        return V::NULL;
    }
    rt.call_functional("java/util/function/Supplier.get.()Ljava/lang/Object;", s, &[])
}

/// `fluid.method(args)` as Java dispatches it: the mod's own method when the fluid's class has
/// one, else the API's.
fn on_fluid(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
    let short: &'static str = key.split_once('.').map_or(key, |(_, r)| r);
    let class = rt.class_of(args.first().copied().unwrap_or_default());
    if rt.overrides.is_some_and(|o| o(class, short))
        && let Some(vc) = rt.vcall
    {
        return vc(rt, key, short, args);
    }
    let (name, desc) = short.split_once('.').unwrap_or((short, ""));
    fluid_api(rt, name, desc, args).unwrap_or(V::NULL)
}

/// `fluid.getFluidType()`, as the fluid's class answers it (the mod's override, else the API's).
pub(crate) fn type_of(rt: &mut Rt, fluid: V) -> V {
    on_fluid(rt, TYPE_OF, &[fluid])
}

/// `fluidType.getX()` as Java dispatches it: the mod's own getter when the type's class has one,
/// else what its properties say.
fn on_type(rt: &mut Rt, key: &'static str, this: V) -> V {
    let short: &'static str = key.split_once('.').map_or(key, |(_, r)| r);
    let class = rt.class_of(this);
    if rt.overrides.is_some_and(|o| o(class, short))
        && let Some(vc) = rt.vcall
    {
        return vc(rt, key, short, &[this]);
    }
    let (name, desc) = short.split_once('.').unwrap_or((short, ""));
    fluid_type_call(rt, FLUID_TYPE, name, desc, &[this]).unwrap_or(V::NULL)
}

/// The API's own fluid methods (vanilla's fluids by name, `BaseFlowingFluid`'s by its
/// properties): what a call reaches when the mod's class doesn't have the method, or calls super.
fn fluid_api(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let f = a(0);
    let vanilla = rt.str_of(f).map(str::to_owned);
    let fam = vanilla.as_deref().and_then(family);
    Some(match name {
        // Fluid constructors: what they set lives in the runtime's fields.
        "<init>" => V::Void,
        "defaultFluidState" => default_state(rt, f),
        "getSource" if desc.starts_with("()") => match (fam, &vanilla) {
            (Some((s, _)), _) => fluid(rt, s),
            (None, Some(_)) => return None,
            (None, None) => supplied(rt, f, "$still"),
        },
        "getFlowing" if desc.starts_with("()") => match (fam, &vanilla) {
            (Some((_, fl)), _) => fluid(rt, fl),
            (None, Some(_)) => return None,
            (None, None) => supplied(rt, f, "$flowing"),
        },
        // `getSource(falling)` and `getFlowing(level, falling)`: that fluid's default state with
        // those values, the fluid as `getSource()` and `getFlowing()` answer (the mod's own, if any).
        "getSource" if desc.starts_with("(Z)") => {
            let s = on_fluid(rt, SOURCE, &[f]);
            let d = default_state(rt, s);
            let level = level_of_state(rt, d);
            fluid_state(rt, s, level, a(1).i() != 0)
        }
        "getFlowing" if desc.starts_with("(IZ)") => {
            let fl = on_fluid(rt, FLOWING, &[f]);
            fluid_state(rt, fl, a(1).i(), a(2).i() != 0)
        }
        "isSame" => {
            let other = a(1);
            let same = match (fam, &vanilla) {
                (Some((s, fl)), _) => other == fluid(rt, s) || other == fluid(rt, fl),
                (None, Some(_)) => other == f,
                (None, None) => other.r() != 0 && (other == supplied(rt, f, "$still") || other == supplied(rt, f, "$flowing")),
            };
            V::I(same as i32)
        }
        "isSource" => V::I((kind(rt, f) == Kind::Source) as i32),
        "getAmount" => V::I(match kind(rt, f) {
            Kind::Empty => 0,
            Kind::Source => 8,
            Kind::Flowing => level_of_state(rt, a(1)),
        }),
        "isEmpty" => V::I((kind(rt, f) == Kind::Empty) as i32),
        // `FlowingFluid.getOwnHeight`: the amount over 9.
        "getOwnHeight" => match kind(rt, f) {
            Kind::Empty => V::F(0.0),
            _ => V::F(on_fluid(rt, GET_AMOUNT, &[f, a(1)]).i() as f32 / 9.0),
        },
        // `FlowingFluid.getHeight`: full when the same fluid is above, else its own height.
        "getHeight" if desc.starts_with("(Lnet/minecraft/world/level/material/FluidState;") => {
            if kind(rt, f) == Kind::Empty {
                return Some(V::F(0.0));
            }
            let p = api::pos_of(rt, a(3));
            let text = rt.host().map_or_else(|| "minecraft:air".to_owned(), |h| h.block_state([p[0], p[1] + 1, p[2]]));
            let above = of_block_state(rt, &text);
            let above = rt.get_field(above, "$fluid");
            if on_fluid(rt, IS_SAME, &[f, above]).i() != 0 { V::F(1.0) } else { on_fluid(rt, OWN_HEIGHT, &[f, a(1)]) }
        }
        "getBucket" => match (fam, &vanilla) {
            (Some(("minecraft:water", _)), _) => crate::api_common::item(rt, "minecraft:water_bucket"),
            (Some(_), _) => crate::api_common::item(rt, "minecraft:lava_bucket"),
            (None, Some(_)) => crate::api_common::item(rt, "minecraft:air"),
            (None, None) => match supplied(rt, f, "$bucket") {
                b if b.r() != 0 => b,
                _ => crate::api_common::item(rt, "minecraft:air"),
            },
        },
        // NeoForge's type of a vanilla fluid is named after it (`minecraft:water`); a mod's comes
        // from its properties.
        "getFluidType" => match (fam, &vanilla) {
            (Some((s, _)), _) => fluid_type(rt, s),
            (None, Some(_)) => fluid_type(rt, "minecraft:empty"),
            (None, None) => supplied(rt, f, "$type"),
        },
        "getTickDelay" => V::I(match (fam, &vanilla) {
            (Some(("minecraft:water", _)), _) => 5,
            // `dimensionType().ultraWarm() ? 10 : 30`: the level modelled is the overworld.
            (Some(_), _) => 30,
            (None, Some(_)) => 0,
            (None, None) => rt.get_field(f, "$tickRate").i(),
        }),
        "getExplosionResistance" => V::F(match (fam, &vanilla) {
            (Some(_), _) => 100.0,
            (None, Some(_)) => 0.0,
            (None, None) => rt.get_field(f, "$resistance").f(),
        }),
        "createLegacyBlock" => legacy_block(rt, f, a(1)),
        // `FlowingFluid.getNewLiquid(level, pos, state)`: the fluid the cell at `pos` takes from
        // its neighbours if it held `state`, as the game's own fluids work it out. Create asks it
        // of a source a pipe drains: when the source would fill again (two sources beside it over
        // solid ground), the source stays: infinite water. With no game to ask, none.
        "getNewLiquid" => {
            let p = api::pos_of(rt, a(2));
            let state = rt.str_of(a(3)).unwrap_or("minecraft:air").to_owned();
            let name = name_of(rt, f);
            match rt.host_mut().and_then(|h| h.new_liquid(p, &name, &state)) {
                Some((new, amount, falls)) => {
                    let n = fluid(rt, &new);
                    match kind(rt, n) {
                        Kind::Empty => default_state(rt, n),
                        Kind::Source => on_fluid(rt, SOURCE_STATE, &[n, V::I(falls as i32)]),
                        Kind::Flowing => on_fluid(rt, FLOWING_STATE, &[n, V::I(i32::from(amount)), V::I(falls as i32)]),
                    }
                }
                None => {
                    let e = fluid(rt, "minecraft:empty");
                    default_state(rt, e)
                }
            }
        }
        // The registry entry the fluid is (`builtInRegistryHolder().is(tag)`).
        "builtInRegistryHolder" => {
            let n = name_of(rt, f);
            crate::neo::registry_holder(rt, "fluid", &n, f)
        }
        "is" if desc.starts_with("(Lnet/minecraft/tags/TagKey;") => {
            let n = name_of(rt, f);
            V::I(in_tag(rt, a(1), &n) as i32)
        }
        _ => return None,
    })
}

/// What a `FluidType` answers before its `Properties` say otherwise (NeoForge 1.21.1's defaults), and
/// vanilla's water and lava (`NeoForgeMod.WATER_TYPE`, `LAVA_TYPE`): density, viscosity,
/// temperature, light.
fn type_defaults(name: Option<&str>) -> [i32; 4] {
    match name {
        Some("minecraft:water") => [1000, 1000, 300, 0],
        Some("minecraft:lava") => [3000, 6000, 1300, 15],
        Some("minecraft:empty") => [0, 0, 0, 0],
        _ => [1000, 1000, 300, 0],
    }
}

/// NeoForge's `FluidType.Properties` (each builder call kept as a `$` field, its sounds by action),
/// a `FluidType` made from them (`new FluidType(properties)`, or a mod's subclass calling it) and
/// what its getters answer; `SoundAction`s by name.
fn fluid_type_call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    Some(match (owner, name) {
        (SOUND_ACTION, "get") => sound_action(rt, rt.str_of(a(0)).unwrap_or("").to_owned()),
        (SOUND_ACTION, "name") => rt.get_field(this, "$name"),
        (TYPE_PROPERTIES, "create") => {
            let p = rt.new_object(TYPE_PROPERTIES);
            let sounds = rt.alloc("java/util/HashMap", Data::Map(Default::default()));
            rt.put_field(p, "$sounds", sounds);
            p
        }
        (TYPE_PROPERTIES, "sound") => {
            let action = rt.str_of(rt.get_field(a(1), "$name")).unwrap_or("").to_owned();
            let sounds = rt.get_field(this, "$sounds").r() as usize;
            if let Some(Data::Map(m)) = rt.heap.get_mut(sounds).map(|o| &mut o.data) {
                m.insert(action, a(2));
            }
            this
        }
        (TYPE_PROPERTIES, n) if desc.ends_with(")Lnet/neoforged/neoforge/fluids/FluidType$Properties;") && args.len() >= 2 => {
            let field: &'static str = rt.static_key(&format!("${n}"));
            rt.put_field(this, field, a(1));
            this
        }
        (FLUID_TYPE, "<init>") if desc == "(Lnet/neoforged/neoforge/fluids/FluidType$Properties;)V" => {
            if let Some(Data::Fields(f)) = rt.heap.get(a(1).r() as usize).map(|o| o.data.clone()) {
                for (k, v) in f {
                    rt.put_field(this, k, v);
                }
            }
            V::Void
        }
        (FLUID_TYPE, n) => {
            let vanilla = rt.str_of(rt.get_field(this, "$name")).map(str::to_owned);
            let [density, viscosity, temperature, light] = type_defaults(vanilla.as_deref());
            let own = |rt: &Rt, f: &'static str, default: V| match rt.get_field(this, f) {
                V::Void => default,
                v => v,
            };
            match n {
                "getDensity" if desc.starts_with("()") => own(rt, "$density", V::I(density)),
                "getViscosity" if desc.starts_with("()") => own(rt, "$viscosity", V::I(viscosity)),
                "getTemperature" if desc.starts_with("()") => own(rt, "$temperature", V::I(temperature)),
                "getLightLevel" if desc.starts_with("()") => own(rt, "$lightLevel", V::I(light)),
                // NeoForge's `getDensity() <= 0`, by the type's own getter.
                "isLighterThanAir" => V::I((on_type(rt, TYPE_DENSITY, this).i() <= 0) as i32),
                // The stack's and the state's overloads (`getLightLevel(stack)`, `getDensity(state,
                // level, pos)`...) are NeoForge's `this.getX()`: the type's own getter, a mod's
                // override of it included.
                "getDensity" | "getViscosity" | "getTemperature" | "getLightLevel" | "getRarity" | "getDescriptionId" if !desc.starts_with("()") => {
                    let getter = match n {
                        "getDensity" => TYPE_DENSITY,
                        "getViscosity" => TYPE_VISCOSITY,
                        "getTemperature" => TYPE_TEMPERATURE,
                        "getLightLevel" => TYPE_LIGHT,
                        "getRarity" => TYPE_RARITY,
                        _ => TYPE_DESCRIPTION_ID,
                    };
                    on_type(rt, getter, this)
                }
                // NeoForge's `new ItemStack(stack.getFluid().getBucket())`, the fluid's own answer.
                "getBucket" if desc.starts_with("(Lnet/neoforged/neoforge/fluids/FluidStack;)") => {
                    let f = rt.get_field(a(1), "$fluid");
                    let item = crate::tanks::virt(rt, FLUID_BUCKET, &[f]);
                    let name = crate::api_common::item_like_name(rt, item);
                    crate::api_common::stack(rt, &name, 1)
                }
                "getFallDistanceModifier" => own(rt, "$fallDistanceModifier", V::F(match vanilla.as_deref() { Some("minecraft:water") => 0.0, Some("minecraft:empty") => 1.0, _ => 0.5 })),
                "motionScale" => own(rt, "$motionScale", V::D(match vanilla.as_deref() { Some("minecraft:lava") => 0.0023333333333333335, Some("minecraft:empty") => 1.0, _ => 0.014 })),
                "canExtinguish" => own(rt, "$canExtinguish", V::I((vanilla.as_deref() == Some("minecraft:water")) as i32)),
                "canSwim" | "canDrown" => {
                    let f = if n == "canSwim" { "$canSwim" } else { "$canDrown" };
                    own(rt, f, V::I(!matches!(vanilla.as_deref(), Some("minecraft:lava" | "minecraft:empty")) as i32))
                }
                "canPushEntity" => own(rt, "$canPushEntity", V::I((vanilla.as_deref() != Some("minecraft:empty")) as i32)),
                "supportsBoating" => own(rt, "$supportsBoating", V::I((vanilla.as_deref() == Some("minecraft:water")) as i32)),
                "canHydrate" => own(rt, "$canHydrate", V::I((vanilla.as_deref() == Some("minecraft:water")) as i32)),
                "canConvertToSource" => own(rt, "$canConvertToSource", V::I((vanilla.as_deref() == Some("minecraft:water")) as i32)),
                "getRarity" if desc.starts_with("()") => match rt.get_field(this, "$rarity") {
                    V::Void => api::get_static(rt, "net/minecraft/world/item/Rarity.COMMON"),
                    v => v,
                },
                // The translation key: the properties' own, else `fluid_type.<namespace>.<path>`.
                "getDescriptionId" if desc.starts_with("()") => match rt.get_field(this, "$descriptionId") {
                    V::Void => {
                        let id = vanilla.or_else(|| rt.registered_name(this, "fluid_type").map(str::to_owned)).unwrap_or_default();
                        rt.string(&format!("fluid_type.{}", id.replace(':', ".")))
                    }
                    v => v,
                },
                "getSound" if desc.starts_with("(Lnet/neoforged/neoforge/common/SoundAction;)") => {
                    let action = rt.str_of(rt.get_field(a(1), "$name")).unwrap_or("").to_owned();
                    match rt.heap.get(rt.get_field(this, "$sounds").r() as usize).map(|o| &o.data) {
                        Some(Data::Map(m)) => m.get(&action).copied().unwrap_or(V::NULL),
                        _ => V::NULL,
                    }
                }
                _ => return None,
            }
        }
        _ => return None,
    })
}

/// NeoForge's `SoundAction`: one object per name (`SoundActions.BUCKET_FILL` is `bucket_fill`).
pub(crate) fn sound_action(rt: &mut Rt, name: String) -> V {
    rt.intern(format!("sound-action/{name}"), move |rt| {
        let o = rt.new_object(SOUND_ACTION);
        let n = rt.string(&name);
        rt.put_field(o, "$name", n);
        o
    })
}

/// NeoForge's `FluidType` of a vanilla fluid: one per name.
fn fluid_type(rt: &mut Rt, name: &str) -> V {
    rt.intern(format!("fluid-type/{name}"), |rt| {
        let t = rt.new_object(FLUID_TYPE);
        let n = rt.string(name);
        rt.put_field(t, "$name", n);
        t
    })
}

/// `createLegacyBlock(state)`: the block that holds the fluid, its `level` from the state
/// (`FlowingFluid.getLegacyLevel`: 0 for a source, else 8 minus the amount, plus 8 when falling).
fn legacy_block(rt: &mut Rt, f: V, state: V) -> V {
    let block = match rt.str_of(f).map(str::to_owned) {
        Some(n) => match family(&n) {
            Some((source, _)) => source.to_owned(),
            None => return api::state(rt, "minecraft:air"),
        },
        None => match supplied(rt, f, "$block") {
            b if b.r() != 0 => api::block_name(rt, b),
            _ => return api::state(rt, "minecraft:air"),
        },
    };
    let level = if on_fluid(rt, IS_SOURCE, &[f, state]).i() != 0 {
        0
    } else {
        8 - on_fluid(rt, GET_AMOUNT, &[f, state]).i().min(8) + if falling(rt, state) { 8 } else { 0 }
    };
    let level = level.to_string();
    let text = crate::host::join_state(&block, vec![("level", level.as_str())]);
    api::state(rt, &text)
}

/// `LiquidBlock.getFluidState`: level 0 is the source, 1 to 7 flowing with 8 minus it, 8 and up
/// falling with 8.
fn liquid(rt: &mut Rt, f: V, level: i32) -> V {
    match level {
        ..=0 => on_fluid(rt, SOURCE_STATE, &[f, V::I(0)]),
        1..=7 => on_fluid(rt, FLOWING_STATE, &[f, V::I(8 - level), V::I(0)]),
        _ => on_fluid(rt, FLOWING_STATE, &[f, V::I(8), V::I(1)]),
    }
}

/// The fluid in one of vanilla's bucket items (`Items.WATER_BUCKET.content`): fish are carried in
/// water; an empty bucket holds `minecraft:empty`.
fn bucket_fluid(item: &str) -> &'static str {
    match item {
        "minecraft:lava_bucket" => "minecraft:lava",
        "minecraft:water_bucket" | "minecraft:cod_bucket" | "minecraft:salmon_bucket" | "minecraft:pufferfish_bucket" | "minecraft:tropical_fish_bucket" | "minecraft:axolotl_bucket" | "minecraft:tadpole_bucket" => {
            "minecraft:water"
        }
        _ => "minecraft:empty",
    }
}

fn water_source(rt: &mut Rt) -> V {
    let w = fluid(rt, "minecraft:water");
    fluid_state(rt, w, 0, false)
}

/// The fluid a vanilla block state holds (26.3's `getFluidState` overrides): water and lava by
/// their `level`, the plants that always stand in water, and `waterlogged=true` (which every
/// other override reads) when the block's class honours it.
pub fn vanilla_fluid_state(rt: &mut Rt, text: &str, waterlogged: bool) -> V {
    let (name, props) = crate::host::split_state(text);
    let prop = |k: &str| props.iter().find(|(n, _)| *n == k).map(|(_, v)| *v);
    match name {
        "minecraft:water" | "minecraft:lava" => {
            let level = prop("level").and_then(|v| v.parse().ok()).unwrap_or(0);
            let f = fluid(rt, name);
            liquid(rt, f, level)
        }
        "minecraft:bubble_column" | "minecraft:kelp" | "minecraft:kelp_plant" | "minecraft:seagrass" | "minecraft:tall_seagrass" => water_source(rt),
        _ if waterlogged && prop("waterlogged") == Some("true") => water_source(rt),
        _ => {
            let e = fluid(rt, "minecraft:empty");
            default_state(rt, e)
        }
    }
}

/// The first class up `class`'s superclasses that isn't the mod's own: the API class it extends.
fn api_class(rt: &Rt, class: &'static str) -> &'static str {
    let mut c = class;
    while rt.class_ids.contains_key(c) {
        match rt.supers.get(c) {
            Some(&s) => c = s,
            None => break,
        }
    }
    c
}

/// The fluid in a state of `block` as the vanilla class `class` answers: a `LiquidBlock`'s from
/// its fluid, others' from `waterlogged` unless the class holds no fluid.
fn of_class(rt: &mut Rt, class: &str, block: V, text: &str) -> V {
    if class == LIQUID_BLOCK {
        let f = rt.get_field(block, "fluid");
        if f.r() != 0 {
            let (_, props) = crate::host::split_state(text);
            let level = props.iter().find(|(n, _)| *n == "level").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
            return liquid(rt, f, level);
        }
    }
    vanilla_fluid_state(rt, text, !NO_FLUID.contains(&class))
}

/// `state.getFluidState()` for a block state's text: the mod block's own `getFluidState` when its
/// class has one, else what the vanilla class it extends answers.
pub fn of_block_state(rt: &mut Rt, text: &str) -> V {
    let Some((b, class)) = crate::glue::mod_block(rt, text) else {
        return vanilla_fluid_state(rt, text, true);
    };
    if rt.overrides.is_some_and(|o| o(class, GET_FLUID_STATE))
        && let Some(vc) = rt.vcall
    {
        let st = api::state(rt, text);
        return vc(rt, GET_FLUID_STATE_KEY, GET_FLUID_STATE, &[b, st]);
    }
    let base = api_class(rt, class);
    of_class(rt, base, b, text)
}

/// A tag key, one per registry and location (Java interns them): `$registry` is the registry as
/// this runtime names registries (`fluid`, which is what `Registries.FLUID` is here), `$location`
/// the tag (`minecraft:water`). A vanilla tag 26.3 renamed is the 26.3 one: the mod's 1.21.1
/// `BlockTags.DIRT` is `minecraft:substrate_overworld` ([`crate::vanilla_tags::current`]).
pub fn tag_key(rt: &mut Rt, registry: &str, location: &str) -> V {
    let location = if location.contains(':') { location.to_owned() } else { format!("minecraft:{location}") };
    let location = crate::vanilla_tags::current(registry, &location).unwrap_or(location);
    rt.intern(format!("tag-key/{registry}/{location}"), |rt| {
        let t = rt.new_object(TAG_KEY);
        let (r, l) = (rt.string(registry), rt.string(&location));
        rt.put_field(t, "$registry", r);
        rt.put_field(t, "$location", l);
        t
    })
}

/// Whether the fluid named `id` is in `tag`, as the game's loaded tags hold it (vanilla's, the
/// mods' data files', the packs'); the water and lava tags hold what vanilla's and NeoForge's own
/// data put in them even where no game answers (tests).
pub fn in_tag(rt: &Rt, tag: V, id: &str) -> bool {
    let registry = rt.str_of(rt.get_field(tag, "$registry")).unwrap_or("");
    let location = rt.str_of(rt.get_field(tag, "$location")).unwrap_or("");
    if registry.trim_start_matches("minecraft:") != "fluid" {
        return false;
    }
    let built_in = match location {
        "minecraft:water" | "c:water" => matches!(id, "minecraft:water" | "minecraft:flowing_water"),
        "minecraft:lava" | "c:lava" => matches!(id, "minecraft:lava" | "minecraft:flowing_lava"),
        _ => false,
    };
    built_in || rt.host().is_some_and(|h| h.fluid_in_tag(id, location))
}

/// NeoForge's `FluidStack`: a fluid and an amount in millibuckets (`$fluid`, `$amount`).
pub fn fluid_stack(rt: &mut Rt, fluid: V, amount: i32) -> V {
    let s = rt.new_object(FLUID_STACK);
    rt.put_field(s, "$fluid", fluid);
    rt.put_field(s, "$amount", V::I(amount));
    s
}

/// `FluidStack.EMPTY`, one object as Java's.
pub fn fluid_stack_empty(rt: &mut Rt) -> V {
    rt.intern("compat:fluid-stack/empty".into(), |rt| {
        let e = fluid(rt, "minecraft:empty");
        fluid_stack(rt, e, 0)
    })
}

/// Whether a fluid stack is empty: no amount, or the empty fluid.
pub fn stack_empty(rt: &Rt, s: V) -> bool {
    let f = rt.get_field(s, "$fluid");
    s.r() == 0 || f.r() == 0 || rt.get_field(s, "$amount").i() <= 0 || name_of(rt, f) == "minecraft:empty"
}

/// A fluid stack's amount, 0 when empty.
pub fn stack_amount(rt: &Rt, s: V) -> i32 {
    if stack_empty(rt, s) { 0 } else { rt.get_field(s, "$amount").i() }
}

fn fluid_stack_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let s = a(0);
    Some(match name {
        // `new FluidStack(fluid, amount[, patch])`, or a holder of the fluid.
        "<init>" => {
            let f = if desc.starts_with("(Lnet/minecraft/core/Holder;") { rt.get_field(a(1), "value") } else { a(1) };
            let f = if f.r() == 0 { a(1) } else { f };
            rt.put_field(s, "$fluid", f);
            rt.put_field(s, "$amount", a(2));
            V::Void
        }
        "getFluid" => {
            if stack_empty(rt, s) { fluid(rt, "minecraft:empty") } else { rt.get_field(s, "$fluid") }
        }
        "getAmount" => V::I(stack_amount(rt, s)),
        "isEmpty" => V::I(stack_empty(rt, s) as i32),
        "setAmount" => {
            rt.put_field(s, "$amount", a(1));
            V::Void
        }
        "grow" | "shrink" => {
            let d = if name == "grow" { a(1).i() } else { -a(1).i() };
            let n = rt.get_field(s, "$amount").i() + d;
            rt.put_field(s, "$amount", V::I(n));
            V::Void
        }
        "copy" => {
            let (f, n) = (rt.get_field(s, "$fluid"), rt.get_field(s, "$amount").i());
            if stack_empty(rt, s) { fluid_stack_empty(rt) } else { fluid_stack(rt, f, n) }
        }
        "copyWithAmount" => {
            let f = rt.get_field(s, "$fluid");
            if stack_empty(rt, s) { fluid_stack_empty(rt) } else { fluid_stack(rt, f, a(1).i()) }
        }
        "split" => {
            let take = a(1).i().min(stack_amount(rt, s));
            let f = rt.get_field(s, "$fluid");
            let out = fluid_stack(rt, f, take);
            let left = rt.get_field(s, "$amount").i() - take;
            rt.put_field(s, "$amount", V::I(left));
            out
        }
        "is" if desc.starts_with("(Lnet/minecraft/tags/TagKey;") => {
            let n = name_of(rt, rt.get_field(s, "$fluid"));
            V::I((!stack_empty(rt, s) && in_tag(rt, a(1), &n)) as i32)
        }
        "is" if desc.starts_with("(Lnet/minecraft/world/level/material/Fluid;") => V::I((rt.get_field(s, "$fluid") == a(1)) as i32),
        "is" if desc.starts_with("(Lnet/neoforged/neoforge/fluids/FluidType;") => {
            let f = rt.get_field(s, "$fluid");
            let t = on_fluid(rt, TYPE_OF, &[f]);
            V::I((t == a(1)) as i32)
        }
        // Same fluid (components aren't kept yet, so same components too).
        "isSameFluid" | "isSameFluidSameComponents" => {
            let (x, y) = (a(0), a(1));
            V::I((rt.get_field(x, "$fluid") == rt.get_field(y, "$fluid")) as i32)
        }
        "matches" => {
            let (x, y) = (a(0), a(1));
            V::I((rt.get_field(x, "$fluid") == rt.get_field(y, "$fluid") && stack_amount(rt, x) == stack_amount(rt, y)) as i32)
        }
        "getFluidType" => {
            let f = rt.get_field(s, "$fluid");
            on_fluid(rt, TYPE_OF, &[f])
        }
        "getFluidHolder" => {
            let h = rt.new_object("net/minecraft/core/Holder");
            let f = rt.get_field(s, "$fluid");
            rt.put_field(h, "value", f);
            h
        }
        "getComponentsPatch" => crate::recipes::get_static(rt, "net/minecraft/core/component/DataComponentPatch", "EMPTY").unwrap_or(V::NULL),
        // NBT in `FluidStack.CODEC`'s form, `{id, amount}` (components aren't kept on fluid stacks
        // yet), in a new compound or the one given. `save` of nothing throws, as NeoForge's does;
        // `saveOptional` gives an empty compound.
        "save" | "saveOptional" => {
            let tag = if desc.contains(";Lnet/minecraft/nbt/Tag;)") { a(2) } else { crate::tags::new_compound(rt) };
            if stack_empty(rt, s) {
                if name == "save" {
                    let e = rt.new_object("java/lang/IllegalStateException");
                    let m = rt.string("Cannot encode empty FluidStack");
                    rt.put_field(e, "message", m);
                    rt.thrown = Some(e);
                }
                return Some(tag);
            }
            for (k, v) in crate::tanks::save_stack(rt, s) {
                let key = rt.string(&k);
                let value = crate::nbt::element_to_runtime(rt, &v).unwrap_or(V::NULL);
                api::call(rt, "net/minecraft/nbt/CompoundTag.put.(Ljava/lang/String;Lnet/minecraft/nbt/Tag;)Lnet/minecraft/nbt/Tag;", &[tag, key, value]);
            }
            tag
        }
        // `parse(registries, tag)`, an `Optional`, and `parseOptional(registries, compound)`: the
        // stack saved, or nothing (`EMPTY`) for an empty compound or a fluid nobody registered.
        "parse" | "parseOptional" => {
            let saved = crate::nbt::from_runtime(rt, a(1));
            let stack = crate::tanks::parse_stack(rt, &saved);
            if name == "parseOptional" {
                return Some(stack);
            }
            let value = if stack_empty(rt, stack) { V::NULL } else { stack };
            let o = rt.new_object("java/util/Optional");
            rt.put_field(o, "value", value);
            o
        }
        _ => return None,
    })
}

/// Static fields this module owns: `Fluids` and `FluidTags` (NeoForge's `Tags.Fluids` are `neo_tags`').
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match owner {
        FLUID_STACK | SIMPLE_FLUID_CONTENT if field == "EMPTY" => Some(fluid_stack_empty(rt)),
        "net/minecraft/world/level/material/Fluids" => {
            let f = fluid(rt, &format!("minecraft:{}", field.to_ascii_lowercase()));
            (f.r() != 0).then_some(f)
        }
        "net/minecraft/tags/FluidTags" if matches!(field, "WATER" | "LAVA") => Some(tag_key(rt, "fluid", &format!("minecraft:{}", field.to_ascii_lowercase()))),
        "net/neoforged/neoforge/common/SoundActions" => Some(sound_action(rt, field.to_ascii_lowercase())),
        // `NeoForgeMod.WATER_TYPE` and `LAVA_TYPE` (`EMPTY_TYPE`): holders of vanilla's fluid types.
        "net/neoforged/neoforge/common/NeoForgeMod" if matches!(field, "WATER_TYPE" | "LAVA_TYPE" | "EMPTY_TYPE") => {
            let n = format!("minecraft:{}", field.strip_suffix("_TYPE").unwrap_or(field).to_ascii_lowercase());
            let t = fluid_type(rt, &n);
            Some(crate::neo::registry_holder(rt, "fluid_type", &n, t))
        }
        _ => None,
    }
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(
        owner,
        FLUID_STACK | SIMPLE_FLUID_CONTENT | FLUID_STATE | PROPERTIES | BASE | BASE_FLOWING | BASE_SOURCE | LIQUID_BLOCK | TAG_KEY | "net/minecraft/world/item/BucketItem"
            | "net/minecraft/tags/FluidTags" | FLUID_TYPE | TYPE_PROPERTIES | SOUND_ACTION | FLUID_INTERACTIONS | INTERACTION_INFO
    ) || is_fluid_class(owner)
        || name == "getFluidState"
        || (matches!(owner, "net/minecraft/tags/BlockTags" | "net/minecraft/tags/ItemTags") && name == "create")
}

/// A fluid call, or None when this module doesn't model it.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let (owner, name, desc) = crate::parts(key)?;
    if owner == FLUID_STACK {
        return fluid_stack_call(rt, name, desc, args);
    }
    // NeoForge's `SimpleFluidContent` (an item's fluid, immutable): held as the stack it wraps.
    if owner == SIMPLE_FLUID_CONTENT {
        return match name {
            "copyOf" if stack_empty(rt, a(0)) => Some(fluid_stack_empty(rt)),
            "copyOf" => fluid_stack_call(rt, "copy", desc, args),
            _ => fluid_stack_call(rt, name, desc, args),
        };
    }
    Some(match (owner, name) {
        (FLUID_STATE, n) => {
            let s = a(0);
            let f = rt.get_field(s, "$fluid");
            match n {
                "getType" => f,
                "isSource" => on_fluid(rt, IS_SOURCE, &[f, s]),
                "isSourceOfType" => V::I((f == a(1) && on_fluid(rt, IS_SOURCE, &[f, s]).i() != 0) as i32),
                "isEmpty" => on_fluid(rt, IS_EMPTY, &[f]),
                "getAmount" => on_fluid(rt, GET_AMOUNT, &[f, s]),
                "getOwnHeight" => on_fluid(rt, OWN_HEIGHT, &[f, s]),
                "getHeight" => on_fluid(rt, HEIGHT, &[f, s, a(1), a(2)]),
                "createLegacyBlock" => on_fluid(rt, LEGACY_BLOCK, &[f, s]),
                "getFluidType" => on_fluid(rt, TYPE_OF, &[f]),
                // `FlowingFluid.getFlow(level, pos)`: the game's current at the position (a mod
                // asks about the state that is there).
                "getFlow" => {
                    let p = api::pos_of(rt, a(2));
                    let flow = if on_fluid(rt, IS_EMPTY, &[f]).i() != 0 { [0.0; 3] } else { rt.host().map_or([0.0; 3], |h| h.fluid_flow(p)) };
                    let v = rt.new_object("net/minecraft/world/phys/Vec3");
                    for (k, x) in ["x", "y", "z"].into_iter().zip(flow) {
                        rt.put_field(v, k, V::D(x));
                    }
                    v
                }
                "getExplosionResistance" => on_fluid(rt, RESISTANCE, &[f]),
                "is" if desc.starts_with("(Lnet/minecraft/tags/TagKey;") => {
                    let n = name_of(rt, f);
                    V::I(in_tag(rt, a(1), &n) as i32)
                }
                "is" if desc.starts_with("(Lnet/minecraft/world/level/material/Fluid;") => V::I((f == a(1)) as i32),
                _ => return None,
            }
        }
        (PROPERTIES, n) => {
            let this = a(0);
            let field = match n {
                "<init>" => {
                    for (f, v) in [("$type", a(1)), ("$still", a(2)), ("$flowing", a(3))] {
                        rt.put_field(this, f, v);
                    }
                    // NeoForge's defaults.
                    for (f, v) in [("$slope", V::I(4)), ("$drop", V::I(1)), ("$resistance", V::F(1.0)), ("$tickRate", V::I(5))] {
                        rt.put_field(this, f, v);
                    }
                    return Some(V::Void);
                }
                "bucket" => "$bucket",
                "block" => "$block",
                "slopeFindDistance" => "$slope",
                "levelDecreasePerBlock" => "$drop",
                "explosionResistance" => "$resistance",
                "tickRate" => "$tickRate",
                _ => return None,
            };
            rt.put_field(this, field, a(1));
            this
        }
        // `super(properties)` from a mod's fluid, or `new BaseFlowingFluid.Flowing(properties)`.
        (BASE | BASE_FLOWING | BASE_SOURCE, "<init>") => {
            for f in PROPERTY_FIELDS {
                let v = rt.get_field(a(1), f);
                rt.put_field(a(0), f, v);
            }
            V::Void
        }
        (TYPE_PROPERTIES | FLUID_TYPE | SOUND_ACTION, _) => return fluid_type_call(rt, owner, name, desc, args),
        // `addInteraction(source, information)`: kept under the source fluid's type.
        (FLUID_INTERACTIONS, "addInteraction") => {
            let all = fluid_interactions(rt);
            let list = match crate::jutil::table_get(rt, all, a(0)).filter(|l| l.r() != 0) {
                Some(l) => l,
                None => {
                    let l = crate::jutil::new_list(rt, Vec::new());
                    crate::jutil::table_put(rt, all, a(0), l);
                    l
                }
            };
            crate::jutil::list_add(rt, list, a(1));
            return Some(V::Void);
        }
        // `InteractionInformation`'s shorthands: the other fluid's type (or a predicate) and the
        // block it makes (or a function of the fluid's state giving it); the record's own
        // constructor takes the predicate and the interaction.
        (INTERACTION_INFO, "<init>") => {
            let (when, then) = (a(1), a(2));
            let what = if desc.starts_with("(Lnet/neoforged/neoforge/fluids/FluidType;") { "$adjacent" } else { "predicate" };
            rt.put_field(a(0), what, when);
            let how = match desc.split_once(';').map(|(_, rest)| rest) {
                Some(r) if r.starts_with("Lnet/minecraft/world/level/block/state/BlockState;") => "$state",
                Some(r) if r.starts_with("Ljava/util/function/Function;") => "$function",
                _ => "interaction",
            };
            rt.put_field(a(0), how, then);
            return Some(V::Void);
        }
        (o, _) if is_fluid_class(o) => return fluid_api(rt, name, desc, args),
        // NeoForge's `LiquidBlock.getFluid()` and `BucketItem.getFluid()`: what their constructors got.
        (LIQUID_BLOCK, "getFluid") => rt.get_field(a(0), "fluid"),
        ("net/minecraft/world/item/BucketItem", "getFluid") => match rt.str_of(a(0)).map(str::to_owned) {
            Some(item) => fluid(rt, bucket_fluid(&item)),
            None => rt.get_field(a(0), "content"),
        },
        (TAG_KEY, "create") => {
            let registry = rt.str_of(a(0)).unwrap_or("").to_owned();
            let location = rt.str_of(a(1)).unwrap_or("").to_owned();
            tag_key(rt, &registry, &location)
        }
        ("net/minecraft/tags/FluidTags", "create") => {
            let location = rt.str_of(a(0)).unwrap_or("").to_owned();
            tag_key(rt, "fluid", &location)
        }
        // NeoForge's public `BlockTags.create(location)` and `ItemTags.create(location)`: the same
        // keys `TagKey.create(Registries.BLOCK / ITEM, location)` gives.
        ("net/minecraft/tags/BlockTags" | "net/minecraft/tags/ItemTags", "create") if desc == "(Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/tags/TagKey;" => {
            let location = crate::neo::location(rt, a(0));
            tag_key(rt, if owner == "net/minecraft/tags/BlockTags" { "block" } else { "item" }, &location)
        }
        (TAG_KEY, "location") => rt.get_field(a(0), "$location"),
        (TAG_KEY, "registry") => rt.get_field(a(0), "$registry"),
        (TAG_KEY, "isFor") => V::I((rt.str_of(rt.get_field(a(0), "$registry")) == rt.str_of(a(1))) as i32),
        (TAG_KEY, "equals") => V::I((a(0) == a(1)) as i32),
        // The fluid in a block state, in the level at a position, or as a vanilla block class
        // answers (`super.getFluidState(state)`).
        (STATE | STATE_BASE, "getFluidState") => {
            let text = rt.str_of(a(0)).unwrap_or("minecraft:air").to_owned();
            of_block_state(rt, &text)
        }
        (o, "getFluidState") if desc == "(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/material/FluidState;" && (api::is_level(o) || o.ends_with("BlockAndTintGetter")) => {
            let p = api::pos_of(rt, a(1));
            let text = rt.host().map_or_else(|| "minecraft:air".to_owned(), |h| h.block_state(p));
            of_block_state(rt, &text)
        }
        (o, "getFluidState") if desc == "(Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/world/level/material/FluidState;" && o.starts_with("net/minecraft/world/level/block/") => {
            let text = rt.str_of(a(1)).unwrap_or("minecraft:air").to_owned();
            let b = a(0);
            let class = rt.class_of(b);
            if rt.class_ids.contains_key(class) {
                let base = api_class(rt, class);
                of_class(rt, base, b, &text)
            } else if rt.str_of(b).is_some() {
                vanilla_fluid_state(rt, &text, true)
            } else {
                of_class(rt, class, b, &text)
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::call as c;
    use crate::host::MemoryHost;

    #[test]
    fn a_fluid_type_answers_what_its_properties_say_and_neoforges_defaults() {
        let mut rt = Rt::new();
        let k = |s: &str| -> &'static str { Box::leak(format!("{TYPE_PROPERTIES}.{s}").into_boxed_str()) };
        let p = c(&mut rt, k("create.()Lnet/neoforged/neoforge/fluids/FluidType$Properties;"), &[]);
        let id = rt.string("fluid.t.goo");
        c(&mut rt, k("descriptionId.(Ljava/lang/String;)Lnet/neoforged/neoforge/fluids/FluidType$Properties;"), &[p, id]);
        c(&mut rt, k("density.(I)Lnet/neoforged/neoforge/fluids/FluidType$Properties;"), &[p, V::I(3000)]);
        let fill = api::get_static(&mut rt, "net/neoforged/neoforge/common/SoundActions.BUCKET_FILL");
        let empty = api::get_static(&mut rt, "net/neoforged/neoforge/common/SoundActions.BUCKET_EMPTY");
        let sound = rt.string("t:goo_fill");
        c(&mut rt, k("sound.(Lnet/neoforged/neoforge/common/SoundAction;Lnet/minecraft/sounds/SoundEvent;)Lnet/neoforged/neoforge/fluids/FluidType$Properties;"), &[p, fill, sound]);
        let t = rt.new_object(FLUID_TYPE);
        c(&mut rt, "net/neoforged/neoforge/fluids/FluidType.<init>.(Lnet/neoforged/neoforge/fluids/FluidType$Properties;)V", &[t, p]);
        let get = |rt: &mut Rt, m: &str, args: &[V]| c(rt, Box::leak(format!("{FLUID_TYPE}.{m}").into_boxed_str()), args);
        assert_eq!(get(&mut rt, "getDensity.()I", &[t]), V::I(3000));
        assert_eq!(get(&mut rt, "getViscosity.()I", &[t]), V::I(1000));
        assert_eq!(get(&mut rt, "canExtinguish.(Lnet/minecraft/world/entity/Entity;)Z", &[t, V::NULL]), V::I(0));
        let d = get(&mut rt, "getDescriptionId.()Ljava/lang/String;", &[t]);
        assert_eq!(rt.str_of(d), Some("fluid.t.goo"));
        let s = "getSound.(Lnet/neoforged/neoforge/common/SoundAction;)Lnet/minecraft/sounds/SoundEvent;";
        assert_eq!(get(&mut rt, s, &[t, fill]), sound);
        assert_eq!(get(&mut rt, s, &[t, empty]), V::NULL);
        // Vanilla's lava, through `NeoForgeMod.LAVA_TYPE.value()`.
        let holder = api::get_static(&mut rt, "net/neoforged/neoforge/common/NeoForgeMod.LAVA_TYPE");
        let lava = c(&mut rt, "net/minecraft/core/Holder.value.()Ljava/lang/Object;", &[holder]);
        assert_eq!(get(&mut rt, "getTemperature.()I", &[lava]), V::I(1300));
        assert_eq!(get(&mut rt, "getLightLevel.()I", &[lava]), V::I(15));
        let fluid = api::get_static(&mut rt, "net/minecraft/world/level/material/Fluids.LAVA");
        assert_eq!(lava, c(&mut rt, "net/minecraft/world/level/material/Fluid.getFluidType.()Lnet/neoforged/neoforge/fluids/FluidType;", &[fluid]));
    }

    /// A fluid type's stack and state overloads ask its own getters, as NeoForge's do: a stack of
    /// lava gives lava's light, 15, and a mod's type that has its own `getLightLevel()` answers it
    /// for a stack too. `isLighterThanAir` asks the type's density.
    #[test]
    fn a_fluid_types_stack_and_state_overloads_ask_its_own_getters() {
        fn overrides(class: &str, key: &str) -> bool {
            class == "t/GlowType" && matches!(key, "getLightLevel.()I" | "getDensity.()I")
        }
        fn vcall(_: &mut Rt, _: &'static str, key: &'static str, _: &[V]) -> V {
            V::I(if key == "getLightLevel.()I" { 9 } else { -1 })
        }
        let mut rt = Rt::new();
        rt.overrides = Some(overrides);
        rt.vcall = Some(vcall);
        let get = |rt: &mut Rt, m: &str, args: &[V]| c(rt, Box::leak(format!("{FLUID_TYPE}.{m}").into_boxed_str()), args);
        let holder = api::get_static(&mut rt, "net/neoforged/neoforge/common/NeoForgeMod.LAVA_TYPE");
        let lava_type = c(&mut rt, "net/minecraft/core/Holder.value.()Ljava/lang/Object;", &[holder]);
        let lava = fluids(&mut rt, "LAVA");
        let stack = fluid_stack(&mut rt, lava, 1000);
        let by_stack = "getLightLevel.(Lnet/neoforged/neoforge/fluids/FluidStack;)I";
        assert_eq!(get(&mut rt, by_stack, &[lava_type, stack]), V::I(15));
        let state = fluid_in(&mut rt, "minecraft:lava[level=0]");
        let by_state = "getTemperature.(Lnet/minecraft/world/level/material/FluidState;Lnet/minecraft/world/level/BlockAndTintGetter;Lnet/minecraft/core/BlockPos;)I";
        assert_eq!(get(&mut rt, by_state, &[lava_type, state, V::NULL, V::NULL]), V::I(1300));
        assert_eq!(get(&mut rt, "isLighterThanAir.()Z", &[lava_type]), V::I(0));
        let glow = rt.new_object("t/GlowType");
        assert_eq!(get(&mut rt, by_stack, &[glow, stack]), V::I(9));
        assert_eq!(get(&mut rt, "isLighterThanAir.()Z", &[glow]), V::I(1));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A pipe's or a tank's fluid saved as NeoForge's `FluidStack` saves it (`saveOptional`:
    /// `{id, amount}`) reads back the same (`parseOptional`, or `parse` in an `Optional`). Nothing
    /// saves as an empty compound that reads back as `EMPTY`, and `save` of nothing throws.
    #[test]
    fn a_fluid_stack_saved_as_neoforge_saves_it_reads_back_the_same() {
        let mut rt = Rt::new();
        let save_optional = "net/neoforged/neoforge/fluids/FluidStack.saveOptional.(Lnet/minecraft/core/HolderLookup$Provider;)Lnet/minecraft/nbt/Tag;";
        let save = "net/neoforged/neoforge/fluids/FluidStack.save.(Lnet/minecraft/core/HolderLookup$Provider;)Lnet/minecraft/nbt/Tag;";
        let parse_optional = "net/neoforged/neoforge/fluids/FluidStack.parseOptional.(Lnet/minecraft/core/HolderLookup$Provider;Lnet/minecraft/nbt/CompoundTag;)Lnet/neoforged/neoforge/fluids/FluidStack;";
        let parse = "net/neoforged/neoforge/fluids/FluidStack.parse.(Lnet/minecraft/core/HolderLookup$Provider;Lnet/minecraft/nbt/Tag;)Ljava/util/Optional;";
        let water = fluids(&mut rt, "WATER");
        let stack = fluid_stack(&mut rt, water, 250);
        let saved = c(&mut rt, save_optional, &[stack, V::NULL]);
        let expected = std::collections::BTreeMap::from([
            ("amount".to_owned(), crate::nbt::Nbt::Int(250)),
            ("id".to_owned(), crate::nbt::Nbt::String("minecraft:water".into())),
        ]);
        assert_eq!(crate::nbt::from_runtime(&rt, saved), expected);
        let back = c(&mut rt, parse_optional, &[V::NULL, saved]);
        assert_eq!((rt.get_field(back, "$fluid"), stack_amount(&rt, back)), (water, 250));
        let parsed = c(&mut rt, parse, &[V::NULL, saved]);
        let inside = rt.get_field(parsed, "value");
        assert_eq!((rt.get_field(inside, "$fluid"), stack_amount(&rt, inside)), (water, 250));
        let empty = fluid_stack_empty(&mut rt);
        let none = c(&mut rt, save_optional, &[empty, V::NULL]);
        assert!(crate::nbt::from_runtime(&rt, none).is_empty());
        assert_eq!(c(&mut rt, parse_optional, &[V::NULL, none]), empty);
        let parsed = c(&mut rt, parse, &[V::NULL, none]);
        assert_eq!(rt.get_field(parsed, "value"), V::NULL);
        c(&mut rt, save, &[empty, V::NULL]);
        assert!(rt.thrown.take().is_some(), "saving nothing throws");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Create's pipe drains a source and asks, through its mixin accessor on vanilla's
    /// `FlowingFluid` (`instanceof FlowingFluidAccessor`, then `getNewLiquid`), whether the source
    /// fills again. Mixin makes `FlowingFluid` implement the accessor, so water and flowing water
    /// are instances of it and the empty fluid isn't; the answer is the game's: a source between
    /// two sources (the same state the block gives), flowing water by its amount, none elsewhere.
    #[test]
    fn a_drained_source_asks_the_game_whether_it_fills_again() {
        let mut rt = Rt::new();
        crate::install();
        let accessor = "t/FlowingFluidAccessor";
        rt.add_game_interfaces("net/minecraft/world/level/material/FlowingFluid", &["t/FlowingFluidAccessor"]);
        let (water, flowing, empty) = (fluids(&mut rt, "WATER"), fluids(&mut rt, "FLOWING_WATER"), fluids(&mut rt, "EMPTY"));
        assert!(rt.instance_of_static(water, accessor) && rt.instance_of_static(flowing, accessor));
        assert!(!rt.instance_of_static(empty, accessor));
        let (source, level_five, nothing) = (fluid_in(&mut rt, "minecraft:water[level=0]"), fluid_in(&mut rt, "minecraft:water[level=5]"), fluid_in(&mut rt, "minecraft:air"));
        let mut host = MemoryHost::default();
        host.new_liquids.insert([4, 64, 0], ("minecraft:water".into(), 8, false));
        host.new_liquids.insert([5, 64, 0], ("minecraft:flowing_water".into(), 3, false));
        let get_new = "net/minecraft/world/level/material/FlowingFluid.getNewLiquid.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/world/level/material/FluidState;";
        rt.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            let drained = api::state(rt, "minecraft:water[level=14]");
            for (x, expected) in [(4, source), (5, level_five), (9, nothing)] {
                let at = api::block_pos(rt, x, 64, 0);
                assert_eq!(c(rt, get_new, &[flowing, level, at, drained]), expected, "at x {x}");
            }
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    const GET_TYPE: &str = "net/minecraft/world/level/material/FluidState.getType.()Lnet/minecraft/world/level/material/Fluid;";
    const AMOUNT: &str = "net/minecraft/world/level/material/FluidState.getAmount.()I";
    const IS_SOURCE_NOW: &str = "net/minecraft/world/level/material/FluidState.isSource.()Z";
    const LEGACY: &str = "net/minecraft/world/level/material/FluidState.createLegacyBlock.()Lnet/minecraft/world/level/block/state/BlockState;";
    const STATE_FLUID: &str = "net/minecraft/world/level/block/state/BlockState.getFluidState.()Lnet/minecraft/world/level/material/FluidState;";

    fn fluid_in(rt: &mut Rt, text: &str) -> V {
        let s = api::state(rt, text);
        c(rt, STATE_FLUID, &[s])
    }

    fn fluids(rt: &mut Rt, name: &'static str) -> V {
        api::get_static(rt, rt_static(name))
    }

    fn rt_static(name: &'static str) -> &'static str {
        match name {
            "WATER" => "net/minecraft/world/level/material/Fluids.WATER",
            "FLOWING_WATER" => "net/minecraft/world/level/material/Fluids.FLOWING_WATER",
            "LAVA" => "net/minecraft/world/level/material/Fluids.LAVA",
            "FLOWING_LAVA" => "net/minecraft/world/level/material/Fluids.FLOWING_LAVA",
            _ => "net/minecraft/world/level/material/Fluids.EMPTY",
        }
    }

    #[test]
    fn vanilla_block_states_hold_the_fluids_vanilla_gives_them() {
        let mut rt = Rt::new();
        let (water, flowing_water, flowing_lava, empty) = (fluids(&mut rt, "WATER"), fluids(&mut rt, "FLOWING_WATER"), fluids(&mut rt, "FLOWING_LAVA"), fluids(&mut rt, "EMPTY"));
        // A water source; the same state object each time, as Java's.
        let s = fluid_in(&mut rt, "minecraft:water[level=0]");
        assert_eq!(fluid_in(&mut rt, "minecraft:water[level=0]"), s);
        assert_eq!((c(&mut rt, GET_TYPE, &[s]), c(&mut rt, AMOUNT, &[s]), c(&mut rt, IS_SOURCE_NOW, &[s])), (water, V::I(8), V::I(1)));
        // Level 3 is flowing water with 5; back to a block, level 3 again.
        let s = fluid_in(&mut rt, "minecraft:water[level=3]");
        assert_eq!((c(&mut rt, GET_TYPE, &[s]), c(&mut rt, AMOUNT, &[s]), c(&mut rt, IS_SOURCE_NOW, &[s])), (flowing_water, V::I(5), V::I(0)));
        let b = c(&mut rt, LEGACY, &[s]);
        assert_eq!(rt.str_of(b), Some("minecraft:water[level=3]"));
        // Level 9 is falling lava with 8, which goes back as level 8.
        let s = fluid_in(&mut rt, "minecraft:lava[level=9]");
        assert_eq!((c(&mut rt, GET_TYPE, &[s]), c(&mut rt, AMOUNT, &[s])), (flowing_lava, V::I(8)));
        let b = c(&mut rt, LEGACY, &[s]);
        assert_eq!(rt.str_of(b), Some("minecraft:lava[level=8]"));
        // Waterlogged blocks and the plants that always stand in water hold a source.
        for text in ["minecraft:oak_stairs[facing=north,half=bottom,shape=straight,waterlogged=true]", "minecraft:kelp[age=3]", "minecraft:bubble_column[drag=true]"] {
            let s = fluid_in(&mut rt, text);
            assert_eq!(c(&mut rt, GET_TYPE, &[s]), water, "{text}");
        }
        // Dry ones hold nothing.
        for text in ["minecraft:oak_stairs[facing=north,half=bottom,shape=straight,waterlogged=false]", "minecraft:stone", "minecraft:air"] {
            let s = fluid_in(&mut rt, text);
            assert_eq!(c(&mut rt, GET_TYPE, &[s]), empty, "{text}");
            assert_eq!(c(&mut rt, "net/minecraft/world/level/material/FluidState.isEmpty.()Z", &[s]), V::I(1));
        }
        // `Fluids.WATER.getSource(false)` is the waterlogged state; its legacy block is a source.
        let src = c(&mut rt, SOURCE_STATE, &[water, V::I(0)]);
        assert_eq!(src, fluid_in(&mut rt, "minecraft:kelp[age=3]"));
        let b = c(&mut rt, LEGACY, &[src]);
        assert_eq!(rt.str_of(b), Some("minecraft:water[level=0]"));
        assert_eq!(c(&mut rt, IS_SAME, &[flowing_water, water]), V::I(1));
        assert_eq!(c(&mut rt, IS_SAME, &[water, flowing_lava]), V::I(0));
    }

    #[test]
    fn the_level_answers_the_fluid_at_a_position() {
        let mut rt = Rt::new();
        let mut host = MemoryHost::default();
        host.blocks.insert([0, 64, 0], "minecraft:water[level=0]".into());
        let level = rt.new_object("net/minecraft/server/level/ServerLevel");
        let (wet, dry) = (api::block_pos(&mut rt, 0, 64, 0), api::block_pos(&mut rt, 1, 64, 0));
        let key = "net/minecraft/world/level/LevelAccessor.getFluidState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/material/FluidState;";
        let (w, d) = rt.with_host(&mut host, |rt| (c(rt, key, &[level, wet]), c(rt, key, &[level, dry])));
        // Create's `withWater` compares the type with `Fluids.WATER`: a block placed in air stays dry.
        let (water, empty) = (fluids(&mut rt, "WATER"), fluids(&mut rt, "EMPTY"));
        assert_eq!((c(&mut rt, GET_TYPE, &[w]), c(&mut rt, GET_TYPE, &[d])), (water, empty));
    }

    #[test]
    fn a_mods_fluid_answers_from_its_properties() {
        fn give(rt: &mut Rt, _id: u32, args: &[V]) -> Option<V> {
            Some(rt.captured(args[0]).first().copied().unwrap_or_default())
        }
        let mut rt = Rt::new();
        rt.lambda_call = Some(give);
        // As Registrate builds a fluid: suppliers of its type, source, flowing fluid and block.
        let (source, flowing) = (rt.new_object(BASE_SOURCE), rt.new_object(BASE_FLOWING));
        let liquid = rt.new_object(LIQUID_BLOCK);
        let ty = rt.new_object(FLUID_TYPE);
        let mut supply = |v: V| rt.lambda_id("java/util/function/Supplier", 1, vec![v]);
        let (type_s, still_s, flowing_s, block_s) = (supply(ty), supply(source), supply(flowing), supply(liquid));
        let props = rt.new_object(PROPERTIES);
        c(&mut rt, "net/neoforged/neoforge/fluids/BaseFlowingFluid$Properties.<init>.(Ljava/util/function/Supplier;Ljava/util/function/Supplier;Ljava/util/function/Supplier;)V", &[props, type_s, still_s, flowing_s]);
        let chained = c(&mut rt, "net/neoforged/neoforge/fluids/BaseFlowingFluid$Properties.block.(Ljava/util/function/Supplier;)Lnet/neoforged/neoforge/fluids/BaseFlowingFluid$Properties;", &[props, block_s]);
        assert_eq!(chained, props);
        c(&mut rt, "net/neoforged/neoforge/fluids/BaseFlowingFluid$Properties.tickRate.(I)Lnet/neoforged/neoforge/fluids/BaseFlowingFluid$Properties;", &[props, V::I(25)]);
        for f in [source, flowing] {
            let owner = if f == source { "net/neoforged/neoforge/fluids/BaseFlowingFluid$Source.<init>.(Lnet/neoforged/neoforge/fluids/BaseFlowingFluid$Properties;)V" } else { "net/neoforged/neoforge/fluids/BaseFlowingFluid$Flowing.<init>.(Lnet/neoforged/neoforge/fluids/BaseFlowingFluid$Properties;)V" };
            c(&mut rt, owner, &[f, props]);
        }
        let block_props = rt.new_object(crate::blocks::BLOCK_PROPERTIES);
        c(&mut rt, "net/minecraft/world/level/block/LiquidBlock.<init>.(Lnet/minecraft/world/level/material/FlowingFluid;Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;)V", &[liquid, flowing, block_props]);
        rt.registered.insert(("block".into(), "t:honey".into()), liquid);
        // The registered source, which Create's `FluidEntry.getSource` reads (it was null).
        assert_eq!(c(&mut rt, "net/neoforged/neoforge/fluids/BaseFlowingFluid.getSource.()Lnet/minecraft/world/level/material/Fluid;", &[flowing]), source);
        assert_eq!(c(&mut rt, "net/neoforged/neoforge/fluids/BaseFlowingFluid$Flowing.getFluidType.()Lnet/neoforged/neoforge/fluids/FluidType;", &[flowing]), ty);
        let water = fluids(&mut rt, "WATER");
        assert_eq!((c(&mut rt, IS_SAME, &[flowing, source]), c(&mut rt, IS_SAME, &[flowing, water])), (V::I(1), V::I(0)));
        assert_eq!(c(&mut rt, "net/minecraft/world/level/material/FlowingFluid.getTickDelay.(Lnet/minecraft/world/level/LevelReader;)I", &[flowing, V::NULL]), V::I(25));
        let bucket = c(&mut rt, "net/neoforged/neoforge/fluids/BaseFlowingFluid$Flowing.getBucket.()Lnet/minecraft/world/item/Item;", &[flowing]);
        assert_eq!(rt.str_of(bucket), Some("minecraft:air"));
        // `getSource(false)` is a source state of the source fluid; the flowing fluid's default
        // state has NeoForge's level 7.
        let s = c(&mut rt, SOURCE_STATE, &[flowing, V::I(0)]);
        assert_eq!((c(&mut rt, GET_TYPE, &[s]), c(&mut rt, AMOUNT, &[s]), c(&mut rt, IS_SOURCE_NOW, &[s])), (source, V::I(8), V::I(1)));
        let d = c(&mut rt, "net/minecraft/world/level/material/Fluid.defaultFluidState.()Lnet/minecraft/world/level/material/FluidState;", &[flowing]);
        assert_eq!((c(&mut rt, AMOUNT, &[d]), c(&mut rt, IS_SOURCE_NOW, &[d])), (V::I(7), V::I(0)));
        // Its block: flowing with 3 is level 5, and the block's states hold the fluid back.
        let three = c(&mut rt, FLOWING_STATE, &[flowing, V::I(3), V::I(0)]);
        let b = c(&mut rt, LEGACY, &[three]);
        assert_eq!(rt.str_of(b), Some("t:honey[level=5]"));
        let s = fluid_in(&mut rt, "t:honey[level=2]");
        assert_eq!((c(&mut rt, GET_TYPE, &[s]), c(&mut rt, AMOUNT, &[s])), (flowing, V::I(6)));
        assert_eq!(rt.get_field(liquid, "fluid"), flowing);
    }

    #[test]
    fn a_mod_block_without_its_own_fluid_state_answers_as_its_vanilla_class() {
        let mut rt = Rt::new();
        for (class, id, sup, name) in [("t/ModStairs", 1, "net/minecraft/world/level/block/StairBlock", "t:stairs"), ("t/ModBox", 2, "net/minecraft/world/level/block/Block", "t:box")] {
            rt.class_ids.insert(class, id);
            rt.supers.insert(class, sup);
            let b = rt.new_object(class);
            rt.registered.insert(("block".into(), name.into()), b);
        }
        let (water, empty) = (fluids(&mut rt, "WATER"), fluids(&mut rt, "EMPTY"));
        let s = fluid_in(&mut rt, "t:stairs[facing=north,waterlogged=true]");
        assert_eq!(c(&mut rt, GET_TYPE, &[s]), water);
        // `Block` itself holds no fluid: a block that keeps a `waterlogged` without answering it is dry.
        let s = fluid_in(&mut rt, "t:box[waterlogged=true]");
        assert_eq!(c(&mut rt, GET_TYPE, &[s]), empty);
    }

    #[test]
    fn fluid_tags_hold_vanilla_water_and_lava() {
        let mut rt = Rt::new();
        let water_tag = api::get_static(&mut rt, "net/minecraft/tags/FluidTags.WATER");
        let lava_common = api::get_static(&mut rt, "net/neoforged/neoforge/common/Tags$Fluids.LAVA");
        // `TagKey.create(Registries.FLUID, c:lava)` is the same key as `Tags.Fluids.LAVA`.
        let fluid_registry = api::get_static(&mut rt, "net/minecraft/core/registries/Registries.FLUID");
        let loc = rt.string("c:lava");
        let made = c(&mut rt, "net/minecraft/tags/TagKey.create.(Lnet/minecraft/resources/ResourceKey;Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/tags/TagKey;", &[fluid_registry, loc]);
        assert_eq!(made, lava_common);
        let l = c(&mut rt, "net/minecraft/tags/TagKey.location.()Lnet/minecraft/resources/ResourceLocation;", &[made]);
        assert_eq!(rt.str_of(l), Some("c:lava"));
        let is_tag = "net/minecraft/world/level/material/FluidState.is.(Lnet/minecraft/tags/TagKey;)Z";
        let flowing = fluid_in(&mut rt, "minecraft:water[level=4]");
        assert_eq!((c(&mut rt, is_tag, &[flowing, water_tag]), c(&mut rt, is_tag, &[flowing, lava_common])), (V::I(1), V::I(0)));
        let lava = fluids(&mut rt, "LAVA");
        assert_eq!(c(&mut rt, "net/minecraft/world/level/material/Fluid.is.(Lnet/minecraft/tags/TagKey;)Z", &[lava, lava_common]), V::I(1));
        let milk = api::get_static(&mut rt, "net/neoforged/neoforge/common/Tags$Fluids.MILK");
        assert_eq!(c(&mut rt, "net/minecraft/world/level/material/Fluid.is.(Lnet/minecraft/tags/TagKey;)Z", &[lava, milk]), V::I(0));
        // Create asks the fluid's registry entry (`builtInRegistryHolder().is(tag)`): the same tags.
        let holder = c(&mut rt, "net/minecraft/world/level/material/Fluid.builtInRegistryHolder.()Lnet/minecraft/core/Holder$Reference;", &[lava]);
        let is_holder = "net/minecraft/core/Holder.is.(Lnet/minecraft/tags/TagKey;)Z";
        assert_eq!((c(&mut rt, is_holder, &[holder, lava_common]), c(&mut rt, is_holder, &[holder, water_tag])), (V::I(1), V::I(0)));
        assert_eq!(c(&mut rt, "net/minecraft/core/Holder.value.()Ljava/lang/Object;", &[holder]), lava);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn a_mods_fluid_tag_holds_what_the_games_tags_hold() {
        let mut rt = Rt::new();
        let mut host = MemoryHost::default();
        let splashing = "create:fan_processing_catalysts/splashing";
        host.fluid_tags.insert(splashing.into(), vec!["minecraft:water".into(), "minecraft:flowing_water".into()]);
        let fluid_registry = api::get_static(&mut rt, "net/minecraft/core/registries/Registries.FLUID");
        let make = |rt: &mut Rt, name: &str| {
            let loc = rt.string(name);
            c(rt, "net/minecraft/tags/TagKey.create.(Lnet/minecraft/resources/ResourceKey;Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/tags/TagKey;", &[fluid_registry, loc])
        };
        let (tag, blasting) = (make(&mut rt, splashing), make(&mut rt, "create:fan_processing_catalysts/blasting"));
        let is_tag = "net/minecraft/world/level/material/FluidState.is.(Lnet/minecraft/tags/TagKey;)Z";
        let (flowing, lava) = (fluid_in(&mut rt, "minecraft:water[level=3]"), fluid_in(&mut rt, "minecraft:lava[level=0]"));
        let asked = rt.with_host(&mut host, |rt| {
            [c(rt, is_tag, &[flowing, tag]), c(rt, is_tag, &[lava, tag]), c(rt, is_tag, &[flowing, blasting])]
        });
        assert_eq!(asked, [V::I(1), V::I(0), V::I(0)]);
        // No game to ask: a mod's tag holds nothing.
        assert_eq!(c(&mut rt, is_tag, &[flowing, tag]), V::I(0));
    }
}
