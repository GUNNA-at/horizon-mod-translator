//! Block entities' saved data as vanilla 1.21.1 writes and reads it, with NeoForge's additions,
//! and the rest of what the base class answers itself:
//!
//! - Saving: `saveCustomOnly` is a new compound the block entity's own `saveAdditional` fills (the
//!   base class's puts NeoForge's `getPersistentData()` in it, as `NeoForgeData`);
//!   `saveWithoutMetadata` adds what it kept of the stack it was placed from, under `components`;
//!   `saveWithId` its type's id; `saveWithFullMetadata` that and its position (`x`, `y`, `z`), as
//!   Create keeps each block entity a contraption or schematic holds; `saveCustomAndMetadata` its
//!   own data, id and position. A block entity whose type nothing registered can't save its id:
//!   vanilla's `RuntimeException`.
//! - Loading: `loadCustomOnly` is its own `loadAdditional` (the base class's takes `NeoForgeData`
//!   back); `loadWithComponents` also takes back what it kept, none when `components` is absent;
//!   `loadStatic(pos, state, tag)` makes a block entity of the type `id` names and loads it, null
//!   when the type is unknown or making or loading it throws.
//! - Items: `saveToItem(stack)` puts its own data on the stack as `block_entity_data`, with its
//!   type's id (none when it saved nothing), and its components. A block item placed with such
//!   data loads it into the new block entity on a server (`BlockItem.updateCustomBlockEntityTag`):
//!   merged into what the block entity saves, and loaded when that changes it
//!   (`CustomData.loadInto`, which loads the old data back when loading throws). Data only
//!   operators may set (`onlyOpCanSetNbt`) is taken only from a player who may use game master
//!   blocks (`canUseGameMasterBlocks`: in creative, at operator level 2 or more).
//! - Block events (`level.blockEvent`): a block's `triggerEvent` is false, an entity block's
//!   (`BaseEntityBlock`, a comparator) its block entity's, whose own is false.
//! - Model data (NeoForge): a block entity's `getModelData()` is `ModelData.EMPTY`; a client
//!   level's `getModelData(pos)` is what the block entity there gives (NeoForge's manager keeps it
//!   as of its last refresh), any other getter's `EMPTY`.
//! - A type's id (`BlockEntityType.getKey`), registry entry and valid blocks.
use crate::{Rt, V, api};
use std::collections::BTreeMap;

const BE_PACKAGE: &str = "net/minecraft/world/level/block/entity/";
const TYPE: &str = "net/minecraft/world/level/block/entity/BlockEntityType";
const CUSTOM_DATA: &str = "net/minecraft/world/item/component/CustomData";
const LOAD_INTO_DESC: &str = "(Lnet/minecraft/world/level/block/entity/BlockEntity;Lnet/minecraft/core/HolderLookup$Provider;)Z";
const BLOCK: &str = "net/minecraft/world/level/block/Block";
const BEHAVIOUR: &str = "net/minecraft/world/level/block/state/BlockBehaviour";
const BASE_ENTITY_BLOCK: &str = "net/minecraft/world/level/block/BaseEntityBlock";
const COMPARATOR: &str = "net/minecraft/world/level/block/ComparatorBlock";
const BLOCK_TRIGGER_DESC: &str = "(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;II)Z";
const STATE_TRIGGER_DESC: &str =
    "(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;II)Z";
const MODEL_DATA_DESC: &str = "()Lnet/neoforged/neoforge/client/model/data/ModelData;";
const MODEL_DATA_AT_DESC: &str =
    "(Lnet/minecraft/core/BlockPos;)Lnet/neoforged/neoforge/client/model/data/ModelData;";
const MODEL_DATA_EMPTY: &str = "net/neoforged/neoforge/client/model/data/ModelData.EMPTY";
/// Vanilla's blocks with a `triggerEvent` of their own (a note block plays, a piston moves), and
/// block entities (a chest's lid, a bell's ring): theirs isn't the base class's.
const OWN_BLOCK_TRIGGER: [&str; 2] = [
    "net/minecraft/world/level/block/NoteBlock",
    "net/minecraft/world/level/block/piston/PistonBaseBlock",
];
const OWN_ENTITY_TRIGGER: [&str; 7] = [
    "BellBlockEntity",
    "ChestBlockEntity",
    "DecoratedPotBlockEntity",
    "EnderChestBlockEntity",
    "ShulkerBoxBlockEntity",
    "SpawnerBlockEntity",
    "TheEndGatewayBlockEntity",
];

/// A virtual call's full key and its `name.desc`.
type Virtual = (&'static str, &'static str);
const SAVE: Virtual = (
    "net/minecraft/world/level/block/entity/BlockEntity.saveAdditional.(Lnet/minecraft/nbt/CompoundTag;Lnet/minecraft/core/HolderLookup$Provider;)V",
    "saveAdditional.(Lnet/minecraft/nbt/CompoundTag;Lnet/minecraft/core/HolderLookup$Provider;)V",
);
const LOAD: Virtual = (
    "net/minecraft/world/level/block/entity/BlockEntity.loadAdditional.(Lnet/minecraft/nbt/CompoundTag;Lnet/minecraft/core/HolderLookup$Provider;)V",
    "loadAdditional.(Lnet/minecraft/nbt/CompoundTag;Lnet/minecraft/core/HolderLookup$Provider;)V",
);
const ONLY_OP: Virtual = (
    "net/minecraft/world/level/block/entity/BlockEntity.onlyOpCanSetNbt.()Z",
    "onlyOpCanSetNbt.()Z",
);
const REMOVE_COMPONENTS: Virtual = (
    "net/minecraft/world/level/block/entity/BlockEntity.removeComponentsFromTag.(Lnet/minecraft/nbt/CompoundTag;)V",
    "removeComponentsFromTag.(Lnet/minecraft/nbt/CompoundTag;)V",
);
const SET_CHANGED: Virtual = (
    "net/minecraft/world/level/block/entity/BlockEntity.setChanged.()V",
    "setChanged.()V",
);
const ENTITY_TRIGGER: Virtual = (
    "net/minecraft/world/level/block/entity/BlockEntity.triggerEvent.(II)Z",
    "triggerEvent.(II)Z",
);
const MODEL_DATA: Virtual = (
    "net/minecraft/world/level/block/entity/BlockEntity.getModelData.()Lnet/neoforged/neoforge/client/model/data/ModelData;",
    "getModelData.()Lnet/neoforged/neoforge/client/model/data/ModelData;",
);
const BLOCK_TRIGGER: Virtual = (
    "net/minecraft/world/level/block/state/BlockBehaviour.triggerEvent.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;II)Z",
    "triggerEvent.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;II)Z",
);
const PUT: &str = "net/minecraft/nbt/CompoundTag.put.(Ljava/lang/String;Lnet/minecraft/nbt/Tag;)Lnet/minecraft/nbt/Tag;";
const PUT_STRING: &str =
    "net/minecraft/nbt/CompoundTag.putString.(Ljava/lang/String;Ljava/lang/String;)V";
const PUT_INT: &str = "net/minecraft/nbt/CompoundTag.putInt.(Ljava/lang/String;I)V";
const GET_STRING: &str =
    "net/minecraft/nbt/CompoundTag.getString.(Ljava/lang/String;)Ljava/lang/String;";
const GET_INT: &str = "net/minecraft/nbt/CompoundTag.getInt.(Ljava/lang/String;)I";
const GET_COMPOUND: &str =
    "net/minecraft/nbt/CompoundTag.getCompound.(Ljava/lang/String;)Lnet/minecraft/nbt/CompoundTag;";
const CONTAINS: &str = "net/minecraft/nbt/CompoundTag.contains.(Ljava/lang/String;I)Z";
const COPY: &str = "net/minecraft/nbt/CompoundTag.copy.()Lnet/minecraft/nbt/CompoundTag;";
const MERGE: &str = "net/minecraft/nbt/CompoundTag.merge.(Lnet/minecraft/nbt/CompoundTag;)Lnet/minecraft/nbt/CompoundTag;";
const REMOVE: &str = "net/minecraft/nbt/CompoundTag.remove.(Ljava/lang/String;)V";
const IS_EMPTY: &str = "net/minecraft/nbt/CompoundTag.isEmpty.()Z";
const EQUALS: &str = "net/minecraft/nbt/CompoundTag.equals.(Ljava/lang/Object;)Z";
const ENTITY_DATA: &str = "net/minecraft/core/component/DataComponents.BLOCK_ENTITY_DATA";
const EMPTY_DATA: &str = "net/minecraft/world/item/component/CustomData.EMPTY";
const CUSTOM_DATA_OF: &str = "net/minecraft/world/item/component/CustomData.of.(Lnet/minecraft/nbt/CompoundTag;)Lnet/minecraft/world/item/component/CustomData;";
const DATA_IS_EMPTY: &str = "net/minecraft/world/item/component/CustomData.isEmpty.()Z";
const STACK_SET: &str = "net/minecraft/world/item/ItemStack.set.(Lnet/minecraft/core/component/DataComponentType;Ljava/lang/Object;)Ljava/lang/Object;";
const STACK_REMOVE: &str = "net/minecraft/world/item/ItemStack.remove.(Lnet/minecraft/core/component/DataComponentType;)Ljava/lang/Object;";
const STACK_GET_OR_DEFAULT: &str = "net/minecraft/world/item/ItemStack.getOrDefault.(Lnet/minecraft/core/component/DataComponentType;Ljava/lang/Object;)Ljava/lang/Object;";
const STACK_APPLY: &str = "net/minecraft/world/item/ItemStack.applyComponents.(Lnet/minecraft/core/component/DataComponentMap;)V";
const GET_SERVER: &str =
    "net/minecraft/world/level/Level.getServer.()Lnet/minecraft/server/MinecraftServer;";
const GET_BLOCK_ENTITY: &str = "net/minecraft/world/level/Level.getBlockEntity.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/entity/BlockEntity;";
const IS_CLIENT: &str = "net/minecraft/world/level/Level.isClientSide.()Z";
const REGISTRY_ACCESS: &str =
    "net/minecraft/world/level/Level.registryAccess.()Lnet/minecraft/core/RegistryAccess;";
const CREATE: &str = "net/minecraft/world/level/block/entity/BlockEntityType.create.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/world/level/block/entity/BlockEntity;";
const GET_BLOCK: &str = "net/minecraft/world/level/block/state/BlockBehaviour$BlockStateBase.getBlock.()Lnet/minecraft/world/level/block/Block;";
/// The key of NeoForge's own data of a block entity (`getPersistentData()`), and its field.
const NEOFORGE_DATA: &str = "NeoForgeData";
const PERSISTENT: &str = "$persistentData";
/// A compound's kind, for `contains(key, kind)`.
const COMPOUND_KIND: i32 = 10;

/// Whether `owner` is the API's block entity class or one of vanilla's.
fn block_entity_class(owner: &str) -> bool {
    owner.starts_with(BE_PACKAGE) && owner.ends_with("BlockEntity")
}

fn player_class(owner: &str) -> bool {
    matches!(
        owner,
        "net/minecraft/world/entity/player/Player"
            | "net/minecraft/server/level/ServerPlayer"
            | "net/minecraft/client/player/LocalPlayer"
            | "net/minecraft/client/player/AbstractClientPlayer"
    )
}

/// The block classes whose `triggerEvent` [`call`] answers: the base classes' and the entity
/// blocks'.
fn block_trigger_class(owner: &str) -> bool {
    matches!(owner, BLOCK | BEHAVIOUR | BASE_ENTITY_BLOCK | COMPARATOR)
        || crate::api_common::vanilla_is_a(owner, BASE_ENTITY_BLOCK)
}

/// The keys [`call`] may answer.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    let entity = block_entity_class(owner)
        && match name {
            "triggerEvent" => {
                desc == "(II)Z" && !OWN_ENTITY_TRIGGER.iter().any(|c| owner.ends_with(c))
            }
            "getModelData" => desc == MODEL_DATA_DESC,
            _ => matches!(
                name,
                "saveWithFullMetadata"
                    | "saveWithId"
                    | "saveWithoutMetadata"
                    | "saveCustomOnly"
                    | "saveCustomAndMetadata"
                    | "loadWithComponents"
                    | "loadCustomOnly"
                    | "loadStatic"
                    | "addEntityType"
                    | "getPosFromTag"
                    | "saveToItem"
                    | "removeComponentsFromTag"
                    | "onlyOpCanSetNbt"
                    | "getPersistentData"
                    | "fillCrashReportCategory"
                    | "hasCustomOutlineRendering"
                    | "getRenderData"
            ),
        };
    let kind =
        owner == TYPE && matches!(name, "getKey" | "getValidBlocks" | "builtInRegistryHolder");
    // A sign's item opens the sign's editor after.
    let item = owner.starts_with("net/minecraft/world/item/")
        && !owner.ends_with("SignItem")
        && matches!(name, "updateCustomBlockEntityTag" | "setBlockEntityData");
    let trigger = name == "triggerEvent"
        && ((desc == BLOCK_TRIGGER_DESC && block_trigger_class(owner))
            || (desc == STATE_TRIGGER_DESC
                && owner == "net/minecraft/world/level/block/state/BlockState"));
    let model_data =
        name == "getModelData" && desc == MODEL_DATA_AT_DESC && owner.starts_with("net/minecraft/");
    entity
        || kind
        || item
        || trigger
        || model_data
        || (owner == CUSTOM_DATA && name == "loadInto" && desc == LOAD_INTO_DESC)
        || (player_class(owner) && name == "canUseGameMasterBlocks" && desc == "()Z")
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "saveCustomOnly" => save_custom(rt, a(0), a(1)),
        "saveWithoutMetadata" => save_without_metadata(rt, a(0), a(1)),
        "saveWithId" | "saveWithFullMetadata" | "saveCustomAndMetadata" => {
            let tag = if name == "saveCustomAndMetadata" {
                save_custom(rt, a(0), a(1))
            } else {
                save_without_metadata(rt, a(0), a(1))
            };
            if rt.thrown.is_none() {
                if name == "saveWithId" {
                    save_id(rt, a(0), tag);
                } else {
                    save_metadata(rt, a(0), tag);
                }
            }
            tag
        }
        "loadCustomOnly" => {
            load_custom(rt, a(0), a(1), a(2));
            V::Void
        }
        "loadWithComponents" => {
            load_with_components(rt, a(0), a(1), a(2));
            V::Void
        }
        "loadStatic" => load_static(rt, a(0), a(1), a(2), a(3)),
        "addEntityType" => {
            add_entity_type(rt, a(0), a(1));
            V::Void
        }
        "getPosFromTag" => {
            let [x, y, z] = ["x", "y", "z"].map(|k| {
                let k = rt.string(k);
                api::call(rt, GET_INT, &[a(0), k]).i()
            });
            api::block_pos(rt, x.into(), y.into(), z.into())
        }
        "saveToItem" => {
            save_to_item(rt, a(0), a(1), a(2));
            V::Void
        }
        "removeComponentsFromTag" | "fillCrashReportCategory" => V::Void,
        "onlyOpCanSetNbt" | "hasCustomOutlineRendering" => V::I(0),
        "getRenderData" => V::NULL,
        "getPersistentData" => persistent_data(rt, a(0)),
        "getModelData" if desc == MODEL_DATA_AT_DESC => model_data_at(rt, a(0), a(1)),
        "getModelData" => api::get_static(rt, MODEL_DATA_EMPTY),
        "triggerEvent" if desc == "(II)Z" => V::I(0),
        "triggerEvent" if desc == STATE_TRIGGER_DESC => {
            let block = api::call(rt, GET_BLOCK, &[a(0)]);
            api::virtual_call(rt, BLOCK_TRIGGER, &[block, a(0), a(1), a(2), a(3), a(4)])
        }
        "triggerEvent" => return block_trigger(rt, a(0), a(2), a(3), a(4), a(5)),
        "getKey" => match type_key(rt, a(0)) {
            Some(id) => rt.string(&id),
            None => V::NULL,
        },
        "getValidBlocks" => {
            let blocks = rt.get_field(a(0), "$blocks");
            let blocks = if blocks.r() == 0 {
                Vec::new()
            } else {
                crate::jutil::elements(rt, blocks)
            };
            crate::jutil::new_set_of(rt, &blocks)
        }
        "builtInRegistryHolder" => {
            let id = type_key(rt, a(0)).unwrap_or_default();
            crate::neo::registry_holder(rt, "block_entity_type", &id, a(0))
        }
        // The instance method (pos, level, player, stack, state) is the static one (level, player,
        // pos, stack).
        "updateCustomBlockEntityTag" if desc.starts_with("(Lnet/minecraft/core/BlockPos;") => {
            V::I(i32::from(update_custom_tag(rt, a(2), a(3), a(1), a(4))))
        }
        "updateCustomBlockEntityTag" => {
            V::I(i32::from(update_custom_tag(rt, a(0), a(1), a(2), a(3))))
        }
        "setBlockEntityData" => {
            set_block_entity_data(rt, a(0), a(1), a(2));
            V::Void
        }
        "loadInto" => V::I(i32::from(load_into(rt, a(0), a(1), a(2)))),
        "canUseGameMasterBlocks" => V::I(i32::from(may_use_game_master_blocks(rt, a(0)))),
        _ => return None,
    })
}

/// The base class's `saveAdditional`: NeoForge's own data, when the block entity has any.
pub(crate) fn base_save(rt: &mut Rt, be: V, tag: V) {
    let data = rt.get_field(be, PERSISTENT);
    if data.r() != 0 && tag.r() != 0 {
        let copy = api::call(rt, COPY, &[data]);
        let key = rt.string(NEOFORGE_DATA);
        api::call(rt, PUT, &[tag, key, copy]);
    }
}

/// The base class's `loadAdditional`: NeoForge's own data back, when the tag has it.
pub(crate) fn base_load(rt: &mut Rt, be: V, tag: V) {
    if tag.r() == 0 || be.r() == 0 {
        return;
    }
    let key = rt.string(NEOFORGE_DATA);
    if api::call(rt, CONTAINS, &[tag, key, V::I(COMPOUND_KIND)]).i() != 0 {
        let data = api::call(rt, GET_COMPOUND, &[tag, key]);
        rt.put_field(be, PERSISTENT, data);
    }
}

/// `getPersistentData()`: NeoForge's own compound of the block entity, made the first time.
fn persistent_data(rt: &mut Rt, be: V) -> V {
    let data = rt.get_field(be, PERSISTENT);
    if data.r() != 0 {
        return data;
    }
    let data = crate::nbt::to_runtime(rt, &BTreeMap::new());
    rt.put_field(be, PERSISTENT, data);
    data
}

/// `saveCustomOnly(registries)`: a new compound its own `saveAdditional` fills.
fn save_custom(rt: &mut Rt, be: V, registries: V) -> V {
    let tag = crate::nbt::to_runtime(rt, &BTreeMap::new());
    api::virtual_call(rt, SAVE, &[be, tag, registries]);
    tag
}

/// `saveWithoutMetadata(registries)`: [`save_custom`], and what it kept of its stack merged in
/// under `components` (nothing when it kept nothing).
fn save_without_metadata(rt: &mut Rt, be: V, registries: V) -> V {
    let tag = save_custom(rt, be, registries);
    if rt.thrown.is_some() {
        return tag;
    }
    let kept = crate::components::kept_saved(rt, be);
    if !kept.is_empty() {
        let wrapped = BTreeMap::from([(
            "components".to_owned(),
            crate::nbt::Nbt::Compound(kept.into_iter().collect()),
        )]);
        let wrapped = crate::nbt::to_runtime(rt, &wrapped);
        api::call(rt, MERGE, &[tag, wrapped]);
    }
    tag
}

/// The id block entity type `ty` is registered under (`create:mechanical_press`).
fn type_key(rt: &Rt, ty: V) -> Option<String> {
    if ty.r() == 0 {
        return None;
    }
    rt.registered_name(ty, "block_entity_type")
        .map(str::to_owned)
}

fn put_string(rt: &mut Rt, tag: V, key: &str, value: &str) {
    let (k, v) = (rt.string(key), rt.string(value));
    api::call(rt, PUT_STRING, &[tag, k, v]);
}

/// `saveId(tag)`: its type's id; false, with vanilla's `RuntimeException` thrown, for a type
/// nothing registered.
fn save_id(rt: &mut Rt, be: V, tag: V) -> bool {
    let ty = rt.get_field(be, "$type");
    match type_key(rt, ty) {
        Some(id) => {
            put_string(rt, tag, "id", &id);
            true
        }
        None => {
            let class = rt.class_of(be).replace('/', ".");
            crate::jlang::throw_with(
                rt,
                "java/lang/RuntimeException",
                &format!("class {class} is missing a mapping! This is a bug!"),
            );
            false
        }
    }
}

/// `saveMetadata(tag)`: its type's id and its position.
fn save_metadata(rt: &mut Rt, be: V, tag: V) {
    if !save_id(rt, be, tag) {
        return;
    }
    let pos = api::pos_of(rt, rt.get_field(be, "$pos"));
    for (k, v) in ["x", "y", "z"].into_iter().zip(pos) {
        let k = rt.string(k);
        api::call(rt, PUT_INT, &[tag, k, V::I(v)]);
    }
}

/// `addEntityType(tag, type)`: the type's id as `id`; a type nothing registered is vanilla's
/// `NullPointerException`.
fn add_entity_type(rt: &mut Rt, tag: V, ty: V) {
    match type_key(rt, ty) {
        Some(id) => put_string(rt, tag, "id", &id),
        None => {
            crate::jlang::throw_with(
                rt,
                "java/lang/NullPointerException",
                "Cannot invoke \"net.minecraft.resources.ResourceLocation.toString()\" because the return value of \"net.minecraft.world.level.block.entity.BlockEntityType.getKey(net.minecraft.world.level.block.entity.BlockEntityType)\" is null",
            );
        }
    }
}

/// `loadCustomOnly(tag, registries)`: its own `loadAdditional`.
fn load_custom(rt: &mut Rt, be: V, tag: V, registries: V) {
    api::virtual_call(rt, LOAD, &[be, tag, registries]);
}

/// `loadWithComponents(tag, registries)`: [`load_custom`], then what it keeps of a stack from
/// `components` (none when the tag has none).
fn load_with_components(rt: &mut Rt, be: V, tag: V, registries: V) {
    load_custom(rt, be, tag, registries);
    if rt.thrown.is_some() {
        return;
    }
    let kept: Vec<(String, crate::nbt::Nbt)> =
        match crate::nbt::from_runtime(rt, tag).remove("components") {
            Some(crate::nbt::Nbt::Compound(c)) => c.into_iter().collect(),
            _ => Vec::new(),
        };
    crate::components::load_kept(rt, be, &kept);
}

/// `BlockEntity.loadStatic(pos, state, tag, registries)`: see the module's doc. What throws is
/// logged, as vanilla logs it.
fn load_static(rt: &mut Rt, pos: V, state: V, tag: V, registries: V) -> V {
    let id_key = rt.string("id");
    let id = api::call(rt, GET_STRING, &[tag, id_key]);
    let id = rt.str_of(id).unwrap_or("").to_owned();
    let Ok(id) = crate::neo::checked_location(&id, ':') else {
        return V::NULL;
    };
    let ty = crate::neo::registry_get(rt, "block_entity_type", &id);
    if ty.r() == 0 {
        return V::NULL;
    }
    let be = api::call(rt, CREATE, &[ty, pos, state]);
    if rt.thrown.is_some() || be.r() == 0 {
        rt.report_thrown("BlockEntity.loadStatic (making it)");
        return V::NULL;
    }
    load_with_components(rt, be, tag, registries);
    if rt.thrown.is_some() {
        rt.report_thrown("BlockEntity.loadStatic (loading it)");
        return V::NULL;
    }
    be
}

/// `BlockItem.setBlockEntityData(stack, type, tag)`: the tag, without an id, as the stack's
/// `block_entity_data` with the type's id; an empty one takes it off the stack.
fn set_block_entity_data(rt: &mut Rt, stack: V, ty: V, tag: V) {
    let id = rt.string("id");
    api::call(rt, REMOVE, &[tag, id]);
    let kind = api::get_static(rt, ENTITY_DATA);
    if api::call(rt, IS_EMPTY, &[tag]).i() != 0 {
        api::call(rt, STACK_REMOVE, &[stack, kind]);
        return;
    }
    add_entity_type(rt, tag, ty);
    if rt.thrown.is_some() {
        return;
    }
    let data = api::call(rt, CUSTOM_DATA_OF, &[tag]);
    api::call(rt, STACK_SET, &[stack, kind, data]);
}

/// `saveToItem(stack, registries)`: its own data on the stack (`setBlockEntityData`), less what
/// its `removeComponentsFromTag` takes out, and its components.
fn save_to_item(rt: &mut Rt, be: V, stack: V, registries: V) {
    let tag = save_custom(rt, be, registries);
    if rt.thrown.is_some() {
        return;
    }
    api::virtual_call(rt, REMOVE_COMPONENTS, &[be, tag]);
    let ty = rt.get_field(be, "$type");
    set_block_entity_data(rt, stack, ty, tag);
    if rt.thrown.is_some() {
        return;
    }
    let components = crate::components::collect_components(rt, be);
    api::call(rt, STACK_APPLY, &[stack, components]);
}

/// `BlockItem.updateCustomBlockEntityTag(level, player, pos, stack)`: see the module's doc.
fn update_custom_tag(rt: &mut Rt, level: V, player: V, pos: V, stack: V) -> bool {
    if api::call(rt, GET_SERVER, &[level]).r() == 0 {
        return false;
    }
    let (kind, empty) = (
        api::get_static(rt, ENTITY_DATA),
        api::get_static(rt, EMPTY_DATA),
    );
    let data = api::call(rt, STACK_GET_OR_DEFAULT, &[stack, kind, empty]);
    if data.r() == 0 || api::call(rt, DATA_IS_EMPTY, &[data]).i() != 0 {
        return false;
    }
    let be = api::call(rt, GET_BLOCK_ENTITY, &[level, pos]);
    if be.r() == 0 {
        return false;
    }
    let allowed = api::call(rt, IS_CLIENT, &[level]).i() != 0
        || api::virtual_call(rt, ONLY_OP, &[be]).i() == 0
        || may_use_game_master_blocks(rt, player);
    if !allowed {
        return false;
    }
    let registries = api::call(rt, REGISTRY_ACCESS, &[level]);
    load_into(rt, data, be, registries)
}

/// `CustomData.loadInto(blockEntity, registries)`: see the module's doc. What throws is logged,
/// as vanilla logs it.
fn load_into(rt: &mut Rt, data: V, be: V, registries: V) -> bool {
    if data.r() == 0 || be.r() == 0 {
        return false;
    }
    let current = save_custom(rt, be, registries);
    if rt.thrown.is_some() {
        return false;
    }
    let before = api::call(rt, COPY, &[current]);
    let given = rt.get_field(data, "tag");
    api::call(rt, MERGE, &[current, given]);
    if api::call(rt, EQUALS, &[current, before]).i() != 0 {
        return false;
    }
    load_custom(rt, be, current, registries);
    if rt.thrown.is_none() {
        api::virtual_call(rt, SET_CHANGED, &[be]);
    }
    if rt.thrown.is_none() {
        return true;
    }
    rt.report_thrown("CustomData.loadInto");
    load_custom(rt, be, before, registries);
    rt.report_thrown("CustomData.loadInto (rolling back)");
    false
}

/// `Player.canUseGameMasterBlocks()`: a player in creative at operator level 2 or more; never a
/// fake player. The level is the server's, else (on a client) the local player's, as the server
/// last said it.
pub(crate) fn may_use_game_master_blocks(rt: &Rt, player: V) -> bool {
    if player.r() == 0 || crate::fake_players::is_fake(rt, player) {
        return false;
    }
    let Some(h) = rt.host() else { return false };
    let level = crate::network::player_uuid(rt, player)
        .and_then(|uuid| h.player_permission(uuid))
        .or_else(|| h.client_player().map(|(_, level)| level));
    h.player_creative() && level.unwrap_or(0) >= 2
}

/// `block.triggerEvent(state, level, pos, id, param)`: an entity block's is its block entity's;
/// any other's false. None for a vanilla block with its own ([`OWN_BLOCK_TRIGGER`]).
fn block_trigger(rt: &mut Rt, block: V, level: V, pos: V, id: V, param: V) -> Option<V> {
    if OWN_BLOCK_TRIGGER.iter().any(|c| rt.instance_of(block, c)) {
        return None;
    }
    if !(rt.instance_of(block, BASE_ENTITY_BLOCK) || rt.instance_of(block, COMPARATOR)) {
        return Some(V::I(0));
    }
    let be = api::call(rt, GET_BLOCK_ENTITY, &[level, pos]);
    if be.r() == 0 {
        return Some(V::I(0));
    }
    Some(V::I(i32::from(
        api::virtual_call(rt, ENTITY_TRIGGER, &[be, id, param]).i() != 0,
    )))
}

/// `getter.getModelData(pos)`: on a client level, the model data of the block entity at `pos`
/// (none for a removed one); `ModelData.EMPTY` elsewhere. A block entity giving null is NeoForge's
/// `NullPointerException`.
fn model_data_at(rt: &mut Rt, getter: V, pos: V) -> V {
    let empty = api::get_static(rt, MODEL_DATA_EMPTY);
    let client = getter.r() != 0
        && (rt.class_of(getter) == "net/minecraft/client/multiplayer/ClientLevel"
            || rt.get_field(getter, "isClientSide").i() != 0);
    if !client {
        return empty;
    }
    let be = api::call(rt, GET_BLOCK_ENTITY, &[getter, pos]);
    if be.r() == 0 || rt.get_field(be, "remove").i() != 0 {
        return empty;
    }
    let data = api::virtual_call(rt, MODEL_DATA, &[be]);
    if data.r() == 0 && rt.thrown.is_none() {
        let id = type_key(rt, rt.get_field(be, "$type")).unwrap_or_else(|| "null".to_owned());
        let [x, y, z] = api::pos_of(rt, pos);
        crate::jlang::throw_with(
            rt,
            "java/lang/NullPointerException",
            &format!("Null ModelData provided by {id} @ BlockPos{{x={x}, y={y}, z={z}}}"),
        );
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;
    use crate::nbt::Nbt;

    const SAVE_FULL: &str = "net/minecraft/world/level/block/entity/BlockEntity.saveWithFullMetadata.(Lnet/minecraft/core/HolderLookup$Provider;)Lnet/minecraft/nbt/CompoundTag;";
    const LOAD_WITH: &str = "net/minecraft/world/level/block/entity/BlockEntity.loadWithComponents.(Lnet/minecraft/nbt/CompoundTag;Lnet/minecraft/core/HolderLookup$Provider;)V";
    const LOAD_STATIC: &str = "net/minecraft/world/level/block/entity/BlockEntity.loadStatic.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/nbt/CompoundTag;Lnet/minecraft/core/HolderLookup$Provider;)Lnet/minecraft/world/level/block/entity/BlockEntity;";
    const SAVE_TO_ITEM: &str = "net/minecraft/world/level/block/entity/BlockEntity.saveToItem.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/core/HolderLookup$Provider;)V";
    const PERSISTENT_DATA: &str = "net/minecraft/world/level/block/entity/BlockEntity.getPersistentData.()Lnet/minecraft/nbt/CompoundTag;";
    const INIT: &str = "net/minecraft/world/level/block/entity/BlockEntity.<init>.(Lnet/minecraft/world/level/block/entity/BlockEntityType;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)V";
    const STACK_GET: &str = "net/minecraft/world/item/ItemStack.get.(Lnet/minecraft/core/component/DataComponentType;)Ljava/lang/Object;";
    /// `BlockItem.updateCustomBlockEntityTag(pos, level, player, stack, state)`.
    const UPDATE_TAG: Virtual = (
        "net/minecraft/world/item/BlockItem.updateCustomBlockEntityTag.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/Level;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/level/block/state/BlockState;)Z",
        "updateCustomBlockEntityTag.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/Level;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/level/block/state/BlockState;)Z",
    );
    const PRESS: &str = "test/PressEntity";
    const PRESS_BLOCK: &str = "test/PressBlock";

    /// A mod's block entity that saves and loads its `speed`, after its base class (where
    /// NeoForge's own data goes), opens its lid on block event 1 and gives its model its speed.
    fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        if rt.class_of(args[0]) == PRESS {
            if short == SAVE.1 {
                api::call(rt, key, args);
                let (k, speed) = (rt.string("speed"), rt.get_field(args[0], "speed"));
                api::call(rt, PUT_INT, &[args[1], k, V::I(speed.i())]);
                return V::Void;
            }
            if short == LOAD.1 {
                api::call(rt, key, args);
                let k = rt.string("speed");
                let speed = api::call(rt, GET_INT, &[args[1], k]);
                rt.put_field(args[0], "speed", speed);
                return V::Void;
            }
            if short == ENTITY_TRIGGER.1 && args[1] == V::I(1) {
                rt.put_field(args[0], "open", args[2]);
                return V::I(1);
            }
            if short == MODEL_DATA.1 {
                return rt.get_field(args[0], "model");
            }
        }
        api::call(rt, key, args)
    }

    fn press_type(rt: &mut Rt) -> V {
        let ty = rt.new_object(TYPE);
        rt.registered
            .insert(("block_entity_type".into(), "testmod:press".into()), ty);
        ty
    }

    fn press(rt: &mut Rt, ty: V, at: [i64; 3], speed: i32) -> V {
        let be = rt.new_object(PRESS);
        let pos = api::block_pos(rt, at[0], at[1], at[2]);
        let state = api::state(rt, "testmod:press");
        api::call(rt, INIT, &[be, ty, pos, state]);
        rt.put_field(be, "speed", V::I(speed));
        be
    }

    fn runtime() -> Rt {
        crate::install();
        let mut rt = Rt::new();
        rt.vcall = Some(vcall);
        rt.supers
            .insert(PRESS, "net/minecraft/world/level/block/entity/BlockEntity");
        rt.supers.insert(PRESS_BLOCK, BASE_ENTITY_BLOCK);
        rt
    }

    /// A press placed at `at` in the level `host` holds.
    fn placed(rt: &mut Rt, host: &mut MemoryHost, ty: V, at: [i32; 3]) -> V {
        let be = press(rt, ty, at.map(i64::from), 0);
        rt.block_entities.insert(at, be);
        rt.block_entity_meta
            .insert(at, ("testmod:press".into(), false));
        host.blocks.insert(at, "testmod:press".into());
        be
    }

    /// Saved with its full metadata, a press is its own data, NeoForge's data of it, its type's id
    /// and its position; another press loads it back, and `loadStatic` makes a press of it at a
    /// new place. An unknown id makes nothing.
    #[test]
    fn a_block_entity_saves_its_metadata_and_loads_back() {
        let mut rt = runtime();
        let ty = press_type(&mut rt);
        let supplier = rt.lambda_id(
            "net/minecraft/world/level/block/entity/BlockEntityType$BlockEntitySupplier",
            1,
            vec![ty],
        );
        rt.put_field(ty, "$factory", supplier);
        fn make(rt: &mut Rt, _: u32, args: &[V]) -> Option<V> {
            let ty = rt.captured(args[0])[0];
            let be = rt.new_object(PRESS);
            api::call(rt, INIT, &[be, ty, args[1], args[2]]);
            Some(be)
        }
        rt.lambda_call = Some(make);
        let be = press(&mut rt, ty, [1, 64, -3], 32);
        let own = api::call(&mut rt, PERSISTENT_DATA, &[be]);
        let (k, v) = (rt.string("owner"), rt.string("Alex"));
        api::call(&mut rt, PUT_STRING, &[own, k, v]);
        let provider = rt.new_object("net/minecraft/core/HolderLookup$Provider");
        let tag = api::call(&mut rt, SAVE_FULL, &[be, provider]);
        let neo = BTreeMap::from([("owner".to_owned(), Nbt::String("Alex".into()))]);
        let expected = BTreeMap::from([
            ("speed".to_owned(), Nbt::Int(32)),
            ("id".to_owned(), Nbt::String("testmod:press".into())),
            ("x".to_owned(), Nbt::Int(1)),
            ("y".to_owned(), Nbt::Int(64)),
            ("z".to_owned(), Nbt::Int(-3)),
            (NEOFORGE_DATA.to_owned(), Nbt::Compound(neo.clone())),
        ]);
        assert_eq!(crate::nbt::from_runtime(&rt, tag), expected);

        let other = press(&mut rt, ty, [0, 0, 0], 0);
        api::call(&mut rt, LOAD_WITH, &[other, tag, provider]);
        assert_eq!(rt.get_field(other, "speed"), V::I(32));
        let again = api::call(&mut rt, PERSISTENT_DATA, &[other]);
        assert_eq!(crate::nbt::from_runtime(&rt, again), neo);

        let pos = api::block_pos(&mut rt, 5, 70, 5);
        let state = api::state(&mut rt, "testmod:press");
        let made = api::call(&mut rt, LOAD_STATIC, &[pos, state, tag, provider]);
        assert_eq!(rt.class_of(made), PRESS);
        assert_eq!(rt.get_field(made, "speed"), V::I(32));
        assert_eq!(api::pos_of(&rt, rt.get_field(made, "$pos")), [5, 70, 5]);
        let unknown = crate::nbt::to_runtime(
            &mut rt,
            &BTreeMap::from([("id".to_owned(), Nbt::String("testmod:gone".into()))]),
        );
        assert_eq!(
            api::call(&mut rt, LOAD_STATIC, &[pos, state, unknown, provider]),
            V::NULL
        );
        assert!(rt.thrown.is_none());
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A block entity of a type nothing registered can't save its id.
    #[test]
    fn an_unregistered_type_cant_save_its_id() {
        let mut rt = runtime();
        let ty = rt.new_object(TYPE);
        let be = press(&mut rt, ty, [0, 0, 0], 1);
        let provider = rt.new_object("net/minecraft/core/HolderLookup$Provider");
        api::call(&mut rt, SAVE_FULL, &[be, provider]);
        let thrown = rt.thrown.take().expect("no id to save");
        assert_eq!(rt.class_of(thrown), "java/lang/RuntimeException");
        assert_eq!(
            rt.str_of(rt.get_field(thrown, "message")),
            Some("class test.PressEntity is missing a mapping! This is a bug!")
        );
    }

    /// Saved to a stack, a press is the stack's `block_entity_data` with its type's id; placed on
    /// a server, the item loads it into the new press, but not on a client.
    #[test]
    fn a_block_entitys_data_goes_on_its_item_and_back_into_a_placed_one() {
        let mut rt = runtime();
        let ty = press_type(&mut rt);
        let be = press(&mut rt, ty, [0, 0, 0], 48);
        let stack = crate::api_common::stack(&mut rt, "testmod:press", 1);
        let provider = rt.new_object("net/minecraft/core/HolderLookup$Provider");
        api::call(&mut rt, SAVE_TO_ITEM, &[be, stack, provider]);
        let kind = api::get_static(&mut rt, ENTITY_DATA);
        let data = api::call(&mut rt, STACK_GET, &[stack, kind]);
        let on_item = crate::nbt::from_runtime(&rt, rt.get_field(data, "tag"));
        assert_eq!(
            on_item,
            BTreeMap::from([
                ("id".to_owned(), Nbt::String("testmod:press".into())),
                ("speed".to_owned(), Nbt::Int(48))
            ])
        );

        let mut host = MemoryHost::default();
        let placed = placed(&mut rt, &mut host, ty, [4, 64, 4]);
        let item = rt.new_object("net/minecraft/world/item/BlockItem");
        let pos = api::block_pos(&mut rt, 4, 64, 4);
        let state = api::state(&mut rt, "testmod:press");
        rt.with_host(&mut host, |rt| {
            let client = rt.new_object("net/minecraft/client/multiplayer/ClientLevel");
            rt.put_field(client, "isClientSide", V::I(1));
            assert_eq!(
                api::virtual_call(rt, UPDATE_TAG, &[item, pos, client, V::NULL, stack, state]),
                V::I(0),
                "a client has no server"
            );
            assert_eq!(rt.get_field(placed, "speed"), V::I(0));
            let level = crate::glue::level(rt);
            assert_eq!(
                api::virtual_call(rt, UPDATE_TAG, &[item, pos, level, V::NULL, stack, state]),
                V::I(1)
            );
            assert_eq!(rt.get_field(placed, "speed"), V::I(48));
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A block event reaches an entity block's block entity (a press opens its lid on event 1),
    /// not a plain block's; a client level's model data at a position is its block entity's.
    #[test]
    fn block_events_and_model_data_reach_the_block_entity() {
        let mut rt = runtime();
        let ty = press_type(&mut rt);
        let mut host = MemoryHost::default();
        let be = placed(&mut rt, &mut host, ty, [2, 70, 2]);
        let model = rt.new_object("net/neoforged/neoforge/client/model/data/ModelData");
        rt.put_field(be, "model", model);
        let pos = api::block_pos(&mut rt, 2, 70, 2);
        let state = api::state(&mut rt, "testmod:press");
        let entity_block = rt.new_object(PRESS_BLOCK);
        let plain = rt.new_object(BLOCK);
        const TRIGGER: &str = "net/minecraft/world/level/block/BaseEntityBlock.triggerEvent.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;II)Z";
        const PLAIN_TRIGGER: &str = "net/minecraft/world/level/block/Block.triggerEvent.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;II)Z";
        const MODEL_DATA_AT: &str = "net/minecraft/world/level/Level.getModelData.(Lnet/minecraft/core/BlockPos;)Lnet/neoforged/neoforge/client/model/data/ModelData;";
        rt.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            assert_eq!(
                api::call(
                    rt,
                    TRIGGER,
                    &[entity_block, state, level, pos, V::I(1), V::I(3)]
                ),
                V::I(1)
            );
            assert_eq!(rt.get_field(be, "open"), V::I(3));
            assert_eq!(
                api::call(
                    rt,
                    TRIGGER,
                    &[entity_block, state, level, pos, V::I(2), V::I(0)]
                ),
                V::I(0),
                "the base block entity's"
            );
            assert_eq!(
                api::call(
                    rt,
                    PLAIN_TRIGGER,
                    &[plain, state, level, pos, V::I(1), V::I(0)]
                ),
                V::I(0)
            );
            let empty = api::get_static(rt, MODEL_DATA_EMPTY);
            assert_eq!(
                api::call(rt, MODEL_DATA_AT, &[level, pos]),
                empty,
                "a server level has none"
            );
            let client = rt.new_object("net/minecraft/client/multiplayer/ClientLevel");
            rt.put_field(client, "isClientSide", V::I(1));
            assert_eq!(api::call(rt, MODEL_DATA_AT, &[client, pos]), model);
            let elsewhere = api::block_pos(rt, 9, 70, 9);
            assert_eq!(api::call(rt, MODEL_DATA_AT, &[client, elsewhere]), empty);
            rt.put_field(be, "model", V::NULL);
            api::call(rt, MODEL_DATA_AT, &[client, pos]);
            let thrown = rt.thrown.take().expect("null model data");
            assert_eq!(
                rt.str_of(rt.get_field(thrown, "message")),
                Some("Null ModelData provided by testmod:press @ BlockPos{x=2, y=70, z=2}")
            );
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
