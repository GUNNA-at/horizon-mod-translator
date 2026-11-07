//! Mobs for native server mods (docs/native-mods.md, "Mobs", API 37): spawn one,
//! read the mobs of the level in place, and move, hurt or remove one by its id.
//! They act where the world is simulated, never on a client's copy, and only on
//! the level in place, which each names by its `dimension` (`minecraft:overworld`)
//! as `native_set_block` does: another one is refused. While the game ticks its
//! mobs they are out of the level: a mob a hook spawns then joins it after the
//! tick, and the others read as gone.
use super::{Game, commands::Target};
use horizon_client::mobs::{Damage, DamageKind, Who};
use horizon_core::geometry::Vec3;

/// A mob as a native mod reads it.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeMob {
    pub(crate) id: u32,
    /// The level it is in (`minecraft:overworld`).
    pub(crate) dimension: String,
    /// `minecraft:zombie`.
    pub(crate) kind: String,
    pub(crate) pos: [f64; 3],
    /// Degrees; 0 faces south.
    pub(crate) yaw: f32,
    pub(crate) health: f32,
    pub(crate) max_health: f32,
}

/// What a mob starts with (`native_spawn_mob`); the default is a plain mob.
#[allow(dead_code)]
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NativeSpawn {
    /// A name over it, shown to everyone.
    pub(crate) name: Option<String>,
    /// Cut to its maximum.
    pub(crate) health: Option<f32>,
    pub(crate) no_ai: bool,
    pub(crate) invisible: bool,
    pub(crate) silent: bool,
    pub(crate) invulnerable: bool,
    pub(crate) no_gravity: bool,
}

/// A blow's source as a native mod tells it (`native_hurt_mob`), vanilla's `DamageSource`.
#[allow(dead_code)]
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NativeHurt {
    /// Its damage type (`minecraft:thrown`): one the game doesn't know (a mod's own) hurts as
    /// `minecraft:generic`.
    pub(crate) damage_type: String,
    /// The player it is credited to (`getEntity()`), by UUID.
    pub(crate) player: Option<u128>,
    /// Else the mob it is credited to, by id.
    pub(crate) mob: Option<u32>,
    /// Where it came from (`getSourcePosition()`): the knockback's direction.
    pub(crate) from: Option<[f64; 3]>,
    /// The horizontal movement of the projectile that dealt it: the knockback's direction instead
    /// (`Projectile.calculateHorizontalHurtKnockbackDirection`).
    pub(crate) motion: Option<[f64; 2]>,
    /// The type of what dealt it (`getDirectEntity()`) when that isn't the one it is credited to,
    /// a projectile: the blow isn't direct (`isDirect`).
    pub(crate) direct: Option<String>,
}

/// Whom a blow struck (`native_post_attack`).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum NativeStruck {
    /// A player, by UUID.
    Player(u128),
    /// A mob, by id.
    Mob(u32),
}

/// The weapon whose enchantments act after a blow (`native_post_attack`).
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NativeWeapon {
    /// None: the victim's worn enchantments only.
    None,
    /// What the player the blow is credited to holds in their main hand.
    Held,
    /// This stack, with its enchantments.
    Stack(horizon_client::items::ItemStack),
}

fn finite(v: [f64; 3]) -> bool {
    v.into_iter().all(f64::is_finite)
}

/// `/summon`'s world bounds (`Level.isInSpawnableBounds`).
fn spawnable(pos: [f64; 3]) -> bool {
    finite(pos)
        && (-30_000_000.0..30_000_000.0).contains(&pos[0])
        && (-30_000_000.0..30_000_000.0).contains(&pos[2])
        && (-20_000_000.0..20_000_000.0).contains(&pos[1].floor())
}

// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// Whether this is a level that simulates (not a client's copy) and the one
    /// in place is `dimension`.
    fn native_mob_level(&self, dimension: &str) -> bool {
        self.remote.is_none() && dimension == self.dim.id()
    }
    /// Spawns a mob of `kind` (`minecraft:zombie`) at `pos` in the level in
    /// place, `dimension`, finalized as a natural spawn is, and kept when the
    /// chunk unloads: its id. None for a kind the game doesn't know, another
    /// dimension, a position outside `/summon`'s bounds or in a chunk that isn't
    /// loaded (it would never tick or be saved), or a monster on peaceful (it
    /// would despawn at once). API 37.
    pub(super) fn native_spawn_mob(
        &mut self,
        dimension: &str,
        kind: &str,
        pos: [f64; 3],
        options: &NativeSpawn,
    ) -> Option<u32> {
        if !self.native_mob_level(dimension) || !spawnable(pos) || !self.native_chunk_loaded(pos) {
            return None;
        }
        let qualified = if kind.contains(':') {
            kind.to_owned()
        } else {
            ["minecraft:", kind].concat()
        };
        let monster = self
            .res
            .mob_types()
            .get(&qualified)
            .is_some_and(|t| t.category == "monster");
        if monster && self.ui.difficulty == 0 {
            return None;
        }
        let id = self.spawn_mob(kind, pos, true)?;
        // A jockey's other mob comes after it: this one is found by its id.
        if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
            m.no_ai = options.no_ai;
            m.invisible = options.invisible;
            m.silent = options.silent;
            m.invulnerable_flag = options.invulnerable;
            m.no_gravity = options.no_gravity;
            if let Some(name) = &options.name {
                m.custom_name = Some(name.clone());
                m.name_visible = true;
            }
            if let Some(health) = options.health.filter(|h| h.is_finite() && *h > 0.0) {
                m.health = health.min(m.attrs.max_health);
            }
        }
        Some(id)
    }
    /// Whether the chunk holding `pos` is loaded in the level in place.
    fn native_chunk_loaded(&self, pos: [f64; 3]) -> bool {
        let (x, z) = (pos[0].floor() as i32, pos[2].floor() as i32);
        self.world.chunks.contains_key(&(x >> 4, z >> 4))
    }
    /// The living mob `id` in the level in place, `dimension`; none on a
    /// client's copy. API 37.
    pub(super) fn native_mob(&self, dimension: &str, id: u32) -> Option<NativeMob> {
        if !self.native_mob_level(dimension) {
            return None;
        }
        let mob = self.mobs.iter().find(|m| m.id == id && !m.removed)?;
        mob.alive().then(|| self.native_mob_of(mob))
    }
    /// The living mobs in the level in place, `dimension`; none on a client's
    /// copy. API 37.
    pub(super) fn native_mobs(&self, dimension: &str) -> Vec<NativeMob> {
        if !self.native_mob_level(dimension) {
            return Vec::new();
        }
        self.mobs
            .iter()
            .filter(|m| !m.removed && m.alive())
            .map(|m| self.native_mob_of(m))
            .collect()
    }
    /// Moves mob `id` to `pos` at once, as `/tp` does: it dismounts, drops its
    /// path and resets its fall. False for no such mob, another dimension or a
    /// position outside `/summon`'s bounds. API 37.
    pub(super) fn native_teleport_mob(&mut self, dimension: &str, id: u32, pos: [f64; 3]) -> bool {
        if !self.native_mob_level(dimension) || !spawnable(pos) {
            return false;
        }
        let Some(m) = self.mobs.iter_mut().find(|m| m.id == id && !m.removed) else {
            return false;
        };
        let to = Vec3::new(pos[0], pos[1], pos[2]);
        // `TeleportCommand`: stop the navigation, no vertical motion, and a
        // rider is put back on its vehicle unless it gets off first.
        m.ai.nav.stop();
        m.velocity.y = 0.0;
        m.extra.mount = None;
        m.extra.passenger = false;
        m.position = to;
        m.previous = to;
        m.fall_distance = 0.0;
        m.needs_sync = true;
        true
    }
    /// Adds `velocity` to mob `id`'s: false for no such mob, another dimension
    /// or a velocity that isn't finite. API 37.
    pub(super) fn native_push_mob(&mut self, dimension: &str, id: u32, velocity: [f64; 3]) -> bool {
        if !self.native_mob_level(dimension) || !finite(velocity) {
            return false;
        }
        let Some(m) = self.mobs.iter_mut().find(|m| m.id == id && !m.removed) else {
            return false;
        };
        m.velocity = Vec3::new(
            m.velocity.x + velocity[0],
            m.velocity.y + velocity[1],
            m.velocity.z + velocity[2],
        );
        m.needs_sync = true;
        true
    }
    /// Sets mob `id`'s health, cut to its maximum: false for no such mob,
    /// another dimension or a health that isn't above 0 (`native_kill_mob`
    /// kills). A dying mob's too, as `LivingEntity.setHealth` sets it: one a
    /// mod keeps from dying (`native_living_death`) lives on with it. API 37.
    pub(super) fn native_set_mob_health(&mut self, dimension: &str, id: u32, health: f32) -> bool {
        if !self.native_mob_level(dimension) || !health.is_finite() || health <= 0.0 {
            return false;
        }
        let Some(m) = self.mobs.iter_mut().find(|m| m.id == id && !m.removed) else {
            return false;
        };
        m.health = health.min(m.attrs.max_health);
        m.needs_sync = true;
        true
    }
    /// Hurts mob `id` by `amount` as `LivingEntity.hurt(source, amount)` does,
    /// as `/damage` deals it: its invulnerability frames, armor and effects,
    /// the knockback (along a projectile's movement, else away from where the
    /// blow came from), the hurt flash and sound, its anger, and the kill
    /// credited to the player or mob `hurt` names. A guest's blow is dealt as
    /// them, unless another guest is in place (then nobody is credited).
    /// False when it didn't hurt: no such mob, another dimension, an amount
    /// that isn't finite, or a mob that refused it. API @API:mob_hurt@.
    pub(super) fn native_hurt_mob(&mut self, dimension: &str, id: u32, amount: f32, hurt: &NativeHurt) -> bool {
        if !self.native_mob_level(dimension) || !amount.is_finite() || !self.mobs.iter().any(|m| m.id == id && !m.removed) {
            return false;
        }
        let player = hurt.player.and_then(|u| self.player_with_uuid(horizon_client::lan::playerdata::uuid_ints(u)));
        let attacker = match player {
            Some(p) if p == self.lan_acting => Some(Who::Player),
            Some(Some(conn)) if self.lan_acting.is_none() => {
                let Some(mut host) = self.lan.take() else {
                    return false;
                };
                let hurt_it = self.as_guest(&mut host, conn, |g| g.hurt_mob_by(id, amount, hurt, Some(Who::Player)));
                // The mob now answers to the guest (`LivingEntity.lastHurtByPlayer`).
                if hurt_it.is_some() && self.mobs.iter().any(|m| m.id == id) {
                    host.mob_players.insert(id, conn);
                }
                self.lan = Some(host);
                return hurt_it.unwrap_or(false);
            }
            Some(_) => None,
            None => hurt.mob.filter(|m| self.mobs.iter().any(|x| x.id == *m && !x.removed)).map(Who::Mob),
        };
        self.hurt_mob_by(id, amount, hurt, attacker)
    }
    /// Hurts the player `uuid` by `amount` as `LivingEntity.hurt(source, amount)`
    /// does on a server's player, as `/damage` deals it: nothing in creative,
    /// else their invulnerability frames, a raised shield, armor and effects,
    /// the knockback away from where the blow came from, the hurt sound, and
    /// the death message's killer, the mob `hurt` names (a player's credit
    /// isn't kept). A damage type the game doesn't know (a mod's own) hurts as
    /// `minecraft:generic`. Also inside a player's hook, for the acting player.
    /// False when it didn't hurt: no such player in `dimension`, dead, an
    /// amount that isn't finite, or a hit refused. API @API:player_hurt@.
    pub(super) fn native_hurt_player(
        &mut self,
        dimension: &str,
        uuid: u128,
        amount: f32,
        hurt: &NativeHurt,
    ) -> bool {
        if !amount.is_finite() {
            return false;
        }
        let from = hurt.from.filter(|f| finite(*f));
        let mut hurt_it = false;
        self.native_as_player(uuid, |game| {
            if !game.native_mob_level(dimension) {
                return;
            }
            let known = game.motion.health.damage.id(&hurt.damage_type).is_some();
            let damage_type = if known {
                hurt.damage_type.as_str()
            } else {
                "minecraft:generic"
            };
            let mob = hurt
                .mob
                .filter(|m| game.mobs.iter().any(|x| x.id == *m && !x.removed));
            hurt_it = game.hurt_player_from(amount, from, damage_type, Some(mob));
        });
        hurt_it
    }
    /// The UUID of the player whose entity is `entity` (`Entity.getId`): the
    /// acting player, or a player on this server. None for any other entity.
    /// API @API:player_hurt@.
    pub(super) fn native_player_uuid_of(&self, entity: i32) -> Option<u128> {
        if self.player.as_ref().is_some_and(|p| p.entity_id == entity) {
            return self.native_acting_player().map(|p| p.uuid);
        }
        let host = self.lan.as_ref()?;
        host.hub
            .guests
            .iter()
            .find(|g| g.placed && g.entity_id == entity)
            .map(|g| g.uuid)
    }
    fn hurt_mob_by(&mut self, id: u32, amount: f32, hurt: &NativeHurt, attacker: Option<Who>) -> bool {
        let source = Damage {
            kind: Self::damage_kind(&hurt.damage_type),
            attacker,
            from: hurt.from.filter(|f| finite(*f)),
        };
        let aim = self.attacker_aim(attacker);
        let motion = hurt.motion.filter(|m| m[0].is_finite() && m[1].is_finite());
        let hurt_it = self.mobs.iter_mut().find(|m| m.id == id && !m.removed).is_some_and(|m| match motion {
            Some(motion) => m.damage_projectile(amount, source, aim, motion),
            None => m.damage_aimed(amount, source, aim),
        });
        if attacker == Some(Who::Player) {
            self.last_hurt_by_player.insert(id, self.lan_acting);
        }
        if hurt_it {
            self.resolve_player_responsible(id, source);
        }
        hurt_it
    }
    /// Runs the enchantments' effects that follow a blow `hurt` on `victim`, as
    /// `EnchantmentHelper.doPostAttackEffectsWithItemSource` does once it has
    /// landed. A player's blow on a mob: `weapon`'s effects on the mob (fire
    /// aspect sets it alight, bane of arthropods slows it; for a blow through a
    /// projectile, `hurt.direct`, only the ones that don't need a direct blow).
    /// A mob's blow on a player: the player's worn thorns hurt that mob. A
    /// guest's run as them, unless another guest is in place. False when none
    /// could run: another dimension, no such mob or player, or a blow the game
    /// doesn't model the effects of (one of a mob on a mob, or of a player on
    /// a player). API @API:post_attack@.
    pub(super) fn native_post_attack(&mut self, dimension: &str, victim: NativeStruck, hurt: &NativeHurt, weapon: &NativeWeapon) -> bool {
        if !self.native_mob_level(dimension) {
            return false;
        }
        match victim {
            NativeStruck::Mob(id) => {
                let Some(player) = hurt.player else {
                    return false;
                };
                let direct = hurt.direct.as_deref();
                let mut ran = false;
                self.native_as_player(player, |game| {
                    let held = match weapon {
                        NativeWeapon::None => None,
                        NativeWeapon::Held => game.held().cloned(),
                        NativeWeapon::Stack(s) => Some(s.clone()),
                    };
                    let index = game.mobs.iter().position(|m| m.id == id && !m.removed && m.alive());
                    if let (Some(held), Some(index)) = (held, index) {
                        game.post_attack_with(index, &held, direct);
                        ran = true;
                    }
                });
                ran
            }
            NativeStruck::Player(uuid) => {
                let Some(mob) = hurt.mob.filter(|m| self.mobs.iter().any(|x| x.id == *m && !x.removed && x.alive())) else {
                    return false;
                };
                self.native_as_player(uuid, |game| game.thorns(mob))
            }
        }
    }
    /// Knocks mob `id` back as `LivingEntity.knockback(strength, x, z)` does:
    /// away from (`x`, `z`), less its knockback resistance, and as native mods'
    /// knockback hooks (`native_living_knockback`) change it. False for no such
    /// mob, another dimension or numbers that aren't finite. API @API:mob_hurt@.
    pub(super) fn native_knockback_mob(&mut self, dimension: &str, id: u32, strength: f64, x: f64, z: f64) -> bool {
        if !self.native_mob_level(dimension) || !finite([strength, x, z]) {
            return false;
        }
        let Some(m) = self.mobs.iter_mut().find(|m| m.id == id && !m.removed) else {
            return false;
        };
        m.knockback(strength, x, z, &Damage::of(DamageKind::Generic));
        true
    }
    /// Kills mob `id` as `/kill` does: it falls and drops its loot. False for
    /// no such living mob or another dimension. API 37.
    pub(super) fn native_kill_mob(&mut self, dimension: &str, id: u32) -> bool {
        if self.native_mob(dimension, id).is_none() {
            return false;
        }
        self.kill_target(Target::Mob(id));
        true
    }
    /// Takes mob `id` out of the world with no death and no loot: false for no
    /// such mob or another dimension. API 37.
    pub(super) fn native_remove_mob(&mut self, dimension: &str, id: u32) -> bool {
        if !self.native_mob_level(dimension) {
            return false;
        }
        let Some(m) = self.mobs.iter_mut().find(|m| m.id == id && !m.removed) else {
            return false;
        };
        m.removed = true;
        true
    }
    fn native_mob_of(&self, m: &horizon_client::mobs::Mob) -> NativeMob {
        NativeMob {
            id: m.id,
            dimension: self.dim.id().to_owned(),
            kind: horizon_client::command::args::normalize(&m.kind.id),
            pos: [m.position.x, m.position.y, m.position.z],
            yaw: m.yaw,
            health: m.health,
            max_health: m.attrs.max_health,
        }
    }
}
