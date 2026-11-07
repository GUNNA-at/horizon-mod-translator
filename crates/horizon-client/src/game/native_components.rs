//! Native item data component helpers.
use super::{Game, GameLevel};
use horizon_client::items::{ItemStack, Items};
use horizon_world::nbt::Tag;

/// Failure to access a native item component.
#[derive(Debug)]
pub(crate) enum NativeComponentError {
    UnknownType,
    InvalidValue(horizon_protocol::Error),
}
impl std::fmt::Display for NativeComponentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownType => f.write_str("unregistered component type"),
            Self::InvalidValue(error) => write!(f, "invalid component value: {error}"),
        }
    }
}
impl std::error::Error for NativeComponentError {}

// Native mods use these helpers.
#[allow(dead_code)]
impl Game<'_> {
    /// The native component types registered at start (every mod's
    /// `native_component_types`), by name. API @API:registered_components@.
    pub(super) fn native_registered_components(&self) -> Vec<&'static str> {
        registered()
    }

    /// Read a native component, including the item's default when not patched.
    pub(super) fn native_component<'a>(
        &'a self,
        stack: &'a ItemStack,
        id: &str,
    ) -> Result<Option<&'a Tag>, NativeComponentError> {
        component(self.items(), stack, id)
    }

    /// Set a registered native component after checking the network NBT limits.
    pub(super) fn native_set_component(
        &self,
        stack: &mut ItemStack,
        id: &str,
        value: Tag,
    ) -> Result<(), NativeComponentError> {
        set_component(self.items(), stack, id, value)
    }

    /// Remove a native component, including its item default.
    pub(super) fn native_remove_component(
        &self,
        stack: &mut ItemStack,
        id: &str,
    ) -> Result<(), NativeComponentError> {
        remove_component(self.items(), stack, id)
    }

    /// The vanilla components a translated mod's stacks carry beside the registered ones: the
    /// stack's enchantments and stored (book) enchantments, those it has, as vanilla's
    /// `components` map writes them, its custom data (`minecraft:custom_data`) and its potion
    /// contents (`minecraft:potion_contents`). API @API:vanilla_components@.
    pub(super) fn native_vanilla_components(&self, stack: &ItemStack) -> Vec<(&'static str, Tag)> {
        vanilla_components(stack)
    }

    /// Sets (None: clears) a component [`Self::native_vanilla_components`] names, from vanilla's
    /// form (1.21.1's too). Custom data is a compound within the network NBT limits; an empty one
    /// clears it, as vanilla's `CustomData.set` does. Potion contents are a potion's id or
    /// vanilla's compound, read as the game's own stacks read them. API @API:vanilla_components@.
    pub(super) fn native_set_vanilla_component(
        &self,
        stack: &mut ItemStack,
        id: &str,
        value: Option<&Tag>,
    ) -> Result<(), NativeComponentError> {
        set_vanilla_component(stack, id, value)
    }
}

/// The component helpers on a level's hook (a mod block's tick popping what it held), which the
/// stacks of its drops cross with, as on the game: the same answers from the level's items.
/// API @API:level_item_entities@.
#[allow(dead_code)]
impl GameLevel<'_> {
    /// [`Game::native_registered_components`].
    pub(super) fn native_registered_components(&self) -> Vec<&'static str> {
        registered()
    }
    /// [`Game::native_component`].
    pub(super) fn native_component<'a>(
        &'a self,
        stack: &'a ItemStack,
        id: &str,
    ) -> Result<Option<&'a Tag>, NativeComponentError> {
        component(self.items, stack, id)
    }
    /// [`Game::native_set_component`].
    pub(super) fn native_set_component(
        &self,
        stack: &mut ItemStack,
        id: &str,
        value: Tag,
    ) -> Result<(), NativeComponentError> {
        set_component(self.items, stack, id, value)
    }
    /// [`Game::native_remove_component`].
    pub(super) fn native_remove_component(
        &self,
        stack: &mut ItemStack,
        id: &str,
    ) -> Result<(), NativeComponentError> {
        remove_component(self.items, stack, id)
    }
    /// [`Game::native_vanilla_components`].
    pub(super) fn native_vanilla_components(&self, stack: &ItemStack) -> Vec<(&'static str, Tag)> {
        vanilla_components(stack)
    }
    /// [`Game::native_set_vanilla_component`].
    pub(super) fn native_set_vanilla_component(
        &self,
        stack: &mut ItemStack,
        id: &str,
        value: Option<&Tag>,
    ) -> Result<(), NativeComponentError> {
        set_vanilla_component(stack, id, value)
    }
    /// The damage at which `item` breaks (its `max_damage`), 0 for an item
    /// without durability or one the level doesn't know.
    pub(super) fn native_item_max_damage(&self, item: &str) -> u32 {
        self.items.get(item).map_or(0, |i| i.max_damage)
    }
}

/// The native component types registered at start, by name.
fn registered() -> Vec<&'static str> {
    horizon_protocol::item::custom::entries()
        .map(|(_, name)| name)
        .collect()
}

/// `stack`'s registered component `id`, its item's default when the stack doesn't patch it.
fn component<'a>(
    items: &'a Items,
    stack: &'a ItemStack,
    id: &str,
) -> Result<Option<&'a Tag>, NativeComponentError> {
    if horizon_protocol::item::custom::id(id).is_none() {
        return Err(NativeComponentError::UnknownType);
    }
    if let Some(data) = stack.data() {
        if data.other(&format!("!{id}")).is_some() {
            return Ok(None);
        }
        if let Some(value) = data.other(id) {
            return Ok(Some(value));
        }
    }
    Ok(items
        .get(&stack.item)
        .and_then(|item| item.custom_components.get(id)))
}

/// Sets registered component `id` on `stack` within the network NBT limits; its item's default
/// leaves no patch.
fn set_component(
    items: &Items,
    stack: &mut ItemStack,
    id: &str,
    value: Tag,
) -> Result<(), NativeComponentError> {
    if horizon_protocol::item::custom::id(id).is_none() {
        return Err(NativeComponentError::UnknownType);
    }
    horizon_protocol::item::custom::validate(&value).map_err(NativeComponentError::InvalidValue)?;
    let default = items
        .get(&stack.item)
        .and_then(|item| item.custom_components.get(id));
    let value = (default != Some(&value)).then_some(value);
    let removed = format!("!{id}");
    let data = stack.data_mut();
    data.set_other(&removed, None);
    data.set_other(id, value);
    stack.tidy();
    Ok(())
}

/// Removes registered component `id` from `stack`, its item's default with it.
fn remove_component(
    items: &Items,
    stack: &mut ItemStack,
    id: &str,
) -> Result<(), NativeComponentError> {
    if horizon_protocol::item::custom::id(id).is_none() {
        return Err(NativeComponentError::UnknownType);
    }
    let default = items
        .get(&stack.item)
        .is_some_and(|item| item.custom_components.contains_key(id));
    let removed = format!("!{id}");
    let data = stack.data_mut();
    data.set_other(id, None);
    data.set_other(&removed, default.then(|| Tag::Compound(Default::default())));
    stack.tidy();
    Ok(())
}

/// The vanilla components of [`Game::native_vanilla_components`] that `stack` has.
fn vanilla_components(stack: &ItemStack) -> Vec<(&'static str, Tag)> {
    let mut out: Vec<_> = [
        (VANILLA_COMPONENTS[0], false),
        (VANILLA_COMPONENTS[1], true),
    ]
    .into_iter()
    .filter_map(|(id, stored)| Some((id, stack.enchantments_tag(stored)?)))
    .collect();
    let custom = stack.data().and_then(|d| d.other(VANILLA_COMPONENTS[2]));
    out.extend(custom.map(|tag| (VANILLA_COMPONENTS[2], tag.clone())));
    let potion = stack
        .data()
        .filter(|d| d.potion.is_some() || !d.potion_extra.is_empty())
        .and_then(|_| horizon_client::item_data::encode_components(stack))
        .and_then(|all| all.get(VANILLA_COMPONENTS[3]).cloned());
    out.extend(potion.map(|tag| (VANILLA_COMPONENTS[3], tag)));
    out
}

/// Sets (None: clears) vanilla component `id` of `stack`, as
/// [`Game::native_set_vanilla_component`].
fn set_vanilla_component(
    stack: &mut ItemStack,
    id: &str,
    value: Option<&Tag>,
) -> Result<(), NativeComponentError> {
    match VANILLA_COMPONENTS.iter().position(|c| *c == id) {
        Some(2) => {
            let value = match value {
                Some(Tag::Compound(c)) if c.is_empty() => None,
                Some(tag @ Tag::Compound(_)) => {
                    horizon_protocol::item::custom::validate(tag)
                        .map_err(NativeComponentError::InvalidValue)?;
                    Some(tag.clone())
                }
                Some(_) => {
                    return Err(NativeComponentError::InvalidValue(
                        horizon_protocol::Error::new("custom data must be a compound"),
                    ));
                }
                None => None,
            };
            if value.is_some() || stack.data().is_some() {
                stack.data_mut().set_other(id, value);
                stack.tidy();
            }
        }
        Some(3) => {
            if let Some(tag) = value {
                horizon_protocol::item::custom::validate(tag)
                    .map_err(NativeComponentError::InvalidValue)?;
            }
            let read = value.and_then(|tag| {
                let all = Tag::compound([(id, tag.clone())]);
                horizon_client::item_data::decode_components(Some(&all)).1
            });
            let (potion, extra) = read.map_or((None, Vec::new()), |d| {
                let d = *d;
                (d.potion, d.potion_extra)
            });
            if potion.is_some() || !extra.is_empty() || stack.data().is_some() {
                let data = stack.data_mut();
                data.potion = potion;
                data.potion_extra = extra;
                stack.tidy();
            }
        }
        Some(i) => stack.set_enchantments_tag(i == 1, value),
        None => return Err(NativeComponentError::UnknownType),
    }
    Ok(())
}

/// The vanilla components [`Game::native_vanilla_components`] carries.
const VANILLA_COMPONENTS: [&str; 4] = [
    "minecraft:enchantments",
    "minecraft:stored_enchantments",
    "minecraft:custom_data",
    "minecraft:potion_contents",
];
