//! Creative inventory tabs a mod registers (`CreativeModeTab.builder()...build()` in its
//! `creative_mode_tab` registry): their title, icon and items, from the mod's own code. The tab's
//! `displayItems` generator runs into an `Output` that keeps what it accepts, then NeoForge's
//! `BuildCreativeModeTabContentsEvent` lets the mod's listeners add more, as NeoForge builds a
//! tab's contents. The game shows them as a content mod's `creative_tab` files ([`tabs`]).
use crate::{api, Rt, V};

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

/// Static fields this module owns: `TabVisibility` and vanilla's tab keys.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match owner {
        VISIBILITY => VISIBILITIES.iter().position(|n| *n == field).map(|i| visibility(rt, i)),
        "net/minecraft/world/item/CreativeModeTabs" if VANILLA_TABS.contains(&field) => Some(rt.string(&format!("creative_mode_tab#minecraft:{}", field.to_ascii_lowercase()))),
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
    let block = match rt.str_of(v) {
        Some(n) => n.to_owned(),
        None => rt.registered_name(v, "block").map(str::to_owned).unwrap_or_default(),
    };
    if block.is_empty() {
        return crate::api_common::item(rt, "minecraft:air");
    }
    // The block item made for this block (`new BlockItem(block, properties)`), by any name.
    let made = rt
        .registered
        .iter()
        .filter(|((r, _), i)| r == "item" && rt.get_field(**i, "$block") == v)
        .map(|((_, n), i)| (n.clone(), *i))
        .min_by(|a, b| (a.0 != block).cmp(&(b.0 != block)).then(a.0.cmp(&b.0)));
    if let Some((_, i)) = made {
        return i;
    }
    match rt.registered.get(&("item".to_owned(), block.clone())) {
        Some(&i) => i,
        None if block.starts_with("minecraft:") => crate::api_common::item(rt, &block),
        None => crate::api_common::item(rt, "minecraft:air"),
    }
}

/// A creative tab call, or None when this module doesn't model it.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let (owner, rest) = key.split_once('.')?;
    let (name, desc) = rest.split_once('.')?;
    Some(match (owner, name) {
        (TAB, "builder") => {
            let b = rt.new_object(BUILDER);
            for f in ["$before", "$after"] {
                let l = crate::jutil::new_list(rt, Vec::new());
                rt.put_field(b, f, l);
            }
            b
        }
        (BUILDER, n) => {
            let this = a(0);
            match n {
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
        (TAB, "getIconItem") => {
            let icon = rt.get_field(a(0), "$icon");
            rt.call_functional(SUPPLY, icon, &[])
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
        (CONTENTS_EVENT, "getTabKey") => rt.get_field(a(0), "$key"),
        (CONTENTS_EVENT, "getTab") => rt.get_field(a(0), "$tab"),
        (CONTENTS_EVENT, "getParameters") => rt.get_field(a(0), "$parameters"),
        (CONTENTS_EVENT | PARAMETERS, "hasPermissions") => V::I(0),
        (CONTENTS_EVENT | PARAMETERS, "getFlags" | "enabledFeatures") => rt.intern("compat:feature-flags".into(), |rt| rt.new_object("net/minecraft/world/flag/FeatureFlagSet")),
        ("net/minecraft/world/flag/FeatureFlagSet", "contains" | "isSubsetOf") => V::I(1),
        (_, "asItem") if desc == "()Lnet/minecraft/world/item/Item;" => api_as_item(rt, a(0)),
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
}
