//! What a block state answers of itself where it stands, as 1.21.1 and NeoForge answer it:
//!
//! - NeoForge's `IBlockStateExtension` methods: each asks the state's block for the method of the
//!   same name, the state first (`state.getFriction(level, pos, entity)` is
//!   `block.getFriction(state, level, pos, entity)`). A mod block's own answers; else
//!   `IBlockExtension`'s default, which is also what a mod's `super.` call reaches: friction from
//!   the block's `getFriction()`, light from the state's `getLightEmission()`, the clone stack from
//!   its `getCloneItemStack(level, pos, state)`, an enchanting bonus of 1 for a block in
//!   `enchantment_power_provider`, no running or landing effects or dynamic light of its own, no
//!   reaction to a neighbour's change or to catching fire, the dragon kept from `dragon_immune`
//!   blocks and the wither and its skulls from `wither_immune` ones, the state as its own
//!   appearance, the three airs as empty, the fluid overlay drawn on half-transparent blocks and
//!   leaves, weak power read through redstone conductors, ladders in `climbable`, beds of
//!   `BedBlock`, fences, walls and gates reaching up for collisions, fire and lava burning, the
//!   conduit's prismarine and sea lantern frames and the portal's obsidian one.
//! - Vanilla's reads of a block's properties: `isRedstoneConductor(level, pos)` (the predicate its
//!   properties hold: a mod's own, the one `ofFullCopy` took from a vanilla block, a vanilla
//!   block's never or always; else a full collision cube conducts), `getLightEmission()` (what
//!   `lightLevel` gave, a copied vanilla block's, a vanilla block's own: lit furnaces, candles by
//!   count, sea pickles in water...), `Block.getFriction()` (ice and slime slide), `Blocks.never`
//!   and `Blocks.always`, and the stack a block clones to (`getCloneItemStack(level, pos, state)`:
//!   its item, a crop's seeds, a stem's, a potted plant, nothing for a portal).
use crate::host::split_state;
use crate::{Rt, V, api};

const STATE: &str = "net/minecraft/world/level/block/state/BlockState";
const STATE_BASE: &str = "net/minecraft/world/level/block/state/BlockBehaviour$BlockStateBase";
const STATE_EXT: &str = "net/neoforged/neoforge/common/extensions/IBlockStateExtension";
const BLOCK_EXT: &str = "net/neoforged/neoforge/common/extensions/IBlockExtension";
const BLOCK_PACKAGE: &str = "net/minecraft/world/level/block/";
const BLOCKS: &str = "net/minecraft/world/level/block/Blocks";
const ITEM_STACK: &str = "net/minecraft/world/item/ItemStack";
const STACK_OF: &str =
    "net/minecraft/world/item/ItemStack.<init>.(Lnet/minecraft/world/level/ItemLike;)V";
const EMPTY_STACK: &str = "net/minecraft/world/item/ItemStack.EMPTY";
const FULL_BLOCK: &str = "net/minecraft/world/level/block/state/BlockState.isCollisionShapeFullBlock.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Z";
const PREDICATE_TEST: &str = "net/minecraft/world/level/block/state/BlockBehaviour$StatePredicate.test.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Z";
const TO_INT: &str = "java/util/function/ToIntFunction.applyAsInt.(Ljava/lang/Object;)I";
/// `isRedstoneConductor(level, pos)` on a state; `Blocks.never`/`always` as a `StatePredicate`.
const CONDUCTOR_DESC: &str =
    "(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Z";
const PREDICATE_DESC: &str = "(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Z";
/// `Blocks.never`/`always` as a `StateArgumentPredicate<EntityType<?>>` (`isValidSpawn`).
const SPAWN_PREDICATE_DESC: &str = "(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/EntityType;)Ljava/lang/Boolean;";
/// Vanilla's `Block.getCloneItemStack(level, pos, state)`, under NeoForge's own.
const CLONE_DESC: &str = "(Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/world/item/ItemStack;";
const CLONE: &str = "getCloneItemStack.(Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/world/item/ItemStack;";
const CLONE_KEY: &str = "net/minecraft/world/level/block/Block.getCloneItemStack.(Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/world/item/ItemStack;";
const FRICTION: &str = "getFriction.()F";
const FRICTION_KEY: &str = "net/minecraft/world/level/block/Block.getFriction.()F";
const SEED: &str = "getBaseSeedId.()Lnet/minecraft/world/level/ItemLike;";
const SEED_KEY: &str = "net/minecraft/world/level/block/CropBlock.getBaseSeedId.()Lnet/minecraft/world/level/ItemLike;";
const HEAD: &str = "getHeadBlock.()Lnet/minecraft/world/level/block/GrowingPlantHeadBlock;";
const HEAD_KEY: &str = "net/minecraft/world/level/block/GrowingPlantBodyBlock.getHeadBlock.()Lnet/minecraft/world/level/block/GrowingPlantHeadBlock;";

/// One of NeoForge's `IBlockStateExtension` methods: its name and descriptor on the state, and the
/// block's method it asks (`name.desc`, the state first) with that method's `IBlockExtension` key.
struct Ext {
    name: &'static str,
    state: &'static str,
    short: &'static str,
    key: &'static str,
}

macro_rules! ext {
    ($name:literal, $args:literal, $ret:literal) => {
        Ext {
            name: $name,
            state: concat!("(", $args, ")", $ret),
            short: concat!(
                $name,
                ".(Lnet/minecraft/world/level/block/state/BlockState;",
                $args,
                ")",
                $ret
            ),
            key: concat!(
                "net/neoforged/neoforge/common/extensions/IBlockExtension.",
                $name,
                ".(Lnet/minecraft/world/level/block/state/BlockState;",
                $args,
                ")",
                $ret
            ),
        }
    };
}

/// The extension methods answered here, as NeoForge 21.1 declares them.
const EXTENSIONS: [Ext; 21] = [
    ext!(
        "getFriction",
        "Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/Entity;",
        "F"
    ),
    ext!(
        "getLightEmission",
        "Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;",
        "I"
    ),
    ext!(
        "getCloneItemStack",
        "Lnet/minecraft/world/phys/HitResult;Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;",
        "Lnet/minecraft/world/item/ItemStack;"
    ),
    ext!(
        "getEnchantPowerBonus",
        "Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;",
        "F"
    ),
    ext!(
        "addRunningEffects",
        "Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/Entity;",
        "Z"
    ),
    ext!(
        "addLandingEffects",
        "Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/entity/LivingEntity;I",
        "Z"
    ),
    ext!(
        "canEntityDestroy",
        "Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/Entity;",
        "Z"
    ),
    ext!(
        "getAppearance",
        "Lnet/minecraft/world/level/BlockAndTintGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/BlockPos;",
        "Lnet/minecraft/world/level/block/state/BlockState;"
    ),
    ext!("isEmpty", "", "Z"),
    ext!(
        "shouldDisplayFluidOverlay",
        "Lnet/minecraft/world/level/BlockAndTintGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/material/FluidState;",
        "Z"
    ),
    ext!(
        "onNeighborChange",
        "Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/BlockPos;",
        "V"
    ),
    ext!(
        "onCaughtFire",
        "Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;Lnet/minecraft/world/entity/LivingEntity;",
        "V"
    ),
    ext!(
        "shouldCheckWeakPower",
        "Lnet/minecraft/world/level/SignalGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;",
        "Z"
    ),
    ext!(
        "isLadder",
        "Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/LivingEntity;",
        "Z"
    ),
    ext!(
        "getWeakChanges",
        "Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;",
        "Z"
    ),
    ext!(
        "isBed",
        "Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/LivingEntity;",
        "Z"
    ),
    ext!(
        "collisionExtendsVertically",
        "Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/Entity;",
        "Z"
    ),
    ext!("hasDynamicLightEmission", "", "Z"),
    ext!(
        "isBurning",
        "Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;",
        "Z"
    ),
    ext!(
        "isConduitFrame",
        "Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/BlockPos;",
        "Z"
    ),
    ext!(
        "isPortalFrame",
        "Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;",
        "Z"
    ),
];

fn state_class(owner: &str) -> bool {
    matches!(owner, STATE | STATE_BASE | STATE_EXT)
}

/// A block class a mod's call may name: `IBlockExtension`, or vanilla's `Block` and its kinds, as
/// `javac` names the class a `super.` call was written on.
fn block_class(owner: &str) -> bool {
    owner == BLOCK_EXT
        || (owner.starts_with(BLOCK_PACKAGE)
            && owner.ends_with("Block")
            && !owner[BLOCK_PACKAGE.len()..].contains('/'))
        || owner.starts_with("net/minecraft/world/level/block/piston/") && owner.ends_with("Block")
}

/// The extension method `name` with descriptor `desc` on a state (`on_state`) or on a block.
fn extension(name: &str, desc: &str, on_state: bool) -> Option<&'static Ext> {
    EXTENSIONS.iter().find(|e| {
        e.name == name
            && if on_state {
                e.state == desc
            } else {
                &e.short[e.name.len() + 1..] == desc
            }
    })
}

/// The keys [`call`] answers, by owner, name and descriptor alone.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    if state_class(owner) {
        return extension(name, desc, true).is_some()
            || (name == "isRedstoneConductor" && desc == CONDUCTOR_DESC)
            || (name == "getLightEmission" && desc == "()I");
    }
    if owner == BLOCKS {
        return matches!(name, "never" | "always")
            && matches!(desc, PREDICATE_DESC | SPAWN_PREDICATE_DESC);
    }
    block_class(owner)
        && (extension(name, desc, false).is_some()
            || (name == "getFriction" && desc == "()F")
            || (name == "getCloneItemStack" && desc == CLONE_DESC))
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    if owner == BLOCKS {
        let answer = V::I(i32::from(name == "always"));
        return Some(if desc == SPAWN_PREDICATE_DESC {
            crate::mixin::box_value(rt, answer, 'Z')
        } else {
            answer
        });
    }
    if state_class(owner) {
        match (name, desc) {
            ("isRedstoneConductor", CONDUCTOR_DESC) => {
                return Some(V::I(i32::from(conductor(rt, a(0), a(1), a(2)))));
            }
            ("getLightEmission", "()I") => return Some(V::I(light_emission(rt, a(0)))),
            _ => {}
        }
        let e = extension(name, desc, true)?;
        // The block's own, on the state and the call's arguments.
        let text = rt.str_of(a(0)).unwrap_or("minecraft:air").to_owned();
        if let Some(v) = crate::block_breaking::block_override(rt, &text, e.key, e.short, args) {
            return Some(v);
        }
        let (block_name, _) = split_state(&text);
        let block = api::block(rt, block_name);
        let mut all = vec![block];
        all.extend_from_slice(args);
        return extension_default(rt, e, &all);
    }
    // A block's: `this` first.
    match (name, desc) {
        ("getFriction", "()F") => Some(V::F(property_friction(rt, a(0)))),
        ("getCloneItemStack", CLONE_DESC) => clone_stack(rt, a(0), a(3)),
        _ => extension_default(rt, extension(name, desc, false)?, args),
    }
}

/// `IBlockExtension`'s default for `e` on `args` (the block, the state, the rest).
fn extension_default(rt: &mut Rt, e: &Ext, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let (block, state) = (a(0), a(1));
    let name = api::block_name(rt, block);
    let tagged = |rt: &Rt, tag: &str| rt.host().is_some_and(|h| h.block_in_tag(&name, tag));
    let yes = |b: bool| V::I(i32::from(b));
    Some(match e.name {
        "getFriction" => V::F(friction(rt, block)),
        "getLightEmission" => V::I(light_emission(rt, state)),
        "getCloneItemStack" => {
            // `self().getCloneItemStack(level, pos, state)`: the block's own, else vanilla's.
            if let Some(v) = own(rt, block, CLONE_KEY, CLONE, &[a(3), a(4), state]) {
                return Some(v);
            }
            return clone_stack(rt, block, state);
        }
        "getEnchantPowerBonus" => V::F(if tagged(rt, "minecraft:enchantment_power_provider") {
            1.0
        } else {
            0.0
        }),
        "addRunningEffects"
        | "addLandingEffects"
        | "getWeakChanges"
        | "hasDynamicLightEmission" => yes(false),
        "canEntityDestroy" => {
            let entity = a(4);
            if rt.instance_of(
                entity,
                "net/minecraft/world/entity/boss/enderdragon/EnderDragon",
            ) {
                yes(!tagged(rt, "minecraft:dragon_immune"))
            } else if rt.instance_of(entity, "net/minecraft/world/entity/boss/wither/WitherBoss")
                || rt.instance_of(entity, "net/minecraft/world/entity/projectile/WitherSkull")
            {
                // `state.isAir() || WitherBoss.canDestroy(state)`: air, or a block not immune.
                yes(is_air(&name) || !tagged(rt, "minecraft:wither_immune"))
            } else {
                yes(true)
            }
        }
        "getAppearance" => state,
        "isEmpty" => yes(is_air(&name)),
        "shouldDisplayFluidOverlay" => yes(rt.instance_of(
            block,
            "net/minecraft/world/level/block/HalfTransparentBlock",
        ) || rt
            .instance_of(block, "net/minecraft/world/level/block/LeavesBlock")),
        "onNeighborChange" | "onCaughtFire" => V::Void,
        "shouldCheckWeakPower" => yes(conductor(rt, state, a(2), a(3))),
        "isLadder" => yes(tagged(rt, "minecraft:climbable")),
        "isBed" => yes(rt.instance_of(block, "net/minecraft/world/level/block/BedBlock")),
        "collisionExtendsVertically" => yes(tagged(rt, "minecraft:fences")
            || tagged(rt, "minecraft:walls")
            || rt.instance_of(block, "net/minecraft/world/level/block/FenceGateBlock")),
        "isBurning" => yes(matches!(name.as_str(), "minecraft:fire" | "minecraft:lava")),
        "isConduitFrame" => yes(matches!(
            name.as_str(),
            "minecraft:prismarine"
                | "minecraft:prismarine_bricks"
                | "minecraft:sea_lantern"
                | "minecraft:dark_prismarine"
        )),
        "isPortalFrame" => yes(name == "minecraft:obsidian"),
        _ => return None,
    })
}

fn is_air(block: &str) -> bool {
    matches!(
        block,
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}

/// Mod block `block`'s own `short` (its class's or a mod superclass's), `key` being the API's, on
/// `args` after the block: None for a vanilla block, without one, or when it threw (told).
fn own(rt: &mut Rt, block: V, key: &'static str, short: &'static str, args: &[V]) -> Option<V> {
    let class = rt.class_of(block);
    if block.r() == 0 || !rt.is_mod_class(class) || !rt.overrides.is_some_and(|o| o(class, short)) {
        return None;
    }
    let vc = rt.vcall?;
    let mut all = vec![block];
    all.extend_from_slice(args);
    let r = vc(rt, key, short, &all);
    if rt.thrown.is_none() {
        return Some(r);
    }
    rt.report_thrown(short);
    None
}

/// The `BlockBehaviour.Properties` a mod block was made with; None for a vanilla block.
fn mod_properties(rt: &Rt, block: V) -> Option<V> {
    (block.r() != 0 && rt.str_of(block).is_none()).then(|| rt.get_field(block, "$properties"))
}

/// The vanilla block a mod block's properties copy (`ofFullCopy(Blocks.ICE)`), and whether all of
/// them (`ofFullCopy`, which takes the predicates too, not `ofLegacyCopy`).
fn copied(rt: &Rt, props: V) -> Option<(String, bool)> {
    let name = rt.str_of(rt.get_field(props, "$copy"))?.to_owned();
    Some((name, rt.get_field(props, "$fullCopy").i() != 0))
}

/// `Block.getFriction()` called on `block`: a mod block's own, else what its properties hold.
fn friction(rt: &mut Rt, block: V) -> f32 {
    match own(rt, block, FRICTION_KEY, FRICTION, &[]) {
        Some(v) => v.f(),
        None => property_friction(rt, block),
    }
}

/// The friction a block's properties give it: a mod's `friction(f)`, else the vanilla block's they
/// copy (both copies take it), else `Properties.of()`'s 0.6; a vanilla block's own.
fn property_friction(rt: &Rt, block: V) -> f32 {
    let Some(props) = mod_properties(rt, block) else {
        return vanilla_friction(rt.str_of(block).unwrap_or(""));
    };
    match rt.get_field(props, "$friction") {
        V::F(f) => f,
        _ => copied(rt, props).map_or(DEFAULT_FRICTION, |(c, _)| vanilla_friction(&c)),
    }
}

const DEFAULT_FRICTION: f32 = 0.6;

/// 1.21.1's `Blocks` that `friction(..)` sets.
fn vanilla_friction(block: &str) -> f32 {
    match block {
        "minecraft:ice" | "minecraft:packed_ice" | "minecraft:frosted_ice" => 0.98,
        "minecraft:blue_ice" => 0.989,
        "minecraft:slime_block" => 0.8,
        _ => DEFAULT_FRICTION,
    }
}

/// `BlockState.isRedstoneConductor(level, pos)`: the predicate the block's properties hold, else
/// vanilla's default, a full collision cube.
fn conductor(rt: &mut Rt, state: V, level: V, pos: V) -> bool {
    let text = rt.str_of(state).unwrap_or("minecraft:air").to_owned();
    let (name, _) = split_state(&text);
    let block = api::block(rt, name);
    let rule = match mod_properties(rt, block) {
        Some(props) => {
            let predicate = rt.get_field(props, "$isRedstoneConductor");
            if props.r() != 0 && predicate.r() != 0 {
                return rt
                    .call_functional(PREDICATE_TEST, predicate, &[state, level, pos])
                    .i()
                    != 0;
            }
            copied(rt, props)
                .filter(|(_, full)| *full)
                .and_then(|(c, _)| vanilla_conductor(&c))
        }
        None => vanilla_conductor(name),
    };
    rule.unwrap_or_else(|| api::call(rt, FULL_BLOCK, &[state, level, pos]).i() != 0)
}

/// The vanilla blocks 1.21.1's `Blocks` gives `isRedstoneConductor(Blocks::never)` (leaves, glass,
/// pistons, TNT, ice, glowstone, the copper grates and bulbs...) or `Blocks::always` (soul sand and
/// mud, which aren't full cubes); None for the rest, which conduct when they are full cubes.
fn vanilla_conductor(block: &str) -> Option<bool> {
    let path = block.strip_prefix("minecraft:")?;
    match path {
        "soul_sand" | "mud" => Some(true),
        p if p.ends_with("_leaves")
            || p.ends_with("stained_glass")
            || p.ends_with("copper_grate")
            || p.ends_with("copper_bulb") =>
        {
            Some(false)
        }
        "glass" | "tinted_glass" | "piston" | "sticky_piston" | "moving_piston" | "tnt" | "ice"
        | "frosted_ice" | "glowstone" | "beacon" | "redstone_block" | "sea_lantern"
        | "chorus_flower" | "observer" | "bamboo" | "scaffolding" | "powder_snow"
        | "pointed_dripstone" => Some(false),
        _ => None,
    }
}

/// `BlockState.getLightEmission()`: what the block's `lightLevel` function gives for the state; a
/// mod block's own function, else the vanilla block's its properties copy (both copies take it),
/// else none; a vanilla block's own.
fn light_emission(rt: &mut Rt, state: V) -> i32 {
    let text = rt.str_of(state).unwrap_or("minecraft:air").to_owned();
    let (name, props) = split_state(&text);
    let block = api::block(rt, name);
    let Some(p) = mod_properties(rt, block) else {
        return vanilla_light(name, &props);
    };
    let function = rt.get_field(p, "$light");
    if p.r() != 0 && function.r() != 0 {
        return rt.call_functional(TO_INT, function, &[state]).i();
    }
    copied(rt, p).map_or(0, |(c, _)| vanilla_light(&c, &props))
}

/// The light 1.21.1's `Blocks` gives a vanilla block's state (`lightLevel(...)`), reading the
/// properties its function reads off `props` (a property `props` lacks has its block's default).
fn vanilla_light(block: &str, props: &[(&str, &str)]) -> i32 {
    let Some(path) = block.strip_prefix("minecraft:") else {
        return 0;
    };
    let value = |k: &str| props.iter().find(|(n, _)| *n == k).map(|&(_, v)| v);
    let on = |k: &str, default: bool| value(k).map_or(default, |v| v == "true");
    let int = |k: &str, default: i32| value(k).and_then(|v| v.parse().ok()).unwrap_or(default);
    // `litBlockEmission(n)`: n while lit.
    let lit = |n: i32, default: bool| if on("lit", default) { n } else { 0 };
    match path {
        "furnace" | "smoker" | "blast_furnace" => lit(13, false),
        "redstone_ore" | "deepslate_redstone_ore" => lit(9, false),
        "redstone_torch" | "redstone_wall_torch" => lit(7, true),
        "redstone_lamp" => lit(15, false),
        "campfire" => lit(15, true),
        "soul_campfire" => lit(10, true),
        "copper_bulb" | "waxed_copper_bulb" => lit(15, false),
        "exposed_copper_bulb" | "waxed_exposed_copper_bulb" => lit(12, false),
        "weathered_copper_bulb" | "waxed_weathered_copper_bulb" => lit(8, false),
        "oxidized_copper_bulb" | "waxed_oxidized_copper_bulb" => lit(4, false),
        p if p.ends_with("candle_cake") => lit(3, false),
        // `CandleBlock.LIGHT_EMISSION`: 3 a lit candle.
        p if p == "candle" || p.ends_with("_candle") => lit(3 * int("candles", 1), false),
        // `SeaPickleBlock`: 3 and 3 a pickle, in water.
        "sea_pickle" => {
            if on("waterlogged", true) {
                3 + 3 * int("pickles", 1)
            } else {
                0
            }
        }
        // `RespawnAnchorBlock.getScaledChargeLevel(state, 15)`: floor(charge / 4 * 15).
        "respawn_anchor" => int("charge", 0) * 15 / 4,
        "light" => int("level", 15),
        // `GlowLichenBlock.emission(7)`: with a face anywhere.
        "glow_lichen" => {
            if ["north", "east", "south", "west", "up", "down"]
                .iter()
                .any(|d| on(*d, false))
            {
                7
            } else {
                0
            }
        }
        // `CaveVines.emission(14)`: with berries.
        "cave_vines" | "cave_vines_plant" => {
            if on("berries", false) {
                14
            } else {
                0
            }
        }
        // `TrialSpawnerState.lightLevel()` and `VaultState.lightLevel()`.
        "trial_spawner" => match value("trial_spawner_state").unwrap_or("inactive") {
            "waiting_for_players" => 4,
            "active" | "waiting_for_reward_ejection" | "ejecting_reward" => 8,
            _ => 0,
        },
        "vault" => match value("vault_state").unwrap_or("inactive") {
            "inactive" => 6,
            _ => 12,
        },
        "lava"
        | "fire"
        | "glowstone"
        | "jack_o_lantern"
        | "lava_cauldron"
        | "end_portal"
        | "beacon"
        | "sea_lantern"
        | "end_gateway"
        | "conduit"
        | "lantern"
        | "shroomlight"
        | "ochre_froglight"
        | "verdant_froglight"
        | "pearlescent_froglight" => 15,
        "torch" | "wall_torch" | "end_rod" => 14,
        "nether_portal" => 11,
        "soul_fire" | "soul_torch" | "soul_wall_torch" | "soul_lantern" | "crying_obsidian" => 10,
        "enchanting_table" | "ender_chest" => 7,
        "sculk_catalyst" => 6,
        "amethyst_cluster" => 5,
        "large_amethyst_bud" => 4,
        "magma_block" => 3,
        "medium_amethyst_bud" => 2,
        "brown_mushroom"
        | "brewing_stand"
        | "end_portal_frame"
        | "dragon_egg"
        | "small_amethyst_bud"
        | "sculk_sensor"
        | "calibrated_sculk_sensor" => 1,
        _ => 0,
    }
}

/// Vanilla's `Block.getCloneItemStack(level, pos, state)` for `block` (a mod block's is the vanilla
/// class's above it): a stack of its item, but what 1.21.1's classes give instead. None where the
/// answer rests on a block entity's data (a banner's patterns, a decorated pot's sherds, a shulker
/// box's contents) or on what a mod's stem, flower pot or light block keeps.
fn clone_stack(rt: &mut Rt, block: V, state: V) -> Option<V> {
    let is = |rt: &Rt, class: &str| rt.instance_of(block, &format!("{BLOCK_PACKAGE}{class}"));
    let name = api::block_name(rt, block);
    let vanilla = name
        .strip_prefix("minecraft:")
        .filter(|_| rt.str_of(block).is_some());
    if [
        "EndGatewayBlock",
        "EndPortalBlock",
        "FrostedIceBlock",
        "NetherPortalBlock",
    ]
    .iter()
    .any(|c| is(rt, c))
    {
        return Some(api::get_static(rt, EMPTY_STACK));
    }
    if [
        "AbstractBannerBlock",
        "DecoratedPotBlock",
        "ShulkerBoxBlock",
    ]
    .iter()
    .any(|c| is(rt, c))
    {
        return None;
    }
    let item = if is(rt, "CaveVinesBlock") || is(rt, "CaveVinesPlantBlock") {
        Some("glow_berries")
    } else if is(rt, "CropBlock") {
        // `getBaseSeedId()`: a mod crop's own, else vanilla's (wheat seeds unless the crop says).
        if let Some(seed) = own(rt, block, SEED_KEY, SEED, &[]) {
            return Some(stack_of(rt, seed));
        }
        Some(match vanilla {
            Some("beetroots") => "beetroot_seeds",
            Some("carrots") => "carrot",
            Some("potatoes") => "potato",
            Some("torchflower_crop") => "torchflower_seeds",
            _ => "wheat_seeds",
        })
    } else if is(rt, "StemBlock") || is(rt, "AttachedStemBlock") {
        Some(match vanilla? {
            "pumpkin_stem" | "attached_pumpkin_stem" => "pumpkin_seeds",
            "melon_stem" | "attached_melon_stem" => "melon_seeds",
            _ => return None,
        })
    } else if is(rt, "GrowingPlantBodyBlock") {
        if let Some(head) = own(rt, block, HEAD_KEY, HEAD, &[]) {
            return Some(stack_of(rt, head));
        }
        Some(match vanilla? {
            "kelp_plant" => "kelp",
            "weeping_vines_plant" => "weeping_vines",
            "twisting_vines_plant" => "twisting_vines",
            _ => return None,
        })
    } else if is(rt, "FlowerPotBlock") {
        // The plant potted (its block's item), the pot when empty.
        let path = vanilla?;
        let plant = match path.strip_prefix("potted_") {
            Some("azalea_bush") => "azalea".to_owned(),
            Some("flowering_azalea_bush") => "flowering_azalea".to_owned(),
            Some(p) => p.to_owned(),
            None => path.to_owned(),
        };
        let b = api::block(rt, &format!("minecraft:{plant}"));
        return Some(stack_of(rt, b));
    } else if is(rt, "LightBlock") {
        // The light block's item; one below full light also carries its level as a block state
        // component (`LightBlock.setLightOnStack`), not made here.
        let text = rt.str_of(state).unwrap_or("").to_owned();
        if split_state(&text)
            .1
            .iter()
            .any(|&(k, v)| k == "level" && v != "15")
        {
            return None;
        }
        None
    } else if is(rt, "BambooSaplingBlock") {
        Some("bamboo")
    } else if is(rt, "BigDripleafStemBlock") {
        Some("big_dripleaf")
    } else if is(rt, "CandleCakeBlock") {
        Some("cake")
    } else if is(rt, "NetherWartBlock") {
        Some("nether_wart")
    } else if is(rt, "SweetBerryBushBlock") {
        Some("sweet_berries")
    } else if is(rt, "TallSeagrassBlock") {
        Some("seagrass")
    } else {
        None
    };
    Some(match item {
        Some(i) => crate::api_common::stack(rt, &format!("minecraft:{i}"), 1),
        None => stack_of(rt, block),
    })
}

/// `new ItemStack(like)`.
fn stack_of(rt: &mut Rt, like: V) -> V {
    let s = rt.new_object(ITEM_STACK);
    api::call(rt, STACK_OF, &[s, like]);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    const CONDUCTS: &str = "net/minecraft/world/level/block/state/BlockState.isRedstoneConductor.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Z";
    const WEAK_POWER: &str = "net/minecraft/world/level/block/state/BlockState.shouldCheckWeakPower.(Lnet/minecraft/world/level/SignalGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Direction;)Z";
    const LIGHT: &str = "net/minecraft/world/level/block/state/BlockState.getLightEmission.()I";
    const LIGHT_AT: &str = "net/minecraft/world/level/block/state/BlockState.getLightEmission.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)I";
    const FRICTION_AT: &str = "net/minecraft/world/level/block/state/BlockState.getFriction.(Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/Entity;)F";
    const BONUS_AT: &str = "net/minecraft/world/level/block/state/BlockState.getEnchantPowerBonus.(Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;)F";
    const CLONE_AT: &str = "net/minecraft/world/level/block/state/BlockState.getCloneItemStack.(Lnet/minecraft/world/phys/HitResult;Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;)Lnet/minecraft/world/item/ItemStack;";
    const DESTROYS: &str = "net/minecraft/world/level/block/state/BlockState.canEntityDestroy.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/Entity;)Z";
    const EMPTY: &str = "net/minecraft/world/level/block/state/BlockState.isEmpty.()Z";
    const NEVER: &str = "net/minecraft/world/level/block/Blocks.never.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Z";
    const ALWAYS_SPAWN: &str = "net/minecraft/world/level/block/Blocks.always.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/EntityType;)Ljava/lang/Boolean;";
    const PROPS: &str = "net/minecraft/world/level/block/state/BlockBehaviour$Properties";
    const BLOCK_INIT: &str = "net/minecraft/world/level/block/Block.<init>.(Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;)V";
    const NEVER_CONDUCTS: &str = "test/NeverConducts";
    const GLOWING: &str = "test/GlowingBlock";
    const SLIPPERY: &str = "test/SlipperyBlock";
    const SHAFT: &str = "test/ShaftBlock";
    const PREDICATE: &str = "test.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Z";

    fn overrides(class: &str, short: &str) -> bool {
        match class {
            GLOWING => short == EXTENSIONS[1].short,
            SLIPPERY => short == FRICTION,
            SHAFT => short == CLONE,
            _ => false,
        }
    }

    /// The mod classes' own: a predicate that never conducts, a block that glows at 11 where it
    /// stands, one as slippery as 0.95 and a shaft that clones to a stick.
    fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        match (rt.class_of(args[0]), short) {
            (NEVER_CONDUCTS, PREDICATE) => V::I(0),
            (GLOWING, s) if s == EXTENSIONS[1].short => V::I(11),
            (SLIPPERY, FRICTION) => V::F(0.95),
            (SHAFT, CLONE) => crate::api_common::stack(rt, "minecraft:stick", 1),
            _ => api::call(rt, key, args),
        }
    }

    fn runtime() -> Rt {
        crate::install();
        let mut rt = Rt::new();
        rt.vcall = Some(vcall);
        rt.overrides = Some(overrides);
        rt
    }

    /// `Properties.<name>` on `this` with `args`, as a mod's builder calls it.
    fn with(rt: &mut Rt, this: V, name: &str, arg_desc: &str, args: &[V]) -> V {
        let key: &'static str = rt.static_key(&format!("{PROPS}.{name}.({arg_desc})L{PROPS};"));
        let mut all = vec![this];
        all.extend_from_slice(args);
        api::call(rt, key, &all)
    }

    fn properties(rt: &mut Rt) -> V {
        let key: &'static str = rt.static_key(&format!("{PROPS}.of.()L{PROPS};"));
        api::call(rt, key, &[])
    }

    /// `Properties.ofFullCopy`/`ofLegacyCopy` of vanilla block `vanilla`.
    fn copy(rt: &mut Rt, how: &str, vanilla: &str) -> V {
        let key: &'static str = rt.static_key(&format!(
            "{PROPS}.{how}.(Lnet/minecraft/world/level/block/state/BlockBehaviour;)L{PROPS};"
        ));
        let b = api::block(rt, vanilla);
        api::call(rt, key, &[b])
    }

    /// A mod block `id` of class `class`, made with `props`.
    fn mod_block(rt: &mut Rt, id: &str, class: &'static str, props: V) -> V {
        rt.supers
            .insert(class, "net/minecraft/world/level/block/Block");
        let b = rt.new_object(class);
        api::call(rt, BLOCK_INIT, &[b, props]);
        rt.registered.insert(("block".into(), id.into()), b);
        b
    }

    fn item_of(rt: &Rt, stack: V) -> String {
        crate::api_common::item_like_name(rt, rt.get_field(stack, "$item"))
    }

    /// A state's light is what its block's `lightLevel` function gives for it: 1.21.1's for a
    /// vanilla block (a lit furnace 13, three lit candles 9, four sea pickles 15 in water and none
    /// out of it, a respawn anchor charged twice 7), a mod block's copied from a vanilla block's
    /// (a redstone lamp's: 15 while lit). Where it stands, a mod block's own answer comes first.
    #[test]
    fn a_states_light_is_its_blocks_light_function() {
        let mut rt = runtime();
        for (text, light) in [
            ("minecraft:furnace[facing=north,lit=true]", 13),
            ("minecraft:furnace[facing=north,lit=false]", 0),
            ("minecraft:candle[candles=3,lit=true,waterlogged=false]", 9),
            ("minecraft:sea_pickle[pickles=4,waterlogged=true]", 15),
            ("minecraft:sea_pickle[pickles=4,waterlogged=false]", 0),
            ("minecraft:respawn_anchor[charge=2]", 7),
            ("minecraft:light[level=9,waterlogged=false]", 9),
            ("minecraft:redstone_wall_torch[facing=east,lit=true]", 7),
            (
                "minecraft:vault[facing=north,ominous=false,vault_state=active]",
                12,
            ),
            ("minecraft:glowstone", 15),
            ("minecraft:stone", 0),
        ] {
            let s = api::state(&mut rt, text);
            assert_eq!(api::call(&mut rt, LIGHT, &[s]), V::I(light), "{text}");
        }
        let lamp = copy(&mut rt, "ofLegacyCopy", "minecraft:redstone_lamp");
        mod_block(&mut rt, "testmod:lamp", "test/Lamp", lamp);
        let glowing = properties(&mut rt);
        mod_block(&mut rt, "testmod:glowing", GLOWING, glowing);
        let mut h = MemoryHost::default();
        let at = rt.with_host(&mut h, |rt| {
            let (level, pos) = (crate::glue::level(rt), api::block_pos(rt, 0, 64, 0));
            [
                "testmod:lamp[lit=true]",
                "testmod:lamp[lit=false]",
                "testmod:glowing",
            ]
            .map(|text| {
                let s = api::state(rt, text);
                (
                    api::call(rt, LIGHT, &[s]),
                    api::call(rt, LIGHT_AT, &[s, level, pos]),
                )
            })
        });
        assert_eq!(
            at,
            [
                (V::I(15), V::I(15)),
                (V::I(0), V::I(0)),
                (V::I(0), V::I(11))
            ]
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Redstone goes through a full collision cube (stone), not a half one (a slab), but what the
    /// block's properties say instead: glass never conducts, soul sand always does, and so do the
    /// mod blocks that copy them in full (`ofFullCopy`), not those that copy them the old way, nor a
    /// mod's own predicate's block. Weak power is read through a conductor (NeoForge).
    #[test]
    fn redstone_conducts_through_full_cubes_unless_the_block_says() {
        let mut rt = runtime();
        let full_glass = copy(&mut rt, "ofFullCopy", "minecraft:glass");
        mod_block(&mut rt, "testmod:full_glass", "test/FullGlass", full_glass);
        let legacy_glass = copy(&mut rt, "ofLegacyCopy", "minecraft:glass");
        mod_block(
            &mut rt,
            "testmod:legacy_glass",
            "test/LegacyGlass",
            legacy_glass,
        );
        rt.supers.insert(NEVER_CONDUCTS, "java/lang/Object");
        let predicate = rt.new_object(NEVER_CONDUCTS);
        let own = properties(&mut rt);
        let own = with(
            &mut rt,
            own,
            "isRedstoneConductor",
            "Lnet/minecraft/world/level/block/state/BlockBehaviour$StatePredicate;",
            &[predicate],
        );
        mod_block(&mut rt, "testmod:insulated", "test/Insulated", own);
        // A legacy copy of a mod block leaves its predicate behind.
        let insulated = rt.registered[&("block".to_owned(), "testmod:insulated".to_owned())];
        let legacy = {
            let key: &'static str = rt.static_key(&format!("{PROPS}.ofLegacyCopy.(Lnet/minecraft/world/level/block/state/BlockBehaviour;)L{PROPS};"));
            api::call(&mut rt, key, &[insulated])
        };
        mod_block(&mut rt, "testmod:plain", "test/Plain", legacy);

        let mut h = MemoryHost::default();
        let blocks = [
            "minecraft:stone",
            "minecraft:oak_slab[type=bottom,waterlogged=false]",
            "minecraft:glass",
            "minecraft:soul_sand",
            "testmod:full_glass",
            "testmod:legacy_glass",
            "testmod:insulated",
            "testmod:plain",
        ];
        for (x, b) in blocks.iter().enumerate() {
            h.blocks.insert([x as i32, 64, 0], (*b).to_owned());
        }
        h.collisions
            .insert([1, 64, 0], vec![[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]]);
        h.collisions
            .insert([3, 64, 0], vec![[0.0, 0.0, 0.0, 1.0, 0.875, 1.0]]);
        let (conducts, weak) = rt.with_host(&mut h, |rt| {
            let level = crate::glue::level(rt);
            let north = api::get_static(rt, "net/minecraft/core/Direction.NORTH");
            let mut weak = Vec::new();
            let conducts: Vec<bool> = blocks
                .iter()
                .enumerate()
                .map(|(x, text)| {
                    let (s, p) = (api::state(rt, text), api::block_pos(rt, x as i64, 64, 0));
                    weak.push(api::call(rt, WEAK_POWER, &[s, level, p, north]).i() != 0);
                    api::call(rt, CONDUCTS, &[s, level, p]).i() != 0
                })
                .collect();
            (conducts, weak)
        });
        assert_eq!(
            conducts,
            [true, false, false, true, false, true, false, true]
        );
        assert_eq!(weak, conducts);
        let (stone, never) = (
            api::state(&mut rt, "minecraft:stone"),
            api::call(&mut rt, NEVER, &[V::NULL, V::NULL, V::NULL]),
        );
        assert_eq!(never, V::I(0));
        let always = api::call(&mut rt, ALWAYS_SPAWN, &[stone, V::NULL, V::NULL, V::NULL]);
        assert_eq!(
            (rt.class_of(always), rt.get_field(always, "value")),
            ("java/lang/Boolean", V::I(1))
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// NeoForge's questions go to the block, a mod block's own first: friction (ice 0.98, slime
    /// 0.8, stone 0.6, a mod's `friction(0.8)`, a mod block's own `getFriction()`), the
    /// enchanting bonus of the blocks in `enchantment_power_provider`, the stack a block clones to
    /// (a crop's seeds, a potted azalea's azalea, nothing for an end portal, a mod shaft's own
    /// stick), the dragon kept from `dragon_immune` blocks, and air as empty.
    #[test]
    fn neoforge_asks_the_block_for_friction_bonus_clone_stack_and_more() {
        let mut rt = runtime();
        let sticky = properties(&mut rt);
        let sticky = with(&mut rt, sticky, "friction", "F", &[V::F(0.8)]);
        mod_block(&mut rt, "testmod:sticky", "test/Sticky", sticky);
        let slippery = properties(&mut rt);
        mod_block(&mut rt, "testmod:slippery", SLIPPERY, slippery);
        let shaft = properties(&mut rt);
        mod_block(&mut rt, "testmod:shaft", SHAFT, shaft);
        let dragon = rt.new_object("net/minecraft/world/entity/boss/enderdragon/EnderDragon");
        let cow = rt.new_object("net/minecraft/world/entity/animal/Cow");
        let mut h = MemoryHost::default();
        h.block_tags.insert(
            "minecraft:enchantment_power_provider".into(),
            vec!["minecraft:bookshelf".into()],
        );
        h.block_tags.insert(
            "minecraft:dragon_immune".into(),
            vec!["minecraft:obsidian".into()],
        );
        let (friction, bonus, clones, destroys, empty) = rt.with_host(&mut h, |rt| {
            let (level, pos) = (crate::glue::level(rt), api::block_pos(rt, 0, 64, 0));
            let on = |rt: &mut Rt, key: &'static str, text: &str, rest: &[V]| {
                let s = api::state(rt, text);
                let mut all = vec![s, level, pos];
                all.extend_from_slice(rest);
                api::call(rt, key, &all)
            };
            let friction: Vec<V> = [
                "minecraft:ice",
                "minecraft:slime_block",
                "minecraft:stone",
                "testmod:sticky",
                "testmod:slippery",
            ]
            .iter()
            .map(|t| on(rt, FRICTION_AT, t, &[V::NULL]))
            .collect();
            let bonus: Vec<V> = ["minecraft:bookshelf", "minecraft:stone"]
                .iter()
                .map(|t| on(rt, BONUS_AT, t, &[]))
                .collect();
            let clones: Vec<String> = [
                "minecraft:wheat[age=7]",
                "minecraft:carrots[age=0]",
                "minecraft:potted_azalea_bush",
                "minecraft:end_portal",
                "minecraft:stone",
                "testmod:shaft",
            ]
            .iter()
            .map(|t| {
                let s = api::state(rt, t);
                let stack = api::call(rt, CLONE_AT, &[s, V::NULL, level, pos, V::NULL]);
                item_of(rt, stack)
            })
            .collect();
            let destroys: Vec<V> = [
                (dragon, "minecraft:obsidian"),
                (dragon, "minecraft:stone"),
                (cow, "minecraft:obsidian"),
            ]
            .iter()
            .map(|&(e, t)| on(rt, DESTROYS, t, &[e]))
            .collect();
            let empty: Vec<V> = ["minecraft:cave_air", "minecraft:stone"]
                .iter()
                .map(|t| {
                    let s = api::state(rt, t);
                    api::call(rt, EMPTY, &[s])
                })
                .collect();
            (friction, bonus, clones, destroys, empty)
        });
        assert_eq!(
            friction,
            [V::F(0.98), V::F(0.8), V::F(0.6), V::F(0.8), V::F(0.95)]
        );
        assert_eq!(bonus, [V::F(1.0), V::F(0.0)]);
        assert_eq!(
            clones,
            [
                "minecraft:wheat_seeds",
                "minecraft:carrot",
                "minecraft:azalea",
                "minecraft:air",
                "minecraft:stone",
                "minecraft:stick"
            ]
        );
        assert_eq!(destroys, [V::I(0), V::I(1), V::I(1)]);
        assert_eq!(empty, [V::I(1), V::I(0)]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
