//! What a mod's blocks do to the entities that touch them, as 1.21.1's `Entity.move` reports it:
//! `Block.stepOn(level, pos, state, entity)` to the block under an entity on the ground, and
//! `BlockBehaviour.entityInside(state, level, pos, entity)` to each block whose cell its box is in
//! (`Entity.checkInsideBlocks`), a block's own method. A running saw or drill hurts what is in it, a
//! thorny bush pricks it, a hot block burns what stands on it unless it steps carefully.
//!
//! The game reports a contact inside the entity's move, where no level is at hand: its hooks run
//! these once the level's tick is over, a player's as that player (their state in place), so the
//! entity is the one that touched the block, where its move left it. An item entity in a block
//! with a block entity is that block entity's tick's ([`crate::entities::item_contacts`], which
//! also tells the block what lands on it).

use crate::host::Host;
use crate::{Rt, V, api};

const STEP_ON: &str = "stepOn.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/entity/Entity;)V";
const STEP_ON_KEY: &str = "net/minecraft/world/level/block/Block.stepOn.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/entity/Entity;)V";
const ENTITY_INSIDE: &str = "entityInside.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/Entity;)V";
const ENTITY_INSIDE_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.entityInside.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/Entity;)V";

/// `native_block_entity_inside`: the entity `entity`, of type `kind` (the acting player when a
/// player), is in the mod's block at `pos`, in state `state`: the block's own `entityInside`.
/// Whether it ran.
pub fn entity_inside(
    rt: &mut Rt,
    host: &mut dyn Host,
    pos: [i32; 3],
    state: &str,
    entity: i32,
    kind: &str,
) -> bool {
    contact(
        rt,
        host,
        pos,
        state,
        entity,
        kind,
        ENTITY_INSIDE_KEY,
        ENTITY_INSIDE,
    )
}

/// `native_block_step_on`: the entity `entity`, of type `kind` (the acting player when a player),
/// stands on the mod's block at `pos`, in state `state`: the block's own `stepOn`. Whether it ran.
pub fn step_on(
    rt: &mut Rt,
    host: &mut dyn Host,
    pos: [i32; 3],
    state: &str,
    entity: i32,
    kind: &str,
) -> bool {
    contact(rt, host, pos, state, entity, kind, STEP_ON_KEY, STEP_ON)
}

fn contact(
    rt: &mut Rt,
    host: &mut dyn Host,
    pos: [i32; 3],
    state: &str,
    entity: i32,
    kind: &str,
    key: &'static str,
    short: &'static str,
) -> bool {
    // A hook's outermost entry: no translated frame holds values, so garbage can go.
    rt.safe_point();
    let Some((b, class)) = crate::glue::mod_block(rt, state) else {
        return false;
    };
    // `Block`'s own are empty: only a block's own does anything.
    if !rt.overrides.is_some_and(|o| o(class, short)) {
        return false;
    }
    let Some(vc) = rt.vcall else { return false };
    rt.with_host(host, |rt| {
        if kind == "minecraft:item" && rt.block_entities.contains_key(&pos) {
            return false;
        }
        let e = object(rt, entity, kind);
        if e.r() == 0 {
            return false;
        }
        let level = crate::glue::level(rt);
        let st = api::state(rt, state);
        let p = api::block_pos(rt, pos[0] as i64, pos[1] as i64, pos[2] as i64);
        let args = if short == STEP_ON {
            [b, level, p, st, e]
        } else {
            [b, st, level, p, e]
        };
        vc(rt, key, short, &args);
        rt.report_thrown(if short == STEP_ON {
            "stepOn"
        } else {
            "entityInside"
        });
        true
    })
}

/// The object of the entity a contact names: a client's own player's, the acting player's (the hook
/// runs as them), else the game's entity of that id (an item entity's own object for an item).
fn object(rt: &mut Rt, id: i32, kind: &str) -> V {
    // A client's own player: the one object its runtime keeps for them (a belt's passengers are
    // keyed by it).
    if kind == "minecraft:player" && rt.host().and_then(|h| h.client_player_id()) == Some(id) {
        return crate::network::local_player(rt);
    }
    if kind == "minecraft:player" {
        let Some(uuid) = rt.host().and_then(|h| h.player_id()) else {
            return V::NULL;
        };
        let at = rt
            .host()
            .and_then(|h| h.player_place())
            .map_or([0.0; 3], |p| p.0);
        return crate::vanilla_entities::player(rt, id, uuid, at);
    }
    crate::vanilla_entities::by_id(rt, id).unwrap_or(V::NULL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{EffectView, EntityView, ItemEntity, MemoryHost};

    /// A saw as Create writes one, cut down: what is in it, not an item, is hurt by 3 (a cactus's
    /// damage type stands for Create's saw's).
    const SAW: &str = "test/SawBlock";
    /// A hot plate, as the pack's magma-like blocks: what stands on it gets fire resistance's
    /// opposite here, slowness, unless it steps carefully.
    const PLATE: &str = "test/HotPlateBlock";
    /// A pool of dye, a block with no block entity: an item in it is dyed (its stack becomes red
    /// wool).
    const DYE: &str = "test/DyeBlock";
    /// A belt as Create's carries what is in it, cut down: a quarter block along it, and an eighth
    /// toward its middle unless the entity walks (`xxa`, `zza`), by the entity's own `move`.
    const BELT: &str = "test/BeltBlock";
    const ENTITY: &str = "net/minecraft/world/entity/Entity";
    const LIVING: &str = "net/minecraft/world/entity/LivingEntity";

    fn overrides(class: &str, key: &str) -> bool {
        matches!(
            (class, key),
            (SAW, ENTITY_INSIDE) | (PLATE, STEP_ON) | (DYE, ENTITY_INSIDE) | (BELT, ENTITY_INSIDE)
        )
    }

    thread_local! {
        /// The entity the belt last carried.
        static CARRIED: std::cell::Cell<V> = const { std::cell::Cell::new(V::NULL) };
    }

    fn call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> V {
        let key = rt.static_key(&format!("{owner}.{name}.{desc}"));
        api::call(rt, key, args)
    }

    fn is(rt: &Rt, e: V, class: &str) -> bool {
        crate::api_common::api_is_a(rt.class_of(e), class)
    }

    fn vcall(rt: &mut Rt, owner_key: &'static str, key: &'static str, args: &[V]) -> V {
        let class = rt.class_of(args[0]);
        if !overrides(class, key) {
            return api::call(rt, owner_key, args);
        }
        match (class, key) {
            (SAW, _) => {
                let (level, e) = (args[2], args[4]);
                if !is(rt, e, "net/minecraft/world/entity/item/ItemEntity") {
                    let sources = call(
                        rt,
                        "net/minecraft/world/level/Level",
                        "damageSources",
                        "()Lnet/minecraft/world/damagesource/DamageSources;",
                        &[level],
                    );
                    let source = call(
                        rt,
                        "net/minecraft/world/damagesource/DamageSources",
                        "cactus",
                        "()Lnet/minecraft/world/damagesource/DamageSource;",
                        &[sources],
                    );
                    call(
                        rt,
                        ENTITY,
                        "hurt",
                        "(Lnet/minecraft/world/damagesource/DamageSource;F)Z",
                        &[e, source, V::F(3.0)],
                    );
                }
            }
            (PLATE, _) => {
                let e = args[4];
                let careful = call(rt, ENTITY, "isSteppingCarefully", "()Z", &[e]).i() != 0;
                if !careful && is(rt, e, LIVING) {
                    let slowness = api::get_static(
                        rt,
                        "net/minecraft/world/effect/MobEffects.MOVEMENT_SLOWDOWN",
                    );
                    let instance = rt.new_object("net/minecraft/world/effect/MobEffectInstance");
                    call(
                        rt,
                        "net/minecraft/world/effect/MobEffectInstance",
                        "<init>",
                        "(Lnet/minecraft/core/Holder;I)V",
                        &[instance, slowness, V::I(60)],
                    );
                    call(
                        rt,
                        LIVING,
                        "addEffect",
                        "(Lnet/minecraft/world/effect/MobEffectInstance;)Z",
                        &[e, instance],
                    );
                }
            }
            (DYE, _) => {
                let e = args[4];
                let item = "net/minecraft/world/entity/item/ItemEntity";
                if is(rt, e, item) {
                    let wool = crate::api_common::get_static(
                        rt,
                        "net/minecraft/world/item/Items",
                        "RED_WOOL",
                    )
                    .unwrap_or(V::NULL);
                    let stack = rt.new_object("net/minecraft/world/item/ItemStack");
                    call(
                        rt,
                        "net/minecraft/world/item/ItemStack",
                        "<init>",
                        "(Lnet/minecraft/world/level/ItemLike;)V",
                        &[stack, wool],
                    );
                    call(
                        rt,
                        item,
                        "setItem",
                        "(Lnet/minecraft/world/item/ItemStack;)V",
                        &[e, stack],
                    );
                }
            }
            (BELT, _) => {
                let e = args[4];
                let walks = ["$xxa", "$zza"]
                    .into_iter()
                    .any(|input| call(rt, LIVING, input, "()F", &[e]).f() != 0.0);
                let along = crate::api_common::vec3(rt, if walks { 0.0 } else { 0.125 }, 0.0, 0.25);
                call(
                    rt,
                    ENTITY,
                    "move",
                    "(Lnet/minecraft/world/entity/MoverType;Lnet/minecraft/world/phys/Vec3;)V",
                    &[e, V::NULL, along],
                );
                CARRIED.set(e);
            }
            _ => unreachable!(),
        }
        V::Void
    }

    fn runtime() -> Rt {
        let mut rt = Rt::new();
        rt.overrides = Some(overrides);
        rt.vcall = Some(vcall);
        for (class, id) in [
            (SAW, "test:saw"),
            (PLATE, "test:hot_plate"),
            (DYE, "test:dye"),
            (BELT, "test:belt"),
        ] {
            rt.supers
                .insert(class, "net/minecraft/world/level/block/Block");
            let b = rt.new_object(class);
            rt.registered.insert(("block".into(), id.into()), b);
        }
        rt
    }

    const PIG: i32 = 7;
    const STEVE: u128 = 0x5157_e7e5;
    const STEVE_ENTITY: i32 = 3;

    /// A pig at (0, 64, 0), and the acting player at (2, 64, 0).
    fn level() -> MemoryHost {
        let mut host = MemoryHost::default();
        host.entities.push(EntityView {
            id: PIG,
            kind: "minecraft:pig".into(),
            pos: [0.5, 64.2, 0.5],
            yaw: 0.0,
            health: Some((10.0, 10.0)),
            ..Default::default()
        });
        host.effects.insert(Some(PIG), Vec::new());
        host.effects.insert(None, Vec::new());
        host.player_id = Some(STEVE);
        host.place = Some(([2.5, 64.0, 0.5], 0.0, 0.0));
        host.players.push((STEVE, [2.5, 64.0, 0.5]));
        host.player_entity_ids.insert(STEVE, STEVE_ENTITY);
        host
    }

    fn effects(host: &MemoryHost, entity: Option<i32>) -> Vec<String> {
        host.effects
            .get(&entity)
            .map(|e| e.iter().map(|e: &EffectView| e.id.clone()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn a_mod_block_hurts_the_mob_in_it_by_its_own_entity_inside() {
        let mut rt = runtime();
        let mut host = level();
        assert!(entity_inside(
            &mut rt,
            &mut host,
            [0, 64, 0],
            "test:saw",
            PIG,
            "minecraft:pig"
        ));
        assert_eq!(
            host.blows
                .iter()
                .map(|b| (b.0, b.1, b.2.damage_type.as_str()))
                .collect::<Vec<_>>(),
            [(PIG, 3.0, "minecraft:cactus")]
        );
        assert_eq!(host.entity(PIG).and_then(|p| p.health), Some((7.0, 10.0)));
        // The acting player in it: hurt through the game, as a server's player.
        assert!(entity_inside(
            &mut rt,
            &mut host,
            [2, 64, 0],
            "test:saw",
            STEVE_ENTITY,
            "minecraft:player"
        ));
        let hurt: Vec<(u128, f32, &str)> = host
            .player_blows
            .iter()
            .map(|b| (b.0, b.1, b.2.damage_type.as_str()))
            .collect();
        assert_eq!(hurt, [(STEVE, 3.0, "minecraft:cactus")]);
        // Standing on it is not being in it: the saw has no `stepOn`.
        assert!(!step_on(
            &mut rt,
            &mut host,
            [0, 64, 0],
            "test:saw",
            PIG,
            "minecraft:pig"
        ));
        assert_eq!(host.blows.len(), 1);
    }

    #[test]
    fn a_mod_block_stepped_on_acts_on_the_mob_and_the_player_unless_they_step_carefully() {
        let mut rt = runtime();
        let mut host = level();
        // The acting player sneaks: that doesn't make the pig careful.
        host.sneaking = true;
        assert!(step_on(
            &mut rt,
            &mut host,
            [0, 63, 0],
            "test:hot_plate",
            PIG,
            "minecraft:pig"
        ));
        assert_eq!(effects(&host, Some(PIG)), ["minecraft:slowness"]);
        // The acting player, sneaking: careful.
        assert!(step_on(
            &mut rt,
            &mut host,
            [2, 63, 0],
            "test:hot_plate",
            STEVE_ENTITY,
            "minecraft:player"
        ));
        assert!(effects(&host, None).is_empty());
        host.sneaking = false;
        assert!(step_on(
            &mut rt,
            &mut host,
            [2, 63, 0],
            "test:hot_plate",
            STEVE_ENTITY,
            "minecraft:player"
        ));
        assert_eq!(effects(&host, None), ["minecraft:slowness"]);
    }

    #[test]
    fn an_item_in_a_block_is_its_hooks_unless_a_block_entity_ticks_there() {
        let mut rt = runtime();
        let mut host = level();
        let item = |id: u32, x: f64| ItemEntity {
            id,
            pos: [x, 64.1, 0.5],
            motion: [0.0; 3],
            on_ground: true,
            item: "minecraft:white_wool".into(),
            count: 2,
            pickup_delay: 0,
            age: 0,
        };
        host.item_entities.push(item(11, 4.5));
        host.item_entities.push(item(12, 6.5));
        for (id, x) in [(11, 4.5), (12, 6.5)] {
            host.entities.push(EntityView {
                id,
                kind: "minecraft:item".into(),
                pos: [x, 64.1, 0.5],
                yaw: 0.0,
                health: None,
                ..Default::default()
            });
        }
        // A block entity of the block at x 6: its tick tells it of items.
        rt.block_entities.insert([6, 64, 0], V::NULL);
        assert!(entity_inside(
            &mut rt,
            &mut host,
            [4, 64, 0],
            "test:dye",
            11,
            "minecraft:item"
        ));
        assert!(!entity_inside(
            &mut rt,
            &mut host,
            [6, 64, 0],
            "test:dye",
            12,
            "minecraft:item"
        ));
        let stacks: Vec<(u32, &str, u32)> = host
            .item_entities
            .iter()
            .map(|e| (e.id, e.item.as_str(), e.count))
            .collect();
        assert_eq!(
            stacks,
            [
                (11, "minecraft:red_wool", 1),
                (12, "minecraft:white_wool", 2)
            ]
        );
    }

    /// On a client, its own player in a mod's block is its `LocalPlayer`, the one object the
    /// client's runtime keeps for them (a belt keys its passengers by it), and a belt there carries
    /// them by their own move, held to its middle unless they walk. A server's player is moved by
    /// their client: the server's belt doesn't move them.
    #[test]
    fn a_clients_belt_carries_its_own_player() {
        crate::install();
        let mut rt = runtime();
        let mut client = MemoryHost {
            client: true,
            client_player_id: Some(57),
            place: Some(([0.25, 64.8125, 0.5], 0.0, 0.0)),
            ..MemoryHost::default()
        };
        let at = |host: &MemoryHost| host.place.map(|p| p.0);
        assert!(entity_inside(
            &mut rt,
            &mut client,
            [0, 64, 0],
            "test:belt",
            57,
            "minecraft:player"
        ));
        assert_eq!(at(&client), Some([0.375, 64.8125, 0.75]));
        let own = crate::network::local_player(&mut rt);
        assert!(CARRIED.get() == own, "the client's own player object");
        // Walking: not pulled to the middle.
        client.client_input = [0.0, 0.98];
        assert!(entity_inside(
            &mut rt,
            &mut client,
            [0, 64, 0],
            "test:belt",
            57,
            "minecraft:player"
        ));
        assert_eq!(at(&client), Some([0.375, 64.8125, 1.0]));
        assert!(CARRIED.get() == own);
        let mut rt = runtime();
        let mut server = level();
        assert!(entity_inside(
            &mut rt,
            &mut server,
            [2, 64, 0],
            "test:belt",
            STEVE_ENTITY,
            "minecraft:player"
        ));
        assert_eq!(at(&server), Some([2.5, 64.0, 0.5]));
    }

    #[test]
    fn a_block_without_its_own_contact_methods_does_nothing() {
        let mut rt = runtime();
        let mut host = level();
        assert!(!entity_inside(
            &mut rt,
            &mut host,
            [0, 64, 0],
            "test:hot_plate",
            PIG,
            "minecraft:pig"
        ));
        assert!(!step_on(
            &mut rt,
            &mut host,
            [0, 63, 0],
            "test:dye",
            PIG,
            "minecraft:pig"
        ));
        assert!(!entity_inside(
            &mut rt,
            &mut host,
            [0, 64, 0],
            "minecraft:stone",
            PIG,
            "minecraft:pig"
        ));
        assert!(host.blows.is_empty() && effects(&host, Some(PIG)).is_empty());
    }
}
