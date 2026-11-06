//! Food a mod's items give. In 1.21.1 a mod builds `FoodProperties` with its builder (`nutrition`,
//! `saturationModifier`, `alwaysEdible`, `fast`, `effect`, `usingConvertsTo`) and hands it to
//! `Item.Properties.food`; `build()` makes the record vanilla's way (saturation is nutrition ×
//! modifier × 2). The effects are `MobEffectInstance`s (an effect's holder, duration, amplifier and
//! flags) of a mod's own effects or vanilla's `MobEffects` (each field's id from `vanilla_effects.rs`).
//!
//! 26.3 splits that one component in three, as its own items have them (items.json): `food`
//! (nutrition, saturation, whether it can always be eaten), `consumable` (seconds to eat, animation,
//! sound, particles, the effects it applies) and `use_remainder` (what eating leaves).
//! [`components`] writes them for a mod's item. Its class may change how it's eaten by overriding
//! `Item`'s `getUseDuration`, `getUseAnimation` (a drink), `getEatingSound` or `getDrinkingSound`:
//! those are asked once, with a stack of the item, as they answer the same for any.
//!
//! Potions: `Potions.X` are holders of vanilla's potions, `PotionContents` a potion, a color and
//! effects of its own (`createItemStack`, `is`, `withPotion` here). A stack's contents are vanilla's
//! `minecraft:potion_contents`, which the game's stacks carry ([`encode_potion_contents`],
//! [`decode_potion_contents`]).
use crate::nbt::Nbt;
use crate::{api, api_common, jutil, Rt, V};
use std::collections::BTreeMap;

const BUILDER: &str = "net/minecraft/world/food/FoodProperties$Builder";
const FOOD: &str = "net/minecraft/world/food/FoodProperties";
const POSSIBLE: &str = "net/minecraft/world/food/FoodProperties$PossibleEffect";
const INSTANCE: &str = "net/minecraft/world/effect/MobEffectInstance";
const EFFECT: &str = "net/minecraft/world/effect/MobEffect";
const EFFECTS: &str = "net/minecraft/world/effect/MobEffects";
const USE_ANIM: &str = "net/minecraft/world/item/UseAnim";
const POTIONS: &str = "net/minecraft/world/item/alchemy/Potions";
const POTION: &str = "net/minecraft/world/item/alchemy/Potion";
const POTION_CONTENTS: &str = "net/minecraft/world/item/alchemy/PotionContents";
/// `UseAnim`'s constants in 1.21.1's order (NeoForge adds `CUSTOM`).
const USE_ANIMS: [&str; 11] = ["NONE", "EAT", "DRINK", "BLOCK", "BOW", "SPEAR", "CROSSBOW", "SPYGLASS", "TOOT_HORN", "BRUSH", "CUSTOM"];
const SUPPLY: &str = "java/util/function/Supplier.get.()Ljava/lang/Object;";
const LIST_ADD: &str = "java/util/List.add.(Ljava/lang/Object;)Z";
/// `FoodProperties.DEFAULT_EAT_SECONDS`, and what `fast()` sets.
const EAT_SECONDS: f32 = 1.6;
const FAST_SECONDS: f32 = 0.8;
use crate::item_use::{USE_ANIMATION, USE_ANIMATION_KEY, USE_DURATION, USE_DURATION_KEY};
const EATING_SOUND: &str = "getEatingSound.()Lnet/minecraft/sounds/SoundEvent;";
const EATING_SOUND_KEY: &str = "net/minecraft/world/item/Item.getEatingSound.()Lnet/minecraft/sounds/SoundEvent;";
const DRINKING_SOUND: &str = "getDrinkingSound.()Lnet/minecraft/sounds/SoundEvent;";
const DRINKING_SOUND_KEY: &str = "net/minecraft/world/item/Item.getDrinkingSound.()Lnet/minecraft/sounds/SoundEvent;";

/// 1.21.1's `Foods`, as its static initialiser builds them: (field, nutrition, saturation modifier,
/// always edible, fast, what eating leaves, effects as (`MobEffects` field, duration, amplifier,
/// probability)).
#[allow(clippy::type_complexity)]
const FOODS: [(&str, i32, f32, bool, bool, Option<&str>, &[(&str, i32, i32, f32)]); 41] = [
    ("APPLE", 4, 0.3, false, false, None, &[]),
    ("BAKED_POTATO", 5, 0.6, false, false, None, &[]),
    ("BEEF", 3, 0.3, false, false, None, &[]),
    ("BEETROOT", 1, 0.6, false, false, None, &[]),
    ("BEETROOT_SOUP", 6, 0.6, false, false, Some("minecraft:bowl"), &[]),
    ("BREAD", 5, 0.6, false, false, None, &[]),
    ("CARROT", 3, 0.6, false, false, None, &[]),
    ("CHICKEN", 2, 0.3, false, false, None, &[("HUNGER", 600, 0, 0.3)]),
    ("CHORUS_FRUIT", 4, 0.3, true, false, None, &[]),
    ("COD", 2, 0.1, false, false, None, &[]),
    ("COOKED_BEEF", 8, 0.8, false, false, None, &[]),
    ("COOKED_CHICKEN", 6, 0.6, false, false, None, &[]),
    ("COOKED_COD", 5, 0.6, false, false, None, &[]),
    ("COOKED_MUTTON", 6, 0.8, false, false, None, &[]),
    ("COOKED_PORKCHOP", 8, 0.8, false, false, None, &[]),
    ("COOKED_RABBIT", 5, 0.6, false, false, None, &[]),
    ("COOKED_SALMON", 6, 0.8, false, false, None, &[]),
    ("COOKIE", 2, 0.1, false, false, None, &[]),
    ("DRIED_KELP", 1, 0.3, false, true, None, &[]),
    ("ENCHANTED_GOLDEN_APPLE", 4, 1.2, true, false, None, &[("REGENERATION", 400, 1, 1.0), ("DAMAGE_RESISTANCE", 6000, 0, 1.0), ("FIRE_RESISTANCE", 6000, 0, 1.0), ("ABSORPTION", 2400, 3, 1.0)]),
    ("GOLDEN_APPLE", 4, 1.2, true, false, None, &[("REGENERATION", 100, 1, 1.0), ("ABSORPTION", 2400, 0, 1.0)]),
    ("GOLDEN_CARROT", 6, 1.2, false, false, None, &[]),
    ("HONEY_BOTTLE", 6, 0.1, false, false, None, &[]),
    ("MELON_SLICE", 2, 0.3, false, false, None, &[]),
    ("MUSHROOM_STEW", 6, 0.6, false, false, Some("minecraft:bowl"), &[]),
    ("MUTTON", 2, 0.3, false, false, None, &[]),
    ("POISONOUS_POTATO", 2, 0.3, false, false, None, &[("POISON", 100, 0, 0.6)]),
    ("PORKCHOP", 3, 0.3, false, false, None, &[]),
    ("POTATO", 1, 0.3, false, false, None, &[]),
    ("PUFFERFISH", 1, 0.1, false, false, None, &[("POISON", 1200, 1, 1.0), ("HUNGER", 300, 2, 1.0), ("CONFUSION", 300, 0, 1.0)]),
    ("PUMPKIN_PIE", 8, 0.3, false, false, None, &[]),
    ("RABBIT", 3, 0.3, false, false, None, &[]),
    ("RABBIT_STEW", 10, 0.6, false, false, Some("minecraft:bowl"), &[]),
    ("ROTTEN_FLESH", 4, 0.1, false, false, None, &[("HUNGER", 600, 0, 0.8)]),
    ("SALMON", 2, 0.1, false, false, None, &[]),
    ("SPIDER_EYE", 2, 0.8, false, false, None, &[("POISON", 100, 0, 1.0)]),
    ("SUSPICIOUS_STEW", 6, 0.6, true, false, Some("minecraft:bowl"), &[]),
    ("SWEET_BERRIES", 2, 0.1, false, false, None, &[]),
    ("GLOW_BERRIES", 2, 0.1, false, false, None, &[]),
    ("TROPICAL_FISH", 1, 0.1, false, false, None, &[]),
    ("OMINOUS_BOTTLE", 1, 0.1, false, false, None, &[]),
];

/// `Foods.FIELD`, built by the builder as vanilla's is: one object per field.
fn vanilla_food(rt: &mut Rt, field: &str) -> Option<V> {
    let &(name, nutrition, modifier, always, fast, converts, effects) = FOODS.iter().find(|f| f.0 == field)?;
    Some(rt.intern(format!("food/{name}"), |rt| {
        let b = rt.new_object(BUILDER);
        call(rt, NEW_BUILDER, &[b]);
        call(rt, NUTRITION, &[b, V::I(nutrition)]);
        call(rt, MODIFIER, &[b, V::F(modifier)]);
        if always {
            call(rt, ALWAYS, &[b]);
        }
        if fast {
            call(rt, FAST, &[b]);
        }
        if let Some(item) = converts {
            let item = api_common::item(rt, item);
            call(rt, CONVERTS, &[b, item]);
        }
        for &(effect, duration, amplifier, probability) in effects {
            let holder = get_static(rt, EFFECTS, effect).unwrap_or(V::NULL);
            let e = rt.new_object(INSTANCE);
            call(rt, INSTANCE_3, &[e, holder, V::I(duration), V::I(amplifier)]);
            call(rt, EFFECT_OF, &[b, e, V::F(probability)]);
        }
        call(rt, BUILD, &[b]).unwrap_or(V::NULL)
    }))
}

const NEW_BUILDER: &str = "net/minecraft/world/food/FoodProperties$Builder.<init>.()V";
const NUTRITION: &str = "net/minecraft/world/food/FoodProperties$Builder.nutrition.(I)Lnet/minecraft/world/food/FoodProperties$Builder;";
const MODIFIER: &str = "net/minecraft/world/food/FoodProperties$Builder.saturationModifier.(F)Lnet/minecraft/world/food/FoodProperties$Builder;";
const ALWAYS: &str = "net/minecraft/world/food/FoodProperties$Builder.alwaysEdible.()Lnet/minecraft/world/food/FoodProperties$Builder;";
const FAST: &str = "net/minecraft/world/food/FoodProperties$Builder.fast.()Lnet/minecraft/world/food/FoodProperties$Builder;";
const EFFECT_OF: &str = "net/minecraft/world/food/FoodProperties$Builder.effect.(Lnet/minecraft/world/effect/MobEffectInstance;F)Lnet/minecraft/world/food/FoodProperties$Builder;";
const CONVERTS: &str = "net/minecraft/world/food/FoodProperties$Builder.usingConvertsTo.(Lnet/minecraft/world/level/ItemLike;)Lnet/minecraft/world/food/FoodProperties$Builder;";
const BUILD: &str = "net/minecraft/world/food/FoodProperties$Builder.build.()Lnet/minecraft/world/food/FoodProperties;";
const INSTANCE_3: &str = "net/minecraft/world/effect/MobEffectInstance.<init>.(Lnet/minecraft/core/Holder;II)V";

/// `MobEffects.FIELD`: a holder of vanilla's effect with the id that field registers. One object per
/// field: a mod may compare them. `UseAnim`'s constants, and vanilla's foods (`Foods.GOLDEN_APPLE`).
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    if owner == "net/minecraft/world/food/Foods" {
        return vanilla_food(rt, field);
    }
    if owner == USE_ANIM {
        let i = USE_ANIMS.iter().position(|n| *n == field)?;
        return Some(api::api_enum(rt, USE_ANIM, USE_ANIMS[i], i as i32));
    }
    // `Potions.WATER`: each field registers the potion of its own name (all 46 of 1.21.1's).
    if owner == POTIONS && !field.is_empty() && field.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_') {
        return Some(potion_holder(rt, &format!("minecraft:{}", field.to_ascii_lowercase())));
    }
    // `PotionContents.EMPTY`: no potion, no color, no effects.
    if owner == POTION_CONTENTS && field == "EMPTY" {
        return Some(rt.intern("compat:potion-contents/empty".into(), |rt| {
            let (none, effects) = (api_common::optional(rt, V::NULL), jutil::new_list(rt, Vec::new()));
            crate::records::make(rt, POTION_CONTENTS, &[none, none, effects])
        }));
    }
    if owner != EFFECTS {
        return None;
    }
    let i = crate::vanilla_effects::MOB_EFFECTS.binary_search_by(|(f, _)| (*f).cmp(field)).ok()?;
    Some(effect_holder(rt, &format!("minecraft:{}", crate::vanilla_effects::MOB_EFFECTS[i].1)))
}

/// The holder of the effect `id` (`minecraft:speed`, a mod's `<namespace>:<path>`): vanilla's is
/// the one its `MobEffects` field gives, a mod's its registry's (a `DeferredHolder` compares with it
/// by id).
pub(crate) fn effect_holder(rt: &mut Rt, id: &str) -> V {
    let id = if id.contains(':') { id.to_owned() } else { format!("minecraft:{id}") };
    if id.starts_with("minecraft:") {
        return rt.intern(format!("effect-holder/{id}"), |rt| {
            let effect = rt.new_object(EFFECT);
            let s = rt.string(&id);
            rt.put_field(effect, "$id", s);
            crate::neo::registry_holder(rt, "mob_effect", &id, effect)
        });
    }
    let value = crate::neo::registry_get(rt, "mob_effect", &id);
    crate::neo::registry_holder(rt, "mob_effect", &id, value)
}

/// Whether `id` names one of vanilla's 39 effects (`minecraft:poison`).
pub(crate) fn is_vanilla_effect(id: &str) -> bool {
    id.strip_prefix("minecraft:").is_some_and(|path| crate::vanilla_effects::MOB_EFFECTS.iter().any(|(_, p)| *p == path))
}

/// Vanilla's potion `id` (`minecraft:water`) as its registry's holder, one per id; the potion
/// knows its id (`$id`).
pub(crate) fn potion_holder(rt: &mut Rt, id: &str) -> V {
    let name = id.to_owned();
    rt.intern(format!("potion-holder/{id}"), move |rt| {
        let potion = rt.new_object(POTION);
        let s = rt.string(&name);
        rt.put_field(potion, "$id", s);
        crate::neo::registry_holder(rt, "potion", &name, potion)
    })
}

/// `minecraft:potion_contents`, the component a stack's potion contents are.
pub(crate) const POTION_CONTENTS_ID: &str = "minecraft:potion_contents";

/// A potion's id from its holder: vanilla's (a `Potions` field's), or a mod's registered one.
fn potion_id(rt: &Rt, h: V) -> Option<String> {
    if let Some(id) = rt.str_of(rt.get_field(h, "$id")).filter(|s| !s.is_empty()) {
        return Some(id.to_owned());
    }
    rt.registered_name(rt.get_field(h, "$value"), "potion").or_else(|| rt.registered_name(h, "potion")).map(str::to_owned)
}

/// Potion contents, as the record holds them: the potion's holder (null for none), a color of its
/// own (null for none) and effects of its own.
fn potion_contents(rt: &mut Rt, potion: V, color: V, effects: Vec<V>) -> V {
    let (potion, color, effects) = (api_common::optional(rt, potion), api_common::optional(rt, color), jutil::new_list(rt, effects));
    crate::records::make(rt, POTION_CONTENTS, &[potion, color, effects])
}

/// Potion contents `c` as vanilla's codec writes them: `{potion, custom_color, custom_effects}`, each
/// when there, an effect as `{id, amplifier, duration, ambient, show_particles, show_icon}` with
/// its defaults left out (`show_icon` is always written); None for contents without any of them.
pub(crate) fn encode_potion_contents(rt: &Rt, c: V) -> Option<Nbt> {
    let mut m = BTreeMap::new();
    let potion = rt.get_field(rt.get_field(c, "potion"), "value");
    if potion.r() != 0
        && let Some(id) = potion_id(rt, potion)
    {
        m.insert("potion".to_owned(), Nbt::String(id));
    }
    let color = rt.get_field(rt.get_field(c, "customColor"), "value");
    if !matches!(color, V::Void | V::R(0)) {
        m.insert("custom_color".to_owned(), Nbt::Int(crate::mixin::unbox(rt, color, 'I').i()));
    }
    let effects: Vec<Nbt> = jutil::elements(rt, rt.get_field(c, "customEffects")).into_iter().filter_map(|e| encode_effect(rt, e)).collect();
    if !effects.is_empty() {
        m.insert("custom_effects".to_owned(), Nbt::List(effects));
    }
    (!m.is_empty()).then_some(Nbt::Compound(m))
}

fn encode_effect(rt: &Rt, e: V) -> Option<Nbt> {
    let id = effect_id(rt, rt.get_field(e, "$effect"))?;
    let int = |f: &'static str| rt.get_field(e, f).i();
    let (amplifier, duration, visible) = (int("$amplifier"), int("$duration"), int("$visible") != 0);
    let mut m = BTreeMap::from([("id".to_owned(), Nbt::String(id)), ("show_icon".to_owned(), Nbt::Byte((int("$showIcon") != 0) as i8))]);
    if amplifier != 0 {
        m.insert("amplifier".to_owned(), Nbt::Byte(amplifier as i8));
    }
    if duration != 0 {
        m.insert("duration".to_owned(), Nbt::Int(duration));
    }
    if int("$ambient") != 0 {
        m.insert("ambient".to_owned(), Nbt::Byte(1));
    }
    if !visible {
        m.insert("show_particles".to_owned(), Nbt::Byte(0));
    }
    Some(Nbt::Compound(m))
}

/// Potion contents read from vanilla's form (as JSON): a potion's id alone, or what
/// [`encode_potion_contents`] writes.
pub(crate) fn decode_potion_contents(rt: &mut Rt, v: &serde_json::Value) -> V {
    use serde_json::Value;
    let (potion, color, effects) = match v {
        Value::String(id) => (Some(id.clone()), None, Vec::new()),
        Value::Object(m) => (
            m.get("potion").and_then(Value::as_str).map(str::to_owned),
            m.get("custom_color").and_then(Value::as_i64),
            m.get("custom_effects").and_then(Value::as_array).cloned().unwrap_or_default(),
        ),
        _ => (None, None, Vec::new()),
    };
    let holder = potion.map_or(V::NULL, |id| potion_holder(rt, &id));
    let effects: Vec<V> = effects.iter().filter_map(|e| decode_effect(rt, e)).collect();
    potion_contents(rt, holder, color.map_or(V::NULL, |c| V::I(c as i32)), effects)
}

fn decode_effect(rt: &mut Rt, e: &serde_json::Value) -> Option<V> {
    let id = e.get("id")?.as_str()?;
    let int = |k: &str, default: i64| e.get(k).and_then(|v| v.as_i64().or_else(|| v.as_bool().map(i64::from))).unwrap_or(default);
    let visible = int("show_particles", 1);
    let holder = effect_holder(rt, id);
    let i = rt.new_object(INSTANCE);
    rt.put_field(i, "$effect", holder);
    for (f, v) in [("$duration", int("duration", 0)), ("$amplifier", int("amplifier", 0)), ("$ambient", int("ambient", 0)), ("$visible", visible), ("$showIcon", int("show_icon", visible))] {
        rt.put_field(i, f, V::I(v as i32));
    }
    Some(i)
}

/// The id of an effect: a holder's (vanilla's, or a mod's `DeferredHolder`), or a registered one's.
pub(crate) fn effect_id(rt: &Rt, v: V) -> Option<String> {
    if let Some(id) = rt.str_of(rt.get_field(v, "$id")).filter(|s| !s.is_empty()) {
        return Some(id.to_owned());
    }
    rt.registered_name(v, "mob_effect").map(str::to_owned)
}

/// The instance a `PossibleEffect` applies: the one it was given, else what its supplier makes.
fn possible_instance(rt: &mut Rt, e: V) -> V {
    let given = rt.get_field(e, "$instance");
    if given.r() != 0 {
        return given;
    }
    let supplier = rt.get_field(e, "effectSupplier");
    if supplier.r() == 0 {
        return V::NULL;
    }
    let made = rt.call_functional(SUPPLY, supplier, &[]);
    rt.report_thrown("FoodProperties$PossibleEffect.effect");
    made
}

/// A copy of the instance `from` (`new MobEffectInstance(other)`).
fn copy_instance(rt: &mut Rt, from: V) -> V {
    let e = rt.new_object(INSTANCE);
    for f in ["$effect", "$duration", "$amplifier", "$ambient", "$visible", "$showIcon"] {
        let v = rt.get_field(from, f);
        rt.put_field(e, f, v);
    }
    e
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    matches!(owner, BUILDER | POSSIBLE | INSTANCE)
        || (owner == EFFECT && EFFECT_METHODS.contains(&name))
        || (owner == POTION_CONTENTS && (matches!(name, "createItemStack" | "is" | "withPotion") || (name == "<init>" && desc == "(Lnet/minecraft/core/Holder;)V")))
}

/// What a mod's own effect (a `MobEffect` subclass) takes from vanilla's class.
const EFFECT_METHODS: [&str; 15] = [
    "<init>",
    "getCategory",
    "getColor",
    "isBeneficial",
    "isInstantenous",
    "addAttributeModifier",
    "applyEffectTick",
    "shouldApplyEffectTickThisTick",
    "onEffectStarted",
    "onEffectAdded",
    "onMobRemoved",
    "onMobHurt",
    "getDescriptionId",
    "getDisplayName",
    "withSoundOnAdded",
];

/// `FoodProperties.Builder`, `PossibleEffect.effect()` and `MobEffectInstance`; None for anything else.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let set = |rt: &mut Rt, field: &'static str, v: V| {
        rt.put_field(this, field, v);
        this
    };
    Some(match (owner, name) {
        // `PotionContents.createItemStack(item, potion)`: one of the item holding that potion.
        (POTION_CONTENTS, "createItemStack") => {
            let item = api_common::item_like_name(rt, a(0));
            let stack = api_common::stack(rt, &item, 1);
            let contents = potion_contents(rt, a(1), V::NULL, Vec::new());
            crate::components::set_vanilla(rt, stack, POTION_CONTENTS_ID, contents);
            stack
        }
        // `is(potion)`: the contents are that potion alone (holders compare by their ids), with no
        // effects of their own.
        (POTION_CONTENTS, "is") => {
            let held = rt.get_field(rt.get_field(this, "potion"), "value");
            let same = held.r() != 0 && potion_id(rt, held).is_some_and(|id| potion_id(rt, a(1)).as_deref() == Some(id.as_str()));
            V::I((same && jutil::elements(rt, rt.get_field(this, "customEffects")).is_empty()) as i32)
        }
        // `withPotion(potion)`: the same color and effects with that potion.
        (POTION_CONTENTS, "withPotion") => {
            let (color, effects) = (rt.get_field(this, "customColor"), rt.get_field(this, "customEffects"));
            let potion = api_common::optional(rt, a(1));
            crate::records::make(rt, POTION_CONTENTS, &[potion, color, effects])
        }
        // `new PotionContents(potion)`: that potion, no color of its own, no effects added.
        (POTION_CONTENTS, "<init>") => {
            let (potion, none, effects) = (api_common::optional(rt, a(1)), api_common::optional(rt, V::NULL), jutil::new_list(rt, Vec::new()));
            rt.put_field(this, "potion", potion);
            rt.put_field(this, "customColor", none);
            rt.put_field(this, "customEffects", effects);
            V::Void
        }
        (BUILDER, "<init>") => {
            let list = jutil::new_list(rt, Vec::new());
            rt.put_field(this, "$effects", list);
            rt.put_field(this, "$eatSeconds", V::F(EAT_SECONDS));
            V::Void
        }
        (BUILDER, "nutrition") => set(rt, "$nutrition", V::I(a(1).i())),
        (BUILDER, "saturationModifier") => set(rt, "$saturationModifier", V::F(a(1).f())),
        (BUILDER, "alwaysEdible") => set(rt, "$canAlwaysEat", V::I(1)),
        (BUILDER, "fast") => set(rt, "$eatSeconds", V::F(FAST_SECONDS)),
        // `effect(instance, probability)`, or NeoForge's `effect(supplier, probability)`: asked
        // each time the food is eaten, so a mod's own effect may be registered after the food.
        (BUILDER, "effect") => {
            let e = rt.new_object(POSSIBLE);
            let field = if desc.starts_with("(Lnet/minecraft/world/effect/MobEffectInstance;") { "$instance" } else { "effectSupplier" };
            rt.put_field(e, field, a(1));
            rt.put_field(e, "probability", V::F(a(2).f()));
            let list = rt.get_field(this, "$effects");
            api::call(rt, LIST_ADD, &[list, e]);
            this
        }
        (BUILDER, "usingConvertsTo") => {
            let item = api_common::item_like_name(rt, a(1));
            let s = api_common::stack(rt, &item, 1);
            set(rt, "$usingConvertsTo", s)
        }
        // The record, as vanilla's `build()` makes it: `FoodConstants.saturationByModifier`.
        (BUILDER, "build") => {
            let nutrition = rt.get_field(this, "$nutrition").i();
            let modifier = rt.get_field(this, "$saturationModifier").f();
            let seconds = match rt.get_field(this, "$eatSeconds") {
                V::F(s) => s,
                _ => EAT_SECONDS,
            };
            let converts = rt.get_field(this, "$usingConvertsTo");
            let converts = api_common::optional(rt, converts);
            let effects = jutil::elements(rt, rt.get_field(this, "$effects"));
            let effects = jutil::new_list(rt, effects);
            let f = rt.new_object(FOOD);
            rt.put_field(f, "nutrition", V::I(nutrition));
            rt.put_field(f, "saturation", V::F(nutrition as f32 * modifier * 2.0));
            rt.put_field(f, "canAlwaysEat", V::I(rt.get_field(this, "$canAlwaysEat").i()));
            rt.put_field(f, "eatSeconds", V::F(seconds));
            rt.put_field(f, "usingConvertsTo", converts);
            rt.put_field(f, "effects", effects);
            f
        }
        (POSSIBLE, "effect") => {
            let made = possible_instance(rt, this);
            if made.r() == 0 {
                return Some(V::NULL);
            }
            copy_instance(rt, made)
        }
        // `(effect[, duration[, amplifier[, ambient, visible[, showIcon[, hiddenEffect]]]]])`: an
        // instance shows its icon when it shows its particles, unless told; or a copy of another.
        (INSTANCE, "<init>") if desc.starts_with("(Lnet/minecraft/world/effect/MobEffectInstance;)") => {
            let from = a(1);
            for f in ["$effect", "$duration", "$amplifier", "$ambient", "$visible", "$showIcon"] {
                let v = rt.get_field(from, f);
                rt.put_field(this, f, v);
            }
            V::Void
        }
        (INSTANCE, "<init>") => {
            let n = crate::jlang::kinds(desc).len();
            let visible = if n >= 5 { a(5).i() } else { 1 };
            rt.put_field(this, "$effect", a(1));
            rt.put_field(this, "$duration", V::I(if n >= 2 { a(2).i() } else { 0 }));
            rt.put_field(this, "$amplifier", V::I(if n >= 3 { a(3).i() } else { 0 }));
            rt.put_field(this, "$ambient", V::I(if n >= 4 { a(4).i() } else { 0 }));
            rt.put_field(this, "$visible", V::I(visible));
            rt.put_field(this, "$showIcon", V::I(if n >= 6 { a(6).i() } else { visible }));
            V::Void
        }
        // `MobEffect(category, color[, particle])`, as a mod's effect calls `super`.
        (EFFECT, "<init>") => {
            rt.put_field(this, "$category", a(1));
            rt.put_field(this, "$color", V::I(a(2).i()));
            if desc.contains("ParticleOptions;") || desc.contains("Ljava/util/function/Function;") {
                rt.put_field(this, "$particle", a(3));
            }
            V::Void
        }
        (EFFECT, "getCategory") => rt.get_field(this, "$category"),
        (EFFECT, "getColor") => V::I(rt.get_field(this, "$color").i()),
        (EFFECT, "isBeneficial") => {
            let category = rt.get_field(this, "$category");
            V::I((rt.str_of(rt.get_field(category, "$name")) == Some("BENEFICIAL")) as i32)
        }
        (EFFECT, "isInstantenous" | "shouldApplyEffectTickThisTick") => V::I(0),
        // Vanilla's own tick: nothing happens, and the effect stays.
        (EFFECT, "applyEffectTick") => V::I(1),
        (EFFECT, "onEffectStarted" | "onEffectAdded" | "onMobRemoved" | "onMobHurt") => V::Void,
        // `addAttributeModifier(attribute, id, amount, operation)`: kept in order; the effect again.
        (EFFECT, "addAttributeModifier") => {
            let mut list = rt.get_field(this, "$modifiers");
            if list.r() == 0 {
                list = jutil::new_list(rt, Vec::new());
                rt.put_field(this, "$modifiers", list);
            }
            let entry = jutil::new_list(rt, args[1..].to_vec());
            jutil::list_add(rt, list, entry);
            this
        }
        (EFFECT, "withSoundOnAdded") => set(rt, "$soundOnAdded", a(1)),
        // `effect.<namespace>.<path>`, from the name it was registered under.
        (EFFECT, "getDescriptionId" | "getDisplayName") => {
            let id = effect_id(rt, this).unwrap_or_default();
            let (ns, path) = id.split_once(':').unwrap_or(("minecraft", id.as_str()));
            let key = format!("effect.{ns}.{}", path.replace('/', "."));
            let key = rt.string(&key);
            if name == "getDescriptionId" { key } else { crate::api_common::component(rt, key, true, V::NULL) }
        }
        (INSTANCE, "getEffect") => rt.get_field(this, "$effect"),
        (INSTANCE, "getDuration") => V::I(rt.get_field(this, "$duration").i()),
        (INSTANCE, "getAmplifier") => V::I(rt.get_field(this, "$amplifier").i()),
        (INSTANCE, "isAmbient") => V::I(rt.get_field(this, "$ambient").i()),
        (INSTANCE, "isVisible") => V::I(rt.get_field(this, "$visible").i()),
        (INSTANCE, "showIcon") => V::I(rt.get_field(this, "$showIcon").i()),
        (INSTANCE, "isInfiniteDuration") => V::I((rt.get_field(this, "$duration").i() == -1) as i32),
        // `endsWithin(ticks)`: an instance with an end, at most that many ticks away.
        (INSTANCE, "endsWithin") => {
            let duration = rt.get_field(this, "$duration").i();
            V::I((duration != -1 && duration <= a(1).i()) as i32)
        }
        // Its effect's (`effect.minecraft.speed`).
        (INSTANCE, "getDescriptionId") => {
            let id = effect_id(rt, rt.get_field(this, "$effect")).unwrap_or_default();
            let (ns, path) = id.split_once(':').unwrap_or(("minecraft", id.as_str()));
            rt.string(&format!("effect.{ns}.{}", path.replace('/', ".")))
        }
        (INSTANCE, "is") => {
            let mine = effect_id(rt, rt.get_field(this, "$effect"));
            V::I((mine.is_some() && mine == effect_id(rt, a(1))) as i32)
        }
        _ => return None,
    })
}

/// A float as JSON writes it: Java's shortest form (`7.2000003`), `6.0` for a whole one.
fn number(f: f32) -> String {
    let s = format!("{f}");
    if s.contains(['.', 'e', 'E']) || !f.is_finite() { s } else { format!("{s}.0") }
}

/// One effect as `MobEffectInstance.CODEC` writes it, as vanilla's items have them.
fn effect_json(rt: &Rt, instance: V) -> Option<String> {
    let id = effect_id(rt, rt.get_field(instance, "$effect"))?;
    let mut parts = vec![format!("\"id\": \"{id}\"")];
    let int = |f: &'static str| rt.get_field(instance, f).i();
    if int("$duration") != 0 {
        parts.push(format!("\"duration\": {}", int("$duration")));
    }
    if int("$amplifier") != 0 {
        parts.push(format!("\"amplifier\": {}", int("$amplifier")));
    }
    if int("$ambient") != 0 {
        parts.push("\"ambient\": true".into());
    }
    if int("$visible") == 0 {
        parts.push("\"show_particles\": false".into());
    }
    parts.push(format!("\"show_icon\": {}", int("$showIcon") != 0));
    Some(format!("{{{}}}", parts.join(", ")))
}

// ---- `MobEffectInstance.CODEC` and `FoodProperties.DIRECT_CODEC` (a mod's data: Create's potato
// projectile types) ----

type JsonMap = serde_json::Map<String, serde_json::Value>;

/// A number a codec reads at `key`, if there.
fn json_number(o: &JsonMap, key: &str) -> Result<Option<f64>, String> {
    match o.get(key) {
        None => Ok(None),
        Some(n) => n.as_f64().map(Some).ok_or_else(|| format!("{key}: Not a number: {n}")),
    }
}

/// A boolean a codec reads at `key`, else `default`: JSON's, or a number (DFU's `JsonOps` takes
/// one as true unless its byte is 0).
fn json_bool(o: &JsonMap, key: &str, default: bool) -> Result<bool, String> {
    match o.get(key) {
        None => Ok(default),
        Some(serde_json::Value::Bool(b)) => Ok(*b),
        Some(n) if n.is_number() => Ok(n.as_f64().unwrap_or(0.0) as i64 as i8 != 0),
        Some(other) => Err(format!("{key}: Not a boolean: {other}")),
    }
}

/// `MobEffectInstance.CODEC`'s instance: `{id, amplifier, duration, ambient, show_particles,
/// show_icon}`, all but the id optional (the amplifier an unsigned byte, the icon shown with the
/// particles unless told). A hidden effect and NeoForge's cures aren't kept.
pub(crate) fn decode_instance(rt: &mut Rt, j: &serde_json::Value) -> Result<V, String> {
    let o = j.as_object().ok_or_else(|| format!("Not a map: {j}"))?;
    let id = o.get("id").ok_or("No key id in MapLike")?;
    let id = id.as_str().ok_or_else(|| format!("id: Not a string: {id}"))?;
    let id = if id.contains(':') { id.to_owned() } else { format!("minecraft:{id}") };
    if crate::neo::registry_get(rt, "mob_effect", &id).r() == 0 {
        return Err(format!("id: Unknown registry key in mob_effect: {id}"));
    }
    let effect = effect_holder(rt, &id);
    let amplifier = json_number(o, "amplifier")?.map_or(0, |n| n as i64 as i8 as u8 as i32);
    let duration = json_number(o, "duration")?.map_or(0, |n| n as i32);
    let ambient = json_bool(o, "ambient", false)?;
    let particles = json_bool(o, "show_particles", true)?;
    let icon = json_bool(o, "show_icon", particles)?;
    let e = rt.new_object(INSTANCE);
    rt.put_field(e, "$effect", effect);
    rt.put_field(e, "$duration", V::I(duration));
    rt.put_field(e, "$amplifier", V::I(amplifier));
    rt.put_field(e, "$ambient", V::I(ambient as i32));
    rt.put_field(e, "$visible", V::I(particles as i32));
    rt.put_field(e, "$showIcon", V::I(icon as i32));
    Ok(e)
}

/// The tag `MobEffectInstance.CODEC` writes: what differs from the defaults, and the icon always.
pub(crate) fn encode_instance(rt: &Rt, e: V) -> Result<Nbt, String> {
    let id = effect_id(rt, rt.get_field(e, "$effect")).ok_or("Unknown registry element in mob_effect")?;
    let int = |f: &'static str| rt.get_field(e, f).i();
    let mut m = BTreeMap::from([("id".to_owned(), Nbt::String(id))]);
    if int("$amplifier") != 0 {
        m.insert("amplifier".to_owned(), Nbt::Byte(int("$amplifier") as i8));
    }
    if int("$duration") != 0 {
        m.insert("duration".to_owned(), Nbt::Int(int("$duration")));
    }
    if int("$ambient") != 0 {
        m.insert("ambient".to_owned(), Nbt::Byte(1));
    }
    if int("$visible") == 0 {
        m.insert("show_particles".to_owned(), Nbt::Byte(0));
    }
    m.insert("show_icon".to_owned(), Nbt::Byte((int("$showIcon") != 0) as i8));
    Ok(Nbt::Compound(m))
}

/// `FoodProperties.DIRECT_CODEC`'s food: `{nutrition, saturation, can_always_eat, eat_seconds,
/// using_converts_to, effects}`, the last four optional; an effect is `{effect, probability}`.
pub(crate) fn decode_food(rt: &mut Rt, j: &serde_json::Value) -> Result<V, String> {
    let o = j.as_object().ok_or_else(|| format!("Not a map: {j}"))?;
    let nutrition = json_number(o, "nutrition")?.ok_or("No key nutrition in MapLike")? as i32;
    if nutrition < 0 {
        return Err(format!("nutrition: Value must be non-negative: {nutrition}"));
    }
    let saturation = json_number(o, "saturation")?.ok_or("No key saturation in MapLike")? as f32;
    let always = json_bool(o, "can_always_eat", false)?;
    let seconds = json_number(o, "eat_seconds")?.map_or(EAT_SECONDS, |s| s as f32);
    if seconds <= 0.0 {
        return Err(format!("eat_seconds: Value must be positive: {seconds}"));
    }
    let converts = match o.get("using_converts_to") {
        Some(s) => crate::recipes::decode_native(rt, "item_stack", s).map_err(|e| format!("using_converts_to: {e}"))?,
        None => V::NULL,
    };
    let mut effects = Vec::new();
    for e in o.get("effects").map(|l| l.as_array().ok_or_else(|| format!("effects: Not a list: {l}"))).transpose()?.into_iter().flatten() {
        let eo = e.as_object().ok_or_else(|| format!("effects: Not a map: {e}"))?;
        let instance = decode_instance(rt, eo.get("effect").ok_or("effects: No key effect in MapLike")?).map_err(|err| format!("effects: effect: {err}"))?;
        let p = json_number(eo, "probability")?.map_or(1.0, |p| p as f32);
        if !(0.0..=1.0).contains(&p) {
            return Err(format!("effects: probability: Value {p} outside of range [0.0:1.0]"));
        }
        let possible = rt.new_object(POSSIBLE);
        rt.put_field(possible, "$instance", instance);
        rt.put_field(possible, "probability", V::F(p));
        effects.push(possible);
    }
    let (converts, effects) = (api_common::optional(rt, converts), jutil::new_list(rt, effects));
    let f = rt.new_object(FOOD);
    rt.put_field(f, "nutrition", V::I(nutrition));
    rt.put_field(f, "saturation", V::F(saturation));
    rt.put_field(f, "canAlwaysEat", V::I(always as i32));
    rt.put_field(f, "eatSeconds", V::F(seconds));
    rt.put_field(f, "usingConvertsTo", converts);
    rt.put_field(f, "effects", effects);
    Ok(f)
}

/// The tag `FoodProperties.DIRECT_CODEC` writes: what differs from the defaults (the remainder a
/// single item, `{id, components}`).
pub(crate) fn encode_food(rt: &mut Rt, f: V) -> Result<Nbt, String> {
    let field = |rt: &Rt, n: &'static str| rt.get_field(f, n);
    let mut m = BTreeMap::from([("nutrition".to_owned(), Nbt::Int(field(rt, "nutrition").i())), ("saturation".to_owned(), Nbt::Float(field(rt, "saturation").f()))]);
    if field(rt, "canAlwaysEat").i() != 0 {
        m.insert("can_always_eat".to_owned(), Nbt::Byte(1));
    }
    if field(rt, "eatSeconds").f() != EAT_SECONDS {
        m.insert("eat_seconds".to_owned(), Nbt::Float(field(rt, "eatSeconds").f()));
    }
    let converts = rt.get_field(field(rt, "usingConvertsTo"), "value");
    if converts.r() != 0 {
        let mut item = crate::codecs::write_native(rt, "item_stack", converts)?;
        if let Nbt::Compound(c) = &mut item {
            c.remove("count");
        }
        m.insert("using_converts_to".to_owned(), item);
    }
    let mut effects = Vec::new();
    for e in jutil::elements(rt, field(rt, "effects")) {
        let instance = possible_instance(rt, e);
        if instance.r() == 0 {
            return Err("effects: effect: no instance".into());
        }
        let mut written = BTreeMap::from([("effect".to_owned(), encode_instance(rt, instance)?)]);
        let p = rt.get_field(e, "probability").f();
        if p != 1.0 {
            written.insert("probability".to_owned(), Nbt::Float(p));
        }
        effects.push(Nbt::Compound(written));
    }
    if !effects.is_empty() {
        m.insert("effects".to_owned(), Nbt::List(effects));
    }
    Ok(Nbt::Compound(m))
}

/// What a mod class of `item` answers to `Item`'s `nd`, when it overrides it.
fn ask(rt: &mut Rt, item: V, key: &'static str, nd: &'static str, args: &[V]) -> Option<V> {
    let class = rt.class_of(item);
    if !rt.overrides.is_some_and(|o| o(class, nd)) {
        return None;
    }
    let vc = rt.vcall?;
    let mut all = vec![item];
    all.extend_from_slice(args);
    let r = vc(rt, key, nd, &all);
    rt.report_thrown(nd);
    Some(r)
}

/// What an edible item declares, in 26.3's components (`"key": value` JSON entries): `food`,
/// `consumable` and, when eating leaves something, `use_remainder`. Empty for an item with no food.
/// `item` is the item, `props` the `Item.Properties` it was made with.
pub fn components(rt: &mut Rt, item: V, props: V) -> Vec<String> {
    let food = if props.r() != 0 { rt.get_field(props, "$food") } else { V::NULL };
    if food.r() == 0 {
        return Vec::new();
    }
    let field = |rt: &Rt, n: &'static str| rt.get_field(food, n);
    let mut f = format!("\"nutrition\": {}, \"saturation\": {}", field(rt, "nutrition").i(), number(field(rt, "saturation").f()));
    if field(rt, "canAlwaysEat").i() != 0 {
        f.push_str(", \"can_always_eat\": true");
    }
    let mut out = vec![format!("\"minecraft:food\": {{{f}}}")];

    let name = rt.registered_name(item, "item").unwrap_or("minecraft:air").to_owned();
    let stack = api_common::stack(rt, &name, 1);
    let ticks = ask(rt, item, USE_DURATION_KEY, USE_DURATION, &[stack, V::NULL]).map(|v| v.i()).filter(|t| *t > 0);
    let seconds = match ticks {
        Some(t) => t as f32 / 20.0,
        None => field(rt, "eatSeconds").f(),
    };
    // 26.3's names: 1.21.1's `SPEAR` (the trident's) is `trident`; NeoForge's `CUSTOM` has none.
    let animation = match ask(rt, item, USE_ANIMATION_KEY, USE_ANIMATION, &[stack]).and_then(|v| rt.str_of(rt.get_field(v, "$name")).map(str::to_ascii_lowercase)) {
        Some(a) if a == "spear" => "trident".to_owned(),
        Some(a) if a == "custom" => "none".to_owned(),
        Some(a) => a,
        None => "eat".to_owned(),
    };
    // As `Consumables.defaultFood()` and `defaultDrink()`: a drink's sound, and no crumbs (1.21.1
    // shows an item's particles only while it's eaten).
    let drink = animation == "drink";
    let sound = if drink { ask(rt, item, DRINKING_SOUND_KEY, DRINKING_SOUND, &[]) } else { ask(rt, item, EATING_SOUND_KEY, EATING_SOUND, &[]) };
    let sound = sound.and_then(|s| crate::sounds::sound_id(rt, s));
    let mut c = Vec::new();
    if seconds != EAT_SECONDS {
        c.push(format!("\"consume_seconds\": {}", number(seconds)));
    }
    if animation != "eat" {
        c.push(format!("\"animation\": \"{animation}\""));
    }
    match sound {
        Some(s) => c.push(format!("\"sound\": \"{s}\"")),
        None if drink => c.push("\"sound\": \"minecraft:entity.generic.drink\"".into()),
        None => {}
    }
    if animation != "eat" {
        c.push("\"has_consume_particles\": false".into());
    }
    // Each effect rolls its own chance in 1.21.1: the sure ones apply together, the others alone.
    let (mut sure, mut chance) = (Vec::new(), Vec::new());
    for e in jutil::elements(rt, field(rt, "effects")) {
        let p = rt.get_field(e, "probability").f();
        let instance = possible_instance(rt, e);
        let Some(json) = (instance.r() != 0).then(|| effect_json(rt, instance)).flatten() else { continue };
        if p >= 1.0 { sure.push(json) } else { chance.push((json, p)) }
    }
    let mut applied = Vec::new();
    if !sure.is_empty() {
        applied.push(format!("{{\"type\": \"minecraft:apply_effects\", \"effects\": [{}]}}", sure.join(", ")));
    }
    for (json, p) in chance {
        applied.push(format!("{{\"type\": \"minecraft:apply_effects\", \"effects\": [{json}], \"probability\": {}}}", number(p)));
    }
    if !applied.is_empty() {
        c.push(format!("\"on_consume_effects\": [{}]", applied.join(", ")));
    }
    out.push(format!("\"minecraft:consumable\": {{{}}}", c.join(", ")));

    let converts = rt.get_field(field(rt, "usingConvertsTo"), "value");
    if converts.r() != 0 && !api_common::stack_empty(rt, converts) {
        let rest = api_common::item_like_name(rt, rt.get_field(converts, "$item"));
        out.push(format!("\"minecraft:use_remainder\": {{\"id\": \"{rest}\"}}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const INSTANCE_6: &str = "net/minecraft/world/effect/MobEffectInstance.<init>.(Lnet/minecraft/core/Holder;IIZZZ)V";
    const GET: &str = "net/minecraft/world/item/ItemStack.get.(Lnet/minecraft/core/component/DataComponentType;)Ljava/lang/Object;";
    const IS: &str = "net/minecraft/world/item/alchemy/PotionContents.is.(Lnet/minecraft/core/Holder;)Z";

    /// A water bottle as Create's spout makes it (`PotionContents.createItemStack(Items.POTION,
    /// Potions.WATER)`): a potion holding water, which the game's stack carries as vanilla's
    /// `minecraft:potion_contents`. Contents with a color and an effect of their own come from a
    /// game stack and go back the same; a plain potion holds the empty contents.
    #[test]
    fn a_made_potion_carries_its_contents_to_the_game_and_back() {
        let mut rt = Rt::new();
        crate::install();
        let item = api::get_static(&mut rt, "net/minecraft/world/item/Items.POTION");
        let water = api::get_static(&mut rt, "net/minecraft/world/item/alchemy/Potions.WATER");
        let make = "net/minecraft/world/item/alchemy/PotionContents.createItemStack.(Lnet/minecraft/world/item/Item;Lnet/minecraft/core/Holder;)Lnet/minecraft/world/item/ItemStack;";
        let stack = api::call(&mut rt, make, &[item, water]);
        assert_eq!(api_common::item_like_name(&rt, rt.get_field(stack, "$item")), "minecraft:potion");
        let ty = api::get_static(&mut rt, "net/minecraft/core/component/DataComponents.POTION_CONTENTS");
        let contents = api::call(&mut rt, GET, &[stack, ty]);
        assert_eq!(api::call(&mut rt, IS, &[contents, water]).i(), 1);
        let awkward = api::get_static(&mut rt, "net/minecraft/world/item/alchemy/Potions.AWKWARD");
        assert_eq!(api::call(&mut rt, IS, &[contents, awkward]).i(), 0);
        let water_only = Nbt::Compound(BTreeMap::from([("potion".to_owned(), Nbt::String("minecraft:water".into()))]));
        assert_eq!(crate::components::saved(&mut rt, stack), vec![(POTION_CONTENTS_ID.to_owned(), water_only)]);

        let effect = BTreeMap::from([
            ("id".to_owned(), Nbt::String("minecraft:speed".into())),
            ("duration".to_owned(), Nbt::Int(600)),
            ("amplifier".to_owned(), Nbt::Byte(1)),
            ("show_icon".to_owned(), Nbt::Byte(1)),
        ]);
        let carried = Nbt::Compound(BTreeMap::from([
            ("potion".to_owned(), Nbt::String("minecraft:awkward".into())),
            ("custom_color".to_owned(), Nbt::Int(0x385DC6)),
            ("custom_effects".to_owned(), Nbt::List(vec![Nbt::Compound(effect)])),
        ]));
        let other = api_common::stack(&mut rt, "minecraft:potion", 1);
        crate::components::load(&mut rt, other, &[(POTION_CONTENTS_ID.to_owned(), carried.clone())]);
        let read = api::call(&mut rt, GET, &[other, ty]);
        assert_eq!(rt.get_field(rt.get_field(read, "potion"), "value"), awkward);
        assert_eq!(api::call(&mut rt, IS, &[read, awkward]).i(), 0, "effects of its own make it another potion");
        assert_eq!(crate::components::saved(&mut rt, other), vec![(POTION_CONTENTS_ID.to_owned(), carried)]);

        let plain = api_common::stack(&mut rt, "minecraft:potion", 1);
        let empty = api::call(&mut rt, GET, &[plain, ty]);
        assert_eq!(empty, api::get_static(&mut rt, "net/minecraft/world/item/alchemy/PotionContents.EMPTY"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    fn builder(rt: &mut Rt, nutrition: i32, modifier: f32) -> V {
        let b = rt.new_object(BUILDER);
        api::call(rt, NEW_BUILDER, &[b]);
        api::call(rt, NUTRITION, &[b, V::I(nutrition)]);
        api::call(rt, MODIFIER, &[b, V::F(modifier)]);
        b
    }

    fn declared(rt: &mut Rt, food: V) -> serde_json::Value {
        let props = rt.new_object(crate::blocks::ITEM_PROPERTIES);
        rt.put_field(props, "$food", food);
        let item = rt.new_object("net/minecraft/world/item/Item");
        let json = format!("{{{}}}", components(rt, item, props).join(", "));
        serde_json::from_str(&json).unwrap()
    }

    /// Vanilla's own foods, built as 1.21.1's `Foods` builds them, declare what 26.3's items.json
    /// has for them: mushroom stew (6, 0.6; Java's float product 7.2000003, and a bowl back) and
    /// the golden apple (4, 1.2, always edible; regeneration II for 5 s and absorption for 2 min).
    #[test]
    fn vanilla_foods_built_by_the_builder_declare_what_vanilla_items_have() {
        let mut rt = Rt::new();
        crate::install();
        let stew = builder(&mut rt, 6, 0.6);
        let bowl = crate::api_common::item(&mut rt, "minecraft:bowl");
        api::call(&mut rt, CONVERTS, &[stew, bowl]);
        let stew = api::call(&mut rt, BUILD, &[stew]);
        assert_eq!(
            declared(&mut rt, stew),
            serde_json::json!({
                "minecraft:consumable": {},
                "minecraft:food": {"nutrition": 6, "saturation": 7.2000003},
                "minecraft:use_remainder": {"id": "minecraft:bowl"}
            })
        );

        let apple = builder(&mut rt, 4, 1.2);
        api::call(&mut rt, ALWAYS, &[apple]);
        for (field, duration, amplifier) in [("REGENERATION", 100, 1), ("ABSORPTION", 2400, 0)] {
            let effect = api::get_static(&mut rt, Box::leak(format!("{EFFECTS}.{field}").into_boxed_str()));
            let e = rt.new_object(INSTANCE);
            api::call(&mut rt, INSTANCE_3, &[e, effect, V::I(duration), V::I(amplifier)]);
            api::call(&mut rt, EFFECT_OF, &[apple, e, V::F(1.0)]);
        }
        let apple = api::call(&mut rt, BUILD, &[apple]);
        assert_eq!(
            declared(&mut rt, apple),
            serde_json::json!({
                "minecraft:consumable": {"on_consume_effects": [{"type": "minecraft:apply_effects", "effects": [
                    {"amplifier": 1, "duration": 100, "id": "minecraft:regeneration", "show_icon": true},
                    {"duration": 2400, "id": "minecraft:absorption", "show_icon": true}
                ]}]},
                "minecraft:food": {"can_always_eat": true, "nutrition": 4, "saturation": 9.6}
            })
        );
    }

    /// Vanilla's own foods (`Foods.GOLDEN_APPLE`), one object each, declare what 26.3's items.json
    /// has for them; dried kelp is eaten fast.
    #[test]
    fn vanillas_foods_are_its_own() {
        let mut rt = Rt::new();
        crate::install();
        let apple = api::get_static(&mut rt, "net/minecraft/world/food/Foods.GOLDEN_APPLE");
        assert_eq!(api::get_static(&mut rt, "net/minecraft/world/food/Foods.GOLDEN_APPLE"), apple);
        assert_eq!(
            declared(&mut rt, apple),
            serde_json::json!({
                "minecraft:consumable": {"on_consume_effects": [{"type": "minecraft:apply_effects", "effects": [
                    {"amplifier": 1, "duration": 100, "id": "minecraft:regeneration", "show_icon": true},
                    {"duration": 2400, "id": "minecraft:absorption", "show_icon": true}
                ]}]},
                "minecraft:food": {"can_always_eat": true, "nutrition": 4, "saturation": 9.6}
            })
        );
        let kelp = api::get_static(&mut rt, "net/minecraft/world/food/Foods.DRIED_KELP");
        assert_eq!(declared(&mut rt, kelp)["minecraft:consumable"], serde_json::json!({"consume_seconds": 0.8}));
    }

    /// A chance effect keeps its own roll (raw chicken's hunger: 30 %), and an instance that hides
    /// its particles and icon says so (Create's builder's tea: haste for 3 minutes, unseen).
    #[test]
    fn chance_effects_roll_alone_and_hidden_effects_stay_hidden() {
        let mut rt = Rt::new();
        crate::install();
        let chicken = builder(&mut rt, 2, 0.3);
        let hunger = api::get_static(&mut rt, "net/minecraft/world/effect/MobEffects.HUNGER");
        let e = rt.new_object(INSTANCE);
        api::call(&mut rt, INSTANCE_3, &[e, hunger, V::I(600), V::I(0)]);
        api::call(&mut rt, EFFECT_OF, &[chicken, e, V::F(0.3)]);
        let haste = api::get_static(&mut rt, "net/minecraft/world/effect/MobEffects.DIG_SPEED");
        let h = rt.new_object(INSTANCE);
        api::call(&mut rt, INSTANCE_6, &[h, haste, V::I(3600), V::I(0), V::I(0), V::I(0), V::I(0)]);
        api::call(&mut rt, EFFECT_OF, &[chicken, h, V::F(1.0)]);
        let food = api::call(&mut rt, BUILD, &[chicken]);
        let declared = declared(&mut rt, food);
        assert_eq!(
            declared["minecraft:consumable"]["on_consume_effects"],
            serde_json::json!([
                {"type": "minecraft:apply_effects", "effects": [{"duration": 3600, "id": "minecraft:haste", "show_icon": false, "show_particles": false}]},
                {"type": "minecraft:apply_effects", "effects": [{"duration": 600, "id": "minecraft:hunger", "show_icon": true}], "probability": 0.3}
            ])
        );
        assert_eq!(declared["minecraft:food"], serde_json::json!({"nutrition": 2, "saturation": 1.2}));
    }

    /// Vanilla's potions: `Potions.WATER` is the potion registry's holder of `minecraft:water`, the
    /// same the registry gives for that id; `new PotionContents(holder)` holds it, with no color or
    /// effects of its own (SecurityCraft's water bottles).
    #[test]
    fn potions_are_holders_and_contents_hold_them() {
        let mut rt = Rt::new();
        crate::install();
        let water = api::get_static(&mut rt, "net/minecraft/world/item/alchemy/Potions.WATER");
        let registry = rt.string("potion");
        let id = rt.string("minecraft:water");
        let potion = api::call(&mut rt, "net/minecraft/core/Registry.get.(Lnet/minecraft/resources/ResourceLocation;)Ljava/lang/Object;", &[registry, id]);
        assert_eq!(api::call(&mut rt, "net/minecraft/core/Holder.value.()Ljava/lang/Object;", &[water]), potion);
        let key = api::call(&mut rt, "net/minecraft/core/Registry.getKey.(Ljava/lang/Object;)Lnet/minecraft/resources/ResourceLocation;", &[registry, potion]);
        assert_eq!(rt.str_of(key), Some("minecraft:water"));
        let contents = rt.new_object(POTION_CONTENTS);
        api::call(&mut rt, "net/minecraft/world/item/alchemy/PotionContents.<init>.(Lnet/minecraft/core/Holder;)V", &[contents, water]);
        let held = api::call(&mut rt, "net/minecraft/world/item/alchemy/PotionContents.potion.()Ljava/util/Optional;", &[contents]);
        assert_eq!(api::call(&mut rt, "java/util/Optional.get.()Ljava/lang/Object;", &[held]), water);
        let effects = api::call(&mut rt, "net/minecraft/world/item/alchemy/PotionContents.customEffects.()Ljava/util/List;", &[contents]);
        assert!(jutil::elements(&rt, effects).is_empty());
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
