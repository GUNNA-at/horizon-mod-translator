//! A player's blows and clicks on the entities translated mods keep themselves (Waddles' penguins;
//! docs/native-mods.md, "Mod entities hit and clicked"; hook points of API @API:mod_entity_hits@).
//! The window's client finds the one its crosshair meets (`native_client_mod_entity_hit`) and
//! sends the server the attack or the click as for any entity, its mod predicting the click
//! (`native_client_mod_entity_interact`). The server's game, the player's state swapped in, works
//! the blow out as `Player.attack` does and has the entity's mod take it
//! (`native_mod_entity_attack`), then does the player's side of it (the sounds, the particles, the
//! sweep, the weapon's wear and the exhaustion); a click its mod makes (`native_mod_entity_interact`:
//! an animal eats its food from the hand).
use super::{Game, native_mod_entities::ModEntity};
use horizon_client::first_person::Hand;
use horizon_core::geometry::{Aabb, Vec3};
use horizon_protocol::packets::play::{self, Serverbound as S};

/// One of a translated mod's entities the window's view meets.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ModEntityHit {
    /// Its network id.
    pub entity: i32,
    /// How far along the view, in blocks.
    pub distance: f64,
    /// Where the view meets its box.
    pub location: [f64; 3],
    /// Its feet: the click's place on it is `location` less these.
    pub feet: [f64; 3],
}

/// A player's blow on a translated mod's entity, as `Player.attack` worked it out in the game.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ModEntityBlow {
    /// The damage: the weapon's scaled by the attack strength, a critical hit's, the enchantments'.
    pub amount: f32,
    /// The type of the player's damage source: `minecraft:player_attack`, `minecraft:mace_smash`.
    pub damage_type: &'static str,
    /// The extra knockback's strength (`causeExtraKnockback`), 0 for none.
    pub knockback: f32,
    /// `(sin yaw, -cos yaw)` of the player's yaw, which the extra knockback is along.
    pub direction: [f64; 2],
}

/// What a player's blow did to a translated mod's entity.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ModEntityStruck {
    /// Whether it could be hit at all (`isAttackable`, and no `skipAttackInteraction`).
    pub attackable: bool,
    /// Whether it took the blow (`hurt`).
    pub landed: bool,
    /// A living one's health before and after.
    pub health: Option<[f32; 2]>,
    /// Its feet, and its box's width and height, after the blow.
    pub position: [f64; 3],
    pub size: [f32; 2],
    /// Whether it is still alive.
    pub alive: bool,
}

/// What a click on a translated mod's entity came to (`InteractionResult`).
// Only translated mods' generated code answers these, so a build without one never does.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModEntityClick {
    /// Nothing used: the hand's own item use and the other hand go on.
    Pass,
    /// Nothing used, the click goes on as for `Pass` (`FAIL` doesn't consume the action).
    Fail,
    /// Used, unswung: the click ends.
    Consume,
    /// Used, and the hand swings: the click ends.
    Success,
}

impl ModEntityClick {
    /// `InteractionResult.consumesAction`.
    fn used(self) -> bool {
        matches!(self, ModEntityClick::Consume | ModEntityClick::Success)
    }
}

impl Game<'_> {
    /// Mod entity `id` of the level in place as its mod tells it now (the mods are asked again),
    /// for a player's attack or click on it. None for one of the game's own entities.
    pub(super) fn current_mod_entity(&mut self, id: i32) -> Option<ModEntity> {
        let network = u32::try_from(id).ok()?;
        if self
            .level_entity(id)
            .is_some_and(|t| !matches!(t, super::commands::Target::Mod(_)))
        {
            return None;
        }
        self.refresh_mod_entities();
        self.mod_entity(network).cloned()
    }

    /// The translated mods' entity the window's view meets before any block, as their mods tell
    /// it (`native_client_mod_entity_hit`). Only a server's client asks.
    pub(super) fn mod_entity_hit(&self) -> Option<ModEntityHit> {
        if self.remote.is_none() {
            return None;
        }
        let (eye, look) = self.aim()?;
        let mut hit = None;
        self.native_client_mod_entity_hit(eye, look, self.entity_reach, &mut hit);
        hit.filter(|h| self.before_block(h.distance))
    }

    /// Where the client's copy of a mod's entity `vehicle` seats the window's own player
    /// (`native_client_mod_seat`): None when it isn't a mod's entity the client has.
    pub(super) fn mod_seat_of(&mut self, vehicle: i32) -> Option<[f64; 3]> {
        let me = self.remote.as_ref()?.entity_id;
        let mut seat = None;
        self.native_client_mod_seat(vehicle, me, &mut seat);
        seat
    }
    /// The translated mods' entity the crosshair targets (`Minecraft.hitResult`): the one the view
    /// meets before any block, unless one of the game's own entities is nearer.
    pub(super) fn mod_entity_targeted(&self) -> Option<ModEntityHit> {
        let hit = self.mod_entity_hit()?;
        let nearer = [
            self.painting_hit().map(|h| h.1),
            self.frame_hit().map(|h| h.1),
            self.armor_stand_hit().map(|h| h.1),
            self.cushion_hit().map(|h| h.1),
            self.boat_hit().map(|h| h.1),
            self.cart_hit().map(|h| h.1),
            self.interaction_hit().map(|h| h.1),
            self.dragon_hit().map(|h| h.1),
            self.mob_hit().map(|h| h.1),
            self.redirectable_hit().map(|h| h.1),
            self.remote_object_hit(true).map(|h| h.distance),
        ]
        .into_iter()
        .flatten()
        .any(|d| d < hit.distance);
        (!nearer).then_some(hit)
    }

    /// `MultiPlayerGameMode.interact` on the translated mod's entity `hit` with `hand`: the click
    /// goes to the server (`ServerboundInteractPacket`, with where it met the entity), and the
    /// entity's mod predicts what it comes to on this client (`native_client_mod_entity_interact`).
    /// Whether the click ends there: the hand swings for a `SUCCESS`.
    pub(super) fn use_mod_entity(&mut self, hit: ModEntityHit, hand: Hand, sneaking: bool) -> bool {
        self.send(S::Interact(play::Interact {
            id: hit.entity,
            hand: i32::from(hand == Hand::Off),
            location: [0, 1, 2].map(|a| hit.location[a] - hit.feet[a]),
            secondary: sneaking,
        }));
        let mut click = None;
        self.native_client_mod_entity_interact(hit.entity, hand, &mut click);
        match click {
            Some(ModEntityClick::Success) => {
                self.with_hand(hand, Game::swing_arm);
                true
            }
            Some(c) => c.used(),
            None => false,
        }
    }

    /// `Player.attack` on the translated mod's entity `target`, by the player in place on a server:
    /// the blow worked out as for the game's own (the weapon, the attack strength, a critical hit
    /// on a living one, the enchantments against its type, the extra knockback), taken by its mod
    /// (`native_mod_entity_attack`); then the player's side of a blow that landed: the sounds, the
    /// critical hit's and the damage's particles, a sword's sweep at the game's mobs around it, the
    /// weapon's use and wear, the exhaustion. Whether a mod took it.
    pub(super) fn attack_mod_entity(&mut self, target: &ModEntity) -> bool {
        self.with_hit_player(|game| game.strike_mod_entity(target))
    }

    fn strike_mod_entity(&mut self, target: &ModEntity) -> bool {
        let kind = target.kind.clone();
        let (amount, scale, crit, magic) = self.melee_damage(&kind, target.living);
        let sweep = if target.living {
            self.sweep_attack_position(scale, crit)
        } else {
            None
        };
        let (knockback, knockback_attack) = self.extra_knockback(&kind, scale);
        self.attack_ticks = 0;
        let from = self
            .player
            .as_ref()
            .map(|p| [p.position.x, p.position.y, p.position.z]);
        let blow = ModEntityBlow {
            amount,
            damage_type: if self.smash_attack() {
                "minecraft:mace_smash"
            } else {
                "minecraft:player_attack"
            },
            knockback,
            direction: {
                let (x, z) = self.extra_knockback_direction();
                [x, z]
            },
        };
        let mut struck = None;
        let dimension = self.dim.id();
        self.native_mod_entity_attack(dimension, target.id, &blow, &mut struck);
        let Some(struck) = struck else {
            return false;
        };
        if !struck.attackable {
            return true;
        }
        if knockback_attack {
            self.audio
                .play("minecraft:entity.player.attack.knockback", from, 1.0, 1.0);
        }
        if !struck.landed {
            self.audio
                .play("minecraft:entity.player.attack.nodamage", from, 1.0, 1.0);
            return true;
        }
        if knockback > 0.0 {
            self.stop_sprint_after_extra_knockback();
        }
        let [width, height] = struck.size.map(f64::from);
        let feet = struck.position;
        let dealt = struck.health.map_or(0.0, |[before, after]| before - after);
        if struck.health.is_some() {
            // Player.damageStatsAndHearts: the damage the blow did.
            self.stats
                .custom("damage_dealt", (dealt * 10.0).round() as i32);
        }
        // The critical hit's and the enchantments' particles, around its box.
        for (shown, particle) in [
            (crit, horizon_client::particles::ParticleType::Crit),
            (magic, horizon_client::particles::ParticleType::EnchantedHit),
        ] {
            if shown {
                self.burst(particle, feet, [width, height], 0.0, 16);
            }
        }
        if dealt > 2.0 {
            self.burst(
                horizon_client::particles::ParticleType::DamageIndicator,
                feet,
                [0.2, 0.0],
                0.2,
                (dealt * 0.5) as i32,
            );
        }
        let sound = if crit {
            "minecraft:entity.player.attack.crit"
        } else if scale > 0.9 {
            "minecraft:entity.player.attack.strong"
        } else {
            "minecraft:entity.player.attack.weak"
        };
        if let Some(at) = sweep {
            let half = width / 2.0;
            let area = Aabb::new(
                Vec3::new(feet[0] - half, feet[1], feet[2] - half),
                Vec3::new(feet[0] + half, feet[1] + height, feet[2] + half),
            );
            self.sweep_attack(at, from, Some((area, super::mace::Spared::Nothing)), scale);
        } else {
            self.audio.play(sound, from, 1.0, 1.0);
        }
        if let Some(stack) = self.held().cloned() {
            if target.living
                && self
                    .res
                    .items()
                    .get(&stack.item)
                    .is_some_and(|i| i.weapon_damage.is_some())
            {
                self.stats.add("used", &stack.item, 1);
            }
            if let Ok(id) = u32::try_from(target.id) {
                self.native_hurt_enemy(&stack, id, &kind);
            }
            self.weapon_wear(&stack);
        }
        // Player.attack: causeFoodExhaustion(0.1F) for a blow that hurt.
        if let Some(p) = self.player.as_mut()
            && !self.creative
        {
            p.cause_food_exhaustion(0.1);
        }
        true
    }

    /// `count` particles of `particle` around a box of `size` (width, height) standing at `feet`,
    /// as a server sends them (`ServerLevel.sendParticles`), to the players here too.
    fn burst(
        &mut self,
        particle: horizon_client::particles::ParticleType,
        feet: [f64; 3],
        size: [f64; 2],
        speed: f64,
        count: i32,
    ) {
        let options = horizon_client::particles::ParticleOptions::simple(particle);
        let at = [feet[0], feet[1] + size[1] * 0.5, feet[2]];
        let spread = [size[0] * 0.25, size[1] * 0.25, size[0] * 0.25];
        self.particles
            .send_particles(&options, at, spread, speed, count.max(0) as u32);
        self.lan_send_particles(super::lan_particles::QueuedParticle {
            dimension: self.dim.id(),
            options,
            state: None,
            at,
            spread,
            speed: [speed; 3],
            count,
            randomization: 0,
        });
    }

    /// `Player.interactOn` with `hand` on the translated mod's entity `target`, by the player in
    /// place on a server: the mods hear `EntityInteract` first (`native_interact_entity`), then the
    /// entity's mod makes the click (`native_mod_entity_interact`). Whether the hand swung.
    pub(super) fn interact_mod_entity(&mut self, target: &ModEntity, hand: Hand) -> bool {
        let swings = self.swings_started;
        let stack = self.with_hand(hand, |g| g.held().cloned());
        let mut claimed = false;
        let previous = std::mem::replace(&mut self.hand, hand);
        self.native_interact_entity(target.id, &target.kind, stack.as_ref(), &mut claimed);
        self.hand = previous;
        if claimed {
            return self.swings_started != swings;
        }
        let mut click = None;
        let dimension = self.dim.id();
        self.native_mod_entity_interact(dimension, target.id, hand, &mut click);
        if click == Some(ModEntityClick::Success) {
            self.with_hand(hand, Game::swing_arm);
        }
        self.swings_started != swings
    }
}
