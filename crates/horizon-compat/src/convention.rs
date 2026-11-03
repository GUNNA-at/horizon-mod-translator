//! NeoForge's convention tags (`c:`): Minecraft has none, yet any NeoForge mod's data and code may name
//! them (`#c:nuggets/iron` in Create's andesite alloy, `Tags.Items.INGOTS_IRON`). These are NeoForge
//! 1.21.1's own lists, as its jar ships them (`neoforge_tags.txt`).
const TABLE: &str = include_str!("neoforge_tags.txt");

/// A member of a tag: an id, or another tag as `#namespace:path`. A member that isn't required may be
/// missing from the game (NeoForge's milk fluid).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Member {
    pub id: &'static str,
    pub required: bool,
}

/// Every convention tag: its kind (`item`, `block`, `fluid`), its path under `c:` and its members.
pub fn all() -> impl Iterator<Item = (&'static str, &'static str, Vec<Member>)> {
    TABLE.lines().filter(|l| !l.is_empty() && !l.starts_with('#')).filter_map(|line| {
        let mut parts = line.split(' ');
        let (kind, path) = (parts.next()?, parts.next()?);
        let members = parts
            .map(|m| match m.strip_suffix('?') {
                Some(id) => Member { id, required: false },
                None => Member { id: m, required: true },
            })
            .collect();
        Some((kind, path, members))
    })
}

/// The members of `c:<path>` of `kind`, when NeoForge defines that tag.
pub fn members(kind: &str, path: &str) -> Option<Vec<Member>> {
    all().find(|(k, p, _)| *k == kind && *p == path).map(|(_, _, m)| m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_convention_tag_a_tag_names_is_there_or_optional() {
        // A required `#c:` member NeoForge doesn't define would fail the whole tag in the game.
        let mut checked = 0;
        for (kind, path, members) in all() {
            for m in members.iter().filter(|m| m.required) {
                if let Some(sub) = m.id.strip_prefix("#c:") {
                    assert!(members_exist(kind, sub), "c:{path} ({kind}) names #c:{sub}, which NeoForge doesn't define");
                    checked += 1;
                }
            }
        }
        assert!(checked > 50, "{checked}");
        fn members_exist(kind: &str, path: &str) -> bool {
            members(kind, path).is_some()
        }
    }
}
