//! The mods' own particles, as vanilla's particle engine runs them on a client (`ParticleEngine`):
//! the providers a mod registers (`RegisterParticleProvidersEvent`: a provider, a provider given a
//! sprite set, or one whose particle takes a sprite of its type's set), the particles they make when
//! a client level's `addParticle` names one of those types, each ticked with the client
//! (`ParticleEngine.tick`) and drawn each frame by its own `render` into the faces the game adds
//! ([`crate::host::Host::particle_quad`]).
//!
//! The mods' particle classes extend vanilla's, which are modelled here as vanilla 1.21.1 writes
//! them: `Particle` (motion, collision with the level's blocks, lifetime, colour),
//! `SingleQuadParticle` (a quad facing the camera), `TextureSheetParticle` (its sprite),
//! `SimpleAnimatedParticle` (a frame for its age, fading out) and `NoRenderParticle`. Their state
//! is in the fields vanilla declares, by the same names, so a mod's code reads and writes them as
//! its bytecode does. What vanilla's code calls on `this` (`move`, `remove`, `getLightColor`,
//! `getQuadSize`...) runs the mod's own method when its class has one.
//!
//! A type no mod gave a provider is vanilla's, or a mod's drawn without one: the game draws it
//! ([`crate::particles`]).
use crate::{Data, Rt, V};
use std::cell::RefCell;
use std::rc::Rc;

const PARTICLE: &str = "net/minecraft/client/particle/Particle";
const SINGLE_QUAD: &str = "net/minecraft/client/particle/SingleQuadParticle";
const TEXTURE_SHEET: &str = "net/minecraft/client/particle/TextureSheetParticle";
const SIMPLE_ANIMATED: &str = "net/minecraft/client/particle/SimpleAnimatedParticle";
const NO_RENDER: &str = "net/minecraft/client/particle/NoRenderParticle";
const ENGINE: &str = "net/minecraft/client/particle/ParticleEngine";
const SPRITE_SET: &str = "net/minecraft/client/particle/ParticleEngine$MutableSpriteSet";
const SPRITE_SET_API: &str = "net/minecraft/client/particle/SpriteSet";
const RENDER_TYPE: &str = "net/minecraft/client/particle/ParticleRenderType";
const FACING: &str = "net/minecraft/client/particle/SingleQuadParticle$FacingCameraMode";
const PROVIDER: &str = "net/minecraft/client/particle/ParticleProvider";
pub(crate) const PROVIDERS_EVENT: &str =
    "net/neoforged/neoforge/client/event/RegisterParticleProvidersEvent";
/// The provider `registerSprite` keeps: the mod's `ParticleProvider.Sprite`, whose particle then
/// takes a sprite of its type's set (the lambda `ParticleEngine.register(type, sprite)` makes).
const SPRITE_PROVIDER: &str = "compat:sprite-provider";
const MINECRAFT: &str = "net/minecraft/client/Minecraft";

/// Vanilla's render types (`ParticleRenderType`'s constants): the first five in the order the
/// engine ticks and draws them (`RENDER_ORDER`). `NO_RENDER` isn't in that order, so it goes with
/// the mods' own types, after them.
const TYPES: [&str; 6] = [
    "TERRAIN_SHEET",
    "PARTICLE_SHEET_OPAQUE",
    "PARTICLE_SHEET_LIT",
    "PARTICLE_SHEET_TRANSLUCENT",
    "CUSTOM",
    "NO_RENDER",
];
const NO_RENDER_TYPE: usize = 5;
/// `MAX_PARTICLES_PER_LAYER`: a render type's queue holds this many, the oldest giving way.
const PER_TYPE: usize = 16384;
const PARTICLE_ATLAS: &str = "minecraft:textures/atlas/particles.png";
const BLOCK_ATLAS: &str = "minecraft:textures/atlas/blocks.png";

const TICK: &str = "net/minecraft/client/particle/Particle.tick.()V";
const RENDER: &str = "net/minecraft/client/particle/Particle.render.(Lcom/mojang/blaze3d/vertex/VertexConsumer;Lnet/minecraft/client/Camera;F)V";
const GET_RENDER_TYPE: &str = "net/minecraft/client/particle/Particle.getRenderType.()Lnet/minecraft/client/particle/ParticleRenderType;";
const IS_ALIVE: &str = "net/minecraft/client/particle/Particle.isAlive.()Z";
const MOVE: &str = "net/minecraft/client/particle/Particle.move.(DDD)V";
const REMOVE: &str = "net/minecraft/client/particle/Particle.remove.()V";
const LIGHT: &str = "net/minecraft/client/particle/Particle.getLightColor.(F)I";
const SET_SIZE: &str = "net/minecraft/client/particle/Particle.setSize.(FF)V";
const SET_POS: &str = "net/minecraft/client/particle/Particle.setPos.(DDD)V";
const SET_ALPHA: &str = "net/minecraft/client/particle/Particle.setAlpha.(F)V";
const SET_COLOR: &str = "net/minecraft/client/particle/Particle.setColor.(FFF)V";
const GET_BOX: &str =
    "net/minecraft/client/particle/Particle.getBoundingBox.()Lnet/minecraft/world/phys/AABB;";
const SET_BOX: &str =
    "net/minecraft/client/particle/Particle.setBoundingBox.(Lnet/minecraft/world/phys/AABB;)V";
const SET_LOCATION: &str = "net/minecraft/client/particle/Particle.setLocationFromBoundingbox.()V";
const QUAD_SIZE: &str = "net/minecraft/client/particle/SingleQuadParticle.getQuadSize.(F)F";
const FACING_MODE: &str = "net/minecraft/client/particle/SingleQuadParticle.getFacingCameraMode.()Lnet/minecraft/client/particle/SingleQuadParticle$FacingCameraMode;";
const ROTATED_QUAD: &str = "net/minecraft/client/particle/SingleQuadParticle.renderRotatedQuad.(Lcom/mojang/blaze3d/vertex/VertexConsumer;Lnet/minecraft/client/Camera;Lorg/joml/Quaternionf;F)V";
const ROTATED_QUAD_AT: &str = "net/minecraft/client/particle/SingleQuadParticle.renderRotatedQuad.(Lcom/mojang/blaze3d/vertex/VertexConsumer;Lorg/joml/Quaternionf;FFFF)V";
const U0: &str = "net/minecraft/client/particle/SingleQuadParticle.getU0.()F";
const U1: &str = "net/minecraft/client/particle/SingleQuadParticle.getU1.()F";
const V0: &str = "net/minecraft/client/particle/SingleQuadParticle.getV0.()F";
const V1: &str = "net/minecraft/client/particle/SingleQuadParticle.getV1.()F";
const SET_ROTATION: &str = "net/minecraft/client/particle/SingleQuadParticle$FacingCameraMode.setRotation.(Lorg/joml/Quaternionf;Lnet/minecraft/client/Camera;F)V";
const SET_SPRITE: &str = "net/minecraft/client/particle/TextureSheetParticle.setSprite.(Lnet/minecraft/client/renderer/texture/TextureAtlasSprite;)V";
const PICK_SPRITE: &str = "net/minecraft/client/particle/TextureSheetParticle.pickSprite.(Lnet/minecraft/client/particle/SpriteSet;)V";
const SPRITE_FROM_AGE: &str = "net/minecraft/client/particle/TextureSheetParticle.setSpriteFromAge.(Lnet/minecraft/client/particle/SpriteSet;)V";
const SPRITE_BY_AGE: &str = "net/minecraft/client/particle/SpriteSet.get.(II)Lnet/minecraft/client/renderer/texture/TextureAtlasSprite;";
const SPRITE_BY_RANDOM: &str = "net/minecraft/client/particle/SpriteSet.get.(Lnet/minecraft/util/RandomSource;)Lnet/minecraft/client/renderer/texture/TextureAtlasSprite;";
const SPRITE_U0: &str = "net/minecraft/client/renderer/texture/TextureAtlasSprite.getU0.()F";
const SPRITE_U1: &str = "net/minecraft/client/renderer/texture/TextureAtlasSprite.getU1.()F";
const SPRITE_V0: &str = "net/minecraft/client/renderer/texture/TextureAtlasSprite.getV0.()F";
const SPRITE_V1: &str = "net/minecraft/client/renderer/texture/TextureAtlasSprite.getV1.()F";
const BEGIN: &str = "net/minecraft/client/particle/ParticleRenderType.begin.(Lcom/mojang/blaze3d/vertex/Tesselator;Lnet/minecraft/client/renderer/texture/TextureManager;)Lcom/mojang/blaze3d/vertex/BufferBuilder;";
const IS_TRANSLUCENT: &str = "net/minecraft/client/particle/ParticleRenderType.isTranslucent.()Z";
const CREATE_PARTICLE: &str = "net/minecraft/client/particle/ParticleProvider.createParticle.(Lnet/minecraft/core/particles/ParticleOptions;Lnet/minecraft/client/multiplayer/ClientLevel;DDDDDD)Lnet/minecraft/client/particle/Particle;";
const SPRITE_CREATE: &str = "net/minecraft/client/particle/ParticleProvider$Sprite.createParticle.(Lnet/minecraft/core/particles/ParticleOptions;Lnet/minecraft/client/multiplayer/ClientLevel;DDDDDD)Lnet/minecraft/client/particle/TextureSheetParticle;";
const REGISTRATION_CREATE: &str = "net/minecraft/client/particle/ParticleEngine$SpriteParticleRegistration.create.(Lnet/minecraft/client/particle/SpriteSet;)Lnet/minecraft/client/particle/ParticleProvider;";
const GET_TYPE: &str = "net/minecraft/core/particles/ParticleOptions.getType.()Lnet/minecraft/core/particles/ParticleType;";
const OVERRIDE_LIMITER: &str = "net/minecraft/core/particles/ParticleType.getOverrideLimiter.()Z";
const NEXT_FLOAT: &str = "net/minecraft/util/RandomSource.nextFloat.()F";
const NEXT_INT: &str = "net/minecraft/util/RandomSource.nextInt.(I)I";
const CAMERA_POSITION: &str =
    "net/minecraft/client/Camera.getPosition.()Lnet/minecraft/world/phys/Vec3;";
const CAMERA_ROTATION: &str = "net/minecraft/client/Camera.rotation.()Lorg/joml/Quaternionf;";
const TESSELATOR: &str =
    "com/mojang/blaze3d/vertex/Tesselator.getInstance.()Lcom/mojang/blaze3d/vertex/Tesselator;";
const ADD_VERTEX: &str = "com/mojang/blaze3d/vertex/VertexConsumer.addVertex.(FFF)Lcom/mojang/blaze3d/vertex/VertexConsumer;";
const SET_UV: &str =
    "com/mojang/blaze3d/vertex/VertexConsumer.setUv.(FF)Lcom/mojang/blaze3d/vertex/VertexConsumer;";
const SET_VERTEX_COLOR: &str = "com/mojang/blaze3d/vertex/VertexConsumer.setColor.(FFFF)Lcom/mojang/blaze3d/vertex/VertexConsumer;";
const SET_LIGHT: &str = "com/mojang/blaze3d/vertex/VertexConsumer.setLight.(I)Lcom/mojang/blaze3d/vertex/VertexConsumer;";
const EMPTY_OPTIONAL: &str = "java/util/Optional.empty.()Ljava/util/Optional;";

fn arg(args: &[V], i: usize) -> V {
    args.get(i).copied().unwrap_or_default()
}

// ---- the engine ----

const ENGINE_KEY: &str = "client/net/minecraft/client/particle/ParticleEngine";

/// The client's particle engine (`Minecraft.particleEngine`): one object, holding the providers by
/// type id (`providers`, the map a mod's accessor reads), the render types its particles have so
/// far (`$types`, in the order they tick and draw) with each one's queue (`$queues`), and the
/// particles added since the last tick (`$pending`).
pub(crate) fn engine(rt: &mut Rt) -> V {
    let e = rt.intern(ENGINE_KEY.into(), |rt| rt.new_object(ENGINE));
    if rt.get_field(e, "$providers").r() == 0 {
        let providers = rt.alloc("java/util/HashMap", Data::Table(Default::default()));
        rt.put_field(e, "$providers", providers);
        for field in ["$types", "$queues", "$pending"] {
            let l = crate::jutil::new_list(rt, Vec::new());
            rt.put_field(e, field, l);
        }
    }
    e
}

/// The engine, if a mod has had any use of it.
fn engine_if_any(rt: &Rt) -> Option<V> {
    rt.interned
        .get(ENGINE_KEY)
        .copied()
        .filter(|e| rt.get_field(*e, "$providers").r() != 0)
}

fn items(rt: &Rt, list: V) -> Vec<V> {
    match rt.heap.get(list.r() as usize).map(|o| &o.data) {
        Some(Data::List(l)) if list.r() != 0 => l.clone(),
        _ => Vec::new(),
    }
}

fn items_mut(rt: &mut Rt, list: V) -> Option<&mut Vec<V>> {
    match rt.heap.get_mut(list.r() as usize).map(|o| &mut o.data) {
        Some(Data::List(l)) if list.r() != 0 => Some(l),
        _ => None,
    }
}

/// Whether any of the mods' particles is live (one added since the last tick isn't drawn until it
/// joins): what a frame asks before it draws them.
pub(crate) fn any(rt: &Rt) -> bool {
    let Some(e) = engine_if_any(rt) else {
        return false;
    };
    items(rt, rt.get_field(e, "$queues"))
        .iter()
        .any(|q| !items(rt, *q).is_empty())
}

/// `RegisterParticleProvidersEvent`, which `Minecraft`'s constructor posts once it has made the
/// particle engine (after the colour handlers): each mod registers its types' providers.
pub(crate) fn post_register(rt: &mut Rt) {
    let e = engine(rt);
    let ev = rt.new_object(PROVIDERS_EVENT);
    rt.put_field(ev, "$engine", e);
    crate::neo::post(rt, PROVIDERS_EVENT, ev);
}

/// How a provider was registered (`ParticleEngine.register`'s three overloads).
#[derive(Clone, Copy, PartialEq)]
enum Registration {
    /// `registerSpecial`: the provider itself.
    Provider,
    /// `registerSprite`: a `ParticleProvider.Sprite`, whose particle takes a sprite of the set.
    Sprite,
    /// `registerSpriteSet`: a function from the type's sprite set to its provider.
    SpriteSet,
}

/// `ParticleEngine.register(type, ...)`: `type`'s provider, by its id, replacing one before.
fn register(rt: &mut Rt, ty: V, how: Registration, given: V) {
    let Some(id) = crate::particles::type_id(rt, ty) else {
        crate::glue::trace_once(rt, "particle-provider/unregistered", |_| {
            "client: a particle provider for a type that isn't registered".into()
        });
        return;
    };
    let provider = match how {
        Registration::Provider => given,
        Registration::Sprite => {
            let set = sprite_set(rt, &id);
            let p = rt.new_object(SPRITE_PROVIDER);
            rt.put_field(p, "$sprite", given);
            rt.put_field(p, "$set", set);
            p
        }
        Registration::SpriteSet => {
            let set = sprite_set(rt, &id);
            let p = rt.call_functional(REGISTRATION_CREATE, given, &[set]);
            if rt.thrown.is_some() {
                rt.report_thrown("a particle provider's registration");
                return;
            }
            p
        }
    };
    let made_by = if how == Registration::Sprite {
        given
    } else {
        provider
    };
    if !mods_own(rt, made_by) {
        // One of vanilla's providers given a mod's type (`FlameParticle.Provider::new`): vanilla's
        // particle classes aren't modelled, so the game draws the type as it draws a type with no
        // provider, from its sprites.
        crate::glue::trace_once(rt, &format!("particle-provider/{id}"), |rt| {
            format!(
                "client: {id}'s particle provider is vanilla's {}; the game draws the type",
                rt.class_of(made_by)
            )
        });
        return;
    }
    let e = engine(rt);
    let providers = rt.get_field(e, "$providers");
    let key = rt.string(&id);
    crate::jutil::table_put(rt, providers, key, provider);
}

/// Whether `provider` is the mod's own code (a lambda, a method reference, an object of its
/// classes), which runs here.
fn mods_own(rt: &Rt, provider: V) -> bool {
    provider.r() != 0
        && (rt.cid(provider) & crate::LAMBDA != 0 || rt.is_mod_class(rt.class_of(provider)))
}

/// The provider registered for type `id`, if any.
fn provider(rt: &mut Rt, id: &str) -> Option<V> {
    let e = engine_if_any(rt)?;
    let providers = rt.get_field(e, "$providers");
    let key = rt.string(id);
    crate::jutil::table_get(rt, providers, key).filter(|p| p.r() != 0)
}

/// The type of `options`: `getType()`, a mod's own data class's answer included.
fn type_of(rt: &mut Rt, options: V) -> V {
    let ty = crate::menus::virt(rt, GET_TYPE, options, &[]);
    rt.report_thrown("a particle options' getType");
    ty
}

/// `ParticleEngine.makeParticle`: `options`' particle at `at` moving by `speed`, made by its type's
/// provider. None when no mod gave the type one (the game draws it); `Some(null)` when the provider
/// made none.
fn make(rt: &mut Rt, options: V, at: [f64; 3], speed: [f64; 3]) -> Option<V> {
    let ty = type_of(rt, options);
    let id = crate::particles::type_id(rt, ty)?;
    let provider = provider(rt, &id)?;
    let level = crate::glue::level(rt);
    let args = [
        options,
        level,
        V::D(at[0]),
        V::D(at[1]),
        V::D(at[2]),
        V::D(speed[0]),
        V::D(speed[1]),
        V::D(speed[2]),
    ];
    let p = if rt.class_of(provider) == SPRITE_PROVIDER {
        let (sprite, set) = (
            rt.get_field(provider, "$sprite"),
            rt.get_field(provider, "$set"),
        );
        let p = rt.call_functional(SPRITE_CREATE, sprite, &args);
        if p.r() != 0 && rt.thrown.is_none() {
            virt(rt, PICK_SPRITE, p, &[set]);
        }
        p
    } else {
        rt.call_functional(CREATE_PARTICLE, provider, &args)
    };
    if rt.thrown.is_some() {
        // Vanilla's `LevelRenderer.addParticle` reports it as a crash; here the particle isn't made.
        rt.report_thrown(&format!("{id}'s particle provider"));
        return Some(V::NULL);
    }
    Some(p)
}

/// `ParticleEngine.add`: joins the engine's particles at its next tick.
fn add(rt: &mut Rt, p: V) {
    if p.r() == 0 {
        return;
    }
    let e = engine(rt);
    let pending = rt.get_field(e, "$pending");
    crate::jutil::list_add(rt, pending, p);
}

/// A client level's `addParticle` of `options` (`ClientLevel.addParticle` and
/// `addAlwaysVisibleParticle`, `always`; `forced` the `boolean` its overloads with one were given)
/// when a mod gave its type a provider: made as `LevelRenderer.addParticleInternal` makes it, or
/// not when the game's limit leaves it out. False for a type without one, which the game draws.
pub(crate) fn level_particle(
    rt: &mut Rt,
    options: V,
    forced: Option<bool>,
    always: bool,
    at: [f64; 3],
    speed: [f64; 3],
) -> bool {
    let Some(e) = engine_if_any(rt) else {
        return false;
    };
    if crate::jutil::table_pairs(rt, rt.get_field(e, "$providers")).is_empty() {
        return false;
    }
    let ty = type_of(rt, options);
    let Some(id) = crate::particles::type_id(rt, ty) else {
        return false;
    };
    if provider(rt, &id).is_none() {
        return false;
    }
    // `ClientLevel`: the type's own `overrideLimiter` forces it, or the overload's `boolean`; but
    // `addAlwaysVisibleParticle(options, ...)` without one is never forced, only kept at the lowest
    // particle setting (`decreased`).
    let limiter = |rt: &mut Rt| crate::menus::virt(rt, OVERRIDE_LIMITER, ty, &[]).i() != 0;
    let force = match (forced, always) {
        (None, true) => false,
        (None, false) => limiter(rt),
        (Some(f), _) => limiter(rt) || f,
    };
    if !rt
        .host_mut()
        .is_none_or(|h| h.particle_admitted(at, force, always))
    {
        return true;
    }
    if let Some(p) = make(rt, options, at, speed) {
        add(rt, p);
    }
    true
}

/// `ParticleEngine.tick`, unless the game is paused (`Minecraft.tick`): each of the mods' particles
/// ticks, by render type in order, and those no longer alive go; then the particles added since join
/// their render type's queue (`getRenderType()`). A particle whose tick throws goes (vanilla reports
/// the crash).
pub(crate) fn tick(rt: &mut Rt) {
    let Some(e) = engine_if_any(rt) else { return };
    if rt.host().is_some_and(|h| h.paused()) {
        return;
    }
    let queues = items(rt, rt.get_field(e, "$queues"));
    BOXES.with(|b| *b.borrow_mut() = Some(crate::HashMap::default()));
    for q in queues {
        let live = items(rt, q);
        if live.is_empty() {
            continue;
        }
        let mut kept = Vec::with_capacity(live.len());
        for p in live {
            virt(rt, TICK, p, &[]);
            if rt.thrown.is_some() {
                rt.report_thrown("a particle's tick");
                continue;
            }
            if virt(rt, IS_ALIVE, p, &[]).i() != 0 {
                kept.push(p);
            }
        }
        if let Some(l) = items_mut(rt, q) {
            *l = kept;
        }
    }
    BOXES.with(|b| *b.borrow_mut() = None);
    let pending = rt.get_field(e, "$pending");
    let added = items_mut(rt, pending)
        .map(std::mem::take)
        .unwrap_or_default();
    for p in added {
        let ty = virt(rt, GET_RENDER_TYPE, p, &[]);
        if rt.thrown.is_some() || ty.r() == 0 {
            rt.report_thrown("a particle's getRenderType");
            continue;
        }
        let q = queue(rt, e, ty);
        if let Some(l) = items_mut(rt, q) {
            if l.len() >= PER_TYPE {
                l.remove(0);
            }
            l.push(p);
        }
    }
}

/// Where render type `ty` sorts (NeoForge's `makeParticleRenderTypeComparator`): vanilla's in
/// `RENDER_ORDER`, then the rest (`NO_RENDER` and the mods' own) in the order they came.
fn order(rt: &Rt, ty: V) -> usize {
    vanilla_type(rt, ty)
        .filter(|&i| i != NO_RENDER_TYPE)
        .unwrap_or(TYPES.len())
}

/// Render type `ty`'s queue in the engine, made in its place when it is new.
fn queue(rt: &mut Rt, e: V, ty: V) -> V {
    let (types, queues) = (rt.get_field(e, "$types"), rt.get_field(e, "$queues"));
    let known = items(rt, types);
    if let Some(i) = known.iter().position(|t| *t == ty) {
        return items(rt, queues)[i];
    }
    let rank = order(rt, ty);
    let at = known
        .iter()
        .position(|t| order(rt, *t) > rank)
        .unwrap_or(known.len());
    let q = crate::jutil::new_list(rt, Vec::new());
    if let Some(l) = items_mut(rt, types) {
        l.insert(at, ty);
    }
    if let Some(l) = items_mut(rt, queues) {
        l.insert(at, q);
    }
    q
}

/// `ParticleEngine.render`: each render type's particles but `NO_RENDER`'s, in order, each drawing
/// itself (`Particle.render`) into a buffer of its type's: the faces they make go to the game,
/// textured from the type's atlas (or what a mod's own type binds) and blended after the
/// translucent blocks when the type is translucent. The camera is the frame's
/// ([`crate::camera::prepare`]).
pub(crate) fn render(rt: &mut Rt, partial: f32) {
    let Some(e) = engine_if_any(rt) else { return };
    let types = items(rt, rt.get_field(e, "$types"));
    let queues = items(rt, rt.get_field(e, "$queues"));
    let camera = crate::camera::main(rt);
    for (ty, q) in types.into_iter().zip(queues) {
        let particles = items(rt, q);
        let vanilla = vanilla_type(rt, ty);
        if particles.is_empty() || vanilla == Some(NO_RENDER_TYPE) {
            continue;
        }
        let Some((texture, translucent)) = begin_batch(rt, ty, vanilla) else {
            continue;
        };
        let buffer = buffer(rt, &texture, translucent);
        for p in particles {
            virt(rt, RENDER, p, &[buffer, camera, V::F(partial)]);
            if rt.thrown.is_some() {
                rt.report_thrown("a particle's render");
                crate::vertices::drop_unfinished(rt, buffer);
            }
        }
        let pseudo = rt.get_field(buffer, "$type");
        crate::vertices::flush(rt, Some(pseudo));
    }
}

/// What render type `ty`'s batch draws with: its texture and whether it is translucent
/// (`isTranslucent`). Vanilla's bind the particle atlas, but `TERRAIN_SHEET` the block atlas and
/// `CUSTOM` none (its particles keep the particle atlas here). A mod's own type's `begin` binds its
/// own (`RenderSystem.setShaderTexture`); None when it gives no buffer, which draws nothing.
fn begin_batch(rt: &mut Rt, ty: V, vanilla: Option<usize>) -> Option<(String, bool)> {
    match vanilla {
        Some(0) => return Some((BLOCK_ATLAS.into(), true)),
        Some(1 | 2) => return Some((PARTICLE_ATLAS.into(), false)),
        Some(_) => return Some((PARTICLE_ATLAS.into(), true)),
        None => {}
    }
    let saved = rt.interned.get(crate::screens::SHADER_TEXTURE).copied();
    rt.interned.remove(crate::screens::SHADER_TEXTURE);
    let tesselator = crate::api::call(rt, TESSELATOR, &[]);
    let textures = rt.intern(
        "client/net/minecraft/client/renderer/texture/TextureManager".into(),
        |rt| rt.new_object("net/minecraft/client/renderer/texture/TextureManager"),
    );
    let builder = crate::menus::virt(rt, BEGIN, ty, &[tesselator, textures]);
    let threw = rt.thrown.is_some();
    rt.report_thrown("a particle render type's begin");
    let bound = rt
        .interned
        .get(crate::screens::SHADER_TEXTURE)
        .and_then(|t| rt.str_of(*t))
        .map(str::to_owned);
    match saved {
        Some(t) => rt.interned.insert(crate::screens::SHADER_TEXTURE.into(), t),
        None => rt.interned.remove(crate::screens::SHADER_TEXTURE),
    };
    if threw || builder.r() == 0 {
        return None;
    }
    let translucent = crate::menus::virt(rt, IS_TRANSLUCENT, ty, &[]).i() != 0;
    rt.report_thrown("a particle render type's isTranslucent");
    Some((bound.unwrap_or_else(|| PARTICLE_ATLAS.into()), translucent))
}

/// The buffer a batch of particles draws into: a world consumer whose faces go to the game as the
/// mods' particles' ([`crate::vertices`]).
fn buffer(rt: &mut Rt, texture: &str, translucent: bool) -> V {
    let texture = texture.to_owned();
    rt.intern(
        format!("compat:particle-buffer/{}/{texture}", u8::from(translucent)),
        move |rt| {
            let t = rt.string(&texture);
            let ty = crate::render::textured_render_type(rt, "particle", t);
            let c = rt.new_object("com/mojang/blaze3d/vertex/VertexConsumer");
            rt.put_field(c, "$type", ty);
            rt.put_field(c, "$faces", V::I(1));
            rt.put_field(c, "$particles", V::I(1));
            rt.put_field(c, "$translucent", V::I(i32::from(translucent)));
            c
        },
    )
}

// ---- render types, camera modes, sprite sets ----

/// Vanilla's render type `name` (`ParticleRenderType.PARTICLE_SHEET_TRANSLUCENT`): one object each.
fn render_type(rt: &mut Rt, name: &str) -> V {
    let n = name.to_owned();
    rt.intern(format!("particle-render-type/{name}"), move |rt| {
        let t = rt.new_object(RENDER_TYPE);
        let s = rt.string(&n);
        rt.put_field(t, "$name", s);
        t
    })
}

/// Which of vanilla's render types `ty` is ([`TYPES`]), or None for a mod's own.
fn vanilla_type(rt: &Rt, ty: V) -> Option<usize> {
    if rt.class_of(ty) != RENDER_TYPE {
        return None;
    }
    let name = rt.str_of(rt.get_field(ty, "$name"))?;
    TYPES.iter().position(|n| *n == name)
}

/// `FacingCameraMode.LOOKAT_XYZ` or `LOOKAT_Y`: one object each.
fn facing_mode(rt: &mut Rt, name: &str) -> V {
    let n = name.to_owned();
    rt.intern(format!("particle-facing/{name}"), move |rt| {
        let m = rt.new_object(FACING);
        let s = rt.string(&n);
        rt.put_field(m, "$name", s);
        m
    })
}

/// `ParticleRenderType`'s and `FacingCameraMode`'s constants.
pub(crate) fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    match owner {
        RENDER_TYPE if TYPES.contains(&field) => Some(render_type(rt, field)),
        FACING if matches!(field, "LOOKAT_XYZ" | "LOOKAT_Y") => Some(facing_mode(rt, field)),
        _ => None,
    }
}

/// `mode.setRotation(q, camera, partial)`: vanilla's two face the camera (`LOOKAT_XYZ`: its whole
/// rotation; `LOOKAT_Y`: turned about the vertical only); a mod's own mode sets it its way.
fn set_rotation(rt: &mut Rt, mode: V, q: V, camera: V, partial: f32) {
    let vanilla = match rt.class_of(mode) {
        FACING => rt.str_of(rt.get_field(mode, "$name")).map(str::to_owned),
        _ => None,
    };
    let Some(name) = vanilla else {
        rt.call_functional(SET_ROTATION, mode, &[q, camera, V::F(partial)]);
        return;
    };
    let rotation = crate::api::call(rt, CAMERA_ROTATION, &[camera]);
    let [x, y, z, w] = ["x", "y", "z", "w"].map(|f| rt.get_field(rotation, f).f());
    let set = if name == "LOOKAT_Y" {
        [0.0, y, 0.0, w]
    } else {
        [x, y, z, w]
    };
    for (f, c) in ["x", "y", "z", "w"].into_iter().zip(set) {
        rt.put_field(q, f, V::F(c));
    }
}

/// The sprite set of particle type `id` (`ParticleEngine.MutableSpriteSet`): one object a type,
/// its sprites the textures the type's particle description lists
/// (`assets/<namespace>/particles/<path>.json`), read when first asked for.
fn sprite_set(rt: &mut Rt, id: &str) -> V {
    let id = id.to_owned();
    rt.intern(format!("particle-sprites/{id}"), move |rt| {
        let set = rt.new_object(SPRITE_SET);
        let s = rt.string(&id);
        rt.put_field(set, "$id", s);
        set
    })
}

/// The sprites of `set`: the textures its type's description lists, each the sprite
/// `<namespace>:particle/<path>` (`minecraft:missingno` alone when it lists none, as an atlas
/// without the texture draws it).
fn sprites(rt: &mut Rt, set: V) -> Vec<V> {
    let bound = rt.get_field(set, "$sprites");
    if bound.r() != 0 {
        return items(rt, bound);
    }
    let id = rt
        .str_of(rt.get_field(set, "$id"))
        .unwrap_or("minecraft:missingno")
        .to_owned();
    let (ns, path) = id.split_once(':').unwrap_or(("minecraft", &id));
    let file = format!("assets/{ns}/particles/{path}.json");
    let Some(bytes) = rt.host().and_then(|h| h.resource(&file)) else {
        let missing = crate::render::sprite_named(rt, "minecraft:missingno");
        return vec![missing];
    };
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    let mut names: Vec<String> = json["textures"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t.as_str())
        .map(|t| match t.split_once(':') {
            Some((ns, path)) => format!("{ns}:particle/{path}"),
            None => format!("minecraft:particle/{t}"),
        })
        .collect();
    if names.is_empty() {
        crate::glue::trace_once(rt, &format!("particle-sprites/{id}"), |_| {
            format!("client: {file} lists no textures")
        });
        names.push("minecraft:missingno".into());
    }
    let sprites: Vec<V> = names
        .iter()
        .map(|n| crate::render::sprite_named(rt, n))
        .collect();
    let list = crate::jutil::new_list(rt, sprites.clone());
    rt.put_field(set, "$sprites", list);
    sprites
}

/// `MutableSpriteSet.get(age, lifetime)` (the frame for the age, `age * (n - 1) / lifetime`) and
/// `get(random)` (one at random), throwing as Java's arithmetic and lists do.
fn sprite_set_get(rt: &mut Rt, set: V, desc: &str, args: &[V]) -> V {
    let all = sprites(rt, set);
    let n = all.len() as i32;
    let i = if desc.starts_with("(II)") {
        let (age, max) = (arg(args, 1).i(), arg(args, 2).i());
        match age.wrapping_mul(n - 1).checked_div(max) {
            Some(i) => i,
            None if max == 0 => {
                crate::jlang::throw_with(rt, "java/lang/ArithmeticException", "/ by zero");
                return V::NULL;
            }
            None => i32::MIN,
        }
    } else {
        crate::menus::virt(rt, NEXT_INT, arg(args, 1), &[V::I(n)]).i()
    };
    match usize::try_from(i).ok().and_then(|i| all.get(i)) {
        Some(s) => *s,
        None => {
            crate::jlang::throw_with(
                rt,
                "java/lang/IndexOutOfBoundsException",
                &format!("Index {i} out of bounds for length {n}"),
            );
            V::NULL
        }
    }
}

/// `set.get(...)` on any sprite set: the engine's own, or a mod's.
fn get_sprite(rt: &mut Rt, set: V, key: &'static str, args: &[V]) -> V {
    if rt.class_of(set) == SPRITE_SET {
        let desc = key.rsplit_once('.').map_or("", |(_, d)| d);
        let mut all = vec![set];
        all.extend_from_slice(args);
        return sprite_set_get(rt, set, desc, &all);
    }
    rt.call_functional(key, set, args)
}

// ---- the particle classes ----

/// The modelled class a particle's class extends, nearest first: its own when it is one, else the
/// first up its superclasses (a mod's, then vanilla's). None for a class that extends none of them.
fn base_of(rt: &Rt, this: V) -> Option<&'static str> {
    if this.r() == 0 {
        return None;
    }
    let mut c = rt.class_of(this);
    for _ in 0..64 {
        if matches!(
            c,
            PARTICLE | SINGLE_QUAD | TEXTURE_SHEET | SIMPLE_ANIMATED | NO_RENDER
        ) {
            return Some(c);
        }
        c = rt
            .supers
            .get(c)
            .copied()
            .or_else(|| crate::api::api_super(c))?;
    }
    None
}

/// The superclasses of the modelled classes (for [`crate::api::api_super`]).
pub(crate) fn api_super(class: &str) -> Option<&'static str> {
    match class {
        SIMPLE_ANIMATED => Some(TEXTURE_SHEET),
        TEXTURE_SHEET => Some(SINGLE_QUAD),
        SINGLE_QUAD | NO_RENDER => Some(PARTICLE),
        _ => None,
    }
}

/// Whether modelled class `class` itself declares method `name.desc` (with code: its abstract
/// methods are the subclass's).
fn declares(class: &str, name: &str, desc: &str) -> bool {
    match class {
        PARTICLE => {
            matches!(
                name,
                "setPower"
                    | "setParticleSpeed"
                    | "scale"
                    | "setAlpha"
                    | "setLifetime"
                    | "getLifetime"
                    | "tick"
                    | "remove"
                    | "setSize"
                    | "setPos"
                    | "move"
                    | "setLocationFromBoundingbox"
                    | "getLightColor"
                    | "isAlive"
                    | "getBoundingBox"
                    | "setBoundingBox"
                    | "getParticleGroup"
                    | "getRenderBoundingBox"
                    | "getPos"
            ) || (name == "setColor" && desc == "(FFF)V")
        }
        SINGLE_QUAD => matches!(
            name,
            "getFacingCameraMode"
                | "render"
                | "renderRotatedQuad"
                | "getRenderBoundingBox"
                | "getQuadSize"
                | "scale"
        ),
        TEXTURE_SHEET => matches!(
            name,
            "setSprite" | "getU0" | "getU1" | "getV0" | "getV1" | "pickSprite" | "setSpriteFromAge"
        ),
        SIMPLE_ANIMATED => {
            matches!(
                name,
                "setFadeColor" | "getRenderType" | "tick" | "getLightColor"
            ) || (name == "setColor" && desc == "(I)V")
        }
        NO_RENDER => matches!(name, "render" | "getRenderType"),
        _ => false,
    }
}

/// The modelled class that declares `name.desc` for a particle of modelled class `class`: its own,
/// else the nearest above it that does.
fn implementer(class: &'static str, name: &str, desc: &str) -> Option<&'static str> {
    let mut c = class;
    loop {
        if declares(c, name, desc) {
            return Some(c);
        }
        c = api_super(c)?;
    }
}

/// Whether modelled class `class` is `above` or extends it.
fn extends(class: &'static str, above: &str) -> bool {
    let mut c = class;
    loop {
        if c == above {
            return true;
        }
        let Some(up) = api_super(c) else { return false };
        c = up;
    }
}

/// `this.<key>(args)` as vanilla's code calls it on a particle: the mod's own method when its class
/// has one, else the model's.
fn virt(rt: &mut Rt, key: &'static str, this: V, args: &[V]) -> V {
    let class = rt.class_of(this);
    let short = key.split_once('.').map_or(key, |(_, r)| r);
    if rt.is_mod_class(class) && rt.overrides.is_some_and(|o| o(class, short)) {
        return rt.call_functional(key, this, args);
    }
    let mut all = Vec::with_capacity(args.len() + 1);
    all.push(this);
    all.extend_from_slice(args);
    match call(rt, key, &all) {
        Some(v) => v,
        None => crate::api::call(rt, key, &all),
    }
}

fn get_d(rt: &Rt, p: V, f: &'static str) -> f64 {
    rt.get_field(p, f).d()
}
fn get_f(rt: &Rt, p: V, f: &'static str) -> f32 {
    rt.get_field(p, f).f()
}
fn get_i(rt: &Rt, p: V, f: &'static str) -> i32 {
    rt.get_field(p, f).i()
}
fn set_d(rt: &mut Rt, p: V, f: &'static str, v: f64) {
    rt.put_field(p, f, V::D(v));
}
fn set_f(rt: &mut Rt, p: V, f: &'static str, v: f32) {
    rt.put_field(p, f, V::F(v));
}
fn set_i(rt: &mut Rt, p: V, f: &'static str, v: i32) {
    rt.put_field(p, f, V::I(v));
}

/// The particle's box (`getBoundingBox()`), as its corners.
fn bounding_box(rt: &mut Rt, p: V) -> [f64; 6] {
    let b = virt(rt, GET_BOX, p, &[]);
    crate::shapes::aabb_of(rt, b)
}

fn set_bounding_box(rt: &mut Rt, p: V, b: [f64; 6]) {
    let sorted = [
        b[0].min(b[3]),
        b[1].min(b[4]),
        b[2].min(b[5]),
        b[0].max(b[3]),
        b[1].max(b[4]),
        b[2].max(b[5]),
    ];
    let o = crate::shapes::aabb(rt, sorted);
    virt(rt, SET_BOX, p, &[o]);
}

/// `Particle(level, x, y, z)`: its fields as vanilla initialises them, then the constructor's own
/// (`setSize(0.2F, 0.2F)`, `setPos`, the old position, a lifetime of 4 to 40 ticks).
fn init(rt: &mut Rt, p: V, level: V, [x, y, z]: [f64; 3]) {
    rt.put_field(p, "level", level);
    let empty = crate::shapes::aabb(rt, [0.0; 6]);
    rt.put_field(p, "$bb", empty);
    let random = crate::api::call(
        rt,
        "net/minecraft/util/RandomSource.create.()Lnet/minecraft/util/RandomSource;",
        &[],
    );
    rt.put_field(p, "random", random);
    for f in ["xo", "yo", "zo", "x", "y", "z", "xd", "yd", "zd"] {
        set_d(rt, p, f, 0.0);
    }
    for (f, v) in [
        ("onGround", 0),
        ("hasPhysics", 1),
        ("$stoppedByCollision", 0),
        ("removed", 0),
        ("age", 0),
        ("lifetime", 0),
        ("speedUpWhenYMotionIsBlocked", 0),
    ] {
        set_i(rt, p, f, v);
    }
    for (f, v) in [
        ("bbWidth", 0.6),
        ("bbHeight", 1.8),
        ("gravity", 0.0),
        ("rCol", 1.0),
        ("gCol", 1.0),
        ("bCol", 1.0),
        ("alpha", 1.0),
        ("roll", 0.0),
        ("oRoll", 0.0),
        ("friction", 0.98),
    ] {
        set_f(rt, p, f, v);
    }
    virt(rt, SET_SIZE, p, &[V::F(0.2), V::F(0.2)]);
    virt(rt, SET_POS, p, &[V::D(x), V::D(y), V::D(z)]);
    set_d(rt, p, "xo", x);
    set_d(rt, p, "yo", y);
    set_d(rt, p, "zo", z);
    let r = crate::menus::virt(rt, NEXT_FLOAT, random, &[]).f();
    set_i(rt, p, "lifetime", (4.0 / (r * 0.9 + 0.1)) as i32);
}

/// `Math.random()`, as vanilla's particles draw it.
fn math_random(rt: &mut Rt) -> f64 {
    crate::api::call(rt, "java/lang/Math.random.()D", &[]).d()
}

/// `Particle(level, x, y, z, xSpeed, ySpeed, zSpeed)`: the speed given, nudged at random (each axis
/// up to 0.4 either way), then scaled to a random length about 0.15 and lifted by 0.1.
fn init_moving(rt: &mut Rt, p: V, level: V, at: [f64; 3], speed: [f64; 3]) {
    init(rt, p, level, at);
    let spread = f64::from(0.4f32);
    let mut d = [0.0; 3];
    for (i, s) in speed.into_iter().enumerate() {
        d[i] = s + (math_random(rt) * 2.0 - 1.0) * spread;
    }
    let length = (math_random(rt) + math_random(rt) + 1.0) * f64::from(0.15f32);
    let norm = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    set_d(rt, p, "xd", d[0] / norm * length * spread);
    set_d(
        rt,
        p,
        "yd",
        d[1] / norm * length * spread + f64::from(0.1f32),
    );
    set_d(rt, p, "zd", d[2] / norm * length * spread);
}

/// The constructors, by the class whose constructor the mod's `super(...)` names.
fn construct(rt: &mut Rt, owner: &str, desc: &str, args: &[V]) -> V {
    let a = |i| arg(args, i);
    let (p, level, at) = (a(0), a(1), [a(2).d(), a(3).d(), a(4).d()]);
    if desc.contains("DDDDDD") {
        init_moving(rt, p, level, at, [a(5).d(), a(6).d(), a(7).d()]);
    } else {
        init(rt, p, level, at);
    }
    if matches!(owner, SINGLE_QUAD | TEXTURE_SHEET | SIMPLE_ANIMATED) {
        // `quadSize = 0.1F * (random.nextFloat() * 0.5F + 0.5F) * 2.0F`, once `Particle`'s is done.
        let random = rt.get_field(p, "random");
        let r = crate::menus::virt(rt, NEXT_FLOAT, random, &[]).f();
        set_f(rt, p, "quadSize", 0.1 * (r * 0.5 + 0.5) * 2.0);
    }
    if matches!(owner, TEXTURE_SHEET | SIMPLE_ANIMATED) {
        rt.put_field(p, "sprite", V::NULL);
    }
    if owner == SIMPLE_ANIMATED {
        // `(level, x, y, z, sprites, gravity)`.
        set_f(rt, p, "friction", 0.91);
        set_f(rt, p, "gravity", a(6).f());
        rt.put_field(p, "sprites", a(5));
        for f in ["$fadeR", "$fadeG", "$fadeB"] {
            set_f(rt, p, f, 0.0);
        }
        set_i(rt, p, "$hasFade", 0);
    }
    V::Void
}

/// `Particle.move(x, y, z)`: unless a fall stopped it for good, the particle moves by `(x, y, z)`,
/// as far as the level's blocks let its box when it has physics (`Entity.collideBoundingBox`), and
/// lands, stops or loses its speed across where they stopped it.
fn move_by(rt: &mut Rt, p: V, wanted: [f64; 3]) {
    if get_i(rt, p, "$stoppedByCollision") != 0 {
        return;
    }
    let mut d = wanted;
    let length2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    if get_i(rt, p, "hasPhysics") != 0 && d != [0.0; 3] && length2 < 100.0 * 100.0 {
        let b = bounding_box(rt, p);
        d = collide(rt, b, d);
    }
    if d != [0.0; 3] {
        let b = bounding_box(rt, p);
        set_bounding_box(
            rt,
            p,
            [
                b[0] + d[0],
                b[1] + d[1],
                b[2] + d[2],
                b[3] + d[0],
                b[4] + d[1],
                b[5] + d[2],
            ],
        );
        virt(rt, SET_LOCATION, p, &[]);
    }
    let least = f64::from(1.0e-5f32);
    if wanted[1].abs() >= least && d[1].abs() < least {
        set_i(rt, p, "$stoppedByCollision", 1);
    }
    set_i(
        rt,
        p,
        "onGround",
        i32::from(wanted[1] != d[1] && wanted[1] < 0.0),
    );
    if wanted[0] != d[0] {
        set_d(rt, p, "xd", 0.0);
    }
    if wanted[2] != d[2] {
        set_d(rt, p, "zd", 0.0);
    }
}

thread_local! {
    /// While the engine ticks: each block's collision boxes the particles asked for, so a cloud of
    /// them asks the game once a block.
    static BOXES: RefCell<Option<crate::HashMap<[i32; 3], Rc<[[f64; 6]]>>>> = const { RefCell::new(None) };
}

/// The collision boxes of the block at `pos`, relative to it.
fn block_boxes(rt: &Rt, pos: [i32; 3]) -> Rc<[[f64; 6]]> {
    if let Some(b) = BOXES.with(|c| c.borrow().as_ref().and_then(|m| m.get(&pos).cloned())) {
        return b;
    }
    let boxes: Rc<[[f64; 6]]> = rt
        .host()
        .and_then(|h| h.collision_boxes(pos))
        .unwrap_or_default()
        .into();
    BOXES.with(|c| {
        if let Some(m) = c.borrow_mut().as_mut() {
            m.insert(pos, boxes.clone());
        }
    });
    boxes
}

const EPSILON: f64 = 1.0e-7;

fn intersects(a: &[f64; 6], b: &[f64; 6]) -> bool {
    a[0] < b[3] && a[3] > b[0] && a[1] < b[4] && a[4] > b[1] && a[2] < b[5] && a[5] > b[2]
}

/// `Entity.collideBoundingBox(null, motion, box, level, [])`: `motion` cut short where box `b` meets
/// the level's blocks (`level.getBlockCollisions` over the box stretched along it), along y, then
/// along the shorter of x and z, then the other (`collideWithShapes`).
fn collide(rt: &Rt, b: [f64; 6], motion: [f64; 3]) -> [f64; 3] {
    let mut area = b;
    for i in 0..3 {
        if motion[i] < 0.0 {
            area[i] += motion[i];
        } else {
            area[i + 3] += motion[i];
        }
    }
    // `BlockCollisions`: the blocks in the area and a block round it, leaving out the corners and
    // edges of that rim, and on its faces all but the shapes taller than a block (which are the
    // only ones that can reach in).
    let lo = [0, 1, 2].map(|i| (area[i] - EPSILON).floor() as i32 - 1);
    let hi = [0, 1, 2].map(|i| (area[i + 3] + EPSILON).floor() as i32 + 1);
    let mut shapes: Vec<[f64; 6]> = Vec::new();
    for x in lo[0]..=hi[0] {
        for y in lo[1]..=hi[1] {
            for z in lo[2]..=hi[2] {
                let rim = u8::from(x == lo[0] || x == hi[0])
                    + u8::from(y == lo[1] || y == hi[1])
                    + u8::from(z == lo[2] || z == hi[2]);
                if rim >= 2 {
                    continue;
                }
                let o = [f64::from(x), f64::from(y), f64::from(z)];
                for s in block_boxes(rt, [x, y, z]).iter() {
                    let w = [
                        s[0] + o[0],
                        s[1] + o[1],
                        s[2] + o[2],
                        s[3] + o[0],
                        s[4] + o[1],
                        s[5] + o[2],
                    ];
                    if intersects(&w, &area) {
                        shapes.push(w);
                    }
                }
            }
        }
    }
    if shapes.is_empty() {
        return motion;
    }
    let mut b = b;
    let mut d = motion;
    let shift = |b: &mut [f64; 6], axis: usize, by: f64| {
        b[axis] += by;
        b[axis + 3] += by;
    };
    if d[1] != 0.0 {
        d[1] = collide_axis(1, &b, &shapes, d[1]);
        if d[1] != 0.0 {
            shift(&mut b, 1, d[1]);
        }
    }
    let z_first = d[0].abs() < d[2].abs();
    if z_first && d[2] != 0.0 {
        d[2] = collide_axis(2, &b, &shapes, d[2]);
        if d[2] != 0.0 {
            shift(&mut b, 2, d[2]);
        }
    }
    if d[0] != 0.0 {
        d[0] = collide_axis(0, &b, &shapes, d[0]);
        if !z_first && d[0] != 0.0 {
            shift(&mut b, 0, d[0]);
        }
    }
    if !z_first && d[2] != 0.0 {
        d[2] = collide_axis(2, &b, &shapes, d[2]);
    }
    d
}

/// `Shapes.collide(axis, box, shapes, distance)`: how far box `b` goes along `axis` toward
/// `distance` before it meets one of `shapes` (each as `VoxelShape.collide` meets a box: one ahead
/// of it, overlapping it across the axis by more than vanilla's epsilon).
fn collide_axis(axis: usize, b: &[f64; 6], shapes: &[[f64; 6]], mut distance: f64) -> f64 {
    let (u, w) = ((axis + 1) % 3, (axis + 2) % 3);
    for s in shapes {
        if distance.abs() < EPSILON {
            return 0.0;
        }
        let across = |i: usize| b[i] + EPSILON < s[i + 3] && b[i + 3] - EPSILON >= s[i];
        if !across(u) || !across(w) {
            continue;
        }
        if distance > 0.0 && b[axis + 3] - EPSILON < s[axis] {
            let gap = s[axis] - b[axis + 3];
            if gap >= -EPSILON {
                distance = distance.min(gap);
            }
        } else if distance < 0.0 && b[axis] + EPSILON >= s[axis + 3] {
            let gap = s[axis + 3] - b[axis];
            if gap <= EPSILON {
                distance = distance.max(gap);
            }
        }
    }
    distance
}

/// `Particle.tick`: the old position kept, a year older, gone at its lifetime; else falling by its
/// gravity, moved, faster along the ground where it can't fall (`speedUpWhenYMotionIsBlocked`),
/// slowed by its friction, and more on the ground.
fn particle_tick(rt: &mut Rt, p: V) {
    for (old, now) in [("xo", "x"), ("yo", "y"), ("zo", "z")] {
        let v = get_d(rt, p, now);
        set_d(rt, p, old, v);
    }
    let age = get_i(rt, p, "age");
    set_i(rt, p, "age", age.wrapping_add(1));
    if age >= get_i(rt, p, "lifetime") {
        virt(rt, REMOVE, p, &[]);
        return;
    }
    let yd = get_d(rt, p, "yd") - 0.04 * f64::from(get_f(rt, p, "gravity"));
    set_d(rt, p, "yd", yd);
    let d = [get_d(rt, p, "xd"), yd, get_d(rt, p, "zd")];
    virt(rt, MOVE, p, &[V::D(d[0]), V::D(d[1]), V::D(d[2])]);
    if get_i(rt, p, "speedUpWhenYMotionIsBlocked") != 0 && get_d(rt, p, "y") == get_d(rt, p, "yo") {
        for f in ["xd", "zd"] {
            let v = get_d(rt, p, f) * 1.1;
            set_d(rt, p, f, v);
        }
    }
    let friction = f64::from(get_f(rt, p, "friction"));
    for f in ["xd", "yd", "zd"] {
        let v = get_d(rt, p, f) * friction;
        set_d(rt, p, f, v);
    }
    if get_i(rt, p, "onGround") != 0 {
        for f in ["xd", "zd"] {
            let v = get_d(rt, p, f) * f64::from(0.7f32);
            set_d(rt, p, f, v);
        }
    }
}

/// `SimpleAnimatedParticle.tick`: `Particle`'s, then the frame for its age; past half its life it
/// fades, and drifts a fifth of the way to its fade colour each tick when it has one.
fn animated_tick(rt: &mut Rt, p: V) {
    particle_tick(rt, p);
    let sprites = rt.get_field(p, "sprites");
    virt(rt, SPRITE_FROM_AGE, p, &[sprites]);
    let (age, lifetime) = (get_i(rt, p, "age"), get_i(rt, p, "lifetime"));
    if age > lifetime / 2 {
        let alpha = 1.0 - (age as f32 - (lifetime / 2) as f32) / lifetime as f32;
        virt(rt, SET_ALPHA, p, &[V::F(alpha)]);
        if get_i(rt, p, "$hasFade") != 0 {
            for (c, fade) in [("rCol", "$fadeR"), ("gCol", "$fadeG"), ("bCol", "$fadeB")] {
                let (now, to) = (get_f(rt, p, c), get_f(rt, p, fade));
                set_f(rt, p, c, now + (to - now) * 0.2);
            }
        }
    }
}

/// JOML's `Quaternionf.transform` (`Vector3f.rotate`): `v` turned by `q`, as JOML computes it.
fn rotate([qx, qy, qz, qw]: [f32; 4], [x, y, z]: [f32; 3]) -> [f32; 3] {
    let (xx, yy, zz, ww) = (qx * qx, qy * qy, qz * qz, qw * qw);
    let (xy, xz, yz, xw, zw, yw) = (qx * qy, qx * qz, qy * qz, qx * qw, qz * qw, qy * qw);
    let k = 1.0 / (xx + yy + zz + ww);
    [
        (xx - yy - zz + ww) * k * x + (2.0 * (xy - zw) * k * y + 2.0 * (xz + yw) * k * z),
        2.0 * (xy + zw) * k * x + ((yy - xx - zz + ww) * k * y + 2.0 * (yz - xw) * k * z),
        2.0 * (xz - yw) * k * x + (2.0 * (yz + xw) * k * y + (zz - xx - yy + ww) * k * z),
    ]
}

/// `SingleQuadParticle.render`: a quad turned to face the camera as its mode says, rolled by its
/// roll, drawn where it is this frame.
fn single_quad_render(rt: &mut Rt, p: V, buffer: V, camera: V, partial: f32) {
    let mode = virt(rt, FACING_MODE, p, &[]);
    let q = crate::render::new_quat(rt, [0.0, 0.0, 0.0, 1.0]);
    set_rotation(rt, mode, q, camera, partial);
    let (roll, o_roll) = (get_f(rt, p, "roll"), get_f(rt, p, "oRoll"));
    if roll != 0.0 {
        crate::api::call(
            rt,
            "org/joml/Quaternionf.rotateZ.(F)Lorg/joml/Quaternionf;",
            &[q, V::F(o_roll + partial * (roll - o_roll))],
        );
    }
    virt(rt, ROTATED_QUAD, p, &[buffer, camera, q, V::F(partial)]);
}

/// `renderRotatedQuad(buffer, camera, q, partial)`: where the particle is this frame, from the camera.
fn rotated_quad(rt: &mut Rt, p: V, buffer: V, camera: V, q: V, partial: f32) {
    let at = crate::api::call(rt, CAMERA_POSITION, &[camera]);
    let cam = crate::api_common::xyz(rt, at);
    let t = f64::from(partial);
    let lerp = |rt: &Rt, old: &'static str, now: &'static str| {
        let (o, n) = (get_d(rt, p, old), get_d(rt, p, now));
        o + t * (n - o)
    };
    let x = (lerp(rt, "xo", "x") - cam[0]) as f32;
    let y = (lerp(rt, "yo", "y") - cam[1]) as f32;
    let z = (lerp(rt, "zo", "z") - cam[2]) as f32;
    virt(
        rt,
        ROTATED_QUAD_AT,
        p,
        &[buffer, q, V::F(x), V::F(y), V::F(z), V::F(partial)],
    );
}

/// `renderRotatedQuad(buffer, q, x, y, z, partial)`: the quad's four corners round `(x, y, z)`, its
/// size across, turned by `q`, each with its sprite's corner, the particle's colour and its light.
fn rotated_quad_at(rt: &mut Rt, p: V, buffer: V, q: V, at: [f32; 3], partial: f32) {
    let size = virt(rt, QUAD_SIZE, p, &[V::F(partial)]).f();
    let [u0, u1, v0, v1] = [U0, U1, V0, V1].map(|k| virt(rt, k, p, &[]).f());
    let light = virt(rt, LIGHT, p, &[V::F(partial)]).i();
    let q = ["x", "y", "z", "w"].map(|f| rt.get_field(q, f).f());
    let color = [
        get_f(rt, p, "rCol"),
        get_f(rt, p, "gCol"),
        get_f(rt, p, "bCol"),
        get_f(rt, p, "alpha"),
    ];
    for (corner, uv) in [
        ([1.0, -1.0], [u1, v1]),
        ([1.0, 1.0], [u1, v0]),
        ([-1.0, 1.0], [u0, v0]),
        ([-1.0, -1.0], [u0, v1]),
    ] {
        let r = rotate(q, [corner[0], corner[1], 0.0]);
        let v = [
            r[0] * size + at[0],
            r[1] * size + at[1],
            r[2] * size + at[2],
        ];
        vertex(rt, buffer, v, uv, color, light);
    }
}

/// One corner: `buffer.addVertex(v).setUv(uv).setColor(r, g, b, a).setLight(light)`.
fn vertex(rt: &mut Rt, buffer: V, v: [f32; 3], uv: [f32; 2], [r, g, b, a]: [f32; 4], light: i32) {
    if rt.get_field(buffer, "$type").r() != 0 {
        let c = |f: f32| ((f * 255.0) as i32).clamp(0, 255) as u32;
        let argb = (c(a) << 24 | c(r) << 16 | c(g) << 8 | c(b)) as i32;
        crate::vertices::vertex(rt, buffer, v, uv, argb, light);
        return;
    }
    // A buffer of the mod's own (one wrapping the engine's): its own calls.
    let next = crate::menus::virt(
        rt,
        ADD_VERTEX,
        buffer,
        &[V::F(v[0]), V::F(v[1]), V::F(v[2])],
    );
    let next = crate::menus::virt(rt, SET_UV, next, &[V::F(uv[0]), V::F(uv[1])]);
    let next = crate::menus::virt(
        rt,
        SET_VERTEX_COLOR,
        next,
        &[V::F(r), V::F(g), V::F(b), V::F(a)],
    );
    crate::menus::virt(rt, SET_LIGHT, next, &[V::I(light)]);
}

/// `0xRRGGBB` as `setColor(int)` reads it: each channel over 255.
fn channels(rgb: i32) -> [f32; 3] {
    [
        ((rgb & 0xFF0000) >> 16) as f32 / 255.0,
        ((rgb & 0xFF00) >> 8) as f32 / 255.0,
        (rgb & 0xFF) as f32 / 255.0,
    ]
}

/// `Particle.setSize(width, height)`: when either changes, the box keeps its centre and floor and
/// takes the new size.
fn set_size(rt: &mut Rt, p: V, width: f32, height: f32) {
    if width == get_f(rt, p, "bbWidth") && height == get_f(rt, p, "bbHeight") {
        return;
    }
    set_f(rt, p, "bbWidth", width);
    set_f(rt, p, "bbHeight", height);
    let b = bounding_box(rt, p);
    let (w, h) = (f64::from(width), f64::from(height));
    let x = (b[0] + b[3] - w) / 2.0;
    let z = (b[2] + b[5] - w) / 2.0;
    set_bounding_box(rt, p, [x, b[1], z, x + w, b[1] + h, z + w]);
}

/// `Particle.setPos(x, y, z)`: there, its box round it (its width across, its height up).
fn set_pos(rt: &mut Rt, p: V, [x, y, z]: [f64; 3]) {
    set_d(rt, p, "x", x);
    set_d(rt, p, "y", y);
    set_d(rt, p, "z", z);
    let f = f64::from(get_f(rt, p, "bbWidth") / 2.0);
    let h = f64::from(get_f(rt, p, "bbHeight"));
    set_bounding_box(rt, p, [x - f, y, z - f, x + f, y + h, z + f]);
}

/// The class a particle method's key names, as the module's own constant (keys are `'static`).
fn modelled(owner: &str) -> Option<&'static str> {
    [
        PARTICLE,
        SINGLE_QUAD,
        TEXTURE_SHEET,
        SIMPLE_ANIMATED,
        NO_RENDER,
    ]
    .into_iter()
    .find(|c| *c == owner)
}

/// Modelled class `class`'s own method `name.desc` on particle `args[0]`; None for one it doesn't
/// declare.
fn particle_method(rt: &mut Rt, class: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| arg(args, i);
    let p = a(0);
    if p.r() == 0 {
        rt.throw_new("java/lang/NullPointerException");
        return Some(V::Void);
    }
    Some(match (class, name) {
        // `setPower(multiplier)`: its speed times it, the upward 0.1 it was given kept.
        (PARTICLE, "setPower") => {
            let m = f64::from(a(1).f());
            let lift = f64::from(0.1f32);
            let (xd, yd, zd) = (get_d(rt, p, "xd"), get_d(rt, p, "yd"), get_d(rt, p, "zd"));
            set_d(rt, p, "xd", xd * m);
            set_d(rt, p, "yd", (yd - lift) * m + lift);
            set_d(rt, p, "zd", zd * m);
            p
        }
        (PARTICLE, "setParticleSpeed") => {
            for (i, f) in ["xd", "yd", "zd"].into_iter().enumerate() {
                set_d(rt, p, f, a(i + 1).d());
            }
            V::Void
        }
        (PARTICLE, "scale") => {
            let s = a(1).f();
            virt(rt, SET_SIZE, p, &[V::F(0.2 * s), V::F(0.2 * s)]);
            p
        }
        (SINGLE_QUAD, "scale") => {
            let size = get_f(rt, p, "quadSize") * a(1).f();
            set_f(rt, p, "quadSize", size);
            return particle_method(rt, PARTICLE, name, desc, args);
        }
        (PARTICLE, "setColor") => {
            for (i, f) in ["rCol", "gCol", "bCol"].into_iter().enumerate() {
                set_f(rt, p, f, a(i + 1).f());
            }
            V::Void
        }
        (SIMPLE_ANIMATED, "setColor") => {
            let rgb = channels(a(1).i());
            virt(rt, SET_COLOR, p, &rgb.map(V::F));
            V::Void
        }
        (SIMPLE_ANIMATED, "setFadeColor") => {
            for (f, c) in ["$fadeR", "$fadeG", "$fadeB"]
                .into_iter()
                .zip(channels(a(1).i()))
            {
                set_f(rt, p, f, c);
            }
            set_i(rt, p, "$hasFade", 1);
            V::Void
        }
        (PARTICLE, "setAlpha") => {
            set_f(rt, p, "alpha", a(1).f());
            V::Void
        }
        (PARTICLE, "setLifetime") => {
            set_i(rt, p, "lifetime", a(1).i());
            V::Void
        }
        (PARTICLE, "getLifetime") => V::I(get_i(rt, p, "lifetime")),
        (PARTICLE, "tick") => {
            particle_tick(rt, p);
            V::Void
        }
        (SIMPLE_ANIMATED, "tick") => {
            animated_tick(rt, p);
            V::Void
        }
        (PARTICLE, "remove") => {
            set_i(rt, p, "removed", 1);
            V::Void
        }
        (PARTICLE, "isAlive") => V::I(i32::from(get_i(rt, p, "removed") == 0)),
        (PARTICLE, "setSize") => {
            set_size(rt, p, a(1).f(), a(2).f());
            V::Void
        }
        (PARTICLE, "setPos") => {
            set_pos(rt, p, [a(1).d(), a(2).d(), a(3).d()]);
            V::Void
        }
        (PARTICLE, "move") => {
            move_by(rt, p, [a(1).d(), a(2).d(), a(3).d()]);
            V::Void
        }
        (PARTICLE, "setLocationFromBoundingbox") => {
            let b = bounding_box(rt, p);
            set_d(rt, p, "x", (b[0] + b[3]) / 2.0);
            set_d(rt, p, "y", b[1]);
            set_d(rt, p, "z", (b[2] + b[5]) / 2.0);
            V::Void
        }
        // The light where it is (`LevelRenderer.getLightColor` at the block it is in; the level's
        // every chunk a mod reaches is loaded).
        (PARTICLE, "getLightColor") => {
            let [x, y, z] = ["x", "y", "z"].map(|f| get_d(rt, p, f).floor() as i64);
            let pos = crate::api::block_pos(rt, x, y, z);
            let level = rt.get_field(p, "level");
            V::I(crate::render::light_color(rt, level, pos))
        }
        // Lit fully, wherever it is.
        (SIMPLE_ANIMATED, "getLightColor") => V::I(15728880),
        (PARTICLE, "getBoundingBox") => rt.get_field(p, "$bb"),
        (PARTICLE, "setBoundingBox") => {
            rt.put_field(p, "$bb", a(1));
            V::Void
        }
        (PARTICLE, "getParticleGroup") => crate::api::call(rt, EMPTY_OPTIONAL, &[]),
        (PARTICLE, "getRenderBoundingBox") => {
            let b = bounding_box(rt, p);
            crate::shapes::aabb(
                rt,
                [
                    b[0] - 1.0,
                    b[1] - 1.0,
                    b[2] - 1.0,
                    b[3] + 1.0,
                    b[4] + 1.0,
                    b[5] + 1.0,
                ],
            )
        }
        (SINGLE_QUAD, "getRenderBoundingBox") => {
            let size = f64::from(virt(rt, QUAD_SIZE, p, &[a(1)]).f());
            let [x, y, z] = ["x", "y", "z"].map(|f| get_d(rt, p, f));
            crate::shapes::aabb(
                rt,
                [x - size, y - size, z - size, x + size, y + size, z + size],
            )
        }
        (PARTICLE, "getPos") => {
            let [x, y, z] = ["x", "y", "z"].map(|f| get_d(rt, p, f));
            crate::api_common::vec3(rt, x, y, z)
        }
        (SINGLE_QUAD, "getFacingCameraMode") => facing_mode(rt, "LOOKAT_XYZ"),
        (SINGLE_QUAD, "render") => {
            single_quad_render(rt, p, a(1), a(2), a(3).f());
            V::Void
        }
        (SINGLE_QUAD, "renderRotatedQuad") if desc.contains("Lnet/minecraft/client/Camera;") => {
            rotated_quad(rt, p, a(1), a(2), a(3), a(4).f());
            V::Void
        }
        (SINGLE_QUAD, "renderRotatedQuad") => {
            rotated_quad_at(rt, p, a(1), a(2), [a(3).f(), a(4).f(), a(5).f()], a(6).f());
            V::Void
        }
        (SINGLE_QUAD, "getQuadSize") => V::F(get_f(rt, p, "quadSize")),
        (TEXTURE_SHEET, "setSprite") => {
            rt.put_field(p, "sprite", a(1));
            V::Void
        }
        // Its sprite's corners (a particle drawn before it has a sprite throws, as vanilla's does).
        (TEXTURE_SHEET, "getU0" | "getU1" | "getV0" | "getV1") => {
            let sprite = rt.get_field(p, "sprite");
            if sprite.r() == 0 {
                rt.throw_new("java/lang/NullPointerException");
                return Some(V::F(0.0));
            }
            let key = match name {
                "getU0" => SPRITE_U0,
                "getU1" => SPRITE_U1,
                "getV0" => SPRITE_V0,
                _ => SPRITE_V1,
            };
            crate::menus::virt(rt, key, sprite, &[])
        }
        (TEXTURE_SHEET, "pickSprite") => {
            let random = rt.get_field(p, "random");
            let sprite = get_sprite(rt, a(1), SPRITE_BY_RANDOM, &[random]);
            if rt.thrown.is_none() {
                virt(rt, SET_SPRITE, p, &[sprite]);
            }
            V::Void
        }
        (TEXTURE_SHEET, "setSpriteFromAge") => {
            if get_i(rt, p, "removed") == 0 {
                let (age, lifetime) = (get_i(rt, p, "age"), get_i(rt, p, "lifetime"));
                let sprite = get_sprite(rt, a(1), SPRITE_BY_AGE, &[V::I(age), V::I(lifetime)]);
                if rt.thrown.is_none() {
                    virt(rt, SET_SPRITE, p, &[sprite]);
                }
            }
            V::Void
        }
        (SIMPLE_ANIMATED, "getRenderType") => render_type(rt, "PARTICLE_SHEET_TRANSLUCENT"),
        (NO_RENDER, "getRenderType") => render_type(rt, "NO_RENDER"),
        (NO_RENDER, "render") => V::Void,
        _ => return None,
    })
}

// ---- the API ----

const SPRITE_PROVIDER_API: &str = "net/minecraft/client/particle/ParticleProvider$Sprite";
const BUFFER_BUILDER: &str = "com/mojang/blaze3d/vertex/BufferBuilder";

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key
/// only to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    match owner {
        PARTICLE | SINGLE_QUAD | TEXTURE_SHEET | SIMPLE_ANIMATED | NO_RENDER => {
            name == "<init>"
                || [
                    PARTICLE,
                    SINGLE_QUAD,
                    TEXTURE_SHEET,
                    SIMPLE_ANIMATED,
                    NO_RENDER,
                ]
                .into_iter()
                .any(|c| declares(c, name, desc))
        }
        ENGINE => matches!(name, "register" | "add" | "createParticle" | "$providers"),
        SPRITE_SET | SPRITE_SET_API => name == "get",
        RENDER_TYPE => matches!(name, "begin" | "isTranslucent" | "toString"),
        FACING => name == "setRotation",
        PROVIDERS_EVENT => matches!(
            name,
            "registerSpecial" | "registerSprite" | "registerSpriteSet"
        ),
        PROVIDER | SPRITE_PROVIDER_API => name == "createParticle",
        MINECRAFT => name == "$particleEngine",
        _ => false,
    }
}

/// The particle classes' calls, the engine's, its sprite sets', render types' and camera modes', and
/// the providers' event's; None for anything else.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| arg(args, i);
    Some(match owner {
        PARTICLE | SINGLE_QUAD | TEXTURE_SHEET | SIMPLE_ANIMATED | NO_RENDER => {
            let owner = modelled(owner)?;
            if name == "<init>" {
                return Some(construct(rt, owner, desc, args));
            }
            if a(0).r() == 0 {
                // On null, an abstract method's call (`Particle.render`) included, as the JVM's.
                rt.throw_new("java/lang/NullPointerException");
                return Some(V::Void);
            }
            // A virtual call runs the method of the nearest modelled class the particle's class
            // extends (none of the mod's classes overrides it, or the call wouldn't be here); a
            // `super` call names the class it extends, which is that one.
            let from = base_of(rt, a(0))
                .filter(|b| extends(b, owner))
                .unwrap_or(owner);
            let class = implementer(from, name, desc)?;
            return particle_method(rt, class, name, desc, args);
        }
        ENGINE => match name {
            "$providers" => {
                let e = engine(rt);
                rt.get_field(e, "$providers")
            }
            "register" => {
                let how = if desc.contains(
                    "Lnet/minecraft/client/particle/ParticleEngine$SpriteParticleRegistration;",
                ) {
                    Registration::SpriteSet
                } else if desc.contains("Lnet/minecraft/client/particle/ParticleProvider$Sprite;") {
                    Registration::Sprite
                } else {
                    Registration::Provider
                };
                register(rt, a(1), how, a(2));
                V::Void
            }
            "add" => {
                add(rt, a(1));
                V::Void
            }
            // `createParticle(options, x, y, z, xSpeed, ySpeed, zSpeed)`: made by its provider and
            // added, the limit aside. A type no mod gave a provider is the game's to draw: it draws
            // it, and the mod gets none back.
            "createParticle" => {
                let (options, at, speed) = (
                    a(1),
                    [a(2).d(), a(3).d(), a(4).d()],
                    [a(5).d(), a(6).d(), a(7).d()],
                );
                match make(rt, options, at, speed) {
                    Some(p) => {
                        add(rt, p);
                        p
                    }
                    None => {
                        crate::particles::hand_to_game(rt, options, at, speed);
                        V::NULL
                    }
                }
            }
            _ => return None,
        },
        // A sprite set's `get` and a provider's `createParticle` on null throw, as the JVM's do.
        SPRITE_SET | SPRITE_SET_API | PROVIDER | SPRITE_PROVIDER_API
            if a(0).r() == 0 && claims(owner, name, desc) =>
        {
            rt.throw_new("java/lang/NullPointerException");
            V::NULL
        }
        SPRITE_SET | SPRITE_SET_API if name == "get" && rt.class_of(a(0)) == SPRITE_SET => {
            sprite_set_get(rt, a(0), desc, args)
        }
        RENDER_TYPE => {
            let vanilla = vanilla_type(rt, a(0))?;
            match name {
                "toString" => rt.string(TYPES[vanilla]),
                "isTranslucent" => V::I(i32::from(!matches!(vanilla, 1 | 2))),
                // Vanilla's `begin`: its atlas bound (the terrain's the blocks', `CUSTOM` none) and a
                // buffer of quads to draw into; none for `NO_RENDER`. A mod's own type often begins
                // with one of these.
                "begin" => {
                    let texture = match vanilla {
                        0 => Some(BLOCK_ATLAS),
                        4 => None,
                        NO_RENDER_TYPE => return Some(V::NULL),
                        _ => Some(PARTICLE_ATLAS),
                    };
                    if let Some(t) = texture {
                        let t = rt.string(t);
                        rt.interned.insert(crate::screens::SHADER_TEXTURE.into(), t);
                    }
                    rt.new_object(BUFFER_BUILDER)
                }
                _ => return None,
            }
        }
        FACING if name == "setRotation" && rt.class_of(a(0)) == FACING => {
            set_rotation(rt, a(0), a(1), a(2), a(3).f());
            V::Void
        }
        PROVIDERS_EVENT => {
            let how = match name {
                "registerSpecial" => Registration::Provider,
                "registerSprite" => Registration::Sprite,
                "registerSpriteSet" => Registration::SpriteSet,
                _ => return None,
            };
            register(rt, a(1), how, a(2));
            V::Void
        }
        // The provider `registerSprite` keeps: the mod's, then a sprite of the type's set.
        PROVIDER | SPRITE_PROVIDER_API
            if name == "createParticle" && rt.class_of(a(0)) == SPRITE_PROVIDER =>
        {
            let (sprite, set) = (rt.get_field(a(0), "$sprite"), rt.get_field(a(0), "$set"));
            let p = rt.call_functional(SPRITE_CREATE, sprite, args.get(1..).unwrap_or_default());
            if p.r() != 0 && rt.thrown.is_none() {
                virt(rt, PICK_SPRITE, p, &[set]);
            }
            p
        }
        MINECRAFT if name == "$particleEngine" => engine(rt),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    const PUFF: &str = "test/Puff";
    const PUFF_DATA: &str = "test/PuffData";
    const PUFF_TYPE: &str = "test/PuffType";
    const ADD: &str = "net/minecraft/world/level/Level.addParticle.(Lnet/minecraft/core/particles/ParticleOptions;DDDDDD)V";

    /// The mod's own classes: its options answer their type, its particle draws on the opaque sheet.
    fn overrides(class: &str, key: &str) -> bool {
        (class == PUFF_DATA && key.starts_with("getType."))
            || (class == PUFF && key.starts_with("getRenderType."))
    }

    fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        match (
            rt.class_of(args[0]),
            short.split_once('.').map_or(short, |(n, _)| n),
        ) {
            (PUFF_DATA, "getType") => rt.interned["test:puff-type"],
            (PUFF, "getRenderType") => crate::api::get_static(
                rt,
                "net/minecraft/client/particle/ParticleRenderType.PARTICLE_SHEET_OPAQUE",
            ),
            _ => crate::api::call(rt, key, args),
        }
    }

    /// The mod's lambdas: 1 its listener, registering its providers; 2 its registration
    /// (`PuffProvider::new`); 3 that provider, making a `Puff` as the mod's constructor does.
    fn lambdas(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
        Some(match id {
            1 => {
                let (ev, puff, spark) = (
                    args[1],
                    rt.interned["test:puff-type"],
                    rt.interned["test:spark-type"],
                );
                let registration = rt.lambda_id(
                    "net/minecraft/client/particle/ParticleEngine$SpriteParticleRegistration",
                    2,
                    vec![],
                );
                crate::api::call(
                    rt,
                    "net/neoforged/neoforge/client/event/RegisterParticleProvidersEvent.registerSpriteSet.(Lnet/minecraft/core/particles/ParticleType;Lnet/minecraft/client/particle/ParticleEngine$SpriteParticleRegistration;)V",
                    &[ev, puff, registration],
                );
                // Vanilla's flame provider for its other type.
                let flame = rt.new_object("net/minecraft/client/particle/FlameParticle$Provider");
                crate::api::call(
                    rt,
                    "net/neoforged/neoforge/client/event/RegisterParticleProvidersEvent.registerSpecial.(Lnet/minecraft/core/particles/ParticleType;Lnet/minecraft/client/particle/ParticleProvider;)V",
                    &[ev, spark, flame],
                );
                V::Void
            }
            2 => rt.lambda_id(
                "net/minecraft/client/particle/ParticleProvider",
                3,
                vec![args[1]],
            ),
            3 => {
                let set = rt.captured(args[0])[0];
                let p = rt.new_object(PUFF);
                crate::api::call(
                    rt,
                    "net/minecraft/client/particle/TextureSheetParticle.<init>.(Lnet/minecraft/client/multiplayer/ClientLevel;DDD)V",
                    &[p, args[2], args[3], args[4], args[5]],
                );
                crate::api::call(
                    rt,
                    "test/Puff.setParticleSpeed.(DDD)V",
                    &[p, args[6], args[7], args[8]],
                );
                rt.put_field(p, "lifetime", V::I(6));
                rt.put_field(p, "gravity", V::F(1.0));
                rt.put_field(p, "quadSize", V::F(0.25));
                crate::api::call(
                    rt,
                    "test/Puff.pickSprite.(Lnet/minecraft/client/particle/SpriteSet;)V",
                    &[p, set],
                );
                p
            }
            _ => return None,
        })
    }

    fn live(rt: &Rt) -> Vec<V> {
        let Some(e) = engine_if_any(rt) else {
            return Vec::new();
        };
        items(rt, rt.get_field(e, "$queues"))
            .into_iter()
            .flat_map(|q| items(rt, q))
            .collect()
    }

    /// A mod registers its type's provider when the client posts `RegisterParticleProvidersEvent`; a
    /// client level's `addParticle` of its options (a class of its own, naming the type by
    /// `getType()`) has the provider make its particle. It joins at the next tick, falls by its
    /// gravity onto the stone, lands and stops, and goes at its lifetime; meanwhile it draws its
    /// quad facing the camera with a sprite of its type's set and the light where it is. A type the
    /// mod gave vanilla's provider stays the game's to draw.
    #[test]
    fn a_mods_particle_is_made_by_its_provider_falls_lands_and_draws_its_quad() {
        let mut rt = Rt::new();
        crate::install();
        rt.overrides = Some(overrides);
        rt.vcall = Some(vcall);
        rt.lambda_call = Some(lambdas);
        for (class, sup) in [
            (PUFF, TEXTURE_SHEET),
            (PUFF_DATA, "java/lang/Object"),
            (PUFF_TYPE, "net/minecraft/core/particles/ParticleType"),
        ] {
            rt.supers.insert(class, sup);
        }
        let puff = rt.new_object(PUFF_TYPE);
        crate::api::call(
            &mut rt,
            "net/minecraft/core/particles/ParticleType.<init>.(Z)V",
            &[puff, V::I(0)],
        );
        let spark = rt.new_object("net/minecraft/core/particles/SimpleParticleType");
        crate::api::call(
            &mut rt,
            "net/minecraft/core/particles/SimpleParticleType.<init>.(Z)V",
            &[spark, V::I(0)],
        );
        for (name, ty) in [("test:puff", puff), ("test:spark", spark)] {
            rt.registered
                .insert(("particle_type".into(), name.into()), ty);
        }
        rt.interned.insert("test:puff-type".into(), puff);
        rt.interned.insert("test:spark-type".into(), spark);
        let bus = rt.new_object("net/neoforged/bus/api/IEventBus");
        let class = crate::jlang::class_object(&mut rt, PROVIDERS_EVENT);
        let listener = rt.lambda_id("java/util/function/Consumer", 1, vec![]);
        crate::api::call(
            &mut rt,
            "net/neoforged/bus/api/IEventBus.addListener.(Ljava/lang/Class;Ljava/util/function/Consumer;)V",
            &[bus, class, listener],
        );
        crate::neo::fire_client_setup(&mut rt);

        let mut host = MemoryHost {
            client: true,
            place: Some(([0.5, 64.0, -3.0], 0.0, 0.0)),
            ..MemoryHost::default()
        };
        host.blocks.insert([0, 63, 0], "minecraft:stone".into());
        host.light.insert([0, 64, 0], (3, 15));
        host.resources.insert(
            "assets/test/particles/puff.json".into(),
            br#"{"textures": ["test:puff_0", "test:puff_1"]}"#.to_vec(),
        );
        let data = rt.new_object(PUFF_DATA);
        rt.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            let d = V::D;
            crate::api::call(
                rt,
                ADD,
                &[
                    level,
                    data,
                    d(0.5),
                    d(64.5),
                    d(0.5),
                    d(0.0),
                    d(-0.3),
                    d(0.0),
                ],
            );
            crate::api::call(
                rt,
                ADD,
                &[
                    level,
                    spark,
                    d(2.0),
                    d(65.0),
                    d(2.0),
                    d(0.0),
                    d(0.0),
                    d(0.0),
                ],
            );
        });
        let to_game: Vec<_> = host
            .particles
            .iter()
            .map(|p| (p.kind.as_str(), p.at))
            .collect();
        assert_eq!(to_game, [("test:spark", [2.0, 65.0, 2.0])]);
        assert!(
            live(&rt).is_empty(),
            "a particle made since the last tick isn't live yet"
        );

        // Each tick: (y, on the ground, stopped). It falls 0.34 (its speed and the gravity), then
        // lands on the stone's top short of the next fall, then stays.
        let mut path = Vec::new();
        for _ in 0..4 {
            crate::glue::client_tick(&mut rt, &mut host);
            let p = live(&rt)[0];
            path.push((
                rt.get_field(p, "y").d(),
                rt.get_field(p, "onGround").i(),
                rt.get_field(p, "$stoppedByCollision").i(),
            ));
        }
        assert_eq!(path[0], (64.5, 0, 0));
        assert!(
            (path[1].0 - 64.16).abs() < 1e-9 && path[1].1 == 0,
            "{path:?}"
        );
        assert_eq!(path[2], (64.0, 1, 0));
        assert_eq!(path[3], (64.0, 1, 1));

        crate::glue::render_particles(&mut rt, &mut host, [0.5, 66.0, -3.0], 0.5);
        assert_eq!(host.particle_quads.len(), 1);
        let q = &host.particle_quads[0];
        // Facing the camera, which looks south at it: its right (+x on the sprite) is the world's -x.
        let corners = [
            [-0.25, -2.25, 3.5],
            [-0.25, -1.75, 3.5],
            [0.25, -1.75, 3.5],
            [0.25, -2.25, 3.5],
        ];
        for (got, want) in q.positions.iter().zip(corners) {
            assert!(
                got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-5),
                "{:?}",
                q.positions
            );
        }
        assert!(
            ["test:particle/puff_0", "test:particle/puff_1"].contains(&q.sprite.as_str()),
            "{}",
            q.sprite
        );
        for (got, want) in
            q.uv.iter()
                .zip([[1.0, 1.0], [1.0, 0.0], [0.0, 0.0], [0.0, 1.0]])
        {
            assert!(
                got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-4),
                "{:?}",
                q.uv
            );
        }
        assert_eq!(q.colors, [0xFFFF_FFFF; 4]);
        // `LightTexture.pack(3, 15)`: the block light and the sky's at the block it is in.
        assert_eq!(q.light, 3 << 4 | 15 << 20);
        assert!(!q.translucent);

        // Its lifetime of 6: gone at the tick its age reaches it.
        for _ in 0..3 {
            crate::glue::client_tick(&mut rt, &mut host);
        }
        assert_eq!(live(&rt).len(), 1);
        crate::glue::client_tick(&mut rt, &mut host);
        assert!(live(&rt).is_empty());
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `SimpleAnimatedParticle`'s: the colour of an int, the frame of its set for its age, and past
    /// half its life the fade out and the drift to its fade colour; lit fully wherever it is.
    #[test]
    fn an_animated_particle_takes_its_frames_and_fades_past_half_its_life() {
        const GLOW: &str = "test/Glow";
        let mut rt = Rt::new();
        rt.supers.insert(GLOW, SIMPLE_ANIMATED);
        let mut host = MemoryHost {
            client: true,
            ..MemoryHost::default()
        };
        host.resources.insert(
            "assets/test/particles/glow.json".into(),
            br#"{"textures": ["test:glow_0", "test:glow_1", "test:glow_2"]}"#.to_vec(),
        );
        let set = sprite_set(&mut rt, "test:glow");
        let mut seen = Vec::new();
        let light = rt.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            let p = rt.new_object(GLOW);
            crate::api::call(rt, "net/minecraft/client/particle/SimpleAnimatedParticle.<init>.(Lnet/minecraft/client/multiplayer/ClientLevel;DDDLnet/minecraft/client/particle/SpriteSet;F)V", &[p, level, V::D(0.5), V::D(80.0), V::D(0.5), set, V::F(0.0)]);
            rt.put_field(p, "lifetime", V::I(10));
            crate::api::call(rt, "test/Glow.setColor.(I)V", &[p, V::I(0xFF8000)]);
            crate::api::call(rt, "test/Glow.setFadeColor.(I)V", &[p, V::I(0x000000)]);
            assert_eq!(["rCol", "gCol", "bCol"].map(|f| rt.get_field(p, f).f()), [1.0, 128.0 / 255.0, 0.0]);
            for _ in 0..11 {
                crate::api::call(rt, "test/Glow.tick.()V", &[p]);
                let sprite = rt.get_field(p, "sprite");
                let name = rt.str_of(rt.get_field(sprite, "$name")).unwrap_or("").to_owned();
                seen.push((rt.get_field(p, "age").i(), name, rt.get_field(p, "alpha").f(), rt.get_field(p, "rCol").f(), rt.get_field(p, "removed").i()));
            }
            crate::api::call(rt, "test/Glow.getLightColor.(F)I", &[p, V::F(0.0)]).i()
        });
        let frame = |i: usize| format!("test:particle/glow_{i}");
        // Its frame: age * 2 / 10 of the three.
        let frames: Vec<_> = seen.iter().map(|s| s.1.clone()).collect();
        assert_eq!(frames, [0, 0, 0, 0, 1, 1, 1, 1, 1, 2, 2].map(frame));
        // Opaque to age 5; then 1 - (age - 5) / 10, its red a fifth nearer black each tick.
        let mut red = 1.0f32;
        for (age, _, alpha, r, removed) in &seen {
            let age = *age;
            let want = if age > 5 {
                1.0 - (age as f32 - 5.0) / 10.0
            } else {
                1.0
            };
            if age > 5 {
                red += (0.0 - red) * 0.2;
            }
            assert!(
                (alpha - want).abs() < 1e-6 && (r - red).abs() < 1e-6,
                "{seen:?}"
            );
            assert_eq!(*removed, i32::from(age > 10), "{seen:?}");
        }
        assert_eq!(light, 15728880);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `Math.random()` (vanilla's particles nudge their speed by it) draws a new number in [0, 1)
    /// each time.
    #[test]
    fn math_random_draws_a_new_number_each_time() {
        let mut rt = Rt::new();
        let draws: Vec<f64> = (0..4)
            .map(|_| crate::api::call(&mut rt, "java/lang/Math.random.()D", &[]).d())
            .collect();
        assert!(draws.iter().all(|d| (0.0..1.0).contains(d)), "{draws:?}");
        assert!(draws.windows(2).all(|w| w[0] != w[1]), "{draws:?}");
    }

    #[test]
    fn a_call_on_null_throws_as_the_jvm_does() {
        // Abstract `Particle.render`, an interface's `SpriteSet.get` and `createParticle`.
        for key in [RENDER, GET_RENDER_TYPE, SPRITE_BY_RANDOM, CREATE_PARTICLE] {
            let mut rt = Rt::new();
            crate::api::call(&mut rt, key, &[V::NULL; 9]);
            assert!(rt.unmapped.is_empty(), "{key}: {:?}", rt.unmapped);
            let thrown = rt.thrown.take().map(|e| rt.class_of(e).to_owned());
            assert_eq!(
                thrown.as_deref(),
                Some("java/lang/NullPointerException"),
                "{key}"
            );
        }
    }
}
