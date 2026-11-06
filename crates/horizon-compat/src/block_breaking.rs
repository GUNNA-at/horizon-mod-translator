//! A player breaking a block, as NeoForge 1.21.1 has it, for a mod that breaks blocks through a
//! player of its own: Create's deployer punches with one (`DeployerHandler.tryHarvestBlock`, its copy
//! of `ServerPlayerGameMode.destroyBlock`), its drills and saws break with one
//! (`BlockHelper.destroyBlockAs`), and mods' miners and farms do the same:
//!
//! - how fast it breaks the block (`BlockState.getDestroyProgress`, `Player.getDigSpeed`): its
//!   tool's speed on the block, whether that tool is right for the drops, the player's mining
//!   attributes, haste, conduit power and mining fatigue, its eyes under water, its feet off the
//!   ground, and NeoForge's `PlayerEvent.HarvestCheck` and `PlayerEvent.BreakSpeed`;
//! - whether it may (`CommonHooks.fireBlockBreak`: `BlockEvent.BreakEvent`, cancelled beforehand for
//!   a sword in creative, a game mode that can't break, a game master block);
//! - the tool's wear (`ItemStack.mineBlock`), the block's removal (`onDestroyedByPlayer`, `destroy`),
//!   its drops rolled from its loot table with the tool (`Block.getDrops`), what comes out of it
//!   afterwards (`spawnAfterBreak`: an infested block's silverfish), the cracks shown on it
//!   (`Level.destroyBlockProgress`) and its sounds (`BlockState.getSoundType`).
//!
//! A mod block's or item's own override of each answers first. What a tool does to a block, the
//! block's sounds and loot, and the cracks shown are the game's ([`crate::host::Host::item_tool`],
//! [`block_sounds`](crate::host::Host::block_sounds), [`block_drops`](crate::host::Host::block_drops),
//! [`destroy_block_progress`](crate::host::Host::destroy_block_progress)).
//!
//! Not here yet: a mod block's own `getDrops(state, LootParams.Builder)` (its loot table answers for
//! it), a mod item's tool component when the game doesn't know the item, Unbreaking on a stack
//! outside the acting player's hands, and an adventure player's `can_break` predicates.

use crate::{api, api_common, Rt, V};

const STATE: &str = "net/minecraft/world/level/block/state/BlockState";
const STATE_BASE: &str = "net/minecraft/world/level/block/state/BlockBehaviour$BlockStateBase";
const BEHAVIOUR: &str = "net/minecraft/world/level/block/state/BlockBehaviour";
const ITEM_STACK: &str = "net/minecraft/world/item/ItemStack";
const COMMON_HOOKS: &str = "net/neoforged/neoforge/common/CommonHooks";
const EVENT_HOOKS: &str = "net/neoforged/neoforge/event/EventHooks";
const GAME_MODE: &str = "net/minecraft/server/level/ServerPlayerGameMode";
const GAME_TYPE: &str = "net/minecraft/world/level/GameType";
const GAME_TYPES: [&str; 4] = ["SURVIVAL", "CREATIVE", "ADVENTURE", "SPECTATOR"];
const BREAK_SPEED: &str = "net/neoforged/neoforge/event/entity/player/PlayerEvent$BreakSpeed";
const HARVEST_CHECK: &str = "net/neoforged/neoforge/event/entity/player/PlayerEvent$HarvestCheck";
const DESTROY_ITEM: &str = "net/neoforged/neoforge/event/entity/player/PlayerDestroyItemEvent";
const PLAYER_EVENT: &str = "net/neoforged/neoforge/event/entity/player/PlayerEvent";
const LIVING_EVENT: &str = "net/neoforged/neoforge/event/entity/living/LivingEvent";
const ENTITY_EVENT: &str = "net/neoforged/neoforge/event/entity/EntityEvent";
const EVENT: &str = "net/neoforged/bus/api/Event";
const CANCELLABLE: &str = "net/neoforged/bus/api/ICancellableEvent";
const BREAK: &str = crate::world_events::BREAK;
const BLOCK_DROPS: &str = "net/neoforged/neoforge/event/level/BlockDropsEvent";
const BLOCK_EVENT: &str = "net/neoforged/neoforge/event/level/BlockEvent";
/// The events' classes above them, so a listener for `PlayerEvent` (or any event) hears them.
const SUPERS: [(&str, &str); 8] = [
    (BREAK_SPEED, PLAYER_EVENT),
    (HARVEST_CHECK, PLAYER_EVENT),
    (DESTROY_ITEM, PLAYER_EVENT),
    (PLAYER_EVENT, LIVING_EVENT),
    (LIVING_EVENT, ENTITY_EVENT),
    (ENTITY_EVENT, EVENT),
    (BLOCK_DROPS, BLOCK_EVENT),
    (BLOCK_EVENT, EVENT),
];

// The block state's calls (`BlockStateBase`'s, NeoForge's `IBlockStateExtension`'s).
const STATE_PROGRESS: &str = "(Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)F";
const STATE_HARVEST: &str = "(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;)Z";
const STATE_DESTROYED: &str = "(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;ZLnet/minecraft/world/level/material/FluidState;)Z";
const STATE_AFTER_BREAK: &str = "(Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/item/ItemStack;Z)V";
const STATE_SOUND: &str = "()Lnet/minecraft/world/level/block/SoundType;";
const STATE_EXP: &str = "(Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/entity/BlockEntity;Lnet/minecraft/world/entity/Entity;Lnet/minecraft/world/item/ItemStack;)I";
const STATE_EXP_KEY: &str = "net/minecraft/world/level/block/state/BlockState.getExpDrop.(Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/entity/BlockEntity;Lnet/minecraft/world/entity/Entity;Lnet/minecraft/world/item/ItemStack;)I";
const STATE_SOUND_AT: &str = "(Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/Entity;)Lnet/minecraft/world/level/block/SoundType;";

// A block's own (`BlockBehaviour`'s, `Block`'s and NeoForge's `IBlockExtension`'s), by name and
// descriptor as a mod block overrides them, and the key of vanilla's.
const PROGRESS: &str = "getDestroyProgress.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)F";
const PROGRESS_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.getDestroyProgress.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/entity/player/Player;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)F";
const HARVEST: &str = "canHarvestBlock.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;)Z";
const HARVEST_KEY: &str = "net/minecraft/world/level/block/Block.canHarvestBlock.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;)Z";
const DESTROYED: &str = "onDestroyedByPlayer.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;ZLnet/minecraft/world/level/material/FluidState;)Z";
const DESTROYED_KEY: &str = "net/minecraft/world/level/block/Block.onDestroyedByPlayer.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;ZLnet/minecraft/world/level/material/FluidState;)Z";
const AFTER_BREAK: &str = "spawnAfterBreak.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/item/ItemStack;Z)V";
const AFTER_BREAK_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.spawnAfterBreak.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/item/ItemStack;Z)V";
const SOUND: &str = "getSoundType.(Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/world/level/block/SoundType;";
const SOUND_KEY: &str = "net/minecraft/world/level/block/state/BlockBehaviour.getSoundType.(Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/world/level/block/SoundType;";
const SOUND_AT: &str = "getSoundType.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/Entity;)Lnet/minecraft/world/level/block/SoundType;";
const SOUND_AT_KEY: &str = "net/minecraft/world/level/block/Block.getSoundType.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/Entity;)Lnet/minecraft/world/level/block/SoundType;";
const DESTROY_DESC: &str = "(Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)V";
const EXP: &str = "getExpDrop.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/entity/BlockEntity;Lnet/minecraft/world/entity/Entity;Lnet/minecraft/world/item/ItemStack;)I";
const EXP_KEY: &str = "net/minecraft/world/level/block/Block.getExpDrop.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/entity/BlockEntity;Lnet/minecraft/world/entity/Entity;Lnet/minecraft/world/item/ItemStack;)I";
/// `Block.popExperience(level, pos, amount)`.
const POP_EXPERIENCE: &str = "(Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/core/BlockPos;I)V";
const DROPS_INIT: &str = "(Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/block/entity/BlockEntity;Ljava/util/List;Lnet/minecraft/world/entity/Entity;Lnet/minecraft/world/item/ItemStack;)V";
const PROCESS_EXPERIENCE: &str = "net/minecraft/world/item/enchantment/EnchantmentHelper.processBlockExperience.(Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/world/item/ItemStack;I)I";
const DEFAULT_STATE: &str = "net/minecraft/world/level/block/Block.defaultBlockState.()Lnet/minecraft/world/level/block/state/BlockState;";
const GET_RANDOM: &str = "net/minecraft/world/level/Level.getRandom.()Lnet/minecraft/util/RandomSource;";
const SAMPLE: &str = "net/minecraft/util/valueproviders/IntProvider.sample.(Lnet/minecraft/util/RandomSource;)I";
/// `Block.getDrops(state, level, pos, blockEntity)` and `(state, level, pos, blockEntity, entity,
/// tool)`, the static ones.
const DROPS: &str = "(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/entity/BlockEntity;)Ljava/util/List;";
const DROPS_BY: &str = "(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/entity/BlockEntity;Lnet/minecraft/world/entity/Entity;Lnet/minecraft/world/item/ItemStack;)Ljava/util/List;";
const DROPS_BY_KEY: &str = "net/minecraft/world/level/block/Block.getDrops.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/entity/BlockEntity;Lnet/minecraft/world/entity/Entity;Lnet/minecraft/world/item/ItemStack;)Ljava/util/List;";
/// `BlockBehaviour.onRemove(state, level, pos, newState, movedByPiston)`.
pub(crate) const ON_REMOVE: &str = "(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Z)V";
/// `Block.dropResources(state, level, pos)`, `(state, level, pos, blockEntity)` and `(state, level,
/// pos, blockEntity, entity, tool)`, the static ones.
const DROP_RESOURCES: [&str; 3] = [
    "(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;)V",
    "(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/entity/BlockEntity;)V",
    "(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/entity/BlockEntity;Lnet/minecraft/world/entity/Entity;Lnet/minecraft/world/item/ItemStack;)V",
];
const HAS_BLOCK_ENTITY: &str = "net/minecraft/world/level/block/state/BlockState.hasBlockEntity.()Z";
const BLOCK_ENTITY_AT: &str = "net/minecraft/world/level/Level.getBlockEntity.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/entity/BlockEntity;";
const REMOVE_BLOCK_ENTITY: &str = "net/minecraft/world/level/Level.removeBlockEntity.(Lnet/minecraft/core/BlockPos;)V";
const STATE_AFTER_BREAK_KEY: &str = "net/minecraft/world/level/block/state/BlockState.spawnAfterBreak.(Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/item/ItemStack;Z)V";
const POP_EXPERIENCE_KEY: &str = "net/minecraft/world/level/block/Block.popExperience.(Lnet/minecraft/server/level/ServerLevel;Lnet/minecraft/core/BlockPos;I)V";
const GET_BLOCK: &str = "net/minecraft/world/level/block/state/BlockState.getBlock.()Lnet/minecraft/world/level/block/Block;";
const LEVEL_EVENT: &str = "net/minecraft/world/level/Level.levelEvent.(ILnet/minecraft/core/BlockPos;I)V";
const BLOCK_ID: &str = "net/minecraft/world/level/block/Block.getId.(Lnet/minecraft/world/level/block/state/BlockState;)I";

// An item's (`Item`'s) and its stack's.
const ITEM_SPEED: &str = "getDestroySpeed.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/level/block/state/BlockState;)F";
const ITEM_SPEED_KEY: &str = "net/minecraft/world/item/Item.getDestroySpeed.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/level/block/state/BlockState;)F";
const ITEM_CORRECT: &str = "isCorrectToolForDrops.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/level/block/state/BlockState;)Z";
const ITEM_CORRECT_KEY: &str = "net/minecraft/world/item/Item.isCorrectToolForDrops.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/level/block/state/BlockState;)Z";
const ITEM_MINE: &str = "mineBlock.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/level/Level;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/LivingEntity;)Z";
const ITEM_MINE_KEY: &str = "net/minecraft/world/item/Item.mineBlock.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/level/Level;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/LivingEntity;)Z";
const ITEM_ATTACK: &str = "canAttackBlock.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;)Z";
const ITEM_ATTACK_KEY: &str = "net/minecraft/world/item/Item.canAttackBlock.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;)Z";
const STACK_SPEED: &str = "(Lnet/minecraft/world/level/block/state/BlockState;)F";
const STACK_CORRECT: &str = "(Lnet/minecraft/world/level/block/state/BlockState;)Z";
const STACK_MINE: &str = "(Lnet/minecraft/world/level/Level;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/entity/player/Player;)V";

// A player's.
const DIG_SPEED: &str = "(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/BlockPos;)F";
const DIG_SPEED_KEY: &str = "net/minecraft/world/entity/player/Player.getDigSpeed.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/BlockPos;)F";
const CORRECT_TOOL: &str = "(Lnet/minecraft/world/level/block/state/BlockState;)Z";
const CORRECT_TOOL_AT: &str = "(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;)Z";
const RESTRICTED: &str = "(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/GameType;)Z";
const RESTRICTED_KEY: &str = "net/minecraft/world/entity/player/Player.blockActionRestricted.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/GameType;)Z";
const MAIN_HAND: &str = "net/minecraft/world/entity/player/Player.getMainHandItem.()Lnet/minecraft/world/item/ItemStack;";
const IS_CREATIVE: &str = "net/minecraft/world/entity/player/Player.isCreative.()Z";
const MAY_BUILD: &str = "net/minecraft/world/entity/player/Player.mayBuild.()Z";
const ATTRIBUTE_VALUE: &str = "net/minecraft/world/entity/LivingEntity.getAttributeValue.(Lnet/minecraft/core/Holder;)D";
const HAS_EFFECT: &str = "net/minecraft/world/entity/LivingEntity.hasEffect.(Lnet/minecraft/core/Holder;)Z";
const GET_EFFECT: &str = "net/minecraft/world/entity/LivingEntity.getEffect.(Lnet/minecraft/core/Holder;)Lnet/minecraft/world/effect/MobEffectInstance;";
const AMPLIFIER: &str = "net/minecraft/world/effect/MobEffectInstance.getAmplifier.()I";
const EYE_Y: &str = "net/minecraft/world/entity/Entity.getEyeY.()D";
const GET_X: &str = "net/minecraft/world/entity/Entity.getX.()D";
const GET_Z: &str = "net/minecraft/world/entity/Entity.getZ.()D";
const HURT: &str = "net/minecraft/world/item/ItemStack.hurtAndBreak.(ILnet/minecraft/world/entity/LivingEntity;Lnet/minecraft/world/entity/EquipmentSlot;)V";
const IS_CLIENT: &str = "net/minecraft/world/level/Level.isClientSide.()Z";
const SET_BLOCK: &str = "net/minecraft/world/level/Level.setBlock.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;I)Z";
const REMOVE_BLOCK: &str = "net/minecraft/world/level/Level.removeBlock.(Lnet/minecraft/core/BlockPos;Z)Z";
const FLUID_AT: &str = "net/minecraft/world/level/Level.getFluidState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/material/FluidState;";
const FLUID_IS: &str = "net/minecraft/world/level/material/FluidState.is.(Lnet/minecraft/tags/TagKey;)Z";
const FLUID_HEIGHT: &str = "net/minecraft/world/level/material/FluidState.getHeight.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)F";
const LEGACY_BLOCK: &str = "net/minecraft/world/level/material/FluidState.createLegacyBlock.()Lnet/minecraft/world/level/block/state/BlockState;";
const BREAK_INIT: &str = "net/neoforged/neoforge/event/level/BlockEvent$BreakEvent.<init>.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/entity/player/Player;)V";

/// Vanilla 1.21.1's `GameMasterBlock`s: a player breaks one only when it may use them.
const GAME_MASTER_BLOCKS: [&str; 5] = ["minecraft:command_block", "minecraft:chain_command_block", "minecraft:repeating_command_block", "minecraft:structure_block", "minecraft:jigsaw"];

/// Whether `owner` is a vanilla block class a mod block's call (a `super.` one) names.
fn block_class(owner: &str) -> bool {
    owner == BEHAVIOUR || (owner.starts_with("net/minecraft/world/level/block/") && !owner.contains("/state/") && !owner.contains("/entity/"))
}

/// Whether `owner` is a vanilla item class a mod item's call names.
fn item_class(owner: &str) -> bool {
    owner.starts_with("net/minecraft/world/item/") && owner.ends_with("Item")
}

/// The keys [`call`] may answer.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    match owner {
        STATE | STATE_BASE => matches!(
            (name, desc),
            ("getDestroyProgress", STATE_PROGRESS) | ("getExpDrop", STATE_EXP) | ("isSolid" | "blocksMotion" | "liquid", "()Z")
                | ("canHarvestBlock", STATE_HARVEST)
                | ("requiresCorrectToolForDrops", "()Z")
                | ("onDestroyedByPlayer", STATE_DESTROYED)
                | ("spawnAfterBreak", STATE_AFTER_BREAK)
                | ("getSoundType", STATE_SOUND | STATE_SOUND_AT)
        ),
        ITEM_STACK => matches!((name, desc), ("getDestroySpeed", STACK_SPEED) | ("isCorrectToolForDrops", STACK_CORRECT) | ("mineBlock", STACK_MINE)),
        COMMON_HOOKS => name == "fireBlockBreak",
        EVENT_HOOKS => matches!(name, "doPlayerHarvestCheck" | "getBreakSpeed" | "onPlayerDestroyItem"),
        GAME_MODE => matches!(name, "getGameModeForPlayer" | "getPreviousGameModeForPlayer" | "isCreative" | "isSurvival"),
        BREAK_SPEED | HARVEST_CHECK | DESTROY_ITEM | BLOCK_DROPS => true,
        BLOCK_EVENT => matches!(name, "getLevel" | "getPos" | "getState"),
        PLAYER_EVENT | LIVING_EVENT | ENTITY_EVENT => name == "getEntity",
        EVENT | CANCELLABLE => matches!(name, "isCanceled" | "setCanceled"),
        o if api_common::is_player(o) => matches!(
            (name, desc),
            ("getDigSpeed", DIG_SPEED) | ("hasCorrectToolForDrops", CORRECT_TOOL | CORRECT_TOOL_AT) | ("blockActionRestricted", RESTRICTED) | ("$gameMode", "()Lnet/minecraft/server/level/ServerPlayerGameMode;")
        ),
        o if api::is_level(o) => name == "destroyBlockProgress" && desc == "(ILnet/minecraft/core/BlockPos;I)V",
        o if item_class(o) => {
            let nd = (name, desc);
            [ITEM_SPEED, ITEM_CORRECT, ITEM_MINE, ITEM_ATTACK].iter().any(|k| k.split_once('.') == Some(nd))
        }
        o if block_class(o) => {
            let nd = (name, desc);
            [PROGRESS, HARVEST, DESTROYED, AFTER_BREAK, SOUND, SOUND_AT].iter().any(|k| k.split_once('.') == Some(nd))
                || (name == "destroy" && desc == DESTROY_DESC)
                || (name == "getDrops" && matches!(desc, DROPS | DROPS_BY))
                || (name == "popExperience" && desc == POP_EXPERIENCE)
                || (name == "onRemove" && desc == ON_REMOVE)
                || (name == "dropResources" && DROP_RESOURCES.contains(&desc))
                || (o == "net/minecraft/world/level/block/IceBlock" && name == "meltsInto")
                || EXP.split_once('.') == Some(nd)
        }
        _ => false,
    }
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match owner {
        STATE | STATE_BASE => {
            let st = a(0);
            let text = text_of(rt, st);
            match name {
                "getDestroyProgress" => match block_override(rt, &text, PROGRESS_KEY, PROGRESS, &[st, a(1), a(2), a(3)]) {
                    Some(v) => v,
                    None => V::F(destroy_progress(rt, st, a(1), a(2), a(3))),
                },
                "canHarvestBlock" => match block_override(rt, &text, HARVEST_KEY, HARVEST, &[st, a(1), a(2), a(3)]) {
                    Some(v) => v,
                    None => V::I(harvest_check(rt, a(3), st, a(1), a(2)) as i32),
                },
                "requiresCorrectToolForDrops" => V::I(requires_tool(rt, &text) as i32),
                "getExpDrop" => V::I(exp_drop(rt, st, &text, [a(1), a(2), a(3), a(4), a(5)])),
                "isSolid" => V::I(solid(rt, &text) as i32),
                // `liquid()`: a water or lava block (a mod's fluid's own `LiquidBlock` too).
                "liquid" => V::I(liquid(rt, &text) as i32),
                // `blocksMotion`: solid, but never a cobweb or a bamboo sapling.
                "blocksMotion" => {
                    let (block, _) = crate::host::split_state(&text);
                    V::I((!matches!(block, "minecraft:cobweb" | "minecraft:bamboo_sapling") && solid(rt, &text)) as i32)
                }
                "onDestroyedByPlayer" => match block_override(rt, &text, DESTROYED_KEY, DESTROYED, &[st, a(1), a(2), a(3), a(4), a(5)]) {
                    Some(v) => v,
                    None => destroyed_by_player(rt, a(1), a(2), a(5)),
                },
                "spawnAfterBreak" => {
                    if block_override(rt, &text, AFTER_BREAK_KEY, AFTER_BREAK, &[st, a(1), a(2), a(3), a(4)]).is_none() && mod_block_object(rt, &text).is_none() {
                        let p = api::pos_of(rt, a(2));
                        let tool = held(rt, a(3));
                        if let Some(h) = rt.host_mut() {
                            h.spawn_after_break(p, &text, tool.as_ref());
                        }
                    }
                    V::Void
                }
                _ if desc == STATE_SOUND_AT => match block_override(rt, &text, SOUND_AT_KEY, SOUND_AT, &[st, a(1), a(2), a(3)]) {
                    Some(v) => v,
                    None => sound_type(rt, st, &text),
                },
                _ => sound_type(rt, st, &text),
            }
        }
        ITEM_STACK => {
            let (stack, st) = (a(0), a(1));
            match name {
                "getDestroySpeed" => V::F(destroy_speed(rt, stack, st)),
                "isCorrectToolForDrops" => V::I(correct_tool(rt, stack, st) as i32),
                // `item.mineBlock(...)`, then `awardStat(ITEM_USED)`: a fake player's stats go nowhere,
                // and the game keeps its own player's.
                _ => {
                    if item_override(rt, stack, ITEM_MINE_KEY, ITEM_MINE, &[stack, a(1), a(2), a(3), a(4)]).is_none() {
                        mine_block(rt, stack, a(1), a(2), a(4));
                    }
                    V::Void
                }
            }
        }
        COMMON_HOOKS => fire_block_break(rt, [a(0), a(1), a(2), a(3), a(4)]),
        EVENT_HOOKS => match name {
            "doPlayerHarvestCheck" => V::I(harvest_check(rt, a(0), a(1), a(2), a(3)) as i32),
            "getBreakSpeed" => V::F(break_speed(rt, a(0), a(1), a(2).f(), a(3))),
            _ => {
                destroyed_item(rt, a(0), a(1), a(2));
                V::Void
            }
        },
        GAME_MODE => {
            let mode = rt.get_field(a(0), "$type");
            let player = rt.get_field(a(0), "player");
            let i = if mode.r() != 0 {
                rt.get_field(mode, "$ordinal").i()
            } else if crate::fake_players::is_fake(rt, player) {
                0
            } else {
                rt.host().is_some_and(|h| h.player_creative()) as i32
            };
            match name {
                "getGameModeForPlayer" => game_type(rt, i),
                "getPreviousGameModeForPlayer" => V::NULL,
                "isCreative" => V::I((i == 1) as i32),
                _ => V::I(matches!(i, 0 | 2) as i32),
            }
        }
        BLOCK_DROPS if name == "<init>" && desc == DROPS_INIT => {
            block_drops_event(rt, args);
            V::Void
        }
        BREAK_SPEED | HARVEST_CHECK | DESTROY_ITEM | BLOCK_DROPS | BLOCK_EVENT | PLAYER_EVENT | LIVING_EVENT | ENTITY_EVENT | EVENT | CANCELLABLE => {
            return event_call(rt, owner, name, args);
        }
        o if api_common::is_player(o) => match name {
            "getDigSpeed" => V::F(dig_speed(rt, a(0), a(1), a(2))),
            // The deprecated one is vanilla's own test; the other NeoForge's, with `HarvestCheck`.
            "hasCorrectToolForDrops" if desc == CORRECT_TOOL => V::I(vanilla_harvest(rt, a(0), a(1)) as i32),
            "hasCorrectToolForDrops" => V::I(harvest_check(rt, a(0), a(1), a(2), a(3)) as i32),
            "blockActionRestricted" => V::I(action_restricted(rt, a(0), a(2), a(3)) as i32),
            _ => game_mode_of(rt, a(0)),
        },
        o if api::is_level(o) => {
            let p = api::pos_of(rt, a(2));
            if let Some(h) = rt.host_mut() {
                h.destroy_block_progress(a(1).i(), p, a(3).i());
            }
            V::Void
        }
        o if item_class(o) => {
            // Vanilla's own, a mod item's `super.` call among them.
            let stack = a(1);
            match name {
                "getDestroySpeed" => V::F(tool(rt, stack, a(2)).map_or(1.0, |t| t.speed)),
                "isCorrectToolForDrops" => V::I(tool(rt, stack, a(2)).is_some_and(|t| t.correct) as i32),
                "mineBlock" => V::I(mine_block(rt, stack, a(2), a(3), a(5)) as i32),
                _ => V::I(vanilla_can_attack(rt, a(0), a(4)) as i32),
            }
        }
        // A block's own (a mod block's `super.` call): vanilla's, never the override again.
        _ => match name {
            "getDestroyProgress" => V::F(destroy_progress(rt, a(1), a(2), a(3), a(4))),
            "canHarvestBlock" => V::I(harvest_check(rt, a(4), a(1), a(2), a(3)) as i32),
            "onDestroyedByPlayer" => destroyed_by_player(rt, a(2), a(3), a(6)),
            "getSoundType" if desc.starts_with("(Lnet/minecraft/world/level/block/state/BlockState;)") => {
                let text = text_of(rt, a(1));
                own_sound_type(rt, &text)
            }
            // `IBlockExtension.getSoundType(state, level, pos, entity)`: the state's.
            "getSoundType" => {
                let text = text_of(rt, a(1));
                sound_type(rt, a(1), &text)
            }
            // `getDrops`, the static ones: by hand without a tool.
            "getDrops" => {
                let tool = if desc == DROPS_BY { a(5) } else { V::NULL };
                drops(rt, a(0), a(2), tool)
            }
            // NeoForge's `getExpDrop`, a mod block's `super.` call: an ore's range, else none.
            "getExpDrop" => V::I(own_exp_drop(rt, a(0), a(1), a(2))),
            // `popExperience(level, pos, amount)`: orbs, on the server with `doTileDrops` on.
            "popExperience" => {
                if api::call(rt, IS_CLIENT, &[a(1)]).i() == 0 && crate::gamerules::blocks_drop(rt) {
                    let p = api::pos_of(rt, a(2));
                    if let Some(h) = rt.host_mut() {
                        h.pop_experience(p, a(3).i());
                    }
                }
                V::Void
            }
            // `onRemove(state, level, pos, newState, moved)`, `BlockBehaviour`'s.
            "onRemove" => {
                on_remove(rt, a(1), a(2), a(3), a(4));
                V::Void
            }
            // The static `dropResources`: the 3-argument one has no block entity, none an entity
            // or a tool but the 6-argument one.
            "dropResources" => {
                let be = if desc == DROP_RESOURCES[0] { V::NULL } else { a(3) };
                let (entity, tool) = if desc == DROP_RESOURCES[2] { (a(4), a(5)) } else { (V::NULL, V::NULL) };
                drop_resources(rt, a(0), a(1), a(2), be, entity, tool);
                V::Void
            }
            // `IceBlock.meltsInto()`: water.
            "meltsInto" => {
                let water = api::block(rt, "minecraft:water");
                api::call(rt, DEFAULT_STATE, &[water])
            }
            // `destroy` and `spawnAfterBreak` do nothing in `Block` and `BlockBehaviour`.
            _ => V::Void,
        },
    })
}

/// The block state `v` names (`minecraft:stone`), air for none.
fn text_of(rt: &Rt, v: V) -> String {
    rt.str_of(v).unwrap_or("minecraft:air").to_owned()
}

/// `BlockBehaviour.onRemove(state, level, pos, newState, moved)`: a block entity goes with its
/// block (`removeBlockEntity`), not when only the state's properties changed. Vanilla's subclasses'
/// own parts (a button's neighbours, a chest's contents) are the game's, which runs them for a mod
/// block of that template.
pub(crate) fn on_remove(rt: &mut Rt, st: V, level: V, pos: V, new: V) {
    let (old, new) = (text_of(rt, st), text_of(rt, new));
    if crate::host::split_state(&old).0 == crate::host::split_state(&new).0 {
        return;
    }
    if api::call(rt, HAS_BLOCK_ENTITY, &[st]).i() != 0 {
        api::call(rt, REMOVE_BLOCK_ENTITY, &[level, pos]);
    }
}

/// NeoForge's `Block.dropResources(state, level, pos, blockEntity, entity, tool)`: on the server, the
/// block's drops (`getDrops`), each popped about the block as an item entity (`popResource`, only
/// with `doTileDrops`), then `CommonHooks.handleBlockDrops`: a `BlockDropsEvent` with them, which
/// listeners may change or cancel; then the item entities join the level, the state's
/// `spawnAfterBreak` runs without its experience and the event's experience pops out.
fn drop_resources(rt: &mut Rt, st: V, level: V, pos: V, be: V, entity: V, tool: V) {
    if api::call(rt, IS_CLIENT, &[level]).i() != 0 {
        return;
    }
    let tool = if tool.r() == 0 { api_common::stack(rt, "minecraft:air", 0) } else { tool };
    let drops = api::call(rt, DROPS_BY_KEY, &[st, level, pos, be, entity, tool]);
    let p = api::pos_of(rt, pos);
    let mut popped = Vec::new();
    for stack in crate::jutil::elements(rt, drops) {
        // `popResource` draws its offsets before it checks the rule.
        if let Some(e) = crate::entities::popped(rt, p, stack)
            && crate::gamerules::blocks_drop(rt)
        {
            popped.push(e);
        }
    }
    let list = crate::jutil::new_list(rt, popped);
    let ev = rt.new_object(BLOCK_DROPS);
    block_drops_event(rt, &[ev, level, pos, st, be, list, entity, tool]);
    crate::neo::post(rt, BLOCK_DROPS, ev);
    if rt.get_field(ev, "$canceled").i() != 0 {
        return;
    }
    let left = rt.get_field(ev, "$drops");
    for e in crate::jutil::elements(rt, left) {
        crate::entities::add_item_entity(rt, e);
    }
    api::call(rt, STATE_AFTER_BREAK_KEY, &[st, level, pos, tool, V::I(0)]);
    let xp = rt.get_field(ev, "$experience").i();
    if xp > 0 {
        let b = api::call(rt, GET_BLOCK, &[st]);
        api::call(rt, POP_EXPERIENCE_KEY, &[b, level, pos, V::I(xp)]);
    }
}

/// `Level.destroyBlock(pos, drop, entity)`: nothing for air; else the break's effect (level event
/// 2001, not for fire), the block's drops when asked (`Block.dropResources` with its block entity
/// and `entity`, no tool), then its fluid's block in its place. Whether the block changed.
pub(crate) fn destroy_block(rt: &mut Rt, level: V, pos: V, drop: bool, entity: V) -> bool {
    let p = api::pos_of(rt, pos);
    let here = rt.host().map_or_else(|| "minecraft:air".to_owned(), |h| h.block_state(p));
    let (block, _) = crate::host::split_state(&here);
    if matches!(block, "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air") {
        return false;
    }
    let st = api::state(rt, &here);
    if !matches!(block, "minecraft:fire" | "minecraft:soul_fire") {
        let id = api::call(rt, BLOCK_ID, &[st]);
        api::call(rt, LEVEL_EVENT, &[level, V::I(2001), pos, id]);
    }
    if drop {
        let be = if api::call(rt, HAS_BLOCK_ENTITY, &[st]).i() != 0 { api::call(rt, BLOCK_ENTITY_AT, &[level, pos]) } else { V::NULL };
        drop_resources(rt, st, level, pos, be, entity, V::NULL);
    }
    let left = fluid_block(rt, &here);
    let set = rt.host_mut().is_some_and(|h| h.set_block_state(p, &left));
    crate::glue::drain_replaced(rt);
    set
}

/// What a block removed leaves: its fluid's block (a waterlogged one leaves water), as vanilla's
/// `fluidState.createLegacyBlock()`.
pub(crate) fn fluid_block(rt: &mut Rt, here: &str) -> String {
    let fluid = crate::fluids::of_block_state(rt, here);
    let left = api::call(rt, LEGACY_BLOCK, &[fluid]);
    rt.str_of(left).unwrap_or("minecraft:air").to_owned()
}

/// `BlockState.liquid()` of state `text`: water and lava, and a mod's fluid's block (a
/// `LiquidBlock`), as vanilla's `Properties.liquid()` marks them.
fn liquid(rt: &Rt, text: &str) -> bool {
    let (block, _) = crate::host::split_state(text);
    matches!(block, "minecraft:water" | "minecraft:lava")
        || mod_block_object(rt, text).is_some_and(|b| rt.is_a(rt.class_of(b), "net/minecraft/world/level/block/LiquidBlock"))
}

/// NeoForge's `getExpDrop(level, pos, blockEntity, breaker, tool)` of state `st` (`text`): a mod
/// block's own; else a mod block made as an ore draws from its range (vanilla's
/// `DropExperienceBlock`), and a vanilla block's experience is what the game draws for it. Before
/// enchantments: `EnchantmentHelper.processBlockExperience` takes silk touch's away.
fn exp_drop(rt: &mut Rt, st: V, text: &str, args: [V; 5]) -> i32 {
    let mut all = vec![st];
    all.extend_from_slice(&args);
    if let Some(v) = block_override(rt, text, EXP_KEY, EXP, &all) {
        return v.i();
    }
    if let Some(b) = mod_block_object(rt, text) {
        return own_exp_drop(rt, b, st, args[0]);
    }
    rt.host_mut().map_or(0, |h| h.block_experience(text))
}

/// `Block.getExpDrop` as vanilla's own classes answer for block `b` in state `st`: an ore's range
/// (`DropExperienceBlock.xpRange`) drawn from `level`'s random, else none; a vanilla block's what
/// the game draws.
fn own_exp_drop(rt: &mut Rt, b: V, st: V, level: V) -> i32 {
    let range = rt.get_field(b, "xpRange");
    if range.r() != 0 {
        let level = if level.r() != 0 { level } else { crate::glue::level(rt) };
        let random = api::call(rt, GET_RANDOM, &[level]);
        return api::call(rt, SAMPLE, &[range, random]).i();
    }
    if rt.str_of(b).is_some() {
        let text = text_of(rt, st);
        return rt.host_mut().map_or(0, |h| h.block_experience(&text));
    }
    0
}

/// `BlockState.isSolid()` (vanilla's `legacySolid`) of state `text`: as the game has the block;
/// a mod block the game doesn't know is solid when its `Properties` force it (`forceSolidOn`).
fn solid(rt: &Rt, text: &str) -> bool {
    if let Some(s) = rt.host().and_then(|h| h.block_solid(text)) {
        return s;
    }
    mod_block_object(rt, text).is_some_and(|b| {
        let props = rt.get_field(b, "$properties");
        props.r() != 0 && rt.get_field(props, "$forceSolidOn").i() == 1
    })
}

/// A call on `this` as Java dispatches it: a mod object's own override (a mod's player class), else
/// vanilla's.
fn virt(rt: &mut Rt, key: &'static str, this: V, args: &[V]) -> V {
    if rt.vcall.is_some() {
        return crate::menus::virt(rt, key, this, args);
    }
    let mut all = vec![this];
    all.extend_from_slice(args);
    api::call(rt, key, &all)
}

/// The mod block object registered for state `text`'s block (one made of its `Properties`).
fn mod_block_object(rt: &Rt, text: &str) -> Option<V> {
    let (name, _) = crate::host::split_state(text);
    rt.registered.get(&("block".to_owned(), name.to_owned())).copied().filter(|b| rt.str_of(*b).is_none())
}

/// Mod block `text`'s own `nd` (its class's, or a mod superclass's), `key` being vanilla's, on
/// `args` after the block: None without one, or when it threw (told; vanilla's answers then).
fn block_override(rt: &mut Rt, text: &str, key: &'static str, nd: &'static str, args: &[V]) -> Option<V> {
    let (blk, class) = crate::glue::mod_block(rt, text)?;
    if !rt.overrides.is_some_and(|o| o(class, nd)) {
        return None;
    }
    let vc = rt.vcall?;
    let mut all = vec![blk];
    all.extend_from_slice(args);
    let r = vc(rt, key, nd, &all);
    if rt.thrown.is_none() {
        return Some(r);
    }
    rt.report_thrown(nd);
    None
}

/// The mod item in `stack`'s own `nd`, `key` being vanilla's, on `args` after the item: None
/// without one, or when it threw.
fn item_override(rt: &mut Rt, stack: V, key: &'static str, nd: &'static str, args: &[V]) -> Option<V> {
    let item = rt.get_field(stack, "$item");
    let class = rt.class_of(item);
    if item.r() == 0 || !rt.is_mod_class(class) || !rt.overrides.is_some_and(|o| o(class, nd)) {
        return None;
    }
    let vc = rt.vcall?;
    let mut all = vec![item];
    all.extend_from_slice(args);
    let r = vc(rt, key, nd, &all);
    if rt.thrown.is_none() {
        return Some(r);
    }
    rt.report_thrown(nd);
    None
}

/// What `stack` holds as the game takes a tool: None for an empty one.
fn held(rt: &mut Rt, stack: V) -> Option<crate::host::HandStack> {
    if stack.r() == 0 || api_common::stack_empty(rt, stack) {
        return None;
    }
    crate::hand::hand_stack_of(rt, stack)
}

/// What the tool in `stack` does to state `st`, as the game knows its item: None for no tool (an
/// empty hand, an item without a `minecraft:tool` component).
fn tool(rt: &mut Rt, stack: V, st: V) -> Option<crate::host::ToolFacts> {
    if stack.r() == 0 || api_common::stack_empty(rt, stack) {
        return None;
    }
    let item = api_common::item_like_name(rt, rt.get_field(stack, "$item"));
    let text = text_of(rt, st);
    rt.host().and_then(|h| h.item_tool(&item, &text))
}

/// `ItemStack.getDestroySpeed(state)`: its item's (`Tool.getMiningSpeed`), 1 without a tool.
fn destroy_speed(rt: &mut Rt, stack: V, st: V) -> f32 {
    match item_override(rt, stack, ITEM_SPEED_KEY, ITEM_SPEED, &[stack, st]) {
        Some(v) => v.f(),
        None => tool(rt, stack, st).map_or(1.0, |t| t.speed),
    }
}

/// `ItemStack.isCorrectToolForDrops(state)`: its item's (`Tool.isCorrectForDrops`).
fn correct_tool(rt: &mut Rt, stack: V, st: V) -> bool {
    match item_override(rt, stack, ITEM_CORRECT_KEY, ITEM_CORRECT, &[stack, st]) {
        Some(v) => v.i() != 0,
        None => tool(rt, stack, st).is_some_and(|t| t.correct),
    }
}

/// `BlockState.requiresCorrectToolForDrops()`: what a mod block's `Properties` say, else the game.
fn requires_tool(rt: &mut Rt, text: &str) -> bool {
    match mod_block_object(rt, text) {
        Some(b) => crate::blocks::properties_require_tool(rt, b),
        None => rt.host().is_some_and(|h| h.block_requires_tool(text)),
    }
}

/// `Player.hasCorrectToolForDrops(state)`, the deprecated one: a block that doesn't need the right
/// tool, or the stack in hand is it.
fn vanilla_harvest(rt: &mut Rt, p: V, st: V) -> bool {
    let text = text_of(rt, st);
    if !requires_tool(rt, &text) {
        return true;
    }
    let stack = virt(rt, MAIN_HAND, p, &[]);
    correct_tool(rt, stack, st)
}

/// `EventHooks.doPlayerHarvestCheck(player, state, level, pos)`: vanilla's answer, as a
/// `PlayerEvent.HarvestCheck` listener leaves it.
fn harvest_check(rt: &mut Rt, p: V, st: V, level: V, pos: V) -> bool {
    let vanilla = vanilla_harvest(rt, p, st);
    if !listened(rt, HARVEST_CHECK) {
        return vanilla;
    }
    let ev = rt.new_object(HARVEST_CHECK);
    put_all(rt, ev, &[("$player", p), ("$state", st), ("$level", level), ("$pos", pos), ("$success", V::I(vanilla as i32))]);
    crate::neo::post(rt, HARVEST_CHECK, ev);
    rt.get_field(ev, "$success").i() != 0
}

/// `BlockBehaviour.getDestroyProgress(state, player, level, pos)`: what of the block one tick of
/// the player's breaking takes, 1 being all of it; none for a block nothing breaks.
fn destroy_progress(rt: &mut Rt, st: V, p: V, level: V, pos: V) -> f32 {
    let text = text_of(rt, st);
    let hardness = crate::blocks::state_facts(rt, &text).hardness;
    if hardness == -1.0 {
        return 0.0;
    }
    let i = if harvest_check(rt, p, st, level, pos) { 30.0 } else { 100.0 };
    virt(rt, DIG_SPEED_KEY, p, &[st, pos]).f() / hardness / i
}

/// `Player.getDigSpeed(state, pos)`, vanilla's formula with NeoForge's `BreakSpeed` at its end.
fn dig_speed(rt: &mut Rt, p: V, st: V, pos: V) -> f32 {
    let stack = virt(rt, MAIN_HAND, p, &[]);
    let mut f = destroy_speed(rt, stack, st);
    if f > 1.0 {
        f += attribute(rt, p, "minecraft:player.mining_efficiency") as f32;
    }
    // `MobEffectUtil.hasDigSpeed` and `getDigSpeedAmplification`.
    let haste = amplifier(rt, p, "minecraft:haste");
    let conduit = amplifier(rt, p, "minecraft:conduit_power");
    if haste.is_some() || conduit.is_some() {
        let amp = haste.unwrap_or(0).max(conduit.unwrap_or(0));
        f *= 1.0 + (amp + 1) as f32 * 0.2;
    }
    if let Some(fatigue) = amplifier(rt, p, "minecraft:mining_fatigue") {
        f *= match fatigue {
            0 => 0.3,
            1 => 0.09,
            2 => 0.0027,
            _ => 8.1E-4,
        };
    }
    f *= attribute(rt, p, "minecraft:player.block_break_speed") as f32;
    if eyes_in_water(rt, p) {
        f *= attribute(rt, p, "minecraft:player.submerged_mining_speed") as f32;
    }
    if !on_ground(rt, p) {
        f /= 5.0;
    }
    break_speed(rt, p, st, f, pos)
}

/// The value of player `p`'s attribute `id`.
fn attribute(rt: &mut Rt, p: V, id: &str) -> f64 {
    let holder = crate::attributes::attribute(rt, id);
    virt(rt, ATTRIBUTE_VALUE, p, &[holder]).d()
}

/// The amplifier of player `p`'s effect `id`, None without it.
fn amplifier(rt: &mut Rt, p: V, id: &str) -> Option<i32> {
    let holder = crate::food::effect_holder(rt, id);
    if virt(rt, HAS_EFFECT, p, &[holder]).i() == 0 {
        return None;
    }
    let effect = virt(rt, GET_EFFECT, p, &[holder]);
    Some(api::call(rt, AMPLIFIER, &[effect]).i())
}

/// Whether player `p` stands on the ground. A fake player never ticks, so never lands (Create's
/// deployer breaks a fifth as fast as a standing player); the game keeps a real one's footing to
/// itself, so one stands unless its object says otherwise.
fn on_ground(rt: &Rt, p: V) -> bool {
    if crate::fake_players::is_fake(rt, p) {
        return false;
    }
    !matches!(rt.get_field(p, "$onGround"), V::I(0))
}

/// Whether player `p`'s eyes are in water (`isEyeInFluid(FluidTags.WATER)`), as `Entity`'s tick
/// finds it: the water at its eyes (less a ninth of a block) reaches above them. A fake player never
/// ticks, so never finds any.
fn eyes_in_water(rt: &mut Rt, p: V) -> bool {
    if crate::fake_players::is_fake(rt, p) {
        return false;
    }
    let eye = virt(rt, EYE_Y, p, &[]).d() - f64::from(0.111_111_11_f32);
    let (x, z) = (virt(rt, GET_X, p, &[]).d(), virt(rt, GET_Z, p, &[]).d());
    let at = api::block_pos(rt, x.floor() as i64, eye.floor() as i64, z.floor() as i64);
    let level = crate::glue::level(rt);
    let fluid = api::call(rt, FLUID_AT, &[level, at]);
    let Some(water) = api_common::get_static(rt, "net/minecraft/tags/FluidTags", "WATER") else { return false };
    if api::call(rt, FLUID_IS, &[fluid, water]).i() == 0 {
        return false;
    }
    let height = api::call(rt, FLUID_HEIGHT, &[fluid, level, at]).f();
    f64::from(eye.floor() as f32 + height) > eye
}

/// `EventHooks.getBreakSpeed(player, state, original, pos)`: a `PlayerEvent.BreakSpeed`
/// listener's speed, -1 when one cancels it.
fn break_speed(rt: &mut Rt, p: V, st: V, original: f32, pos: V) -> f32 {
    if !listened(rt, BREAK_SPEED) {
        return original;
    }
    let ev = rt.new_object(BREAK_SPEED);
    let at = api_common::optional(rt, pos);
    put_all(rt, ev, &[("$player", p), ("$state", st), ("$original", V::F(original)), ("$newSpeed", V::F(original)), ("$position", at), ("$canceled", V::I(0))]);
    crate::neo::post(rt, BREAK_SPEED, ev);
    if rt.get_field(ev, "$canceled").i() != 0 { -1.0 } else { rt.get_field(ev, "$newSpeed").f() }
}

/// `EventHooks.onPlayerDestroyItem(player, stack, hand)`: `PlayerDestroyItemEvent`, the stack worn
/// out in the hand (null: another way).
fn destroyed_item(rt: &mut Rt, p: V, stack: V, hand: V) {
    if !listened(rt, DESTROY_ITEM) {
        return;
    }
    let ev = rt.new_object(DESTROY_ITEM);
    put_all(rt, ev, &[("$player", p), ("$original", stack), ("$hand", hand)]);
    crate::neo::post(rt, DESTROY_ITEM, ev);
}

/// `CommonHooks.fireBlockBreak(level, gameType, player, pos, state)`: the `BlockEvent.BreakEvent`
/// of the player breaking the block, cancelled beforehand when the item in hand can't attack blocks
/// (a sword in creative), the player's game mode keeps it from breaking there, or the block is a
/// game master block the player may not use. A cancelled one reaches no listener (none here hears a
/// cancelled event); the caller keeps the block.
fn fire_block_break(rt: &mut Rt, [level, game_type, p, pos, st]: [V; 5]) -> V {
    let text = text_of(rt, st);
    let stack = virt(rt, MAIN_HAND, p, &[]);
    let mut cancel = !api_common::stack_empty(rt, stack) && !can_attack_block(rt, stack, st, level, pos, p);
    cancel |= virt(rt, RESTRICTED_KEY, p, &[level, pos, game_type]).i() != 0;
    cancel |= game_master_block(rt, &text) && !may_use_game_master_blocks(rt, p);
    let ev = rt.new_object(BREAK);
    api::call(rt, BREAK_INIT, &[ev, level, pos, st, p]);
    rt.put_field(ev, "$canceled", V::I(cancel as i32));
    if !cancel {
        crate::neo::post(rt, BREAK, ev);
    }
    ev
}

/// `item.canAttackBlock(state, level, pos, player)` for the stack in hand: a mod item's own, else
/// vanilla's.
fn can_attack_block(rt: &mut Rt, stack: V, st: V, level: V, pos: V, p: V) -> bool {
    match item_override(rt, stack, ITEM_ATTACK_KEY, ITEM_ATTACK, &[st, level, pos, p]) {
        Some(v) => v.i() != 0,
        None => {
            let item = rt.get_field(stack, "$item");
            vanilla_can_attack(rt, item, p)
        }
    }
}

/// Vanilla's `canAttackBlock`: a sword, a trident or a mace not in creative; never the debug stick
/// (its own use); anything else.
fn vanilla_can_attack(rt: &mut Rt, item: V, p: V) -> bool {
    let weapon = ["SwordItem", "TridentItem", "MaceItem"].iter().any(|c| rt.instance_of(item, &format!("net/minecraft/world/item/{c}")));
    let name = api_common::item_like_name(rt, item);
    if name == "minecraft:debug_stick" || rt.instance_of(item, "net/minecraft/world/item/DebugStickItem") {
        return false;
    }
    let vanilla_weapon = name.starts_with("minecraft:") && (name.ends_with("_sword") || matches!(name.as_str(), "minecraft:trident" | "minecraft:mace"));
    if weapon || vanilla_weapon {
        return virt(rt, IS_CREATIVE, p, &[]).i() == 0;
    }
    true
}

/// Whether state `text`'s block is a `GameMasterBlock` (a command block, a structure block, a
/// jigsaw; a mod's that is one).
fn game_master_block(rt: &Rt, text: &str) -> bool {
    match mod_block_object(rt, text) {
        Some(b) => rt.instance_of(b, "net/minecraft/world/level/block/GameMasterBlock"),
        None => GAME_MASTER_BLOCKS.contains(&crate::host::split_state(text).0),
    }
}

/// `Player.canUseGameMasterBlocks()`: never a fake player; the game's player in creative.
fn may_use_game_master_blocks(rt: &Rt, p: V) -> bool {
    !crate::fake_players::is_fake(rt, p) && rt.host().is_some_and(|h| h.player_creative())
}

/// `Player.blockActionRestricted(level, pos, gameMode)`: a spectator breaks nothing; an adventure
/// player that may not build breaks only with an item that says it may (its `can_break`, whose
/// block predicates aren't read here: none may).
fn action_restricted(rt: &mut Rt, p: V, _pos: V, game_type: V) -> bool {
    match rt.get_field(game_type, "$ordinal").i() {
        3 => true,
        2 => virt(rt, MAY_BUILD, p, &[]).i() == 0,
        _ => false,
    }
}

/// `GameType` constant `i` (`SURVIVAL`...).
fn game_type(rt: &mut Rt, i: i32) -> V {
    let i = i.clamp(0, 3);
    api::api_enum(rt, GAME_TYPE, GAME_TYPES[i as usize], i)
}

/// A player's `gameMode` (`ServerPlayerGameMode`): one per player, kept on it. A fake player plays
/// in survival (`GameType.DEFAULT_MODE`, as its `ServerPlayer` constructor leaves it); the game's
/// player in the mode it has at each call.
fn game_mode_of(rt: &mut Rt, p: V) -> V {
    let own = rt.get_field(p, "gameMode");
    if own.r() != 0 {
        return own;
    }
    let mode = rt.new_object(GAME_MODE);
    rt.put_field(mode, "player", p);
    if crate::fake_players::is_fake(rt, p) {
        let survival = game_type(rt, 0);
        rt.put_field(mode, "$type", survival);
    }
    rt.put_field(p, "gameMode", mode);
    mode
}

/// `Item.mineBlock(stack, level, state, pos, miner)` of vanilla's items: a tool wears by its
/// `damagePerBlock` on a block that doesn't break at once (on the server), shears by one on any but
/// fire. Whether it was a tool's (or shears') mining.
fn mine_block(rt: &mut Rt, stack: V, level: V, st: V, miner: V) -> bool {
    if stack.r() == 0 || api_common::stack_empty(rt, stack) {
        return false;
    }
    let text = text_of(rt, st);
    let client = api::call(rt, IS_CLIENT, &[level]).i() != 0;
    let item = rt.get_field(stack, "$item");
    let shears = api_common::item_like_name(rt, item) == "minecraft:shears" || rt.instance_of(item, "net/minecraft/world/item/ShearsItem");
    let wear = if shears {
        let fire = rt.host().is_some_and(|h| h.block_in_tag(crate::host::split_state(&text).0, "minecraft:fire"));
        (!fire).then_some(1)
    } else {
        let Some(t) = tool(rt, stack, st) else { return false };
        (crate::blocks::state_facts(rt, &text).hardness != 0.0 && t.damage_per_block > 0).then_some(t.damage_per_block)
    };
    if let Some(n) = wear
        && !client
    {
        let slot = api_common::get_static(rt, "net/minecraft/world/entity/EquipmentSlot", "MAINHAND").unwrap_or(V::NULL);
        api::call(rt, HURT, &[stack, V::I(n), miner, slot]);
    }
    true
}

/// NeoForge's `IBlockExtension.onDestroyedByPlayer`: on the server, `level.removeBlock(pos, false)`;
/// on a client, the fluid left there set in the block's place. Whether the block went.
fn destroyed_by_player(rt: &mut Rt, level: V, pos: V, fluid: V) -> V {
    if api::call(rt, IS_CLIENT, &[level]).i() != 0 {
        let left = api::call(rt, LEGACY_BLOCK, &[fluid]);
        return api::call(rt, SET_BLOCK, &[level, pos, left, V::I(11)]);
    }
    api::call(rt, REMOVE_BLOCK, &[level, pos, V::I(0)])
}

/// The static `Block.getDrops(state, level, pos, blockEntity, entity, tool)`: the block's loot,
/// rolled by the game with `tool` (its enchantments, a block entity's contents kept for the break),
/// as stacks a mod may put where it likes.
fn drops(rt: &mut Rt, st: V, pos: V, tool: V) -> V {
    let text = text_of(rt, st);
    let p = api::pos_of(rt, pos);
    let tool = held(rt, tool);
    let got = rt.host_mut().map(|h| h.block_drops(p, &text, tool.as_ref())).unwrap_or_default();
    let stacks: Vec<V> = got.into_iter().map(|s| crate::hand::load(rt, Some(s))).collect();
    crate::jutil::new_list(rt, stacks)
}

/// `BlockState.getSoundType()`: the mod block's own `getSoundType(state)`, else its own.
fn sound_type(rt: &mut Rt, st: V, text: &str) -> V {
    match block_override(rt, text, SOUND_KEY, SOUND, &[st]) {
        Some(v) => v,
        None => own_sound_type(rt, text),
    }
}

/// `BlockBehaviour.getSoundType(state)`: what a mod block's `Properties` say; a vanilla block's
/// sounds as the game has them, as vanilla's own constant (`SoundType.STONE`).
fn own_sound_type(rt: &mut Rt, text: &str) -> V {
    if let Some(b) = mod_block_object(rt, text) {
        return crate::blocks::properties_sound_type(rt, b);
    }
    match rt.host().and_then(|h| h.block_sounds(text)) {
        Some(s) => crate::blocks::sound_type_of(rt, &s),
        None => api::get_static(rt, "net/minecraft/world/level/block/SoundType.STONE"),
    }
}

fn put_all(rt: &mut Rt, o: V, fields: &[(&'static str, V)]) {
    for &(f, v) in fields {
        rt.put_field(o, f, v);
    }
}

fn know_classes(rt: &mut Rt) {
    if rt.supers.get(BREAK_SPEED) != Some(&PLAYER_EVENT) || rt.supers.get(PLAYER_EVENT) != Some(&LIVING_EVENT) || rt.supers.get(BLOCK_DROPS) != Some(&BLOCK_EVENT) {
        for (class, sup) in SUPERS {
            rt.supers.insert(class, sup);
        }
    }
}

/// Whether any listener would hear an event of `class`.
fn listened(rt: &mut Rt, class: &str) -> bool {
    know_classes(rt);
    !crate::neo::hearing(rt, class).is_empty()
}

/// The events' own calls (`event.getNewSpeed()`, `PlayerEvent.getEntity` on one), and a mod
/// making one; None for another event.
fn event_call(rt: &mut Rt, owner: &str, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let ev = a(0);
    if rt.class_of(ev) == BLOCK_DROPS || owner == BLOCK_DROPS {
        return drops_event_call(rt, name, args);
    }
    if name == "<init>" {
        know_classes(rt);
        let fields = match owner {
            BREAK_SPEED => vec![("$player", a(1)), ("$state", a(2)), ("$original", V::F(a(3).f())), ("$newSpeed", V::F(a(3).f())), ("$canceled", V::I(0))],
            HARVEST_CHECK => vec![("$player", a(1)), ("$state", a(2)), ("$level", a(3)), ("$pos", a(4)), ("$success", V::I(a(5).i()))],
            DESTROY_ITEM => vec![("$player", a(1)), ("$original", a(2)), ("$hand", a(3))],
            _ => return None,
        };
        put_all(rt, ev, &fields);
        if owner == BREAK_SPEED {
            let at = api_common::optional(rt, a(4));
            rt.put_field(ev, "$position", at);
        }
        return Some(V::Void);
    }
    let class = rt.class_of(ev);
    if !matches!(class, BREAK_SPEED | HARVEST_CHECK | DESTROY_ITEM) {
        return None;
    }
    Some(match name {
        "getEntity" => rt.get_field(ev, "$player"),
        "getState" | "getTargetBlock" => rt.get_field(ev, "$state"),
        "getOriginalSpeed" => V::F(rt.get_field(ev, "$original").f()),
        "getNewSpeed" => V::F(rt.get_field(ev, "$newSpeed").f()),
        "setNewSpeed" => {
            rt.put_field(ev, "$newSpeed", V::F(a(1).f()));
            V::Void
        }
        "getPosition" => rt.get_field(ev, "$position"),
        "getLevel" => rt.get_field(ev, "$level"),
        "getPos" => rt.get_field(ev, "$pos"),
        "canHarvest" => V::I(rt.get_field(ev, "$success").i()),
        "setCanHarvest" => {
            rt.put_field(ev, "$success", V::I(a(1).i()));
            V::Void
        }
        "getOriginal" => rt.get_field(ev, "$original"),
        "getHand" => rt.get_field(ev, "$hand"),
        // Only `BreakSpeed` may be cancelled.
        "isCanceled" => V::I(rt.get_field(ev, "$canceled").i()),
        "setCanceled" if class == BREAK_SPEED => {
            rt.put_field(ev, "$canceled", V::I(a(1).i()));
            V::Void
        }
        _ => return None,
    })
}

/// `new BlockDropsEvent(level, pos, state, blockEntity, drops, breaker, tool)`: what NeoForge's
/// constructor keeps, and the experience the break drops (`state.getExpDrop(...)` after the tool's
/// enchantments), which a listener may change.
fn block_drops_event(rt: &mut Rt, args: &[V]) {
    know_classes(rt);
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let ev = a(0);
    put_all(rt, ev, &[("$level", a(1)), ("$pos", a(2)), ("$state", a(3)), ("$blockEntity", a(4)), ("$drops", a(5)), ("$breaker", a(6)), ("$tool", a(7)), ("$canceled", V::I(0))]);
    let experience = api::call(rt, STATE_EXP_KEY, &[a(3), a(1), a(2), a(4), a(6), a(7)]).i();
    let experience = api::call(rt, PROCESS_EXPERIENCE, &[a(1), a(7), V::I(experience)]);
    rt.put_field(ev, "$experience", V::I(experience.i()));
}

/// A `BlockDropsEvent`'s own calls, and `BlockEvent`'s and `Event`'s on one.
fn drops_event_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let ev = a(0);
    let field = match name {
        "getLevel" => "$level",
        "getPos" => "$pos",
        "getState" => "$state",
        "getDrops" => "$drops",
        "getBlockEntity" => "$blockEntity",
        "getBreaker" => "$breaker",
        "getTool" => "$tool",
        "getDroppedExperience" => "$experience",
        "isCanceled" => "$canceled",
        "setCanceled" => {
            rt.put_field(ev, "$canceled", V::I((a(1).i() != 0) as i32));
            return Some(V::Void);
        }
        // `Preconditions.checkArgument`: no negative experience.
        "setDroppedExperience" if a(1).i() < 0 => {
            crate::jlang::throw_with(rt, "java/lang/IllegalArgumentException", "May not set a negative experience drop.");
            return Some(V::Void);
        }
        "setDroppedExperience" => {
            rt.put_field(ev, "$experience", V::I(a(1).i()));
            return Some(V::Void);
        }
        _ => return None,
    };
    Some(match rt.get_field(ev, field) {
        V::Void if matches!(field, "$experience" | "$canceled") => V::I(0),
        V::Void => V::NULL,
        v => v,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{BlockFacts, BlockSounds, MemoryHost, ToolFacts};

    const BLOCK: &str = "net/minecraft/world/level/block/Block";
    const FAKE_INIT: &str = "net/neoforged/neoforge/common/util/FakePlayer.<init>.(Lnet/minecraft/server/level/ServerLevel;Lcom/mojang/authlib/GameProfile;)V";
    const PROFILE_INIT: &str = "com/mojang/authlib/GameProfile.<init>.(Ljava/util/UUID;Ljava/lang/String;)V";
    const SET_IN_HAND: &str = "net/minecraft/world/entity/LivingEntity.setItemInHand.(Lnet/minecraft/world/InteractionHand;Lnet/minecraft/world/item/ItemStack;)V";
    const GAME_MODE_FIELD: &str = "net/minecraft/server/level/ServerPlayer.$gameMode.()Lnet/minecraft/server/level/ServerPlayerGameMode;";
    const MODE_FOR_PLAYER: &str = "net/minecraft/server/level/ServerPlayerGameMode.getGameModeForPlayer.()Lnet/minecraft/world/level/GameType;";
    const GET_BLOCK_STATE: &str = "net/minecraft/server/level/ServerLevel.getBlockState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;";
    const DAMAGE: &str = "net/minecraft/world/item/ItemStack.getDamageValue.()I";
    const HIT_SOUND: &str = "net/minecraft/world/level/block/SoundType.getHitSound.()Lnet/minecraft/sounds/SoundEvent;";
    const STONE: &str = "minecraft:stone";
    const AT: [i32; 3] = [3, 64, -2];

    fn key(owner: &str, name: &str, desc: &str) -> &'static str {
        Box::leak(format!("{owner}.{name}.{desc}").into_boxed_str())
    }

    /// The listeners: 1 notes a `BreakEvent` (its state and whether its player is the fake one);
    /// 2 doubles a `BreakSpeed`; 3 says no to a `HarvestCheck`.
    fn lambdas(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
        let ev = args[1];
        match id {
            1 => {
                let state = api::call(rt, key(BREAK, "getState", "()Lnet/minecraft/world/level/block/state/BlockState;"), &[ev]);
                let player = api::call(rt, key(BREAK, "getPlayer", "()Lnet/minecraft/world/entity/player/Player;"), &[ev]);
                let line = format!("break {} by fake {}", rt.str_of(state).unwrap_or(""), crate::fake_players::is_fake(rt, player));
                let s = rt.string(&line);
                rt.interned.insert("test:heard".into(), s);
            }
            2 => {
                let original = api::call(rt, key(BREAK_SPEED, "getOriginalSpeed", "()F"), &[ev]).f();
                api::call(rt, key(BREAK_SPEED, "setNewSpeed", "(F)V"), &[ev, V::F(original * 2.0)]);
            }
            3 => {
                api::call(rt, key(HARVEST_CHECK, "setCanHarvest", "(Z)V"), &[ev, V::I(0)]);
            }
            4 => {
                let xp = api::call(rt, key(BLOCK_DROPS, "getDroppedExperience", "()I"), &[ev]).i();
                api::call(rt, key(BLOCK_DROPS, "setDroppedExperience", "(I)V"), &[ev, V::I(xp + 2)]);
            }
            _ => return None,
        }
        Some(V::Void)
    }

    fn runtime(listeners: &[(&str, u32)]) -> Rt {
        crate::install();
        let mut rt = Rt::new();
        rt.lambda_call = Some(lambdas);
        for &(class, id) in listeners {
            let l = rt.lambda_id("java/util/function/Consumer", id, vec![]);
            rt.listeners.push((class.to_owned(), l));
        }
        rt
    }

    /// The game of the test: stone at [`AT`] (breaks in 1.5, needs a pickaxe, drops cobblestone,
    /// sounds as stone), and an iron pickaxe (speed 6 on stone, right for its drops, wears by one).
    fn world() -> MemoryHost {
        let mut h = MemoryHost::default();
        h.blocks.insert(AT, STONE.into());
        h.facts.insert(STONE.into(), BlockFacts { hardness: 1.5, push: 0, replaceable: false });
        h.requires_tool.push(STONE.into());
        h.tools.insert(("minecraft:iron_pickaxe".into(), STONE.into()), ToolFacts { speed: 6.0, correct: true, damage_per_block: 1 });
        h.loot.insert(STONE.into(), vec![("minecraft:cobblestone".into(), 1)]);
        let events = ["break", "step", "place", "hit", "fall"].map(|e| format!("minecraft:block.stone.{e}"));
        h.block_sounds.insert(STONE.into(), BlockSounds { volume: 1.0, pitch: 1.0, events });
        h
    }

    /// A fake player holding `item` (none: an empty hand).
    fn fake(rt: &mut Rt, item: Option<&str>) -> (V, V) {
        let p = rt.new_object(crate::fake_players::FAKE_PLAYER);
        let profile = rt.new_object("com/mojang/authlib/GameProfile");
        let uuid = crate::tags::uuid(rt, 0x77, 1);
        let name = rt.string("Deployer");
        api::call(rt, PROFILE_INIT, &[profile, uuid, name]);
        let level = crate::glue::level(rt);
        api::call(rt, FAKE_INIT, &[p, level, profile]);
        let stack = match item {
            Some(i) => {
                let s = api_common::stack(rt, i, 1);
                rt.put_field(s, "$maxDamage", V::I(250));
                s
            }
            None => api_common::get_static(rt, ITEM_STACK, "EMPTY").unwrap(),
        };
        let main = api_common::get_static(rt, "net/minecraft/world/InteractionHand", "MAIN_HAND").unwrap();
        api::call(rt, SET_IN_HAND, &[p, main, stack]);
        (p, stack)
    }

    fn progress(rt: &mut Rt, p: V) -> f32 {
        let level = crate::glue::level(rt);
        let pos = api::block_pos(rt, AT[0].into(), AT[1].into(), AT[2].into());
        let st = api::state(rt, STONE);
        api::call(rt, key(STATE, "getDestroyProgress", STATE_PROGRESS), &[st, p, level, pos]).f()
    }

    /// Create's deployer punching stone with an iron pickaxe, call for call as
    /// `DeployerHandler.activateInner` and `tryHarvestBlock` make them: each punch takes a
    /// sixteenth-tick's worth of vanilla's progress (the fake player, never on the ground, breaks a
    /// fifth as fast as a standing one); the break is heard, wears the pickaxe by one, leaves air,
    /// and its cobblestone goes into the fake player's inventory; the cracks and the hit sound are
    /// stone's.
    #[test]
    fn a_fake_player_breaks_stone_with_a_pickaxe_as_the_deployer_does() {
        let mut rt = runtime(&[(BREAK, 1)]);
        let mut h = world();
        rt.with_host(&mut h, |rt| {
            let (p, pickaxe) = fake(rt, Some("minecraft:iron_pickaxe"));
            let level = crate::glue::level(rt);
            let pos = api::block_pos(rt, AT[0].into(), AT[1].into(), AT[2].into());
            // Vanilla's: speed 6, off the ground / 5, hardness 1.5, the right tool's 30.
            assert!((progress(rt, p) - 6.0 / 5.0 / 1.5 / 30.0).abs() < 1e-6, "{}", progress(rt, p));
            let st = api::call(rt, GET_BLOCK_STATE, &[level, pos]);
            let sound = api::call(rt, key(STATE, "getSoundType", STATE_SOUND), &[st]);
            assert_eq!(sound, api::get_static(rt, "net/minecraft/world/level/block/SoundType.STONE"));
            let hit = api::call(rt, HIT_SOUND, &[sound]);
            assert_eq!(crate::sounds::sound_id(rt, hit).as_deref(), Some("minecraft:block.stone.hit"));
            // `tryHarvestBlock`.
            let mode = api::call(rt, GAME_MODE_FIELD, &[p]);
            let survival = api::call(rt, MODE_FOR_PLAYER, &[mode]);
            assert_eq!(rt.get_field(survival, "$ordinal").i(), 0);
            let fire = key(COMMON_HOOKS, "fireBlockBreak", "(Lnet/minecraft/world/level/Level;Lnet/minecraft/world/level/GameType;Lnet/minecraft/server/level/ServerPlayer;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)Lnet/neoforged/neoforge/event/level/BlockEvent$BreakEvent;");
            let ev = api::call(rt, fire, &[level, survival, p, pos, st]);
            assert_eq!(api::call(rt, key(BREAK, "isCanceled", "()Z"), &[ev]).i(), 0);
            let heard = rt.interned.get("test:heard").copied().unwrap_or_default();
            assert_eq!(rt.str_of(heard), Some("break minecraft:stone by fake true"));
            assert_eq!(api::call(rt, key(STATE, "canHarvestBlock", STATE_HARVEST), &[st, level, pos, p]).i(), 1);
            let tool = api::call(rt, key(ITEM_STACK, "copy", "()Lnet/minecraft/world/item/ItemStack;"), &[pickaxe]);
            api::call(rt, key(ITEM_STACK, "mineBlock", STACK_MINE), &[pickaxe, level, st, pos, p]);
            assert_eq!(api::call(rt, DAMAGE, &[pickaxe]).i(), 1, "a pickaxe wears by one on stone");
            let fluid = api::call(rt, FLUID_AT, &[level, pos]);
            assert_eq!(api::call(rt, key(STATE, "onDestroyedByPlayer", STATE_DESTROYED), &[st, level, pos, p, V::I(1), fluid]).i(), 1);
            api::call(rt, key(BLOCK, "destroy", DESTROY_DESC), &[V::NULL, level, pos, st]);
            let drops = api::call(rt, key(BLOCK, "getDrops", DROPS_BY), &[st, level, pos, V::NULL, p, tool]);
            let inv = api::call(rt, key("net/minecraft/world/entity/player/Player", "getInventory", "()Lnet/minecraft/world/entity/player/Inventory;"), &[p]);
            for s in crate::jutil::elements(rt, drops) {
                api::call(rt, key(crate::menus::INVENTORY, "placeItemBackInInventory", "(Lnet/minecraft/world/item/ItemStack;)V"), &[inv, s]);
            }
            let second = crate::fake_players::slot(rt, inv, 1);
            assert_eq!((api_common::item_like_name(rt, rt.get_field(second, "$item")), rt.get_field(second, "$count").i()), ("minecraft:cobblestone".into(), 1));
            api::call(rt, key(STATE, "spawnAfterBreak", STATE_AFTER_BREAK), &[st, level, pos, tool, V::I(1)]);
            let id = api::call(rt, key("net/minecraft/world/entity/Entity", "getId", "()I"), &[p]).i();
            api::call(rt, key("net/minecraft/server/level/ServerLevel", "destroyBlockProgress", "(ILnet/minecraft/core/BlockPos;I)V"), &[level, V::I(id), pos, V::I(4)]);
            assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        });
        assert_eq!(h.blocks.get(&AT).map(String::as_str), Some("minecraft:air"));
        assert_eq!(h.after_breaks, vec![(AT, STONE.to_owned(), Some("minecraft:iron_pickaxe".to_owned()))]);
        assert!(matches!(h.cracks.as_slice(), [(id, AT, 4)] if *id < 0), "{:?}", h.cracks);
    }

    /// Create's drills and saws break through `BlockHelper.destroyBlockAs`: an ore's experience as
    /// `BlockDropsEvent` makes it (silk touch takes it away, a listener changes it) and the orbs it
    /// pops; broken ice melts into water when what is below blocks motion (a cobweb never does); a
    /// mod's ore made as vanilla's `DropExperienceBlock` draws from its own range.
    #[test]
    fn a_drill_pops_an_ores_experience_through_block_drops_event() {
        const ORE: &str = "minecraft:diamond_ore";
        const EXPERIENCE: &str = "()I";
        let mut rt = runtime(&[(BLOCK_DROPS, 4)]);
        let mut h = world();
        h.blocks.insert(AT, ORE.into());
        h.experience.insert(ORE.into(), 5);
        h.solid.extend([STONE.to_owned(), "minecraft:cobweb".to_owned()]);
        rt.with_host(&mut h, |rt| {
            let (p, pickaxe) = fake(rt, Some("minecraft:iron_pickaxe"));
            let level = crate::glue::level(rt);
            let pos = api::block_pos(rt, AT[0].into(), AT[1].into(), AT[2].into());
            let st = api::call(rt, GET_BLOCK_STATE, &[level, pos]);
            let made = |rt: &mut Rt, tool: V| {
                let drops = crate::jutil::new_list(rt, Vec::new());
                let ev = rt.new_object(BLOCK_DROPS);
                api::call(rt, key(BLOCK_DROPS, "<init>", DROPS_INIT), &[ev, level, pos, st, V::NULL, drops, p, tool]);
                ev
            };
            let ev = made(rt, pickaxe);
            assert_eq!(api::call(rt, key(BLOCK_DROPS, "getDroppedExperience", EXPERIENCE), &[ev]).i(), 5);
            crate::neo::post(rt, BLOCK_DROPS, ev);
            let xp = api::call(rt, key(BLOCK_DROPS, "getDroppedExperience", EXPERIENCE), &[ev]).i();
            assert_eq!(xp, 7, "the listener's");
            assert_eq!(api::call(rt, key(BLOCK_DROPS, "isCanceled", "()Z"), &[ev]).i(), 0);
            let breaker = api::call(rt, key(BLOCK_DROPS, "getBreaker", "()Lnet/minecraft/world/entity/Entity;"), &[ev]);
            assert_eq!(breaker, p);
            let block = api::call(rt, key(STATE, "getBlock", "()Lnet/minecraft/world/level/block/Block;"), &[st]);
            api::call(rt, key(BLOCK, "popExperience", POP_EXPERIENCE), &[block, level, pos, V::I(xp)]);
            // Silk touch drops none.
            let registry = api::get_static(rt, "net/minecraft/core/registries/Registries.ENCHANTMENT");
            let silk = api::get_static(rt, "net/minecraft/world/item/enchantment/Enchantments.SILK_TOUCH");
            let silk = api::call(rt, "net/minecraft/core/HolderGetter.getOrThrow.(Lnet/minecraft/resources/ResourceKey;)Lnet/minecraft/core/Holder$Reference;", &[registry, silk]);
            let silky = api_common::stack(rt, "minecraft:iron_pickaxe", 1);
            api::call(rt, "net/minecraft/world/item/ItemStack.enchant.(Lnet/minecraft/core/Holder;I)V", &[silky, silk, V::I(1)]);
            let ev = made(rt, silky);
            assert_eq!(api::call(rt, key(BLOCK_DROPS, "getDroppedExperience", EXPERIENCE), &[ev]).i(), 0);
            // Ice melts into water over a block that blocks motion.
            let ice = api::state(rt, "minecraft:ice");
            let ice = api::call(rt, key(STATE, "getBlock", "()Lnet/minecraft/world/level/block/Block;"), &[ice]);
            assert!(rt.instance_of(ice, "net/minecraft/world/level/block/IceBlock"));
            let water = api::call(rt, "net/minecraft/world/level/block/IceBlock.meltsInto.()Lnet/minecraft/world/level/block/state/BlockState;", &[]);
            assert_eq!(crate::host::split_state(&text_of(rt, water)).0, "minecraft:water");
            for (block, blocks) in [(STONE, 1), ("minecraft:cobweb", 0), ("minecraft:air", 0)] {
                let below = api::state(rt, block);
                assert_eq!(api::call(rt, key(STATE, "blocksMotion", "()Z"), &[below]).i(), blocks, "{block}");
            }
            // A mod's ore: `DropExperienceBlock(ConstantInt.of(4), properties)`.
            rt.supers.insert("com/example/ZincOre", "net/minecraft/world/level/block/DropExperienceBlock");
            let ore = rt.new_object("com/example/ZincOre");
            let range = api::call(rt, "net/minecraft/util/valueproviders/ConstantInt.of.(I)Lnet/minecraft/util/valueproviders/ConstantInt;", &[V::I(4)]);
            let props = api::call(rt, "net/minecraft/world/level/block/state/BlockBehaviour$Properties.of.()Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;", &[]);
            api::call(rt, "net/minecraft/world/level/block/DropExperienceBlock.<init>.(Lnet/minecraft/util/valueproviders/IntProvider;Lnet/minecraft/world/level/block/state/BlockBehaviour$Properties;)V", &[ore, range, props]);
            rt.registered.insert(("block".into(), "example:zinc_ore".into()), ore);
            let ore_state = api::state(rt, "example:zinc_ore");
            assert_eq!(api::call(rt, key(STATE, "getExpDrop", STATE_EXP), &[ore_state, level, pos, V::NULL, p, pickaxe]).i(), 4);
            assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        });
        assert_eq!(h.orbs, vec![([3.5, 64.5, -1.5], 7, None)], "at the ore's centre");
    }

    /// NeoForge's events have their say: a `BreakSpeed` listener's speed is the player's, and a
    /// `HarvestCheck` listener's answer whether the tool is right (a hundredth of the speed a tick,
    /// not a thirtieth). By hand, stone needs a pickaxe: a hundredth of the bare hand's 1.
    #[test]
    fn break_speed_and_harvest_check_listeners_decide() {
        let mut rt = runtime(&[(BREAK_SPEED, 2), (HARVEST_CHECK, 3)]);
        let mut h = world();
        rt.with_host(&mut h, |rt| {
            let (p, _) = fake(rt, Some("minecraft:iron_pickaxe"));
            assert!((progress(rt, p) - 6.0 / 5.0 * 2.0 / 1.5 / 100.0).abs() < 1e-6, "{}", progress(rt, p));
        });
        let mut rt = runtime(&[]);
        let mut h = world();
        rt.with_host(&mut h, |rt| {
            let (p, _) = fake(rt, None);
            assert!((progress(rt, p) - 1.0 / 5.0 / 1.5 / 100.0).abs() < 1e-6, "{}", progress(rt, p));
            assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        });
    }

    /// `Level.destroyBlock(pos, drop)`, as Create's machines break blocks (a belt's ends, a
    /// pulley's rope, a link that lost its support): the break's effect each time; with drops, the
    /// ore's diamond popped about its block and its experience at its centre, as the
    /// `BlockDropsEvent` listener left it; without, air only; with `doTileDrops` off, neither
    /// drops nor experience. Air isn't destroyed. No block snapshots are ever restored here, and
    /// water and lava are the liquids.
    #[test]
    fn destroy_block_drops_through_block_drops_event_as_the_rules_allow() {
        const ORE: &str = "minecraft:diamond_ore";
        const DESTROY: &str = "net/minecraft/world/level/Level.destroyBlock.(Lnet/minecraft/core/BlockPos;Z)Z";
        let mut rt = runtime(&[(BLOCK_DROPS, 4)]);
        let mut h = world();
        let (east, west) = ([AT[0] + 1, AT[1], AT[2]], [AT[0] - 1, AT[1], AT[2]]);
        for p in [AT, east, west] {
            h.blocks.insert(p, ORE.into());
        }
        h.loot.insert(ORE.into(), vec![("minecraft:diamond".into(), 1)]);
        h.experience.insert(ORE.into(), 5);
        rt.with_host(&mut h, |rt| {
            let level = crate::glue::level(rt);
            let at = |rt: &mut Rt, p: [i32; 3]| api::block_pos(rt, p[0].into(), p[1].into(), p[2].into());
            let pos = at(rt, AT);
            assert_eq!(api::call(rt, DESTROY, &[level, pos, V::I(1)]).i(), 1);
            assert_eq!(api::call(rt, DESTROY, &[level, pos, V::I(1)]).i(), 0, "air");
            let pos = at(rt, east);
            assert_eq!(api::call(rt, DESTROY, &[level, pos, V::I(0)]).i(), 1);
            let rules = api::call(rt, "net/minecraft/world/level/Level.getGameRules.()Lnet/minecraft/world/level/GameRules;", &[level]);
            let drops = api::get_static(rt, "net/minecraft/world/level/GameRules.RULE_DOBLOCKDROPS");
            let rule = api::call(rt, "net/minecraft/world/level/GameRules.getRule.(Lnet/minecraft/world/level/GameRules$Key;)Lnet/minecraft/world/level/GameRules$Value;", &[rules, drops]);
            api::call(rt, "net/minecraft/world/level/GameRules$BooleanValue.set.(ZLnet/minecraft/server/MinecraftServer;)V", &[rule, V::I(0), V::NULL]);
            let pos = at(rt, west);
            assert_eq!(api::call(rt, DESTROY, &[level, pos, V::I(1)]).i(), 1);
            assert_eq!(api::call(rt, "net/minecraft/world/level/Level.$restoringBlockSnapshots.()Z", &[level]).i(), 0);
            for (state, liquid) in [("minecraft:water[level=0]", 1), ("minecraft:lava[level=4]", 1), (STONE, 0)] {
                let st = api::state(rt, state);
                assert_eq!(api::call(rt, key(STATE, "liquid", "()Z"), &[st]).i(), liquid, "{state}");
            }
            assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        });
        for p in [AT, east, west] {
            assert_eq!(h.blocks.get(&p).map(String::as_str), Some("minecraft:air"));
        }
        let [item] = h.item_entities.as_slice() else { panic!("one diamond: {:?}", h.item_entities) };
        assert_eq!((item.item.as_str(), item.count), ("minecraft:diamond", 1));
        let (lo, hi) = ([3.25, 64.125, -1.75], [3.75, 64.625, -1.25]);
        assert!((0..3).all(|i| (lo[i]..=hi[i]).contains(&item.pos[i])), "about the block: {:?}", item.pos);
        assert_eq!(h.orbs, vec![([3.5, 64.5, -1.5], 7, None)], "the listener's 5 + 2");
        let events: Vec<_> = h.level_events.iter().map(|&(e, p, _)| (e, p)).collect();
        assert_eq!(events, [(2001, AT), (2001, east), (2001, west)]);
    }
}
