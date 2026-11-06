//! NeoForge's client extensions: what a mod tells the client about how its blocks, items, mob
//! effects and fluid types look (`IClientBlockExtensions`, `IClientItemExtensions`,
//! `IClientMobEffectExtensions`, `IClientFluidTypeExtensions`), and where the client finds them
//! (`IClientFluidTypeExtensions.of(fluid)`...).
//!
//! The client gathers them once at its setup ([`init`]), as NeoForge's `ClientExtensionsManager`
//! does: first from each registered object whose class overrides `initializeClient(consumer)` (the
//! deprecated way, Create's fluid types and tools), then from `RegisterClientExtensionsEvent` on the
//! mod bus. `of` hands out what was registered for the object, else NeoForge's own for vanilla's
//! water and lava (their textures and tint), else the interface's `DEFAULT`. The interfaces' default
//! methods answer as NeoForge's do; the ones that defer to another (`getTintColor(stack)` is
//! `getTintColor()`) ask the receiver, so a mod's override of the plain one counts.
use crate::{api, Data, Rt, V};

const IFLUID: &str = "net/neoforged/neoforge/client/extensions/common/IClientFluidTypeExtensions";
const IITEM: &str = "net/neoforged/neoforge/client/extensions/common/IClientItemExtensions";
const IBLOCK: &str = "net/neoforged/neoforge/client/extensions/common/IClientBlockExtensions";
const IEFFECT: &str = "net/neoforged/neoforge/client/extensions/common/IClientMobEffectExtensions";
const EVENT: &str = "net/neoforged/neoforge/client/extensions/common/RegisterClientExtensionsEvent";
/// NeoForge's keeper of the extensions; the runtime's own keys on it register one (the function an
/// `initializeClient` consumer runs).
const MANAGER: &str = "net/neoforged/neoforge/client/extensions/common/ClientExtensionsManager";
const INITIALIZE: &str = "initializeClient.(Ljava/util/function/Consumer;)V";

const GET_TINT: (&str, &str) = ("net/neoforged/neoforge/client/extensions/common/IClientFluidTypeExtensions.getTintColor.()I", "getTintColor.()I");
const GET_STILL: (&str, &str) = (
    "net/neoforged/neoforge/client/extensions/common/IClientFluidTypeExtensions.getStillTexture.()Lnet/minecraft/resources/ResourceLocation;",
    "getStillTexture.()Lnet/minecraft/resources/ResourceLocation;",
);
const GET_FLOWING: (&str, &str) = (
    "net/neoforged/neoforge/client/extensions/common/IClientFluidTypeExtensions.getFlowingTexture.()Lnet/minecraft/resources/ResourceLocation;",
    "getFlowingTexture.()Lnet/minecraft/resources/ResourceLocation;",
);
const GET_OVERLAY: (&str, &str) = (
    "net/neoforged/neoforge/client/extensions/common/IClientFluidTypeExtensions.getOverlayTexture.()Lnet/minecraft/resources/ResourceLocation;",
    "getOverlayTexture.()Lnet/minecraft/resources/ResourceLocation;",
);
const HUMANOID_ARMOR_MODEL: (&str, &str) = (
    "net/neoforged/neoforge/client/extensions/common/IClientItemExtensions.getHumanoidArmorModel.(Lnet/minecraft/world/entity/LivingEntity;Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/entity/EquipmentSlot;Lnet/minecraft/client/model/HumanoidModel;)Lnet/minecraft/client/model/HumanoidModel;",
    "getHumanoidArmorModel.(Lnet/minecraft/world/entity/LivingEntity;Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/entity/EquipmentSlot;Lnet/minecraft/client/model/HumanoidModel;)Lnet/minecraft/client/model/HumanoidModel;",
);
const HELMET_OVERLAY: (&str, &str) = (
    "net/neoforged/neoforge/client/extensions/common/IClientItemExtensions.renderHelmetOverlay.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/entity/player/Player;IIF)V",
    "renderHelmetOverlay.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/entity/player/Player;IIF)V",
);
const STACK_ITEM: &str = "net/minecraft/world/item/ItemStack.getItem.()Lnet/minecraft/world/item/Item;";
const STATE_BLOCK: &str = "net/minecraft/world/level/block/state/BlockState.getBlock.()Lnet/minecraft/world/level/block/Block;";
const STATE_FLUID_TYPE: &str = "net/minecraft/world/level/material/FluidState.getFluidType.()Lnet/neoforged/neoforge/fluids/FluidType;";
const INSTANCE_EFFECT: &str = "net/minecraft/world/effect/MobEffectInstance.getEffect.()Lnet/minecraft/core/Holder;";
const HOLDER_VALUE: &str = "net/minecraft/core/Holder.value.()Ljava/lang/Object;";
const GUI_WIDTH: &str = "net/minecraft/client/gui/GuiGraphics.guiWidth.()I";
const GUI_HEIGHT: &str = "net/minecraft/client/gui/GuiGraphics.guiHeight.()I";
const DELTA_PARTIAL: &str = "net/minecraft/client/DeltaTracker.getGameTimeDeltaPartialTick.(Z)F";

/// Vanilla water's tint (`0xFF3F76E4`), as NeoForge's extensions for it give a fluid stack of it.
const WATER_TINT: i32 = 0xFF3F_76E4_u32 as i32;
/// Opaque white: no tint.
const WHITE: i32 = -1;

/// What an extension is registered for.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Block,
    Item,
    MobEffect,
    FluidType,
}

const KINDS: [Kind; 4] = [Kind::Block, Kind::Item, Kind::MobEffect, Kind::FluidType];

impl Kind {
    /// The registry its objects are registered in.
    fn registry(self) -> &'static str {
        match self {
            Kind::Block => "block",
            Kind::Item => "item",
            Kind::MobEffect => "mob_effect",
            Kind::FluidType => "fluid_type",
        }
    }
    /// The interface of its extensions.
    fn interface(self) -> &'static str {
        match self {
            Kind::Block => IBLOCK,
            Kind::Item => IITEM,
            Kind::MobEffect => IEFFECT,
            Kind::FluidType => IFLUID,
        }
    }
    /// `initializeClient(consumer)` on its objects' API class.
    fn initialize_key(self) -> &'static str {
        match self {
            Kind::Block => "net/minecraft/world/level/block/Block.initializeClient.(Ljava/util/function/Consumer;)V",
            Kind::Item => "net/minecraft/world/item/Item.initializeClient.(Ljava/util/function/Consumer;)V",
            Kind::MobEffect => "net/minecraft/world/effect/MobEffect.initializeClient.(Ljava/util/function/Consumer;)V",
            Kind::FluidType => "net/neoforged/neoforge/fluids/FluidType.initializeClient.(Ljava/util/function/Consumer;)V",
        }
    }
    /// The runtime's key that registers an extension of this kind for an object ([`MANAGER`]'s).
    fn register_key(self) -> &'static str {
        match self {
            Kind::Block => "net/neoforged/neoforge/client/extensions/common/ClientExtensionsManager.block.(Ljava/lang/Object;Ljava/lang/Object;)V",
            Kind::Item => "net/neoforged/neoforge/client/extensions/common/ClientExtensionsManager.item.(Ljava/lang/Object;Ljava/lang/Object;)V",
            Kind::MobEffect => "net/neoforged/neoforge/client/extensions/common/ClientExtensionsManager.mob_effect.(Ljava/lang/Object;Ljava/lang/Object;)V",
            Kind::FluidType => "net/neoforged/neoforge/client/extensions/common/ClientExtensionsManager.fluid_type.(Ljava/lang/Object;Ljava/lang/Object;)V",
        }
    }
    fn of_register_name(name: &str) -> Option<Kind> {
        Some(match name {
            "registerBlock" | "isBlockRegistered" | "block" => Kind::Block,
            "registerItem" | "isItemRegistered" | "item" => Kind::Item,
            "registerMobEffect" | "isMobEffectRegistered" | "mob_effect" => Kind::MobEffect,
            "registerFluidType" | "isFluidTypeRegistered" | "fluid_type" => Kind::FluidType,
            _ => return None,
        })
    }
}

/// The extensions registered for objects of `kind`: an identity map, object to extension.
fn table(rt: &mut Rt, kind: Kind) -> V {
    rt.intern(format!("compat:client-extensions/{}", kind.registry()), |rt| rt.alloc("java/util/IdentityHashMap", Data::Table(Default::default())))
}

/// Registers `ext` for `target`. NeoForge refuses a second registration for one object by throwing
/// as the client starts; the runtime keeps the last.
fn register(rt: &mut Rt, kind: Kind, target: V, ext: V) {
    if target.r() == 0 || ext.r() == 0 {
        return;
    }
    let t = table(rt, kind);
    crate::jutil::table_put(rt, t, target, ext);
}

fn registered(rt: &mut Rt, kind: Kind, target: V) -> Option<V> {
    if target.r() == 0 {
        return None;
    }
    let t = table(rt, kind);
    crate::jutil::table_get(rt, t, target).filter(|e| e.r() != 0)
}

/// The interface's `DEFAULT`: an extension that overrides nothing.
fn default_ext(rt: &mut Rt, kind: Kind) -> V {
    rt.intern(format!("compat:client-extensions/{}/default", kind.registry()), |rt| rt.new_object(kind.interface()))
}

/// NeoForge's own extensions for vanilla's water or lava (`ClientNeoForgeMod`), by the fluid type's
/// name.
fn vanilla_fluid_ext(rt: &mut Rt, name: &str) -> V {
    rt.intern(format!("compat:client-extensions/fluid_type/{name}"), |rt| {
        let e = rt.new_object(IFLUID);
        let n = rt.string(name);
        rt.put_field(e, "$vanilla", n);
        e
    })
}

/// `IClient*Extensions.of(target)`: what was registered for it, else (a fluid type) vanilla's
/// water's or lava's, else the interface's `DEFAULT`.
fn ext_of(rt: &mut Rt, kind: Kind, target: V) -> V {
    if let Some(e) = registered(rt, kind, target) {
        return e;
    }
    if kind == Kind::FluidType
        && target.r() != 0
        && let Some(name) = rt.str_of(rt.get_field(target, "$name")).map(str::to_owned)
        && matches!(name.as_str(), "minecraft:water" | "minecraft:lava")
    {
        return vanilla_fluid_ext(rt, &name);
    }
    default_ext(rt, kind)
}

/// The client's extensions, gathered once at its setup ([`crate::neo::fire_client_setup`]): each
/// registered block, item, mob effect and fluid type whose class overrides `initializeClient` hands
/// its own (`ClientExtensionsManager.earlyInit`), then the mods register theirs through
/// `RegisterClientExtensionsEvent` (`init`).
pub(crate) fn init(rt: &mut Rt) {
    if let (Some(overrides), Some(vc)) = (rt.overrides, rt.vcall) {
        // The registries' order is their registration order, which the runtime doesn't keep: by
        // name, so each run asks in the same order.
        let mut objects: Vec<(Kind, String, V)> = rt
            .registered
            .iter()
            .filter_map(|((registry, name), &v)| KINDS.iter().find(|k| k.registry() == registry).map(|&k| (k, name.clone(), v)))
            .collect();
        objects.sort_by(|a, b| (a.0 as u8, &a.1).cmp(&(b.0 as u8, &b.1)));
        for (kind, _, v) in objects {
            if v.r() == 0 || !overrides(rt.class_of(v), INITIALIZE) {
                continue;
            }
            let consumer = crate::functions::bound(rt, kind.register_key(), v);
            vc(rt, kind.initialize_key(), INITIALIZE, &[v, consumer]);
            rt.report_thrown("a client extension's initializeClient");
        }
    }
    let ev = rt.new_object(EVENT);
    crate::neo::post(rt, EVENT, ev);
}

/// `IClientFluidTypeExtensions`' plain accessors on vanilla's water and lava, and the interface's
/// defaults on any other: `(still, flowing, overlay, screen overlay)` textures.
fn vanilla_textures(name: Option<&str>) -> [Option<&'static str>; 4] {
    match name {
        Some("minecraft:water") => [
            Some("minecraft:block/water_still"),
            Some("minecraft:block/water_flow"),
            Some("minecraft:block/water_overlay"),
            Some("minecraft:textures/misc/underwater.png"),
        ],
        Some("minecraft:lava") => [Some("minecraft:block/lava_still"), Some("minecraft:block/lava_flow"), None, None],
        _ => [None; 4],
    }
}

/// The water colour of the biome at `pos` (`BiomeColors.getAverageWaterColor`, without the blend
/// across neighbouring biomes), opaque; vanilla's default where the host doesn't know the biome.
fn water_color(rt: &mut Rt, pos: V) -> i32 {
    let p = api::pos_of(rt, pos);
    let rgb = rt
        .host()
        .and_then(|h| h.biome(p).and_then(|b| h.biome_json(&b)))
        .and_then(|json| biome_water_color(&json))
        .unwrap_or(0x3F_76E4);
    rgb | 0xFF00_0000_u32 as i32
}

/// A biome's `effects.water_color`: a number (1.21.1's files) or `#rrggbb` (26.3's).
fn biome_water_color(json: &str) -> Option<i32> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    match &v["effects"]["water_color"] {
        serde_json::Value::Number(n) => n.as_i64().map(|n| n as i32 & 0xFF_FFFF),
        serde_json::Value::String(s) => i32::from_str_radix(s.trim_start_matches('#'), 16).ok(),
        _ => None,
    }
}

fn texture(rt: &mut Rt, location: Option<&str>) -> V {
    location.map_or(V::NULL, |l| rt.string(l))
}

fn fluid_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if name == "of" {
        let target = a(0);
        let ty = if desc.starts_with("(Lnet/neoforged/neoforge/fluids/FluidType;)") {
            target
        } else if desc.starts_with("(Lnet/minecraft/world/level/material/FluidState;)") {
            api::call(rt, STATE_FLUID_TYPE, &[target])
        } else {
            crate::fluids::type_of(rt, target)
        };
        return Some(ext_of(rt, Kind::FluidType, ty));
    }
    let this = a(0);
    let vanilla = rt.str_of(rt.get_field(this, "$vanilla")).map(str::to_owned);
    let water = vanilla.as_deref() == Some("minecraft:water");
    let [still, flowing, overlay, screen] = vanilla_textures(vanilla.as_deref());
    let plain = desc.starts_with("()");
    Some(match name {
        "getTintColor" if plain => V::I(if water { WATER_TINT } else { WHITE }),
        "getStillTexture" if plain => texture(rt, still),
        "getFlowingTexture" if plain => texture(rt, flowing),
        "getOverlayTexture" if plain => texture(rt, overlay),
        // In the level: vanilla water takes its biome's colour.
        "getTintColor" if water && desc.starts_with("(Lnet/minecraft/world/level/material/FluidState;") => V::I(water_color(rt, a(3))),
        // The level's and a stack's accessors are the plain ones, as the receiver answers them.
        "getTintColor" => api::virtual_call(rt, GET_TINT, &[this]),
        "getStillTexture" => api::virtual_call(rt, GET_STILL, &[this]),
        "getFlowingTexture" => api::virtual_call(rt, GET_FLOWING, &[this]),
        "getOverlayTexture" => api::virtual_call(rt, GET_OVERLAY, &[this]),
        "getRenderOverlayTexture" => texture(rt, screen),
        // `modifyFogColor(camera, partialTick, level, renderDistance, darkenWorldAmount, color)`.
        "modifyFogColor" => a(6),
        "renderOverlay" | "modifyFogRender" => V::Void,
        "renderFluid" => V::I(0),
        _ => return None,
    })
}

fn item_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "of" => {
            let item = if desc.starts_with("(Lnet/minecraft/world/item/ItemStack;)") { api::call(rt, STACK_ITEM, &[a(0)]) } else { a(0) };
            ext_of(rt, Kind::Item, item)
        }
        "getFont" | "getArmPose" => V::NULL,
        "applyForgeHandTransform" => V::I(0),
        // `(living, stack, slot, original)`: the original model.
        "getHumanoidArmorModel" => a(4),
        // The humanoid armor model the receiver gives, else the original.
        "getGenericArmorModel" => api::virtual_call(rt, HUMANOID_ARMOR_MODEL, &[a(0), a(1), a(2), a(3), a(4)]),
        "setupModelAnimations" => V::Void,
        // `(stack, player, guiGraphics, deltaTracker)`: the overlay at the GUI's size, as the
        // receiver draws it with the older arguments.
        "renderHelmetOverlay" if desc.contains("GuiGraphics") => {
            let (w, h) = (api::call(rt, GUI_WIDTH, &[a(3)]), api::call(rt, GUI_HEIGHT, &[a(3)]));
            let partial = api::call(rt, DELTA_PARTIAL, &[a(4), V::I(1)]);
            api::virtual_call(rt, HELMET_OVERLAY, &[a(0), a(1), a(2), w, h, partial]);
            V::Void
        }
        "renderHelmetOverlay" => V::Void,
        "getCustomRenderer" => rt.intern("client/net/minecraft/client/renderer/BlockEntityWithoutLevelRenderer".into(), |rt| {
            rt.new_object("net/minecraft/client/renderer/BlockEntityWithoutLevelRenderer")
        }),
        "shouldBobAsEntity" | "shouldSpreadAsEntity" => V::I(1),
        // `(stack, entity, layer, layerIdx, fallbackColor)`: a dyeable layer's colour, else none.
        "getArmorLayerTintColor" => {
            let dyeable = api::call(rt, "net/minecraft/world/item/ArmorMaterial$Layer.dyeable.()Z", &[a(3)]);
            if dyeable.i() != 0 { a(5) } else { V::I(WHITE) }
        }
        "getScopeOverlayTexture" => rt.string("minecraft:textures/misc/spyglass_scope.png"),
        _ => return None,
    })
}

fn block_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "of" => {
            let block = if desc.starts_with("(Lnet/minecraft/world/level/block/state/BlockState;)") { api::call(rt, STATE_BLOCK, &[a(0)]) } else { a(0) };
            ext_of(rt, Kind::Block, block)
        }
        "addHitEffects" | "playBreakSound" => V::I(0),
        // Vanilla's terrain particles, unless the state spawns none.
        "addDestroyEffects" => {
            let spawns = api::call(rt, "net/minecraft/world/level/block/state/BlockBehaviour$BlockStateBase.shouldSpawnTerrainParticles.()Z", &[a(1)]);
            V::I((spawns.i() == 0) as i32)
        }
        // `(state, level, pos, entity, originalColor, partialTick)`: the colour the game chose.
        "getFogColor" => a(5),
        "areBreakingParticlesTinted" => V::I(!rt.str_of(a(1)).is_some_and(|s| s == "minecraft:grass_block" || s.starts_with("minecraft:grass_block[")) as i32),
        _ => return None,
    })
}

fn effect_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "of" => {
            let effect = if desc.starts_with("(Lnet/minecraft/world/effect/MobEffectInstance;)") {
                let holder = api::call(rt, INSTANCE_EFFECT, &[a(0)]);
                api::call(rt, HOLDER_VALUE, &[holder])
            } else {
                a(0)
            };
            ext_of(rt, Kind::MobEffect, effect)
        }
        "isVisibleInInventory" | "isVisibleInGui" => V::I(1),
        "renderInventoryIcon" | "renderInventoryText" | "renderGuiIcon" => V::I(0),
        _ => return None,
    })
}

/// `RegisterClientExtensionsEvent.registerFluidType(extensions, types...)` (objects, or holders of
/// them) and `isFluidTypeRegistered(type)`, and the same of blocks, items and mob effects.
fn event_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let kind = Kind::of_register_name(name)?;
    if name.starts_with("is") {
        return Some(V::I(registered(rt, kind, a(1)).is_some() as i32));
    }
    let (ext, holders) = (a(1), desc.contains("[Lnet/minecraft/core/Holder;"));
    for target in crate::jutil::elements(rt, a(2)) {
        let target = if holders { api::call(rt, HOLDER_VALUE, &[target]) } else { target };
        register(rt, kind, target, ext);
    }
    Some(V::Void)
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    match owner {
        IFLUID | IITEM | IBLOCK | IEFFECT => true,
        EVENT | MANAGER => Kind::of_register_name(name).is_some(),
        _ => false,
    }
}

/// A client extension call, or None when this module doesn't model it.
pub(crate) fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    match owner {
        IFLUID => fluid_call(rt, name, desc, args),
        IITEM => item_call(rt, name, desc, args),
        IBLOCK => block_call(rt, name, desc, args),
        IEFFECT => effect_call(rt, name, desc, args),
        EVENT => event_call(rt, name, desc, args),
        // An `initializeClient` consumer's function: [target, extensions].
        MANAGER => {
            let kind = Kind::of_register_name(name)?;
            register(rt, kind, args.first().copied().unwrap_or_default(), args.get(1).copied().unwrap_or_default());
            Some(V::Void)
        }
        _ => None,
    }
}

/// `IClientFluidTypeExtensions.DEFAULT`...
pub(crate) fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    let kind = KINDS.into_iter().find(|k| k.interface() == owner)?;
    (field == "DEFAULT").then(|| default_ext(rt, kind))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::call as c;

    const OF_FLUID: &str = "net/neoforged/neoforge/client/extensions/common/IClientFluidTypeExtensions.of.(Lnet/minecraft/world/level/material/Fluid;)Lnet/neoforged/neoforge/client/extensions/common/IClientFluidTypeExtensions;";
    const OF_TYPE: &str = "net/neoforged/neoforge/client/extensions/common/IClientFluidTypeExtensions.of.(Lnet/neoforged/neoforge/fluids/FluidType;)Lnet/neoforged/neoforge/client/extensions/common/IClientFluidTypeExtensions;";
    const STILL_OF_STACK: &str = "net/neoforged/neoforge/client/extensions/common/IClientFluidTypeExtensions.getStillTexture.(Lnet/neoforged/neoforge/fluids/FluidStack;)Lnet/minecraft/resources/ResourceLocation;";
    const TINT_OF_STACK: &str = "net/neoforged/neoforge/client/extensions/common/IClientFluidTypeExtensions.getTintColor.(Lnet/neoforged/neoforge/fluids/FluidStack;)I";
    const FLOWING_OF_STACK: &str = "net/neoforged/neoforge/client/extensions/common/IClientFluidTypeExtensions.getFlowingTexture.(Lnet/neoforged/neoforge/fluids/FluidStack;)Lnet/minecraft/resources/ResourceLocation;";

    fn fluid(rt: &mut Rt, name: &str) -> V {
        api::get_static(rt, Box::leak(format!("net/minecraft/world/level/material/Fluids.{name}").into_boxed_str()))
    }

    fn text(rt: &Rt, v: V) -> Option<String> {
        rt.str_of(v).map(str::to_owned)
    }

    /// Without a vcall (no mod code), `virtual_call` is the API's own answer.
    fn ask(rt: &mut Rt, key: &'static str, ext: V) -> V {
        c(rt, key, &[ext, V::NULL])
    }

    /// Vanilla's water and lava answer as NeoForge's own extensions for them do: their block
    /// textures and water's blue, through the stack accessors Create's fluid renderer asks; a fluid
    /// with no extensions (empty) answers the interface's defaults.
    #[test]
    fn vanilla_fluids_have_neoforges_textures_and_tint() {
        let mut rt = Rt::new();
        let water = fluid(&mut rt, "WATER");
        let ext = c(&mut rt, OF_FLUID, &[water]);
        let still = ask(&mut rt, STILL_OF_STACK, ext);
        assert_eq!(text(&rt, still).as_deref(), Some("minecraft:block/water_still"));
        let flowing = ask(&mut rt, FLOWING_OF_STACK, ext);
        assert_eq!(text(&rt, flowing).as_deref(), Some("minecraft:block/water_flow"));
        assert_eq!(ask(&mut rt, TINT_OF_STACK, ext), V::I(0xFF3F_76E4_u32 as i32));
        // One extension for the fluid's type, whichever of its fluids asks.
        let flowing_water = fluid(&mut rt, "FLOWING_WATER");
        assert_eq!(c(&mut rt, OF_FLUID, &[flowing_water]), ext);

        let lava = fluid(&mut rt, "LAVA");
        let ext = c(&mut rt, OF_FLUID, &[lava]);
        let still = ask(&mut rt, STILL_OF_STACK, ext);
        assert_eq!(text(&rt, still).as_deref(), Some("minecraft:block/lava_still"));
        assert_eq!(ask(&mut rt, TINT_OF_STACK, ext), V::I(-1));

        let empty = fluid(&mut rt, "EMPTY");
        let ext = c(&mut rt, OF_FLUID, &[empty]);
        assert_eq!(ext, api::get_static(&mut rt, "net/neoforged/neoforge/client/extensions/common/IClientFluidTypeExtensions.DEFAULT"));
        assert_eq!(ask(&mut rt, STILL_OF_STACK, ext), V::NULL);
        assert_eq!(ask(&mut rt, TINT_OF_STACK, ext), V::I(-1));
    }

    /// A mod's fluid type gets the extensions it registered through `RegisterClientExtensionsEvent`
    /// (the type itself or its holder); its items, blocks and effects likewise, each kind apart.
    #[test]
    fn the_registration_event_registers_extensions_by_object_or_holder() {
        let mut rt = Rt::new();
        let goo = rt.new_object("net/neoforged/neoforge/fluids/FluidType");
        let ext = rt.new_object("test/GooExtensions");
        let holder = crate::neo::registry_holder(&mut rt, "fluid_type", "goo:goo", goo);
        let types = rt.alloc("[Lnet/minecraft/core/Holder;", Data::Array(vec![holder]));
        let ev = rt.new_object(EVENT);
        let registered = |rt: &mut Rt, what: &str, target: V| {
            let key: &'static str = Box::leak(format!("{EVENT}.is{what}Registered.(Ljava/lang/Object;)Z").into_boxed_str());
            c(rt, key, &[ev, target]) == V::I(1)
        };
        assert!(!registered(&mut rt, "FluidType", goo));
        c(&mut rt, "net/neoforged/neoforge/client/extensions/common/RegisterClientExtensionsEvent.registerFluidType.(Lnet/neoforged/neoforge/client/extensions/common/IClientFluidTypeExtensions;[Lnet/minecraft/core/Holder;)V", &[ev, ext, types]);
        assert!(registered(&mut rt, "FluidType", goo));
        assert_eq!(c(&mut rt, OF_TYPE, &[goo]), ext);

        let item = rt.new_object("test/WrenchItem");
        let item_ext = rt.new_object("test/WrenchExtensions");
        let items = rt.alloc("[Lnet/minecraft/world/item/Item;", Data::Array(vec![item]));
        c(&mut rt, "net/neoforged/neoforge/client/extensions/common/RegisterClientExtensionsEvent.registerItem.(Lnet/neoforged/neoforge/client/extensions/common/IClientItemExtensions;[Lnet/minecraft/world/item/Item;)V", &[ev, item_ext, items]);
        assert!(registered(&mut rt, "Item", item));
        assert!(!registered(&mut rt, "Block", item));
        let of_item = "net/neoforged/neoforge/client/extensions/common/IClientItemExtensions.of.(Lnet/minecraft/world/item/Item;)Lnet/neoforged/neoforge/client/extensions/common/IClientItemExtensions;";
        assert_eq!(c(&mut rt, of_item, &[item]), item_ext);
        // An item with none gets the interface's default, which draws nothing of its own.
        let other = rt.new_object("test/OtherItem");
        let default = c(&mut rt, of_item, &[other]);
        assert_eq!(default, api::get_static(&mut rt, "net/neoforged/neoforge/client/extensions/common/IClientItemExtensions.DEFAULT"));
        let bob = "net/neoforged/neoforge/client/extensions/common/IClientItemExtensions.shouldBobAsEntity.(Lnet/minecraft/world/item/ItemStack;)Z";
        assert_eq!(c(&mut rt, bob, &[default, V::NULL]), V::I(1));
    }

    const GOO_TYPE: &str = "test/GooFluidType";
    const GOO_EXT: &str = "test/GooFluidType$1";

    /// Mod code: the fluid type's `initializeClient` hands an extension whose plain `getTintColor`
    /// and `getStillTexture` are its own (as Create's tinted fluid types do).
    fn goo_overrides(class: &str, key: &str) -> bool {
        (class == GOO_TYPE && key == INITIALIZE) || (class == GOO_EXT && (key == GET_TINT.1 || key == GET_STILL.1))
    }

    fn goo_code(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {
        let recv = args.first().copied().unwrap_or_default();
        match (rt.class_of(recv), key) {
            (GOO_TYPE, INITIALIZE) => {
                let ext = rt.new_object(GOO_EXT);
                c(rt, "java/util/function/Consumer.accept.(Ljava/lang/Object;)V", &[args[1], ext])
            }
            (GOO_EXT, k) if k == GET_TINT.1 => V::I(0x7F12_3456),
            (GOO_EXT, k) if k == GET_STILL.1 => rt.string("goo:block/goo_still"),
            _ => c(rt, owner_key, args),
        }
    }

    /// At the client's setup, a registered fluid type whose class overrides `initializeClient` gives
    /// its extensions (NeoForge's `earlyInit`), and the stack accessors reach the extension's own
    /// plain ones; a type that doesn't override it keeps the default.
    #[test]
    fn initialize_client_overrides_hand_their_extensions_at_setup() {
        let mut rt = Rt::new();
        rt.overrides = Some(goo_overrides);
        rt.vcall = Some(goo_code);
        let goo = rt.new_object(GOO_TYPE);
        let plain = rt.new_object("net/neoforged/neoforge/fluids/FluidType");
        // Registered in NeoForge's registry of fluid types, as Registrate does on its event.
        let types = api::get_static(&mut rt, "net/neoforged/neoforge/registries/NeoForgeRegistries.FLUID_TYPES");
        for (name, ty) in [("goo:goo", goo), ("goo:plain", plain)] {
            let id = rt.string(name);
            c(&mut rt, "net/minecraft/core/Registry.register.(Lnet/minecraft/core/Registry;Lnet/minecraft/resources/ResourceLocation;Ljava/lang/Object;)Ljava/lang/Object;", &[types, id, ty]);
        }
        init(&mut rt);
        let ext = c(&mut rt, OF_TYPE, &[goo]);
        assert_eq!(rt.class_of(ext), GOO_EXT);
        assert_eq!(ask(&mut rt, TINT_OF_STACK, ext), V::I(0x7F12_3456));
        let still = ask(&mut rt, STILL_OF_STACK, ext);
        assert_eq!(text(&rt, still).as_deref(), Some("goo:block/goo_still"));
        // Its flowing texture is the interface's default: none.
        assert_eq!(ask(&mut rt, FLOWING_OF_STACK, ext), V::NULL);
        let ext = c(&mut rt, OF_TYPE, &[plain]);
        assert_eq!(ext, default_ext(&mut rt, Kind::FluidType));
    }

    /// A biome's water colour, in either file format.
    #[test]
    fn a_biomes_water_colour_reads_both_formats() {
        assert_eq!(biome_water_color(r##"{"effects":{"water_color":"#617b64"}}"##), Some(0x61_7B64));
        assert_eq!(biome_water_color(r#"{"effects":{"water_color":4159204}}"#), Some(0x3F_76E4));
        assert_eq!(biome_water_color(r#"{"effects":{}}"#), None);
    }
}
