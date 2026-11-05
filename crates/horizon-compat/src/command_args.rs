//! Vanilla 1.21.1's command argument types as their parsers read a line, natively: each reads
//! with Brigadier's [`Reader`] and fails with vanilla's error where vanilla's parser leaves the
//! cursor (`argument.pos3d.incomplete`, `argument.entity.options.unknown`...). What one reads is
//! what its getter resolves later, with the source the command runs for (a redirect's modifier
//! may give another): coordinates from that source's place and look, selectors over the server's
//! players.
//!
//! - Brigadier's numbers within their type's bounds (`argument.integer.big`...), booleans, words,
//!   strings and greedy strings; messages (with the selectors in them, as `MessageArgument`).
//! - Entity selectors as `EntitySelectorParser` reads them: `@a @e @n @p @r @s`, a player's name
//!   or UUID, and every option of 1.21.1 with its errors. Selecting evaluates the name, type,
//!   position (`x y z`), box (`dx dy dz`), distance, sort and limit options over the server's
//!   players (the entities kept here); gamemode, level, team, tag, nbt, scores, advancements,
//!   predicate and the rotations are read but select as if they matched.
//! - Coordinates: world (`~`, whole numbers a block's centre where the type says) and local (`^`),
//!   as `WorldCoordinates` and `LocalCoordinates` resolve them.
//! - Resource locations, dimensions, items (known to the game, with their `[components]`), block
//!   states (with `[properties]` and `{nbt}`), times in units, angles, UUIDs and colours. SNBT is
//!   read as `TagParser` reads it, its structure checked (its values aren't typed here).
use crate::brigadier::{decimal, is_quoted_string_start, unit, Arg, Read, Reader, Syntax};
use crate::{api, Rt, V};

pub(crate) const SELECTOR: &str = "net/minecraft/commands/arguments/selector/EntitySelector";
pub(crate) const WORLD_COORDINATES: &str = "net/minecraft/commands/arguments/coordinates/WorldCoordinates";
pub(crate) const LOCAL_COORDINATES: &str = "net/minecraft/commands/arguments/coordinates/LocalCoordinates";
pub(crate) const ITEM_INPUT: &str = "net/minecraft/commands/arguments/item/ItemInput";
pub(crate) const BLOCK_INPUT: &str = "net/minecraft/commands/arguments/blocks/BlockInput";
pub(crate) const SINGLE_ANGLE: &str = "net/minecraft/commands/arguments/AngleArgument$SingleAngle";
pub(crate) const MESSAGE: &str = "net/minecraft/commands/arguments/MessageArgument$Message";
pub(crate) const PROFILES: &str = "net/minecraft/commands/arguments/GameProfileArgument$Result";
const SERVER_PLAYER: &str = "net/minecraft/server/level/ServerPlayer";

/// Vanilla's chat colours, `ChatFormatting`'s first sixteen in order (then `reset`, ordinal 21).
pub(crate) const COLOURS: [&str; 16] = [
    "black", "dark_blue", "dark_green", "dark_aqua", "dark_red", "dark_purple", "gold", "gray", "dark_gray", "blue", "green", "aqua", "red", "light_purple",
    "yellow", "white",
];

/// The source's operator level.
pub(crate) fn permission(rt: &Rt, source: V) -> i32 {
    rt.get_field(source, "$permission").i()
}

/// Reads one argument of the vanilla type of `kind` (`IntegerArgumentType.integer`), made with the
/// factory's arguments `given` (its bounds), as the type's parser reads it: the value
/// `getArgument` gives, or vanilla's error. `selectors`: whether the source may use selectors
/// (`EntitySelectorParser.allowSelectors`, level 2). None for a type not kept here.
pub(crate) fn parse(rt: &mut Rt, kind: &str, given: &[V], r: &mut Reader, selectors: bool) -> Option<Read<V>> {
    let arg = |i: usize| given.get(i).copied();
    let centre = arg(0).is_none_or(|v| v.i() != 0);
    Some(match kind {
        "IntegerArgumentType.integer" => {
            let range = (arg(0).map_or(i32::MIN, V::i), arg(1).map_or(i32::MAX, V::i));
            bounded(r, Reader::read_int, range, ("argument.integer.low", "argument.integer.big"), Arg::Int).map(V::I)
        }
        "LongArgumentType.longArg" => {
            let range = (arg(0).map_or(i64::MIN, V::j), arg(1).map_or(i64::MAX, V::j));
            bounded(r, Reader::read_long, range, ("argument.long.low", "argument.long.big"), Arg::Long).map(V::J)
        }
        "FloatArgumentType.floatArg" => {
            let range = (arg(0).map_or(-f32::MAX, V::f), arg(1).map_or(f32::MAX, V::f));
            bounded(r, Reader::read_float, range, ("argument.float.low", "argument.float.big"), |f| decimal(f64::from(f), true)).map(V::F)
        }
        "DoubleArgumentType.doubleArg" => {
            let range = (arg(0).map_or(-f64::MAX, V::d), arg(1).map_or(f64::MAX, V::d));
            bounded(r, Reader::read_double, range, ("argument.double.low", "argument.double.big"), |d| decimal(d, false)).map(V::D)
        }
        "BoolArgumentType.bool" => r.read_boolean().map(|b| crate::api_common::boolean(rt, b)),
        "StringArgumentType.word" => {
            let s = r.read_unquoted_string();
            Ok(rt.string(&s))
        }
        "StringArgumentType.string" => r.read_string().map(|s| rt.string(&s)),
        "StringArgumentType.greedyString" => {
            let s = r.remaining();
            r.cursor = r.total();
            Ok(rt.string(&s))
        }
        "MessageArgument.message" => message(rt, r, selectors),
        "EntityArgument.entity" | "EntityArgument.entities" | "EntityArgument.player" | "EntityArgument.players" => entity(rt, r, kind, selectors),
        "GameProfileArgument.gameProfile" => profiles(rt, r, selectors),
        "BlockPosArgument.blockPos" => {
            if r.at(b'^') {
                local_three(r).map(|v| local_object(rt, v))
            } else {
                world_three(r, false, false).map(|(v, rel)| world_object(rt, v, rel))
            }
        }
        "Vec3Argument.vec3" => {
            if r.at(b'^') {
                local_three(r).map(|v| local_object(rt, v))
            } else {
                world_three(r, true, centre).map(|(v, rel)| world_object(rt, v, rel))
            }
        }
        // Two coordinates (`x z`): the middle one stays the source's (relative, nothing added).
        "ColumnPosArgument.columnPos" => world_two(r, false, false, "argument.pos2d.incomplete").map(|(a, b)| world_object(rt, [a.0, 0.0, b.0], i32::from(a.1) | 2 | i32::from(b.1) << 2)),
        "Vec2Argument.vec2" => world_two(r, true, centre, "argument.pos2d.incomplete").map(|(a, b)| world_object(rt, [a.0, 0.0, b.0], i32::from(a.1) | 2 | i32::from(b.1) << 2)),
        // `yaw pitch`, kept as vanilla's `WorldCoordinates(pitch, yaw, ~)`.
        "RotationArgument.rotation" => world_two(r, true, false, "argument.rotation.incomplete").map(|(yaw, pitch)| world_object(rt, [pitch.0, yaw.0, 0.0], i32::from(pitch.1) | i32::from(yaw.1) << 1 | 4)),
        "AngleArgument.angle" => angle(rt, r),
        "ResourceLocationArgument.id" | "DimensionArgument.dimension" => read_location(r).map(|id| rt.string(&id)),
        "ItemArgument.item" => item(rt, r),
        "BlockStateArgument.block" => block(rt, r),
        "TimeArgument.time" => time(r, arg(0).map_or(0, V::i)).map(V::I),
        "UuidArgument.uuid" => uuid(rt, r),
        "ColorArgument.color" => colour(rt, r),
        _ => return None,
    })
}

/// A number within its type's bounds, as Brigadier's number types read one: below or above them,
/// `low` or `big` (the bound, then what was read) back at the number's start.
fn bounded<T: PartialOrd + Copy>(r: &mut Reader, read: fn(&mut Reader) -> Read<T>, (min, max): (T, T), (low, big): (&'static str, &'static str), arg: impl Fn(T) -> Arg) -> Read<T> {
    let start = r.cursor;
    let n = read(r)?;
    if n < min {
        r.cursor = start;
        return Err(r.error(low, vec![arg(min), arg(n)]));
    }
    if n > max {
        r.cursor = start;
        return Err(r.error(big, vec![arg(max), arg(n)]));
    }
    Ok(n)
}

// ---- Selectors ----

/// How a selector sorts what it finds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum Order {
    #[default]
    Arbitrary,
    Nearest,
    Furthest,
    Random,
}

/// What `EntitySelectorParser` read: the selector it builds, and its own state (which options
/// may still come).
#[derive(Clone, Debug, Default)]
pub(crate) struct Selector {
    pub(crate) max: i32,
    /// `includesEntities`: it may find entities that aren't players.
    pub(crate) entities: bool,
    /// `@s`.
    pub(crate) current: bool,
    /// It was written with `@` (so selecting needs level 2).
    pub(crate) uses: bool,
    pub(crate) name: Option<String>,
    pub(crate) uuid: Option<(i64, i64)>,
    pub(crate) order: Order,
    pub(crate) at: [Option<f64>; 3],
    pub(crate) delta: [Option<f64>; 3],
    pub(crate) distance: Option<(Option<f64>, Option<f64>)>,
    /// The `name=` options: the name, and whether it was negated.
    pub(crate) names: Vec<(String, bool)>,
    /// Whether its `type=` options let a player through (an entity tag isn't judged).
    pub(crate) players_pass: bool,
    limited: bool,
    sorted: bool,
    name_equals: bool,
    name_not_equals: bool,
    gamemode_equals: bool,
    gamemode_not_equals: bool,
    team_equals: bool,
    type_limited: bool,
    type_inverse: bool,
    scores: bool,
    advancements: bool,
    level: bool,
    rot_x: bool,
    rot_y: bool,
}

/// `EntitySelectorParser.parse`: `@` and a selector's letter with its options, or else a player's
/// name or an entity's UUID. `allow`: the source may use `@` (`argument.entity.selector.not_allowed`).
pub(crate) fn parse_selector(r: &mut Reader, allow: bool) -> Read<Selector> {
    let mut s = Selector { players_pass: true, ..Selector::default() };
    if !r.at(b'@') {
        name_or_uuid(r, &mut s)?;
        return Ok(s);
    }
    if !allow {
        return Err(r.error("argument.entity.selector.not_allowed", Vec::new()));
    }
    r.skip();
    s.uses = true;
    if !r.can_read() {
        return Err(r.error("argument.entity.selector.missing", Vec::new()));
    }
    let i = r.cursor;
    let c = r.peek();
    r.skip();
    match u8::try_from(c).unwrap_or(0) {
        b'a' => (s.max, s.type_limited) = (i32::MAX, true),
        b'e' => (s.max, s.entities) = (i32::MAX, true),
        b'n' => (s.max, s.entities, s.order) = (1, true, Order::Nearest),
        b'p' => (s.max, s.order, s.type_limited) = (1, Order::Nearest, true),
        b'r' => (s.max, s.order, s.type_limited) = (1, Order::Random, true),
        b's' => (s.max, s.entities, s.current) = (1, true, true),
        _ => {
            r.cursor = i;
            return Err(r.error("argument.entity.selector.unknown", vec![Arg::Text(format!("@{}", String::from_utf16_lossy(&[c])))]));
        }
    }
    if r.at(b'[') {
        r.skip();
        options(r, &mut s)?;
    }
    Ok(s)
}

/// `parseNameOrUUID`: a string, an entity's UUID when it reads as one, else a player's name (1 to
/// 16 characters).
fn name_or_uuid(r: &mut Reader, s: &mut Selector) -> Read<()> {
    let i = r.cursor;
    let text = r.read_string()?;
    match java_uuid(&text) {
        Some(id) => (s.uuid, s.entities) = (Some(id), true),
        None => {
            if text.is_empty() || text.encode_utf16().count() > 16 {
                r.cursor = i;
                return Err(r.error("argument.entity.invalid", Vec::new()));
            }
            s.name = Some(text);
        }
    }
    s.max = 1;
    Ok(())
}

/// `parseOptions`: `name=value` pairs up to `]`.
fn options(r: &mut Reader, s: &mut Selector) -> Read<()> {
    r.skip_whitespace();
    while r.can_read() && r.peek() != unit(b']') {
        r.skip_whitespace();
        let i = r.cursor;
        let name = r.read_string()?;
        let applicable = match name.as_str() {
            "name" => !s.name_equals,
            "distance" => s.distance.is_none(),
            "level" => !s.level,
            "x" | "y" | "z" => s.at[usize::from(name.as_bytes()[0] - b'x')].is_none(),
            "dx" | "dy" | "dz" => s.delta[usize::from(name.as_bytes()[1] - b'x')].is_none(),
            "x_rotation" => !s.rot_x,
            "y_rotation" => !s.rot_y,
            "limit" => !s.current && !s.limited,
            "sort" => !s.current && !s.sorted,
            "gamemode" => !s.gamemode_equals,
            "team" => !s.team_equals,
            "type" => !s.type_limited,
            "tag" | "nbt" | "predicate" => true,
            "scores" => !s.scores,
            "advancements" => !s.advancements,
            _ => {
                r.cursor = i;
                return Err(r.error("argument.entity.options.unknown", vec![Arg::Text(name)]));
            }
        };
        if !applicable {
            return Err(r.error("argument.entity.options.inapplicable", vec![Arg::Text(name)]));
        }
        r.skip_whitespace();
        if !r.at(b'=') {
            r.cursor = i;
            return Err(r.error("argument.entity.options.valueless", vec![Arg::Text(name)]));
        }
        r.skip();
        r.skip_whitespace();
        option(r, s, &name)?;
        r.skip_whitespace();
        if r.can_read() {
            if r.peek() != unit(b',') {
                if r.peek() != unit(b']') {
                    return Err(r.error("argument.entity.options.unterminated", Vec::new()));
                }
                break;
            }
            r.skip();
        }
    }
    if !r.can_read() {
        return Err(r.error("argument.entity.options.unterminated", Vec::new()));
    }
    r.skip();
    Ok(())
}

/// `!` before an option's value (`shouldInvertValue`).
fn inverted(r: &mut Reader) -> bool {
    r.skip_whitespace();
    if r.at(b'!') {
        r.skip();
        r.skip_whitespace();
        return true;
    }
    false
}

/// One option's value, as its handler in `EntitySelectorOptions` reads it.
fn option(r: &mut Reader, s: &mut Selector, name: &str) -> Read<()> {
    let i = r.cursor;
    match name {
        "name" => {
            let invert = inverted(r);
            let value = r.read_string()?;
            if s.name_not_equals && !invert {
                r.cursor = i;
                return Err(r.error("argument.entity.options.inapplicable", vec![Arg::Text("name".into())]));
            }
            if invert {
                s.name_not_equals = true;
            } else {
                s.name_equals = true;
            }
            s.names.push((value, invert));
        }
        "distance" => {
            let (min, max) = min_max::<f64>(r, "parsing.double.invalid")?;
            if min.is_some_and(|m| m < 0.0) || max.is_some_and(|m| m < 0.0) {
                r.cursor = i;
                return Err(r.error("argument.entity.options.distance.negative", Vec::new()));
            }
            s.distance = Some((min, max));
        }
        "level" => {
            let (min, max) = min_max::<i32>(r, "parsing.int.invalid")?;
            if min.is_some_and(|m| m < 0) || max.is_some_and(|m| m < 0) {
                r.cursor = i;
                return Err(r.error("argument.entity.options.level.negative", Vec::new()));
            }
            s.level = true;
            s.entities = false;
        }
        "x" | "y" | "z" => s.at[usize::from(name.as_bytes()[0] - b'x')] = Some(r.read_double()?),
        "dx" | "dy" | "dz" => s.delta[usize::from(name.as_bytes()[1] - b'x')] = Some(r.read_double()?),
        "x_rotation" | "y_rotation" => {
            wrapped_min_max(r)?;
            if name == "x_rotation" {
                s.rot_x = true;
            } else {
                s.rot_y = true;
            }
        }
        "limit" => {
            let n = r.read_int()?;
            if n < 1 {
                r.cursor = i;
                return Err(r.error("argument.entity.options.limit.toosmall", Vec::new()));
            }
            (s.max, s.limited) = (n, true);
        }
        "sort" => {
            let value = r.read_unquoted_string();
            s.order = match value.as_str() {
                "nearest" => Order::Nearest,
                "furthest" => Order::Furthest,
                "random" => Order::Random,
                "arbitrary" => Order::Arbitrary,
                _ => {
                    r.cursor = i;
                    return Err(r.error("argument.entity.options.sort.irreversible", vec![Arg::Text(value)]));
                }
            };
            s.sorted = true;
        }
        "gamemode" => {
            let invert = inverted(r);
            if s.gamemode_not_equals && !invert {
                r.cursor = i;
                return Err(r.error("argument.entity.options.inapplicable", vec![Arg::Text("gamemode".into())]));
            }
            let value = r.read_unquoted_string();
            if !matches!(value.as_str(), "survival" | "creative" | "adventure" | "spectator") {
                r.cursor = i;
                return Err(r.error("argument.entity.options.mode.invalid", vec![Arg::Text(value)]));
            }
            s.entities = false;
            if invert {
                s.gamemode_not_equals = true;
            } else {
                s.gamemode_equals = true;
            }
        }
        "team" => {
            if !inverted(r) {
                s.team_equals = true;
            }
            r.read_unquoted_string();
        }
        "type" => {
            let invert = inverted(r);
            if s.type_inverse && !invert {
                r.cursor = i;
                return Err(r.error("argument.entity.options.inapplicable", vec![Arg::Text("type".into())]));
            }
            if invert {
                s.type_inverse = true;
            }
            if tag(r) {
                read_location(r)?;
            } else {
                let id = read_location(r)?;
                let player = id == "minecraft:player";
                if player && !invert {
                    s.entities = false;
                }
                s.players_pass &= player != invert;
                if !invert {
                    s.type_limited = true;
                }
            }
        }
        "tag" => {
            inverted(r);
            r.read_unquoted_string();
        }
        "nbt" => {
            inverted(r);
            snbt_struct(r, 0)?;
        }
        "scores" => {
            r.expect(unit(b'{'))?;
            r.skip_whitespace();
            while r.can_read() && r.peek() != unit(b'}') {
                r.skip_whitespace();
                r.read_unquoted_string();
                r.skip_whitespace();
                r.expect(unit(b'='))?;
                r.skip_whitespace();
                min_max::<i32>(r, "parsing.int.invalid")?;
                r.skip_whitespace();
                if r.at(b',') {
                    r.skip();
                }
            }
            r.expect(unit(b'}'))?;
            s.scores = true;
        }
        "advancements" => {
            r.expect(unit(b'{'))?;
            r.skip_whitespace();
            let mut any = false;
            while r.can_read() && r.peek() != unit(b'}') {
                r.skip_whitespace();
                read_location(r)?;
                r.skip_whitespace();
                r.expect(unit(b'='))?;
                r.skip_whitespace();
                if r.at(b'{') {
                    r.skip_whitespace();
                    r.expect(unit(b'{'))?;
                    r.skip_whitespace();
                    while r.can_read() && r.peek() != unit(b'}') {
                        r.skip_whitespace();
                        r.read_unquoted_string();
                        r.skip_whitespace();
                        r.expect(unit(b'='))?;
                        r.skip_whitespace();
                        r.read_boolean()?;
                        r.skip_whitespace();
                        if r.at(b',') {
                            r.skip();
                        }
                    }
                    r.skip_whitespace();
                    r.expect(unit(b'}'))?;
                    r.skip_whitespace();
                } else {
                    r.read_boolean()?;
                }
                any = true;
                r.skip_whitespace();
                if r.at(b',') {
                    r.skip();
                }
            }
            r.expect(unit(b'}'))?;
            if any {
                s.entities = false;
            }
            s.advancements = true;
        }
        _ => {
            // predicate
            inverted(r);
            read_location(r)?;
        }
    }
    Ok(())
}

/// `#` before a type's value: an entity type tag (`isTag`).
fn tag(r: &mut Reader) -> bool {
    r.skip_whitespace();
    if r.at(b'#') {
        r.skip();
        r.skip_whitespace();
        return true;
    }
    false
}

/// `MinMaxBounds.fromReader`: `a`, `a..`, `..b` or `a..b`; any error back at its start.
fn min_max<T: std::str::FromStr + PartialOrd + Copy>(r: &mut Reader, invalid: &'static str) -> Read<(Option<T>, Option<T>)> {
    if !r.can_read() {
        return Err(r.error("argument.range.empty", Vec::new()));
    }
    let i = r.cursor;
    let read = |r: &mut Reader| -> Read<(Option<T>, Option<T>)> {
        let min = range_number::<T>(r, invalid)?;
        let max = if r.can_read_n(2) && r.peek() == unit(b'.') && r.peek_at(1) == unit(b'.') {
            r.skip();
            r.skip();
            let max = range_number::<T>(r, invalid)?;
            if min.is_none() && max.is_none() {
                return Err(r.error("argument.range.empty", Vec::new()));
            }
            max
        } else {
            min
        };
        if min.is_none() && max.is_none() {
            return Err(r.error("argument.range.empty", Vec::new()));
        }
        if let (Some(a), Some(b)) = (min, max)
            && a > b
        {
            return Err(r.error("argument.range.swapped", Vec::new()));
        }
        Ok((min, max))
    };
    read(r).map_err(|mut e| {
        r.cursor = i;
        e.cursor = i;
        e
    })
}

/// A bound of a range: digits and minus signs, and points but the `..` between bounds.
fn range_number<T: std::str::FromStr>(r: &mut Reader, invalid: &'static str) -> Read<Option<T>> {
    let i = r.cursor;
    while r.can_read() && in_range_number(r, true) {
        r.skip();
    }
    let s = r.text(i, r.cursor);
    if s.is_empty() {
        return Ok(None);
    }
    s.parse().map(Some).map_err(|_| r.error(invalid, vec![Arg::Text(s)]))
}

fn in_range_number(r: &Reader, points: bool) -> bool {
    let c = r.peek();
    if (unit(b'0')..=unit(b'9')).contains(&c) || c == unit(b'-') {
        return true;
    }
    points && c == unit(b'.') && !(r.can_read_n(2) && r.peek_at(1) == unit(b'.'))
}

/// `WrappedMinMaxBounds.fromReader` (of floats): a range of angles, each wrapped to -180..180.
fn wrapped_min_max(r: &mut Reader) -> Read<(Option<f32>, Option<f32>)> {
    if !r.can_read() {
        return Err(r.error("argument.range.empty", Vec::new()));
    }
    let i = r.cursor;
    let number = |r: &mut Reader| -> Read<Option<f32>> {
        let start = r.cursor;
        while r.can_read() && in_range_number(r, true) {
            r.skip();
        }
        let s = r.text(start, r.cursor);
        if s.is_empty() {
            return Ok(None);
        }
        s.parse::<f32>().map(|f| Some(wrap_degrees(f))).map_err(|_| r.error("parsing.double.invalid", vec![Arg::Text(s)]))
    };
    let min = number(r)?;
    let max = if r.can_read_n(2) && r.peek() == unit(b'.') && r.peek_at(1) == unit(b'.') {
        r.skip();
        r.skip();
        number(r)?
    } else {
        min
    };
    if min.is_none() && max.is_none() {
        r.cursor = i;
        return Err(r.error("argument.range.empty", Vec::new()));
    }
    Ok((min, max))
}

/// `Mth.wrapDegrees(float)`.
pub(crate) fn wrap_degrees(v: f32) -> f32 {
    let mut f = v % 360.0;
    if f >= 180.0 {
        f -= 360.0;
    }
    if f < -180.0 {
        f += 360.0;
    }
    f
}

/// The object of a selector read from `text`: its text (read again when it selects) and what the
/// selector's getters say.
pub(crate) fn selector_object(rt: &mut Rt, s: &Selector, text: &str) -> V {
    let o = rt.new_object(SELECTOR);
    let t = rt.string(text);
    rt.put_field(o, "$text", t);
    rt.put_field(o, "$max", V::I(s.max));
    rt.put_field(o, "$entities", V::I(i32::from(s.entities)));
    rt.put_field(o, "$current", V::I(i32::from(s.current)));
    rt.put_field(o, "$uses", V::I(i32::from(s.uses)));
    o
}

/// `EntityArgument`'s parse: a selector, of one target for `entity` and `player`, of players for
/// `player` and `players` (`@s` passes, being the source). Its errors are at the line's start, as
/// vanilla's.
fn entity(rt: &mut Rt, r: &mut Reader, kind: &str, selectors: bool) -> Read<V> {
    let start = r.cursor;
    let s = parse_selector(r, selectors)?;
    let single = matches!(kind, "EntityArgument.entity" | "EntityArgument.player");
    let players = matches!(kind, "EntityArgument.player" | "EntityArgument.players");
    if s.max > 1 && single {
        r.cursor = 0;
        return Err(r.error(if players { "argument.player.toomany" } else { "argument.entity.toomany" }, Vec::new()));
    }
    if s.entities && players && !s.current {
        r.cursor = 0;
        return Err(r.error("argument.player.entities", Vec::new()));
    }
    let text = r.text(start, r.cursor);
    Ok(selector_object(rt, &s, &text))
}

/// `GameProfileArgument`'s parse: a selector of players, or a name up to the next space.
fn profiles(rt: &mut Rt, r: &mut Reader, selectors: bool) -> Read<V> {
    let start = r.cursor;
    let o = rt.new_object(PROFILES);
    if r.at(b'@') {
        let s = parse_selector(r, selectors)?;
        if s.entities {
            return Err(r.error("argument.player.entities", Vec::new()));
        }
        let text = r.text(start, r.cursor);
        let sel = selector_object(rt, &s, &text);
        rt.put_field(o, "$selector", sel);
    } else {
        while r.can_read() && r.peek() != unit(b' ') {
            r.skip();
        }
        let name = rt.string(&r.text(start, r.cursor));
        rt.put_field(o, "$name", name);
    }
    Ok(o)
}

/// The players a selector may find for `source`: the server's, the source's own first when the
/// host doesn't list it.
fn players(rt: &mut Rt, source: V) -> Vec<V> {
    let me = rt.get_field(source, "$player");
    let mut all = crate::events::server_players(rt);
    if me.r() != 0 && !all.contains(&me) && rt.class_of(me) == SERVER_PLAYER {
        all.insert(0, me);
    }
    all
}

/// A server player's name: the one the game gave them, else the host's.
pub(crate) fn player_name(rt: &Rt, p: V) -> String {
    if let Some(n) = rt.str_of(rt.get_field(p, "$name")) {
        return n.to_owned();
    }
    let (msb, lsb) = crate::tags::uuid_bits(rt, rt.get_field(p, "$uuid"));
    let uuid = (u128::from(msb as u64) << 64) | u128::from(lsb as u64);
    rt.host().and_then(|h| h.player_name(uuid)).unwrap_or_default()
}

fn feet(rt: &Rt, p: V) -> [f64; 3] {
    [rt.get_field(p, "$x").d(), rt.get_field(p, "$y").d(), rt.get_field(p, "$z").d()]
}

fn distance_sq(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

/// `EntitySelector.findEntities` (`findPlayers` when `players`) for `source`, over the server's
/// players: the selector's text read again. Selecting with `@` needs level 2
/// (`argument.entity.selector.not_allowed`, as `checkPermissions`).
pub(crate) fn select(rt: &mut Rt, selector: V, source: V, players_only: bool) -> Result<Vec<V>, Syntax> {
    let text = rt.str_of(rt.get_field(selector, "$text")).unwrap_or("").to_owned();
    let Ok(s) = parse_selector(&mut Reader::new(&text), true) else { return Ok(Vec::new()) };
    if s.uses && permission(rt, source) < 2 {
        return Err(Syntax { key: "argument.entity.selector.not_allowed", args: Vec::new(), cursor: -1 });
    }
    let all = players(rt, source);
    if let Some(name) = &s.name {
        return Ok(all.into_iter().filter(|p| player_name(rt, *p).eq_ignore_ascii_case(name)).take(1).collect());
    }
    if let Some(id) = s.uuid {
        return Ok(all.into_iter().filter(|p| crate::tags::uuid_bits(rt, rt.get_field(*p, "$uuid")) == id).take(1).collect());
    }
    let origin = [rt.get_field(source, "$x").d(), rt.get_field(source, "$y").d(), rt.get_field(source, "$z").d()];
    let pos = [0, 1, 2].map(|i| s.at[i].unwrap_or(origin[i]));
    // `dx dy dz`: a box from the position, which the player's (0.6 wide, 1.8 tall) must meet.
    let aabb = s.delta.iter().any(Option::is_some).then(|| {
        let d = s.delta.map(|d| d.unwrap_or(0.0));
        let lo = d.map(|d| if d < 0.0 { d } else { 0.0 });
        let hi = d.map(|d| if d < 0.0 { 0.0 } else { d } + 1.0);
        [pos[0] + lo[0], pos[1] + lo[1], pos[2] + lo[2], pos[0] + hi[0], pos[1] + hi[1], pos[2] + hi[2]]
    });
    let pass = |rt: &Rt, p: V| -> bool {
        if !s.players_pass {
            return false;
        }
        let name = player_name(rt, p);
        if s.names.iter().any(|(n, negate)| (name == *n) == *negate) {
            return false;
        }
        let at = feet(rt, p);
        if let Some(b) = aabb {
            let me = [at[0] - 0.3, at[1], at[2] - 0.3, at[0] + 0.3, at[1] + 1.8, at[2] + 0.3];
            if !(b[0] < me[3] && b[3] > me[0] && b[1] < me[4] && b[4] > me[1] && b[2] < me[5] && b[5] > me[2]) {
                return false;
            }
        }
        if let Some((min, max)) = s.distance {
            let d = distance_sq(at, pos);
            if min.is_some_and(|m| m * m > d) || max.is_some_and(|m| m * m < d) {
                return false;
            }
        }
        true
    };
    if s.current {
        let me = rt.get_field(source, "$player");
        let ok = me.r() != 0 && (!players_only || rt.instance_of(me, SERVER_PLAYER)) && pass(rt, me);
        return Ok(if ok { vec![me] } else { Vec::new() });
    }
    let limit = if s.order == Order::Arbitrary { s.max } else { i32::MAX };
    let mut found: Vec<V> = Vec::new();
    for p in all {
        if pass(rt, p) {
            found.push(p);
            if found.len() as i64 >= i64::from(limit) {
                break;
            }
        }
    }
    if found.len() > 1 {
        match s.order {
            Order::Arbitrary => {}
            Order::Nearest => found.sort_by(|a, b| distance_sq(feet(rt, *a), pos).total_cmp(&distance_sq(feet(rt, *b), pos))),
            Order::Furthest => found.sort_by(|a, b| distance_sq(feet(rt, *b), pos).total_cmp(&distance_sq(feet(rt, *a), pos))),
            // `Collections.shuffle`: from the last place down, each swapped with one at random.
            Order::Random => {
                for i in (2..=found.len()).rev() {
                    let j = rt.host_mut().and_then(|h| h.random()).map_or(0, |r| r.next_int_bound(i as i32)) as usize;
                    found.swap(i - 1, j.min(i - 1));
                }
            }
        }
    }
    found.truncate(usize::try_from(s.max).unwrap_or(usize::MAX));
    Ok(found)
}

/// `GameProfileArgument.Result.getNames(source)`: the selected players' profiles, or the one of the
/// player of that name (`argument.player.unknown` when nobody of that name is here).
pub(crate) fn profiles_of(rt: &mut Rt, result: V, source: V) -> Result<Vec<V>, Syntax> {
    let selector = rt.get_field(result, "$selector");
    let found = if selector.r() != 0 {
        let found = select(rt, selector, source, true)?;
        if found.is_empty() {
            return Err(Syntax { key: "argument.entity.notfound.player", args: Vec::new(), cursor: -1 });
        }
        found
    } else {
        let name = rt.str_of(rt.get_field(result, "$name")).unwrap_or("").to_owned();
        let all = players(rt, source);
        let found: Vec<V> = all.into_iter().filter(|p| player_name(rt, *p).eq_ignore_ascii_case(&name)).take(1).collect();
        if found.is_empty() {
            return Err(Syntax { key: "argument.player.unknown", args: Vec::new(), cursor: -1 });
        }
        found
    };
    Ok(found.into_iter().map(|p| api::call(rt, "net/minecraft/server/level/ServerPlayer.getGameProfile.()Lcom/mojang/authlib/GameProfile;", &[p])).collect())
}

// ---- Messages ----

/// `MessageArgument.Message.parseText`: the rest of the line (at most 256 characters), the
/// selectors in it kept where the source may use them (an `@` that starts none is text).
fn message(rt: &mut Rt, r: &mut Reader, selectors: bool) -> Read<V> {
    let left = r.total() - r.cursor;
    if left > 256 {
        return Err(Syntax { key: "argument.message.too_long", args: vec![Arg::Int(left), Arg::Int(256)], cursor: -1 });
    }
    let text = r.remaining();
    let start = r.cursor;
    let mut parts = Vec::new();
    if !selectors {
        r.cursor = r.total();
    } else {
        while r.can_read() {
            if !r.at(b'@') {
                r.skip();
                continue;
            }
            let j = r.cursor;
            match parse_selector(r, true) {
                Ok(s) => parts.push((j - start, r.cursor - start, s, r.text(j, r.cursor))),
                Err(e) if matches!(e.key, "argument.entity.selector.missing" | "argument.entity.selector.unknown") => r.cursor = j + 1,
                Err(e) => return Err(e),
            }
        }
    }
    let m = rt.new_object(MESSAGE);
    let t = rt.string(&text);
    rt.put_field(m, "$text", t);
    let items: Vec<V> = parts
        .into_iter()
        .map(|(a, b, s, text)| {
            let o = selector_object(rt, &s, &text);
            rt.put_field(o, "$start", V::I(a));
            rt.put_field(o, "$end", V::I(b));
            o
        })
        .collect();
    let list = crate::jutil::new_list(rt, items);
    rt.put_field(m, "$parts", list);
    Ok(m)
}

/// `MessageArgument.getMessage`: the message's text, each selector in it the names of what it
/// finds (`, ` between them) for a source that may use selectors.
pub(crate) fn message_component(rt: &mut Rt, m: V, source: V) -> Result<V, Syntax> {
    let text: Vec<u16> = rt.str_of(rt.get_field(m, "$text")).unwrap_or("").encode_utf16().collect();
    let parts = crate::jutil::elements(rt, rt.get_field(m, "$parts"));
    let mut out = String::new();
    if parts.is_empty() || permission(rt, source) < 2 {
        out = String::from_utf16_lossy(&text);
    } else {
        let mut at = 0usize;
        for p in parts {
            let (a, b) = (rt.get_field(p, "$start").i().max(0) as usize, rt.get_field(p, "$end").i().max(0) as usize);
            let (a, b) = (a.min(text.len()), b.min(text.len()));
            if at < a {
                out.push_str(&String::from_utf16_lossy(&text[at..a]));
            }
            let found = select(rt, p, source, false)?;
            let names: Vec<String> = found.into_iter().map(|e| player_name(rt, e)).collect();
            out.push_str(&names.join(", "));
            at = b;
        }
        if at < text.len() {
            out.push_str(&String::from_utf16_lossy(&text[at..]));
        }
    }
    let s = rt.string(&out);
    Ok(crate::api_common::component(rt, s, false, V::NULL))
}

// ---- Coordinates ----

/// `WorldCoordinate.parseInt` (`double`: `parseDouble`, a whole number then the block's centre when
/// `centre`): `~` relative, then a number unless a space or the line's end follows.
fn world_coordinate(r: &mut Reader, double: bool, centre: bool) -> Read<(f64, bool)> {
    if r.at(b'^') {
        return Err(r.error("argument.pos.mixed", Vec::new()));
    }
    if !r.can_read() {
        return Err(r.error(if double { "argument.pos.missing.double" } else { "argument.pos.missing.int" }, Vec::new()));
    }
    let relative = r.at(b'~');
    if relative {
        r.skip();
    }
    let i = r.cursor;
    let value = if r.can_read() && r.peek() != unit(b' ') {
        if double || relative {
            r.read_double()?
        } else {
            f64::from(r.read_int()?)
        }
    } else {
        0.0
    };
    if double {
        let s = r.text(i, r.cursor);
        if relative && s.is_empty() {
            return Ok((0.0, true));
        }
        if !s.contains('.') && !relative && centre {
            return Ok((value + 0.5, false));
        }
    }
    Ok((value, relative))
}

/// `WorldCoordinates.parseInt`/`parseDouble`: three coordinates; which were relative, as bits.
fn world_three(r: &mut Reader, double: bool, centre: bool) -> Read<([f64; 3], i32)> {
    let i = r.cursor;
    let x = world_coordinate(r, double, centre)?;
    let next = |r: &mut Reader, centre: bool| {
        if !r.at(b' ') {
            r.cursor = i;
            return Err(r.error("argument.pos3d.incomplete", Vec::new()));
        }
        r.skip();
        world_coordinate(r, double, centre)
    };
    let y = next(r, false)?;
    let z = next(r, centre)?;
    Ok(([x.0, y.0, z.0], i32::from(x.1) | i32::from(y.1) << 1 | i32::from(z.1) << 2))
}

/// Two world coordinates (`ColumnPosArgument`, `Vec2Argument`, `RotationArgument`).
fn world_two(r: &mut Reader, double: bool, centre: bool, incomplete: &'static str) -> Read<((f64, bool), (f64, bool))> {
    let i = r.cursor;
    if !r.can_read() {
        return Err(r.error(incomplete, Vec::new()));
    }
    let a = world_coordinate(r, double, centre)?;
    if !r.at(b' ') {
        r.cursor = i;
        return Err(r.error(incomplete, Vec::new()));
    }
    r.skip();
    let b = world_coordinate(r, double, centre)?;
    Ok((a, b))
}

/// `LocalCoordinates.parse`: `^left ^up ^forwards`.
fn local_three(r: &mut Reader) -> Read<[f64; 3]> {
    let i = r.cursor;
    let one = |r: &mut Reader| -> Read<f64> {
        if !r.can_read() {
            return Err(r.error("argument.pos.missing.double", Vec::new()));
        }
        if r.peek() != unit(b'^') {
            r.cursor = i;
            return Err(r.error("argument.pos.mixed", Vec::new()));
        }
        r.skip();
        if r.can_read() && r.peek() != unit(b' ') {
            r.read_double()
        } else {
            Ok(0.0)
        }
    };
    let left = one(r)?;
    let mut v = [left, 0.0, 0.0];
    for slot in &mut v[1..] {
        if !r.at(b' ') {
            r.cursor = i;
            return Err(r.error("argument.pos3d.incomplete", Vec::new()));
        }
        r.skip();
        *slot = one(r)?;
    }
    Ok(v)
}

fn world_object(rt: &mut Rt, v: [f64; 3], relative: i32) -> V {
    let c = rt.new_object(WORLD_COORDINATES);
    for (field, x) in ["$cx", "$cy", "$cz"].into_iter().zip(v) {
        rt.put_field(c, field, V::D(x));
    }
    rt.put_field(c, "$relative", V::I(relative));
    c
}

fn local_object(rt: &mut Rt, v: [f64; 3]) -> V {
    let c = rt.new_object(LOCAL_COORDINATES);
    for (field, x) in ["$cx", "$cy", "$cz"].into_iter().zip(v) {
        rt.put_field(c, field, V::D(x));
    }
    c
}

/// `Coordinates.getPosition(source)`: world coordinates each from the source's (when relative),
/// local ones along the source's look from where it stands.
pub(crate) fn position(rt: &Rt, c: V, source: V) -> [f64; 3] {
    let v = [rt.get_field(c, "$cx").d(), rt.get_field(c, "$cy").d(), rt.get_field(c, "$cz").d()];
    let origin = [rt.get_field(source, "$x").d(), rt.get_field(source, "$y").d(), rt.get_field(source, "$z").d()];
    if rt.class_of(c) == LOCAL_COORDINATES {
        return local_position(v, origin, rt.get_field(source, "$yaw").f(), rt.get_field(source, "$pitch").f());
    }
    let relative = rt.get_field(c, "$relative").i();
    [0, 1, 2].map(|i| if relative >> i & 1 != 0 { v[i] + origin[i] } else { v[i] })
}

/// `Coordinates.getBlockPos(source)`: the block the position is in.
pub(crate) fn block_pos(rt: &Rt, c: V, source: V) -> [i32; 3] {
    position(rt, c, source).map(|v| v.floor() as i32)
}

/// `Coordinates.getRotation(source)`: (pitch, yaw), each from the source's when relative; none
/// for local coordinates.
pub(crate) fn rotation(rt: &Rt, c: V, source: V) -> [f32; 2] {
    if rt.class_of(c) == LOCAL_COORDINATES {
        return [0.0, 0.0];
    }
    let relative = rt.get_field(c, "$relative").i();
    let origin = [f64::from(rt.get_field(source, "$pitch").f()), f64::from(rt.get_field(source, "$yaw").f())];
    let v = [rt.get_field(c, "$cx").d(), rt.get_field(c, "$cy").d()];
    [0, 1].map(|i| (if relative >> i & 1 != 0 { v[i] + origin[i] } else { v[i] }) as f32)
}

/// Whether a coordinate is relative (`isXRelative`...): local ones all are.
pub(crate) fn is_relative(rt: &Rt, c: V, axis: usize) -> bool {
    rt.class_of(c) == LOCAL_COORDINATES || rt.get_field(c, "$relative").i() >> axis & 1 != 0
}

/// `LocalCoordinates.getPosition`: `left up forwards` along the look (`Mth`'s table sines, in
/// float as vanilla's).
fn local_position([left, up, forwards]: [f64; 3], origin: [f64; 3], yaw: f32, pitch: f32) -> [f64; 3] {
    let rad = (std::f64::consts::PI / 180.0) as f32;
    let sin = |a: f32| crate::api_common::mth_sin(V::F(a));
    let cos = |a: f32| crate::api_common::mth_cos(V::F(a));
    let (f, f1) = (cos((yaw + 90.0) * rad), sin((yaw + 90.0) * rad));
    let (f2, f3) = (cos(-pitch * rad), sin(-pitch * rad));
    let (f4, f5) = (cos((-pitch + 90.0) * rad), sin((-pitch + 90.0) * rad));
    let a = [f64::from(f * f2), f64::from(f3), f64::from(f1 * f2)];
    let b = [f64::from(f * f4), f64::from(f5), f64::from(f1 * f4)];
    let c = [-(a[1] * b[2] - a[2] * b[1]), -(a[2] * b[0] - a[0] * b[2]), -(a[0] * b[1] - a[1] * b[0])];
    [0, 1, 2].map(|i| origin[i] + (a[i] * forwards + b[i] * up + c[i] * left))
}

/// `AngleArgument.parse`: `~` relative, then a float unless a space or the line's end follows.
fn angle(rt: &mut Rt, r: &mut Reader) -> Read<V> {
    if !r.can_read() {
        return Err(r.error("argument.angle.incomplete", Vec::new()));
    }
    let relative = r.at(b'~');
    if relative {
        r.skip();
    }
    let f = if r.can_read() && r.peek() != unit(b' ') { r.read_float()? } else { 0.0 };
    if !f.is_finite() {
        return Err(r.error("argument.angle.invalid", Vec::new()));
    }
    let a = rt.new_object(SINGLE_ANGLE);
    rt.put_field(a, "$angle", V::F(f));
    rt.put_field(a, "$relative", V::I(i32::from(relative)));
    Ok(a)
}

/// `SingleAngle.getAngle(source)`: wrapped to -180..180, from the source's yaw when relative.
pub(crate) fn angle_of(rt: &Rt, a: V, source: V) -> f32 {
    let f = rt.get_field(a, "$angle").f();
    wrap_degrees(if rt.get_field(a, "$relative").i() != 0 { f + rt.get_field(source, "$yaw").f() } else { f })
}

// ---- Ids, items, blocks, SNBT ----

/// `ResourceLocation.read`: the characters an id may have, `minecraft` its namespace when it names
/// none; `argument.id.invalid` back at its start.
pub(crate) fn read_location(r: &mut Reader) -> Read<String> {
    let i = r.cursor;
    while r.can_read() && matches!(r.peek(), 0x30..=0x39 | 0x61..=0x7A | 0x5F | 0x3A | 0x2F | 0x2E | 0x2D) {
        r.skip();
    }
    let s = r.text(i, r.cursor);
    let path_ok = |p: &str| p.chars().all(|c| matches!(c, '0'..='9' | 'a'..='z' | '_' | '.' | '-' | '/'));
    let id = match s.split_once(':') {
        Some(("", path)) if path_ok(path) => Some(format!("minecraft:{path}")),
        Some((ns, path)) if ns.chars().all(|c| matches!(c, '0'..='9' | 'a'..='z' | '_' | '.' | '-')) && path_ok(path) => Some(s.clone()),
        None if path_ok(&s) => Some(format!("minecraft:{s}")),
        _ => None,
    };
    id.ok_or_else(|| {
        r.cursor = i;
        r.error("argument.id.invalid", Vec::new())
    })
}

/// `ItemParser.parse`: an item the game knows, then its `[components]`.
fn item(rt: &mut Rt, r: &mut Reader) -> Read<V> {
    let start = r.cursor;
    let id = read_location(r)?;
    if rt.host().is_some_and(|h| h.item_stack_limit(&id).is_none()) {
        r.cursor = start;
        return Err(r.error("argument.item.id.invalid", vec![Arg::Text(id)]));
    }
    let components_at = r.cursor;
    if r.at(b'[') {
        components(r)?;
    }
    let components = r.text(components_at, r.cursor);
    let input = rt.new_object(ITEM_INPUT);
    let item = crate::api_common::item(rt, &id);
    rt.put_field(input, "$item", item);
    let s = rt.string(&id);
    rt.put_field(input, "$id", s);
    let c = rt.string(&components);
    rt.put_field(input, "$components", c);
    Ok(input)
}

/// `ItemParser.readComponents`: `[id=value, !id, ...]`, each component once.
fn components(r: &mut Reader) -> Read<()> {
    r.expect(unit(b'['))?;
    let mut seen: Vec<String> = Vec::new();
    let mut component = |r: &mut Reader| -> Read<()> {
        if !r.can_read() {
            return Err(r.error("arguments.item.component.expected", Vec::new()));
        }
        let id = read_location(r)?;
        if seen.contains(&id) {
            return Err(Syntax { key: "arguments.item.component.repeated", args: vec![Arg::Text(id)], cursor: -1 });
        }
        seen.push(id);
        Ok(())
    };
    while r.can_read() && r.peek() != unit(b']') {
        r.skip_whitespace();
        if r.at(b'!') {
            r.skip();
            component(r)?;
            r.skip_whitespace();
        } else {
            component(r)?;
            r.skip_whitespace();
            r.expect(unit(b'='))?;
            r.skip_whitespace();
            snbt_value(r, 0)?;
            r.skip_whitespace();
        }
        if !r.at(b',') {
            break;
        }
        r.skip();
        r.skip_whitespace();
        if !r.can_read() {
            return Err(r.error("arguments.item.component.expected", Vec::new()));
        }
    }
    r.expect(unit(b']'))
}

/// `BlockStateParser.parseForBlock` (no tags, NBT allowed): an id, its `[properties]` and
/// `{nbt}`.
fn block(rt: &mut Rt, r: &mut Reader) -> Read<V> {
    if r.at(b'#') {
        return Err(r.error("argument.block.tag.disallowed", Vec::new()));
    }
    let id = read_location(r)?;
    let mut text = id.clone();
    if r.at(b'[') {
        let props = properties(r, &id)?;
        if !props.is_empty() {
            let joined: Vec<String> = props.iter().map(|(k, v)| format!("{k}={v}")).collect();
            text.push_str(&format!("[{}]", joined.join(",")));
        }
    }
    let nbt_at = r.cursor;
    if r.at(b'{') {
        snbt_struct(r, 0)?;
    }
    let nbt = r.text(nbt_at, r.cursor);
    let input = rt.new_object(BLOCK_INPUT);
    let state = api::state(rt, &text);
    rt.put_field(input, "$state", state);
    let s = rt.string(&text);
    rt.put_field(input, "$text", s);
    let n = rt.string(&nbt);
    rt.put_field(input, "$nbt", n);
    Ok(input)
}

/// `readProperties`: `[name=value, ...]`, each name once.
fn properties(r: &mut Reader, id: &str) -> Read<Vec<(String, String)>> {
    r.skip();
    r.skip_whitespace();
    let mut out: Vec<(String, String)> = Vec::new();
    while r.can_read() && r.peek() != unit(b']') {
        r.skip_whitespace();
        let i = r.cursor;
        let name = r.read_string()?;
        if out.iter().any(|(n, _)| *n == name) {
            r.cursor = i;
            return Err(r.error("argument.block.property.duplicate", vec![Arg::Text(id.to_owned()), Arg::Text(name)]));
        }
        r.skip_whitespace();
        if !r.at(b'=') {
            return Err(r.error("argument.block.property.novalue", vec![Arg::Text(id.to_owned()), Arg::Text(name)]));
        }
        r.skip();
        r.skip_whitespace();
        let value = r.read_string()?;
        out.push((name, value));
        r.skip_whitespace();
        if r.can_read() {
            if r.peek() != unit(b',') {
                if r.peek() != unit(b']') {
                    return Err(r.error("argument.block.property.unclosed", Vec::new()));
                }
                break;
            }
            r.skip();
        }
    }
    if !r.can_read() {
        return Err(r.error("argument.block.property.unclosed", Vec::new()));
    }
    r.skip();
    Ok(out)
}

/// The deepest SNBT read here (`TagParser` itself has no limit; the stack has).
const SNBT_DEPTH: u32 = 256;

/// `TagParser.readStruct`: `{key: value, ...}`.
pub(crate) fn snbt_struct(r: &mut Reader, depth: u32) -> Read<()> {
    if depth > SNBT_DEPTH {
        return Err(r.error("argument.nbt.expected.value", Vec::new()));
    }
    r.expect(unit(b'{'))?;
    r.skip_whitespace();
    while r.can_read() && r.peek() != unit(b'}') {
        let i = r.cursor;
        r.skip_whitespace();
        if !r.can_read() {
            return Err(r.error("argument.nbt.expected.key", Vec::new()));
        }
        if r.read_string()?.is_empty() {
            r.cursor = i;
            return Err(r.error("argument.nbt.expected.key", Vec::new()));
        }
        expect_after_space(r, b':')?;
        snbt_value(r, depth + 1)?;
        if !separator(r) {
            break;
        }
        if !r.can_read() {
            return Err(r.error("argument.nbt.expected.key", Vec::new()));
        }
    }
    expect_after_space(r, b'}')
}

/// `TagParser.readValue`: a compound, a list or array, a quoted string or an unquoted word.
fn snbt_value(r: &mut Reader, depth: u32) -> Read<()> {
    r.skip_whitespace();
    if !r.can_read() {
        return Err(r.error("argument.nbt.expected.value", Vec::new()));
    }
    if r.at(b'{') {
        return snbt_struct(r, depth);
    }
    if !r.at(b'[') {
        let i = r.cursor;
        if is_quoted_string_start(r.peek()) {
            r.read_quoted_string()?;
        } else if r.read_unquoted_string().is_empty() {
            r.cursor = i;
            return Err(r.error("argument.nbt.expected.value", Vec::new()));
        }
        return Ok(());
    }
    if depth > SNBT_DEPTH {
        return Err(r.error("argument.nbt.expected.value", Vec::new()));
    }
    // `[B; ...]`, `[I; ...]`, `[L; ...]`: an array.
    let array = r.can_read_n(3) && !is_quoted_string_start(r.peek_at(1)) && r.peek_at(2) == unit(b';');
    expect_after_space(r, b'[')?;
    if array {
        let i = r.cursor;
        let c = r.peek();
        r.skip();
        r.skip();
        r.skip_whitespace();
        if !r.can_read() {
            return Err(r.error("argument.nbt.expected.value", Vec::new()));
        }
        if !matches!(u8::try_from(c), Ok(b'B' | b'I' | b'L')) {
            r.cursor = i;
            return Err(r.error("argument.nbt.array.invalid", vec![Arg::Text(String::from_utf16_lossy(&[c]))]));
        }
    } else {
        r.skip_whitespace();
        if !r.can_read() {
            return Err(r.error("argument.nbt.expected.value", Vec::new()));
        }
    }
    while r.peek() != unit(b']') {
        snbt_value(r, depth + 1)?;
        if !separator(r) {
            break;
        }
        if !r.can_read() {
            return Err(r.error("argument.nbt.expected.value", Vec::new()));
        }
    }
    expect_after_space(r, b']')
}

/// `TagParser.expect`: spaces, then `c`.
fn expect_after_space(r: &mut Reader, c: u8) -> Read<()> {
    r.skip_whitespace();
    r.expect(unit(c))
}

/// `hasElementSeparator`: a comma between spaces.
fn separator(r: &mut Reader) -> bool {
    r.skip_whitespace();
    if r.at(b',') {
        r.skip();
        r.skip_whitespace();
        return true;
    }
    false
}

// ---- Times, UUIDs, colours ----

/// `TimeArgument.parse`: a float and its unit (`d`, `s`, `t` or none), rounded to ticks.
fn time(r: &mut Reader, minimum: i32) -> Read<i32> {
    let f = r.read_float()?;
    let per = match r.read_unquoted_string().as_str() {
        "d" => 24000,
        "s" => 20,
        "t" | "" => 1,
        _ => return Err(r.error("argument.time.invalid_unit", Vec::new())),
    };
    // `Math.round(float)`: the nearest whole, halves up.
    let ticks = (f64::from(f * per as f32) + 0.5).floor() as i32;
    if ticks < minimum {
        return Err(r.error("argument.time.tick_count_too_low", vec![Arg::Int(minimum), Arg::Int(ticks)]));
    }
    Ok(ticks)
}

/// `UuidArgument.parse`: the hex digits and dashes there, read as Java's `UUID.fromString`.
fn uuid(rt: &mut Rt, r: &mut Reader) -> Read<V> {
    let rest = r.remaining();
    let len = rest.chars().take_while(|c| c.is_ascii_hexdigit() || *c == '-').count();
    match (len > 0).then(|| java_uuid(&rest[..len])).flatten() {
        Some((msb, lsb)) => {
            r.cursor += len as i32;
            Ok(crate::tags::uuid(rt, msb, lsb))
        }
        None => Err(r.error("argument.uuid.invalid", Vec::new())),
    }
}

/// Java's `UUID.fromString`: five groups of hex digits between four dashes (at most 36
/// characters), each group a `long` cut to its width.
pub(crate) fn java_uuid(s: &str) -> Option<(i64, i64)> {
    if s.encode_utf16().count() > 36 {
        return None;
    }
    let groups: Vec<&str> = s.split('-').collect();
    let [a, b, c, d, e] = groups.as_slice() else { return None };
    // `Long.parseLong(group, 16)`: a `+` may lead; no more than a long holds.
    let long = |g: &str| -> Option<i64> {
        let digits = g.strip_prefix('+').unwrap_or(g);
        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let v = digits.chars().try_fold(0u128, |acc, c| {
            let v = acc * 16 + u128::from(c.to_digit(16)?);
            (v <= i64::MAX as u128).then_some(v)
        })?;
        Some(v as i64)
    };
    let msb = (long(a)? & 0xffff_ffff) << 32 | (long(b)? & 0xffff) << 16 | (long(c)? & 0xffff);
    let lsb = (long(d)? & 0xffff) << 48 | (long(e)? & 0xffff_ffff_ffff);
    Some((msb, lsb))
}

/// `ColorArgument.parse`: a colour by its name in any case, its letters alone (`ChatFormatting
/// .getByName`); a format (`bold`) is no colour.
fn colour(rt: &mut Rt, r: &mut Reader) -> Read<V> {
    let s = r.read_unquoted_string();
    let clean: String = s.to_ascii_lowercase().chars().filter(char::is_ascii_lowercase).collect();
    let found = COLOURS.iter().position(|c| c.replace('_', "") == clean).map(|i| (COLOURS[i].to_ascii_uppercase(), i as i32));
    let Some((constant, ordinal)) = found.or_else(|| (clean == "reset").then(|| ("RESET".to_owned(), 21))) else {
        return Err(r.error("argument.color.invalid", vec![Arg::Text(s)]));
    };
    let constant = rt.static_key(&constant);
    Ok(api::api_enum(rt, "net/minecraft/ChatFormatting", constant, ordinal))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(r: Read<impl std::fmt::Debug>) -> (&'static str, Vec<Arg>, i32) {
        let e = r.expect_err("an error");
        (e.key, e.args, e.cursor)
    }

    /// Selectors as `EntitySelectorParser` reads them: each letter's kind, options with their
    /// values, and its errors where vanilla's cursor stands (an unknown option back at its name,
    /// one not applicable after it, a negative distance back at its value).
    #[test]
    fn selectors_read_and_fail_as_vanillas_parser() {
        let s = parse_selector(&mut Reader::new("@e[type=player,limit=2,sort=nearest]"), true).unwrap();
        assert_eq!((s.max, s.entities, s.order, s.players_pass), (2, false, Order::Nearest, true));
        let s = parse_selector(&mut Reader::new("@a[distance=..5,x=1,dz=-2,name=!Steve]"), true).unwrap();
        assert_eq!((s.distance, s.at[0], s.delta[2], s.names.clone()), (Some((None, Some(5.0))), Some(1.0), Some(-2.0), vec![("Steve".to_owned(), true)]));
        let s = parse_selector(&mut Reader::new("Alex"), false).unwrap();
        assert_eq!((s.name.as_deref(), s.max, s.uses), (Some("Alex"), 1, false));
        let s = parse_selector(&mut Reader::new("0-0-0-0-1"), false).unwrap();
        assert_eq!(s.uuid, Some((0, 1)));
        assert_eq!(err(parse_selector(&mut Reader::new("@a"), false)), ("argument.entity.selector.not_allowed", vec![], 0));
        assert_eq!(err(parse_selector(&mut Reader::new("@x"), true)), ("argument.entity.selector.unknown", vec![Arg::Text("@x".into())], 1));
        assert_eq!(err(parse_selector(&mut Reader::new("@"), true)), ("argument.entity.selector.missing", vec![], 1));
        assert_eq!(err(parse_selector(&mut Reader::new("@e[colour=red]"), true)), ("argument.entity.options.unknown", vec![Arg::Text("colour".into())], 3));
        assert_eq!(err(parse_selector(&mut Reader::new("@a[type=zombie]"), true)), ("argument.entity.options.inapplicable", vec![Arg::Text("type".into())], 7));
        assert_eq!(err(parse_selector(&mut Reader::new("@s[limit=2]"), true)).0, "argument.entity.options.inapplicable");
        assert_eq!(err(parse_selector(&mut Reader::new("@e[limit=0]"), true)), ("argument.entity.options.limit.toosmall", vec![], 9));
        assert_eq!(err(parse_selector(&mut Reader::new("@e[distance=-1]"), true)), ("argument.entity.options.distance.negative", vec![], 12));
        assert_eq!(err(parse_selector(&mut Reader::new("@e[distance=5..1]"), true)), ("argument.range.swapped", vec![], 12));
        assert_eq!(err(parse_selector(&mut Reader::new("@e[distance=..]"), true)), ("argument.range.empty", vec![], 12));
        assert_eq!(err(parse_selector(&mut Reader::new("@e[sort=up]"), true)), ("argument.entity.options.sort.irreversible", vec![Arg::Text("up".into())], 8));
        assert_eq!(err(parse_selector(&mut Reader::new("@e[gamemode=hard]"), true)), ("argument.entity.options.mode.invalid", vec![Arg::Text("hard".into())], 12));
        assert_eq!(err(parse_selector(&mut Reader::new("@e[limit]"), true)), ("argument.entity.options.valueless", vec![Arg::Text("limit".into())], 3));
        assert_eq!(err(parse_selector(&mut Reader::new("@e[limit=1"), true)), ("argument.entity.options.unterminated", vec![], 10));
        assert_eq!(err(parse_selector(&mut Reader::new("@e[nbt={a:1]"), true)).0, "parsing.expected");
        let s = parse_selector(&mut Reader::new("@e[nbt={Tags:[\"a\",'b'],x:{y:[I;1,2]}},scores={k=1..}]"), true).unwrap();
        assert!(s.entities);
        assert_eq!(err(parse_selector(&mut Reader::new("ThisNameIsTooLong17"), false)), ("argument.entity.invalid", vec![], 0));
    }

    /// Coordinates as `WorldCoordinates` and `LocalCoordinates` read and resolve them: whole numbers
    /// a block's centre where the type says, relative ones from the source, local ones along its
    /// look; an incomplete or mixed set fails at vanilla's place.
    #[test]
    fn coordinates_read_and_resolve_as_vanillas() {
        let mut rt = Rt::new();
        let source = rt.new_object("net/minecraft/commands/CommandSourceStack");
        for (f, v) in [("$x", 10.5), ("$y", 64.0), ("$z", -3.5)] {
            rt.put_field(source, f, V::D(v));
        }
        rt.put_field(source, "$yaw", V::F(90.0));
        rt.put_field(source, "$pitch", V::F(0.0));
        let read = |rt: &mut Rt, kind: &str, line: &str| parse(rt, kind, &[], &mut Reader::new(line), true).unwrap();
        let c = read(&mut rt, "BlockPosArgument.blockPos", "~ ~1 ~-1").unwrap();
        assert_eq!(block_pos(&rt, c, source), [10, 65, -5]);
        // Looking along -x (yaw 90): `^ ^ ^2` is two blocks that way.
        let c = read(&mut rt, "BlockPosArgument.blockPos", "^ ^ ^2").unwrap();
        assert_eq!(block_pos(&rt, c, source), [8, 64, -4]);
        let c = read(&mut rt, "Vec3Argument.vec3", "1 2.5 ~").unwrap();
        assert_eq!(position(&rt, c, source), [1.5, 2.5, -3.5]);
        let c = read(&mut rt, "RotationArgument.rotation", "~10 -20").unwrap();
        assert_eq!(rotation(&rt, c, source), [-20.0, 100.0]);
        let c = read(&mut rt, "ColumnPosArgument.columnPos", "3 ~").unwrap();
        assert_eq!(block_pos(&rt, c, source), [3, 64, -4]);
        let e = |rt: &mut Rt, kind: &str, line: &str| err(read(rt, kind, line));
        assert_eq!(e(&mut rt, "BlockPosArgument.blockPos", "1.5 2 3"), ("parsing.int.invalid", vec![Arg::Text("1.5".into())], 0));
        assert_eq!(e(&mut rt, "BlockPosArgument.blockPos", "1 2"), ("argument.pos3d.incomplete", vec![], 0));
        assert_eq!(e(&mut rt, "BlockPosArgument.blockPos", "1 ^ 2"), ("argument.pos.mixed", vec![], 2));
        assert_eq!(e(&mut rt, "Vec3Argument.vec3", "^1 ~ ^"), ("argument.pos.mixed", vec![], 0));
        assert_eq!(e(&mut rt, "ColumnPosArgument.columnPos", "5"), ("argument.pos2d.incomplete", vec![], 0));
        assert_eq!(e(&mut rt, "AngleArgument.angle", ""), ("argument.angle.incomplete", vec![], 0));
        let a = read(&mut rt, "AngleArgument.angle", "~100").unwrap();
        assert_eq!(angle_of(&rt, a, source), -170.0);
    }

    /// The other vanilla types: bounds, times in units, UUIDs as Java reads them, colours by name,
    /// ids with their namespace, block states with properties and NBT, items' components.
    #[test]
    fn the_other_types_read_and_fail_as_vanillas() {
        let mut rt = Rt::new();
        let read = |rt: &mut Rt, kind: &str, given: &[V], line: &str| {
            let mut r = Reader::new(line);
            parse(rt, kind, given, &mut r, true).unwrap().map(|v| (v, r.remaining()))
        };
        assert_eq!(err(read(&mut rt, "IntegerArgumentType.integer", &[V::I(1), V::I(64)], "65")), ("argument.integer.big", vec![Arg::Int(64), Arg::Int(65)], 0));
        assert_eq!(err(read(&mut rt, "FloatArgumentType.floatArg", &[V::F(0.5)], "0.25")), ("argument.float.low", vec![Arg::Text("0.5".into()), Arg::Text("0.25".into())], 0));
        assert_eq!(read(&mut rt, "TimeArgument.time", &[], "1.5d").map(|r| r.0), Ok(V::I(36000)));
        assert_eq!(read(&mut rt, "TimeArgument.time", &[], "3s").map(|r| r.0), Ok(V::I(60)));
        assert_eq!(err(read(&mut rt, "TimeArgument.time", &[], "3m")), ("argument.time.invalid_unit", vec![], 2));
        assert_eq!(err(read(&mut rt, "TimeArgument.time", &[V::I(1)], "0.2t")), ("argument.time.tick_count_too_low", vec![Arg::Int(1), Arg::Int(0)], 4));
        assert_eq!(java_uuid("f81d4fae-7dec-11d0-a765-00a0c91e6bf6"), Some((0xf81d4fae7dec11d0u64 as i64, 0xa76500a0c91e6bf6u64 as i64)));
        assert_eq!(java_uuid("1-2-3-4-5"), Some((0x0000_0001_0002_0003, 0x0004_0000_0000_0005)));
        assert_eq!(java_uuid("+1-2-3-4-5"), Some((0x0000_0001_0002_0003, 0x0004_0000_0000_0005)));
        assert_eq!(java_uuid("1-2-3-4"), None);
        assert_eq!(java_uuid("8000000000000000-0-0-0-0"), None, "beyond a long");
        let (v, rest) = read(&mut rt, "UuidArgument.uuid", &[], "1-2-3-4-5 x").unwrap();
        assert_eq!((crate::tags::uuid_bits(&rt, v), rest.as_str()), ((0x0000_0001_0002_0003, 0x0004_0000_0000_0005), " x"));
        assert_eq!(err(read(&mut rt, "UuidArgument.uuid", &[], "xyz")), ("argument.uuid.invalid", vec![], 0));
        let (c, _) = read(&mut rt, "ColorArgument.color", &[], "Dark_Blue").unwrap();
        assert_eq!(rt.get_field(c, "$ordinal"), V::I(1));
        assert_eq!(err(read(&mut rt, "ColorArgument.color", &[], "bold")), ("argument.color.invalid", vec![Arg::Text("bold".into())], 4));
        let (id, _) = read(&mut rt, "ResourceLocationArgument.id", &[], ":stone").unwrap();
        assert_eq!(rt.str_of(id), Some("minecraft:stone"));
        assert_eq!(err(read(&mut rt, "ResourceLocationArgument.id", &[], "a:b:c")), ("argument.id.invalid", vec![], 0));
        let (b, rest) = read(&mut rt, "BlockStateArgument.block", &[], "create:shaft[ axis = x ]{x:1} 2").unwrap();
        assert_eq!((rt.str_of(rt.get_field(b, "$text")), rest.as_str()), (Some("create:shaft[axis=x]"), " 2"));
        assert_eq!(err(read(&mut rt, "BlockStateArgument.block", &[], "stone[axis=x")), ("argument.block.property.unclosed", vec![], 12));
        assert_eq!(err(read(&mut rt, "BlockStateArgument.block", &[], "#logs")), ("argument.block.tag.disallowed", vec![], 0));
        let (i, rest) = read(&mut rt, "ItemArgument.item", &[], "apple[damage=3,!food] 1").unwrap();
        assert_eq!((rt.str_of(rt.get_field(i, "$components")), rest.as_str()), (Some("[damage=3,!food]"), " 1"));
        assert_eq!(err(read(&mut rt, "ItemArgument.item", &[], "apple[damage=3,damage=4]")), ("arguments.item.component.repeated", vec![Arg::Text("minecraft:damage".into())], -1));
        assert_eq!(err(read(&mut rt, "ItemArgument.item", &[], "apple[damage=3")).0, "parsing.expected");
    }
}
