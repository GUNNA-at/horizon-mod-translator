//! Vanilla's crafting recipes as a translated mod builds on them, natively: `ShapedRecipe` and its
//! `ShapedRecipePattern`, read by vanilla's codec (Create's mechanical crafting is a shaped recipe
//! plus a flag, its pattern as large as NeoForge's `setCraftingSize` allows), a mod's subclass of
//! it, `SimpleCraftingRecipeSerializer` (a special recipe: the mod's factory given the category),
//! `CraftingBookCategory`, and `CraftingInput`, the grid a crafting recipe matches.
use crate::{Data, Rt, V, api, api_common, codecs, jutil, recipes};
use serde_json::{Map, Value as Json};

const SHAPED: &str = "net/minecraft/world/item/crafting/ShapedRecipe";
const PATTERN: &str = "net/minecraft/world/item/crafting/ShapedRecipePattern";
const SERIALIZER: &str = "net/minecraft/world/item/crafting/RecipeSerializer";
const SIMPLE: &str = "net/minecraft/world/item/crafting/SimpleCraftingRecipeSerializer";
const CATEGORY: &str = "net/minecraft/world/item/crafting/CraftingBookCategory";
const INPUT: &str = "net/minecraft/world/item/crafting/CraftingInput";
const NON_NULL_LIST: &str = "net/minecraft/core/NonNullList";
/// The class of vanilla's serializers here: `$id`, and `$codec` when this runtime reads its recipes.
const VANILLA_SERIALIZER: &str = "compat:recipe-serializer";
/// `CraftingBookCategory`'s constants in declaration order, with their serialized names.
const CATEGORIES: [(&str, &str); 4] = [("BUILDING", "building"), ("REDSTONE", "redstone"), ("EQUIPMENT", "equipment"), ("MISC", "misc")];
const MISC: usize = 3;
const FACTORY_CREATE: &str = "net/minecraft/world/item/crafting/SimpleCraftingRecipeSerializer$Factory.create.(Lnet/minecraft/world/item/crafting/CraftingBookCategory;)Lnet/minecraft/world/item/crafting/CraftingRecipe;";
/// NeoForge's largest pattern (`ShapedRecipePattern.setCraftingSize` only grows it); vanilla's 3×3.
const MAX_WIDTH: &str = "net/minecraft/world/item/crafting/ShapedRecipePattern.maxWidth";
const MAX_HEIGHT: &str = "net/minecraft/world/item/crafting/ShapedRecipePattern.maxHeight";

fn category(rt: &mut Rt, i: usize) -> V {
    api::api_enum(rt, CATEGORY, CATEGORIES[i].0, i as i32)
}

/// The largest pattern a shaped recipe may have: 3×3 unless a mod asked for more.
fn max_size(rt: &Rt) -> (usize, usize) {
    let get = |k: &str| match rt.statics.get(k) {
        Some(V::I(n)) => (*n as usize).max(3),
        _ => 3,
    };
    (get(MAX_WIDTH), get(MAX_HEIGHT))
}

fn non_null_list(rt: &mut Rt, items: Vec<V>) -> V {
    rt.alloc(NON_NULL_LIST, Data::List(items))
}

fn empty_stack(rt: &mut Rt) -> V {
    api_common::get_static(rt, "net/minecraft/world/item/ItemStack", "EMPTY").unwrap_or(V::NULL)
}

/// The serializer in `RecipeSerializer.<field>`: vanilla's own, which reads its recipes here when
/// this runtime models them (the shaped one).
fn vanilla_serializer(rt: &mut Rt, field: &str) -> Option<V> {
    let id = match field {
        "SHAPED_RECIPE" => "crafting_shaped",
        "SHAPELESS_RECIPE" => "crafting_shapeless",
        "SMELTING_RECIPE" => "smelting",
        "BLASTING_RECIPE" => "blasting",
        "SMOKING_RECIPE" => "smoking",
        "CAMPFIRE_COOKING_RECIPE" => "campfire_cooking",
        "STONECUTTER" => "stonecutting",
        "SMITHING_TRANSFORM" => "smithing_transform",
        "SMITHING_TRIM" => "smithing_trim",
        _ => return None,
    };
    let id = format!("minecraft:{id}");
    let shaped = field == "SHAPED_RECIPE";
    Some(rt.intern(format!("compat:recipe-serializer/{id}"), move |rt| {
        let o = rt.new_object(VANILLA_SERIALIZER);
        let s = rt.string(&id);
        rt.put_field(o, "$id", s);
        let codec = if shaped { codecs::native_map(rt, "shaped_recipe", &[]) } else { V::NULL };
        rt.put_field(o, "$codec", codec);
        o
    }))
}

// ---- Reading ----

/// Reads `o` with vanilla's map codec `name` ([`codecs::native_map`]); `extra` is what the codec
/// was made with (a simple serializer's factory).
pub fn decode_map(rt: &mut Rt, name: &str, extra: &[V], o: &Map<String, Json>) -> Result<V, String> {
    match name {
        "shaped_pattern" => decode_pattern(rt, o),
        "shaped_recipe" => {
            let group = match o.get("group") {
                None => String::new(),
                Some(Json::String(g)) => g.clone(),
                Some(j) => return Err(format!("group: Not a string: {j}")),
            };
            // `fieldOf("category").orElse(MISC)`: a missing or unknown category is misc.
            let cat = o.get("category").and_then(Json::as_str).and_then(|c| CATEGORIES.iter().position(|(_, n)| *n == c)).unwrap_or(MISC);
            let pattern = decode_pattern(rt, o)?;
            let result = recipes::decode_native(rt, "item_stack", o.get("result").ok_or("No key result in MapLike")?).map_err(|e| format!("result: {e}"))?;
            let show = match o.get("show_notification") {
                None => true,
                Some(Json::Bool(b)) => *b,
                Some(j) => return Err(format!("show_notification: Not a boolean: {j}")),
            };
            let r = rt.new_object(SHAPED);
            let (g, c) = (rt.string(&group), category(rt, cat));
            init_shaped(rt, r, g, c, pattern, result, show);
            Ok(r)
        }
        "simple_crafting" => {
            let cat = o.get("category").and_then(Json::as_str).and_then(|c| CATEGORIES.iter().position(|(_, n)| *n == c)).unwrap_or(MISC);
            let c = category(rt, cat);
            let factory = extra.first().copied().unwrap_or_default();
            let r = rt.call_functional(FACTORY_CREATE, factory, &[c]);
            if let Some(e) = rt.thrown.take() {
                return Err(format!("the recipe's factory threw {}", rt.class_of(e)));
            }
            Ok(r)
        }
        _ => Err(format!("no codec {name}")),
    }
}

/// `CraftingBookCategory.CODEC`'s reading of a category name.
pub fn decode_category(rt: &mut Rt, j: &Json) -> Result<V, String> {
    let s = j.as_str().ok_or_else(|| format!("Not a string: {j}"))?;
    match CATEGORIES.iter().position(|(_, n)| *n == s) {
        Some(i) => Ok(category(rt, i)),
        None => Err(format!("Unknown element name:{s}")),
    }
}

/// `ShapedRecipePattern.MAP_CODEC`: `key` and `pattern`, checked as vanilla checks them, the
/// pattern shrunk to the rows and columns it uses (`ShapedRecipePattern.unpack`).
fn decode_pattern(rt: &mut Rt, o: &Map<String, Json>) -> Result<V, String> {
    let (max_w, max_h) = max_size(rt);
    let rows: Vec<Vec<char>> = match o.get("pattern") {
        Some(Json::Array(a)) => a.iter().map(|r| r.as_str().map(|s| s.chars().collect()).ok_or_else(|| format!("pattern: Not a string: {r}"))).collect::<Result<_, _>>()?,
        Some(j) => return Err(format!("pattern: Not a list: {j}")),
        None => return Err("No key pattern in MapLike".into()),
    };
    if rows.len() > max_h {
        return Err(format!("pattern: Invalid pattern: too many rows, {max_h} is maximum"));
    }
    let Some(first) = rows.first() else { return Err("pattern: Invalid pattern: empty pattern not allowed".into()) };
    let width = first.len();
    for r in &rows {
        if r.len() > max_w {
            return Err(format!("pattern: Invalid pattern: too many columns, {max_w} is maximum"));
        }
        if r.len() != width {
            return Err("pattern: Invalid pattern: each row must be the same width".into());
        }
    }
    let Some(Json::Object(keys)) = o.get("key") else { return Err("No key key in MapLike".into()) };
    let mut key: Vec<(char, V)> = Vec::new();
    for (k, v) in keys {
        let mut cs = k.chars();
        let (Some(c), None) = (cs.next(), cs.next()) else {
            return Err(format!("key: Invalid key entry: '{k}' is an invalid symbol (must be 1 character only)."));
        };
        if c == ' ' {
            return Err("key: Invalid key entry: ' ' is a reserved symbol.".into());
        }
        let ing = recipes::decode_native(rt, "ingredient_nonempty", v).map_err(|e| format!("key: {e}"))?;
        key.push((c, ing));
    }
    let rows = shrink(&rows);
    let (h, w) = (rows.len(), rows.first().map_or(0, Vec::len));
    let empty = recipes::empty_ingredient(rt);
    let mut used: Vec<char> = Vec::new();
    let mut cells = Vec::with_capacity(w * h);
    for r in &rows {
        for &c in r {
            if c == ' ' {
                cells.push(empty);
                continue;
            }
            let Some(&(_, ing)) = key.iter().find(|(k, _)| *k == c) else {
                return Err(format!("Pattern references symbol '{c}' but it's not defined in the key"));
            };
            if !used.contains(&c) {
                used.push(c);
            }
            cells.push(ing);
        }
    }
    let unused: Vec<String> = key.iter().map(|(c, _)| *c).filter(|c| !used.contains(c)).map(String::from).collect();
    if !unused.is_empty() {
        return Err(format!("Key defines symbols that aren't used in pattern: [{}]", unused.join(", ")));
    }
    Ok(pattern(rt, w, h, cells))
}

/// Vanilla's `ShapedRecipePattern.shrink`: the blank rows above and below the pattern, and the
/// blank columns either side of it, go.
fn shrink(rows: &[Vec<char>]) -> Vec<Vec<char>> {
    let (mut left, mut right, mut top, mut bottom) = (usize::MAX, -1i64, 0, 0);
    for (i, r) in rows.iter().enumerate() {
        left = left.min(r.iter().position(|&c| c != ' ').unwrap_or(r.len()));
        let last = r.iter().rposition(|&c| c != ' ').map_or(-1, |p| p as i64);
        right = right.max(last);
        if last < 0 {
            if top == i {
                top += 1;
            }
            bottom += 1;
        } else {
            bottom = 0;
        }
    }
    if rows.len() == bottom {
        return Vec::new();
    }
    rows[top..rows.len() - bottom].iter().map(|r| r[left..=right as usize].to_vec()).collect()
}

/// A pattern of `w`×`h` ingredients, row by row (`Ingredient.EMPTY` where the grid stays empty).
fn pattern(rt: &mut Rt, w: usize, h: usize, cells: Vec<V>) -> V {
    // Vanilla's `Util.isSymmetrical`: a symmetrical pattern isn't tried mirrored too.
    let entries: Vec<String> = cells.iter().map(|&c| recipes::ingredient_entries(rt, c).join(" ")).collect();
    let symmetrical = (0..h).all(|y| (0..w / 2).all(|x| entries[y * w + x] == entries[y * w + w - 1 - x]));
    let count = entries.iter().filter(|e| !e.is_empty()).count();
    let o = rt.new_object(PATTERN);
    let list = non_null_list(rt, cells);
    rt.put_field(o, "$width", V::I(w as i32));
    rt.put_field(o, "$height", V::I(h as i32));
    rt.put_field(o, "$ingredients", list);
    rt.put_field(o, "$symmetrical", V::I(symmetrical as i32));
    rt.put_field(o, "$ingredientCount", V::I(count as i32));
    o
}

fn init_shaped(rt: &mut Rt, r: V, group: V, cat: V, pattern: V, result: V, show: bool) {
    rt.put_field(r, "$group", group);
    rt.put_field(r, "$category", cat);
    rt.put_field(r, "$pattern", pattern);
    rt.put_field(r, "$result", result);
    rt.put_field(r, "$showNotification", V::I(show as i32));
}

// ---- Matching ----

/// A crafting grid of `w`×`h` stacks, row by row.
fn input(rt: &mut Rt, o: V, w: i32, h: i32, items: Vec<V>) -> V {
    let list = jutil::new_list(rt, items);
    let count = jutil::elements(rt, list).into_iter().filter(|&s| !api_common::stack_empty(rt, s)).count();
    rt.put_field(o, "$width", V::I(w));
    rt.put_field(o, "$height", V::I(h));
    rt.put_field(o, "$items", list);
    rt.put_field(o, "$ingredientCount", V::I(count as i32));
    o
}

/// `CraftingInput.of(w, h, items)`: the grid trimmed to the stacks in it (`ofPositioned`).
fn input_of(rt: &mut Rt, w: i32, h: i32, items: Vec<V>) -> V {
    let (mut x0, mut x1, mut y0, mut y1) = (w, -1, h, -1);
    for y in 0..h {
        for x in 0..w {
            let s = items.get((x + y * w) as usize).copied().unwrap_or_default();
            if !api_common::stack_empty(rt, s) {
                (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
            }
        }
    }
    let o = rt.new_object(INPUT);
    if x1 < 0 {
        return input(rt, o, 0, 0, Vec::new());
    }
    let (nw, nh) = (x1 - x0 + 1, y1 - y0 + 1);
    let mut kept = Vec::with_capacity((nw * nh) as usize);
    for y in y0..=y1 {
        for x in x0..=x1 {
            kept.push(items.get((x + y * w) as usize).copied().unwrap_or_default());
        }
    }
    input(rt, o, nw, nh, kept)
}

fn input_item(rt: &mut Rt, input: V, i: i32) -> V {
    let list = rt.get_field(input, "$items");
    match jutil::elements(rt, list).get(i.max(0) as usize) {
        Some(&s) => s,
        None => empty_stack(rt),
    }
}

/// `ShapedRecipePattern.matches(input)`: the same size and ingredient count, then every cell,
/// as written or mirrored.
fn pattern_matches(rt: &mut Rt, pattern: V, input: V) -> bool {
    let field = |rt: &Rt, o: V, f: &'static str| rt.get_field(o, f).i();
    let (w, h) = (field(rt, pattern, "$width"), field(rt, pattern, "$height"));
    if field(rt, input, "$ingredientCount") != field(rt, pattern, "$ingredientCount") || field(rt, input, "$width") != w || field(rt, input, "$height") != h {
        return false;
    }
    let list = rt.get_field(pattern, "$ingredients");
    let cells = jutil::elements(rt, list);
    let symmetrical = field(rt, pattern, "$symmetrical") != 0;
    let try_one = |rt: &mut Rt, mirrored: bool| {
        for y in 0..h {
            for x in 0..w {
                let at = if mirrored { w - x - 1 + y * w } else { x + y * w };
                let ing = cells.get(at as usize).copied().unwrap_or_default();
                let stack = input_item(rt, input, x + y * w);
                if !recipes::ingredient_test(rt, ing, stack) {
                    return false;
                }
            }
        }
        true
    };
    (!symmetrical && try_one(rt, true)) || try_one(rt, false)
}

/// The pattern of a shaped recipe (vanilla's, or a mod's subclass of it).
fn pattern_of(rt: &Rt, recipe: V) -> V {
    rt.get_field(recipe, "$pattern")
}

// ---- Calls ----

/// Calls on vanilla's crafting pieces; None for what this module doesn't model.
pub fn call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        // `serializer.codec()`: vanilla's, and a special recipe's (the factory given the category).
        (SERIALIZER | SIMPLE, "codec") if rt.class_of(a(0)) == VANILLA_SERIALIZER => rt.get_field(a(0), "$codec"),
        (SERIALIZER | SIMPLE, "codec") if rt.class_of(a(0)) == SIMPLE => {
            let factory = rt.get_field(a(0), "$factory");
            codecs::native_map(rt, "simple_crafting", &[factory])
        }
        (SIMPLE, "<init>") => {
            rt.put_field(a(0), "$factory", a(1));
            V::Void
        }
        (CATEGORY, "values") => {
            let all: Vec<V> = (0..CATEGORIES.len()).map(|i| category(rt, i)).collect();
            rt.alloc("[Lnet/minecraft/world/item/crafting/CraftingBookCategory;", Data::Array(all))
        }
        (CATEGORY, "getSerializedName") => {
            let i = rt.get_field(a(0), "$ordinal").i() as usize;
            rt.string(CATEGORIES.get(i).map_or("misc", |c| c.1))
        }
        (SHAPED, "<init>") => {
            // (group, category, pattern, result[, showNotification])
            let show = !desc.ends_with("Z)V") || a(5).i() != 0;
            init_shaped(rt, a(0), a(1), a(2), a(3), a(4), show);
            V::Void
        }
        (SHAPED, "getGroup") => rt.get_field(a(0), "$group"),
        (SHAPED, "category") => rt.get_field(a(0), "$category"),
        (SHAPED, "showNotification") => V::I(rt.get_field(a(0), "$showNotification").i()),
        (SHAPED, "getResultItem") => rt.get_field(a(0), "$result"),
        (SHAPED, "assemble") => {
            let result = rt.get_field(a(0), "$result");
            api::call(rt, "net/minecraft/world/item/ItemStack.copy.()Lnet/minecraft/world/item/ItemStack;", &[result])
        }
        (SHAPED, "getWidth") => V::I(rt.get_field(pattern_of(rt, a(0)), "$width").i()),
        (SHAPED, "getHeight") => V::I(rt.get_field(pattern_of(rt, a(0)), "$height").i()),
        (SHAPED, "getIngredients") => rt.get_field(pattern_of(rt, a(0)), "$ingredients"),
        (SHAPED, "canCraftInDimensions") => {
            let p = pattern_of(rt, a(0));
            V::I((a(1).i() >= rt.get_field(p, "$width").i() && a(2).i() >= rt.get_field(p, "$height").i()) as i32)
        }
        (SHAPED, "matches") => {
            let p = pattern_of(rt, a(0));
            V::I(pattern_matches(rt, p, a(1)) as i32)
        }
        (SHAPED, "isSpecial") => V::I(0),
        (SHAPED, "isIncomplete") => {
            let list = rt.get_field(pattern_of(rt, a(0)), "$ingredients");
            let cells = jutil::elements(rt, list);
            let incomplete = cells.is_empty() || cells.iter().any(|&c| !recipes::ingredient_entries(rt, c).is_empty() && recipes::ingredient_items(rt, c).is_empty());
            V::I(incomplete as i32)
        }
        (SHAPED, "getSerializer") => vanilla_serializer(rt, "SHAPED_RECIPE").unwrap_or(V::NULL),
        (SHAPED, "getType") => api::get_static(rt, "net/minecraft/world/item/crafting/RecipeType.CRAFTING"),
        (SHAPED, "getRemainingItems") => {
            let n = rt.get_field(a(1), "$items");
            let size = jutil::elements(rt, n).len();
            let empty = empty_stack(rt);
            non_null_list(rt, vec![empty; size])
        }
        (PATTERN, "width") => V::I(rt.get_field(a(0), "$width").i()),
        (PATTERN, "height") => V::I(rt.get_field(a(0), "$height").i()),
        (PATTERN, "ingredients") => rt.get_field(a(0), "$ingredients"),
        (PATTERN, "matches") => V::I(pattern_matches(rt, a(0), a(1)) as i32),
        (PATTERN, "setCraftingSize") => {
            let (w, h) = max_size(rt);
            rt.statics.insert(MAX_WIDTH, V::I(a(0).i().max(w as i32)));
            rt.statics.insert(MAX_HEIGHT, V::I(a(1).i().max(h as i32)));
            V::Void
        }
        (PATTERN, "getMaxWidth") => V::I(max_size(rt).0 as i32),
        (PATTERN, "getMaxHeight") => V::I(max_size(rt).1 as i32),
        (INPUT, "of") => {
            let items = jutil::elements(rt, a(2));
            input_of(rt, a(0).i(), a(1).i(), items)
        }
        (INPUT, "<init>") => {
            let items = jutil::elements(rt, a(3));
            input(rt, a(0), a(1).i(), a(2).i(), items);
            V::Void
        }
        (INPUT, "width") => V::I(rt.get_field(a(0), "$width").i()),
        (INPUT, "height") => V::I(rt.get_field(a(0), "$height").i()),
        (INPUT, "size") => {
            let list = rt.get_field(a(0), "$items");
            V::I(jutil::elements(rt, list).len() as i32)
        }
        (INPUT, "isEmpty") => V::I((rt.get_field(a(0), "$ingredientCount").i() == 0) as i32),
        (INPUT, "ingredientCount") => V::I(rt.get_field(a(0), "$ingredientCount").i()),
        (INPUT, "items") => rt.get_field(a(0), "$items"),
        (INPUT, "getItem") if desc == "(II)Lnet/minecraft/world/item/ItemStack;" => {
            let w = rt.get_field(a(0), "$width").i();
            input_item(rt, a(0), a(1).i() + a(2).i() * w)
        }
        (INPUT, "getItem") => input_item(rt, a(0), a(1).i()),
        _ => return None,
    })
}

/// Static fields: vanilla's serializers, the categories, the crafting type, the empty grid.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match (owner, field) {
        (SERIALIZER, f) => vanilla_serializer(rt, f),
        (CATEGORY, "CODEC") => Some(codecs::native(rt, "crafting_book_category")),
        (CATEGORY, f) => CATEGORIES.iter().position(|(n, _)| *n == f).map(|i| category(rt, i)),
        (PATTERN, "MAP_CODEC") => Some(codecs::native_map(rt, "shaped_pattern", &[])),
        (INPUT, "EMPTY") => Some(rt.intern("compat:crafting-input/empty".into(), |rt| {
            let o = rt.new_object(INPUT);
            input(rt, o, 0, 0, Vec::new())
        })),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(j: Json) -> Map<String, Json> {
        j.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn a_shaped_pattern_is_shrunk_and_checked_as_vanilla_checks_it() {
        let mut rt = Rt::new();
        crate::install();
        // A blank border goes: the pattern is the 2×1 it uses.
        let p = decode_map(&mut rt, "shaped_pattern", &[], &obj(json!({"key": {"A": {"item": "stone"}, "B": {"tag": "minecraft:planks"}}, "pattern": ["   ", " AB", "   "]}))).unwrap();
        assert_eq!((rt.get_field(p, "$width"), rt.get_field(p, "$height")), (V::I(2), V::I(1)));
        let err = |rt: &mut Rt, j: Json| decode_map(rt, "shaped_pattern", &[], &obj(j)).unwrap_err();
        assert_eq!(err(&mut rt, json!({"key": {"A": {"item": "stone"}}, "pattern": ["AC"]})), "Pattern references symbol 'C' but it's not defined in the key");
        assert_eq!(err(&mut rt, json!({"key": {"A": {"item": "stone"}, "B": {"item": "dirt"}}, "pattern": ["A"]})), "Key defines symbols that aren't used in pattern: [B]");
        // Vanilla's grid is 3×3 until a mod asks for more (Create's mechanical crafting, 9×9).
        let wide = json!({"key": {"A": {"item": "stone"}}, "pattern": ["AAAAA"]});
        assert_eq!(err(&mut rt, wide.clone()), "pattern: Invalid pattern: too many columns, 3 is maximum");
        call(&mut rt, PATTERN, "setCraftingSize", "(II)V", &[V::I(9), V::I(9)]);
        assert!(decode_map(&mut rt, "shaped_pattern", &[], &obj(wide)).is_ok());
    }

    #[test]
    fn a_shaped_recipe_matches_its_shape_anywhere_in_the_grid_as_written_or_mirrored() {
        let mut rt = Rt::new();
        crate::install();
        let recipe = decode_map(&mut rt, "shaped_recipe", &[], &obj(json!({"key": {"A": {"item": "stone"}, "B": {"item": "dirt"}}, "pattern": ["AB", "A "], "result": {"id": "stick", "count": 4}}))).unwrap();
        let grid = |rt: &mut Rt, cells: [&str; 9]| {
            let items: Vec<V> = cells.iter().map(|c| if c.is_empty() { empty_stack(rt) } else { api_common::stack(rt, &format!("minecraft:{c}"), 1) }).collect();
            let list = jutil::new_list(rt, items);
            call(rt, INPUT, "of", "(IILjava/util/List;)Lnet/minecraft/world/item/crafting/CraftingInput;", &[V::I(3), V::I(3), list]).unwrap()
        };
        let matches = |rt: &mut Rt, input: V| {
            call(rt, SHAPED, "matches", "(Lnet/minecraft/world/item/crafting/CraftingInput;Lnet/minecraft/world/level/Level;)Z", &[recipe, input, V::NULL]) == Some(V::I(1))
        };
        let written = grid(&mut rt, ["", "", "", "", "stone", "dirt", "", "stone", ""]);
        assert!(matches(&mut rt, written));
        let mirrored = grid(&mut rt, ["dirt", "stone", "", "", "stone", "", "", "", ""]);
        assert!(matches(&mut rt, mirrored));
        let upside_down = grid(&mut rt, ["stone", "", "", "stone", "dirt", "", "", "", ""]);
        assert!(!matches(&mut rt, upside_down));
        let one_more = grid(&mut rt, ["stone", "dirt", "", "stone", "dirt", "", "", "", ""]);
        assert!(!matches(&mut rt, one_more));
        let result = call(&mut rt, SHAPED, "getResultItem", "(Lnet/minecraft/core/HolderLookup$Provider;)Lnet/minecraft/world/item/ItemStack;", &[recipe, V::NULL]).unwrap();
        assert_eq!(rt.get_field(result, "$count"), V::I(4));
    }
}
