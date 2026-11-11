//! Creative inventory tabs a mod registers (`CreativeModeTab.builder()...build()` in its
//! `creative_mode_tab` registry): their title, icon and items, from the mod's own code. The tab's
//! `displayItems` generator runs into an `Output` that keeps what it accepts, then NeoForge's
//! `BuildCreativeModeTabContentsEvent` lets the mod's listeners add more, as NeoForge builds a
//! tab's contents. The game shows them as a content mod's `creative_tab` files ([`tabs`]).
use crate::{api, Data, Rt, V};

const TAB: &str = "net/minecraft/world/item/CreativeModeTab";
const BUILDER: &str = "net/minecraft/world/item/CreativeModeTab$Builder";
const OUTPUT: &str = "net/minecraft/world/item/CreativeModeTab$Output";
const PARAMETERS: &str = "net/minecraft/world/item/CreativeModeTab$ItemDisplayParameters";
const VISIBILITY: &str = "net/minecraft/world/item/CreativeModeTab$TabVisibility";
const CONTENTS_EVENT: &str = "net/neoforged/neoforge/event/BuildCreativeModeTabContentsEvent";
const GENERATE: &str = "net/minecraft/world/item/CreativeModeTab$DisplayItemsGenerator.accept.(Lnet/minecraft/world/item/CreativeModeTab$ItemDisplayParameters;Lnet/minecraft/world/item/CreativeModeTab$Output;)V";
const SUPPLY: &str = "java/util/function/Supplier.get.()Ljava/lang/Object;";

/// `TabVisibility`, in declaration order.
const VISIBILITIES: [&str; 3] = ["PARENT_AND_SEARCH_TABS", "PARENT_TAB_ONLY", "SEARCH_TAB_ONLY"];

/// Vanilla's tabs (`CreativeModeTabs`), whose keys a mod orders its own after or before.
const VANILLA_TABS: [&str; 14] = [
    "BUILDING_BLOCKS",
    "COLORED_BLOCKS",
    "NATURAL_BLOCKS",
    "FUNCTIONAL_BLOCKS",
    "REDSTONE_BLOCKS",
    "HOTBAR",
    "SEARCH",
    "TOOLS_AND_UTILITIES",
    "COMBAT",
    "FOOD_AND_DRINKS",
    "INGREDIENTS",
    "SPAWN_EGGS",
    "OP_BLOCKS",
    "INVENTORY",
];

/// A tab as the mod's code built it.
#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    /// The tab's registered name (`create:base`).
    pub id: String,
    /// Its title's translation key or text (`itemGroup.create.base`).
    pub title: String,
    /// The item on its button.
    pub icon: String,
    /// Its items in order, each once: what its generator and the contents event accept for the
    /// tab itself (items for the search tab only are left out).
    pub items: Vec<String>,
}

fn visibility(rt: &mut Rt, i: usize) -> V {
    api::api_enum(rt, VISIBILITY, VISIBILITIES[i], i as i32)
}

/// One of vanilla's creative tabs as the registry hands it out (`BuiltInRegistries
/// .CREATIVE_MODE_TAB.get(CreativeModeTabs.BUILDING_BLOCKS)`): one object a tab. None for an id
/// that is no vanilla tab's.
pub(crate) fn vanilla_tab(rt: &mut Rt, id: &str) -> Option<V> {
    let path = id.strip_prefix("minecraft:")?;
    VANILLA_TABS
        .iter()
        .any(|t| t.eq_ignore_ascii_case(path))
        .then(|| rt.intern(format!("vanilla-tab/{path}"), |rt| rt.new_object(TAB)))
}

/// Static fields this module owns: `TabVisibility` and vanilla's tab keys.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match owner {
        VISIBILITY => VISIBILITIES.iter().position(|n| *n == field).map(|i| visibility(rt, i)),
        "net/minecraft/world/item/CreativeModeTabs" if VANILLA_TABS.contains(&field) => Some(rt.string(&format!("creative_mode_tab#minecraft:{}", field.to_ascii_lowercase()))),
        // The feature flags a vanilla world has on: the one set this runtime keeps (everything the
        // game has is enabled; `contains` answers yes).
        "net/minecraft/world/flag/FeatureFlags" if matches!(field, "VANILLA_SET" | "DEFAULT_FLAGS") => {
            Some(rt.intern("compat:feature-flags".into(), |rt| rt.new_object("net/minecraft/world/flag/FeatureFlagSet")))
        }
        // `Item.BY_BLOCK`, read through any item class (`BlockItem.BY_BLOCK` is `Item`'s).
        o if field == "BY_BLOCK" && o.starts_with("net/minecraft/world/item/") => {
            Some(by_block(rt))
        }
        // The registry of flags (`FeatureFlags.REGISTRY.allFlags()`): every flag is on, so its set
        // is that one set.
        "net/minecraft/world/flag/FeatureFlags" if field == "REGISTRY" => {
            Some(rt.intern("compat:feature-flag-registry".into(), |rt| rt.new_object("net/minecraft/world/flag/FeatureFlagRegistry")))
        }
        _ => None,
    }
}

/// Keeps an accepted stack's item on `out` (an `Output`, or the contents event), unless it is
/// for the search tab only.
fn accept(rt: &mut Rt, out: V, thing: V, shown: V) {
    if rt.get_field(shown, "$ordinal").i() == 2 {
        return;
    }
    let item = if rt.class_of(thing) == "net/minecraft/world/item/ItemStack" {
        let i = rt.get_field(thing, "$item");
        if rt.get_field(thing, "$count").i() <= 0 { String::new() } else { item_name(rt, i) }
    } else {
        let i = as_item(rt, thing);
        item_name(rt, i)
    };
    if item.is_empty() || item == "minecraft:air" {
        return;
    }
    let list = rt.get_field(out, "$items");
    let mut items = crate::jutil::elements(rt, list);
    let v = rt.string(&item);
    if !items.contains(&v) {
        items.push(v);
        if let Some(o) = rt.heap.get_mut(list.r() as usize) {
            o.data = crate::Data::List(items);
        }
    }
}

/// `Output.accept(stack, visibility)`, the one method an `Output` implements itself.
const OUTPUT_ACCEPT: &str = "net/minecraft/world/item/CreativeModeTab$Output.accept.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/CreativeModeTab$TabVisibility;)V";

fn item_name(rt: &Rt, item: V) -> String {
    rt.str_of(item).map(str::to_owned).or_else(|| rt.registered_name(item, "item").map(str::to_owned)).unwrap_or_default()
}

/// `itemLike.asItem()` as Java dispatches it: the mod's own method when its class has one
/// (Registrate's entries, a block naming its item), else the API's ([`api_as_item`]).
pub fn as_item(rt: &mut Rt, v: V) -> V {
    const AS_ITEM: &str = "asItem.()Lnet/minecraft/world/item/Item;";
    let class = rt.class_of(v);
    if rt.overrides.is_some_and(|o| o(class, AS_ITEM))
        && let Some(vc) = rt.vcall
    {
        return vc(rt, "net/minecraft/world/level/ItemLike.asItem.()Lnet/minecraft/world/item/Item;", AS_ITEM, &[v]);
    }
    api_as_item(rt, v)
}

/// `Item.BY_BLOCK`: the item each block gives (`Block.asItem()`). One map, as vanilla keeps it,
/// which the mods read and write too: Create's schematicannon asks it for the item a block costs,
/// Puzzles Lib and Moonlight give a block of theirs another block's item through it.
const BY_BLOCK: &str = "compat:by-block";
/// How many registrations [`BY_BLOCK`] was last brought up to.
const BY_BLOCK_COUNT: &str = "compat:by-block-count";
/// Set on a block item once it was told to add itself to [`BY_BLOCK`].
const MAPPED: &str = "$byBlock";
const REGISTER_BLOCKS: &str = "registerBlocks.(Ljava/util/Map;Lnet/minecraft/world/item/Item;)V";
const GET_BLOCK: &str = "getBlock.()Lnet/minecraft/world/level/block/Block;";

/// [`BY_BLOCK`], with every block item registered so far in it. Each adds itself once, as the
/// registry has it do when it is registered (`BlockItem.registerBlocks(map, item)`, which a mod's
/// class overrides to name more blocks: a wall variant, a double block that has no item of its
/// own). The registered items are looked through again only after more were registered: `asItem`
/// runs for every `ItemStack.is(block)` a mod's tick asks (Create's placement helpers, each tick).
pub(crate) fn by_block(rt: &mut Rt) -> V {
    let map = match rt.interned.get(BY_BLOCK) {
        Some(&m) => m,
        None => {
            let m = rt.alloc("java/util/HashMap", Data::Table(Default::default()));
            rt.interned.insert(BY_BLOCK.into(), m);
            m
        }
    };
    let count = rt.registered.len() as i32;
    if rt
        .interned
        .get(BY_BLOCK_COUNT)
        .is_some_and(|c| c.i() == count)
    {
        return map;
    }
    rt.interned.insert(BY_BLOCK_COUNT.into(), V::I(count));
    let mut new: Vec<(bool, String, V)> = Vec::new();
    for ((registry, name), &item) in &rt.registered {
        if registry != "item" {
            continue;
        }
        let block = rt.get_field(item, "$block");
        if block.r() != 0 && rt.get_field(item, MAPPED).i() == 0 {
            new.push((
                rt.registered_name(block, "block") == Some(name.as_str()),
                name.clone(),
                item,
            ));
        }
    }
    // The registry's order is not kept: of the items made for one block, the one named as the
    // block is added last and so is the block's, else the first by name.
    new.sort_by(|x, y| x.0.cmp(&y.0).then_with(|| y.1.cmp(&x.1)));
    // (Marked first: an item's own `registerBlocks` may ask for a block's item.)
    for (_, _, item) in &new {
        rt.put_field(*item, MAPPED, V::I(1));
    }
    for (_, _, item) in new {
        let class = rt.class_of(item);
        match rt.vcall {
            Some(vc) if rt.overrides.is_some_and(|o| o(class, REGISTER_BLOCKS)) => {
                vc(
                    rt,
                    "net/minecraft/world/item/BlockItem.registerBlocks.(Ljava/util/Map;Lnet/minecraft/world/item/Item;)V",
                    REGISTER_BLOCKS,
                    &[item, map, item],
                );
                rt.report_thrown("BlockItem.registerBlocks");
            }
            _ => register_blocks(rt, item, map, item),
        }
    }
    map
}

/// A block item's block: `getBlock()`, the mod's own where its class has one.
fn block_of(rt: &mut Rt, item: V) -> V {
    let class = rt.class_of(item);
    match rt.vcall {
        Some(vc) if rt.overrides.is_some_and(|o| o(class, GET_BLOCK)) => vc(
            rt,
            "net/minecraft/world/item/BlockItem.getBlock.()Lnet/minecraft/world/level/block/Block;",
            GET_BLOCK,
            &[item],
        ),
        _ => rt.get_field(item, "$block"),
    }
}

/// `BlockItem.registerBlocks(map, item)`: `map.put(getBlock(), item)`. An item named as its block
/// stays that block's item when another made for the same block comes later.
fn register_blocks(rt: &mut Rt, this: V, map: V, item: V) {
    let block = block_of(rt, this);
    if block.r() == 0 || map.r() == 0 {
        return;
    }
    let named_as = |rt: &Rt, i: V| {
        rt.registered_name(i, "item")
            .is_some_and(|n| rt.registered_name(block, "block") == Some(n))
    };
    if let Some(had) = crate::jutil::table_get(rt, map, block)
        && had != item
        && named_as(rt, had)
        && !named_as(rt, item)
    {
        return;
    }
    crate::jutil::table_put(rt, map, block, item);
}

/// What [`BY_BLOCK`] gives for one of the game's own blocks it doesn't hold yet: the game's item
/// of the block's name, where that item places a block. The map learns the game's block items as
/// they are asked for, the game's blocks being made here only as a mod names them.
pub(crate) fn game_block_item(rt: &mut Rt, map: V, block: V) -> Option<V> {
    if rt.interned.get(BY_BLOCK) != Some(&map) {
        return None;
    }
    let name = rt
        .str_of(block)
        .filter(|n| n.starts_with("minecraft:"))?
        .to_owned();
    if !rt.host().is_some_and(|h| h.item_is_block(&name)) {
        return None;
    }
    let item = crate::api_common::item(rt, &name);
    crate::jutil::table_put(rt, map, block, item);
    Some(item)
}

/// The API's `asItem()`: an item is its own; a block's is the block item registered for it
/// (`Item.BY_BLOCK`), else one of its name, else air; a registry holder's (NeoForge's
/// `DeferredHolder` and the mod's subclasses of it) is its value's.
pub fn api_as_item(rt: &mut Rt, v: V) -> V {
    let class = rt.class_of(v);
    if class.starts_with("net/neoforged/neoforge/registries/Deferred") || rt.is_a(class, "net/neoforged/neoforge/registries/DeferredHolder") {
        let value = api::call(rt, "net/neoforged/neoforge/registries/DeferredHolder.get.()Ljava/lang/Object;", &[v]);
        return if value.r() != 0 && value != v { as_item(rt, value) } else { crate::api_common::item(rt, "minecraft:air") };
    }
    if rt.is_a(class, "net/minecraft/world/item/Item") {
        return v;
    }
    // The item the block has in `Item.BY_BLOCK`: the block item made for it (`new BlockItem(block,
    // properties)`), by any name, or what a mod put there.
    if v.r() != 0 {
        let map = by_block(rt);
        if let Some(i) = crate::jutil::table_get(rt, map, v) {
            return i;
        }
    }
    let block = match rt.str_of(v) {
        Some(n) => n.to_owned(),
        None => rt.registered_name(v, "block").map(str::to_owned).unwrap_or_default(),
    };
    if block.is_empty() {
        return crate::api_common::item(rt, "minecraft:air");
    }
    match rt.registered.get(&("item".to_owned(), block.clone())) {
        Some(&i) => i,
        None if block.starts_with("minecraft:") => crate::api_common::item(rt, &block),
        None => crate::api_common::item(rt, "minecraft:air"),
    }
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    matches!(
        owner,
        TAB | BUILDER | OUTPUT | CONTENTS_EVENT | PARAMETERS | "net/minecraft/world/flag/FeatureFlagSet" | "net/minecraft/world/flag/FeatureFlagRegistry" | "net/minecraft/world/item/BlockItem"
    ) || name == "asItem"
        || (matches!(name, "registerBlocks" | "removeFromBlockToItemMap")
            && owner.starts_with("net/minecraft/world/item/"))
        || (name == "enabledFeatures" && desc == FLAGS && owner.starts_with("net/minecraft/"))
}

/// `enabledFeatures()`'s descriptor.
const FLAGS: &str = "()Lnet/minecraft/world/flag/FeatureFlagSet;";

/// A new tab builder's state: the tabs it comes before and after, none yet.
fn new_builder(rt: &mut Rt, b: V) {
    for f in ["$before", "$after"] {
        let l = crate::jutil::new_list(rt, Vec::new());
        rt.put_field(b, f, l);
    }
}

/// A creative tab call, or None when this module doesn't model it.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let (owner, name, desc) = crate::parts(key)?;
    Some(match (owner, name) {
        (TAB, "builder") => {
            let b = rt.new_object(BUILDER);
            new_builder(rt, b);
            b
        }
        (BUILDER, n) => {
            let this = a(0);
            match n {
                // `new CreativeModeTab.Builder(row, column)` (`CreativeModeTab.builder(row, column)`
                // reaches vanilla's tabs; a mod's goes where NeoForge sorts it).
                "<init>" => {
                    new_builder(rt, this);
                    return Some(V::Void);
                }
                "title" => rt.put_field(this, "$title", a(1)),
                "icon" => rt.put_field(this, "$icon", a(1)),
                "displayItems" => rt.put_field(this, "$display", a(1)),
                "withTabsBefore" | "withTabsAfter" => {
                    let list = rt.get_field(this, if n == "withTabsBefore" { "$before" } else { "$after" });
                    let mut keys = crate::jutil::elements(rt, list);
                    keys.extend(crate::jutil::elements(rt, a(1)));
                    if let Some(o) = rt.heap.get_mut(list.r() as usize) {
                        o.data = crate::Data::List(keys);
                    }
                }
                "build" => {
                    let t = rt.new_object(TAB);
                    for f in ["$title", "$icon", "$display", "$before", "$after"] {
                        let v = rt.get_field(this, f);
                        rt.put_field(t, f, v);
                    }
                    return Some(t);
                }
                // The rest only change how the game draws the tab (`hideTitle`, `withSearchBar`...).
                _ if desc.ends_with(&format!(")L{BUILDER};")) => {}
                _ => return None,
            }
            this
        }
        (TAB, "getDisplayName") => rt.get_field(a(0), "$title"),
        // The stacks a tab shows, which it holds none of until the game builds its tabs' contents
        // (as in the game before the creative screen first opens): a mod orders its items by a
        // vanilla tab's (SecurityCraft's `getVanillaOrderedItems`).
        (TAB, "getDisplayItems" | "getSearchTabDisplayItems") => rt.alloc(
            "it/unimi/dsi/fastutil/objects/ObjectLinkedOpenCustomHashSet",
            crate::Data::List(Vec::new()),
        ),
        (TAB, "getIconItem") => {
            let icon = rt.get_field(a(0), "$icon");
            rt.call_functional(SUPPLY, icon, &[])
        }
        // A mod's own `Output` (Registrate's `CreativeModeTabModifier`): the interface's default
        // methods, each a stack and a visibility for its own `accept(stack, visibility)`.
        (OUTPUT, "accept" | "acceptAll") if rt.is_mod_class(rt.class_of(a(0))) && !OUTPUT_ACCEPT.ends_with(desc) => {
            let shown = if desc.contains("TabVisibility;") { a(2) } else { visibility(rt, 0) };
            let things = if name == "acceptAll" { crate::jutil::elements(rt, a(1)) } else { vec![a(1)] };
            for thing in things {
                let stack = if rt.class_of(thing) == "net/minecraft/world/item/ItemStack" {
                    thing
                } else {
                    let s = rt.new_object("net/minecraft/world/item/ItemStack");
                    crate::api::call(rt, "net/minecraft/world/item/ItemStack.<init>.(Lnet/minecraft/world/level/ItemLike;)V", &[s, thing]);
                    s
                };
                crate::menus::virt(rt, OUTPUT_ACCEPT, a(0), &[stack, shown]);
            }
            V::Void
        }
        (OUTPUT | CONTENTS_EVENT, "accept") => {
            let shown = if desc.contains("TabVisibility;") { a(2) } else { visibility(rt, 0) };
            accept(rt, a(0), a(1), shown);
            V::Void
        }
        (OUTPUT | CONTENTS_EVENT, "acceptAll") => {
            let shown = if desc.contains("TabVisibility;") { a(2) } else { visibility(rt, 0) };
            for s in crate::jutil::elements(rt, a(1)) {
                accept(rt, a(0), s, shown);
            }
            V::Void
        }
        // The stacks the tab holds so far, in order (NeoForge's ordered sets): the tab's own and
        // the search tab's, which holds the same here.
        (CONTENTS_EVENT, "getParentEntries" | "getSearchEntries") => {
            let list = rt.get_field(a(0), "$items");
            let names: Vec<String> = crate::jutil::elements(rt, list).into_iter().filter_map(|v| rt.str_of(v).map(str::to_owned)).collect();
            let mut stacks = Vec::with_capacity(names.len());
            for n in names {
                let item = crate::api_common::item(rt, &n);
                let s = rt.new_object("net/minecraft/world/item/ItemStack");
                crate::api::call(rt, "net/minecraft/world/item/ItemStack.<init>.(Lnet/minecraft/world/level/ItemLike;)V", &[s, item]);
                stacks.push(s);
            }
            rt.alloc("it/unimi/dsi/fastutil/objects/ObjectLinkedOpenCustomHashSet", crate::Data::List(stacks))
        }
        // A mod's tab is a category of items (not the hotbar's, the search's or the inventory's).
        (TAB, "getType") => crate::api_common::plain_enum(rt, "net/minecraft/world/item/CreativeModeTab$Type", "CATEGORY")?,
        (CONTENTS_EVENT, "getTabKey") => rt.get_field(a(0), "$key"),
        (CONTENTS_EVENT, "getTab") => rt.get_field(a(0), "$tab"),
        (CONTENTS_EVENT, "getParameters") => rt.get_field(a(0), "$parameters"),
        (CONTENTS_EVENT | PARAMETERS, "hasPermissions") => V::I(0),
        // The registries a tab's generator looks its entries up in (`parameters.holders()`:
        // AE2's planes ask it for the enchantments, Ars Nouveau for the paintings): the one
        // registry access there is.
        (PARAMETERS, "holders") => rt.intern("compat:registry-access".into(), |rt| rt.new_object("net/minecraft/core/RegistryAccess")),
        (CONTENTS_EVENT | PARAMETERS, "getFlags" | "enabledFeatures") => rt.intern("compat:feature-flags".into(), |rt| rt.new_object("net/minecraft/world/flag/FeatureFlagSet")),
        ("net/minecraft/world/flag/FeatureFlagSet", "contains" | "isSubsetOf") => V::I(1),
        // `FeatureFlagSet.of(...)`: the one set of flags there is here (every feature on).
        ("net/minecraft/world/flag/FeatureFlagSet", "of") => rt
            .intern("compat:feature-flags".into(), |rt| {
                rt.new_object("net/minecraft/world/flag/FeatureFlagSet")
            }),
        // The features a level, a connection or a command source has on (`level.enabledFeatures()`):
        // that one set.
        (_, "enabledFeatures") if desc == FLAGS => rt.intern("compat:feature-flags".into(), |rt| rt.new_object("net/minecraft/world/flag/FeatureFlagSet")),
        ("net/minecraft/world/flag/FeatureFlagRegistry", "allFlags" | "subset" | "fromNames") => {
            rt.intern("compat:feature-flags".into(), |rt| rt.new_object("net/minecraft/world/flag/FeatureFlagSet"))
        }
        (_, "asItem") if desc == "()Lnet/minecraft/world/item/Item;" => api_as_item(rt, a(0)),
        // `BlockItem.registerBlocks(map, item)` and its undoing, as a mod's own class reaches them
        // (`super.registerBlocks(map, item)`).
        (o, "registerBlocks")
            if o.starts_with("net/minecraft/world/item/")
                && desc == "(Ljava/util/Map;Lnet/minecraft/world/item/Item;)V" =>
        {
            register_blocks(rt, a(0), a(1), a(2));
            V::Void
        }
        (o, "removeFromBlockToItemMap")
            if o.starts_with("net/minecraft/world/item/")
                && desc == "(Ljava/util/Map;Lnet/minecraft/world/item/Item;)V" =>
        {
            let block = block_of(rt, a(0));
            if block.r() != 0 {
                crate::jutil::table_remove(rt, a(1), block);
            }
            V::Void
        }
        ("net/minecraft/world/item/BlockItem", "getBlock") => match rt.str_of(a(0)).map(str::to_owned) {
            Some(n) => api::block(rt, &n),
            None => rt.get_field(a(0), "$block"),
        },
        _ => return None,
    })
}

/// Builds every tab the mod registered: runs each one's `displayItems` and posts its
/// `BuildCreativeModeTabContentsEvent`. Tabs come in the order their `withTabsBefore` and
/// `withTabsAfter` keys ask, then by name.
pub fn tabs(rt: &mut Rt) -> Vec<Tab> {
    let mut made: Vec<(String, V)> = rt.registered.iter().filter(|((r, _), _)| r == "creative_mode_tab").map(|((_, n), v)| (n.clone(), *v)).collect();
    made.sort_by(|a, b| a.0.cmp(&b.0));
    let order = ordered(rt, &made);
    let mut tabs = Vec::new();
    for (id, tab) in order {
        let params = rt.new_object(PARAMETERS);
        let out = rt.new_object(OUTPUT);
        let list = crate::jutil::new_list(rt, Vec::new());
        rt.put_field(out, "$items", list);
        let display = rt.get_field(tab, "$display");
        if display.r() != 0 {
            rt.call_functional(GENERATE, display, &[params, out]);
            rt.report_thrown("CreativeModeTab.displayItems");
        }
        // NeoForge's event for the same tab, accepting into the same list.
        let event = rt.new_object(CONTENTS_EVENT);
        let key = rt.string(&format!("creative_mode_tab#{id}"));
        rt.put_field(event, "$key", key);
        rt.put_field(event, "$tab", tab);
        rt.put_field(event, "$parameters", params);
        rt.put_field(event, "$items", list);
        crate::neo::post(rt, CONTENTS_EVENT, event);
        let items: Vec<String> = crate::jutil::elements(rt, list).into_iter().filter_map(|v| rt.str_of(v).map(str::to_owned)).collect();
        let title = rt.get_field(tab, "$title");
        let title = rt.str_of(rt.get_field(title, "$text")).map(str::to_owned).unwrap_or_else(|| id.clone());
        let icon_supplier = rt.get_field(tab, "$icon");
        let icon = if icon_supplier.r() != 0 {
            let s = rt.call_functional(SUPPLY, icon_supplier, &[]);
            rt.report_thrown("CreativeModeTab.icon");
            let i = rt.get_field(s, "$item");
            item_name(rt, i)
        } else {
            String::new()
        };
        let icon = if icon.is_empty() || icon == "minecraft:air" { items.first().cloned().unwrap_or_default() } else { icon };
        tabs.push(Tab { id, title, icon, items });
    }
    tabs
}

/// The tabs in the order their keys ask: a tab another lists in `withTabsBefore` comes first,
/// one it lists in `withTabsAfter` after it. Unordered ones keep their (name) order.
fn ordered(rt: &Rt, made: &[(String, V)]) -> Vec<(String, V)> {
    let keys = |t: V, f: &'static str| -> Vec<String> {
        let list = rt.get_field(t, f);
        crate::jutil::elements(rt, list).into_iter().map(|k| crate::neo::split_key(rt, k).1).collect()
    };
    // `before[i]` holds the tabs that must come before tab i.
    let before: Vec<Vec<usize>> = made
        .iter()
        .enumerate()
        .map(|(i, (_, t))| {
            let mut b: Vec<usize> = keys(*t, "$before").iter().filter_map(|k| made.iter().position(|(n, _)| n == k)).collect();
            for (j, (_, other)) in made.iter().enumerate() {
                if keys(*other, "$after").contains(&made[i].0) {
                    b.push(j);
                }
            }
            b.retain(|&j| j != i);
            b
        })
        .collect();
    let mut placed = vec![false; made.len()];
    let mut out = Vec::new();
    while out.len() < made.len() {
        // The first unplaced tab whose predecessors are placed; on a cycle, the first unplaced.
        let next = (0..made.len())
            .find(|&i| !placed[i] && before[i].iter().all(|&j| placed[j]))
            .or_else(|| (0..made.len()).find(|&i| !placed[i]))
            .unwrap_or(0);
        placed[next] = true;
        out.push(made[next].clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::call as c;

    #[test]
    fn a_mods_own_output_takes_the_interfaces_default_methods() {
        // Registrate's `CreativeModeTabModifier` implements `Output` itself: `accept(item)`, called
        // on its own class, and `acceptAll(stacks)` reach its `accept(stack, visibility)` with a
        // stack of the item and vanilla's default visibility, as Java's default methods do.
        fn vcall(rt: &mut Rt, _key: &'static str, short: &'static str, args: &[V]) -> V {
            if short == "accept.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/CreativeModeTab$TabVisibility;)V" {
                let item = rt.get_field(args[1], "$item");
                let shown = rt.get_field(args[2], "$name");
                let line = format!("{} {}", item_name(rt, item), rt.str_of(shown).unwrap_or("?"));
                let list = rt.get_field(args[0], "$got");
                let mut got = crate::jutil::elements(rt, list);
                got.push(rt.string(&line));
                if let Some(o) = rt.heap.get_mut(list.r() as usize) {
                    o.data = crate::Data::List(got);
                }
            }
            V::Void
        }
        crate::install();
        let mut rt = Rt::new();
        rt.vcall = Some(vcall);
        rt.supers.insert("t/Modifier", "java/lang/Object");
        rt.interfaces.insert("t/Modifier", &["net/minecraft/world/item/CreativeModeTab$Output"]);
        let gear = rt.new_object("net/minecraft/world/item/Item");
        rt.registered.insert(("item".into(), "t:gear".into()), gear);
        let modifier = rt.new_object("t/Modifier");
        let list = crate::jutil::new_list(&mut rt, Vec::new());
        rt.put_field(modifier, "$got", list);
        c(&mut rt, "t/Modifier.accept.(Lnet/minecraft/world/level/ItemLike;)V", &[modifier, gear]);
        let only_search = api::get_static(&mut rt, "net/minecraft/world/item/CreativeModeTab$TabVisibility.SEARCH_TAB_ONLY");
        c(&mut rt, "net/minecraft/world/item/CreativeModeTab$Output.accept.(Lnet/minecraft/world/level/ItemLike;Lnet/minecraft/world/item/CreativeModeTab$TabVisibility;)V", &[modifier, gear, only_search]);
        let stack = rt.new_object("net/minecraft/world/item/ItemStack");
        c(&mut rt, "net/minecraft/world/item/ItemStack.<init>.(Lnet/minecraft/world/level/ItemLike;)V", &[stack, gear]);
        let stacks = crate::jutil::new_list(&mut rt, vec![stack]);
        c(&mut rt, "net/minecraft/world/item/CreativeModeTab$Output.acceptAll.(Ljava/util/Collection;)V", &[modifier, stacks]);
        let got: Vec<String> = crate::jutil::elements(&rt, list).into_iter().filter_map(|v| rt.str_of(v).map(str::to_owned)).collect();
        assert_eq!(got, ["t:gear PARENT_AND_SEARCH_TABS", "t:gear SEARCH_TAB_ONLY", "t:gear PARENT_AND_SEARCH_TABS"]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `Item.BY_BLOCK` is one map: a block's item (`asItem()`) is what it holds for the block.
    /// The registered block items add themselves, a mod's own class naming more blocks than its
    /// own (FramedBlocks' item of a block and its wall variant; its ghost renderer wants a
    /// `BlockItem` from each); the mods read it and put into it (Puzzles Lib gives a diagonal
    /// fence its fence's item); and it knows the game's block items as they are asked for
    /// (Create's schematicannon asks what a block costs).
    #[test]
    fn a_blocks_item_is_the_one_the_block_item_map_holds() {
        const AS_ITEM: &str =
            "net/minecraft/world/level/block/Block.asItem.()Lnet/minecraft/world/item/Item;";
        const GET: &str = "java/util/Map.get.(Ljava/lang/Object;)Ljava/lang/Object;";
        fn overrides(class: &str, method: &str) -> bool {
            class == "t/StandingAndWallItem" && method == REGISTER_BLOCKS
        }
        // `super.registerBlocks(map, item); map.put(wallBlock, item);`
        fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
            if short != REGISTER_BLOCKS {
                return c(rt, key, args);
            }
            c(
                rt,
                "net/minecraft/world/item/BlockItem.registerBlocks.(Ljava/util/Map;Lnet/minecraft/world/item/Item;)V",
                args,
            );
            let wall = rt.get_field(args[0], "wallBlock");
            c(
                rt,
                "java/util/Map.put.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
                &[args[1], wall, args[2]],
            )
        }
        crate::install();
        let mut rt = Rt::new();
        rt.overrides = Some(overrides);
        rt.vcall = Some(vcall);
        rt.supers.insert(
            "t/StandingAndWallItem",
            "net/minecraft/world/item/BlockItem",
        );
        let block = |rt: &mut Rt, name: &str| {
            let b = rt.new_object("t/Block");
            rt.registered.insert(("block".into(), name.into()), b);
            b
        };
        let (torch, wall_torch, crop, diagonal) = (
            block(&mut rt, "t:torch"),
            block(&mut rt, "t:wall_torch"),
            block(&mut rt, "t:crop"),
            block(&mut rt, "t:diagonal"),
        );
        let item = |rt: &mut Rt, class: &'static str, name: &str, of: V| {
            let i = rt.new_object(class);
            rt.put_field(i, "$block", of);
            rt.registered.insert(("item".into(), name.into()), i);
            i
        };
        let torch_item = item(&mut rt, "t/StandingAndWallItem", "t:torch", torch);
        rt.put_field(torch_item, "wallBlock", wall_torch);
        // Two items made for one block: the one named as the block is the block's.
        let seeds = item(
            &mut rt,
            "net/minecraft/world/item/BlockItem",
            "t:a_seeds",
            crop,
        );
        let crop_item = item(
            &mut rt,
            "net/minecraft/world/item/BlockItem",
            "t:crop",
            crop,
        );
        assert_eq!(c(&mut rt, AS_ITEM, &[torch]), torch_item);
        assert_eq!(
            c(&mut rt, AS_ITEM, &[wall_torch]),
            torch_item,
            "the item's own registerBlocks names its wall block"
        );
        assert_eq!(c(&mut rt, AS_ITEM, &[crop]), crop_item);
        // A block with no item gives air, until a mod gives it one through the map.
        let air = crate::api_common::item(&mut rt, "minecraft:air");
        assert_eq!(c(&mut rt, AS_ITEM, &[diagonal]), air);
        let map = api::get_static(&mut rt, "net/minecraft/world/item/Item.BY_BLOCK");
        assert_eq!(
            api::get_static(&mut rt, "net/minecraft/world/item/BlockItem.BY_BLOCK"),
            map
        );
        assert_eq!(c(&mut rt, GET, &[map, crop]), crop_item);
        c(
            &mut rt,
            "java/util/Map.put.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            &[map, diagonal, seeds],
        );
        assert_eq!(c(&mut rt, AS_ITEM, &[diagonal]), seeds);
        // An item registered later joins the map; one taken out of it no longer is its block's.
        let lamp = block(&mut rt, "t:lamp");
        let lamp_item = item(
            &mut rt,
            "net/minecraft/world/item/BlockItem",
            "t:lamp",
            lamp,
        );
        assert_eq!(c(&mut rt, AS_ITEM, &[lamp]), lamp_item);
        assert_eq!(c(&mut rt, GET, &[map, lamp]), lamp_item);
        c(
            &mut rt,
            "net/minecraft/world/item/BlockItem.removeFromBlockToItemMap.(Ljava/util/Map;Lnet/minecraft/world/item/Item;)V",
            &[lamp_item, map, lamp_item],
        );
        assert_eq!(
            c(
                &mut rt,
                "java/util/Map.containsKey.(Ljava/lang/Object;)Z",
                &[map, lamp]
            ),
            V::I(0)
        );
        // The game's own blocks: the item of the block's name where that item places a block.
        let mut host = crate::host::MemoryHost {
            block_items: vec!["minecraft:stone".into()],
            ..Default::default()
        };
        rt.with_host(&mut host, |rt| {
            let (stone, water) = (api::block(rt, "minecraft:stone"), api::block(rt, "minecraft:water"));
            let map = api::get_static(rt, "net/minecraft/world/item/Item.BY_BLOCK");
            let stone_item = crate::api_common::item(rt, "minecraft:stone");
            assert_eq!(c(rt, "java/util/Map.containsKey.(Ljava/lang/Object;)Z", &[map, stone]), V::I(1));
            assert_eq!(c(rt, GET, &[map, stone]), stone_item);
            let air = crate::api_common::item(rt, "minecraft:air");
            assert_eq!(c(rt, "java/util/Map.getOrDefault.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;", &[map, water, air]), air);
            // Another map knows nothing of the game's blocks.
            let other = c(rt, "com/google/common/collect/Maps.newHashMap.()Ljava/util/HashMap;", &[]);
            assert_eq!(c(rt, GET, &[other, stone]), V::NULL);
        });
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn a_tab_holds_what_its_generator_and_the_contents_event_accept() {
        // The generator and the listener are the mod's lambdas: lambda 1 accepts two stacks (one
        // only for search) into its output, lambda 2 accepts a block into the event's tab, and
        // lambda 3 supplies the icon.
        fn run(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
            match id {
                1 => {
                    let out = args[2];
                    let gear = rt.registered[&("item".to_owned(), "t:gear".to_owned())];
                    let stack = rt.new_object("net/minecraft/world/item/ItemStack");
                    c(rt, "net/minecraft/world/item/ItemStack.<init>.(Lnet/minecraft/world/level/ItemLike;)V", &[stack, gear]);
                    c(rt, "net/minecraft/world/item/CreativeModeTab$Output.accept.(Lnet/minecraft/world/item/ItemStack;)V", &[out, stack]);
                    let hidden = api::get_static(rt, "net/minecraft/world/item/CreativeModeTab$TabVisibility.SEARCH_TAB_ONLY");
                    let secret = crate::api_common::item(rt, "minecraft:bedrock");
                    let s2 = rt.new_object("net/minecraft/world/item/ItemStack");
                    c(rt, "net/minecraft/world/item/ItemStack.<init>.(Lnet/minecraft/world/level/ItemLike;)V", &[s2, secret]);
                    c(rt, "net/minecraft/world/item/CreativeModeTab$Output.accept.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/item/CreativeModeTab$TabVisibility;)V", &[out, s2, hidden]);
                    Some(V::Void)
                }
                2 => {
                    let ev = args[1];
                    let key = c(rt, "net/neoforged/neoforge/event/BuildCreativeModeTabContentsEvent.getTabKey.()Lnet/minecraft/resources/ResourceKey;", &[ev]);
                    if rt.str_of(key) == Some("creative_mode_tab#t:main") {
                        let block = rt.registered[&("block".to_owned(), "t:machine".to_owned())];
                        c(rt, "net/neoforged/neoforge/event/BuildCreativeModeTabContentsEvent.accept.(Lnet/minecraft/world/level/ItemLike;)V", &[ev, block]);
                    }
                    Some(V::Void)
                }
                3 => {
                    let s = rt.new_object("net/minecraft/world/item/ItemStack");
                    let block = rt.registered[&("block".to_owned(), "t:machine".to_owned())];
                    c(rt, "net/minecraft/world/item/ItemStack.<init>.(Lnet/minecraft/world/level/ItemLike;)V", &[s, block]);
                    Some(s)
                }
                _ => None,
            }
        }
        let mut rt = Rt::new();
        rt.lambda_call = Some(run);
        // A block with its block item under another name, and a plain item.
        rt.class_ids.insert("t/Machine", 7);
        rt.supers.insert("t/Machine", "net/minecraft/world/level/block/Block");
        let machine = rt.new_object("t/Machine");
        rt.registered.insert(("block".into(), "t:machine".into()), machine);
        let block_item = rt.new_object("net/minecraft/world/item/BlockItem");
        let props = rt.new_object(crate::blocks::ITEM_PROPERTIES);
        c(&mut rt, "net/minecraft/world/item/BlockItem.<init>.(Lnet/minecraft/world/level/block/Block;Lnet/minecraft/world/item/Item$Properties;)V", &[block_item, machine, props]);
        rt.registered.insert(("item".into(), "t:machine_item".into()), block_item);
        let gear = rt.new_object("net/minecraft/world/item/Item");
        rt.registered.insert(("item".into(), "t:gear".into()), gear);
        assert_eq!(c(&mut rt, "net/minecraft/world/level/block/Block.asItem.()Lnet/minecraft/world/item/Item;", &[machine]), block_item);
        // Two tabs: `t:extra` asks to come after `t:main`, though its name sorts first.
        let tab = |rt: &mut Rt, title: &str, generator: Option<V>, icon: Option<V>, before: Option<&str>| {
            let b = c(rt, "net/minecraft/world/item/CreativeModeTab.builder.()Lnet/minecraft/world/item/CreativeModeTab$Builder;", &[]);
            let t = rt.string(title);
            let title = c(rt, "net/minecraft/network/chat/Component.translatable.(Ljava/lang/String;)Lnet/minecraft/network/chat/MutableComponent;", &[t]);
            c(rt, "net/minecraft/world/item/CreativeModeTab$Builder.title.(Lnet/minecraft/network/chat/Component;)Lnet/minecraft/world/item/CreativeModeTab$Builder;", &[b, title]);
            if let Some(g) = generator {
                c(rt, "net/minecraft/world/item/CreativeModeTab$Builder.displayItems.(Lnet/minecraft/world/item/CreativeModeTab$DisplayItemsGenerator;)Lnet/minecraft/world/item/CreativeModeTab$Builder;", &[b, g]);
            }
            if let Some(i) = icon {
                c(rt, "net/minecraft/world/item/CreativeModeTab$Builder.icon.(Ljava/util/function/Supplier;)Lnet/minecraft/world/item/CreativeModeTab$Builder;", &[b, i]);
            }
            if let Some(k) = before {
                let key = rt.string(k);
                let keys = rt.alloc("[Lnet/minecraft/resources/ResourceKey;", crate::Data::Array(vec![key]));
                c(rt, "net/minecraft/world/item/CreativeModeTab$Builder.withTabsBefore.([Lnet/minecraft/resources/ResourceKey;)Lnet/minecraft/world/item/CreativeModeTab$Builder;", &[b, keys]);
            }
            c(rt, "net/minecraft/world/item/CreativeModeTab$Builder.noScrollBar.()Lnet/minecraft/world/item/CreativeModeTab$Builder;", &[b]);
            c(rt, "net/minecraft/world/item/CreativeModeTab$Builder.build.()Lnet/minecraft/world/item/CreativeModeTab;", &[b])
        };
        let generator = rt.lambda_id("t/Tabs.lambda$0", 1, Vec::new());
        let icon = rt.lambda_id("t/Tabs.lambda$2", 3, Vec::new());
        let main = tab(&mut rt, "itemGroup.t.main", Some(generator), Some(icon), None);
        let extra = tab(&mut rt, "Extra", None, None, Some("creative_mode_tab#t:main"));
        rt.registered.insert(("creative_mode_tab".into(), "t:main".into()), main);
        rt.registered.insert(("creative_mode_tab".into(), "t:extra".into()), extra);
        let listener = rt.lambda_id("t/Tabs.lambda$1", 2, Vec::new());
        rt.listeners.push((CONTENTS_EVENT.to_owned(), listener));
        let tabs = tabs(&mut rt);
        assert_eq!(
            tabs,
            vec![
                Tab { id: "t:main".into(), title: "itemGroup.t.main".into(), icon: "t:machine_item".into(), items: vec!["t:gear".into(), "t:machine_item".into()] },
                Tab { id: "t:extra".into(), title: "Extra".into(), icon: String::new(), items: vec![] },
            ]
        );
    }

    /// `new CreativeModeTab.Builder(row, column)` (Aquaculture's tab) builds as `builder()`'s does.
    #[test]
    fn a_tab_built_from_its_constructor_keeps_its_title() {
        let mut rt = Rt::new();
        crate::install();
        let b = rt.new_object(BUILDER);
        crate::api::call(&mut rt, "net/minecraft/world/item/CreativeModeTab$Builder.<init>.(Lnet/minecraft/world/item/CreativeModeTab$Row;I)V", &[b, V::NULL, V::I(0)]);
        let title = rt.string("Aquaculture");
        crate::api::call(&mut rt, "net/minecraft/world/item/CreativeModeTab$Builder.title.(Lnet/minecraft/network/chat/Component;)Lnet/minecraft/world/item/CreativeModeTab$Builder;", &[b, title]);
        let tab = crate::api::call(&mut rt, "net/minecraft/world/item/CreativeModeTab$Builder.build.()Lnet/minecraft/world/item/CreativeModeTab;", &[b]);
        assert_eq!(crate::api::call(&mut rt, "net/minecraft/world/item/CreativeModeTab.getDisplayName.()Lnet/minecraft/network/chat/Component;", &[tab]), title);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
