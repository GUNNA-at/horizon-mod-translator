//! The baked models a mod finds by location, as vanilla's `ModelManager` keeps them once the mods'
//! `ModelEvent.ModifyBakingResult` listeners ran: at a block state's location the state's model,
//! at an item's (`inventory`) the item's, at a `standalone` one the model of that name, each the
//! game's own unless a mod put another there (Create wraps its custom-rendered items' models, and
//! its lectern draws the controller from the model one wraps). Like vanilla's, the registry has a
//! model at every location: the game's is put at one the first time a mod asks for it there.
//!
//! And what vanilla builds on it: `ModelResourceLocation` itself (its variant in lower case,
//! printed `id#variant`), `ModelManager.getModel`, `ItemModelShaper.getItemModel`,
//! `ItemRenderer.getModel` (the trident's and spyglass's in-hand models), `ModelEvent
//! .BakingCompleted`, and a model's faces drawn into a vertex consumer (`renderModelLists`,
//! `renderQuadList`: each face's `putBulkData`, as vanilla's).
use crate::jutil::{elements, table_get, table_put};
use crate::{Data, Rt, Table, V};

/// `ModelResourceLocation(id, variant)`, a record: [`crate::records`] compares and hashes it.
pub(crate) const LOCATION: &str = "net/minecraft/client/resources/model/ModelResourceLocation";
/// The registry's class: a map ([`crate::jutil`] answers it) that puts the game's model at a
/// location it lacks when asked for one there ([`at`]).
pub(crate) const REGISTRY: &str = "compat:BakedModelMap";
/// Where the runtime keeps its registry ([`registry`]).
const REGISTRY_KEY: &str = "compat:baked-models";
/// Set while the registry waits for the client's setup to post the model events.
const BAKING_PENDING: &str = "compat:baking-pending";
/// Where the runtime keeps an item's model by its id ([`item_model`]).
const ITEM_MODEL_KEY: &str = "compat:item-model/";
const BAKED_MODEL: &str = "net/minecraft/client/resources/model/BakedModel";
const MODEL_MANAGER: &str = "net/minecraft/client/resources/model/ModelManager";
const ITEM_MODEL_SHAPER: &str = "net/minecraft/client/renderer/ItemModelShaper";
const BLOCK_MODEL_SHAPER: &str = "net/minecraft/client/renderer/block/BlockModelShaper";
const ITEM_RENDERER: &str = "net/minecraft/client/renderer/entity/ItemRenderer";
const MODIFY_BAKING_RESULT: &str =
    "net/neoforged/neoforge/client/event/ModelEvent$ModifyBakingResult";
const BAKING_COMPLETED: &str = "net/neoforged/neoforge/client/event/ModelEvent$BakingCompleted";
/// What `getMissingModel` names: vanilla's missing model.
const MISSING: &str = "minecraft:builtin/missing";
const LAST: &str =
    "com/mojang/blaze3d/vertex/PoseStack.last.()Lcom/mojang/blaze3d/vertex/PoseStack$Pose;";
/// NeoForge's `putBulkData(pose, quad, red, green, blue, alpha, light, overlay, readExistingColor)`,
/// what its `renderQuadList` calls.
const PUT_BULK: &str = "putBulkData.(Lcom/mojang/blaze3d/vertex/PoseStack$Pose;Lnet/minecraft/client/renderer/block/model/BakedQuad;FFFFIIZ)V";
const PUT_BULK_KEY: &str = "com/mojang/blaze3d/vertex/VertexConsumer.putBulkData.(Lcom/mojang/blaze3d/vertex/PoseStack$Pose;Lnet/minecraft/client/renderer/block/model/BakedQuad;FFFFIIZ)V";
const GET_QUADS: &str = "getQuads.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;Lnet/minecraft/util/RandomSource;)Ljava/util/List;";
const GET_QUADS_KEY: &str = "net/minecraft/client/resources/model/BakedModel.getQuads.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/core/Direction;Lnet/minecraft/util/RandomSource;)Ljava/util/List;";
const RANDOM: &str = "net/minecraft/util/RandomSource.create.()Lnet/minecraft/util/RandomSource;";
const SET_SEED: &str = "net/minecraft/util/RandomSource.setSeed.(J)V";
const PARTICLE_ICON: &str = "net/minecraft/client/resources/model/BakedModel.getParticleIcon.(Lnet/neoforged/neoforge/client/model/data/ModelData;)Lnet/minecraft/client/renderer/texture/TextureAtlasSprite;";
const MODEL_DATA_OF: &str = "net/minecraft/client/resources/model/BakedModel.getModelData.(Lnet/minecraft/world/level/BlockAndTintGetter;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/neoforged/neoforge/client/model/data/ModelData;)Lnet/neoforged/neoforge/client/model/data/ModelData;";
const LEVEL_MODEL_DATA: &str = "net/minecraft/world/level/Level.getModelData.(Lnet/minecraft/core/BlockPos;)Lnet/neoforged/neoforge/client/model/data/ModelData;";
const MODEL_DATA_EMPTY: &str = "net/neoforged/neoforge/client/model/data/ModelData.EMPTY";

/// The keys [`call`] may answer, by owner, name and descriptor alone.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(
        owner,
        LOCATION | MODEL_MANAGER | ITEM_MODEL_SHAPER | BAKING_COMPLETED
    ) || (owner == MODIFY_BAKING_RESULT && name == "getModels")
        || (owner == BLOCK_MODEL_SHAPER
            && matches!(
                name,
                "stateToModelLocation"
                    | "getModelManager"
                    | "getBlockModel"
                    | "getParticleIcon"
                    | "getTexture"
            ))
        || (owner == ITEM_RENDERER
            && matches!(
                name,
                "getModel" | "getItemModelShaper" | "renderModelLists" | "renderQuadList"
            ))
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        // The canonical constructor keeps the variant in lower case, as vanilla's.
        (LOCATION, "<init>") => {
            let variant = lower(rt, a(2));
            rt.put_field(a(0), "id", a(1));
            rt.put_field(a(0), "variant", variant);
            V::Void
        }
        (LOCATION, "inventory" | "standalone") => {
            let variant = rt.string(name);
            crate::records::make(rt, LOCATION, &[a(0), variant])
        }
        (LOCATION, "vanilla") => {
            let id = format!("minecraft:{}", rt.str_of(a(0)).unwrap_or(""));
            let id = rt.string(&id);
            let variant = lower(rt, a(1));
            crate::records::make(rt, LOCATION, &[id, variant])
        }
        (LOCATION, "getVariant") => rt.get_field(a(0), "variant"),
        (LOCATION, "toString") => {
            let text = location_text(rt, a(0));
            rt.string(&text)
        }
        (MODEL_MANAGER, "getModel") => {
            let models = registry(rt);
            at(rt, models, a(1))
        }
        (MODEL_MANAGER, "getMissingModel") => missing(rt),
        (MODEL_MANAGER, "getBlockModelShaper") => single(rt, BLOCK_MODEL_SHAPER),
        (ITEM_MODEL_SHAPER | BLOCK_MODEL_SHAPER, "getModelManager") => single(rt, MODEL_MANAGER),
        // `getItemModel(stack)` or `getItemModel(item)`.
        (ITEM_MODEL_SHAPER, "getItemModel") => {
            let item = match desc.starts_with("(Lnet/minecraft/world/item/ItemStack;") {
                true => rt.get_field(a(1), "$item"),
                false => a(1),
            };
            let item = crate::api_common::item_like_name(rt, item);
            item_model(rt, &item)
        }
        (ITEM_RENDERER, "getItemModelShaper") => single(rt, ITEM_MODEL_SHAPER),
        // `getModel(stack, level, entity, seed)`: the item's model, the trident's and spyglass's
        // in hand theirs. An item model's overrides (a bow drawn back) aren't the game's here: the
        // model itself.
        (ITEM_RENDERER, "getModel") => {
            let item = crate::api_common::item_like_name(rt, rt.get_field(a(1), "$item"));
            match item.as_str() {
                "minecraft:trident" => item_model(rt, "minecraft:trident_in_hand"),
                "minecraft:spyglass" => item_model(rt, "minecraft:spyglass_in_hand"),
                _ => item_model(rt, &item),
            }
        }
        // `renderModelLists(model, stack, light, overlay, poseStack, consumer)`.
        (ITEM_RENDERER, "renderModelLists") => {
            render_model_lists(rt, a(1), a(2), a(3).i(), a(4).i(), a(5), a(6));
            V::Void
        }
        // `renderQuadList(poseStack, consumer, quads, stack, light, overlay)`.
        (ITEM_RENDERER, "renderQuadList") => {
            let quads = elements(rt, a(3));
            render_quads(rt, a(1), a(2), &quads, a(4), a(5).i(), a(6).i());
            V::Void
        }
        // A block state's location: `stateToModelLocation(state)` or `(id, state)`.
        (BLOCK_MODEL_SHAPER, "stateToModelLocation") => {
            let (id, state) = match desc.starts_with("(Lnet/minecraft/resources/ResourceLocation;")
            {
                true => (rt.str_of(a(0)).map(str::to_owned), a(1)),
                false => (None, a(0)),
            };
            let text = rt.str_of(state).unwrap_or("").to_owned();
            let (block, variant) = state_variant(&text);
            let id = id.unwrap_or_else(|| block.to_owned());
            location(rt, &id, &variant)
        }
        (BLOCK_MODEL_SHAPER, "getBlockModel") => block_model(rt, a(1)),
        // The sprite a state's blocks break into: its model's, with no model data.
        (BLOCK_MODEL_SHAPER, "getParticleIcon") => {
            let model = block_model(rt, a(1));
            let empty = crate::api::get_static(rt, MODEL_DATA_EMPTY);
            crate::menus::virt(rt, PARTICLE_ICON, model, &[empty])
        }
        // `getTexture(state, level, pos)`: its model's, given the model data at `pos` as the model
        // takes it (a copycat's, its material's).
        (BLOCK_MODEL_SHAPER, "getTexture") => {
            let (state, level, pos) = (a(1), a(2), a(3));
            let data = crate::menus::virt(rt, LEVEL_MODEL_DATA, level, &[pos]);
            let model = block_model(rt, state);
            let data = crate::menus::virt(rt, MODEL_DATA_OF, model, &[level, pos, state, data]);
            crate::menus::virt(rt, PARTICLE_ICON, model, &[data])
        }
        (MODIFY_BAKING_RESULT | BAKING_COMPLETED, "getModels") => rt.get_field(a(0), "$models"),
        (BAKING_COMPLETED, "getModelManager") => single(rt, MODEL_MANAGER),
        _ => return None,
    })
}

/// `variant.toLowerCase(Locale.ROOT)`; null stays null.
fn lower(rt: &mut Rt, variant: V) -> V {
    match rt.str_of(variant).map(str::to_lowercase) {
        Some(v) => rt.string(&v),
        None => variant,
    }
}

/// `id#variant`, as a location prints itself.
fn location_text(rt: &Rt, location: V) -> String {
    let part = |field| {
        rt.str_of(rt.get_field(location, field))
            .unwrap_or("null")
            .to_owned()
    };
    format!("{}#{}", part("id"), part("variant"))
}

/// The client's one `class` (its model manager, its item model shaper), as `crate::render`
/// hands them out.
fn single(rt: &mut Rt, class: &'static str) -> V {
    rt.intern(format!("client/{class}"), |rt| rt.new_object(class))
}

/// The client's block model shaper (`BlockRenderDispatcher.getBlockModelShaper`).
pub(crate) fn model_shaper(rt: &mut Rt) -> V {
    single(rt, BLOCK_MODEL_SHAPER)
}

/// `BlockModelShaper.getBlockModel(state)`: the model at the state's location (a mod's where it put
/// one there); the missing model for none.
fn block_model(rt: &mut Rt, state: V) -> V {
    let Some(text) = rt.str_of(state).map(str::to_owned) else {
        return missing(rt);
    };
    let (block, variant) = state_variant(&text);
    let block = block.to_owned();
    let location = location(rt, &block, &variant);
    let models = registry(rt);
    at(rt, models, location)
}

/// `new ModelResourceLocation(id, variant)`.
pub(crate) fn location(rt: &mut Rt, id: &str, variant: &str) -> V {
    let id = rt.string(id);
    let variant = rt.string(&variant.to_lowercase());
    crate::records::make(rt, LOCATION, &[id, variant])
}

/// A block state's name and the variant of its location: its properties by name, as
/// `BlockModelShaper.statePropertiesToString` writes them (`axis=x,waterlogged=false`).
pub(crate) fn state_variant(state: &str) -> (&str, String) {
    let (block, props) = crate::host::split_state(state);
    let mut props: Vec<String> = props.iter().map(|(k, v)| format!("{k}={v}")).collect();
    props.sort();
    (block, props.join(","))
}

/// `(registry, id)` in the runtime's registries: whether the mod registered it.
fn registered(rt: &Rt, registry: &str, id: &str) -> bool {
    rt.registered
        .contains_key(&(registry.to_owned(), id.to_owned()))
}

/// The registry, made the first time it's asked for: the mod's blocks (each at its default
/// state's location) and items at theirs. The mods' `ModifyBakingResult` listeners, then their
/// `BakingCompleted` ones, run on it as vanilla's model manager posts them: once the models are
/// baked, as the client's resources load after its setup. Made before then, it waits for it
/// ([`client_resources_loaded`]): Create keeps the items whose models it swaps by their client
/// extensions, which the setup registers, once, the first time it's asked.
pub(crate) fn registry(rt: &mut Rt) -> V {
    if let Some(&models) = rt.interned.get(REGISTRY_KEY) {
        return models;
    }
    let models = rt.alloc(REGISTRY, Data::Table(Table::default()));
    rt.interned.insert(REGISTRY_KEY.into(), models);
    let mut locations: Vec<(String, String)> = Vec::new();
    for ((kind, id), &v) in &rt.registered {
        match kind.as_str() {
            "block" if rt.class_of(v) != "net/minecraft/world/level/block/Block" => {
                let default = rt.str_of(rt.get_field(v, "$default")).unwrap_or("");
                let (_, variant) = state_variant(&format!("{id}[{default}]"));
                locations.push((id.clone(), variant));
            }
            "item" => locations.push((id.clone(), "inventory".into())),
            _ => {}
        }
    }
    locations.sort();
    for (id, variant) in locations {
        let key = location(rt, &id, &variant);
        at(rt, models, key);
    }
    if crate::neo::is_client(rt) {
        bake(rt, models);
    } else {
        rt.interned.insert(BAKING_PENDING.into(), V::I(1));
    }
    models
}

/// The client's resources loaded, its setup done: the model events of a registry made before
/// ([`registry`]), and the item models found in it before them are found again.
pub(crate) fn client_resources_loaded(rt: &mut Rt) {
    if rt.interned.remove(BAKING_PENDING).is_none() {
        return;
    }
    rt.interned.retain(|k, _| !k.starts_with(ITEM_MODEL_KEY));
    if let Some(&models) = rt.interned.get(REGISTRY_KEY) {
        bake(rt, models);
    }
}

/// The mods' model events on `models`: `ModifyBakingResult`, then `BakingCompleted`.
fn bake(rt: &mut Rt, models: V) {
    for event in [MODIFY_BAKING_RESULT, BAKING_COMPLETED] {
        let ev = rt.new_object(event);
        rt.put_field(ev, "$models", models);
        crate::neo::post(rt, event, ev);
    }
}

/// The model at `location` in `models`: the game's own put there first when no mod put one (a
/// key that isn't a location finds what's there, if anything).
pub(crate) fn at(rt: &mut Rt, models: V, location: V) -> V {
    if let Some(m) = table_get(rt, models, location) {
        return m;
    }
    if location.r() == 0 || rt.class_of(location) != LOCATION {
        return V::NULL;
    }
    let m = game_model(rt, location);
    table_put(rt, models, location, m);
    m
}

/// The game's model at `location`: an item's (`inventory`: its `item/` model), a model by its own
/// name (`standalone`), else a block state's. A mod block's is a probe too (`$probe`):
/// [`crate::glue::render_shape`] tells a model a mod put at its location from it.
fn game_model(rt: &mut Rt, location: V) -> V {
    let id = rt
        .str_of(rt.get_field(location, "id"))
        .unwrap_or("")
        .to_owned();
    let variant = rt
        .str_of(rt.get_field(location, "variant"))
        .unwrap_or("")
        .to_owned();
    let m = rt.new_object(BAKED_MODEL);
    match variant.as_str() {
        "inventory" => {
            let item = rt.string(&id);
            rt.put_field(m, "$item", item);
            let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", &id));
            let name = rt.string(&format!("{namespace}:item/{path}"));
            rt.put_field(m, "$name", name);
        }
        "standalone" => {
            let name = rt.string(&id);
            rt.put_field(m, "$name", name);
        }
        _ => {
            let text = if variant.is_empty() {
                id.clone()
            } else {
                format!("{id}[{variant}]")
            };
            let state = crate::api::state(rt, &text);
            rt.put_field(m, "$state", state);
            if registered(rt, "block", &id) {
                rt.put_field(m, "$probe", V::I(1));
            }
        }
    }
    m
}

/// `getMissingModel()`: vanilla's missing model, by its name.
fn missing(rt: &mut Rt) -> V {
    rt.intern("compat:missing-model".into(), |rt| {
        let m = rt.new_object(BAKED_MODEL);
        let name = rt.string(MISSING);
        rt.put_field(m, "$name", name);
        m
    })
}

/// The model at `item`'s location (`ItemModelShaper.getItemModel`), kept per item as vanilla's
/// shaper keeps it until the models reload.
fn item_model(rt: &mut Rt, item: &str) -> V {
    let key = format!("{ITEM_MODEL_KEY}{item}");
    if let Some(&m) = rt.interned.get(&key) {
        return m;
    }
    let models = registry(rt);
    let location = location(rt, item, "inventory");
    let m = at(rt, models, location);
    rt.interned.insert(key, m);
    m
}

/// `ItemRenderer.renderModelLists`: the model's faces on each side, then those on none, each list
/// drawn by [`render_quads`]; the random source seeded 42 before each, as vanilla's.
fn render_model_lists(
    rt: &mut Rt,
    model: V,
    stack: V,
    light: i32,
    overlay: i32,
    pose_stack: V,
    consumer: V,
) {
    let Some(vc) = rt.vcall else { return };
    let random = crate::api::call(rt, RANDOM, &[]);
    for side in (0..6).map(Some).chain([None]) {
        crate::api::call(rt, SET_SEED, &[random, V::J(42)]);
        let side = match side {
            Some(d) => crate::api_common::direction(rt, d),
            None => V::NULL,
        };
        let quads = vc(
            rt,
            GET_QUADS_KEY,
            GET_QUADS,
            &[model, V::NULL, side, random],
        );
        if rt.thrown.is_some() {
            return;
        }
        let quads = elements(rt, quads);
        render_quads(rt, pose_stack, consumer, &quads, stack, light, overlay);
    }
}

/// `ItemRenderer.renderQuadList`: each face put into the consumer under the stack's last pose
/// (`putBulkData`, its existing colours kept), in white: a tinted face's colour is the item's
/// colour handler's, which mods don't register here yet.
fn render_quads(
    rt: &mut Rt,
    pose_stack: V,
    consumer: V,
    quads: &[V],
    _stack: V,
    light: i32,
    overlay: i32,
) {
    let Some(vc) = rt.vcall else { return };
    let pose = crate::api::call(rt, LAST, &[pose_stack]);
    for &quad in quads {
        let args = [
            consumer,
            pose,
            quad,
            V::F(1.0),
            V::F(1.0),
            V::F(1.0),
            V::F(1.0),
            V::I(light),
            V::I(overlay),
            V::I(1),
        ];
        vc(rt, PUT_BULK_KEY, PUT_BULK, &args);
        if rt.thrown.is_some() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{MemoryHost, Model, Quad};

    const NEW: &str = "net/minecraft/client/resources/model/ModelResourceLocation.<init>.(Lnet/minecraft/resources/ResourceLocation;Ljava/lang/String;)V";
    const INVENTORY: &str = "net/minecraft/client/resources/model/ModelResourceLocation.inventory.(Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/client/resources/model/ModelResourceLocation;";
    const VANILLA: &str = "net/minecraft/client/resources/model/ModelResourceLocation.vanilla.(Ljava/lang/String;Ljava/lang/String;)Lnet/minecraft/client/resources/model/ModelResourceLocation;";
    const TO_STRING: &str =
        "net/minecraft/client/resources/model/ModelResourceLocation.toString.()Ljava/lang/String;";
    const ID: &str = "net/minecraft/client/resources/model/ModelResourceLocation.id.()Lnet/minecraft/resources/ResourceLocation;";
    const GET_MODEL: &str = "net/minecraft/client/resources/model/ModelManager.getModel.(Lnet/minecraft/client/resources/model/ModelResourceLocation;)Lnet/minecraft/client/resources/model/BakedModel;";
    const ITEM_GET_MODEL: &str = "net/minecraft/client/renderer/entity/ItemRenderer.getModel.(Lnet/minecraft/world/item/ItemStack;Lnet/minecraft/world/level/Level;Lnet/minecraft/world/entity/LivingEntity;I)Lnet/minecraft/client/resources/model/BakedModel;";
    const RENDER_LISTS: &str = "net/minecraft/client/renderer/entity/ItemRenderer.renderModelLists.(Lnet/minecraft/client/resources/model/BakedModel;Lnet/minecraft/world/item/ItemStack;IILcom/mojang/blaze3d/vertex/PoseStack;Lcom/mojang/blaze3d/vertex/VertexConsumer;)V";
    const MAP_GET: &str = "java/util/Map.get.(Ljava/lang/Object;)Ljava/lang/Object;";
    const MAP_PUT: &str =
        "java/util/Map.put.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
    const EQUALS: &str = "java/lang/Object.equals.(Ljava/lang/Object;)Z";
    const HASH: &str = "java/lang/Object.hashCode.()I";
    const GET_BUFFER: &str = "net/minecraft/client/renderer/MultiBufferSource.getBuffer.(Lnet/minecraft/client/renderer/RenderType;)Lcom/mojang/blaze3d/vertex/VertexConsumer;";
    const WRAPPER_NEW: &str = "net/neoforged/neoforge/client/model/BakedModelWrapper.<init>.(Lnet/minecraft/client/resources/model/BakedModel;)V";

    fn runtime() -> Rt {
        let mut rt = Rt::new();
        crate::install();
        rt.vcall = Some(|rt, key, _, args| crate::api::call(rt, key, args));
        rt
    }

    /// A face of `sprite` on the model's up side, culled by none.
    fn face(sprite: &str) -> Quad {
        Quad {
            positions: [
                [0.0, 1.0, 0.0],
                [0.0, 1.0, 1.0],
                [1.0, 1.0, 1.0],
                [1.0, 1.0, 0.0],
            ],
            uv: [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
            sprite: sprite.into(),
            facing: 1,
            cull: None,
            tint_index: -1,
            shade: true,
            light_emission: 0,
            colors: [u32::MAX; 4],
        }
    }

    /// A location keeps its variant in lower case and prints `id#variant`; `inventory(id)` is
    /// equal to `new ModelResourceLocation(id, "inventory")`, with the same hash; `vanilla(path,
    /// variant)` is in the `minecraft` namespace.
    #[test]
    fn a_location_is_vanillas_record() {
        let mut rt = runtime();
        let id = rt.string("t:shaft");
        let made = rt.new_object(LOCATION);
        let variant = rt.string("AXIS=X");
        crate::api::call(&mut rt, NEW, &[made, id, variant]);
        let text = crate::api::call(&mut rt, TO_STRING, &[made]);
        assert_eq!(rt.str_of(text), Some("t:shaft#axis=x"));

        let wand = rt.string("t:wand");
        let by_factory = crate::api::call(&mut rt, INVENTORY, &[wand]);
        let by_new = rt.new_object(LOCATION);
        let inventory = rt.string("inventory");
        crate::api::call(&mut rt, NEW, &[by_new, wand, inventory]);
        assert_eq!(
            crate::api::call(&mut rt, EQUALS, &[by_factory, by_new]),
            V::I(1)
        );
        let hashes = [by_factory, by_new].map(|v| crate::api::call(&mut rt, HASH, &[v]));
        assert_eq!(hashes[0], hashes[1]);

        let path = rt.string("trident_in_hand");
        let vanilla = crate::api::call(&mut rt, VANILLA, &[path, inventory]);
        let vanilla_id = crate::api::call(&mut rt, ID, &[vanilla]);
        assert_eq!(rt.str_of(vanilla_id), Some("minecraft:trident_in_hand"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `stateToModelLocation` names a state as `BlockModelShaper` does: its block, then its
    /// properties by name (`t:gearbox#` for one without any); given an id, under that id.
    #[test]
    fn a_states_location_lists_its_properties_by_name() {
        const OF_STATE: &str = "net/minecraft/client/renderer/block/BlockModelShaper.stateToModelLocation.(Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/client/resources/model/ModelResourceLocation;";
        const OF_ID: &str = "net/minecraft/client/renderer/block/BlockModelShaper.stateToModelLocation.(Lnet/minecraft/resources/ResourceLocation;Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/client/resources/model/ModelResourceLocation;";
        let mut rt = runtime();
        let shaft = crate::api::state(&mut rt, "t:shaft[waterlogged=false,axis=x]");
        let gearbox = crate::api::state(&mut rt, "t:gearbox");
        let other = rt.string("t:other");
        let named = [
            crate::api::call(&mut rt, OF_STATE, &[shaft]),
            crate::api::call(&mut rt, OF_STATE, &[gearbox]),
            crate::api::call(&mut rt, OF_ID, &[other, shaft]),
        ]
        .map(|l| {
            let text = crate::api::call(&mut rt, TO_STRING, &[l]);
            rt.str_of(text).unwrap_or("").to_owned()
        });
        assert_eq!(
            named,
            [
                "t:shaft#axis=x,waterlogged=false",
                "t:gearbox#",
                "t:other#axis=x,waterlogged=false"
            ]
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// At a location no mod put a model, the model manager has the game's: a block state's faces,
    /// an item's `item/` model's, a standalone model's by its name.
    #[test]
    fn the_registry_has_the_games_model_where_no_mod_put_one() {
        let mut rt = runtime();
        let mut host = MemoryHost::default();
        for (key, sprite) in [
            ("minecraft:stone", "minecraft:block/stone"),
            ("t:item/wand", "t:item/wand"),
            ("t:block/extra", "t:block/extra"),
        ] {
            host.models
                .insert(key.into(), (vec![face(sprite)], sprite.into()));
        }
        let manager = single(&mut rt, MODEL_MANAGER);
        let sprites = rt.with_host(&mut host, |rt| {
            let wand = rt.string("t:wand");
            let extra = rt.string("t:block/extra");
            let locations = [
                location(rt, "minecraft:stone", ""),
                crate::api::call(rt, INVENTORY, &[wand]),
                crate::api::call(rt, "net/minecraft/client/resources/model/ModelResourceLocation.standalone.(Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/client/resources/model/ModelResourceLocation;", &[extra]),
            ];
            locations.map(|l| {
                let model = crate::api::call(rt, GET_MODEL, &[manager, l]);
                let quads = crate::api::call(rt, GET_QUADS_KEY, &[model, V::NULL, V::NULL, V::NULL]);
                let quads = elements(rt, quads);
                quads.iter().map(|&q| crate::quads::read_quad(rt, q, None).map(|q| q.sprite)).collect::<Vec<_>>()
            })
        });
        assert_eq!(
            sprites,
            [
                vec![Some("minecraft:block/stone".to_owned())],
                vec![Some("t:item/wand".to_owned())],
                vec![Some("t:block/extra".to_owned())],
            ]
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Create's custom-rendered items: a listener wraps the model at the item's location
    /// (`swapModels`: `put(location, factory.apply(get(location)))`). The item renderer then gives
    /// the wrapper for the item's stacks, and the model it wraps is the item's own: drawn with
    /// `renderModelLists`, its face goes into the consumer under the stack's pose.
    #[test]
    fn an_item_model_a_mod_wrapped_is_the_one_the_item_renderer_gives() {
        let mut rt = runtime();
        let mut host = MemoryHost::default();
        host.models.insert(
            "t:item/wand".into(),
            (vec![face("t:item/wand")], "t:item/wand".into()),
        );
        let item = rt.new_object("net/minecraft/world/item/Item");
        rt.registered.insert(("item".into(), "t:wand".into()), item);
        let renderer = single(&mut rt, ITEM_RENDERER);
        let (wrapper, given, original) = rt.with_host(&mut host, |rt| {
            let models = registry(rt);
            let wand = rt.string("t:wand");
            let location = crate::api::call(rt, INVENTORY, &[wand]);
            let original = crate::api::call(rt, MAP_GET, &[models, location]);
            let wrapper = rt.new_object("net/neoforged/neoforge/client/model/BakedModelWrapper");
            crate::api::call(rt, WRAPPER_NEW, &[wrapper, original]);
            crate::api::call(rt, MAP_PUT, &[models, location, wrapper]);
            let stack = crate::api_common::stack(rt, "t:wand", 1);
            let given = crate::api::call(
                rt,
                ITEM_GET_MODEL,
                &[renderer, stack, V::NULL, V::NULL, V::I(0)],
            );
            let original = rt.get_field(given, "originalModel");
            let mut m = crate::render::IDENTITY;
            m[12] = 2.0;
            let poses = crate::render::new_pose_stack(rt, m);
            let ty = crate::render::render_type(rt, "translucent");
            let buffers =
                rt.new_object("net/minecraft/client/renderer/MultiBufferSource$BufferSource");
            let consumer = crate::api::call(rt, GET_BUFFER, &[buffers, ty]);
            let light = crate::render::FULL_BRIGHT;
            crate::api::call(
                rt,
                RENDER_LISTS,
                &[
                    renderer,
                    original,
                    stack,
                    V::I(light),
                    V::I(crate::render::NO_OVERLAY),
                    poses,
                    consumer,
                ],
            );
            (wrapper, given, original)
        });
        assert_eq!(given, wrapper);
        assert_ne!(original, V::NULL);
        let drawn: Vec<(String, f32)> = host
            .drawn
            .iter()
            .flat_map(|(m, pose, _, _)| match m {
                Model::Quads(q) => q.iter().map(|q| (q.sprite.clone(), pose[12])).collect(),
                _ => Vec::new(),
            })
            .collect();
        assert_eq!(drawn, [("t:item/wand".to_owned(), 2.0)]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A listener as Create's model swapper: it wraps the model at the item `t:wand`'s location.
    fn wrap_wand(rt: &mut Rt, _id: u32, args: &[V]) -> Option<V> {
        let models = crate::api::call(
            rt,
            "net/neoforged/neoforge/client/event/ModelEvent$ModifyBakingResult.getModels.()Ljava/util/Map;",
            &[args[1]],
        );
        let wand = rt.string("t:wand");
        let location = crate::api::call(rt, INVENTORY, &[wand]);
        let original = crate::api::call(rt, MAP_GET, &[models, location]);
        let wrapper = rt.new_object("net/neoforged/neoforge/client/model/BakedModelWrapper");
        crate::api::call(rt, WRAPPER_NEW, &[wrapper, original]);
        crate::api::call(rt, MAP_PUT, &[models, location, wrapper]);
        Some(V::Void)
    }

    /// The mods' model events come once the client is set up and its resources load, as vanilla
    /// bakes its models then: an item's model asked for before (a mod's client code run early) is
    /// the game's, and after them the one a listener put there, wrapping the game's.
    #[test]
    fn the_model_events_wait_for_the_clients_setup() {
        let mut rt = runtime();
        rt.lambda_call = Some(wrap_wand);
        let item = rt.new_object("net/minecraft/world/item/Item");
        rt.registered.insert(("item".into(), "t:wand".into()), item);
        let listener = rt.lambda_id("java/util/function/Consumer", 1, Vec::new());
        rt.listeners
            .push((MODIFY_BAKING_RESULT.to_owned(), listener));
        let renderer = single(&mut rt, ITEM_RENDERER);
        let model = |rt: &mut Rt| {
            let stack = crate::api_common::stack(rt, "t:wand", 1);
            crate::api::call(
                rt,
                ITEM_GET_MODEL,
                &[renderer, stack, V::NULL, V::NULL, V::I(0)],
            )
        };
        let early = model(&mut rt);
        assert_eq!(rt.class_of(early), BAKED_MODEL);
        crate::neo::fire_client_setup(&mut rt);
        client_resources_loaded(&mut rt);
        let wrapped = model(&mut rt);
        assert_eq!(
            rt.class_of(wrapped),
            "net/neoforged/neoforge/client/model/BakedModelWrapper"
        );
        assert_eq!(rt.get_field(wrapped, "originalModel"), early);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    const SHAPER_ICON: &str = "net/minecraft/client/renderer/block/BlockModelShaper.getParticleIcon.(Lnet/minecraft/world/level/block/state/BlockState;)Lnet/minecraft/client/renderer/texture/TextureAtlasSprite;";
    const SHAPER_TEXTURE: &str = "net/minecraft/client/renderer/block/BlockModelShaper.getTexture.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/client/renderer/texture/TextureAtlasSprite;";

    /// Create's connected-texture casing: its model (`CTModel`, whose classes extend NeoForge's
    /// `BakedModelWrapper` and leave `getParticleIcon` alone) wraps the game's at the block's
    /// location. Asked through `BakedModel`, as the block model shaper asks it, the wrapper's
    /// method runs: the casing's particle sprite, and the one for its model data at a position,
    /// are the wrapped model's, not the missing sprite.
    #[test]
    fn a_mods_model_wrapper_gives_the_particle_sprite_of_the_model_it_wraps() {
        let mut rt = runtime();
        crate::neo::fire_client_setup(&mut rt);
        rt.supers.insert(
            "test/CtModel",
            "net/neoforged/neoforge/client/model/BakedModelWrapper",
        );
        rt.supers
            .insert("test/CasingBlock", "net/minecraft/world/level/block/Block");
        let block = rt.new_object("test/CasingBlock");
        rt.registered
            .insert(("block".into(), "t:casing".into()), block);
        let mut host = MemoryHost {
            client: true,
            ..MemoryHost::default()
        };
        host.models
            .insert("t:casing".into(), (vec![], "t:block/casing".into()));
        let sprites = rt.with_host(&mut host, |rt| {
            let models = registry(rt);
            let at = location(rt, "t:casing", "");
            let original = crate::api::call(rt, MAP_GET, &[models, at]);
            let wrapper = rt.new_object("test/CtModel");
            crate::api::call(rt, WRAPPER_NEW, &[wrapper, original]);
            crate::api::call(rt, MAP_PUT, &[models, at, wrapper]);
            let (shaper, state) = (model_shaper(rt), crate::api::state(rt, "t:casing"));
            let (level, pos) = (crate::glue::level(rt), crate::api::block_pos(rt, 0, 64, 0));
            let icon = crate::api::call(rt, SHAPER_ICON, &[shaper, state]);
            let texture = crate::api::call(rt, SHAPER_TEXTURE, &[shaper, state, level, pos]);
            [icon, texture].map(|s| rt.str_of(rt.get_field(s, "$name")).unwrap_or("").to_owned())
        });
        assert_eq!(sprites, ["t:block/casing", "t:block/casing"]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
