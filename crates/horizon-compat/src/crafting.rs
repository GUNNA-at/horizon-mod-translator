//! Vanilla's recipes as a translated mod builds on them and looks them up, natively:
//! `ShapedRecipe` and its `ShapedRecipePattern`, read by vanilla's codec (Create's mechanical
//! crafting is a shaped recipe plus a flag, its pattern as large as NeoForge's `setCraftingSize`
//! allows), a mod's subclass of it, `ShapelessRecipe`, the cooking recipes (`SmeltingRecipe`,
//! `BlastingRecipe`, `SmokingRecipe`, `CampfireCookingRecipe`, their `AbstractCookingRecipe`),
//! `StonecutterRecipe` and its `SingleItemRecipe`, `SimpleCraftingRecipeSerializer` (a special
//! recipe: the mod's factory given the category), `CraftingBookCategory`, and `CraftingInput`, the
//! grid a crafting recipe matches. The game's files of Minecraft's own recipe types are read by
//! these ([`vanilla_codec`]): Create's fans smelt by vanilla's smelting and blasting recipes.
use crate::{Data, Rt, V, api, api_common, codecs, jutil, recipes};
use serde_json::{Map, Value as Json};

const RECIPE: &str = "net/minecraft/world/item/crafting/Recipe";
const GET_INGREDIENTS: &str =
    "net/minecraft/world/item/crafting/Recipe.getIngredients.()Lnet/minecraft/core/NonNullList;";
const SHAPED: &str = "net/minecraft/world/item/crafting/ShapedRecipe";
const SHAPELESS: &str = "net/minecraft/world/item/crafting/ShapelessRecipe";
const COOKING: &str = "net/minecraft/world/item/crafting/AbstractCookingRecipe";
const SINGLE_ITEM: &str = "net/minecraft/world/item/crafting/SingleItemRecipe";
const STONECUTTER: &str = "net/minecraft/world/item/crafting/StonecutterRecipe";
const COOKING_CATEGORY: &str = "net/minecraft/world/item/crafting/CookingBookCategory";
/// `CookingBookCategory`'s constants in declaration order, with their serialized names.
const COOKING_CATEGORIES: [(&str, &str); 3] =
    [("FOOD", "food"), ("BLOCKS", "blocks"), ("MISC", "misc")];
const COOKING_MISC: usize = 2;
const SINGLE_INPUT: &str = "net/minecraft/world/item/crafting/SingleRecipeInput";
const INPUT_GET: &str =
    "net/minecraft/world/item/crafting/RecipeInput.getItem.(I)Lnet/minecraft/world/item/ItemStack;";
const INPUT_SIZE: &str = "net/minecraft/world/item/crafting/RecipeInput.size.()I";
const REMAINDER: &str = "net/minecraft/world/item/ItemStack.getCraftingRemainingItem.()Lnet/minecraft/world/item/ItemStack;";
const STACK_COPY: &str =
    "net/minecraft/world/item/ItemStack.copy.()Lnet/minecraft/world/item/ItemStack;";
/// `AbstractCookingRecipe`'s fields in its constructor's order. They are protected: a mod's
/// subclass reads them by these names.
const COOKING_FIELDS: [&str; 7] = [
    "type",
    "group",
    "category",
    "ingredient",
    "result",
    "experience",
    "cookingTime",
];
/// `SingleItemRecipe`'s fields in its constructor's order (`ingredient`, `result` and `group` are
/// protected).
const SINGLE_ITEM_FIELDS: [&str; 5] = ["type", "serializer", "group", "ingredient", "result"];

/// One of vanilla's cooking recipe classes.
struct Cooking {
    class: &'static str,
    /// Its `RecipeType` static.
    ty: &'static str,
    /// Its serializer's `RecipeSerializer` field.
    serializer: &'static str,
    /// The cooking time its serializer gives a file that names none.
    time: i32,
    /// The block its recipe toast shows.
    toast: &'static str,
}

const COOKINGS: [Cooking; 4] = [
    Cooking {
        class: "net/minecraft/world/item/crafting/SmeltingRecipe",
        ty: "net/minecraft/world/item/crafting/RecipeType.SMELTING",
        serializer: "SMELTING_RECIPE",
        time: 200,
        toast: "minecraft:furnace",
    },
    Cooking {
        class: "net/minecraft/world/item/crafting/BlastingRecipe",
        ty: "net/minecraft/world/item/crafting/RecipeType.BLASTING",
        serializer: "BLASTING_RECIPE",
        time: 100,
        toast: "minecraft:blast_furnace",
    },
    Cooking {
        class: "net/minecraft/world/item/crafting/SmokingRecipe",
        ty: "net/minecraft/world/item/crafting/RecipeType.SMOKING",
        serializer: "SMOKING_RECIPE",
        time: 100,
        toast: "minecraft:smoker",
    },
    Cooking {
        class: "net/minecraft/world/item/crafting/CampfireCookingRecipe",
        ty: "net/minecraft/world/item/crafting/RecipeType.CAMPFIRE_COOKING",
        serializer: "CAMPFIRE_COOKING_RECIPE",
        time: 100,
        toast: "minecraft:campfire",
    },
];

/// Vanilla's serializers: their `RecipeSerializer` field, their id, and the codec ([`decode_map`])
/// that reads their recipes here when this runtime models them.
const SERIALIZERS: [(&str, &str, Option<&str>); 9] = [
    (
        "SHAPED_RECIPE",
        "minecraft:crafting_shaped",
        Some("shaped_recipe"),
    ),
    (
        "SHAPELESS_RECIPE",
        "minecraft:crafting_shapeless",
        Some("shapeless_recipe"),
    ),
    (
        "SMELTING_RECIPE",
        "minecraft:smelting",
        Some("SMELTING_RECIPE"),
    ),
    (
        "BLASTING_RECIPE",
        "minecraft:blasting",
        Some("BLASTING_RECIPE"),
    ),
    (
        "SMOKING_RECIPE",
        "minecraft:smoking",
        Some("SMOKING_RECIPE"),
    ),
    (
        "CAMPFIRE_COOKING_RECIPE",
        "minecraft:campfire_cooking",
        Some("CAMPFIRE_COOKING_RECIPE"),
    ),
    (
        "STONECUTTER",
        "minecraft:stonecutting",
        Some("stonecutting"),
    ),
    ("SMITHING_TRANSFORM", "minecraft:smithing_transform", None),
    ("SMITHING_TRIM", "minecraft:smithing_trim", None),
];
const PATTERN: &str = "net/minecraft/world/item/crafting/ShapedRecipePattern";
const SERIALIZER: &str = "net/minecraft/world/item/crafting/RecipeSerializer";
const SIMPLE: &str = "net/minecraft/world/item/crafting/SimpleCraftingRecipeSerializer";
const CATEGORY: &str = "net/minecraft/world/item/crafting/CraftingBookCategory";
const INPUT: &str = "net/minecraft/world/item/crafting/CraftingInput";
const POSITIONED: &str = "net/minecraft/world/item/crafting/CraftingInput$Positioned";
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

/// The serializer in `RecipeSerializer.<field>`: vanilla's own, with the codec that reads its
/// recipes here when this runtime models them.
fn vanilla_serializer(rt: &mut Rt, field: &str) -> Option<V> {
    let &(_, id, codec) = SERIALIZERS.iter().find(|(f, ..)| *f == field)?;
    Some(rt.intern(format!("compat:recipe-serializer/{id}"), move |rt| {
        let o = rt.new_object(VANILLA_SERIALIZER);
        let s = rt.string(id);
        rt.put_field(o, "$id", s);
        let codec = codec.map_or(V::NULL, |c| codecs::native_map(rt, c, &[]));
        rt.put_field(o, "$codec", codec);
        o
    }))
}

/// The codec of Minecraft's serializer `id` (`minecraft:smelting`), for a file of its own type:
/// null for one this runtime doesn't model (a special crafting recipe, smithing).
pub(crate) fn vanilla_codec(rt: &mut Rt, id: &str) -> V {
    match SERIALIZERS.iter().find(|(_, i, _)| *i == id) {
        Some(&(field, ..)) => {
            vanilla_serializer(rt, field).map_or(V::NULL, |s| rt.get_field(s, "$codec"))
        }
        None => V::NULL,
    }
}

/// The name Minecraft's own recipe type or serializer `obj` has in `registry`: the objects
/// `RecipeType.SMELTING` and `RecipeSerializer.SHAPED_RECIPE` hold are registered under
/// `minecraft:smelting` and `minecraft:crafting_shaped` (Create looks a recipe's serializer up in
/// its registry before it automates the recipe).
pub(crate) fn vanilla_entry_name<'a>(rt: &'a Rt, registry: &str, obj: V) -> Option<&'a str> {
    let interned = match registry {
        "recipe_type" => "compat:recipe-type/",
        "recipe_serializer" => "compat:recipe-serializer/",
        _ => return None,
    };
    let id = rt.str_of(rt.get_field(obj, "$id"))?;
    (rt.interned.get(&format!("{interned}{id}")) == Some(&obj)).then_some(id)
}

/// Minecraft's own recipe type or serializer registered under `id` in `registry`, None for a name
/// it has none under.
pub(crate) fn vanilla_entry(rt: &mut Rt, registry: &str, id: &str) -> Option<V> {
    match registry {
        "recipe_type" => recipes::vanilla_type_entry(rt, id),
        "recipe_serializer" => {
            let &(field, ..) = SERIALIZERS.iter().find(|(_, i, _)| *i == id)?;
            vanilla_serializer(rt, field)
        }
        _ => None,
    }
}

/// Vanilla's recipe classes modelled here by their supertypes, for `instanceof` (Create's press
/// compacts by a `CraftingRecipe`).
pub(crate) fn recipe_is_a(c: &str, class: &str) -> bool {
    let crafting = matches!(c, SHAPED | SHAPELESS);
    let cooking = COOKINGS.iter().any(|k| k.class == c);
    match class {
        "net/minecraft/world/item/crafting/Recipe" => {
            crafting || cooking || matches!(c, COOKING | SINGLE_ITEM | STONECUTTER)
        }
        "net/minecraft/world/item/crafting/CraftingRecipe" => crafting,
        COOKING => cooking,
        SINGLE_ITEM => c == STONECUTTER,
        _ => false,
    }
}

/// The cooking kind `recipe` is of: its class's, or the one its mod class extends.
fn cooking_kind(rt: &Rt, recipe: V) -> Option<&'static Cooking> {
    let class = rt.class_of(recipe);
    COOKINGS.iter().find(|c| rt.is_a(class, c.class))
}

fn cooking_category(rt: &mut Rt, i: usize) -> V {
    api::api_enum(rt, COOKING_CATEGORY, COOKING_CATEGORIES[i].0, i as i32)
}

// ---- Reading ----

/// Reads `o` with vanilla's map codec `name` ([`codecs::native_map`]); `extra` is what the codec
/// was made with (a simple serializer's factory).
pub fn decode_map(rt: &mut Rt, name: &str, extra: &[V], o: &Map<String, Json>) -> Result<V, String> {
    match name {
        "shaped_pattern" => decode_pattern(rt, o),
        "shaped_recipe" => {
            let group = read_group(o)?;
            let cat = crafting_category_index(o);
            let pattern = decode_pattern(rt, o)?;
            let result = read_result(rt, o)?;
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
        "shapeless_recipe" => {
            let group = read_group(o)?;
            let cat = crafting_category_index(o);
            let result = read_result(rt, o)?;
            let list = match o.get("ingredients") {
                Some(Json::Array(list)) => list,
                Some(j) => return Err(format!("ingredients: Not a list: {j}")),
                None => return Err("No key ingredients in MapLike".into()),
            };
            let mut ingredients = Vec::with_capacity(list.len());
            for j in list {
                ingredients.push(
                    recipes::decode_native(rt, "ingredient_nonempty", j)
                        .map_err(|e| format!("ingredients: {e}"))?,
                );
            }
            let (w, h) = max_size(rt);
            if ingredients.is_empty() {
                return Err("ingredients: No ingredients for shapeless recipe".into());
            }
            if ingredients.len() > w * h {
                return Err(format!(
                    "ingredients: Too many ingredients for shapeless recipe. The maximum is: {}",
                    w * h
                ));
            }
            let r = rt.new_object(SHAPELESS);
            let (g, c) = (rt.string(&group), category(rt, cat));
            let list = non_null_list(rt, ingredients);
            init_shapeless(rt, r, [g, c, result, list]);
            Ok(r)
        }
        "stonecutting" => {
            let group = read_group(o)?;
            let ingredient = read_ingredient(rt, o)?;
            let result = read_result(rt, o)?;
            let r = rt.new_object(STONECUTTER);
            let ty = api::get_static(
                rt,
                "net/minecraft/world/item/crafting/RecipeType.STONECUTTING",
            );
            let serializer = vanilla_serializer(rt, "STONECUTTER").unwrap_or(V::NULL);
            let g = rt.string(&group);
            init_fields(
                rt,
                r,
                &SINGLE_ITEM_FIELDS,
                &[ty, serializer, g, ingredient, result],
            );
            Ok(r)
        }
        "simple_crafting" => {
            let c = crafting_category_index(o);
            let c = category(rt, c);
            let factory = extra.first().copied().unwrap_or_default();
            let r = rt.call_functional(FACTORY_CREATE, factory, &[c]);
            if let Some(e) = rt.thrown.take() {
                return Err(format!("the recipe's factory threw {}", rt.class_of(e)));
            }
            Ok(r)
        }
        _ => match COOKINGS.iter().find(|c| c.serializer == name) {
            Some(kind) => decode_cooking(rt, kind, o),
            None => Err(format!("no codec {name}")),
        },
    }
}

/// A cooking recipe of `kind` as its serializer reads one (`SimpleCookingSerializer`): `group`,
/// `category` (misc when missing or unknown), `ingredient`, `result`, `experience` (0 when
/// missing or unreadable) and `cookingtime` (the kind's when missing or unreadable).
fn decode_cooking(rt: &mut Rt, kind: &Cooking, o: &Map<String, Json>) -> Result<V, String> {
    let group = read_group(o)?;
    let cat = o
        .get("category")
        .and_then(Json::as_str)
        .and_then(|c| COOKING_CATEGORIES.iter().position(|(_, n)| *n == c))
        .unwrap_or(COOKING_MISC);
    let ingredient = read_ingredient(rt, o)?;
    let result = read_result(rt, o)?;
    let experience = o
        .get("experience")
        .and_then(Json::as_f64)
        .map_or(0.0, |x| x as f32);
    let time = o
        .get("cookingtime")
        .and_then(Json::as_f64)
        .map_or(kind.time, |t| t as i32);
    let r = rt.new_object(kind.class);
    let ty = api::get_static(rt, kind.ty);
    let (g, c) = (rt.string(&group), cooking_category(rt, cat));
    init_fields(
        rt,
        r,
        &COOKING_FIELDS,
        &[ty, g, c, ingredient, result, V::F(experience), V::I(time)],
    );
    Ok(r)
}

/// `Codec.STRING.optionalFieldOf("group", "")`.
fn read_group(o: &Map<String, Json>) -> Result<String, String> {
    match o.get("group") {
        None => Ok(String::new()),
        Some(Json::String(g)) => Ok(g.clone()),
        Some(j) => Err(format!("group: Not a string: {j}")),
    }
}

/// `CraftingBookCategory.CODEC.fieldOf("category").orElse(MISC)`: a missing or unknown category
/// is misc.
fn crafting_category_index(o: &Map<String, Json>) -> usize {
    o.get("category")
        .and_then(Json::as_str)
        .and_then(|c| CATEGORIES.iter().position(|(_, n)| *n == c))
        .unwrap_or(MISC)
}

fn read_result(rt: &mut Rt, o: &Map<String, Json>) -> Result<V, String> {
    recipes::decode_native(
        rt,
        "item_stack",
        o.get("result").ok_or("No key result in MapLike")?,
    )
    .map_err(|e| format!("result: {e}"))
}

fn read_ingredient(rt: &mut Rt, o: &Map<String, Json>) -> Result<V, String> {
    recipes::decode_native(
        rt,
        "ingredient_nonempty",
        o.get("ingredient").ok_or("No key ingredient in MapLike")?,
    )
    .map_err(|e| format!("ingredient: {e}"))
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

/// A shapeless recipe's group, category, result and ingredients (its constructor's order).
fn init_shapeless(rt: &mut Rt, r: V, values: [V; 4]) {
    init_fields(
        rt,
        r,
        &["$group", "$category", "$result", "$ingredients"],
        &values,
    );
}

fn init_fields(rt: &mut Rt, r: V, fields: &[&'static str], values: &[V]) {
    for (&f, &v) in fields.iter().zip(values) {
        rt.put_field(r, f, v);
    }
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

/// `CraftingInput.ofPositioned(w, h, items)`: the grid trimmed to the columns and rows that hold a
/// stack, with the column and row it starts at; the empty input at 0, 0 when nothing is left. As
/// vanilla's, a one-by-one grid stays whole even when its stack is empty.
fn positioned(rt: &mut Rt, w: i32, h: i32, items: &[V]) -> (V, i32, i32) {
    let at = |x: i32, y: i32| items.get((x + y * w) as usize).copied().unwrap_or_default();
    let (mut left, mut right, mut top, mut bottom) = (w - 1, 0, h - 1, 0);
    for y in 0..h {
        let mut row_empty = true;
        for x in 0..w {
            if !api_common::stack_empty(rt, at(x, y)) {
                (left, right, row_empty) = (left.min(x), right.max(x), false);
            }
        }
        if !row_empty {
            (top, bottom) = (top.min(y), bottom.max(y));
        }
    }
    let (nw, nh) = (right - left + 1, bottom - top + 1);
    if w == 0 || h == 0 || nw <= 0 || nh <= 0 {
        return (get_static(rt, INPUT, "EMPTY").unwrap_or(V::NULL), 0, 0);
    }
    let kept = if nw == w && nh == h {
        items.to_vec()
    } else {
        (0..nh)
            .flat_map(|y| (0..nw).map(move |x| (x, y)))
            .map(|(x, y)| at(x + left, y + top))
            .collect()
    };
    let o = rt.new_object(INPUT);
    (input(rt, o, nw, nh, kept), left, top)
}

/// `CraftingInput.of(w, h, items)`: the grid trimmed to the stacks in it ([`positioned`]).
fn input_of(rt: &mut Rt, w: i32, h: i32, items: Vec<V>) -> V {
    positioned(rt, w, h, &items).0
}

/// `CraftingInput.Positioned` of a trimmed grid: vanilla's `EMPTY` for the empty one.
fn positioned_record(rt: &mut Rt, (input, left, top): (V, i32, i32)) -> V {
    match get_static(rt, POSITIONED, "EMPTY") {
        Some(empty) if rt.get_field(empty, "input") == input => empty,
        _ => crate::records::make(rt, POSITIONED, &[input, V::I(left), V::I(top)]),
    }
}

/// A crafting container's grid as a recipe reads it (`CraftingContainer.asCraftInput` and
/// `asPositionedCraftInput`): its width, height and stacks, trimmed.
pub(crate) fn container_input(rt: &mut Rt, w: i32, h: i32, items: &[V], with_position: bool) -> V {
    let trimmed = positioned(rt, w, h, items);
    if with_position {
        positioned_record(rt, trimmed)
    } else {
        trimmed.0
    }
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

/// `ShapelessRecipe.matches(input)`: as many stacks in the grid as ingredients, each meeting a
/// different one (NeoForge's `RecipeMatcher`; vanilla's `StackedContents` pairs them alike).
fn shapeless_matches(rt: &mut Rt, recipe: V, input: V) -> bool {
    let list = rt.get_field(recipe, "$ingredients");
    let ingredients = jutil::elements(rt, list);
    if rt.get_field(input, "$ingredientCount").i() as usize != ingredients.len() {
        return false;
    }
    let list = rt.get_field(input, "$items");
    let stacks: Vec<V> = jutil::elements(rt, list)
        .into_iter()
        .filter(|&s| !api_common::stack_empty(rt, s))
        .collect();
    recipes::pairing(rt, &stacks, &ingredients).is_some()
}

/// The stack a cooking or stonecutting recipe takes: `SingleRecipeInput.item()`, or another
/// input's first.
fn single_input(rt: &mut Rt, input: V) -> V {
    if rt.class_of(input) == SINGLE_INPUT {
        return rt.get_field(input, "$item");
    }
    rt.call_functional(INPUT_GET, input, &[V::I(0)])
}

/// A cooking or stonecutting recipe's `matches`: its ingredient takes the input's stack.
fn single_matches(rt: &mut Rt, recipe: V, input: V) -> bool {
    let ingredient = rt.get_field(recipe, "ingredient");
    let stack = single_input(rt, input);
    recipes::ingredient_test(rt, ingredient, stack)
}

/// `recipe.matches(input, level)` for a recipe of one of vanilla's classes modelled here; None
/// for any other, a mod's subclass included (it may match in its own way).
pub(crate) fn vanilla_matches(rt: &mut Rt, recipe: V, input: V) -> Option<bool> {
    let class = rt.class_of(recipe);
    Some(match class {
        SHAPED => {
            let p = pattern_of(rt, recipe);
            pattern_matches(rt, p, input)
        }
        SHAPELESS => shapeless_matches(rt, recipe, input),
        STONECUTTER => single_matches(rt, recipe, input),
        _ if COOKINGS.iter().any(|c| c.class == class) => single_matches(rt, recipe, input),
        _ => return None,
    })
}

/// The stacks of `input` in order: a crafting grid's, a single input's one, or what another
/// input's `size()` and `getItem(i)` give.
fn input_stacks(rt: &mut Rt, input: V) -> Vec<V> {
    match rt.class_of(input) {
        INPUT => {
            let list = rt.get_field(input, "$items");
            jutil::elements(rt, list)
        }
        SINGLE_INPUT => vec![rt.get_field(input, "$item")],
        _ => {
            let n = rt.call_functional(INPUT_SIZE, input, &[]).i();
            (0..n.max(0))
                .map(|i| rt.call_functional(INPUT_GET, input, &[V::I(i)]))
                .collect()
        }
    }
}

/// `Recipe.getRemainingItems(input)`, vanilla's: what crafting leaves of each of the input's
/// stacks (a bucket of a milk bucket), an empty stack for the rest.
fn remaining_items(rt: &mut Rt, input: V) -> V {
    let stacks = input_stacks(rt, input);
    let left = stacks
        .into_iter()
        .map(|s| api::call(rt, REMAINDER, &[s]))
        .collect();
    non_null_list(rt, left)
}

/// `Recipe.isIncomplete()`, vanilla's: no ingredients, or one no item meets (a tag nothing is in).
fn incomplete(rt: &mut Rt, ingredients: &[V]) -> bool {
    ingredients.is_empty()
        || ingredients
            .iter()
            .any(|&c| recipes::ingredient_items(rt, c).is_empty())
}

// ---- Calls ----

/// The keys [`call`] may answer, by owner, name and descriptor alone (every arm, guards aside): part of
/// [`crate::recipes::claims`], whose `call` asks this one.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(
        owner,
        SERIALIZER | SIMPLE | CATEGORY | SHAPED | PATTERN | INPUT
    ) || recipe_class(owner)
        || (owner == RECIPE && RECIPE_DEFAULTS.contains(&name))
}

/// `Recipe`'s default methods.
const RECIPE_DEFAULTS: [&str; 7] = [
    "getRemainingItems",
    "getIngredients",
    "isSpecial",
    "showNotification",
    "getGroup",
    "getToastSymbol",
    "isIncomplete",
];

/// `Recipe`'s default methods, for a mod's recipe that doesn't override them (Create's basin asks
/// its own recipes what they leave of the grid). A vanilla recipe modelled here answers by its
/// class.
fn recipe_default(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let this = args.first().copied().unwrap_or_default();
    if this.r() == 0 || !rt.is_mod_class(rt.class_of(this)) {
        return None;
    }
    Some(match name {
        "getRemainingItems" => remaining_items(rt, args.get(1).copied().unwrap_or_default()),
        "getIngredients" => non_null_list(rt, Vec::new()),
        "isSpecial" => V::I(0),
        "showNotification" => V::I(1),
        "getGroup" => rt.string(""),
        "getToastSymbol" => api_common::stack(rt, "minecraft:crafting_table", 1),
        "isIncomplete" => {
            let list = crate::menus::virt(rt, GET_INGREDIENTS, this, &[]);
            let ingredients = jutil::elements(rt, list);
            V::I(incomplete(rt, &ingredients) as i32)
        }
        _ => return None,
    })
}

/// One of the recipe classes [`recipe_call`] models.
fn recipe_class(owner: &str) -> bool {
    matches!(owner, SHAPELESS | COOKING | SINGLE_ITEM | STONECUTTER)
        || COOKINGS.iter().any(|c| c.class == owner)
}

/// Calls on a shapeless, cooking or stonecutting recipe: vanilla's, or a member a mod's subclass
/// inherits.
fn recipe_call(rt: &mut Rt, owner: &str, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let shapeless = owner == SHAPELESS;
    let single = owner == SINGLE_ITEM || owner == STONECUTTER;
    let field = |s: &'static str, plain: &'static str| if shapeless { s } else { plain };
    Some(match name {
        "<init>" if shapeless => {
            init_shapeless(rt, this, [a(1), a(2), a(3), a(4)]);
            V::Void
        }
        // `StonecutterRecipe(group, ingredient, result)`, of its own type and serializer.
        "<init>" if owner == STONECUTTER => {
            let ty = api::get_static(
                rt,
                "net/minecraft/world/item/crafting/RecipeType.STONECUTTING",
            );
            let serializer = vanilla_serializer(rt, "STONECUTTER").unwrap_or(V::NULL);
            init_fields(
                rt,
                this,
                &SINGLE_ITEM_FIELDS,
                &[ty, serializer, a(1), a(2), a(3)],
            );
            V::Void
        }
        "<init>" if single => {
            init_fields(
                rt,
                this,
                &SINGLE_ITEM_FIELDS,
                args.get(1..).unwrap_or_default(),
            );
            V::Void
        }
        "<init>" if owner == COOKING => {
            init_fields(rt, this, &COOKING_FIELDS, args.get(1..).unwrap_or_default());
            V::Void
        }
        // A cooking class's `(group, category, ingredient, result, experience, cookingTime)`, of
        // its own type.
        "<init>" => {
            let kind = COOKINGS.iter().find(|c| c.class == owner)?;
            let ty = api::get_static(rt, kind.ty);
            init_fields(
                rt,
                this,
                &COOKING_FIELDS,
                &[ty, a(1), a(2), a(3), a(4), a(5), a(6)],
            );
            V::Void
        }
        "matches" if shapeless => V::I(shapeless_matches(rt, this, a(1)) as i32),
        "matches" => V::I(single_matches(rt, this, a(1)) as i32),
        "getResultItem" => rt.get_field(this, field("$result", "result")),
        "assemble" => {
            let result = rt.get_field(this, field("$result", "result"));
            api::call(rt, STACK_COPY, &[result])
        }
        "getIngredients" if shapeless => rt.get_field(this, "$ingredients"),
        "getIngredients" => {
            let ingredient = rt.get_field(this, "ingredient");
            non_null_list(rt, vec![ingredient])
        }
        "getGroup" => rt.get_field(this, field("$group", "group")),
        "category" => rt.get_field(this, field("$category", "category")),
        "getExperience" => match rt.get_field(this, "experience") {
            V::F(x) => V::F(x),
            _ => V::F(0.0),
        },
        "getCookingTime" => V::I(rt.get_field(this, "cookingTime").i()),
        "getType" if shapeless => {
            api::get_static(rt, "net/minecraft/world/item/crafting/RecipeType.CRAFTING")
        }
        "getType" => rt.get_field(this, "type"),
        "getSerializer" if shapeless => {
            vanilla_serializer(rt, "SHAPELESS_RECIPE").unwrap_or(V::NULL)
        }
        "getSerializer" if single => rt.get_field(this, "serializer"),
        "getSerializer" => match cooking_kind(rt, this) {
            Some(kind) => vanilla_serializer(rt, kind.serializer).unwrap_or(V::NULL),
            None => V::NULL,
        },
        "canCraftInDimensions" if shapeless => {
            let list = rt.get_field(this, "$ingredients");
            let n = jutil::elements(rt, list).len() as i64;
            V::I((a(1).i() as i64 * a(2).i() as i64 >= n) as i32)
        }
        "canCraftInDimensions" => V::I(1),
        "getToastSymbol" => {
            let class = rt.class_of(this);
            let block = match cooking_kind(rt, this) {
                Some(kind) => kind.toast,
                None if rt.is_a(class, STONECUTTER) => "minecraft:stonecutter",
                None => "minecraft:crafting_table",
            };
            api_common::stack(rt, block, 1)
        }
        "isSpecial" => V::I(0),
        "showNotification" => V::I(1),
        "isIncomplete" => {
            let ingredients = if shapeless {
                let list = rt.get_field(this, "$ingredients");
                jutil::elements(rt, list)
            } else {
                vec![rt.get_field(this, "ingredient")]
            };
            V::I(incomplete(rt, &ingredients) as i32)
        }
        "getRemainingItems" => remaining_items(rt, a(1)),
        _ => return None,
    })
}

/// Calls on vanilla's crafting pieces; None for what this module doesn't model.
pub fn call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    if recipe_class(owner) {
        return recipe_call(rt, owner, name, args);
    }
    if owner == RECIPE {
        return recipe_default(rt, name, args);
    }
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
            api::call(rt, STACK_COPY, &[result])
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
        (SHAPED, "getRemainingItems") => remaining_items(rt, a(1)),
        (SHAPED, "getToastSymbol") => api_common::stack(rt, "minecraft:crafting_table", 1),
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
        (INPUT, "ofPositioned") => {
            let items = jutil::elements(rt, a(2));
            let trimmed = positioned(rt, a(0).i(), a(1).i(), &items);
            positioned_record(rt, trimmed)
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
        (POSITIONED, "EMPTY") => Some(rt.intern(
            "compat:crafting-input/positioned-empty".into(),
            |rt| {
                let input = get_static(rt, INPUT, "EMPTY").unwrap_or(V::NULL);
                crate::records::make(rt, POSITIONED, &[input, V::I(0), V::I(0)])
            },
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;
    use serde_json::json;

    fn obj(j: Json) -> Map<String, Json> {
        j.as_object().cloned().unwrap_or_default()
    }

    const GET_RECIPE_FOR: &str = "net/minecraft/world/item/crafting/RecipeManager.getRecipeFor.(Lnet/minecraft/world/item/crafting/RecipeType;Lnet/minecraft/world/item/crafting/RecipeInput;Lnet/minecraft/world/level/Level;)Ljava/util/Optional;";
    const SINGLE_INPUT_INIT: &str = "net/minecraft/world/item/crafting/SingleRecipeInput.<init>.(Lnet/minecraft/world/item/ItemStack;)V";
    const RESULT_ITEM: &str = "net/minecraft/world/item/crafting/Recipe.getResultItem.(Lnet/minecraft/core/HolderLookup$Provider;)Lnet/minecraft/world/item/ItemStack;";
    const SMELTING: &str = "net/minecraft/world/item/crafting/RecipeType.SMELTING";
    const BLASTING: &str = "net/minecraft/world/item/crafting/RecipeType.BLASTING";

    /// What `level.getRecipeManager().getRecipeFor(type, new SingleRecipeInput(item), level)`
    /// finds: the holder's id and its recipe.
    fn found(rt: &mut Rt, ty: &'static str, item: &str) -> Option<(String, V)> {
        let manager = rt.new_object("net/minecraft/world/item/crafting/RecipeManager");
        let ty = api::get_static(rt, ty);
        let input = rt.new_object(SINGLE_INPUT);
        let stack = api_common::stack(rt, item, 1);
        api::call(rt, SINGLE_INPUT_INIT, &[input, stack]);
        let o = api::call(rt, GET_RECIPE_FOR, &[manager, ty, input, V::NULL]);
        let h = rt.get_field(o, "value");
        if h.r() == 0 {
            return None;
        }
        let id = rt.get_field(h, "$id");
        Some((
            rt.str_of(id).unwrap_or_default().to_owned(),
            rt.get_field(h, "$value"),
        ))
    }

    fn item_of(rt: &Rt, stack: V) -> (String, i32) {
        (
            api_common::item_like_name(rt, rt.get_field(stack, "$item")),
            rt.get_field(stack, "$count").i(),
        )
    }

    /// Create's fans smelt as `getRecipeFor(SMELTING or BLASTING, new SingleRecipeInput(stack))`
    /// and the found recipe's `getResultItem`: the game's own recipe files, read as vanilla's
    /// serializers read them (a cooking time a file doesn't name is the kind's own).
    #[test]
    fn a_mod_finds_the_games_smelting_and_blasting_recipes() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        host.item_tags.insert(
            "minecraft:iron_ores".into(),
            vec![
                "minecraft:iron_ore".into(),
                "minecraft:deepslate_iron_ore".into(),
            ],
        );
        host.recipes.insert(
            "minecraft:smelting".into(),
            vec![
                ("minecraft:iron_ingot_from_smelting_raw_iron".into(), r#"{"type":"minecraft:smelting","category":"misc","cookingtime":200,"experience":0.7,"group":"iron_ingot","ingredient":"minecraft:raw_iron","result":{"id":"minecraft:iron_ingot"}}"#.into()),
                ("minecraft:stone".into(), r#"{"type":"minecraft:smelting","category":"blocks","experience":0.1,"ingredient":"minecraft:cobblestone","result":{"id":"minecraft:stone"}}"#.into()),
            ],
        );
        host.recipes.insert(
            "minecraft:blasting".into(),
            vec![("minecraft:iron_ingot_from_blasting_iron_ore".into(), r##"{"type":"minecraft:blasting","ingredient":"#minecraft:iron_ores","result":{"id":"minecraft:iron_ingot"},"experience":0.7}"##.into())],
        );
        rt.with_host(&mut host, |rt| {
            let (id, stone) = found(rt, SMELTING, "minecraft:cobblestone").expect("cobblestone smelts");
            assert_eq!(id, "minecraft:stone");
            let result = api::call(rt, RESULT_ITEM, &[stone, V::NULL]);
            assert_eq!(item_of(rt, result), ("minecraft:stone".into(), 1));
            let time = "net/minecraft/world/item/crafting/AbstractCookingRecipe.getCookingTime.()I";
            assert_eq!(api::call(rt, time, &[stone]), V::I(200), "smelting's own time");
            let category = api::call(rt, "net/minecraft/world/item/crafting/SmeltingRecipe.category.()Lnet/minecraft/world/item/crafting/CookingBookCategory;", &[stone]);
            assert_eq!(category, api::get_static(rt, "net/minecraft/world/item/crafting/CookingBookCategory.BLOCKS"));
            assert!(found(rt, SMELTING, "minecraft:raw_gold").is_none());
            assert!(found(rt, BLASTING, "minecraft:cobblestone").is_none());
            let (_, ore) = found(rt, BLASTING, "minecraft:deepslate_iron_ore").expect("the tag's ores blast");
            assert_eq!(api::call(rt, time, &[ore]), V::I(100), "blasting's own time");
            assert_eq!(api::call(rt, "net/minecraft/world/item/crafting/BlastingRecipe.getExperience.()F", &[ore]), V::F(0.7));
            // Vanilla's type and serializer objects, as a mod compares them.
            let ty = api::call(rt, "net/minecraft/world/item/crafting/Recipe.getType.()Lnet/minecraft/world/item/crafting/RecipeType;", &[ore]);
            assert_eq!(ty, api::get_static(rt, BLASTING));
            let serializer = api::call(rt, "net/minecraft/world/item/crafting/Recipe.getSerializer.()Lnet/minecraft/world/item/crafting/RecipeSerializer;", &[ore]);
            assert_eq!(serializer, api::get_static(rt, "net/minecraft/world/item/crafting/RecipeSerializer.BLASTING_RECIPE"));
            assert!(found(rt, "net/minecraft/world/item/crafting/RecipeType.SMOKING", "minecraft:beef").is_none());
            // The manager holds them with the rest: `byKey` finds one by its id.
            let manager = rt.new_object("net/minecraft/world/item/crafting/RecipeManager");
            let id = rt.string("minecraft:iron_ingot_from_blasting_iron_ore");
            let by_key = api::call(rt, "net/minecraft/world/item/crafting/RecipeManager.byKey.(Lnet/minecraft/resources/ResourceLocation;)Ljava/util/Optional;", &[manager, id]);
            let h = rt.get_field(by_key, "value");
            assert_eq!(rt.get_field(h, "$value"), ore);
            assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        });
    }

    /// The game's crafting recipes are one type: a shapeless one matches its ingredients in any
    /// order and place, each stack meeting a different one; a special recipe this runtime has no
    /// model of is left out, the rest read.
    #[test]
    fn a_shapeless_recipe_matches_its_ingredients_anywhere_in_the_grid() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        host.item_tags.insert(
            "minecraft:planks".into(),
            vec![
                "minecraft:oak_planks".into(),
                "minecraft:birch_planks".into(),
            ],
        );
        host.recipes.insert(
            "minecraft:crafting".into(),
            vec![
                ("minecraft:repair_item".into(), r#"{"type":"minecraft:crafting_special_repairitem","category":"misc"}"#.into()),
                ("minecraft:mossy_cobblestone_from_vine".into(), r#"{"type":"minecraft:crafting_shapeless","category":"building","ingredients":["minecraft:cobblestone","minecraft:vine"],"result":{"id":"minecraft:mossy_cobblestone"}}"#.into()),
                ("test:planks_and_sticks".into(), r##"{"type":"minecraft:crafting_shapeless","ingredients":["#minecraft:planks","#minecraft:planks",["minecraft:stick","minecraft:bamboo"]],"result":{"id":"minecraft:chest","count":2}}"##.into()),
            ],
        );
        rt.with_host(&mut host, |rt| {
            let all = recipes::holders(rt, "minecraft:crafting");
            assert_eq!(all.len(), 2, "the special recipe is left out");
            let grid = |rt: &mut Rt, cells: [&str; 9]| {
                let items: Vec<V> = cells.iter().map(|c| if c.is_empty() { empty_stack(rt) } else { api_common::stack(rt, &format!("minecraft:{c}"), 1) }).collect();
                let list = jutil::new_list(rt, items);
                call(rt, INPUT, "of", "(IILjava/util/List;)Lnet/minecraft/world/item/crafting/CraftingInput;", &[V::I(3), V::I(3), list]).unwrap()
            };
            let manager = rt.new_object("net/minecraft/world/item/crafting/RecipeManager");
            let crafting = api::get_static(rt, "net/minecraft/world/item/crafting/RecipeType.CRAFTING");
            let find = |rt: &mut Rt, input: V| {
                let o = api::call(rt, GET_RECIPE_FOR, &[manager, crafting, input, V::NULL]);
                let h = rt.get_field(o, "value");
                (h.r() != 0).then(|| rt.str_of(rt.get_field(h, "$id")).unwrap_or_default().to_owned())
            };
            let moss = grid(rt, ["", "vine", "", "", "", "", "cobblestone", "", ""]);
            assert_eq!(find(rt, moss).as_deref(), Some("minecraft:mossy_cobblestone_from_vine"));
            let chest = grid(rt, ["bamboo", "", "", "", "birch_planks", "", "", "", "oak_planks"]);
            assert_eq!(find(rt, chest).as_deref(), Some("test:planks_and_sticks"));
            // Each stack meets one ingredient: two sticks and a plank leave a plank unmet.
            let short = grid(rt, ["stick", "stick", "oak_planks", "", "", "", "", "", ""]);
            assert_eq!(find(rt, short), None);
            let extra = grid(rt, ["vine", "cobblestone", "vine", "", "", "", "", "", ""]);
            assert_eq!(find(rt, extra), None);
            let recipe = rt.get_field(all[1], "$value");
            let result = api::call(rt, "net/minecraft/world/item/crafting/CraftingRecipe.assemble.(Lnet/minecraft/world/item/crafting/RecipeInput;Lnet/minecraft/core/HolderLookup$Provider;)Lnet/minecraft/world/item/ItemStack;", &[recipe, chest, V::NULL]);
            assert_eq!(item_of(rt, result), ("minecraft:chest".into(), 2));
            assert!(rt.is_a(rt.class_of(recipe), "net/minecraft/world/item/crafting/CraftingRecipe"), "Create's press takes it");
            let fits = |rt: &mut Rt, w: i32, h: i32| api::call(rt, "net/minecraft/world/item/crafting/ShapelessRecipe.canCraftInDimensions.(II)Z", &[recipe, V::I(w), V::I(h)]);
            assert_eq!((fits(rt, 1, 2), fits(rt, 3, 1)), (V::I(0), V::I(1)));
            assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        });
        // Read as vanilla reads them: no ingredients, or more than the grid holds, is no recipe.
        let mut rt = Rt::new();
        let err =
            |rt: &mut Rt, j: Json| decode_map(rt, "shapeless_recipe", &[], &obj(j)).unwrap_err();
        assert_eq!(
            err(
                &mut rt,
                json!({"ingredients": [], "result": {"id": "stone"}})
            ),
            "ingredients: No ingredients for shapeless recipe"
        );
        let ten = vec![json!("stone"); 10];
        assert_eq!(
            err(
                &mut rt,
                json!({"ingredients": ten, "result": {"id": "stone"}})
            ),
            "ingredients: Too many ingredients for shapeless recipe. The maximum is: 9"
        );
    }

    /// A mod makes vanilla's recipes itself (a datagen, a recipe viewer's display): the cooking
    /// constructors give the kind's type and serializer, a stonecutter's takes its ingredient's
    /// stack, and what crafting leaves (a milk bucket's bucket) stays in the grid.
    #[test]
    fn a_mod_builds_vanilla_recipes_and_reads_them_back() {
        let mut rt = Rt::new();
        crate::install();
        let raw = recipes::ingredient(&mut rt, &["minecraft:raw_copper".into()]);
        let ingot = api_common::stack(&mut rt, "minecraft:copper_ingot", 1);
        let misc = api::get_static(
            &mut rt,
            "net/minecraft/world/item/crafting/CookingBookCategory.MISC",
        );
        let group = rt.string("copper");
        let smoking = rt.new_object("net/minecraft/world/item/crafting/SmokingRecipe");
        let init = "net/minecraft/world/item/crafting/SmokingRecipe.<init>.(Ljava/lang/String;Lnet/minecraft/world/item/crafting/CookingBookCategory;Lnet/minecraft/world/item/crafting/Ingredient;Lnet/minecraft/world/item/ItemStack;FI)V";
        api::call(
            &mut rt,
            init,
            &[smoking, group, misc, raw, ingot, V::F(0.35), V::I(100)],
        );
        let ty = api::call(
            &mut rt,
            "net/minecraft/world/item/crafting/SmokingRecipe.getType.()Lnet/minecraft/world/item/crafting/RecipeType;",
            &[smoking],
        );
        assert_eq!(
            ty,
            api::get_static(
                &mut rt,
                "net/minecraft/world/item/crafting/RecipeType.SMOKING"
            )
        );
        let serializer = api::call(
            &mut rt,
            "net/minecraft/world/item/crafting/SmokingRecipe.getSerializer.()Lnet/minecraft/world/item/crafting/RecipeSerializer;",
            &[smoking],
        );
        assert_eq!(
            serializer,
            api::get_static(
                &mut rt,
                "net/minecraft/world/item/crafting/RecipeSerializer.SMOKING_RECIPE"
            )
        );
        let toast = api::call(
            &mut rt,
            "net/minecraft/world/item/crafting/Recipe.getToastSymbol.()Lnet/minecraft/world/item/ItemStack;",
            &[smoking],
        );
        assert_eq!(item_of(&rt, toast).0, "minecraft:smoker");
        let ingredients = api::call(
            &mut rt,
            "net/minecraft/world/item/crafting/AbstractCookingRecipe.getIngredients.()Lnet/minecraft/core/NonNullList;",
            &[smoking],
        );
        assert_eq!(jutil::elements(&rt, ingredients), [raw]);
        // A stonecutter recipe matches its ingredient's stack; its result keeps its count.
        let stone = recipes::ingredient(&mut rt, &["minecraft:stone".into()]);
        let slabs = api_common::stack(&mut rt, "minecraft:stone_slab", 2);
        let cutter = rt.new_object(STONECUTTER);
        let empty = rt.string("");
        api::call(
            &mut rt,
            "net/minecraft/world/item/crafting/StonecutterRecipe.<init>.(Ljava/lang/String;Lnet/minecraft/world/item/crafting/Ingredient;Lnet/minecraft/world/item/ItemStack;)V",
            &[cutter, empty, stone, slabs],
        );
        let input = |rt: &mut Rt, item: &str| {
            let i = rt.new_object(SINGLE_INPUT);
            let s = api_common::stack(rt, item, 1);
            api::call(rt, SINGLE_INPUT_INIT, &[i, s]);
            i
        };
        let (on_stone, on_dirt) = (
            input(&mut rt, "minecraft:stone"),
            input(&mut rt, "minecraft:dirt"),
        );
        assert_eq!(vanilla_matches(&mut rt, cutter, on_stone), Some(true));
        assert_eq!(vanilla_matches(&mut rt, cutter, on_dirt), Some(false));
        let out = api::call(
            &mut rt,
            "net/minecraft/world/item/crafting/StonecutterRecipe.assemble.(Lnet/minecraft/world/item/crafting/RecipeInput;Lnet/minecraft/core/HolderLookup$Provider;)Lnet/minecraft/world/item/ItemStack;",
            &[cutter, on_stone, V::NULL],
        );
        assert_eq!(item_of(&rt, out), ("minecraft:stone_slab".into(), 2));
        assert_ne!(out, slabs, "a copy");
        let is_a = |rt: &Rt, o: V, class: &str| rt.is_a(rt.class_of(o), class);
        assert!(
            is_a(&rt, smoking, COOKING)
                && is_a(&rt, cutter, SINGLE_ITEM)
                && is_a(&rt, cutter, "net/minecraft/world/item/crafting/Recipe")
        );
        assert!(!is_a(
            &rt,
            smoking,
            "net/minecraft/world/item/crafting/CraftingRecipe"
        ));
        let ty = api::call(
            &mut rt,
            "net/minecraft/world/item/crafting/SingleItemRecipe.getType.()Lnet/minecraft/world/item/crafting/RecipeType;",
            &[cutter],
        );
        assert_eq!(
            ty,
            api::get_static(
                &mut rt,
                "net/minecraft/world/item/crafting/RecipeType.STONECUTTING"
            )
        );
        // Crafting a cake leaves the milk buckets' buckets.
        let cells: Vec<V> = ["milk_bucket", "sugar", "egg"]
            .iter()
            .map(|c| api_common::stack(&mut rt, &format!("minecraft:{c}"), 1))
            .collect();
        let list = jutil::new_list(&mut rt, cells);
        let grid = call(
            &mut rt,
            INPUT,
            "of",
            "(IILjava/util/List;)Lnet/minecraft/world/item/crafting/CraftingInput;",
            &[V::I(3), V::I(1), list],
        )
        .unwrap();
        let shapeless = rt.new_object(SHAPELESS);
        let left = call(
            &mut rt,
            SHAPELESS,
            "getRemainingItems",
            "(Lnet/minecraft/world/item/crafting/CraftingInput;)Lnet/minecraft/core/NonNullList;",
            &[shapeless, grid],
        )
        .unwrap();
        let left: Vec<String> = jutil::elements(&rt, left)
            .into_iter()
            .map(|s| {
                if api_common::stack_empty(&rt, s) {
                    String::new()
                } else {
                    item_of(&rt, s).0
                }
            })
            .collect();
        assert_eq!(left, ["minecraft:bucket", "", ""]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
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

    /// Create checks a recipe's serializer against a tag before it automates the recipe:
    /// `RECIPE_SERIALIZER.getHolder(RECIPE_SERIALIZER.getResourceKey(serializer).orElseThrow())`.
    /// Minecraft's own serializers and types are in their registries under vanilla's names.
    #[test]
    fn vanillas_recipe_serializers_and_types_are_in_their_registries() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| {
            const REGISTRY: &str = "net/minecraft/core/Registry";
            let serializers = api::get_static(rt, "net/minecraft/core/registries/BuiltInRegistries.RECIPE_SERIALIZER");
            let shapeless = api::get_static(rt, "net/minecraft/world/item/crafting/RecipeSerializer.SHAPELESS_RECIPE");
            let key = api::call(rt, "net/minecraft/core/Registry.getResourceKey.(Ljava/lang/Object;)Ljava/util/Optional;", &[serializers, shapeless]);
            let key = rt.get_field(key, "value");
            assert_ne!(key, V::NULL, "the shapeless serializer is registered");
            let holder = api::call(rt, "net/minecraft/core/Registry.getHolder.(Lnet/minecraft/resources/ResourceKey;)Ljava/util/Optional;", &[serializers, key]);
            let holder = rt.get_field(holder, "value");
            assert_eq!(api::call(rt, "net/minecraft/core/Holder.value.()Ljava/lang/Object;", &[holder]), shapeless);
            let name = api::call(rt, "net/minecraft/core/Holder.getRegisteredName.()Ljava/lang/String;", &[holder]);
            assert_eq!(rt.str_of(name), Some("minecraft:crafting_shapeless"));
            // Create's `automation_ignore` holds other mods' serializers, none of vanilla's.
            let registry = api::get_static(rt, "net/minecraft/core/registries/Registries.RECIPE_SERIALIZER");
            let location = rt.string("create:automation_ignore");
            let tag = api::call(rt, "net/minecraft/tags/TagKey.create.(Lnet/minecraft/resources/ResourceKey;Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/tags/TagKey;", &[registry, location]);
            assert_eq!(api::call(rt, "net/minecraft/core/Holder$Reference.is.(Lnet/minecraft/tags/TagKey;)Z", &[holder, tag]), V::I(0));
            // Recipe types, by name and back.
            let types = api::get_static(rt, "net/minecraft/core/registries/BuiltInRegistries.RECIPE_TYPE");
            let get_key = format!("{REGISTRY}.getKey.(Ljava/lang/Object;)Lnet/minecraft/resources/ResourceLocation;");
            let get_key: &'static str = rt.static_key(&get_key);
            let smelting = api::get_static(rt, SMELTING);
            let name = api::call(rt, get_key, &[types, smelting]);
            assert_eq!(rt.str_of(name), Some("minecraft:smelting"));
            let get = "net/minecraft/core/Registry.get.(Lnet/minecraft/resources/ResourceLocation;)Ljava/lang/Object;";
            let blasting = rt.string("minecraft:blasting");
            assert_eq!(api::call(rt, get, &[types, blasting]), api::get_static(rt, BLASTING));
            // A type made apart from the registry has no name in it, and vanilla has no type of
            // a mod's name.
            let press = rt.string("mymod:press");
            let own = api::call(rt, "net/minecraft/world/item/crafting/RecipeType.simple.(Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/world/item/crafting/RecipeType;", &[press]);
            assert_eq!(api::call(rt, get_key, &[types, own]), V::NULL);
            let pressing = rt.string("minecraft:pressing");
            assert_eq!(api::call(rt, get, &[types, pressing]), V::NULL);
            assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        });
    }

    /// The game's crafting grid (`TransientCraftingContainer`) is the input a recipe reads: its
    /// stacks trimmed to those it holds, and where they start. Create's basin crafts through a grid
    /// of no rows, the empty input; a one-by-one grid stays whole even empty, as vanilla's does.
    #[test]
    fn a_crafting_grid_gives_the_input_a_recipe_reads() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        host.recipes.insert(
            "minecraft:crafting".into(),
            vec![("minecraft:stick".into(), r##"{"type":"minecraft:crafting_shaped","category":"misc","key":{"#":"minecraft:oak_planks"},"pattern":["#","#"],"result":{"count":4,"id":"minecraft:stick"}}"##.into())],
        );
        rt.with_host(&mut host, |rt| {
            const GRID: &str = "net/minecraft/world/inventory/TransientCraftingContainer";
            const WITH_ITEMS: &str = "net/minecraft/world/inventory/TransientCraftingContainer.<init>.(Lnet/minecraft/world/inventory/AbstractContainerMenu;IILnet/minecraft/core/NonNullList;)V";
            const SIZED: &str = "net/minecraft/world/inventory/TransientCraftingContainer.<init>.(Lnet/minecraft/world/inventory/AbstractContainerMenu;II)V";
            const AS_INPUT: &str = "net/minecraft/world/inventory/CraftingContainer.asCraftInput.()Lnet/minecraft/world/item/crafting/CraftingInput;";
            const GET: &str = "net/minecraft/world/Container.getItem.(I)Lnet/minecraft/world/item/ItemStack;";
            let cells: Vec<V> = (0..9).map(|i| if i == 1 || i == 4 { api_common::stack(rt, "minecraft:oak_planks", 1) } else { empty_stack(rt) }).collect();
            let list = rt.alloc(NON_NULL_LIST, Data::List(cells));
            let grid = rt.new_object(GRID);
            api::call(rt, WITH_ITEMS, &[grid, V::NULL, V::I(3), V::I(3), list]);
            assert!(rt.is_a(GRID, "net/minecraft/world/inventory/CraftingContainer") && rt.is_a(GRID, "net/minecraft/world/Container"));
            let input = api::call(rt, AS_INPUT, &[grid]);
            let size = |rt: &mut Rt, input: V| (call(rt, INPUT, "width", "()I", &[input]), call(rt, INPUT, "height", "()I", &[input]));
            assert_eq!(size(rt, input), (Some(V::I(1)), Some(V::I(2))), "the middle column's top two");
            let at = api::call(rt, "net/minecraft/world/inventory/CraftingContainer.asPositionedCraftInput.()Lnet/minecraft/world/item/crafting/CraftingInput$Positioned;", &[grid]);
            assert_eq!((rt.get_field(at, "left"), rt.get_field(at, "top")), (V::I(1), V::I(0)));
            let crafting = api::get_static(rt, "net/minecraft/world/item/crafting/RecipeType.CRAFTING");
            let manager = rt.new_object("net/minecraft/world/item/crafting/RecipeManager");
            let found = api::call(rt, GET_RECIPE_FOR, &[manager, crafting, input, V::NULL]);
            let holder = rt.get_field(found, "value");
            assert_eq!(rt.str_of(rt.get_field(holder, "$id")), Some("minecraft:stick"));
            // What `getItems` gives is a copy; `removeItemNoUpdate` takes the stack from the grid.
            let copy = api::call(rt, "net/minecraft/world/inventory/CraftingContainer.getItems.()Ljava/util/List;", &[grid]);
            let stone = api_common::stack(rt, "minecraft:stone", 1);
            api::call(rt, "java/util/List.set.(ILjava/lang/Object;)Ljava/lang/Object;", &[copy, V::I(0), stone]);
            let first = api::call(rt, GET, &[grid, V::I(0)]);
            assert!(api_common::stack_empty(rt, first));
            let taken = api::call(rt, "net/minecraft/world/Container.removeItemNoUpdate.(I)Lnet/minecraft/world/item/ItemStack;", &[grid, V::I(4)]);
            assert_eq!(item_of(rt, taken), ("minecraft:oak_planks".into(), 1));
            let left = api::call(rt, AS_INPUT, &[grid]);
            assert_eq!(size(rt, left), (Some(V::I(1)), Some(V::I(1))));
            // A grid of no rows, as Create's basin makes one, and an empty one-by-one grid.
            let none = rt.new_object(GRID);
            api::call(rt, SIZED, &[none, V::NULL, V::I(0), V::I(0)]);
            let nothing = api::call(rt, AS_INPUT, &[none]);
            assert_eq!(nothing, get_static(rt, INPUT, "EMPTY").unwrap());
            let one = rt.new_object(GRID);
            api::call(rt, SIZED, &[one, V::NULL, V::I(1), V::I(1)]);
            let whole = api::call(rt, AS_INPUT, &[one]);
            assert_eq!(size(rt, whole), (Some(V::I(1)), Some(V::I(1))));
            assert_eq!(call(rt, INPUT, "isEmpty", "()Z", &[whole]), Some(V::I(1)));
            // Setting a stack tells the grid's menu: a grid made without one throws, as vanilla's.
            assert!(rt.thrown.is_none());
            api::call(rt, "net/minecraft/world/Container.setItem.(ILnet/minecraft/world/item/ItemStack;)V", &[one, V::I(0), stone]);
            let thrown = rt.thrown.take().expect("no menu to tell");
            assert_eq!(rt.class_of(thrown), "java/lang/NullPointerException");
            assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        });
    }

    /// A mod's recipe has `Recipe`'s default methods where it declares none: what a grid leaves
    /// (Create's basin asks its own compacting recipe), not special, a notification, no group.
    #[test]
    fn a_mods_recipe_takes_the_default_methods_of_recipe() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| {
            rt.supers.insert("t/Compacting", "java/lang/Object");
            rt.interfaces.insert("t/Compacting", &["net/minecraft/world/item/crafting/Recipe"]);
            let recipe = rt.new_object("t/Compacting");
            let stacks = vec![api_common::stack(rt, "minecraft:milk_bucket", 1), api_common::stack(rt, "minecraft:sugar", 1)];
            let list = jutil::new_list(rt, stacks);
            let input = call(rt, INPUT, "of", "(IILjava/util/List;)Lnet/minecraft/world/item/crafting/CraftingInput;", &[V::I(2), V::I(1), list]).unwrap();
            // As Create's code names the call: the method of a class of its own, which only the
            // interface has code for.
            let left = api::call(rt, "java/lang/Object.getRemainingItems.(Lnet/minecraft/world/item/crafting/RecipeInput;)Lnet/minecraft/core/NonNullList;", &[recipe, input]);
            let left: Vec<String> = jutil::elements(rt, left).into_iter().map(|s| if api_common::stack_empty(rt, s) { String::new() } else { item_of(rt, s).0 }).collect();
            assert_eq!(left, ["minecraft:bucket", ""]);
            let default = |rt: &mut Rt, name: &str, ret: &str| {
                let key: &'static str = rt.static_key(&format!("net/minecraft/world/item/crafting/Recipe.{name}.(){ret}"));
                api::call(rt, key, &[recipe])
            };
            assert_eq!(default(rt, "isSpecial", "Z"), V::I(0));
            assert_eq!(default(rt, "showNotification", "Z"), V::I(1));
            let group = default(rt, "getGroup", "Ljava/lang/String;");
            assert_eq!(rt.str_of(group), Some(""));
            assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        });
    }
}
