//! A mod's statistics of a server's player (docs/native-mods.md): what a
//! NeoForge mod reads and awards through `ServerPlayer.getStats()` and
//! `awardStat` lands in the game's own counters, saved with them, sent to the
//! player's client and counted by the scoreboard's objectives of that
//! statistic, as the game's own awards are.
use super::Game;

#[allow(dead_code)]
impl Game<'_> {
    /// The value of the statistic of type `ty` and value `key` (ids:
    /// `minecraft:custom` and `minecraft:jump`, a mod's own custom statistic)
    /// of the server's player with that UUID, `ServerStatsCounter.getValue`.
    /// None off a server, or for a player not reached (as
    /// `native_award_criterion` reaches them). API @API:mod_stats@.
    pub(super) fn native_stat_value(&mut self, uuid: u128, ty: &str, key: &str) -> Option<i32> {
        self.as_criteria_player(uuid, |g| {
            g.load_stats();
            (!g.no_player_here()).then(|| g.stats.value(ty, key))
        })
        .flatten()
    }

    /// Sets that statistic of the server's player with that UUID to `value`
    /// (`ServerStatsCounter.setValue`): saved with their statistics and sent
    /// to their client; the scoreboard isn't told. False off a server, or
    /// for a player not reached. API @API:mod_stats@.
    pub(super) fn native_set_stat(&mut self, uuid: u128, ty: &str, key: &str, value: i32) -> bool {
        self.as_criteria_player(uuid, |g| {
            g.load_stats();
            if g.no_player_here() {
                return false;
            }
            g.stats.set(ty, key, value);
            true
        })
        .unwrap_or(false)
    }

    /// The scoreboard's part of `ServerPlayer.awardStat` (`change` added to
    /// the player's score in each objective of that statistic's criterion)
    /// and of `resetStat` (None: each reset), applied with the game's own
    /// statistic changes. False off a server, or for a player not reached.
    /// API @API:mod_stats@.
    pub(super) fn native_score_stat(
        &mut self,
        uuid: u128,
        ty: &str,
        key: &str,
        change: Option<i32>,
    ) -> bool {
        self.as_criteria_player(uuid, |g| {
            if g.no_player_here() {
                return false;
            }
            g.stats.events.push((ty.to_owned(), key.to_owned(), change));
            true
        })
        .unwrap_or(false)
    }
}
