//! Vanilla's entity renderers a mod's renderer builds on (`EntityRenderer`, `LivingEntityRenderer`,
//! `MobRenderer`, `HumanoidMobRenderer`, their `RenderLayer`s), the dispatcher that draws an entity
//! for another renderer (`EntityRenderDispatcher.render`), and the textures entity and block entity
//! models are drawn with: a `Material`'s sprite (`Material.buffer`, `TextureAtlasSprite.wrap`) and
//! `Sheets`' render types.
//!
//! A living renderer draws as vanilla's `LivingEntityRenderer.render` does: the entity's body turned
//! to its yaw, the model posed by its `prepareMobModel` and `setupAnim` from the entity's walk, look
//! and attack, drawn with its texture's render type, then each layer; NeoForge's
//! `RenderLivingEvent.Pre` first (a listener may cancel it) and `.Post` after.
use crate::{Rt, V, jutil};

const ENTITY_RENDERER: &str = "net/minecraft/client/renderer/entity/EntityRenderer";
const LIVING_RENDERER: &str = "net/minecraft/client/renderer/entity/LivingEntityRenderer";
const MOB_RENDERER: &str = "net/minecraft/client/renderer/entity/MobRenderer";
const HUMANOID_MOB_RENDERER: &str = "net/minecraft/client/renderer/entity/HumanoidMobRenderer";
const ABSTRACT_ZOMBIE_RENDERER: &str = "net/minecraft/client/renderer/entity/AbstractZombieRenderer";
const NOOP_RENDERER: &str = "net/minecraft/client/renderer/entity/NoopRenderer";
const DISPATCHER: &str = "net/minecraft/client/renderer/entity/EntityRenderDispatcher";
const CONTEXT: &str = "net/minecraft/client/renderer/entity/EntityRendererProvider$Context";
const ITEM_RENDERER: &str = "net/minecraft/client/renderer/entity/ItemRenderer";
const RENDER_LAYER: &str = "net/minecraft/client/renderer/entity/layers/RenderLayer";
const HEAD_LAYER: &str = "net/minecraft/client/renderer/entity/layers/CustomHeadLayer";
const ELYTRA_LAYER: &str = "net/minecraft/client/renderer/entity/layers/ElytraLayer";
const ITEM_IN_HAND_LAYER: &str = "net/minecraft/client/renderer/entity/layers/ItemInHandLayer";
const ARMOR_LAYER: &str = "net/minecraft/client/renderer/entity/layers/HumanoidArmorLayer";
/// Vanilla's layers a renderer of vanilla's adds: kept in its list, not drawn yet (a held item, a
/// worn head, an elytra and armour drawn on a mod's mob).
const VANILLA_LAYERS: [&str; 4] = [HEAD_LAYER, ELYTRA_LAYER, ITEM_IN_HAND_LAYER, ARMOR_LAYER];
const MATERIAL: &str = "net/minecraft/client/resources/model/Material";
const SHEETS: &str = "net/minecraft/client/renderer/Sheets";
const SPRITE: &str = "net/minecraft/client/renderer/texture/TextureAtlasSprite";
const RENDER_LIVING: &str = "net/neoforged/neoforge/client/event/RenderLivingEvent";
const RENDER_LIVING_PRE: &str = "net/neoforged/neoforge/client/event/RenderLivingEvent$Pre";
const RENDER_LIVING_POST: &str = "net/neoforged/neoforge/client/event/RenderLivingEvent$Post";
const EVENT: &str = "net/neoforged/bus/api/Event";

const GET_BUFFER: &str =
    "net/minecraft/client/renderer/MultiBufferSource.getBuffer.(Lnet/minecraft/client/renderer/RenderType;)Lcom/mojang/blaze3d/vertex/VertexConsumer;";
const FUNCTION_APPLY: &str = "java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;";
const PUSH: &str = "com/mojang/blaze3d/vertex/PoseStack.pushPose.()V";
const POP: &str = "com/mojang/blaze3d/vertex/PoseStack.popPose.()V";
const TRANSLATE: &str = "com/mojang/blaze3d/vertex/PoseStack.translate.(FFF)V";
const TRANSLATE_D: &str = "com/mojang/blaze3d/vertex/PoseStack.translate.(DDD)V";
const SCALE: &str = "com/mojang/blaze3d/vertex/PoseStack.scale.(FFF)V";
const MUL_POSE: &str = "com/mojang/blaze3d/vertex/PoseStack.mulPose.(Lorg/joml/Quaternionf;)V";
const IS_INVISIBLE: &str = "net/minecraft/world/entity/Entity.isInvisible.()Z";
const IS_PASSENGER: &str = "net/minecraft/world/entity/Entity.isPassenger.()Z";
const IS_SPECTATOR: &str = "net/minecraft/world/entity/Entity.isSpectator.()Z";
const IS_VEHICLE: &str = "net/minecraft/world/entity/Entity.isVehicle.()Z";
const SHOULD_SHOW_NAME: &str = "net/minecraft/world/entity/Entity.shouldShowName.()Z";

// The renderer's methods a mod may override, as `LivingEntityRenderer.render` calls them.
const GET_BOB: (&str, &str) = (
    "net/minecraft/client/renderer/entity/LivingEntityRenderer.getBob.(Lnet/minecraft/world/entity/LivingEntity;F)F",
    "getBob.(Lnet/minecraft/world/entity/LivingEntity;F)F",
);
const SETUP_ROTATIONS: (&str, &str) = (
    "net/minecraft/client/renderer/entity/LivingEntityRenderer.setupRotations.(Lnet/minecraft/world/entity/LivingEntity;Lcom/mojang/blaze3d/vertex/PoseStack;FFFF)V",
    "setupRotations.(Lnet/minecraft/world/entity/LivingEntity;Lcom/mojang/blaze3d/vertex/PoseStack;FFFF)V",
);
const SCALE_HOOK: (&str, &str) = (
    "net/minecraft/client/renderer/entity/LivingEntityRenderer.scale.(Lnet/minecraft/world/entity/LivingEntity;Lcom/mojang/blaze3d/vertex/PoseStack;F)V",
    "scale.(Lnet/minecraft/world/entity/LivingEntity;Lcom/mojang/blaze3d/vertex/PoseStack;F)V",
);
const IS_BODY_VISIBLE: (&str, &str) = (
    "net/minecraft/client/renderer/entity/LivingEntityRenderer.isBodyVisible.(Lnet/minecraft/world/entity/LivingEntity;)Z",
    "isBodyVisible.(Lnet/minecraft/world/entity/LivingEntity;)Z",
);
const GET_RENDER_TYPE: (&str, &str) = (
    "net/minecraft/client/renderer/entity/LivingEntityRenderer.getRenderType.(Lnet/minecraft/world/entity/LivingEntity;ZZZ)Lnet/minecraft/client/renderer/RenderType;",
    "getRenderType.(Lnet/minecraft/world/entity/LivingEntity;ZZZ)Lnet/minecraft/client/renderer/RenderType;",
);
const WHITE_OVERLAY: (&str, &str) = (
    "net/minecraft/client/renderer/entity/LivingEntityRenderer.getWhiteOverlayProgress.(Lnet/minecraft/world/entity/LivingEntity;F)F",
    "getWhiteOverlayProgress.(Lnet/minecraft/world/entity/LivingEntity;F)F",
);
const GET_ATTACK_ANIM: (&str, &str) = (
    "net/minecraft/client/renderer/entity/LivingEntityRenderer.getAttackAnim.(Lnet/minecraft/world/entity/LivingEntity;F)F",
    "getAttackAnim.(Lnet/minecraft/world/entity/LivingEntity;F)F",
);
const IS_SHAKING: (&str, &str) = (
    "net/minecraft/client/renderer/entity/LivingEntityRenderer.isShaking.(Lnet/minecraft/world/entity/LivingEntity;)Z",
    "isShaking.(Lnet/minecraft/world/entity/LivingEntity;)Z",
);
const FLIP_DEGREES: (&str, &str) = (
    "net/minecraft/client/renderer/entity/LivingEntityRenderer.getFlipDegrees.(Lnet/minecraft/world/entity/LivingEntity;)F",
    "getFlipDegrees.(Lnet/minecraft/world/entity/LivingEntity;)F",
);
const TEXTURE_LOCATION: (&str, &str) = (
    "net/minecraft/client/renderer/entity/EntityRenderer.getTextureLocation.(Lnet/minecraft/world/entity/Entity;)Lnet/minecraft/resources/ResourceLocation;",
    "getTextureLocation.(Lnet/minecraft/world/entity/Entity;)Lnet/minecraft/resources/ResourceLocation;",
);
const RENDER_OFFSET: (&str, &str) = (
    "net/minecraft/client/renderer/entity/EntityRenderer.getRenderOffset.(Lnet/minecraft/world/entity/Entity;F)Lnet/minecraft/world/phys/Vec3;",
    "getRenderOffset.(Lnet/minecraft/world/entity/Entity;F)Lnet/minecraft/world/phys/Vec3;",
);
const RENDER: (&str, &str) = (
    "net/minecraft/client/renderer/entity/EntityRenderer.render.(Lnet/minecraft/world/entity/Entity;FFLcom/mojang/blaze3d/vertex/PoseStack;Lnet/minecraft/client/renderer/MultiBufferSource;I)V",
    "render.(Lnet/minecraft/world/entity/Entity;FFLcom/mojang/blaze3d/vertex/PoseStack;Lnet/minecraft/client/renderer/MultiBufferSource;I)V",
);
const LAYER_RENDER: (&str, &str) = (
    "net/minecraft/client/renderer/entity/layers/RenderLayer.render.(Lcom/mojang/blaze3d/vertex/PoseStack;Lnet/minecraft/client/renderer/MultiBufferSource;ILnet/minecraft/world/entity/Entity;FFFFFF)V",
    "render.(Lcom/mojang/blaze3d/vertex/PoseStack;Lnet/minecraft/client/renderer/MultiBufferSource;ILnet/minecraft/world/entity/Entity;FFFFFF)V",
);
const PARENT_MODEL: (&str, &str) = (
    "net/minecraft/client/renderer/entity/RenderLayerParent.getModel.()Lnet/minecraft/client/model/EntityModel;",
    "getModel.()Lnet/minecraft/client/model/EntityModel;",
);
// The model's, as the renderer calls them.
const PREPARE: (&str, &str) = (
    "net/minecraft/client/model/EntityModel.prepareMobModel.(Lnet/minecraft/world/entity/Entity;FFF)V",
    "prepareMobModel.(Lnet/minecraft/world/entity/Entity;FFF)V",
);
const SETUP_ANIM: (&str, &str) = (
    "net/minecraft/client/model/EntityModel.setupAnim.(Lnet/minecraft/world/entity/Entity;FFFFF)V",
    "setupAnim.(Lnet/minecraft/world/entity/Entity;FFFFF)V",
);
const MODEL_RENDER_TYPE: (&str, &str) = (
    "net/minecraft/client/model/Model.renderType.(Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/client/renderer/RenderType;",
    "renderType.(Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/client/renderer/RenderType;",
);
const RENDER_TO_BUFFER: (&str, &str) = (
    "net/minecraft/client/model/Model.renderToBuffer.(Lcom/mojang/blaze3d/vertex/PoseStack;Lcom/mojang/blaze3d/vertex/VertexConsumer;III)V",
    "renderToBuffer.(Lcom/mojang/blaze3d/vertex/PoseStack;Lcom/mojang/blaze3d/vertex/VertexConsumer;III)V",
);
const COPY_PROPERTIES: (&str, &str) = (
    "net/minecraft/client/model/EntityModel.copyPropertiesTo.(Lnet/minecraft/client/model/EntityModel;)V",
    "copyPropertiesTo.(Lnet/minecraft/client/model/EntityModel;)V",
);

/// What a translucent body is tinted (`654311423`: white at 15% alpha), an opaque one white.
const TRANSLUCENT: i32 = 654_311_423;
const WHITE: i32 = -1;

fn arg(args: &[V], i: usize) -> V {
    args.get(i).copied().unwrap_or_default()
}

/// Vanilla's renderer classes past the ones a mod extends, as the API climbs them.
pub(crate) fn api_super(class: &str) -> Option<&'static str> {
    Some(match class {
        ABSTRACT_ZOMBIE_RENDERER => HUMANOID_MOB_RENDERER,
        HUMANOID_MOB_RENDERER => MOB_RENDERER,
        MOB_RENDERER => LIVING_RENDERER,
        LIVING_RENDERER | NOOP_RENDERER => ENTITY_RENDERER,
        HEAD_LAYER | ELYTRA_LAYER | ITEM_IN_HAND_LAYER | ARMOR_LAYER => RENDER_LAYER,
        RENDER_LIVING_PRE | RENDER_LIVING_POST => RENDER_LIVING,
        RENDER_LIVING => EVENT,
        _ => return None,
    })
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(
        owner,
        LIVING_RENDERER | MOB_RENDERER | HUMANOID_MOB_RENDERER | ABSTRACT_ZOMBIE_RENDERER | NOOP_RENDERER | RENDER_LAYER | MATERIAL | SHEETS
            | RENDER_LIVING | RENDER_LIVING_PRE | RENDER_LIVING_POST
    ) || VANILLA_LAYERS.contains(&owner)
        || (owner == ENTITY_RENDERER && matches!(name, "<init>" | "render" | "getShadowRadius" | "shouldShowName" | "getRenderOffset" | "getBlockLightLevel" | "getSkyLightLevel" | "renderNameTag"))
        || (owner == SPRITE && name == "wrap")
        || (owner == ITEM_RENDERER && matches!(name, "getFoilBuffer" | "getFoilBufferDirect" | "getArmorFoilBuffer" | "getCompassFoilBuffer"))
        || (owner == CONTEXT && matches!(name, "<init>" | "getModelManager" | "getResourceManager"))
        || (owner == DISPATCHER && matches!(name, "getRenderer" | "render" | "setRenderShadow" | "setRenderHitBoxes" | "shouldRenderHitBoxes" | "getPackedLightCoords"))
}

/// The client singleton of `class` (`Minecraft`'s model manager, its entity render dispatcher):
/// the one `crate::render` hands out.
fn single(rt: &mut Rt, class: &'static str) -> V {
    rt.intern(format!("client/{class}"), |rt| rt.new_object(class))
}

// ---- textures: `Material`, `Sheets`, a sprite's consumer ----

/// The consumer `consumer` becomes wrapped in `sprite` (`TextureAtlasSprite.wrap`): the same render
/// type, its faces drawn from the sprite, as vanilla's `SpriteCoordinateExpander` maps a model's
/// 0 to 1 into the sprite. One object a type and sprite.
fn wrap(rt: &mut Rt, sprite: V, consumer: V) -> V {
    let ty = rt.get_field(consumer, "$type");
    let name = rt.str_of(rt.get_field(sprite, "$name")).unwrap_or("minecraft:missingno").to_owned();
    rt.intern(format!("consumer/{}/{name}", ty.r()), |rt| {
        let c = rt.new_object("com/mojang/blaze3d/vertex/VertexConsumer");
        rt.put_field(c, "$type", ty);
        let s = rt.string(&name);
        rt.put_field(c, "$sprite", s);
        c
    })
}

/// `new Material(atlasLocation, texture)`.
fn material(rt: &mut Rt, atlas: &str, texture: &str) -> V {
    let m = rt.new_object(MATERIAL);
    let (a, t) = (rt.string(atlas), rt.string(texture));
    rt.put_field(m, "atlasLocation", a);
    rt.put_field(m, "texture", t);
    m
}

/// `Material.renderType(getter)`: the getter applied to the atlas, kept from the first call.
fn material_render_type(rt: &mut Rt, m: V, getter: V) -> V {
    match rt.get_field(m, "$renderType") {
        V::R(r) if r != 0 => V::R(r),
        _ => {
            let atlas = rt.get_field(m, "atlasLocation");
            let ty = rt.call_functional(FUNCTION_APPLY, getter, &[atlas]);
            rt.put_field(m, "$renderType", ty);
            ty
        }
    }
}

/// `ResourceLocation.hashCode`: the namespace's hash times 31 plus the path's.
fn location_hash(rt: &Rt, v: V) -> i32 {
    let s = crate::neo::location(rt, v);
    let (ns, path) = s.split_once(':').unwrap_or(("minecraft", &s));
    crate::jlang::string_hash(ns).wrapping_mul(31).wrapping_add(crate::jlang::string_hash(path))
}

fn material_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let this = a(0);
    Some(match name {
        "<init>" => {
            rt.put_field(this, "atlasLocation", a(1));
            rt.put_field(this, "texture", a(2));
            V::Void
        }
        "atlasLocation" | "texture" => rt.get_field(this, if name == "texture" { "texture" } else { "atlasLocation" }),
        "sprite" => {
            let texture = crate::neo::location(rt, rt.get_field(this, "texture"));
            crate::render::sprite_named(rt, &texture)
        }
        "renderType" => material_render_type(rt, this, a(1)),
        // The glint of the three-argument form isn't drawn.
        "buffer" => {
            let ty = material_render_type(rt, this, a(2));
            let consumer = crate::api::call(rt, GET_BUFFER, &[a(1), ty]);
            let texture = crate::neo::location(rt, rt.get_field(this, "texture"));
            let sprite = crate::render::sprite_named(rt, &texture);
            wrap(rt, sprite, consumer)
        }
        "equals" if desc == "(Ljava/lang/Object;)Z" => {
            let other = a(1);
            let same = this == other
                || (other.r() != 0
                    && rt.class_of(other) == rt.class_of(this)
                    && ["atlasLocation", "texture"].iter().all(|f| crate::neo::location(rt, rt.get_field(this, f)) == crate::neo::location(rt, rt.get_field(other, f))));
            V::I(i32::from(same))
        }
        // `Objects.hash(atlasLocation, texture)`.
        "hashCode" => {
            let (atlas, texture) = (location_hash(rt, rt.get_field(this, "atlasLocation")), location_hash(rt, rt.get_field(this, "texture")));
            V::I((31i32.wrapping_add(atlas)).wrapping_mul(31).wrapping_add(texture))
        }
        "toString" => {
            let (atlas, texture) = (crate::neo::location(rt, rt.get_field(this, "atlasLocation")), crate::neo::location(rt, rt.get_field(this, "texture")));
            rt.string(&format!("Material{{atlasLocation={atlas}, texture={texture}}}"))
        }
        _ => return None,
    })
}

/// `TextureAtlas.LOCATION_BLOCKS` and `Sheets`' atlases.
const BLOCKS: &str = "minecraft:textures/atlas/blocks.png";
const SHEET_ATLASES: [(&str, &str); 8] = [
    ("SHULKER_SHEET", "minecraft:textures/atlas/shulker_boxes.png"),
    ("BED_SHEET", "minecraft:textures/atlas/beds.png"),
    ("BANNER_SHEET", "minecraft:textures/atlas/banner_patterns.png"),
    ("SHIELD_SHEET", "minecraft:textures/atlas/shield_patterns.png"),
    ("SIGN_SHEET", "minecraft:textures/atlas/signs.png"),
    ("CHEST_SHEET", "minecraft:textures/atlas/chest.png"),
    ("ARMOR_TRIMS_SHEET", "minecraft:textures/atlas/armor_trims.png"),
    ("DECORATED_POT_SHEET", "minecraft:textures/atlas/decorated_pot.png"),
];

fn sheet(field: &str) -> &'static str {
    SHEET_ATLASES.iter().find(|(f, _)| *f == field).map_or(BLOCKS, |(_, l)| l)
}

/// `Sheets`' materials vanilla keeps as constants: the chests', the banner's and shield's base,
/// the default shulker box's.
const SHEET_MATERIALS: [(&str, &str, &str); 13] = [
    ("CHEST_TRAP_LOCATION", "CHEST_SHEET", "entity/chest/trapped"),
    ("CHEST_TRAP_LOCATION_LEFT", "CHEST_SHEET", "entity/chest/trapped_left"),
    ("CHEST_TRAP_LOCATION_RIGHT", "CHEST_SHEET", "entity/chest/trapped_right"),
    ("CHEST_XMAS_LOCATION", "CHEST_SHEET", "entity/chest/christmas"),
    ("CHEST_XMAS_LOCATION_LEFT", "CHEST_SHEET", "entity/chest/christmas_left"),
    ("CHEST_XMAS_LOCATION_RIGHT", "CHEST_SHEET", "entity/chest/christmas_right"),
    ("CHEST_LOCATION", "CHEST_SHEET", "entity/chest/normal"),
    ("CHEST_LOCATION_LEFT", "CHEST_SHEET", "entity/chest/normal_left"),
    ("CHEST_LOCATION_RIGHT", "CHEST_SHEET", "entity/chest/normal_right"),
    ("ENDER_CHEST_LOCATION", "CHEST_SHEET", "entity/chest/ender"),
    ("BANNER_BASE", "BANNER_SHEET", "entity/banner/base"),
    ("SHIELD_BASE", "SHIELD_SHEET", "entity/shield/base"),
    ("DEFAULT_SHULKER_TEXTURE_LOCATION", "SHULKER_SHEET", "entity/shulker/shulker"),
];

/// A sheet's render type: `factory` (`entity_solid`...) of the atlas.
fn sheet_type(rt: &mut Rt, factory: &str, atlas: &str) -> V {
    let location = rt.string(atlas);
    crate::render::textured_render_type(rt, factory, location)
}

/// A wood type's sign material (`Sheets.createSignMaterial`, `createHangingSignMaterial`): its
/// name's namespace, `entity/signs/[hanging/]` and its path, on the sign sheet.
fn sign_material(rt: &mut Rt, wood: V, hanging: bool) -> V {
    let name = rt.str_of(rt.get_field(wood, "$name")).unwrap_or("oak").to_owned();
    let (ns, path) = name.split_once(':').unwrap_or(("minecraft", &name));
    let dir = if hanging { "entity/signs/hanging/" } else { "entity/signs/" };
    material(rt, sheet("SIGN_SHEET"), &format!("{ns}:{dir}{path}"))
}

fn sheets_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    Some(match name {
        "bannerSheet" => sheet_type(rt, "entity_no_outline", sheet("BANNER_SHEET")),
        "shieldSheet" => sheet_type(rt, "entity_no_outline", sheet("SHIELD_SHEET")),
        "bedSheet" => sheet_type(rt, "entity_solid", sheet("BED_SHEET")),
        "shulkerBoxSheet" => sheet_type(rt, "entity_cutout_no_cull", sheet("SHULKER_SHEET")),
        "signSheet" | "hangingSignSheet" => sheet_type(rt, "entity_cutout_no_cull", sheet("SIGN_SHEET")),
        "chestSheet" => sheet_type(rt, "entity_cutout", sheet("CHEST_SHEET")),
        "armorTrimsSheet" => {
            let factory = if a(0).i() != 0 { "armor_decal_cutout_no_cull" } else { "armor_cutout_no_cull" };
            sheet_type(rt, factory, sheet("ARMOR_TRIMS_SHEET"))
        }
        "solidBlockSheet" => sheet_type(rt, "entity_solid", BLOCKS),
        "cutoutBlockSheet" => sheet_type(rt, "entity_cutout", BLOCKS),
        "translucentItemSheet" => sheet_type(rt, "item_entity_translucent_cull", BLOCKS),
        "translucentCullBlockSheet" => sheet_type(rt, "entity_translucent_cull", BLOCKS),
        "getSignMaterial" | "createSignMaterial" => sign_material(rt, a(0), false),
        "getHangingSignMaterial" | "createHangingSignMaterial" => sign_material(rt, a(0), true),
        // A mod's wood type: its sign materials are made when asked for.
        "addWoodType" => V::Void,
        // `chooseMaterial(blockEntity, chestType, holiday)`: an ender chest's, a trapped one's, the
        // Christmas one's, else the normal chest's, of its half.
        "chooseMaterial" => {
            let half = match rt.str_of(rt.get_field(a(1), "$name")) {
                Some("LEFT") => "_left",
                Some("RIGHT") => "_right",
                _ => "",
            };
            let be = rt.class_of(a(0));
            let kind = if be.ends_with("EnderChestBlockEntity") {
                return Some(material(rt, sheet("CHEST_SHEET"), "minecraft:entity/chest/ender"));
            } else if a(2).i() != 0 {
                "christmas"
            } else if be.ends_with("TrappedChestBlockEntity") {
                "trapped"
            } else {
                "normal"
            };
            material(rt, sheet("CHEST_SHEET"), &format!("minecraft:entity/chest/{kind}{half}"))
        }
        _ => return None,
    })
}

/// `Sheets`' constants: its atlases and the materials it keeps.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    if owner != SHEETS {
        return None;
    }
    if let Some((_, l)) = SHEET_ATLASES.iter().find(|(f, _)| *f == field) {
        return Some(rt.string(l));
    }
    let &(_, atlas, texture) = SHEET_MATERIALS.iter().find(|(f, _, _)| *f == field)?;
    Some(rt.intern(format!("compat:sheets/{field}"), |rt| material(rt, sheet(atlas), &format!("minecraft:{texture}"))))
}

// ---- renderers ----

/// `EntityRenderer(context)`: the dispatcher and font it draws with, no shadow yet.
fn init_renderer(rt: &mut Rt, this: V) {
    let dispatcher = single(rt, DISPATCHER);
    rt.put_field(this, "entityRenderDispatcher", dispatcher);
    let font = single(rt, "net/minecraft/client/gui/Font");
    rt.put_field(this, "$font", font);
    rt.put_field(this, "shadowRadius", V::F(0.0));
    rt.put_field(this, "shadowStrength", V::F(1.0));
}

/// `LivingEntityRenderer(context, model, shadowRadius)`: its model, shadow, and no layers yet.
fn init_living(rt: &mut Rt, this: V, model: V, shadow: f32) {
    init_renderer(rt, this);
    rt.put_field(this, "model", model);
    rt.put_field(this, "shadowRadius", V::F(shadow));
    let layers = jutil::new_list(rt, Vec::new());
    rt.put_field(this, "layers", layers);
}

/// A layer of vanilla's kind `class` on `parent` (the renderer it draws for).
fn vanilla_layer(rt: &mut Rt, class: &'static str, parent: V) -> V {
    let l = rt.new_object(class);
    rt.put_field(l, "$renderer", parent);
    l
}

fn add_layer(rt: &mut Rt, renderer: V, layer: V) {
    let layers = rt.get_field(renderer, "layers");
    jutil::list_add(rt, layers, layer);
}

/// Whether `renderer` is one of vanilla's living renderers or a mod's built on one.
fn is_living_renderer(rt: &Rt, renderer: V) -> bool {
    rt.instance_of(renderer, LIVING_RENDERER)
}

fn entity_bool(rt: &mut Rt, key: &'static str, e: V) -> bool {
    crate::api::call(rt, key, &[e]).i() != 0
}

/// A float field of the entity (`yBodyRot`, `walkAnimation`'s...), `default` when it hasn't one: a
/// mod's living entity keeps vanilla's public fields under their names.
fn field_or(rt: &Rt, o: V, field: &'static str, default: f32) -> f32 {
    match rt.get_field(o, field) {
        V::F(f) => f,
        V::I(i) => i as f32,
        V::D(d) => d as f32,
        _ => default,
    }
}

/// `Mth.rotLerp`: from `a` toward `b` by `t`, the short way round.
fn rot_lerp(t: f32, a: f32, b: f32) -> f32 {
    a + t * wrap_degrees(b - a)
}

/// `Mth.wrapDegrees(float)`.
fn wrap_degrees(v: f32) -> f32 {
    let mut f = v % 360.0;
    if f >= 180.0 {
        f -= 360.0;
    }
    if f < -180.0 {
        f += 360.0;
    }
    f
}

/// `Axis.YP/ZP.rotationDegrees(degrees)` as a quaternion: about y (`axis` 1) or z (2).
fn rotation_degrees(rt: &mut Rt, axis: usize, degrees: f32) -> V {
    let half = degrees * (std::f32::consts::PI / 180.0) * 0.5;
    let (s, c) = (half.sin(), half.cos());
    let mut q = [0.0, 0.0, 0.0, c];
    q[axis] = s;
    crate::render::new_quat(rt, q)
}

/// The entity's yaw and its last tick's (`yRot`/`yRotO` as `crate::mod_entities` keeps them).
fn yaw(rt: &Rt, e: V) -> (f32, f32) {
    let now = match rt.get_field(e, "$yRot") {
        V::Void => field_or(rt, e, "yRot", 0.0),
        v => v.f(),
    };
    (field_or(rt, e, "yRotO", now), now)
}

fn pitch(rt: &Rt, e: V) -> (f32, f32) {
    let now = match rt.get_field(e, "$xRot") {
        V::Void => field_or(rt, e, "xRot", 0.0),
        v => v.f(),
    };
    (field_or(rt, e, "xRotO", now), now)
}

/// `WalkAnimationState.speed(partial)` and `.position(partial)` of the entity's `walkAnimation`
/// (none: standing).
fn walk(rt: &Rt, e: V, partial: f32) -> (f32, f32) {
    let w = rt.get_field(e, "walkAnimation");
    if w.r() == 0 {
        return (0.0, 0.0);
    }
    let (old, speed, position) = (field_or(rt, w, "speedOld", 0.0), field_or(rt, w, "speed", 0.0), field_or(rt, w, "position", 0.0));
    (old + partial * (speed - old), position - speed * (1.0 - partial))
}

/// `LivingEntity.getAttackAnim(partial)`: how far into its swing, 0 to 1.
fn attack_anim(rt: &Rt, e: V, partial: f32) -> f32 {
    let (old, now) = (field_or(rt, e, "oAttackAnim", 0.0), field_or(rt, e, "attackAnim", 0.0));
    let mut f = now - old;
    if f < 0.0 {
        f += 1.0;
    }
    old + f * partial
}

fn is_baby(rt: &mut Rt, e: V) -> bool {
    let class = rt.class_of(e);
    rt.overrides.is_some_and(|o| o(class, "isBaby.()Z")) && crate::api::virtual_call(rt, ("net/minecraft/world/entity/LivingEntity.isBaby.()Z", "isBaby.()Z"), &[e]).i() != 0
}

/// `LivingEntity.getScale()`: its scale attribute times its age's (1 for a mod's entity that has
/// neither).
fn entity_scale(rt: &mut Rt, e: V) -> f32 {
    let class = rt.class_of(e);
    if rt.overrides.is_some_and(|o| o(class, "getScale.()F")) {
        return crate::api::virtual_call(rt, ("net/minecraft/world/entity/LivingEntity.getScale.()F", "getScale.()F"), &[e]).f();
    }
    1.0
}

/// `LivingEntityRenderer.getOverlayCoords(entity, whiteness)`: the overlay's white flash and its red
/// while hurt or dying.
fn overlay_coords(rt: &Rt, e: V, u: f32) -> i32 {
    let hurt = field_or(rt, e, "hurtTime", 0.0) > 0.0 || field_or(rt, e, "deathTime", 0.0) > 0.0;
    (u * 15.0) as i32 | (if hurt { 3 } else { 10 }) << 16
}

/// `LivingEntityRenderer.setupRotations(entity, stack, bob, yBodyRot, partial, scale)`: the body
/// turned to its yaw (shaking while frozen), a dying one tipping over.
fn setup_rotations(rt: &mut Rt, renderer: V, e: V, stack: V, mut body: f32, partial: f32) {
    if crate::api::virtual_call(rt, IS_SHAKING, &[renderer, e]).i() != 0 {
        let ticks = rt.get_field(e, "tickCount").i();
        body += ((f64::from(ticks) * 3.25).cos() * std::f64::consts::PI * f64::from(0.4f32)) as f32;
    }
    let turn = rotation_degrees(rt, 1, 180.0 - body);
    crate::api::call(rt, MUL_POSE, &[stack, turn]);
    let death = field_or(rt, e, "deathTime", 0.0);
    if death > 0.0 {
        let f = ((death + partial - 1.0) / 20.0 * 1.6).sqrt().min(1.0);
        let flip = crate::api::virtual_call(rt, FLIP_DEGREES, &[renderer, e]).f();
        let tip = rotation_degrees(rt, 2, f * flip);
        crate::api::call(rt, MUL_POSE, &[stack, tip]);
    }
}

/// `RenderLivingEvent.Pre` or `.Post` for the entity drawn: true when a listener canceled it.
fn post_render_living(rt: &mut Rt, class: &'static str, e: V, renderer: V, partial: f32, stack: V, buffers: V, light: i32) -> bool {
    for (c, sup) in [(RENDER_LIVING_PRE, RENDER_LIVING), (RENDER_LIVING_POST, RENDER_LIVING), (RENDER_LIVING, EVENT)] {
        rt.supers.entry(c).or_insert(sup);
    }
    let ev = rt.new_object(class);
    for (f, v) in [("$entity", e), ("$renderer", renderer), ("$partialTick", V::F(partial)), ("$poseStack", stack), ("$multiBufferSource", buffers), ("$packedLight", V::I(light)), ("$canceled", V::I(0))] {
        rt.put_field(ev, f, v);
    }
    crate::neo::post(rt, class, ev);
    rt.get_field(ev, "$canceled").i() != 0
}

/// `LivingEntityRenderer.render(entity, yaw, partial, stack, buffers, light)`, as vanilla draws a
/// living entity (see the module's comment). Sleeping, riding a living mount and a spin attack are
/// the player's and vanilla mobs': a mod's entity does none of them here.
fn render_living(rt: &mut Rt, renderer: V, e: V, partial: f32, stack: V, buffers: V, light: i32) {
    if post_render_living(rt, RENDER_LIVING_PRE, e, renderer, partial, stack, buffers, light) {
        return;
    }
    crate::api::call(rt, PUSH, &[stack]);
    let model = rt.get_field(renderer, "model");
    let attack = crate::api::virtual_call(rt, GET_ATTACK_ANIM, &[renderer, e, V::F(partial)]).f();
    rt.put_field(model, "attackTime", V::F(attack));
    let riding = entity_bool(rt, IS_PASSENGER, e);
    rt.put_field(model, "riding", V::I(i32::from(riding)));
    let baby = is_baby(rt, e);
    rt.put_field(model, "young", V::I(i32::from(baby)));
    let (yaw_old, yaw_now) = yaw(rt, e);
    let body = rot_lerp(partial, field_or(rt, e, "yBodyRotO", yaw_old), field_or(rt, e, "yBodyRot", yaw_now));
    let head = rot_lerp(partial, field_or(rt, e, "yHeadRotO", yaw_old), field_or(rt, e, "yHeadRot", yaw_now));
    let net_head = wrap_degrees(head - body);
    let (pitch_old, pitch_now) = pitch(rt, e);
    let head_pitch = pitch_old + partial * (pitch_now - pitch_old);
    let scale = entity_scale(rt, e);
    crate::api::call(rt, SCALE, &[stack, V::F(scale), V::F(scale), V::F(scale)]);
    let bob = crate::api::virtual_call(rt, GET_BOB, &[renderer, e, V::F(partial)]).f();
    crate::api::virtual_call(rt, SETUP_ROTATIONS, &[renderer, e, stack, V::F(bob), V::F(body), V::F(partial), V::F(scale)]);
    crate::api::call(rt, SCALE, &[stack, V::F(-1.0), V::F(-1.0), V::F(1.0)]);
    crate::api::virtual_call(rt, SCALE_HOOK, &[renderer, e, stack, V::F(partial)]);
    crate::api::call(rt, TRANSLATE, &[stack, V::F(0.0), V::F(-1.501), V::F(0.0)]);
    let (mut speed, mut position) = (0.0, 0.0);
    if !riding && field_or(rt, e, "deathTime", 0.0) <= 0.0 {
        (speed, position) = walk(rt, e, partial);
        if baby {
            position *= 3.0;
        }
        speed = speed.min(1.0);
    }
    crate::api::virtual_call(rt, PREPARE, &[model, e, V::F(position), V::F(speed), V::F(partial)]);
    crate::api::virtual_call(rt, SETUP_ANIM, &[model, e, V::F(position), V::F(speed), V::F(bob), V::F(net_head), V::F(head_pitch)]);
    // Nothing a mod's entity does makes it glow or shows it translucent to the player here.
    let visible = crate::api::virtual_call(rt, IS_BODY_VISIBLE, &[renderer, e]).i() != 0;
    let translucent = false;
    let ty = crate::api::virtual_call(rt, GET_RENDER_TYPE, &[renderer, e, V::I(i32::from(visible)), V::I(i32::from(translucent)), V::I(0)]);
    if ty.r() != 0 {
        let consumer = crate::api::call(rt, GET_BUFFER, &[buffers, ty]);
        let white = crate::api::virtual_call(rt, WHITE_OVERLAY, &[renderer, e, V::F(partial)]).f();
        let overlay = overlay_coords(rt, e, white);
        let color = if translucent { TRANSLUCENT } else { WHITE };
        crate::api::virtual_call(rt, RENDER_TO_BUFFER, &[model, stack, consumer, V::I(light), V::I(overlay), V::I(color)]);
    }
    if !entity_bool(rt, IS_SPECTATOR, e) {
        let layers = jutil::elements(rt, rt.get_field(renderer, "layers"));
        for layer in layers {
            crate::api::virtual_call(
                rt,
                LAYER_RENDER,
                &[layer, stack, buffers, V::I(light), e, V::F(position), V::F(speed), V::F(partial), V::F(bob), V::F(net_head), V::F(head_pitch)],
            );
        }
    }
    crate::api::call(rt, POP, &[stack]);
    post_render_living(rt, RENDER_LIVING_POST, e, renderer, partial, stack, buffers, light);
}

/// `LivingEntityRenderer.getRenderType(entity, bodyVisible, translucent, glowing)`: the model's
/// render type of the renderer's texture for a visible body, translucent for a see-through one,
/// none for an invisible one.
fn living_render_type(rt: &mut Rt, renderer: V, e: V, visible: bool, translucent: bool) -> V {
    let texture = crate::api::virtual_call(rt, TEXTURE_LOCATION, &[renderer, e]);
    if translucent {
        crate::render::textured_render_type(rt, "item_entity_translucent_cull", texture)
    } else if visible {
        let model = rt.get_field(renderer, "model");
        crate::api::virtual_call(rt, MODEL_RENDER_TYPE, &[model, texture])
    } else {
        V::NULL
    }
}

/// The calls on vanilla's renderers (and a mod's built on them).
fn renderer_call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    let this = a(0);
    Some(match name {
        "<init>" => {
            match owner {
                ENTITY_RENDERER | NOOP_RENDERER => init_renderer(rt, this),
                ABSTRACT_ZOMBIE_RENDERER => {
                    // `super(context, model, 0.5F)`, then armour from the inner and outer models.
                    init_living(rt, this, a(2), 0.5);
                    let armor = vanilla_layer(rt, ARMOR_LAYER, this);
                    rt.put_field(armor, "$inner", a(3));
                    rt.put_field(armor, "$outer", a(4));
                    add_layer(rt, this, armor);
                }
                _ => {
                    init_living(rt, this, a(2), a(3).f());
                    // A humanoid's head item, elytra and held items.
                    if owner == HUMANOID_MOB_RENDERER {
                        for class in [HEAD_LAYER, ELYTRA_LAYER, ITEM_IN_HAND_LAYER] {
                            let l = vanilla_layer(rt, class, this);
                            add_layer(rt, this, l);
                        }
                    }
                }
            }
            V::Void
        }
        "addLayer" => {
            add_layer(rt, this, a(1));
            V::I(1)
        }
        "getModel" => rt.get_field(this, "model"),
        "render" if desc.ends_with("FFLcom/mojang/blaze3d/vertex/PoseStack;Lnet/minecraft/client/renderer/MultiBufferSource;I)V") => {
            if is_living_renderer(rt, this) {
                render_living(rt, this, a(1), a(3).f(), a(4), a(5), a(6).i());
            }
            // The rest is `EntityRenderer.render`'s leash and name tag, not drawn here.
            V::Void
        }
        "getRenderType" if desc.ends_with("ZZZ)Lnet/minecraft/client/renderer/RenderType;") => living_render_type(rt, this, a(1), a(2).i() != 0, a(3).i() != 0),
        "getOverlayCoords" => V::I(overlay_coords(rt, a(0), a(1).f())),
        "isBodyVisible" => V::I(i32::from(!entity_bool(rt, IS_INVISIBLE, a(1)))),
        // Freezing (powder snow) is vanilla's: a mod's entity doesn't freeze here.
        "isShaking" => V::I(0),
        "setupRotations" => {
            setup_rotations(rt, this, a(1), a(2), a(4).f(), a(5).f());
            V::Void
        }
        "getAttackAnim" => V::F(attack_anim(rt, a(1), a(2).f())),
        "getBob" => V::F(rt.get_field(a(1), "tickCount").i() as f32 + a(2).f()),
        "getFlipDegrees" => V::F(90.0),
        "getWhiteOverlayProgress" => V::F(0.0),
        "scale" => V::Void,
        "isEntityUpsideDown" => V::I(0),
        "getShadowRadius" => {
            let r = rt.get_field(this, "shadowRadius").f();
            V::F(if is_living_renderer(rt, this) { r * entity_scale(rt, a(1)) } else { r })
        }
        // Name tags aren't drawn over entities here, so none shows.
        "shouldShowName" => {
            let shown = !entity_bool(rt, IS_INVISIBLE, a(1)) && !entity_bool(rt, IS_VEHICLE, a(1)) && entity_bool(rt, SHOULD_SHOW_NAME, a(1));
            V::I(i32::from(shown))
        }
        "renderNameTag" => V::Void,
        "getRenderOffset" => {
            let v = rt.new_object("net/minecraft/world/phys/Vec3");
            for f in ["x", "y", "z"] {
                rt.put_field(v, f, V::D(0.0));
            }
            v
        }
        // The light the game draws a mod's entity in: full.
        "getBlockLightLevel" | "getSkyLightLevel" => V::I(15),
        "getTextureLocation" if owner == ABSTRACT_ZOMBIE_RENDERER => rt.string("minecraft:textures/entity/zombie/zombie.png"),
        _ => return None,
    })
}

/// `RenderLayer`'s own: its parent, and the static helpers that draw a coloured cutout model.
fn layer_call(rt: &mut Rt, owner: &str, name: &str, args: &[V]) -> Option<V> {
    let a = |i| arg(args, i);
    Some(match name {
        "<init>" => {
            rt.put_field(a(0), "$renderer", a(1));
            V::Void
        }
        "getParentModel" => {
            let parent = rt.get_field(a(0), "$renderer");
            crate::api::virtual_call(rt, PARENT_MODEL, &[parent])
        }
        // `renderColoredCutoutModel(model, texture, stack, buffers, light, entity, color)`.
        "renderColoredCutoutModel" => {
            render_colored_cutout(rt, a(0), a(1), a(2), a(3), a(4).i(), a(5), a(6).i());
            V::Void
        }
        // `coloredCutoutModelCopyLayerRender(parent, model, texture, stack, buffers, light, entity,
        // limbSwing, amount, age, netHeadYaw, headPitch, partial, color)`: the parent's pose on the
        // layer's model, unless the entity is invisible.
        "coloredCutoutModelCopyLayerRender" => {
            let (parent, model, e) = (a(0), a(1), a(6));
            if !entity_bool(rt, IS_INVISIBLE, e) {
                crate::api::virtual_call(rt, COPY_PROPERTIES, &[parent, model]);
                crate::api::virtual_call(rt, PREPARE, &[model, e, a(7), a(8), a(12)]);
                crate::api::virtual_call(rt, SETUP_ANIM, &[model, e, a(7), a(8), a(9), a(10), a(11)]);
                render_colored_cutout(rt, model, a(2), a(3), a(4), a(5).i(), e, a(13).i());
            }
            V::Void
        }
        // Vanilla's layers on a mod's mob (its held items, head, elytra, armour) aren't drawn yet.
        "render" if owner != RENDER_LAYER => {
            let class = rt.class_of(a(0));
            crate::glue::trace_once(rt, &format!("layer-not-drawn/{class}"), |_| format!("client: {class} isn't drawn on a mod's entity yet"));
            V::Void
        }
        _ => return None,
    })
}

fn render_colored_cutout(rt: &mut Rt, model: V, texture: V, stack: V, buffers: V, light: i32, e: V, color: i32) {
    let ty = crate::render::textured_render_type(rt, "entity_cutout_no_cull", texture);
    let consumer = crate::api::call(rt, GET_BUFFER, &[buffers, ty]);
    let overlay = overlay_coords(rt, e, 0.0);
    crate::api::virtual_call(rt, RENDER_TO_BUFFER, &[model, stack, consumer, V::I(light), V::I(overlay), V::I(color)]);
}

/// `EntityRenderDispatcher.render(entity, x, y, z, yaw, partial, stack, buffers, light)`: the
/// entity drawn by its renderer at `(x, y, z)` from the stack's origin (and its renderer's
/// offset). A mod's entity is drawn by the renderer its mod registered; the game's own aren't drawn
/// from a mod's renderer yet.
fn dispatch_render(rt: &mut Rt, args: &[V]) {
    let a = |i| arg(args, i);
    let e = a(1);
    let Some(renderer) = crate::glue::entity_renderer(rt, e) else {
        let class = rt.class_of(e);
        crate::glue::trace_once(rt, &format!("dispatch-not-drawn/{class}"), |_| format!("client: a mod's renderer drew {class}, which isn't drawn from there yet"));
        return;
    };
    let (x, y, z) = (a(2).d(), a(3).d(), a(4).d());
    let (yaw, partial, stack, buffers, light) = (a(5), a(6), a(7), a(8), a(9));
    let offset = crate::api::virtual_call(rt, RENDER_OFFSET, &[renderer, e, partial]);
    let o = ["x", "y", "z"].map(|f| rt.get_field(offset, f).d());
    crate::api::call(rt, PUSH, &[stack]);
    crate::api::call(rt, TRANSLATE_D, &[stack, V::D(x + o[0]), V::D(y + o[1]), V::D(z + o[2])]);
    crate::api::virtual_call(rt, RENDER, &[renderer, e, yaw, partial, stack, buffers, light]);
    crate::api::call(rt, POP, &[stack]);
}

/// The calls this module models (`owner.name.desc`), or None.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i| arg(args, i);
    match owner {
        MATERIAL => material_call(rt, name, desc, args),
        SHEETS => sheets_call(rt, name, args),
        SPRITE => (name == "wrap").then(|| wrap(rt, a(0), a(1))),
        // The glint isn't drawn: the item's own type.
        ITEM_RENDERER if matches!(name, "getFoilBuffer" | "getFoilBufferDirect" | "getArmorFoilBuffer" | "getCompassFoilBuffer") => {
            Some(crate::api::call(rt, GET_BUFFER, &[a(0), a(1)]))
        }
        CONTEXT => match name {
            "<init>" => Some(V::Void),
            "getModelManager" => Some(single(rt, "net/minecraft/client/resources/model/ModelManager")),
            "getResourceManager" => Some(single(rt, "net/minecraft/server/packs/resources/ResourceManager")),
            _ => None,
        },
        DISPATCHER => match name {
            "getRenderer" => Some(crate::glue::entity_renderer(rt, a(1)).unwrap_or(V::NULL)),
            "render" => {
                dispatch_render(rt, args);
                Some(V::Void)
            }
            "setRenderShadow" | "setRenderHitBoxes" => Some(V::Void),
            "shouldRenderHitBoxes" => Some(V::I(0)),
            "getPackedLightCoords" => Some(V::I(crate::render::FULL_BRIGHT)),
            _ => None,
        },
        RENDER_LIVING | RENDER_LIVING_PRE | RENDER_LIVING_POST => Some(match name {
            "getEntity" => rt.get_field(a(0), "$entity"),
            "getRenderer" => rt.get_field(a(0), "$renderer"),
            "getPartialTick" => rt.get_field(a(0), "$partialTick"),
            "getPoseStack" => rt.get_field(a(0), "$poseStack"),
            "getMultiBufferSource" => rt.get_field(a(0), "$multiBufferSource"),
            "getPackedLight" => rt.get_field(a(0), "$packedLight"),
            "isCanceled" => V::I(rt.get_field(a(0), "$canceled").i()),
            "setCanceled" => {
                rt.put_field(a(0), "$canceled", V::I(a(1).i()));
                V::Void
            }
            _ => return None,
        }),
        RENDER_LAYER => layer_call(rt, owner, name, args),
        o if VANILLA_LAYERS.contains(&o) => layer_call(rt, owner, name, args),
        // A renderer a mod's code reaches by `EntityRenderer`'s own name (the game's draw call):
        // a living renderer draws as vanilla's; `crate::render` answers the rest.
        ENTITY_RENDERER if name == "render" && !is_living_renderer(rt, a(0)) => None,
        ENTITY_RENDERER | LIVING_RENDERER | MOB_RENDERER | HUMANOID_MOB_RENDERER | ABSTRACT_ZOMBIE_RENDERER | NOOP_RENDERER => {
            renderer_call(rt, owner, name, desc, args)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    fn key(owner: &str, name: &str, desc: &str) -> &'static str {
        Box::leak(format!("{owner}.{name}.{desc}").into_boxed_str())
    }

    #[test]
    fn a_material_draws_its_sprite_on_its_sheet() {
        let mut rt = Rt::new();
        crate::install();
        let chest = crate::api::get_static(&mut rt, "net/minecraft/client/renderer/Sheets.CHEST_LOCATION");
        let atlas = crate::api::call(&mut rt, key(MATERIAL, "atlasLocation", "()Lnet/minecraft/resources/ResourceLocation;"), &[chest]);
        assert_eq!(crate::neo::location(&rt, atlas), "minecraft:textures/atlas/chest.png");
        let sheet_fn = rt.new_object("java/util/function/Function");
        // The renderer asks for `RenderType::entityCutout` of the chest atlas.
        let ty = crate::api::call(&mut rt, key(SHEETS, "chestSheet", "()Lnet/minecraft/client/renderer/RenderType;"), &[]);
        assert_eq!(rt.str_of(rt.get_field(ty, "$texture")), Some("minecraft:textures/atlas/chest.png"));
        rt.put_field(chest, "$renderType", ty);
        let source = rt.new_object("net/minecraft/client/renderer/MultiBufferSource$BufferSource");
        let consumer = crate::api::call(
            &mut rt,
            key(MATERIAL, "buffer", "(Lnet/minecraft/client/renderer/MultiBufferSource;Ljava/util/function/Function;)Lcom/mojang/blaze3d/vertex/VertexConsumer;"),
            &[chest, source, sheet_fn],
        );
        assert_eq!(rt.get_field(consumer, "$type"), ty);
        assert_eq!(rt.str_of(rt.get_field(consumer, "$sprite")), Some("minecraft:entity/chest/normal"));
        // Equal to a new one of the same atlas and texture, with the same hash.
        let (a1, t1) = (rt.string("minecraft:textures/atlas/chest.png"), rt.string("minecraft:entity/chest/normal"));
        let same = rt.new_object(MATERIAL);
        crate::api::call(&mut rt, key(MATERIAL, "<init>", "(Lnet/minecraft/resources/ResourceLocation;Lnet/minecraft/resources/ResourceLocation;)V"), &[same, a1, t1]);
        assert_eq!(crate::api::call(&mut rt, key(MATERIAL, "equals", "(Ljava/lang/Object;)Z"), &[chest, same]).i(), 1);
        let h = |rt: &mut Rt, m| crate::api::call(rt, key(MATERIAL, "hashCode", "()I"), &[m]).i();
        assert_eq!(h(&mut rt, chest), h(&mut rt, same));
        // `Objects.hash(atlas, texture)` with `ResourceLocation`'s hash (31 * namespace + path).
        let rl = |ns: &str, p: &str| crate::jlang::string_hash(ns).wrapping_mul(31).wrapping_add(crate::jlang::string_hash(p));
        let expected = (31i32.wrapping_add(rl("minecraft", "textures/atlas/chest.png"))).wrapping_mul(31).wrapping_add(rl("minecraft", "entity/chest/normal"));
        assert_eq!(h(&mut rt, chest), expected);
    }

    #[test]
    fn a_wrapped_consumer_draws_a_model_from_its_sprite() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        let sprite = crate::render::sprite_named(&mut rt, "mymod:entity/crate_lid");
        let ty = crate::api::call(&mut rt, key(SHEETS, "cutoutBlockSheet", "()Lnet/minecraft/client/renderer/RenderType;"), &[]);
        let source = rt.new_object("net/minecraft/client/renderer/MultiBufferSource$BufferSource");
        let plain = crate::api::call(&mut rt, GET_BUFFER, &[source, ty]);
        let wrapped = crate::api::call(
            &mut rt,
            key(SPRITE, "wrap", "(Lcom/mojang/blaze3d/vertex/VertexConsumer;)Lcom/mojang/blaze3d/vertex/VertexConsumer;"),
            &[sprite, plain],
        );
        let part = crate::models::tests_support::cube_part(&mut rt);
        let stack = crate::render::new_pose_stack(&mut rt, crate::render::IDENTITY);
        rt.with_host(&mut host, |rt| {
            crate::models::render_part(rt, part, stack, plain, 0, -1);
            crate::models::render_part(rt, part, stack, wrapped, 0, -1);
        });
        // The block atlas drawn whole names no sprite: only the wrapped consumer's faces are drawn.
        assert_eq!(host.drawn.len(), 1);
        let crate::host::Model::Quads(quads) = &host.drawn[0].0 else { panic!("quads") };
        assert!(quads.iter().all(|q| q.sprite == "mymod:entity/crate_lid"));
    }

    #[test]
    fn a_mob_renderer_poses_its_model_and_draws_it_with_its_texture() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        // A mod's renderer: `super(context, model, 0.5F)`, its texture its own.
        rt.supers.insert("mymod/GolemRenderer", MOB_RENDERER);
        fn vcall(rt: &mut Rt, owner_key: &'static str, short: &'static str, args: &[V]) -> V {
            if short == TEXTURE_LOCATION.1 && rt.class_of(args[0]) == "mymod/GolemRenderer" {
                return rt.string("mymod:textures/entity/golem.png");
            }
            crate::api::call(rt, owner_key, args)
        }
        rt.vcall = Some(vcall);
        let model = crate::models::tests_support::humanoid(&mut rt);
        let renderer = rt.new_object("mymod/GolemRenderer");
        let ctx = rt.new_object(CONTEXT);
        crate::api::call(
            &mut rt,
            key(MOB_RENDERER, "<init>", "(Lnet/minecraft/client/renderer/entity/EntityRendererProvider$Context;Lnet/minecraft/client/model/EntityModel;F)V"),
            &[renderer, ctx, model, V::F(0.5)],
        );
        assert_eq!(rt.get_field(renderer, "model"), model);
        assert_eq!(rt.get_field(renderer, "shadowRadius").f(), 0.5);
        // An entity mid-stride, facing south (yaw 0), looking 30 degrees left of its body.
        let e = rt.new_object("mymod/Golem");
        for (f, v) in [("$yRot", 0.0), ("yRotO", 0.0), ("yBodyRot", 0.0), ("yBodyRotO", 0.0), ("yHeadRot", 30.0), ("yHeadRotO", 30.0)] {
            rt.put_field(e, f, V::F(v));
        }
        rt.put_field(e, "tickCount", V::I(0));
        let walk = rt.new_object("net/minecraft/world/entity/WalkAnimationState");
        for (f, v) in [("speedOld", 1.0), ("speed", 1.0), ("position", 0.0)] {
            rt.put_field(walk, f, V::F(v));
        }
        rt.put_field(e, "walkAnimation", walk);
        let stack = crate::render::new_pose_stack(&mut rt, crate::render::IDENTITY);
        let source = rt.new_object("net/minecraft/client/renderer/MultiBufferSource$BufferSource");
        rt.with_host(&mut host, |rt| {
            crate::api::call(rt, RENDER.0, &[renderer, e, V::F(0.0), V::F(1.0), stack, source, V::I(crate::render::FULL_BRIGHT)]);
        });
        // The head turned 30 degrees, in radians; at limb swing 0 and full speed the right arm swings
        // back (cos(pi) * 2 * 1 * 0.5 = -1) and the left forward, plus the arms' idle bob at age 1.
        let head = rt.get_field(model, "head");
        assert!((rt.get_field(head, "yRot").f() - 30f32.to_radians()).abs() < 1e-6);
        let right_arm = rt.get_field(model, "rightArm");
        let bob_x = crate::api_common::mth_sin(V::F(0.067)) * 0.05;
        assert!((rt.get_field(right_arm, "xRot").f() - (-1.0 + bob_x)).abs() < 1e-4, "{}", rt.get_field(right_arm, "xRot").f());
        // Seven parts drawn, each with the renderer's texture, under the body's turn (180 - 0
        // degrees about y), scaled -1, -1, 1 and lowered 1.501.
        assert_eq!(host.drawn.len(), 7);
        let crate::host::Model::Quads(quads) = &host.drawn[0].0 else { panic!("quads") };
        assert!(quads.iter().all(|q| q.sprite == "mymod:entity/golem"));
        // The head's pose: model y is down, so its pivot (y 0) is 1.501 up; turned half round about
        // y and flipped, x maps to x (−1 · −1) and z to −z.
        let pose = host.drawn[0].1;
        assert!((pose[13] - 1.501).abs() < 1e-5, "{pose:?}");
    }

    #[test]
    fn a_canceled_pre_event_draws_nothing() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        let model = crate::models::tests_support::humanoid(&mut rt);
        let renderer = rt.new_object(MOB_RENDERER);
        init_living(&mut rt, renderer, model, 0.5);
        let e = rt.new_object("mymod/Golem");
        // A listener cancels every `RenderLivingEvent.Pre`.
        fn cancel(rt: &mut Rt, _: u32, args: &[V]) -> Option<V> {
            rt.put_field(args[1], "$canceled", V::I(1));
            Some(V::Void)
        }
        rt.lambda_call = Some(cancel);
        let l = rt.lambda_id("java/util/function/Consumer", 1, Vec::new());
        rt.listeners.push((RENDER_LIVING_PRE.to_owned(), l));
        let stack = crate::render::new_pose_stack(&mut rt, crate::render::IDENTITY);
        let source = rt.new_object("net/minecraft/client/renderer/MultiBufferSource$BufferSource");
        rt.with_host(&mut host, |rt| render_living(rt, renderer, e, 0.0, stack, source, 0));
        assert!(host.drawn.is_empty());
    }
}
