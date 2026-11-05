//! Attributes for native server mods (docs/native-mods.md, "Attributes"): a living entity's
//! attribute as a mod reads it (its value, its base, its modifiers) and the changes a mod makes to
//! it. The player in place has the game's whole attribute map (`PlayerMotion::attributes`): what a
//! mod changes there moves, hurts and reaches their client as the game's own changes do. A mob keeps
//! the attributes its behaviour reads as plain values (`mobs::Attrs`): their base, with no
//! modifiers; its other attributes are the map the `/attribute` command keeps for it.
use super::{Game, commands::Target};
use horizon_world::{
    attribute_map::ValueKind,
    attributes::{AttributeError, Instance, Modifier, Observer, Operation},
};

/// An attribute of a living entity, as a mod reads it.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeAttribute {
    /// Its value: the base with the modifiers, kept in the attribute's range.
    pub(crate) value: f64,
    pub(crate) base: f64,
    /// Its modifiers, in the order they came.
    pub(crate) modifiers: Vec<NativeAttributeModifier>,
}

/// One of an attribute's modifiers (vanilla's `AttributeModifier`).
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeAttributeModifier {
    /// Its id (`mymod:speed_boost`): an attribute has one modifier of each.
    pub(crate) id: String,
    pub(crate) amount: f64,
    /// Vanilla's `AttributeModifier.Operation`, in its order: 0 adds the amount to the base, 1 adds
    /// the base times the amount, 2 multiplies the total by 1 plus the amount.
    pub(crate) operation: u8,
    /// Whether it stays with the entity's saved data (`addPermanentModifier`); else it's gone when
    /// the entity is next loaded.
    pub(crate) permanent: bool,
}

/// A change a mod makes to an attribute.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NativeAttributeChange {
    /// Its base (`setBaseValue`).
    Base(f64),
    /// A modifier added (`addTransientModifier`, `addPermanentModifier`). With `replace`, one of its
    /// id already there is replaced (`addOrUpdateTransientModifier`, `addOrReplacePermanentModifier`);
    /// without, the change fails.
    Add { modifier: NativeAttributeModifier, replace: bool },
    /// The modifier of this id taken off (`removeModifier`); fails when there is none.
    Remove(String),
    /// All its modifiers taken off (`removeModifiers`).
    RemoveAll,
}

/// The attributes a mob keeps as plain values, by their names without `minecraft:`.
const PLAIN: [&str; 8] = [
    "max_health",
    "movement_speed",
    "attack_damage",
    "attack_knockback",
    "follow_range",
    "knockback_resistance",
    "spawn_reinforcements",
    "jump_strength",
];

/// Whose attribute a mod asks for.
enum Holder {
    Player,
    Mob(u32),
    Stand(u32),
}

impl Game<'_> {
    /// Attribute `attribute` (`minecraft:movement_speed`; a name without its namespace is
    /// vanilla's) of the living entity `entity` in the level in place, None for the player in
    /// place: the acting player on a server, the client's own on a client. None when it isn't a
    /// living entity of the level or its type doesn't have the attribute. API @API:attributes@.
    #[allow(dead_code)]
    pub(crate) fn native_attribute(&mut self, entity: Option<i32>, attribute: &str) -> Option<NativeAttribute> {
        let name = attribute_name(attribute);
        match self.attribute_holder(entity)? {
            Holder::Player => {
                let attributes = &mut self.player.as_mut()?.attributes;
                let key = attributes.key(&name)?;
                if !attributes.has_attribute(Some(key.clone())).ok()? {
                    return None;
                }
                let value = attributes.value(Some(key.clone()), ValueKind::Computed).ok()?;
                let base = attributes.value(Some(key.clone()), ValueKind::Base).ok()?;
                let instance = attributes.lookup(Some(&key)).and_then(|h| attributes.instance(&h).ok());
                let modifiers = instance.map(modifiers_of).unwrap_or_default();
                Some(NativeAttribute { value, base, modifiers })
            }
            Holder::Mob(id) if PLAIN.contains(&short(&name)) => {
                let kind = self.target_type(Target::Mob(id));
                let attribute = self.motion.attributes.id(&name)?;
                self.motion.attributes.default_base(&kind, attribute)?;
                let m = self.mobs.iter().find(|m| m.id == id)?;
                let value = plain(&m.attrs, short(&name))?;
                Some(NativeAttribute { value, base: value, modifiers: Vec::new() })
            }
            Holder::Mob(id) => self.generic_attribute_of(Target::Mob(id), &name),
            Holder::Stand(id) => self.generic_attribute_of(Target::Stand(id), &name),
        }
    }

    /// Makes `change` to attribute `attribute` of the living entity `entity` (as
    /// [`Game::native_attribute`] names them). Whether it was made: not when the entity doesn't
    /// have the attribute, a modifier of the id is there already (an `Add` without `replace`) or
    /// isn't (a `Remove`), or the modifier's id isn't a resource location; nor a modifier on a
    /// mob's plain attribute, whose base alone changes. API @API:attributes@.
    #[allow(dead_code)]
    pub(crate) fn native_change_attribute(&mut self, entity: Option<i32>, attribute: &str, change: &NativeAttributeChange) -> bool {
        let name = attribute_name(attribute);
        let modifier = match change {
            NativeAttributeChange::Add { modifier, .. } => {
                let operation = match modifier.operation {
                    1 => Operation::AddMultipliedBase,
                    2 => Operation::AddMultipliedTotal,
                    _ => Operation::AddValue,
                };
                match Modifier::new(&modifier.id, modifier.amount, operation) {
                    Ok(m) => Some(m),
                    Err(_) => return false,
                }
            }
            _ => None,
        };
        let Some(holder) = self.attribute_holder(entity) else {
            return false;
        };
        match holder {
            Holder::Player => {
                let Some(player) = self.player.as_mut() else {
                    return false;
                };
                let attributes = &mut player.attributes;
                let Some(handle) = attributes.key(&name).and_then(|key| attributes.get_instance(Some(key)).ok().flatten()) else {
                    return false;
                };
                attributes.edit(&handle, |instance, dirty| apply(instance, change, modifier, dirty)).unwrap_or(false)
            }
            Holder::Mob(id) if PLAIN.contains(&short(&name)) => {
                let NativeAttributeChange::Base(value) = *change else {
                    return false;
                };
                let kind = self.target_type(Target::Mob(id));
                if !self.motion.attributes.id(&name).is_some_and(|a| self.motion.attributes.default_base(&kind, a).is_some()) {
                    return false;
                }
                let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) else {
                    return false;
                };
                set_plain(&mut m.attrs, short(&name), value);
                // A lower maximum health takes what is over it (`LivingEntity.onAttributeUpdated`).
                if short(&name) == "max_health" && m.health > m.attrs.max_health {
                    m.health = m.attrs.max_health;
                }
                true
            }
            Holder::Mob(id) => self.change_generic_attribute(Target::Mob(id), &name, change, modifier),
            Holder::Stand(id) => self.change_generic_attribute(Target::Stand(id), &name, change, modifier),
        }
    }

    /// Who `entity` is, of those with attributes: the player in place (None, or their entity id),
    /// a mob or an armour stand of the level in place.
    fn attribute_holder(&self, entity: Option<i32>) -> Option<Holder> {
        // On a server, a player is in place only while they act.
        let in_place = self.player.as_ref().filter(|_| self.lan.is_none() || self.lan_acting.is_some());
        match entity {
            None => in_place.map(|_| Holder::Player),
            Some(id) if in_place.is_some_and(|p| p.entity_id == id) => Some(Holder::Player),
            Some(id) => match self.level_entity(id)? {
                Target::Mob(id) => Some(Holder::Mob(id)),
                Target::Stand(id) => Some(Holder::Stand(id)),
                _ => None,
            },
        }
    }

    fn generic_attribute_of(&mut self, target: Target, name: &str) -> Option<NativeAttribute> {
        let attribute = self.motion.attributes.id(name)?;
        let map = self.generic_attributes(target)?;
        if !map.has(attribute) {
            return None;
        }
        let value = map.value(attribute).ok()?;
        let base = map.base_value(attribute).ok()?;
        let modifiers = map.instance(attribute).map(modifiers_of).unwrap_or_default();
        Some(NativeAttribute { value, base, modifiers })
    }

    fn change_generic_attribute(&mut self, target: Target, name: &str, change: &NativeAttributeChange, modifier: Option<Modifier>) -> bool {
        let Some(attribute) = self.motion.attributes.id(name) else {
            return false;
        };
        let Some(map) = self.generic_attributes(target) else {
            return false;
        };
        if !map.has(attribute) {
            return false;
        }
        map.edit(attribute, |instance, dirty| apply(instance, change, modifier, dirty).unwrap_or(false)).unwrap_or(false)
    }
}

/// A vanilla attribute's id as the game names it: `minecraft:generic.max_health`, as 1.21.1 named
/// it, and `max_health` are `minecraft:max_health`.
fn attribute_name(attribute: &str) -> String {
    let (namespace, path) = attribute.split_once(':').unwrap_or(("minecraft", attribute));
    let path = match namespace {
        "minecraft" => ["generic.", "player.", "zombie."].iter().find_map(|p| path.strip_prefix(p)).unwrap_or(path),
        _ => path,
    };
    format!("{namespace}:{path}")
}

fn short(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

fn plain(attrs: &horizon_client::mobs::Attrs, name: &str) -> Option<f64> {
    Some(match name {
        "max_health" => f64::from(attrs.max_health),
        "movement_speed" => attrs.speed,
        "attack_damage" => f64::from(attrs.attack_damage),
        "attack_knockback" => attrs.attack_knockback,
        "follow_range" => attrs.follow_range,
        "knockback_resistance" => attrs.knockback_resistance,
        "spawn_reinforcements" => attrs.spawn_reinforcements,
        "jump_strength" => attrs.jump_strength,
        _ => return None,
    })
}

fn set_plain(attrs: &mut horizon_client::mobs::Attrs, name: &str, value: f64) {
    match name {
        "max_health" => attrs.max_health = value as f32,
        "movement_speed" => attrs.speed = value,
        "attack_damage" => attrs.attack_damage = value as f32,
        "attack_knockback" => attrs.attack_knockback = value,
        "follow_range" => attrs.follow_range = value,
        "knockback_resistance" => attrs.knockback_resistance = value,
        "spawn_reinforcements" => attrs.spawn_reinforcements = value,
        "jump_strength" => attrs.jump_strength = value,
        _ => {}
    }
}

fn modifiers_of(instance: &Instance) -> Vec<NativeAttributeModifier> {
    instance
        .modifiers()
        .iter()
        .map(|m| NativeAttributeModifier {
            id: m.id().to_owned(),
            amount: m.amount(),
            operation: m.operation() as u8,
            permanent: instance.permanent_modifiers().iter().any(|p| p.id() == m.id()),
        })
        .collect()
}

/// Makes `change` to `instance` (`modifier` the one an `Add` adds); whether it was made.
fn apply<O: Observer>(instance: &mut Instance, change: &NativeAttributeChange, modifier: Option<Modifier>, dirty: &mut O) -> Result<bool, AttributeError<O::Error>> {
    match (change, modifier) {
        (NativeAttributeChange::Base(value), _) => instance.set_base_value(*value, dirty).map(|()| true),
        (NativeAttributeChange::Add { modifier: m, replace }, Some(modifier)) => {
            if !*replace && instance.modifier(modifier.id()).is_some() {
                return Ok(false);
            }
            match (m.permanent, *replace) {
                (false, false) => instance.add_transient(modifier, dirty),
                (false, true) => instance.add_or_update_transient(modifier, dirty),
                (true, false) => instance.add_permanent(modifier, dirty),
                (true, true) => instance.add_or_replace_permanent(modifier, dirty),
            }
            .map(|()| true)
        }
        (NativeAttributeChange::Add { .. }, None) => Ok(false),
        (NativeAttributeChange::Remove(id), _) => instance.remove(id, dirty),
        (NativeAttributeChange::RemoveAll, _) => instance.remove_all(dirty).map(|()| true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanillas_attribute_ids_of_1_21_1_name_the_games() {
        assert_eq!(attribute_name("minecraft:generic.max_health"), "minecraft:max_health");
        assert_eq!(attribute_name("minecraft:player.block_break_speed"), "minecraft:block_break_speed");
        assert_eq!(attribute_name("minecraft:zombie.spawn_reinforcements"), "minecraft:spawn_reinforcements");
        assert_eq!(attribute_name("movement_speed"), "minecraft:movement_speed");
        assert_eq!(attribute_name("mymod:generic.mana"), "mymod:generic.mana");
    }
}
