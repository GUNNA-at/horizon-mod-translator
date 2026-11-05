//! Inner living/player damage application after the outer hurt gates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageStat {
    Absorbed,
    DealtAbsorbed,
    Taken,
}
pub trait DamageWorld {
    type Error;
    type Attacker: Copy;
    fn invulnerable(&mut self) -> Result<bool, Self::Error>;
    fn after_armor(&mut self, damage: f32) -> Result<f32, Self::Error>;
    fn after_magic(&mut self, damage: f32) -> Result<f32, Self::Error>;
    fn absorption(&mut self) -> Result<f32, Self::Error>;
    fn set_absorption(&mut self, value: f32) -> Result<(), Self::Error>;
    fn server_player_attacker(&mut self) -> Result<Option<Self::Attacker>, Self::Error>;
    fn award_attacker(
        &mut self,
        attacker: Self::Attacker,
        stat: DamageStat,
        amount: i32,
    ) -> Result<(), Self::Error>;
    fn award_self(&mut self, stat: DamageStat, amount: i32) -> Result<(), Self::Error>;
    fn source_food_exhaustion(&mut self) -> Result<f32, Self::Error>;
    fn cause_food_exhaustion(&mut self, amount: f32) -> Result<(), Self::Error>;
    fn record_damage(&mut self, amount: f32) -> Result<(), Self::Error>;
    fn health(&mut self) -> Result<f32, Self::Error>;
    fn set_health(&mut self, value: f32) -> Result<(), Self::Error>;
    fn damage_event(&mut self) -> Result<(), Self::Error>;
    /// The damage once armour and magic took their share, before absorption (where NeoForge's
    /// `LivingEntity.actuallyHurt` posts `LivingDamageEvent.Pre`): what it gives back goes on.
    /// Vanilla has nothing here, so by default it is the damage as it came.
    fn damage_pre(&mut self, damage: f32) -> Result<f32, Self::Error> {
        Ok(damage)
    }
    /// What reached the health, at the end of the inner damage (`LivingDamageEvent.Post`); 0 when
    /// absorption took it all. Nothing by default.
    fn damage_post(&mut self, damage: f32) -> Result<(), Self::Error> {
        let _ = damage;
        Ok(())
    }
}
pub(crate) fn positive(value: f32) -> f32 {
    if value.is_nan() {
        value
    } else if value <= 0.0 {
        0.0
    } else {
        value
    }
}
pub(crate) fn stat_amount(value: f32) -> i32 {
    let value = value * 10.0;
    // Java Math.round(float), including negative ties and integer saturation.
    let bits = value.to_bits() as i32;
    let shift = 149 - ((bits >> 23) & 0xff);
    if (0..32).contains(&shift) {
        let mut significand = (bits & 0x7f_ffff) | 0x80_0000;
        if bits < 0 {
            significand = -significand;
        }
        ((significand >> shift) + 1) >> 1
    } else {
        value as i32
    }
}
pub(crate) const STAT_LIMIT: f32 = 3.402_823_5e37;
pub fn actually_hurt<W: DamageWorld>(
    world: &mut W,
    damage: f32,
    player: bool,
) -> Result<(), W::Error> {
    if world.invulnerable()? {
        return Ok(());
    }
    actually_hurt_after_immunity_check(world, damage, player)
}
/// Caller must have performed this hit's native invulnerability query.
pub(crate) fn actually_hurt_after_immunity_check<W: DamageWorld>(
    world: &mut W,
    damage: f32,
    player: bool,
) -> Result<(), W::Error> {
    let damage = world.after_armor(damage)?;
    let damage = world.after_magic(damage)?;
    let damage = world.damage_pre(damage)?;
    let remaining = positive(damage - world.absorption()?);
    let absorption = world.absorption()? - (damage - remaining);
    world.set_absorption(absorption)?;
    let absorbed = damage - remaining;
    if absorbed > 0.0 && absorbed < STAT_LIMIT {
        if player {
            world.award_self(DamageStat::Absorbed, stat_amount(absorbed))?;
        } else if let Some(attacker) = world.server_player_attacker()? {
            world.award_attacker(attacker, DamageStat::DealtAbsorbed, stat_amount(absorbed))?;
        }
    }
    if remaining == 0.0 {
        return world.damage_post(0.0);
    }
    if player {
        let exhaustion = world.source_food_exhaustion()?;
        world.cause_food_exhaustion(exhaustion)?;
    }
    world.record_damage(remaining)?;
    let health = world.health()?;
    world.set_health(health - remaining)?;
    if player {
        if remaining < STAT_LIMIT {
            world.award_self(DamageStat::Taken, stat_amount(remaining))?;
        }
    } else {
        let absorption = world.absorption()?;
        world.set_absorption(absorption - remaining)?;
    }
    world.damage_event()?;
    world.damage_post(remaining)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mob-like world: no armour or magic, `absorption` to start with, and a log of what the
    /// pre and post steps saw.
    struct World {
        health: f32,
        absorption: f32,
        pre: Option<f32>,
        seen: Vec<(&'static str, f32)>,
    }
    impl DamageWorld for World {
        type Error = ();
        type Attacker = ();
        fn invulnerable(&mut self) -> Result<bool, ()> {
            Ok(false)
        }
        fn after_armor(&mut self, damage: f32) -> Result<f32, ()> {
            Ok(damage)
        }
        fn after_magic(&mut self, damage: f32) -> Result<f32, ()> {
            Ok(damage)
        }
        fn absorption(&mut self) -> Result<f32, ()> {
            Ok(self.absorption)
        }
        fn set_absorption(&mut self, value: f32) -> Result<(), ()> {
            self.absorption = value.max(0.0);
            Ok(())
        }
        fn server_player_attacker(&mut self) -> Result<Option<()>, ()> {
            Ok(None)
        }
        fn award_attacker(&mut self, _: (), _: DamageStat, _: i32) -> Result<(), ()> {
            Ok(())
        }
        fn award_self(&mut self, _: DamageStat, _: i32) -> Result<(), ()> {
            Ok(())
        }
        fn source_food_exhaustion(&mut self) -> Result<f32, ()> {
            Ok(0.0)
        }
        fn cause_food_exhaustion(&mut self, _: f32) -> Result<(), ()> {
            Ok(())
        }
        fn record_damage(&mut self, _: f32) -> Result<(), ()> {
            Ok(())
        }
        fn health(&mut self) -> Result<f32, ()> {
            Ok(self.health)
        }
        fn set_health(&mut self, value: f32) -> Result<(), ()> {
            self.health = value;
            Ok(())
        }
        fn damage_event(&mut self) -> Result<(), ()> {
            Ok(())
        }
        fn damage_pre(&mut self, damage: f32) -> Result<f32, ()> {
            self.seen.push(("pre", damage));
            Ok(self.pre.unwrap_or(damage))
        }
        fn damage_post(&mut self, damage: f32) -> Result<(), ()> {
            self.seen.push(("post", damage));
            Ok(())
        }
    }

    /// The pre step's amount is what absorption and the health take; the post step hears what
    /// reached the health, 0 when absorption took it all.
    #[test]
    fn the_damage_goes_on_as_the_pre_step_leaves_it() {
        let mut w = World { health: 20.0, absorption: 0.0, pre: Some(3.0), seen: Vec::new() };
        actually_hurt(&mut w, 8.0, false).unwrap();
        assert_eq!(w.health, 17.0);
        assert_eq!(w.seen, [("pre", 8.0), ("post", 3.0)]);
        let mut w = World { health: 20.0, absorption: 4.0, pre: None, seen: Vec::new() };
        actually_hurt(&mut w, 3.0, false).unwrap();
        assert_eq!((w.health, w.absorption), (20.0, 1.0));
        assert_eq!(w.seen, [("pre", 3.0), ("post", 0.0)]);
    }
}
