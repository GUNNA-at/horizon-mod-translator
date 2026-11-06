//! Native item data component helpers.
use super::Game;
use horizon_client::items::ItemStack;
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
        horizon_protocol::item::custom::entries()
            .map(|(_, name)| name)
            .collect()
    }

    /// Read a native component, including the item's default when not patched.
    pub(super) fn native_component<'a>(
        &'a self,
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
        Ok(self
            .items()
            .get(&stack.item)
            .and_then(|item| item.custom_components.get(id)))
    }

    /// Set a registered native component after checking the network NBT limits.
    pub(super) fn native_set_component(
        &self,
        stack: &mut ItemStack,
        id: &str,
        value: Tag,
    ) -> Result<(), NativeComponentError> {
        if horizon_protocol::item::custom::id(id).is_none() {
            return Err(NativeComponentError::UnknownType);
        }
        horizon_protocol::item::custom::validate(&value)
            .map_err(NativeComponentError::InvalidValue)?;
        let default = self
            .items()
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

    /// Remove a native component, including its item default.
    pub(super) fn native_remove_component(
        &self,
        stack: &mut ItemStack,
        id: &str,
    ) -> Result<(), NativeComponentError> {
        if horizon_protocol::item::custom::id(id).is_none() {
            return Err(NativeComponentError::UnknownType);
        }
        let default = self
            .items()
            .get(&stack.item)
            .is_some_and(|item| item.custom_components.contains_key(id));
        let removed = format!("!{id}");
        let data = stack.data_mut();
        data.set_other(id, None);
        data.set_other(&removed, default.then(|| Tag::Compound(Default::default())));
        stack.tidy();
        Ok(())
    }

    /// The vanilla components a translated mod's stacks carry beside the registered ones: the
    /// stack's enchantments and stored (book) enchantments, those it has, as vanilla's
    /// `components` map writes them, and its custom data (`minecraft:custom_data`).
    /// API @API:vanilla_components@.
    pub(super) fn native_vanilla_components(&self, stack: &ItemStack) -> Vec<(&'static str, Tag)> {
        let mut out: Vec<_> = [(VANILLA_COMPONENTS[0], false), (VANILLA_COMPONENTS[1], true)]
            .into_iter()
            .filter_map(|(id, stored)| Some((id, stack.enchantments_tag(stored)?)))
            .collect();
        let custom = stack.data().and_then(|d| d.other(VANILLA_COMPONENTS[2]));
        out.extend(custom.map(|tag| (VANILLA_COMPONENTS[2], tag.clone())));
        out
    }

    /// Sets (None: clears) a component [`Self::native_vanilla_components`] names, from vanilla's
    /// form (1.21.1's too). Custom data is a compound within the network NBT limits; an empty one
    /// clears it, as vanilla's `CustomData.set` does. API @API:vanilla_components@.
    pub(super) fn native_set_vanilla_component(
        &self,
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
            Some(i) => stack.set_enchantments_tag(i == 1, value),
            None => return Err(NativeComponentError::UnknownType),
        }
        Ok(())
    }
}

/// The vanilla components [`Game::native_vanilla_components`] carries.
const VANILLA_COMPONENTS: [&str; 3] = [
    "minecraft:enchantments",
    "minecraft:stored_enchantments",
    "minecraft:custom_data",
];
