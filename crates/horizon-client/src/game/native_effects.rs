//! Mob effects for native server mods (docs/native-mods.md, "Effects"): the effects a living entity
//! has, as a mod reads them, and the effects a mod gives and takes off. The player in place has the
//! game's own (`PlayerEffects`): what a mod gives them ticks, changes their attributes and reaches
//! their client as the `/effect` command's does. A mob keeps its own list (`Mob.extra.effects`),
//! which its behaviour and swirls read; a mob that can't have an effect (a spider's poison) refuses
//! it, as vanilla's `canBeAffected`. A content mod's effect given either way runs its hooks
//! (`native_on_effect_added`, `native_mob_effect_added`).
use super::{
    Game,
    native_attributes::Living,
    potions::{mob_can_be_affected, mob_effect_id},
};
use horizon_client::{
    brewing::{self, namespaced},
    effects::EffectInstance,
};

/// One of a living entity's active effects, as a mod reads it and gives it (vanilla's
/// `MobEffectInstance`).
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeEffect {
    /// Its effect: `minecraft:speed`, a content mod's `<namespace>:<path>`.
    pub(crate) id: String,
    /// The ticks left; -1 for one that doesn't end.
    pub(crate) duration: i32,
    pub(crate) amplifier: i32,
    /// A beacon's kind: faint swirls.
    pub(crate) ambient: bool,
    /// Whether it shows its swirls.
    pub(crate) visible: bool,
    /// Whether its icon shows. The game shows an effect's icon when it shows its swirls, so this
    /// reads as `visible`, and one given is as visible.
    pub(crate) show_icon: bool,
}

impl Game<'_> {
    /// The active effects of the living entity `entity` in the level in place, None for the
    /// player in place (as `native_attribute` names them), with their vanilla ids
    /// (`minecraft:speed`). None when it isn't a living entity of the level or has no effects to
    /// keep (an armour stand). API @API:effects@.
    #[allow(dead_code)]
    pub(crate) fn native_effects(&self, entity: Option<i32>) -> Option<Vec<NativeEffect>> {
        match self.living(entity)? {
            Living::Player => Some(
                self.potions
                    .effects
                    .list
                    .iter()
                    .map(|e| NativeEffect {
                        id: namespaced(e.id).to_owned(),
                        duration: e.duration,
                        amplifier: e.amplifier,
                        ambient: e.ambient,
                        visible: e.visible,
                        show_icon: e.visible,
                    })
                    .collect(),
            ),
            Living::Mob(id) => {
                let m = self.mobs.iter().find(|m| m.id == id)?;
                Some(
                    m.extra
                        .effects
                        .iter()
                        .map(|&(effect, duration, amplifier)| {
                            let visible = !m.extra.hidden_effects.contains(&effect);
                            NativeEffect {
                                id: effect.to_owned(),
                                duration,
                                amplifier,
                                ambient: m.extra.ambient_effects.contains(&effect),
                                visible,
                                show_icon: visible,
                            }
                        })
                        .collect(),
                )
            }
            Living::Stand(_) => None,
        }
    }

    /// Gives the living entity `entity` (as [`Game::native_effects`] names it) the effect
    /// `effect` (`LivingEntity.addEffect`: one of the same effect there already is updated, a
    /// stronger one taking over, one as strong lasting as long as the longer; with `force`,
    /// `forceAddEffect`, replaced). Whether it was added or changed what was there: not for an
    /// effect the game doesn't know, nor one the mob can't have. API @API:effects@.
    #[allow(dead_code)]
    pub(crate) fn native_add_effect(&mut self, entity: Option<i32>, effect: &NativeEffect, force: bool) -> bool {
        let Some(def) = brewing::effect(&effect.id) else {
            return false;
        };
        match self.living(entity) {
            Some(Living::Player) => {
                let mut instance = EffectInstance::new(def.id, effect.duration, effect.amplifier);
                instance.ambient = effect.ambient;
                instance.visible = effect.visible;
                if force {
                    self.potions.effects.list.retain(|e| e.id != def.id);
                }
                self.add_player_effect(instance)
            }
            Some(Living::Mob(id)) => {
                let Some(name) = mob_effect_id(def.id) else {
                    return false;
                };
                let tags = &self.res.stations().enchantments.entity_tags;
                let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) else {
                    return false;
                };
                if !mob_can_be_affected(tags, &m.kind.id, name) {
                    return false;
                }
                let flags = |m: &horizon_client::mobs::Mob| {
                    let e = m.extra.effects.iter().find(|e| e.0 == name).copied();
                    (e, m.extra.ambient_effects.contains(&name), m.extra.hidden_effects.contains(&name))
                };
                let before = flags(&*m);
                if force {
                    m.extra.effects.retain(|e| e.0 != name);
                }
                m.add_effect_with(name, effect.duration, effect.amplifier, effect.ambient, effect.visible);
                let changed = flags(&*m) != before;
                self.mod_effect_added(name, Some(id as i32), effect.amplifier, before.0.is_none());
                changed
            }
            Some(Living::Stand(_)) | None => false,
        }
    }

    /// Takes the effect `effect` (`minecraft:speed`) off the living entity `entity`
    /// (`LivingEntity.removeEffect`): whether it had it. API @API:effects@.
    #[allow(dead_code)]
    pub(crate) fn native_remove_effect(&mut self, entity: Option<i32>, effect: &str) -> bool {
        let Some(def) = brewing::effect(effect) else {
            return false;
        };
        match self.living(entity) {
            Some(Living::Player) => {
                let before = self.potions.effects.list.len();
                self.potions.effects.list.retain(|e| e.id != def.id);
                self.apply_effect_status();
                self.potions.effects.list.len() != before
            }
            Some(Living::Mob(id)) => {
                let name = namespaced(def.id);
                let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) else {
                    return false;
                };
                let before = m.extra.effects.len();
                m.extra.effects.retain(|e| e.0 != name);
                m.extra.hidden_effects.retain(|h| *h != name);
                m.extra.ambient_effects.retain(|h| *h != name);
                m.extra.effects.len() != before
            }
            Some(Living::Stand(_)) | None => false,
        }
    }
}
