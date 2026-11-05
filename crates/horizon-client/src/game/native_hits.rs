//! A hit's world for native server mods (docs/native-mods.md, "Hits"). The
//! hooks of a hit (`native_living_incoming_damage`, `native_living_knockback`,
//! `native_living_damage_pre`, `native_living_damage_post`) run inside the
//! game's damage code, with no game at hand to read or change. What they read
//! of the player whose blow the hit is was taken as the blow started
//! ([`native_hit_player`]); what they change of the world waits for the hit
//! to end ([`native_after_hit`]) and is made in the level the hit was dealt
//! in, once the player's action or the level's tick that dealt it is over.
use super::{Game, native_effects::NativeEffect, native_server::NativeServerPlayer};
use horizon_client::{first_person::Hand, items::ItemStack};
use horizon_world::nbt::Tag;
use std::cell::RefCell;

/// A stack in the striking player's hand, with what a mod reads of it.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct NativeHitHand {
    pub(crate) stack: ItemStack,
    /// The damage at which it breaks, 0 for none (`native_item_max_damage`).
    pub(crate) max_damage: u32,
    /// Its native components (`native_component`), then the vanilla ones
    /// translated mods read (`native_vanilla_components`: its enchantments).
    pub(crate) components: Vec<(&'static str, Tag)>,
}

/// The player whose blow a hit is, as the blow started.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct NativeHitPlayer {
    /// Their entity id, as the hit's source names them.
    pub(crate) entity: i32,
    pub(crate) player: NativeServerPlayer,
    pub(crate) sneaking: bool,
    /// The main hand's stack, then the off hand's; None for an empty hand.
    pub(crate) hands: [Option<NativeHitHand>; 2],
    /// Their active effects (`native_effects`).
    pub(crate) effects: Vec<NativeEffect>,
}

/// A change a hit's hook made, for the game once the hit is over.
type AfterHit = Box<dyn for<'a, 'g> FnOnce(&'a mut Game<'g>)>;

thread_local! {
    /// The player whose blow the running hit is ([`native_hit_player`]).
    static HIT_PLAYER: RefCell<Option<NativeHitPlayer>> = const { RefCell::new(None) };
    /// What hits' hooks changed, waiting for their hits to end ([`native_after_hit`]).
    static AFTER_HIT: RefCell<Vec<AfterHit>> = const { RefCell::new(Vec::new()) };
}

/// The player whose blow the running hit is, as the blow started, with what
/// they held then: None for a hit no player's blow dealt (a mob's, a fall),
/// and outside a hit. For a hit's hooks, which have no game to ask.
/// API @API:hit_world@.
#[allow(dead_code)]
pub(crate) fn native_hit_player() -> Option<NativeHitPlayer> {
    HIT_PLAYER.with(|p| p.borrow().clone())
}

/// Has `change` made to the game once the hit in progress is over, in the
/// level it was dealt in: after the player's action or the level's tick that
/// dealt it, changes in the order they came. For a hit's hooks, which have no
/// game to change (a sound, a message, another entity hurt), and an effect's
/// (`native_apply_effect_tick` runs inside the entity's tick). A change that
/// leaves more runs them in turn. API @API:hit_world@.
#[allow(dead_code)]
pub(crate) fn native_after_hit(change: impl for<'a, 'g> FnOnce(&'a mut Game<'g>) + 'static) {
    AFTER_HIT.with(|q| q.borrow_mut().push(Box::new(change)));
}

/// The rounds [`Game::run_after_hits`] runs: the changes of one round may
/// deal hits whose hooks leave more.
const ROUNDS: usize = 8;

impl Game<'_> {
    /// Runs `blow`, a blow of the acting player, with [`native_hit_player`]
    /// telling of them.
    pub(super) fn with_hit_player<R>(&mut self, blow: impl FnOnce(&mut Self) -> R) -> R {
        let player = self.hit_player_now();
        let before = HIT_PLAYER.with(|p| p.replace(player));
        let result = blow(self);
        HIT_PLAYER.with(|p| *p.borrow_mut() = before);
        result
    }
    /// The acting player as a hit's hooks read them; None when nobody acts.
    fn hit_player_now(&self) -> Option<NativeHitPlayer> {
        let player = self.native_acting_player()?;
        let (entity, sneaking) = self
            .player
            .as_ref()
            .map_or((0, false), |p| (p.entity_id, p.shift_key_down));
        let hand = |hand| self.native_hand_stack(hand).map(|stack| self.hit_hand(stack));
        Some(NativeHitPlayer {
            entity,
            player,
            sneaking,
            hands: [hand(Hand::Main), hand(Hand::Off)],
            effects: self.native_effects(None).unwrap_or_default(),
        })
    }
    fn hit_hand(&self, stack: &ItemStack) -> NativeHitHand {
        let mut components: Vec<(&'static str, Tag)> = self
            .native_registered_components()
            .into_iter()
            .filter_map(|name| Some((name, self.native_component(stack, name).ok()??.clone())))
            .collect();
        components.extend(self.native_vanilla_components(stack));
        NativeHitHand {
            stack: stack.clone(),
            max_damage: self.native_item_max_damage(&stack.item),
            components,
        }
    }
    /// Makes the changes hits' hooks left ([`native_after_hit`]), in order,
    /// in the level in place: the game calls it where a player's action or a
    /// level's tick ends, and before another level takes this one's place.
    pub(super) fn run_after_hits(&mut self) {
        for _ in 0..ROUNDS {
            let changes = AFTER_HIT.with(|q| std::mem::take(&mut *q.borrow_mut()));
            if changes.is_empty() {
                return;
            }
            for change in changes {
                change(self);
            }
        }
        let left = AFTER_HIT.with(|q| std::mem::take(&mut *q.borrow_mut())).len();
        if left > 0 {
            log::warn!("native mods: {left} changes of hits not made: each round of them left more");
        }
    }
}

