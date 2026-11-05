//! Recipes as a translated mod reads and runs them, natively: its own recipe types (Create's
//! pressing, milling, mixing...) read from the game's recipe files by the mod's own serializers'
//! codecs ([`crate::codecs`]), the `RecipeManager` that finds them, and the vanilla and NeoForge
//! pieces those recipes are made of: `Ingredient`, `FluidIngredient`, `SizedFluidIngredient`,
//! `RecipeHolder`, `SingleRecipeInput`, `RecipeType`, and the codecs of Minecraft's own values a
//! recipe file holds (`Ingredient.CODEC`, `ItemStack.CODEC`, `FluidStack.CODEC`...).
//!
//! The game hands over a type's recipe files (`Host::recipes_of_type`: data packs merged, load
//! conditions held); each is read once per runtime by the serializer its `type` names, as
//! `Recipe.CODEC` dispatches, and kept in a list per type. An ingredient keeps what it names (item
//! ids and `#tags`) and asks the game what a tag holds (`Host::ingredient_items`) when it tests a
//! stack, so it follows the game's tags, the content mods' included.
use crate::{Data, Rt, V, api_common, codecs, fluids, jutil};
use serde_json::{Map, Value as Json};

const INGREDIENT: &str = "net/minecraft/world/item/crafting/Ingredient";
const TAG_VALUE: &str = "net/minecraft/world/item/crafting/Ingredient$TagValue";
const ITEM_VALUE: &str = "net/minecraft/world/item/crafting/Ingredient$ItemValue";
const HOLDER: &str = "net/minecraft/world/item/crafting/RecipeHolder";
const MANAGER: &str = "net/minecraft/world/item/crafting/RecipeManager";
const BREWING: &str = "net/minecraft/world/item/alchemy/PotionBrewing";
const RECIPE_TYPE: &str = "net/minecraft/world/item/crafting/RecipeType";
const SINGLE_INPUT: &str = "net/minecraft/world/item/crafting/SingleRecipeInput";
const RECIPE_INPUT: &str = "net/minecraft/world/item/crafting/RecipeInput";
const FLUID_INGREDIENT: &str = "net/neoforged/neoforge/fluids/crafting/FluidIngredient";
const SIZED_FLUID: &str = "net/neoforged/neoforge/fluids/crafting/SizedFluidIngredient";
const COMPONENT_PATCH: &str = "net/minecraft/core/component/DataComponentPatch";
const STACK_ARRAY: &str = "[Lnet/minecraft/world/item/ItemStack;";
const FLUID_STACK_ARRAY: &str = "[Lnet/neoforged/neoforge/fluids/FluidStack;";
const SERIALIZER_CODEC: &str = "net/minecraft/world/item/crafting/RecipeSerializer.codec.()Lcom/mojang/serialization/MapCodec;";
const MATCHES: &str = "net/minecraft/world/item/crafting/Recipe.matches.(Lnet/minecraft/world/item/crafting/RecipeInput;Lnet/minecraft/world/level/Level;)Z";
/// A bucket, in millibuckets (`FluidType.BUCKET_VOLUME`).
const BUCKET: i64 = 1000;

/// `minecraft:` for a bare id, as `ResourceLocation.parse` reads one.
fn qualified(id: &str) -> String {
    if id.contains(':') { id.to_owned() } else { format!("minecraft:{id}") }
}

/// Whether `id` is a valid resource location (`ResourceLocation.isValidNamespace`/`isValidPath`).
pub(crate) fn valid_location(id: &str) -> bool {
    let (ns, path) = id.split_once(':').unwrap_or(("minecraft", id));
    !path.is_empty()
        && ns.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.'))
        && path.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.' | b'/'))
}

fn optional(rt: &mut Rt, v: Option<V>) -> V {
    let o = rt.new_object("java/util/Optional");
    rt.put_field(o, "value", v.unwrap_or(V::NULL));
    o
}

fn text(rt: &Rt, v: V) -> String {
    rt.str_of(v).unwrap_or("").to_owned()
}

// ---- Items and the game's tags ----

/// Whether the game has an item `id`: one the mod registered, else one the game knows (vanilla's,
/// a content mod's). Without a game (tests and tools), every `minecraft:` id.
pub(crate) fn item_known(rt: &mut Rt, id: &str) -> bool {
    if rt.registered.contains_key(&("item".to_owned(), id.to_owned())) {
        return true;
    }
    match rt.host() {
        Some(h) => !h.ingredient_items(id).is_empty(),
        None => id.starts_with("minecraft:"),
    }
}

/// The items of tag `#tag`, as the game's tags hold them; kept once known (a tag the game doesn't
/// have yet is asked again).
fn tag_items(rt: &mut Rt, tag: &str) -> Vec<String> {
    let key = format!("compat:tag-items/{tag}");
    if let Some(&list) = rt.interned.get(&key) {
        return jutil::elements(rt, list).into_iter().map(|v| text(rt, v)).collect();
    }
    let items = rt.host().map(|h| h.ingredient_items(tag)).unwrap_or_default();
    if !items.is_empty() {
        let values: Vec<V> = items.iter().map(|i| rt.string(i)).collect();
        let list = jutil::new_list(rt, values);
        rt.interned.insert(key, list);
    }
    items
}

/// Whether `item` is in the item tag `tag` (no `#`), as the game's tags hold it.
pub(crate) fn item_in_tag(rt: &mut Rt, item: &str, tag: &str) -> bool {
    tag_items(rt, &format!("#{tag}")).iter().any(|i| i == item)
}

/// The registered name of the item a stack holds (`create:iron_sheet`); `minecraft:air` for an
/// empty stack.
fn stack_item(rt: &Rt, stack: V) -> String {
    if api_common::stack_empty(rt, stack) {
        return "minecraft:air".into();
    }
    api_common::item_like_name(rt, rt.get_field(stack, "$item"))
}

// ---- Ingredient ----

/// An ingredient of the entries it names: item ids and `#tags`.
pub fn ingredient(rt: &mut Rt, entries: &[String]) -> V {
    let o = rt.new_object(INGREDIENT);
    let s = rt.string(&entries.join(" "));
    rt.put_field(o, "$entries", s);
    o
}

/// `Ingredient.EMPTY`.
pub(crate) fn empty_ingredient(rt: &mut Rt) -> V {
    rt.intern("compat:ingredient/empty".into(), |rt| ingredient(rt, &[]))
}

/// What an ingredient names: item ids and `#tags`.
pub(crate) fn ingredient_entries(rt: &Rt, ingredient: V) -> Vec<String> {
    entries(rt, ingredient)
}

fn entries(rt: &Rt, ingredient: V) -> Vec<String> {
    rt.str_of(rt.get_field(ingredient, "$entries")).map(|s| s.split_whitespace().map(str::to_owned).collect()).unwrap_or_default()
}

/// The items an ingredient holds, in order without repeats: its items, its tags' items.
pub fn ingredient_items(rt: &mut Rt, ingredient: V) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in entries(rt, ingredient) {
        let more = if e.starts_with('#') { tag_items(rt, &e) } else { vec![e] };
        for i in more {
            if !out.contains(&i) {
                out.push(i);
            }
        }
    }
    out
}

/// `Ingredient.test(stack)`: an empty ingredient takes only an empty stack.
pub fn ingredient_test(rt: &mut Rt, ingredient: V, stack: V) -> bool {
    let entries = entries(rt, ingredient);
    let empty = api_common::stack_empty(rt, stack);
    if entries.is_empty() || empty {
        return entries.is_empty() && empty;
    }
    let item = stack_item(rt, stack);
    entries.iter().any(|e| if e.starts_with('#') { tag_items(rt, e).contains(&item) } else { *e == item })
}

/// An ingredient's entry for an `ItemLike` (an item, a block, a holder).
fn item_like_entry(rt: &mut Rt, v: V) -> String {
    let item = crate::creative::as_item(rt, v);
    api_common::item_like_name(rt, item)
}

/// An ingredient's entry for a `TagKey`.
fn tag_entry(rt: &Rt, tag: V) -> String {
    format!("#{}", text(rt, rt.get_field(tag, "$location")))
}

/// `Ingredient.CODEC`'s reading of an ingredient: `{"item": id}`, `{"tag": tag}`, a list of them, or
/// one of NeoForge's own (`neoforge:compound` and the others, read for the items they name).
fn read_ingredient(rt: &mut Rt, j: &Json, out: &mut Vec<String>) -> Result<(), String> {
    match j {
        Json::Array(items) => {
            for i in items {
                read_ingredient(rt, i, out)?;
            }
            Ok(())
        }
        Json::Object(o) => {
            // NeoForge's own kinds, under `type` (or 1.20.4's `neoforge:ingredient_type`); their
            // children under `children` or `ingredients`, as NeoForge's codecs take either.
            if let Some(t) = o.get("type").or_else(|| o.get("neoforge:ingredient_type")).and_then(Json::as_str) {
                let children = || o.get("children").or_else(|| o.get("ingredients")).ok_or_else(|| "No key children in MapLike".to_owned());
                return match qualified(t).as_str() {
                    "neoforge:compound" => read_ingredient(rt, children()?, out),
                    "neoforge:components" => read_ingredient(rt, o.get("items").ok_or("No key items in MapLike")?, out),
                    // The items every child holds.
                    "neoforge:intersection" => {
                        let Json::Array(subs) = children()? else { return Err("children: Not a list".into()) };
                        let mut kept: Option<Vec<String>> = None;
                        for sub in subs {
                            let items = items_of(rt, sub)?;
                            kept = Some(match kept {
                                None => items,
                                Some(k) => k.into_iter().filter(|i| items.contains(i)).collect(),
                            });
                        }
                        out.extend(kept.unwrap_or_default());
                        Ok(())
                    }
                    // The base's items without the subtracted's.
                    "neoforge:difference" => {
                        let base = items_of(rt, o.get("base").ok_or("No key base in MapLike")?)?;
                        let gone = items_of(rt, o.get("subtracted").ok_or("No key subtracted in MapLike")?)?;
                        out.extend(base.into_iter().filter(|i| !gone.contains(i)));
                        Ok(())
                    }
                    "neoforge:block_tag" => Err("block tag ingredients are not read".into()),
                    other => Err(format!("Unknown ingredient type {other}")),
                };
            }
            match (o.get("item"), o.get("tag")) {
                (Some(Json::String(id)), None) => {
                    let id = qualified(id);
                    if !valid_location(&id) || !item_known(rt, &id) {
                        return Err(format!("Unknown item {id}"));
                    }
                    out.push(id);
                    Ok(())
                }
                (None, Some(Json::String(tag))) => {
                    out.push(format!("#{}", qualified(tag)));
                    Ok(())
                }
                _ => Err(format!("Not an ingredient: {j}")),
            }
        }
        // A string as 26.3 and NeoForge's simplified form write one: an id or `#tag`.
        Json::String(s) => {
            let entry = match s.strip_prefix('#') {
                Some(tag) => format!("#{}", qualified(tag)),
                None => qualified(s),
            };
            out.push(entry);
            Ok(())
        }
        _ => Err(format!("Not an ingredient: {j}")),
    }
}

/// The items an ingredient's JSON holds, its tags' included: what an intersection or a
/// difference compares.
fn items_of(rt: &mut Rt, j: &Json) -> Result<Vec<String>, String> {
    let mut entries = Vec::new();
    read_ingredient(rt, j, &mut entries)?;
    let mut out: Vec<String> = Vec::new();
    for e in entries {
        let more = if e.starts_with('#') { tag_items(rt, &e) } else { vec![e] };
        for i in more {
            if !out.contains(&i) {
                out.push(i);
            }
        }
    }
    Ok(out)
}

fn ingredient_call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        (INGREDIENT, "of") => {
            let mut out = Vec::new();
            if desc.starts_with("(Lnet/minecraft/tags/TagKey;") {
                out.push(tag_entry(rt, a(0)));
            } else if desc.starts_with("([Lnet/minecraft/world/level/ItemLike;") {
                for v in jutil::elements(rt, a(0)) {
                    out.push(item_like_entry(rt, v));
                }
            } else if desc.starts_with("([Lnet/minecraft/world/item/ItemStack;") || desc.starts_with("(Ljava/util/stream/Stream;") {
                for s in jutil::elements(rt, a(0)) {
                    if !api_common::stack_empty(rt, s) {
                        out.push(stack_item(rt, s));
                    }
                }
            } else if desc.starts_with("()") {
            } else {
                return None;
            }
            ingredient(rt, &out)
        }
        (INGREDIENT, "fromValues") => {
            let mut out = Vec::new();
            for v in jutil::elements(rt, a(0)) {
                out.push(text(rt, rt.get_field(v, "$entry")));
            }
            ingredient(rt, &out)
        }
        (TAG_VALUE, "<init>") => {
            let e = tag_entry(rt, a(1));
            let s = rt.string(&e);
            rt.put_field(a(0), "$entry", s);
            V::Void
        }
        (ITEM_VALUE, "<init>") => {
            let e = stack_item(rt, a(1));
            let s = rt.string(&e);
            rt.put_field(a(0), "$entry", s);
            V::Void
        }
        (TAG_VALUE, "tag") => {
            let e = text(rt, rt.get_field(a(0), "$entry"));
            fluids::tag_key(rt, "item", e.trim_start_matches('#'))
        }
        (TAG_VALUE | ITEM_VALUE | "net/minecraft/world/item/crafting/Ingredient$Value", "getItems") => {
            let e = text(rt, rt.get_field(a(0), "$entry"));
            let items = if e.starts_with('#') { tag_items(rt, &e) } else { vec![e] };
            let stacks: Vec<V> = items.iter().map(|i| api_common::stack(rt, i, 1)).collect();
            jutil::new_list(rt, stacks)
        }
        (INGREDIENT, "test") => V::I(ingredient_test(rt, a(0), a(1)) as i32),
        (INGREDIENT, "getItems") => {
            let items = ingredient_items(rt, a(0));
            let stacks: Vec<V> = items.iter().map(|i| api_common::stack(rt, i, 1)).collect();
            rt.alloc(STACK_ARRAY, Data::Array(stacks))
        }
        (INGREDIENT, "isEmpty") => V::I(entries(rt, a(0)).is_empty() as i32),
        (INGREDIENT, "hasNoItems") => V::I(ingredient_items(rt, a(0)).is_empty() as i32),
        (INGREDIENT, "isSimple") => V::I(1),
        (INGREDIENT, "isCustom") => V::I(0),
        (INGREDIENT, "getCustomIngredient") => V::NULL,
        (INGREDIENT, "equals") => V::I((rt.class_of(a(1)) == INGREDIENT && entries(rt, a(0)) == entries(rt, a(1))) as i32),
        (INGREDIENT, "hashCode") => {
            let e = entries(rt, a(0)).join(" ");
            let s = rt.string(&e);
            V::I(rt.java_hash(s))
        }
        (INGREDIENT, "toString") => {
            let e = format!("Ingredient{{{}}}", entries(rt, a(0)).join(", "));
            rt.string(&e)
        }
        _ => return None,
    })
}

// ---- Fluid ingredients ----

/// A `FluidIngredient` of fluid ids and `#tags`.
fn fluid_ingredient(rt: &mut Rt, entries: &[String]) -> V {
    let o = rt.new_object(FLUID_INGREDIENT);
    let s = rt.string(&entries.join(" "));
    rt.put_field(o, "$entries", s);
    o
}

/// The data components a fluid ingredient requires of a stack (`neoforge:components`), kept as
/// `{"components": {...}, "strict": bool}`.
fn requires(rt: &mut Rt, ingredient: V, needs: Option<Json>) {
    if let Some(n) = needs {
        let s = rt.string(&n.to_string());
        rt.put_field(ingredient, "$requires", s);
    }
}

/// Whether a fluid stack carries the components its ingredient requires: all of them (and, when
/// strict, only them). Fluid stacks carry their components in `$components` (JSON), none so far.
fn carries_required(rt: &Rt, ingredient: V, stack: V) -> bool {
    let Some(needs) = rt.str_of(rt.get_field(ingredient, "$requires")) else { return true };
    let needs: Json = serde_json::from_str(needs).unwrap_or_default();
    let has: Json = rt.str_of(rt.get_field(stack, "$components")).and_then(|c| serde_json::from_str(c).ok()).unwrap_or_else(|| Json::Object(Map::new()));
    let (Some(want), Some(have)) = (needs["components"].as_object(), has.as_object()) else { return false };
    let all = want.iter().all(|(k, v)| have.get(k) == Some(v));
    all && (!needs["strict"].as_bool().unwrap_or(false) || want.len() == have.len())
}

/// `SizedFluidIngredient`: an ingredient and an amount in millibuckets.
fn sized_fluid(rt: &mut Rt, ingredient: V, amount: i32) -> V {
    let o = rt.new_object(SIZED_FLUID);
    rt.put_field(o, "$ingredient", ingredient);
    rt.put_field(o, "$amount", V::I(amount));
    o
}

/// The fluids a fluid ingredient names: its fluids, and its tags' as far as this runtime knows them.
fn fluid_ingredient_fluids(rt: &mut Rt, ingredient: V) -> Vec<V> {
    let mut out = Vec::new();
    for e in entries(rt, ingredient) {
        if let Some(tag) = e.strip_prefix('#') {
            let key = fluids::tag_key(rt, "fluid", tag);
            for name in ["minecraft:water", "minecraft:lava"] {
                if fluids::in_tag(rt, key, name) {
                    out.push(fluids::fluid(rt, name));
                }
            }
        } else {
            let f = fluids::fluid(rt, &e);
            if f.r() != 0 {
                out.push(f);
            }
        }
    }
    out
}

fn fluid_ingredient_test(rt: &mut Rt, ingredient: V, stack: V) -> bool {
    if fluids::stack_empty(rt, stack) || !carries_required(rt, ingredient, stack) {
        return false;
    }
    let f = rt.get_field(stack, "$fluid");
    let name = fluids::name_of(rt, f);
    entries(rt, ingredient).iter().any(|e| match e.strip_prefix('#') {
        Some(tag) => {
            let key = fluids::tag_key(rt, "fluid", tag);
            fluids::in_tag(rt, key, &name)
        }
        None => *e == name,
    })
}

/// `FluidIngredient.MAP_CODEC_NONEMPTY`'s reading: `{"type": "neoforge:single", "fluid": id}`,
/// `{"type": "neoforge:tag", "tag": tag}`, `neoforge:compound`'s children, or the untyped
/// `{"fluid": id}` and `{"tag": tag}`.
/// `needs` takes the data components a `neoforge:components` ingredient requires of a stack.
fn read_fluid_ingredient(rt: &mut Rt, o: &Map<String, Json>, out: &mut Vec<String>, needs: &mut Option<Json>) -> Result<(), String> {
    let ty = o.get("type").and_then(Json::as_str).map(qualified);
    match ty.as_deref() {
        Some("neoforge:compound") => {
            let Some(Json::Array(children)) = o.get("children").or_else(|| o.get("ingredients")) else { return Err("No key children in MapLike".into()) };
            for c in children {
                let Json::Object(c) = c else { return Err(format!("Not a map: {c}")) };
                read_fluid_ingredient(rt, c, out, needs)?;
            }
            return Ok(());
        }
        // `DataComponentFluidIngredient`: fluids (an id, a `#tag` or a list) whose stack carries
        // the components given (all of them; `strict`, only them).
        Some("neoforge:components") => {
            let fluids = o.get("fluids").ok_or("No key fluids in MapLike")?;
            let named: Vec<&Json> = match fluids {
                Json::Array(a) => a.iter().collect(),
                j => vec![j],
            };
            for f in named {
                let Json::String(f) = f else { return Err(format!("fluids: Not a string: {f}")) };
                match f.strip_prefix('#') {
                    Some(tag) => out.push(format!("#{}", qualified(tag))),
                    None => {
                        let id = qualified(f);
                        if fluids::fluid(rt, &id).r() == 0 {
                            return Err(format!("Unknown registry key in fluid: {id}"));
                        }
                        out.push(id);
                    }
                }
            }
            let Some(Json::Object(components)) = o.get("components") else { return Err("No key components in MapLike".into()) };
            let strict = o.get("strict").and_then(Json::as_bool).unwrap_or(false);
            *needs = Some(serde_json::json!({"components": components, "strict": strict}));
            return Ok(());
        }
        Some("neoforge:single" | "neoforge:tag") | None => {}
        Some(other) => return Err(format!("Unknown fluid ingredient type {other}")),
    }
    match (o.get("fluid"), o.get("tag")) {
        (Some(Json::String(id)), _) if ty.as_deref() != Some("neoforge:tag") => {
            let id = qualified(id);
            if fluids::fluid(rt, &id).r() == 0 {
                return Err(format!("Unknown registry key in fluid: {id}"));
            }
            out.push(id);
            Ok(())
        }
        (_, Some(Json::String(tag))) if ty.as_deref() != Some("neoforge:single") => {
            out.push(format!("#{}", qualified(tag)));
            Ok(())
        }
        _ => Err(format!("Not a fluid ingredient: {}", Json::Object(o.clone()))),
    }
}

fn fluid_ingredient_call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        (SIZED_FLUID, "<init>") => {
            rt.put_field(a(0), "$ingredient", a(1));
            rt.put_field(a(0), "$amount", a(2));
            V::Void
        }
        (SIZED_FLUID, "of") => {
            let entry = if desc.starts_with("(Lnet/minecraft/tags/TagKey;") { tag_entry(rt, a(0)) } else { fluids::name_of(rt, a(0)) };
            let i = fluid_ingredient(rt, &[entry]);
            sized_fluid(rt, i, a(1).i())
        }
        (SIZED_FLUID, "ingredient") => rt.get_field(a(0), "$ingredient"),
        (SIZED_FLUID, "amount") => rt.get_field(a(0), "$amount"),
        (SIZED_FLUID, "test") => {
            let i = rt.get_field(a(0), "$ingredient");
            let enough = fluids::stack_amount(rt, a(1)) >= rt.get_field(a(0), "$amount").i();
            V::I((enough && fluid_ingredient_test(rt, i, a(1))) as i32)
        }
        (SIZED_FLUID, "getFluids") => {
            let i = rt.get_field(a(0), "$ingredient");
            let amount = rt.get_field(a(0), "$amount").i();
            let stacks: Vec<V> = fluid_ingredient_fluids(rt, i).into_iter().map(|f| fluids::fluid_stack(rt, f, amount)).collect();
            rt.alloc(FLUID_STACK_ARRAY, Data::Array(stacks))
        }
        (FLUID_INGREDIENT, "single") => {
            let n = if desc.starts_with("(Lnet/neoforged/neoforge/fluids/FluidStack;") {
                let f = rt.get_field(a(0), "$fluid");
                fluids::name_of(rt, f)
            } else {
                fluids::name_of(rt, a(0))
            };
            fluid_ingredient(rt, &[n])
        }
        (FLUID_INGREDIENT, "tag") => {
            let e = tag_entry(rt, a(0));
            fluid_ingredient(rt, &[e])
        }
        (FLUID_INGREDIENT, "of") if desc.starts_with("([Lnet/minecraft/world/level/material/Fluid;") => {
            let names: Vec<String> = jutil::elements(rt, a(0)).into_iter().map(|f| fluids::name_of(rt, f)).collect();
            fluid_ingredient(rt, &names)
        }
        (FLUID_INGREDIENT, "test") => V::I(fluid_ingredient_test(rt, a(0), a(1)) as i32),
        (FLUID_INGREDIENT, "getStacks") => {
            let stacks: Vec<V> = fluid_ingredient_fluids(rt, a(0)).into_iter().map(|f| fluids::fluid_stack(rt, f, BUCKET as i32)).collect();
            rt.alloc(FLUID_STACK_ARRAY, Data::Array(stacks))
        }
        (FLUID_INGREDIENT, "isEmpty" | "hasNoFluids") => V::I(entries(rt, a(0)).is_empty() as i32),
        (FLUID_INGREDIENT, "isSimple") => V::I(1),
        _ => return None,
    })
}

// ---- Recipe types, holders, inputs, the manager ----

/// A recipe type's id: what `RecipeType.simple` was given, else its registered name.
fn type_id(rt: &Rt, ty: V) -> String {
    let id = rt.get_field(ty, "$id");
    rt.str_of(id).map(str::to_owned).or_else(|| rt.registered_name(ty, "recipe_type").map(str::to_owned)).unwrap_or_default()
}

fn recipe_type(rt: &mut Rt, id: &str) -> V {
    let o = rt.new_object(RECIPE_TYPE);
    let s = rt.string(id);
    rt.put_field(o, "$id", s);
    o
}

fn holder(rt: &mut Rt, id: V, recipe: V) -> V {
    let h = rt.new_object(HOLDER);
    rt.put_field(h, "$id", id);
    rt.put_field(h, "$value", recipe);
    h
}

/// A recipe from its file: the serializer its `type` names reads it with its codec, as
/// `Recipe.CODEC` dispatches.
pub fn decode_recipe(rt: &mut Rt, j: &Json) -> Result<V, String> {
    let Json::Object(o) = j else { return Err(format!("Not a map: {j}")) };
    let Some(ty) = o.get("type").and_then(Json::as_str) else { return Err("No key type in MapLike".into()) };
    let ty = qualified(ty);
    let Some(&serializer) = rt.registered.get(&("recipe_serializer".to_owned(), ty.clone())) else {
        return Err(format!("Unknown registry key in recipe_serializer: {ty}"));
    };
    let codec = rt.call_functional(SERIALIZER_CODEC, serializer, &[]);
    if let Some(e) = rt.thrown.take() {
        return Err(format!("{ty}'s serializer threw {}", rt.class_of(e)));
    }
    if !codecs::is_codec(rt, codec) {
        return Err(format!("{ty}'s serializer has no codec this runtime reads"));
    }
    codecs::decode_map(rt, codec, o)
}

/// The recipes of type `id` the game has, each read once per runtime: `RecipeHolder`s, in the
/// game's order. A recipe the mod's codec refuses is left out, as vanilla's manager does, and
/// reported once.
pub fn holders(rt: &mut Rt, id: &str) -> Vec<V> {
    let key = format!("compat:recipes/{id}");
    if let Some(&list) = rt.interned.get(&key) {
        return jutil::elements(rt, list);
    }
    let files = rt.host().map(|h| h.recipes_of_type(id)).unwrap_or_default();
    if files.is_empty() {
        // Outside a world, or a type nothing uses: asked again next time.
        return Vec::new();
    }
    let (mut out, mut refused) = (Vec::new(), Vec::new());
    for (rid, text) in &files {
        let read = serde_json::from_str::<Json>(text).map_err(|e| e.to_string()).and_then(|j| decode_recipe(rt, &j));
        match read {
            Ok(recipe) => {
                let s = rt.string(rid);
                out.push(holder(rt, s, recipe));
            }
            Err(e) => refused.push(format!("{rid}: {e}")),
        }
    }
    if let Some(first) = refused.first() {
        eprintln!("horizon-compat: {} of {} {id} recipes not read; first: {first}", refused.len(), files.len());
    }
    let list = jutil::new_list(rt, out.clone());
    rt.interned.insert(key, list);
    out
}

/// The recipes of every type the mod registered.
fn own_holders(rt: &mut Rt) -> Vec<V> {
    let mut types: Vec<String> = rt.registered.keys().filter(|(r, _)| r == "recipe_type").map(|(_, n)| n.clone()).collect();
    types.sort();
    types.iter().flat_map(|t| holders(rt, t)).collect()
}

/// `recipe.matches(input, level)`, the mod's own; a recipe that throws doesn't match.
fn matches(rt: &mut Rt, holder: V, input: V, level: V) -> bool {
    let recipe = rt.get_field(holder, "$value");
    let r = rt.call_functional(MATCHES, recipe, &[input, level]);
    rt.thrown.take().is_none() && r.i() != 0
}

fn manager_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "getRecipeFor" => {
            let id = type_id(rt, a(1));
            let all = holders(rt, &id);
            // The last recipe found (a hint), tried first.
            let hint = if desc.contains("RecipeHolder;)") {
                Some(a(4)).filter(|h| h.r() != 0)
            } else if desc.contains("ResourceLocation;)") {
                let wanted = text(rt, a(4));
                all.iter().copied().find(|&h| text(rt, rt.get_field(h, "$id")) == wanted)
            } else {
                None
            };
            let order = hint.into_iter().chain(all.iter().copied().filter(|&h| Some(h) != hint));
            let mut found = None;
            for h in order.collect::<Vec<_>>() {
                if matches(rt, h, a(2), a(3)) {
                    found = Some(h);
                    break;
                }
            }
            optional(rt, found)
        }
        "getRecipesFor" => {
            let id = type_id(rt, a(1));
            let mut found = Vec::new();
            for h in holders(rt, &id) {
                if matches(rt, h, a(2), a(3)) {
                    found.push(h);
                }
            }
            jutil::new_list(rt, found)
        }
        "getAllRecipesFor" | "byType" => {
            let id = type_id(rt, a(1));
            let all = holders(rt, &id);
            jutil::new_list(rt, all)
        }
        "byKey" => {
            let wanted = text(rt, a(1));
            let all = own_holders(rt);
            let found = all.into_iter().find(|&h| text(rt, rt.get_field(h, "$id")) == wanted);
            optional(rt, found)
        }
        "getRecipes" | "getRecipeIds" => {
            let all = own_holders(rt);
            let values = if name == "getRecipeIds" { all.iter().map(|&h| rt.get_field(h, "$id")).collect() } else { all };
            jutil::new_list(rt, values)
        }
        _ => return None,
    })
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    owner.starts_with(INGREDIENT)
        || matches!(
            owner,
            "net/neoforged/neoforge/common/crafting/CompoundIngredient"
                | FLUID_INGREDIENT
                | SIZED_FLUID
                | MANAGER
                | BREWING
                | HOLDER
                | RECIPE_TYPE
                | SINGLE_INPUT
                | RECIPE_INPUT
                | COMPONENT_PATCH
        )
        || matches!(name, "getRecipeManager" | "potionBrewing")
        || crate::crafting::claims(owner, name, desc)
}

/// Calls on recipes and their pieces; None for what this module doesn't model.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let (owner, name, desc) = crate::parts(key)?;
    // NeoForge's `CompoundIngredient.of(children...)`: an ingredient of every child's entries.
    if owner == "net/neoforged/neoforge/common/crafting/CompoundIngredient" && name == "of" {
        let children = if desc.starts_with("([") || desc.starts_with("(Ljava/util/") { jutil::elements(rt, a(0)) } else { args.to_vec() };
        let mut out: Vec<String> = Vec::new();
        for c in children {
            for e in entries(rt, c) {
                if !out.contains(&e) {
                    out.push(e);
                }
            }
        }
        return Some(ingredient(rt, &out));
    }
    if owner.starts_with(INGREDIENT) {
        return ingredient_call(rt, owner, name, desc, args);
    }
    if owner == FLUID_INGREDIENT || owner == SIZED_FLUID {
        return fluid_ingredient_call(rt, owner, name, desc, args);
    }
    if let Some(v) = crate::crafting::call(rt, owner, name, desc, args) {
        return Some(v);
    }
    Some(match (owner, name) {
        (_, "getRecipeManager") if desc == "()Lnet/minecraft/world/item/crafting/RecipeManager;" => {
            rt.intern("compat:recipe-manager".into(), |rt| rt.new_object(MANAGER))
        }
        (MANAGER, n) => return manager_call(rt, n, desc, args),
        // Vanilla brewing (`level.potionBrewing()`): which items are potion containers. Its mixes
        // aren't modelled yet: no mix matches, and NeoForge's brewing recipe list is empty (Create's
        // mixer asks both on every recipe search; its potion mixing reads the mixes through an
        // accessor mixin, `create$getPotionMixes`, which gets the empty lists).
        (_, "potionBrewing") if desc == "()Lnet/minecraft/world/item/alchemy/PotionBrewing;" => {
            rt.intern("compat:potion-brewing".into(), |rt| rt.new_object(BREWING))
        }
        (BREWING, "isContainer") => {
            let item = if desc.contains("ItemStack") { rt.get_field(a(1), "$item") } else { a(1) };
            let id = crate::api_common::item_like_name(rt, item);
            V::I(matches!(id.as_str(), "minecraft:potion" | "minecraft:splash_potion" | "minecraft:lingering_potion") as i32)
        }
        (BREWING, "isIngredient" | "hasMix" | "hasPotionMix" | "hasContainerMix" | "isBrewablePotion" | "isPotionIngredient" | "isContainerIngredient") => V::I(0),
        (BREWING, n) if n == "getRecipes" || n.ends_with("PotionMixes") || n.ends_with("ContainerMixes") => jutil::new_list(rt, Vec::new()),
        (HOLDER, "<init>") => {
            rt.put_field(a(0), "$id", a(1));
            rt.put_field(a(0), "$value", a(2));
            V::Void
        }
        (HOLDER, "id") => rt.get_field(a(0), "$id"),
        (HOLDER, "value") => rt.get_field(a(0), "$value"),
        (HOLDER, "equals") => {
            let same = rt.class_of(a(1)) == HOLDER && text(rt, rt.get_field(a(0), "$id")) == text(rt, rt.get_field(a(1), "$id"));
            V::I(same as i32)
        }
        (HOLDER, "hashCode") => {
            let id = rt.get_field(a(0), "$id");
            V::I(rt.java_hash(id))
        }
        (HOLDER, "toString") => rt.get_field(a(0), "$id"),
        (RECIPE_TYPE, "simple") => {
            let id = text(rt, a(0));
            recipe_type(rt, &id)
        }
        (RECIPE_TYPE, "toString") => {
            let id = type_id(rt, a(0));
            rt.string(&id)
        }
        (SINGLE_INPUT, "<init>") => {
            rt.put_field(a(0), "$item", a(1));
            V::Void
        }
        (SINGLE_INPUT | RECIPE_INPUT, "item" | "getItem" | "size" | "isEmpty") if rt.class_of(a(0)) == SINGLE_INPUT => {
            let stack = rt.get_field(a(0), "$item");
            match name {
                "size" => V::I(1),
                "isEmpty" => V::I(api_common::stack_empty(rt, stack) as i32),
                _ if name == "getItem" && a(1).i() != 0 => {
                    let e = rt.new_object("java/lang/IllegalArgumentException");
                    rt.thrown = Some(e);
                    V::NULL
                }
                _ => stack,
            }
        }
        (COMPONENT_PATCH, "isEmpty") => {
            let json = rt.get_field(a(0), "$json");
            V::I((json.r() == 0 || text(rt, json) == "{}") as i32)
        }
        _ => return None,
    })
}

/// Static fields: vanilla's recipe types, `Ingredient.EMPTY`, `DataComponentPatch.EMPTY`.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    if let Some(v) = crate::crafting::get_static(rt, owner, field) {
        return Some(v);
    }
    Some(match (owner, field) {
        (RECIPE_TYPE, f) => {
            let id = format!("minecraft:{}", f.to_ascii_lowercase());
            rt.intern(format!("compat:recipe-type/{id}"), move |rt| recipe_type(rt, &id))
        }
        (INGREDIENT, "EMPTY") => empty_ingredient(rt),
        (COMPONENT_PATCH, "EMPTY") => rt.intern("compat:component-patch/empty".into(), |rt| rt.new_object(COMPONENT_PATCH)),
        _ => return None,
    })
}

// ---- Minecraft's codecs, read natively ----

fn int_field(o: &Map<String, Json>, key: &str, default: i64) -> Result<i64, String> {
    match o.get(key) {
        None => Ok(default),
        Some(j) => j.as_i64().ok_or_else(|| format!("{key}: Not a number: {j}")),
    }
}

/// A component patch's value: its JSON, kept (components aren't applied yet).
fn component_patch(rt: &mut Rt, j: Option<&Json>) -> V {
    match j {
        None => get_static(rt, COMPONENT_PATCH, "EMPTY").unwrap_or(V::NULL),
        Some(j) => {
            let o = rt.new_object(COMPONENT_PATCH);
            let s = rt.string(&j.to_string());
            rt.put_field(o, "$json", s);
            o
        }
    }
}

/// JSON as NBT, as `JsonOps` converts to `NbtOps` (whole numbers as ints, the rest as doubles).
pub(crate) fn json_nbt(j: &Json) -> Option<crate::nbt::Nbt> {
    use crate::nbt::Nbt;
    Some(match j {
        Json::Null => return None,
        Json::Bool(b) => Nbt::Byte(*b as i8),
        Json::Number(n) => match n.as_i64() {
            Some(i) if i32::try_from(i).is_ok() => Nbt::Int(i as i32),
            Some(i) => Nbt::Long(i),
            None => Nbt::Double(n.as_f64().unwrap_or(0.0)),
        },
        Json::String(s) => Nbt::String(s.clone()),
        Json::Array(a) => Nbt::List(a.iter().filter_map(json_nbt).collect()),
        Json::Object(o) => Nbt::Compound(o.iter().filter_map(|(k, v)| Some((k.clone(), json_nbt(v)?))).collect()),
    })
}

pub(crate) fn nbt_json(n: &crate::nbt::Nbt) -> Json {
    use crate::nbt::Nbt;
    match n {
        Nbt::Byte(b) => Json::from(*b),
        Nbt::Short(s) => Json::from(*s),
        Nbt::Int(i) => Json::from(*i),
        Nbt::Long(l) => Json::from(*l),
        Nbt::Float(f) => Json::from(*f as f64),
        Nbt::Double(d) => Json::from(*d),
        Nbt::String(s) => Json::from(s.clone()),
        Nbt::Compound(c) => Json::Object(c.iter().map(|(k, v)| (k.clone(), nbt_json(v))).collect()),
        Nbt::List(l) => Json::Array(l.iter().map(nbt_json).collect()),
        Nbt::ByteArray(a) => Json::Array(a.iter().map(|b| Json::from(*b)).collect()),
        Nbt::IntArray(a) => Json::Array(a.iter().map(|i| Json::from(*i)).collect()),
        Nbt::LongArray(a) => Json::Array(a.iter().map(|l| Json::from(*l)).collect()),
    }
}

/// A value a mod hands a codec to read (`codec.parse(NbtOps.INSTANCE, tag)`), as JSON: any tag (a
/// compound, a list, a number...), a string; anything else reads as null.
pub fn to_json(rt: &mut Rt, v: V) -> Json {
    if rt.class_of(v) == "net/minecraft/nbt/CompoundTag" {
        let map = crate::nbt::from_runtime(rt, v);
        return Json::Object(map.iter().map(|(k, n)| (k.clone(), nbt_json(n))).collect());
    }
    if let Some(s) = rt.str_of(v) {
        return Json::String(s.to_owned());
    }
    if rt.class_of(v).starts_with("net/minecraft/nbt/") {
        if let Some(n) = crate::nbt::element_from_runtime(rt, v) {
            return nbt_json(&n);
        }
    }
    Json::Null
}

/// Reads `j` with Minecraft's or NeoForge's codec `name` (see [`codecs::native`]).
pub fn decode_native(rt: &mut Rt, name: &str, j: &Json) -> Result<V, String> {
    let object = || j.as_object().ok_or_else(|| format!("Not a map: {j}"));
    match name {
        "resource_location" => {
            let s = j.as_str().ok_or_else(|| format!("Not a string: {j}"))?;
            if !valid_location(s) {
                return Err(format!("Not a valid resource location: {s}"));
            }
            Ok(rt.string(&qualified(s)))
        }
        "block_pos" => {
            let xyz: Vec<i64> = j.as_array().map(|a| a.iter().filter_map(Json::as_i64).collect()).unwrap_or_default();
            match xyz[..] {
                [x, y, z] => Ok(crate::api::block_pos(rt, x, y, z)),
                _ => Err(format!("Not a block position: {j}")),
            }
        }
        "vec3i" => {
            let xyz: Vec<i64> = j.as_array().map(|a| a.iter().filter_map(Json::as_i64).collect()).unwrap_or_default();
            match xyz[..] {
                [x, y, z] => {
                    let v = rt.new_object("net/minecraft/core/Vec3i");
                    if let Some(o) = rt.heap.get_mut(v.r() as usize) {
                        o.data = crate::Data::Native("BlockPos", [x, y, z, 0]);
                    }
                    Ok(v)
                }
                _ => Err(format!("Input is not a list of 3 ints: {j}")),
            }
        }
        "vec3" => {
            let xyz: Vec<f64> = j.as_array().map(|a| a.iter().filter_map(Json::as_f64).collect()).unwrap_or_default();
            match xyz[..] {
                [x, y, z] => Ok(api_common::vec3(rt, x, y, z)),
                _ => Err(format!("Input is not a list of 3 elements: {j}")),
            }
        }
        "uuid" => {
            let ints: Vec<i64> = j.as_array().map(|a| a.iter().filter_map(Json::as_i64).collect()).unwrap_or_default();
            match ints[..] {
                [a, b, c, d] => {
                    let msb = (a << 32) | (b as i32 as u32 as i64);
                    let lsb = (c << 32) | (d as i32 as u32 as i64);
                    Ok(crate::tags::uuid(rt, msb, lsb))
                }
                _ => Err(format!("Input is not a list of 4 elements: {j}")),
            }
        }
        "uuid_string" => {
            let s = j.as_str().ok_or_else(|| format!("Not a string: {j}"))?;
            let (msb, lsb) = crate::tags::uuid_parse(s).ok_or_else(|| format!("Invalid UUID {s}"))?;
            Ok(crate::tags::uuid(rt, msb, lsb))
        }
        "direction" => {
            let s = j.as_str().ok_or_else(|| format!("Not a string: {j}"))?;
            let i = api_common::DIRECTION_NAMES.iter().position(|d| *d == s).ok_or_else(|| format!("Unknown element name: {s}"))?;
            Ok(api_common::direction(rt, i as i64))
        }
        "axis" => {
            let s = j.as_str().ok_or_else(|| format!("Not a string: {j}"))?;
            let i = api_common::AXIS_NAMES.iter().position(|d| *d == s).ok_or_else(|| format!("Unknown element name: {s}"))?;
            Ok(api_common::axis(rt, i as i64))
        }
        // By serialized name: `Rotation.CLOCKWISE_180` is `180`.
        "mirror" | "rotation" => {
            let s = j.as_str().ok_or_else(|| format!("Not a string: {j}"))?;
            let (class, names, serialized): (&str, &[&str], &[&str]) = if name == "mirror" {
                (crate::rotation::MIRROR, &crate::rotation::MIRRORS, &crate::rotation::MIRRORS)
            } else {
                (crate::rotation::ROTATION, &crate::rotation::ROTATIONS, &crate::rotation::ROTATION_NAMES)
            };
            let i = serialized.iter().position(|n| *n == s).ok_or_else(|| format!("Unknown element name: {s}"))?;
            crate::blocks::vanilla_enum_constant(rt, class, names[i]).ok_or_else(|| format!("Unknown element name: {s}"))
        }
        "unit" => Ok(V::NULL),
        "crafting_book_category" => crate::crafting::decode_category(rt, j),
        "item_stack" | "optional_item_stack" => {
            if name == "optional_item_stack" && (j.is_null() || j.as_object().is_some_and(Map::is_empty)) {
                return Ok(api_common::get_static(rt, "net/minecraft/world/item/ItemStack", "EMPTY").unwrap_or(V::NULL));
            }
            let (id, count) = match j {
                Json::String(s) => (s.clone(), 1),
                Json::Object(o) => match o.get("id").and_then(Json::as_str) {
                    Some(id) => (id.to_owned(), int_field(o, "count", 1)?),
                    None => return Err("No key id in MapLike".into()),
                },
                _ => return Err(format!("Not an item stack: {j}")),
            };
            let id = qualified(&id);
            if id == "minecraft:air" || !item_known(rt, &id) {
                return Err(format!("Unknown registry key in item: {id}"));
            }
            if !(1..=99).contains(&count) {
                return Err(format!("Value {count} outside of range [1:99]"));
            }
            let s = api_common::stack(rt, &id, count as i32);
            if let Some(Json::Object(patch)) = j.get("components") {
                crate::components::load_saved(rt, s, patch);
            }
            Ok(s)
        }
        // `[{slot, item}]` (`ItemContainerContents.CODEC`): slots 0 to 255, at most 256 of them.
        "container_contents" => {
            let Json::Array(entries) = j else { return Err(format!("Not a list: {j}")) };
            if entries.len() > 256 {
                return Err(format!("List too long: {} > 256", entries.len()));
            }
            let mut slots = Vec::with_capacity(entries.len());
            for e in entries {
                let Some(o) = e.as_object() else { return Err(format!("Not a map: {e}")) };
                if !o.contains_key("slot") {
                    return Err("No key slot in MapLike".into());
                }
                let slot = int_field(o, "slot", -1)?;
                if !(0..=255).contains(&slot) {
                    return Err(format!("Value {slot} outside of range [0:255]"));
                }
                let item = decode_native(rt, "item_stack", o.get("item").ok_or("No key item in MapLike")?)?;
                slots.push((slot as usize, item));
            }
            Ok(crate::handlers::contents_from_slots(rt, slots))
        }
        "ingredient" | "ingredient_nonempty" => {
            let mut out = Vec::new();
            read_ingredient(rt, j, &mut out)?;
            if out.is_empty() && name == "ingredient_nonempty" {
                return Err("Cannot serialize empty ingredient using the non-empty codec".into());
            }
            Ok(ingredient(rt, &out))
        }
        "fluid_stack" | "optional_fluid_stack" => {
            let o = object()?;
            if name == "optional_fluid_stack" && o.is_empty() {
                return Ok(fluids::fluid_stack_empty(rt));
            }
            let Some(id) = o.get("id").and_then(Json::as_str) else { return Err("No key id in MapLike".into()) };
            let id = qualified(id);
            let f = fluids::fluid(rt, &id);
            if f.r() == 0 || id == "minecraft:empty" {
                return Err(format!("Unknown registry key in fluid: {id}"));
            }
            let Some(amount) = o.get("amount") else { return Err("No key amount in MapLike".into()) };
            let amount = amount.as_i64().filter(|a| *a > 0).ok_or_else(|| format!("amount: Value must be positive: {amount}"))?;
            Ok(fluids::fluid_stack(rt, f, amount as i32))
        }
        "fluid_ingredient" | "fluid_ingredient_map" => {
            let (mut out, mut needs) = (Vec::new(), None);
            match j {
                Json::Array(items) => {
                    for i in items {
                        let Json::Object(i) = i else { return Err(format!("Not a map: {i}")) };
                        read_fluid_ingredient(rt, i, &mut out, &mut needs)?;
                    }
                }
                _ => read_fluid_ingredient(rt, object()?, &mut out, &mut needs)?,
            }
            let i = fluid_ingredient(rt, &out);
            requires(rt, i, needs);
            Ok(i)
        }
        "sized_fluid_ingredient" => {
            let o = object()?;
            // `NESTED_CODEC` holds the ingredient under `ingredient`; `FLAT_CODEC` beside `amount`.
            let (mut out, mut needs) = (Vec::new(), None);
            match o.get("ingredient") {
                Some(Json::Object(inner)) => read_fluid_ingredient(rt, inner, &mut out, &mut needs)?,
                Some(Json::Array(items)) => {
                    for i in items {
                        let Json::Object(i) = i else { return Err(format!("Not a map: {i}")) };
                        read_fluid_ingredient(rt, i, &mut out, &mut needs)?;
                    }
                }
                _ => read_fluid_ingredient(rt, o, &mut out, &mut needs)?,
            }
            let amount = int_field(o, "amount", BUCKET)?;
            if amount <= 0 {
                return Err(format!("amount: Value must be positive: {amount}"));
            }
            let i = fluid_ingredient(rt, &out);
            requires(rt, i, needs);
            Ok(sized_fluid(rt, i, amount as i32))
        }
        "component_patch" => {
            object()?;
            Ok(component_patch(rt, Some(j)))
        }
        "compound_tag" => match json_nbt(j) {
            Some(crate::nbt::Nbt::Compound(c)) => Ok(crate::nbt::to_runtime(rt, &c)),
            _ => Err(format!("Not a compound tag: {j}")),
        },
        "item_enchantments" => crate::enchantments::decode(rt, j),
        "dye_color" => {
            const DYES: [&str; 16] = [
                "white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan", "purple", "blue", "brown", "green", "red", "black",
            ];
            let s = j.as_str().ok_or_else(|| format!("Not a string: {j}"))?;
            let i = DYES.iter().position(|d| *d == s).ok_or_else(|| format!("Unknown element name: {s}"))?;
            let key = rt.static_key(&format!("net/minecraft/world/item/DyeColor.{}", s.to_ascii_uppercase()));
            let v = crate::api::get_static(rt, key);
            Ok(if v.r() != 0 { v } else { crate::api::api_enum(rt, "net/minecraft/world/item/DyeColor", DYES[i], i as i32) })
        }
        "block_state" => {
            let o = object()?;
            let Some(block) = o.get("Name").and_then(Json::as_str) else { return Err("No key Name in MapLike".into()) };
            let props: Vec<(String, String)> = o
                .get("Properties")
                .and_then(Json::as_object)
                .map(|p| p.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned()))).collect())
                .unwrap_or_default();
            let block = qualified(block);
            let text = if props.is_empty() {
                block
            } else {
                format!("{block}[{}]", props.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(","))
            };
            Ok(api_common::state(rt, &text))
        }
        "recipe" => decode_recipe(rt, j),
        "component" => Ok(crate::api_common::component_from_json(rt, j)),
        n if n.starts_with("tag_key/") || n.starts_with("hashed_tag_key/") => {
            let (kind, registry) = n.split_once('/').unwrap_or((n, ""));
            let s = j.as_str().ok_or_else(|| format!("Not a string: {j}"))?;
            let s = match kind {
                "hashed_tag_key" => s.strip_prefix('#').ok_or_else(|| format!("Not a tag id: {s}"))?,
                _ => s,
            };
            if !valid_location(s) {
                return Err(format!("Not a valid resource location: {s}"));
            }
            Ok(crate::fluids::tag_key(rt, registry, &qualified(s)))
        }
        n if n.starts_with("resource_key/") => {
            let registry = &n["resource_key/".len()..];
            let s = j.as_str().ok_or_else(|| format!("Not a string: {j}"))?;
            if !valid_location(s) {
                return Err(format!("Not a valid resource location: {s}"));
            }
            Ok(rt.string(&format!("{registry}#{}", qualified(s))))
        }
        n if n.starts_with("vanilla_enum/") => {
            let class = &n["vanilla_enum/".len()..];
            let s = j.as_str().ok_or_else(|| format!("Not a string: {j}"))?;
            crate::blocks::vanilla_enum_constant(rt, class, s).ok_or_else(|| format!("Unknown element name: {s}"))
        }
        other => Err(format!("{other} is not read yet")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    #[test]
    fn a_compound_ingredient_takes_every_childs_items_and_potions_are_brewing_containers() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| {
            let nugget = ingredient(rt, &["minecraft:iron_nugget".into()]);
            let both = ingredient(rt, &["minecraft:andesite".into(), "minecraft:iron_nugget".into()]);
            let children = rt.alloc("[Lnet/minecraft/world/item/crafting/Ingredient;", Data::Array(vec![nugget, both]));
            let compound = crate::api::call(rt, "net/neoforged/neoforge/common/crafting/CompoundIngredient.of.([Lnet/minecraft/world/item/crafting/Ingredient;)Lnet/minecraft/world/item/crafting/Ingredient;", &[children]);
            assert_eq!(entries(rt, compound), ["minecraft:iron_nugget", "minecraft:andesite"]);
            let andesite = api_common::stack(rt, "minecraft:andesite", 1);
            assert!(ingredient_test(rt, compound, andesite));
            // Vanilla's containers: the three potions, not the empty bottle.
            let level = rt.new_object("net/minecraft/server/level/ServerLevel");
            let brewing = crate::api::call(rt, "net/minecraft/world/level/Level.potionBrewing.()Lnet/minecraft/world/item/alchemy/PotionBrewing;", &[level]);
            for (item, container) in [("minecraft:splash_potion", 1), ("minecraft:glass_bottle", 0)] {
                let s = api_common::stack(rt, item, 1);
                let r = crate::api::call(rt, "net/minecraft/world/item/alchemy/PotionBrewing.isContainer.(Lnet/minecraft/world/item/ItemStack;)Z", &[brewing, s]);
                assert_eq!(r, V::I(container), "{item}");
            }
        });
    }

    #[test]
    fn an_ingredient_reads_items_tags_and_lists_and_tests_stacks_against_the_game_tags() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        host.item_tags.insert("c:ingots/iron".into(), vec!["minecraft:iron_ingot".into()]);
        rt.with_host(&mut host, |rt| {
            let tag = decode_native(rt, "ingredient", &serde_json::json!({"tag": "c:ingots/iron"})).unwrap();
            let iron = api_common::stack(rt, "minecraft:iron_ingot", 1);
            let gold = api_common::stack(rt, "minecraft:gold_ingot", 1);
            assert!(ingredient_test(rt, tag, iron));
            assert!(!ingredient_test(rt, tag, gold));
            let list = decode_native(rt, "ingredient", &serde_json::json!([{"item": "gold_ingot"}, {"tag": "c:ingots/iron"}])).unwrap();
            assert_eq!(ingredient_items(rt, list), ["minecraft:gold_ingot", "minecraft:iron_ingot"]);
            assert!(decode_native(rt, "ingredient", &serde_json::json!({"fluid": "minecraft:water"})).is_err());
            assert!(decode_native(rt, "ingredient_nonempty", &serde_json::json!([])).is_err());
            // An empty ingredient takes an empty stack only.
            let empty = get_static(rt, INGREDIENT, "EMPTY").unwrap();
            let air = api_common::get_static(rt, "net/minecraft/world/item/ItemStack", "EMPTY").unwrap();
            assert!(ingredient_test(rt, empty, air));
            assert!(!ingredient_test(rt, empty, iron));
        });
    }

    #[test]
    fn a_components_fluid_ingredient_takes_only_stacks_that_carry_the_components() {
        let mut rt = Rt::new();
        crate::install();
        let j = serde_json::json!({"type": "neoforge:components", "amount": 25, "fluids": "minecraft:water", "components": {"minecraft:potion_contents": {"potion": "minecraft:night_vision"}}});
        let sized = decode_native(&mut rt, "sized_fluid_ingredient", &j).unwrap();
        let ing = rt.get_field(sized, "$ingredient");
        let water = fluids::fluid(&mut rt, "minecraft:water");
        let plain = fluids::fluid_stack(&mut rt, water, 1000);
        assert!(!fluid_ingredient_test(&mut rt, ing, plain));
        let carrying = fluids::fluid_stack(&mut rt, water, 1000);
        let c = rt.string(r#"{"minecraft:potion_contents":{"potion":"minecraft:night_vision"}}"#);
        rt.put_field(carrying, "$components", c);
        assert!(fluid_ingredient_test(&mut rt, ing, carrying));
    }

    #[test]
    fn a_fluid_stack_needs_an_amount_so_an_item_result_is_not_one() {
        let mut rt = Rt::new();
        crate::install();
        assert!(decode_native(&mut rt, "fluid_stack", &serde_json::json!({"id": "minecraft:water", "amount": 250})).is_ok());
        assert!(decode_native(&mut rt, "fluid_stack", &serde_json::json!({"id": "minecraft:iron_ingot"})).is_err());
        let sized = decode_native(&mut rt, "sized_fluid_ingredient", &serde_json::json!({"type": "neoforge:single", "fluid": "minecraft:lava", "amount": 250})).unwrap();
        assert_eq!(rt.get_field(sized, "$amount"), V::I(250));
        assert!(decode_native(&mut rt, "sized_fluid_ingredient", &serde_json::json!({"item": "minecraft:stick"})).is_err());
    }
}
