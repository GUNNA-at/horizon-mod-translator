//! Loot parameters as 1.21.1 has them (`LootParams.Builder`, `LootParams`), and a block's drops
//! through them (`BlockState.getDrops(builder)`). A mod block's own `getDrops(state,
//! LootParams.Builder)` answers for its block: Create's belt adds the shaft its pulley held, its
//! gearbox on a horizontal axis drops as the vertical one, its backtank keeps its air and
//! enchantments, its clipboard its pages, its cart assembler drops its rail too.
//! `BlockBehaviour.getDrops`, the loot table, is the game's: rolled with the builder's origin and
//! tool ([`crate::host::Host::block_drops`]), or, for a break whose loot the game rolled already (a
//! player's, an explosion's: [`crate::glue::loot_drops`]), those stacks.
//!
//! The builder keeps its parameters by identity (`LootContextParams.BLOCK_ENTITY` is one object), as
//! vanilla's `IdentityHashMap` does; `create` checks only the set's required ones, as NeoForge lets
//! mods pass their own.

use crate::{api, Data, Rt, V};

const BUILDER: &str = "net/minecraft/world/level/storage/loot/LootParams$Builder";
const PARAMS: &str = "net/minecraft/world/level/storage/loot/LootParams";
const DYNAMIC_DROP_ADD: &str = "net/minecraft/world/level/storage/loot/LootParams$DynamicDrop.add.(Ljava/util/function/Consumer;)V";
const PARAM_SET_REQUIRED: &str = "net/minecraft/world/level/storage/loot/parameters/LootContextParamSet.getRequired.()Ljava/util/Set;";
const STATE: &str = "net/minecraft/world/level/block/state/BlockState";
const STATE_BASE: &str = "net/minecraft/world/level/block/state/BlockBehaviour$BlockStateBase";
const BEHAVIOUR: &str = "net/minecraft/world/level/block/state/BlockBehaviour";
/// `BlockState.getDrops(builder)`.
const STATE_DROPS: &str = "(Lnet/minecraft/world/level/storage/loot/LootParams$Builder;)Ljava/util/List;";
/// A block's own `getDrops(state, builder)`.
pub(crate) const DROPS: &str = "getDrops.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/storage/loot/LootParams$Builder;)Ljava/util/List;";
const DROPS_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.getDrops.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/storage/loot/LootParams$Builder;)Ljava/util/List;";
const ORIGIN: &str = "net/minecraft/world/level/storage/loot/parameters/LootContextParams.ORIGIN";
const TOOL: &str = "net/minecraft/world/level/storage/loot/parameters/LootContextParams.TOOL";
const THIS_ENTITY: &str = "net/minecraft/world/level/storage/loot/parameters/LootContextParams.THIS_ENTITY";
const BLOCK_ENTITY: &str = "net/minecraft/world/level/storage/loot/parameters/LootContextParams.BLOCK_ENTITY";
const EXPLOSION_RADIUS: &str = "net/minecraft/world/level/storage/loot/parameters/LootContextParams.EXPLOSION_RADIUS";

/// Whether `owner` is a vanilla block class (a mod block's `super.getDrops` names its nearest one).
fn block_class(owner: &str) -> bool {
    owner == BEHAVIOUR || (owner.starts_with("net/minecraft/world/level/block/") && !owner.contains("/state/") && !owner.contains("/entity/"))
}

/// The keys [`call`] may answer.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    match owner {
        BUILDER => matches!(
            name,
            "<init>" | "getLevel" | "withParameter" | "withOptionalParameter" | "getParameter" | "getOptionalParameter" | "withDynamicDrop" | "withLuck" | "create"
        ),
        PARAMS => matches!(
            name,
            "<init>" | "getLevel" | "getLuck" | "hasParam" | "getParameter" | "getOptionalParameter" | "getParamOrNull" | "addDynamicDrops"
        ),
        STATE | STATE_BASE => name == "getDrops" && desc == STATE_DROPS,
        o if block_class(o) => DROPS.split_once('.') == Some((name, desc)),
        _ => false,
    }
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        (BUILDER, "<init>") => {
            init(rt, a(0), a(1));
            V::Void
        }
        (BUILDER | PARAMS, "getLevel") => rt.get_field(a(0), "level"),
        (PARAMS, "getLuck") => rt.get_field(a(0), "luck"),
        // `new LootParams(level, params, dynamicDrops, luck)`, a mod's own.
        (PARAMS, "<init>") => {
            for (i, field) in ["level", "params", "dynamicDrops", "luck"].into_iter().enumerate() {
                rt.put_field(a(0), field, a(i + 1));
            }
            V::Void
        }
        (BUILDER, "withParameter") => {
            put(rt, a(0), a(1), a(2));
            a(0)
        }
        // A null value takes the parameter out.
        (BUILDER, "withOptionalParameter") => {
            if a(2).r() == 0 {
                let params = rt.get_field(a(0), "params");
                crate::jutil::table_remove(rt, params, a(1));
            } else {
                put(rt, a(0), a(1), a(2));
            }
            a(0)
        }
        (BUILDER | PARAMS, "getParameter") => match param(rt, a(0), a(1)) {
            Some(v) => v,
            None => {
                let name = crate::jlang::text(rt, rt.get_field(a(1), "$name"));
                throw(rt, "java/util/NoSuchElementException", &name)
            }
        },
        (BUILDER | PARAMS, "getOptionalParameter" | "getParamOrNull") => param(rt, a(0), a(1)).unwrap_or(V::NULL),
        (PARAMS, "hasParam") => V::I(param(rt, a(0), a(1)).is_some() as i32),
        (BUILDER, "withDynamicDrop") => {
            let drops = rt.get_field(a(0), "dynamicDrops");
            if crate::jutil::table_put(rt, drops, a(1), a(2)).r() != 0 {
                let name = crate::jlang::text(rt, a(1));
                return Some(throw(rt, "java/lang/IllegalStateException", &format!("Duplicated dynamic drop '{name}'")));
            }
            a(0)
        }
        (BUILDER, "withLuck") => {
            rt.put_field(a(0), "luck", a(1));
            a(0)
        }
        (BUILDER, "create") => create(rt, a(0), a(1)),
        // What the loot table's `minecraft:dynamic` entry of that name adds (a shulker box's
        // contents): the drop given to the builder, if any.
        (PARAMS, "addDynamicDrops") => {
            let drops = rt.get_field(a(0), "dynamicDrops");
            if let Some(drop) = crate::jutil::table_get(rt, drops, a(1)).filter(|d| d.r() != 0) {
                rt.call_functional(DYNAMIC_DROP_ADD, drop, &[a(2)]);
            }
            V::Void
        }
        (STATE | STATE_BASE, _) => state_drops(rt, a(0), a(1)),
        // A vanilla block's own, a mod block's `super.` call among them: none of vanilla's blocks a
        // mod extends has its own (a shulker box's contents are the game's), so the loot table.
        _ => own_drops(rt, a(1), a(2)),
    })
}

/// A new builder's state: its level, no parameters, no dynamic drops, no luck.
fn init(rt: &mut Rt, b: V, level: V) {
    let params = rt.alloc("java/util/IdentityHashMap", Data::Table(Default::default()));
    let dynamic = rt.alloc("java/util/HashMap", Data::Table(Default::default()));
    rt.put_field(b, "level", level);
    rt.put_field(b, "params", params);
    rt.put_field(b, "dynamicDrops", dynamic);
    rt.put_field(b, "luck", V::F(0.0));
}

fn put(rt: &mut Rt, b: V, param: V, value: V) {
    let params = rt.get_field(b, "params");
    crate::jutil::table_put(rt, params, param, value);
}

/// What the builder or the parameters `o` hold for `param`, if anything.
fn param(rt: &mut Rt, o: V, param: V) -> Option<V> {
    let params = rt.get_field(o, "params");
    crate::jutil::table_get(rt, params, param).filter(|v| v.r() != 0)
}

/// The value of vanilla's parameter `key` (`LootContextParams.TOOL`) the builder holds, if any.
fn vanilla_param(rt: &mut Rt, b: V, key: &'static str) -> Option<V> {
    let p = api::get_static(rt, key);
    param(rt, b, p)
}

fn throw(rt: &mut Rt, class: &'static str, message: &str) -> V {
    let e = rt.new_object(class);
    let m = rt.string(message);
    rt.put_field(e, "message", m);
    rt.thrown = Some(e);
    V::NULL
}

/// `Builder.create(set)`: the parameters, once the set's required ones are all there.
fn create(rt: &mut Rt, b: V, set: V) -> V {
    let required = api::call(rt, PARAM_SET_REQUIRED, &[set]);
    let mut missing = Vec::new();
    for p in crate::jutil::elements(rt, required) {
        if param(rt, b, p).is_none() {
            missing.push(crate::jlang::text(rt, rt.get_field(p, "$name")));
        }
    }
    if !missing.is_empty() {
        return throw(rt, "java/lang/IllegalArgumentException", &format!("Missing required parameters: [{}]", missing.join(", ")));
    }
    let p = rt.new_object(PARAMS);
    for field in ["level", "params", "dynamicDrops", "luck"] {
        let v = rt.get_field(b, field);
        rt.put_field(p, field, v);
    }
    p
}

/// A new builder in `level`.
pub(crate) fn builder(rt: &mut Rt, level: V) -> V {
    let b = rt.new_object(BUILDER);
    init(rt, b, level);
    b
}

/// The builder `Block.getDrops(state, level, pos, blockEntity, entity, tool)` makes: the block's
/// centre as the origin and the tool (none: `ItemStack.EMPTY`), with the entity and the block
/// entity when there are.
pub(crate) fn block_builder(rt: &mut Rt, level: V, pos: V, tool: V, entity: V, be: V) -> V {
    let b = builder(rt, level);
    let [x, y, z] = api::pos_of(rt, pos);
    let origin = crate::api_common::vec3(rt, f64::from(x) + 0.5, f64::from(y) + 0.5, f64::from(z) + 0.5);
    let tool = if tool.r() == 0 { crate::hand::load(rt, None) } else { tool };
    for (key, value) in [(ORIGIN, origin), (TOOL, tool), (THIS_ENTITY, entity), (BLOCK_ENTITY, be)] {
        if value.r() != 0 {
            let p = api::get_static(rt, key);
            put(rt, b, p, value);
        }
    }
    b
}

/// The explosion's radius on `b` (`LootContextParams.EXPLOSION_RADIUS`, a `Float`), as
/// `BlockBehaviour.onExplosionHit` gives it.
pub(crate) fn with_explosion(rt: &mut Rt, b: V, radius: f32) {
    let p = api::get_static(rt, EXPLOSION_RADIUS);
    let r = crate::mixin::box_value(rt, V::F(radius), 'F');
    put(rt, b, p, r);
}

/// Makes `stacks` what `b`'s loot table of state `st` gives, once: the game rolled them for the
/// break `b` tells of.
pub(crate) fn rolled(rt: &mut Rt, b: V, st: V, stacks: Vec<V>) {
    let list = crate::jutil::new_list(rt, stacks);
    rt.put_field(b, "$rolled", list);
    rt.put_field(b, "$rolledState", st);
}

/// The static `Block.getDrops(state, level, pos, blockEntity, entity, tool)`: `state.getDrops` with
/// [`block_builder`]'s parameters.
pub(crate) fn block_drops(rt: &mut Rt, st: V, level: V, pos: V, be: V, entity: V, tool: V) -> V {
    let b = block_builder(rt, level, pos, tool, entity, be);
    state_drops(rt, st, b)
}

/// `BlockState.getDrops(builder)`: the mod block's own `getDrops(state, builder)`, else its loot.
pub(crate) fn state_drops(rt: &mut Rt, st: V, b: V) -> V {
    let text = crate::api_common::name_of(rt, st);
    match crate::block_breaking::block_override(rt, &text, DROPS_KEY, DROPS, &[st, b]) {
        Some(v) => v,
        None => own_drops(rt, st, b),
    }
}

/// `BlockBehaviour.getDrops(state, builder)`: the block's loot table, which the game rolls with the
/// builder's origin and tool, unless it rolled this state's already for the break the builder
/// tells of ([`rolled`]): those stacks then, once.
fn own_drops(rt: &mut Rt, st: V, b: V) -> V {
    let text = crate::api_common::name_of(rt, st);
    let kept = rt.get_field(b, "$rolled");
    if kept.r() != 0 && rt.str_of(rt.get_field(b, "$rolledState")) == Some(text.as_str()) {
        rt.put_field(b, "$rolled", V::NULL);
        return kept;
    }
    // No origin: nowhere to roll it (vanilla's `create` would throw for the missing parameter).
    let Some(origin) = vanilla_param(rt, b, ORIGIN) else { return crate::jutil::new_list(rt, Vec::new()) };
    let [x, y, z] = crate::api_common::xyz(rt, origin);
    let at = [x.floor() as i32, y.floor() as i32, z.floor() as i32];
    let tool = vanilla_param(rt, b, TOOL).unwrap_or(V::NULL);
    let tool = crate::block_breaking::held(rt, tool);
    let got = rt.host_mut().map(|h| h.block_drops(at, &text, tool.as_ref())).unwrap_or_default();
    let stacks: Vec<V> = got.into_iter().map(|s| crate::hand::load(rt, Some(s))).collect();
    crate::jutil::new_list(rt, stacks)
}

#[cfg(test)]
mod tests {
    use super::*;

    const INIT: &str = "net/minecraft/world/level/storage/loot/LootParams$Builder.<init>.(Lnet/minecraft/server/level/ServerLevel;)V";
    const WITH: &str = "net/minecraft/world/level/storage/loot/LootParams$Builder.withParameter.(Lnet/minecraft/world/level/storage/loot/parameters/LootContextParam;Ljava/lang/Object;)Lnet/minecraft/world/level/storage/loot/LootParams$Builder;";
    const WITH_OPTIONAL: &str = "net/minecraft/world/level/storage/loot/LootParams$Builder.withOptionalParameter.(Lnet/minecraft/world/level/storage/loot/parameters/LootContextParam;Ljava/lang/Object;)Lnet/minecraft/world/level/storage/loot/LootParams$Builder;";
    const GET: &str = "net/minecraft/world/level/storage/loot/LootParams$Builder.getParameter.(Lnet/minecraft/world/level/storage/loot/parameters/LootContextParam;)Ljava/lang/Object;";
    const OPTIONAL: &str = "net/minecraft/world/level/storage/loot/LootParams$Builder.getOptionalParameter.(Lnet/minecraft/world/level/storage/loot/parameters/LootContextParam;)Ljava/lang/Object;";
    const LUCK: &str = "net/minecraft/world/level/storage/loot/LootParams$Builder.withLuck.(F)Lnet/minecraft/world/level/storage/loot/LootParams$Builder;";
    const CREATE: &str = "net/minecraft/world/level/storage/loot/LootParams$Builder.create.(Lnet/minecraft/world/level/storage/loot/parameters/LootContextParamSet;)Lnet/minecraft/world/level/storage/loot/LootParams;";
    const HAS: &str = "net/minecraft/world/level/storage/loot/LootParams.hasParam.(Lnet/minecraft/world/level/storage/loot/parameters/LootContextParam;)Z";
    const PARAMS_GET: &str = "net/minecraft/world/level/storage/loot/LootParams.getParameter.(Lnet/minecraft/world/level/storage/loot/parameters/LootContextParam;)Ljava/lang/Object;";
    const GET_LUCK: &str = "net/minecraft/world/level/storage/loot/LootParams.getLuck.()F";
    const GET_LEVEL: &str = "net/minecraft/world/level/storage/loot/LootParams.getLevel.()Lnet/minecraft/server/level/ServerLevel;";
    const SET_BUILDER: &str = "net/minecraft/world/level/storage/loot/parameters/LootContextParamSet$Builder";
    const SET_BUILDER_INIT: &str = "net/minecraft/world/level/storage/loot/parameters/LootContextParamSet$Builder.<init>.()V";
    const REQUIRED: &str = "net/minecraft/world/level/storage/loot/parameters/LootContextParamSet$Builder.required.(Lnet/minecraft/world/level/storage/loot/parameters/LootContextParam;)Lnet/minecraft/world/level/storage/loot/parameters/LootContextParamSet$Builder;";
    const BUILD: &str = "net/minecraft/world/level/storage/loot/parameters/LootContextParamSet$Builder.build.()Lnet/minecraft/world/level/storage/loot/parameters/LootContextParamSet;";
    const BLOCK_SET: &str = "net/minecraft/world/level/storage/loot/parameters/LootContextParamSets.BLOCK";

    /// A builder keeps its parameters by identity: a null optional one takes it out, a missing one
    /// throws `NoSuchElementException` naming it. `create` checks the set's required parameters (a
    /// mod's set; vanilla's sets take what the game gives them), then the parameters answer as the
    /// builder held them, with its level and luck.
    #[test]
    fn a_builder_holds_its_parameters_until_create_checks_the_required_ones() {
        let mut rt = Rt::new();
        crate::install();
        let level = crate::glue::level(&mut rt);
        let b = rt.new_object(BUILDER);
        api::call(&mut rt, INIT, &[b, level]);
        let (tool, entity) = (api::get_static(&mut rt, TOOL), api::get_static(&mut rt, THIS_ENTITY));
        let pick = crate::api_common::stack(&mut rt, "minecraft:iron_pickaxe", 1);
        assert_eq!(api::call(&mut rt, WITH, &[b, tool, pick]), b);
        assert_eq!(api::call(&mut rt, GET, &[b, tool]), pick);
        assert_eq!(api::call(&mut rt, GET, &[b, entity]), V::NULL);
        let e = rt.thrown.take().expect("a missing parameter throws");
        assert_eq!(rt.class_of(e), "java/util/NoSuchElementException");
        assert_eq!(rt.str_of(rt.get_field(e, "message")), Some("minecraft:this_entity"));
        api::call(&mut rt, WITH_OPTIONAL, &[b, tool, V::NULL]);
        assert_eq!(api::call(&mut rt, OPTIONAL, &[b, tool]), V::NULL, "a null optional takes it out");
        api::call(&mut rt, WITH_OPTIONAL, &[b, tool, pick]);
        api::call(&mut rt, LUCK, &[b, V::F(1.5)]);
        // Vanilla's block set: nothing to check.
        let block_set = api::get_static(&mut rt, BLOCK_SET);
        assert_ne!(api::call(&mut rt, CREATE, &[b, block_set]).r(), 0);
        assert!(rt.thrown.is_none());
        // A mod's set that requires the entity.
        let sb = rt.new_object(SET_BUILDER);
        api::call(&mut rt, SET_BUILDER_INIT, &[sb]);
        api::call(&mut rt, REQUIRED, &[sb, entity]);
        let set = api::call(&mut rt, BUILD, &[sb]);
        assert_eq!(api::call(&mut rt, CREATE, &[b, set]), V::NULL);
        let e = rt.thrown.take().expect("a required parameter is missing");
        assert_eq!(rt.class_of(e), "java/lang/IllegalArgumentException");
        let player = rt.new_object("net/minecraft/server/level/ServerPlayer");
        api::call(&mut rt, WITH, &[b, entity, player]);
        let params = api::call(&mut rt, CREATE, &[b, set]);
        assert!(rt.thrown.is_none());
        assert_eq!(api::call(&mut rt, HAS, &[params, entity]).i(), 1);
        assert_eq!(api::call(&mut rt, PARAMS_GET, &[params, tool]), pick);
        assert_eq!(api::call(&mut rt, GET_LUCK, &[params]).f(), 1.5);
        assert_eq!(api::call(&mut rt, GET_LEVEL, &[params]), level);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
