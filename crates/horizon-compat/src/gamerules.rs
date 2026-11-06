//! Game rules as a mod reads them (`level.getGameRules().getBoolean(GameRules.RULE_MOBGRIEFING)`):
//! 1.21.1's rules by their keys, each the game's own rule of 26.3's name ([`crate::host::Host::game_rule`]:
//! 1.21.11 renamed them, and turned some around: `disableRaids` is `raids` negated, `doFireTick` a
//! fire spread radius that isn't 0). A rule a mod registers itself (`GameRules.register(name,
//! category, BooleanValue.create(default))`) has its default, the game keeping no such rule; one a
//! mod sets keeps what it was set to in this runtime.
use crate::{Rt, V};

const RULES: &str = "net/minecraft/world/level/GameRules";
const KEY: &str = "net/minecraft/world/level/GameRules$Key";
const TYPE: &str = "net/minecraft/world/level/GameRules$Type";
const VALUE: &str = "net/minecraft/world/level/GameRules$Value";
const BOOLEAN_VALUE: &str = "net/minecraft/world/level/GameRules$BooleanValue";
const INTEGER_VALUE: &str = "net/minecraft/world/level/GameRules$IntegerValue";

/// How a 1.21.1 rule is the game's rule: the same value, the boolean turned around, or a boolean
/// that is the game's integer not being 0.
#[derive(Clone, Copy, PartialEq)]
enum Read {
    Same,
    Not,
    NonZero,
}

/// 1.21.1's rules: the `GameRules` field, the rule's id, its category, whether it is an integer, its
/// default (a boolean's as 0 or 1), and the game's rule it reads (None: 26.3 has none).
const TABLE: [(&str, &str, &str, bool, i32, Option<&str>, Read); 51] = [
    ("RULE_DOFIRETICK", "doFireTick", "UPDATES", false, 1, Some("fire_spread_radius_around_player"), Read::NonZero),
    ("RULE_MOBGRIEFING", "mobGriefing", "MOBS", false, 1, Some("mob_griefing"), Read::Same),
    ("RULE_KEEPINVENTORY", "keepInventory", "PLAYER", false, 0, Some("keep_inventory"), Read::Same),
    ("RULE_DOMOBSPAWNING", "doMobSpawning", "SPAWNING", false, 1, Some("spawn_mobs"), Read::Same),
    ("RULE_DOMOBLOOT", "doMobLoot", "DROPS", false, 1, Some("mob_drops"), Read::Same),
    ("RULE_PROJECTILESCANBREAKBLOCKS", "projectilesCanBreakBlocks", "DROPS", false, 1, Some("projectiles_can_break_blocks"), Read::Same),
    ("RULE_DOBLOCKDROPS", "doTileDrops", "DROPS", false, 1, Some("block_drops"), Read::Same),
    ("RULE_DOENTITYDROPS", "doEntityDrops", "DROPS", false, 1, Some("entity_drops"), Read::Same),
    ("RULE_COMMANDBLOCKOUTPUT", "commandBlockOutput", "CHAT", false, 1, Some("command_block_output"), Read::Same),
    ("RULE_NATURAL_REGENERATION", "naturalRegeneration", "PLAYER", false, 1, Some("natural_health_regeneration"), Read::Same),
    ("RULE_DAYLIGHT", "doDaylightCycle", "UPDATES", false, 1, Some("advance_time"), Read::Same),
    ("RULE_LOGADMINCOMMANDS", "logAdminCommands", "CHAT", false, 1, Some("log_admin_commands"), Read::Same),
    ("RULE_SHOWDEATHMESSAGES", "showDeathMessages", "CHAT", false, 1, Some("show_death_messages"), Read::Same),
    ("RULE_RANDOMTICKING", "randomTickSpeed", "UPDATES", true, 3, Some("random_tick_speed"), Read::Same),
    ("RULE_SENDCOMMANDFEEDBACK", "sendCommandFeedback", "CHAT", false, 1, Some("send_command_feedback"), Read::Same),
    ("RULE_REDUCEDDEBUGINFO", "reducedDebugInfo", "MISC", false, 0, Some("reduced_debug_info"), Read::Same),
    ("RULE_SPECTATORSGENERATECHUNKS", "spectatorsGenerateChunks", "PLAYER", false, 1, Some("spectators_generate_chunks"), Read::Same),
    ("RULE_SPAWN_RADIUS", "spawnRadius", "PLAYER", true, 10, Some("respawn_radius"), Read::Same),
    ("RULE_DISABLE_ELYTRA_MOVEMENT_CHECK", "disableElytraMovementCheck", "PLAYER", false, 0, Some("elytra_movement_check"), Read::Not),
    ("RULE_MAX_ENTITY_CRAMMING", "maxEntityCramming", "MOBS", true, 24, Some("max_entity_cramming"), Read::Same),
    ("RULE_WEATHER_CYCLE", "doWeatherCycle", "UPDATES", false, 1, Some("advance_weather"), Read::Same),
    ("RULE_LIMITED_CRAFTING", "doLimitedCrafting", "PLAYER", false, 0, Some("limited_crafting"), Read::Same),
    ("RULE_MAX_COMMAND_CHAIN_LENGTH", "maxCommandChainLength", "MISC", true, 65536, Some("max_command_sequence_length"), Read::Same),
    ("RULE_MAX_COMMAND_FORK_COUNT", "maxCommandForkCount", "MISC", true, 65536, Some("max_command_forks"), Read::Same),
    ("RULE_COMMAND_MODIFICATION_BLOCK_LIMIT", "commandModificationBlockLimit", "MISC", true, 32768, Some("max_block_modifications"), Read::Same),
    ("RULE_ANNOUNCE_ADVANCEMENTS", "announceAdvancements", "CHAT", false, 1, Some("show_advancement_messages"), Read::Same),
    ("RULE_DISABLE_RAIDS", "disableRaids", "MOBS", false, 0, Some("raids"), Read::Not),
    ("RULE_DOINSOMNIA", "doInsomnia", "SPAWNING", false, 1, Some("spawn_phantoms"), Read::Same),
    ("RULE_DO_IMMEDIATE_RESPAWN", "doImmediateRespawn", "PLAYER", false, 0, Some("immediate_respawn"), Read::Same),
    ("RULE_PLAYERS_NETHER_PORTAL_DEFAULT_DELAY", "playersNetherPortalDefaultDelay", "PLAYER", true, 80, None, Read::Same),
    ("RULE_PLAYERS_NETHER_PORTAL_CREATIVE_DELAY", "playersNetherPortalCreativeDelay", "PLAYER", true, 1, None, Read::Same),
    ("RULE_DROWNING_DAMAGE", "drowningDamage", "PLAYER", false, 1, Some("drowning_damage"), Read::Same),
    ("RULE_FALL_DAMAGE", "fallDamage", "PLAYER", false, 1, Some("fall_damage"), Read::Same),
    ("RULE_FIRE_DAMAGE", "fireDamage", "PLAYER", false, 1, Some("fire_damage"), Read::Same),
    ("RULE_FREEZE_DAMAGE", "freezeDamage", "PLAYER", false, 1, Some("freeze_damage"), Read::Same),
    ("RULE_DO_PATROL_SPAWNING", "doPatrolSpawning", "SPAWNING", false, 1, Some("spawn_patrols"), Read::Same),
    ("RULE_DO_TRADER_SPAWNING", "doTraderSpawning", "SPAWNING", false, 1, Some("spawn_wandering_traders"), Read::Same),
    ("RULE_DO_WARDEN_SPAWNING", "doWardenSpawning", "SPAWNING", false, 1, Some("spawn_wardens"), Read::Same),
    ("RULE_FORGIVE_DEAD_PLAYERS", "forgiveDeadPlayers", "MOBS", false, 1, Some("forgive_dead_players"), Read::Same),
    ("RULE_UNIVERSAL_ANGER", "universalAnger", "MOBS", false, 0, Some("universal_anger"), Read::Same),
    ("RULE_PLAYERS_SLEEPING_PERCENTAGE", "playersSleepingPercentage", "PLAYER", true, 100, Some("players_sleeping_percentage"), Read::Same),
    ("RULE_BLOCK_EXPLOSION_DROP_DECAY", "blockExplosionDropDecay", "DROPS", false, 1, Some("block_explosion_drop_decay"), Read::Same),
    ("RULE_MOB_EXPLOSION_DROP_DECAY", "mobExplosionDropDecay", "DROPS", false, 1, Some("mob_explosion_drop_decay"), Read::Same),
    ("RULE_TNT_EXPLOSION_DROP_DECAY", "tntExplosionDropDecay", "DROPS", false, 0, Some("tnt_explosion_drop_decay"), Read::Same),
    ("RULE_SNOW_ACCUMULATION_HEIGHT", "snowAccumulationHeight", "UPDATES", true, 1, Some("max_snow_accumulation_height"), Read::Same),
    ("RULE_WATER_SOURCE_CONVERSION", "waterSourceConversion", "UPDATES", false, 1, Some("water_source_conversion"), Read::Same),
    ("RULE_LAVA_SOURCE_CONVERSION", "lavaSourceConversion", "UPDATES", false, 0, Some("lava_source_conversion"), Read::Same),
    ("RULE_GLOBAL_SOUND_EVENTS", "globalSoundEvents", "MISC", false, 1, Some("global_sound_events"), Read::Same),
    ("RULE_DO_VINES_SPREAD", "doVinesSpread", "UPDATES", false, 1, Some("spread_vines"), Read::Same),
    ("RULE_ENDER_PEARLS_VANISH_ON_DEATH", "enderPearlsVanishOnDeath", "PLAYER", false, 1, Some("ender_pearls_vanish_on_death"), Read::Same),
    ("RULE_SPAWN_CHUNK_RADIUS", "spawnChunkRadius", "MISC", true, 2, None, Read::Same),
];

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    owner.starts_with(RULES) || name == "getGameRules"
}

/// `GameRules.RULE_*`: 1.21.1's rule keys, one object each.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    if owner != RULES {
        return None;
    }
    let &(_, id, category, int, default, _, _) = TABLE.iter().find(|r| r.0 == field)?;
    Some(rt.intern(format!("gamerule/{id}"), move |rt| {
        let ty = rule_type(rt, int, default);
        let category = crate::records::enum_constant(rt, &format!("{RULES}$Category.{category}")).unwrap_or(V::NULL);
        let name = rt.string(id);
        new_key(rt, name, category, ty)
    }))
}

fn new_key(rt: &mut Rt, id: V, category: V, ty: V) -> V {
    let k = rt.new_object(KEY);
    rt.put_field(k, "$id", id);
    rt.put_field(k, "$category", category);
    rt.put_field(k, "$type", ty);
    k
}

/// A rule's type: an integer's or a boolean's, with its default.
fn rule_type(rt: &mut Rt, int: bool, default: i32) -> V {
    let t = rt.new_object(TYPE);
    rt.put_field(t, "$int", V::I(int as i32));
    rt.put_field(t, "$default", V::I(default));
    t
}

/// The value of the rule `key` names: one a mod set here, else the game's (as 1.21.1 reads it),
/// else its default.
fn value(rt: &Rt, key: V) -> i32 {
    let id = rt.str_of(rt.get_field(key, "$id")).unwrap_or("").to_owned();
    by_id(rt, &id).unwrap_or_else(|| rt.get_field(rt.get_field(key, "$type"), "$default").i())
}

/// The value of 1.21.1's rule `id` (`doTileDrops`): one a mod set here, else the game's, else its
/// default. None for a rule vanilla doesn't have that no mod set.
fn by_id(rt: &Rt, id: &str) -> Option<i32> {
    if let Some(V::I(v)) = rt.interned.get(&format!("gamerule-set/{id}")) {
        return Some(*v);
    }
    let &(_, _, _, _, default, now, read) = TABLE.iter().find(|r| r.1 == id)?;
    let game = now.and_then(|n| rt.host().and_then(|h| h.game_rule(n)));
    Some(match (game, read) {
        (Some(v), Read::Same) => v,
        (Some(v), Read::Not) => (v == 0) as i32,
        (Some(v), Read::NonZero) => (v != 0) as i32,
        (None, _) => default,
    })
}

/// Whether blocks drop their items (`doTileDrops`, vanilla's `RULE_DOBLOCKDROPS`).
pub(crate) fn blocks_drop(rt: &Rt) -> bool {
    by_id(rt, "doTileDrops").is_none_or(|v| v != 0)
}

fn is_int(rt: &Rt, key: V) -> bool {
    rt.get_field(rt.get_field(key, "$type"), "$int").i() != 0
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        // A level's or a server's rules: one object, which reads the game's.
        (_, "getGameRules") if !owner.starts_with(RULES) => rt.intern("compat:game-rules".into(), |rt| rt.new_object(RULES)),
        (RULES, "register") => {
            let k = new_key(rt, a(0), a(1), a(2));
            let id = rt.str_of(a(0)).unwrap_or("").to_owned();
            rt.interned.insert(format!("gamerule/{id}"), k);
            k
        }
        (BOOLEAN_VALUE, "create") => rule_type(rt, false, a(0).i()),
        (INTEGER_VALUE, "create") => rule_type(rt, true, a(0).i()),
        (RULES, "getBoolean") => V::I((value(rt, a(1)) != 0) as i32),
        (RULES, "getInt") => V::I(value(rt, a(1))),
        // `getRule(key)`: the rule's value object, of its type's class.
        (RULES, "getRule") => {
            let v = rt.new_object(if is_int(rt, a(1)) { INTEGER_VALUE } else { BOOLEAN_VALUE });
            rt.put_field(v, "$key", a(1));
            v
        }
        (KEY, "getId" | "toString") => rt.get_field(a(0), "$id"),
        (KEY, "getCategory") => rt.get_field(a(0), "$category"),
        (KEY, "getDescriptionId") => {
            let id = rt.str_of(rt.get_field(a(0), "$id")).unwrap_or("").to_owned();
            rt.string(&format!("gamerule.{id}"))
        }
        (KEY, "equals") => {
            let same = rt.class_of(a(1)) == KEY && rt.str_of(rt.get_field(a(0), "$id")) == rt.str_of(rt.get_field(a(1), "$id"));
            V::I(same as i32)
        }
        (KEY, "hashCode") => V::I(crate::jlang::string_hash(rt.str_of(rt.get_field(a(0), "$id")).unwrap_or(""))),
        (TYPE, "createRule") => {
            let v = rt.new_object(if rt.get_field(a(0), "$int").i() != 0 { INTEGER_VALUE } else { BOOLEAN_VALUE });
            let anonymous = rt.string("");
            let k = new_key(rt, anonymous, V::NULL, a(0));
            rt.put_field(v, "$key", k);
            v
        }
        (VALUE | BOOLEAN_VALUE | INTEGER_VALUE, n) => {
            let k = rt.get_field(a(0), "$key");
            let id = rt.str_of(rt.get_field(k, "$id")).unwrap_or("").to_owned();
            match n {
                "get" if desc.ends_with(")Z") => V::I((value(rt, k) != 0) as i32),
                "get" => V::I(value(rt, k)),
                "getCommandResult" => V::I(value(rt, k)),
                "serialize" | "toString" => {
                    let v = value(rt, k);
                    rt.string(&if is_int(rt, k) { v.to_string() } else { (v != 0).to_string() })
                }
                // Set here: what this runtime reads of the rule from now on.
                "set" | "setValue" => {
                    rt.interned.insert(format!("gamerule-set/{id}"), V::I(a(1).i()));
                    V::Void
                }
                _ => return None,
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A host with the game's rules by 26.3's names.
    struct Rules(Vec<(&'static str, i32)>);
    impl crate::host::Host for Rules {
        fn block_state(&self, _pos: [i32; 3]) -> String {
            "minecraft:air".into()
        }
        fn set_block_state(&mut self, _pos: [i32; 3], _state: &str) -> bool {
            false
        }
        fn game_rule(&self, name: &str) -> Option<i32> {
            self.0.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
        }
    }

    /// 1.21.1's rules read the game's of their 26.3 names: `mobGriefing` off where the world has
    /// `mob_griefing` off, `disableRaids` on where it has `raids` off, `doFireTick` off where fire
    /// spreads in no radius, `randomTickSpeed` the game's number; a rule the game doesn't have is
    /// its default. A mod's own rule is its type's default, and what the mod sets it to after.
    #[test]
    fn a_mod_reads_the_games_rules_by_their_old_keys() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = Rules(vec![("mob_griefing", 0), ("raids", 0), ("fire_spread_radius_around_player", 0), ("random_tick_speed", 7)]);
        let level = rt.new_object("net/minecraft/server/level/ServerLevel");
        let get_bool = "net/minecraft/world/level/GameRules.getBoolean.(Lnet/minecraft/world/level/GameRules$Key;)Z";
        let get_int = "net/minecraft/world/level/GameRules.getInt.(Lnet/minecraft/world/level/GameRules$Key;)I";
        rt.with_host(&mut host, |rt| {
            let rules = crate::api::call(rt, "net/minecraft/world/level/Level.getGameRules.()Lnet/minecraft/world/level/GameRules;", &[level]);
            let read = |rt: &mut Rt, k: &'static str, field: &str| {
                let key = rt.static_key(&format!("net/minecraft/world/level/GameRules.{field}"));
                let key = crate::api::get_static(rt, key);
                crate::api::call(rt, k, &[rules, key])
            };
            assert_eq!(read(rt, get_bool, "RULE_MOBGRIEFING"), V::I(0));
            assert_eq!(read(rt, get_bool, "RULE_DISABLE_RAIDS"), V::I(1));
            assert_eq!(read(rt, get_bool, "RULE_DOFIRETICK"), V::I(0));
            assert_eq!(read(rt, get_int, "RULE_RANDOMTICKING"), V::I(7));
            assert_eq!(read(rt, get_bool, "RULE_KEEPINVENTORY"), V::I(0));
            assert_eq!(read(rt, get_int, "RULE_PLAYERS_NETHER_PORTAL_DEFAULT_DELAY"), V::I(80));

            let name = rt.string("securitycraft:pass_through");
            let misc = crate::api::get_static(rt, "net/minecraft/world/level/GameRules$Category.MISC");
            let ty = crate::api::call(rt, "net/minecraft/world/level/GameRules$BooleanValue.create.(Z)Lnet/minecraft/world/level/GameRules$Type;", &[V::I(1)]);
            let own = crate::api::call(
                rt,
                "net/minecraft/world/level/GameRules.register.(Ljava/lang/String;Lnet/minecraft/world/level/GameRules$Category;Lnet/minecraft/world/level/GameRules$Type;)Lnet/minecraft/world/level/GameRules$Key;",
                &[name, misc, ty],
            );
            assert_eq!(crate::api::call(rt, get_bool, &[rules, own]), V::I(1));
            let rule = crate::api::call(rt, "net/minecraft/world/level/GameRules.getRule.(Lnet/minecraft/world/level/GameRules$Key;)Lnet/minecraft/world/level/GameRules$Value;", &[rules, own]);
            crate::api::call(rt, "net/minecraft/world/level/GameRules$BooleanValue.set.(ZLnet/minecraft/server/MinecraftServer;)V", &[rule, V::I(0), V::NULL]);
            assert_eq!(crate::api::call(rt, get_bool, &[rules, own]), V::I(0));
            let id = crate::api::call(rt, "net/minecraft/world/level/GameRules$Key.getId.()Ljava/lang/String;", &[own]);
            assert_eq!(rt.str_of(id), Some("securitycraft:pass_through"));
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
