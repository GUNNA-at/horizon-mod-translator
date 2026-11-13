//! Mod networking, natively: NeoForge's payloads and Minecraft's stream codecs and byte buffers.
//!
//! A mod registers each payload type with its `StreamCodec` and handler when
//! `RegisterPayloadHandlersEvent` fires (`PayloadRegistrar.playToClient`, `playToServer`), and sends
//! payloads with `PacketDistributor`. Here the payload's codec writes it into a byte buffer, the
//! bytes go over the game's mod message channel ([`crate::host::Host::send_payload`]; the channel
//! is the payload type's id, `create:server_speed`), and the other side's runtime reads them back
//! with the same codec and hands the payload to its handler ([`receive`]) with an
//! `IPayloadContext`.
//!
//! Minecraft's codecs (`ByteBufCodecs.INT`, `BlockPos.STREAM_CODEC`, `StreamCodec.composite`,
//! `.map`, `ByteBufCodecs.collection`...) are objects recording what they were made of (class
//! `compat:stream/<kind>`, their parts in order); a mod's own codec (`StreamCodec.of(encoder,
//! decoder)`, a class implementing `StreamCodec`) runs its translated code. Buffers
//! (`FriendlyByteBuf`, `RegistryFriendlyByteBuf`, netty's `ByteBuf`) write as Minecraft's do:
//! VarInts, big-endian numbers, UTF-8 strings after their VarInt length, NBT. Both ends are this
//! runtime, so a registry entry or a block state travels by name rather than by numeric id, and an
//! item stack as its item and count (its components aren't carried yet).
//!
//! Decoded numbers and booleans are raw values, as everywhere in this runtime: a constructor
//! reference (`Packet::new`) takes its primitives raw.
use crate::{Data, Rt, V, host::PayloadTarget, nbt::Nbt};

/// The class of the buffers made here.
pub const BUF: &str = "net/minecraft/network/RegistryFriendlyByteBuf";
pub const REGISTER_PAYLOADS: &str = "net/neoforged/neoforge/network/event/RegisterPayloadHandlersEvent";
const REGISTRAR: &str = "net/neoforged/neoforge/network/registration/PayloadRegistrar";
const CONTEXT: &str = "net/neoforged/neoforge/network/handling/IPayloadContext";
const PAYLOAD_TYPE: &str = "net/minecraft/network/protocol/common/custom/CustomPacketPayload$Type";
const DISTRIBUTOR: &str = "net/neoforged/neoforge/network/PacketDistributor";

const ENCODE: &str = "net/minecraft/network/codec/StreamEncoder.encode.(Ljava/lang/Object;Ljava/lang/Object;)V";
const DECODE: &str = "net/minecraft/network/codec/StreamDecoder.decode.(Ljava/lang/Object;)Ljava/lang/Object;";
const MEMBER_ENCODE: &str = "net/minecraft/network/codec/StreamMemberEncoder.encode.(Ljava/lang/Object;Ljava/lang/Object;)V";
const OPERATION: &str = "net/minecraft/network/codec/StreamCodec$CodecOperation.apply.(Lnet/minecraft/network/codec/StreamCodec;)Lnet/minecraft/network/codec/StreamCodec;";
const FUNCTION: &str = "java/util/function/Function.apply.(Ljava/lang/Object;)Ljava/lang/Object;";
const INT_FUNCTION: &str = "java/util/function/IntFunction.apply.(I)Ljava/lang/Object;";
const TO_INT: &str = "java/util/function/ToIntFunction.applyAsInt.(Ljava/lang/Object;)I";
const ADD: &str = "java/util/Collection.add.(Ljava/lang/Object;)Z";
const PUT: &str = "java/util/Map.put.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
const RUN: &str = "java/lang/Runnable.run.()V";
const SUPPLIER: &str = "java/util/function/Supplier.get.()Ljava/lang/Object;";
const TYPE_OF: &str = "net/minecraft/network/protocol/common/custom/CustomPacketPayload.type.()Lnet/minecraft/network/protocol/common/custom/CustomPacketPayload$Type;";
const HANDLE: &str = "net/neoforged/neoforge/network/handling/IPayloadHandler.handle.(Lnet/minecraft/network/protocol/common/custom/CustomPacketPayload;Lnet/neoforged/neoforge/network/handling/IPayloadContext;)V";

/// `StreamCodec.composite`'s constructor, by how many fields it takes.
const CONSTRUCT: [&str; 9] = [
    SUPPLIER,
    FUNCTION,
    "java/util/function/BiFunction.apply.(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
    "com/mojang/datafixers/util/Function3.apply.(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
    "com/mojang/datafixers/util/Function4.apply.(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
    "com/mojang/datafixers/util/Function5.apply.(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
    "com/mojang/datafixers/util/Function6.apply.(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
    "com/mojang/datafixers/util/Function7.apply.(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
    "com/mojang/datafixers/util/Function8.apply.(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
];

// Codecs made of others, and their parts.
/// `StreamCodec.unit(value)`: [value]; writes nothing.
const UNIT: &str = "compat:stream/unit";
/// `StreamCodec.of(encoder, decoder)`: [encoder, decoder].
const OF: &str = "compat:stream/of";
/// `StreamCodec.ofMember(encoder, decoder)`: [encoder, decoder], the encoder taking (value, buffer).
const MEMBER: &str = "compat:stream/member";
/// `StreamCodec.composite(codec1, getter1, ..., constructor)`: [constructor, codec1, getter1, ...].
const COMPOSITE: &str = "compat:stream/composite";
/// `codec.map(to, from)`: [codec, to, from].
const MAPPED: &str = "compat:stream/map";
/// `codec.dispatch(typeOf, codecOf)`: [the type's codec, typeOf, codecOf].
const DISPATCH: &str = "compat:stream/dispatch";
/// `ByteBufCodecs.optional(codec)`: [codec], an `Optional`.
const OPTIONAL: &str = "compat:stream/optional";
/// `ByteBufCodecs.collection(factory, codec)` and `codec.apply(ByteBufCodecs.list())`: [factory
/// (null: an `ArrayList`), codec].
const COLLECTION: &str = "compat:stream/collection";
/// `ByteBufCodecs.map(factory, keys, values)`: [factory, keys, values].
const MAP_OF: &str = "compat:stream/map_of";
/// `ByteBufCodecs.idMapper(IdMap)`: [the map]; block states (`Block.BLOCK_STATE_REGISTRY`) by text,
/// a registry's entries by name.
const ID_MAP: &str = "compat:stream/id_map";
/// `ByteBufCodecs.idMapper(byId, toId)`: [byId, toId].
const ID_FN: &str = "compat:stream/id_fn";
/// `ByteBufCodecs.registry(key)`: [the registry]; an entry by name.
const REGISTRY: &str = "compat:stream/registry";
/// `ByteBufCodecs.holderRegistry(key)`: [the registry]; a holder by its entry's name.
const HOLDER: &str = "compat:stream/holder";
/// `ByteBufCodecs.fromCodec(codec)` (and `fromCodecWithRegistries`, `fromCodecTrusted`): [codec];
/// the value as the codec writes it, in NBT.
const FROM_CODEC: &str = "compat:stream/from_codec";
/// `ResourceKey.streamCodec(registry)`: [the registry]; a key by its location.
const RESOURCE_KEY: &str = "compat:stream/resource_key";
/// `ParticleTypes.STREAM_CODEC`: particle options ([`crate::particles::write_options`]).
const PARTICLE: &str = "compat:stream/particle";
/// NeoForge's `enumCodec(EnumClass)`: [the class]; a constant by its ordinal, a VarInt.
const ENUM: &str = "compat:stream/enum";
/// NeoForge's `lazy(supplier)`: [supplier]; the codec it gives, asked once, does the work. And
/// `StreamCodec.recursive(wrapped)`: the codec a function makes of this one itself.
const LAZY: &str = "compat:stream/lazy";
/// What a [`LAZY`] codec holds first when it is `StreamCodec.recursive`'s: the function, of
/// the codec itself, in this object.
const RECURSIVE: &str = "compat:stream/recursive";
/// `ByteBufCodecs.either(left, right)`: [left, right]; a boolean for the side, then its value.
const EITHER: &str = "compat:stream/either";
/// NeoForge's `registryKey()`: a registry's own key, by its location.
const REGISTRY_KEY: &str = "compat:stream/registry_key";
const NEO_CODECS: &str = "net/neoforged/neoforge/network/codec/NeoForgeStreamCodecs";
/// `ByteBufCodecs.list()`, a `CodecOperation` (`codec.apply(list())`).
const LIST_OP: &str = "compat:stream-op/list";
/// `ByteBufCodecs.collection(factory)`, a `CodecOperation` (`codec.apply(collection(HashSet::new))`):
/// [factory].
const COLLECTION_OP: &str = "compat:stream-op/collection";

// Codecs of one kind of value.
const BOOL: &str = "compat:stream/bool";
const BYTE: &str = "compat:stream/byte";
const SHORT: &str = "compat:stream/short";
const UNSIGNED_SHORT: &str = "compat:stream/unsigned_short";
const INT: &str = "compat:stream/int";
const VAR_INT: &str = "compat:stream/var_int";
const VAR_LONG: &str = "compat:stream/var_long";
const LONG: &str = "compat:stream/long";
const FLOAT: &str = "compat:stream/float";
const DOUBLE: &str = "compat:stream/double";
const ROTATION: &str = "compat:stream/rotation";
const STRING: &str = "compat:stream/string";
const BYTES: &str = "compat:stream/bytes";
const TAG: &str = "compat:stream/tag";
const OPTIONAL_TAG: &str = "compat:stream/optional_tag";
const BLOCK_POS: &str = "compat:stream/block_pos";
const UUID: &str = "compat:stream/uuid";
const LOCATION: &str = "compat:stream/location";
const DIRECTION: &str = "compat:stream/direction";
const VEC3: &str = "compat:stream/vec3";
/// JOML's `Vector3f` and `Quaternionf`: their floats in order.
const VECTOR3F: &str = "compat:stream/vector3f";
const QUATERNIONF: &str = "compat:stream/quaternionf";
const STACK: &str = "compat:stream/stack";
const STACKS: &str = "compat:stream/stacks";
const CONTENTS: &str = "compat:stream/container-contents";
const FLUID_STACK: &str = "compat:stream/fluid_stack";
const COMPONENT: &str = "compat:stream/component";

fn make(rt: &mut Rt, kind: &'static str, parts: Vec<V>) -> V {
    rt.alloc(kind, Data::Array(parts))
}

/// The codec a `lazy` one's supplier gives, asked the first time and kept in its parts.
fn lazy_codec(rt: &mut Rt, lazy: V) -> V {
    let parts = rt.captured(lazy);
    if let Some(&made) = parts.get(1) {
        return made;
    }
    let supplier = parts.first().copied().unwrap_or_default();
    let made = if rt.class_of(supplier) == RECURSIVE {
        let wrapped = rt.captured(supplier).first().copied().unwrap_or_default();
        rt.call_functional(FUNCTION, wrapped, &[lazy])
    } else {
        rt.call_functional(SUPPLIER, supplier, &[])
    };
    if rt.thrown.is_none()
        && let Some(o) = rt.heap.get_mut(lazy.r() as usize)
    {
        o.data = Data::Array(vec![supplier, made]);
    }
    made
}

/// `ByteBufCodecs.fromCodecWithRegistries(codec)`: the value as the tag `codec` writes.
pub(crate) fn from_codec(rt: &mut Rt, codec: V) -> V {
    make(rt, FROM_CODEC, vec![codec])
}

/// One object per codec constant, as Minecraft's are.
fn constant(rt: &mut Rt, kind: &'static str) -> V {
    rt.intern(kind.to_owned(), |rt| make(rt, kind, Vec::new()))
}

/// Whether `c` is one of the codecs made here.
pub fn is_codec(rt: &Rt, c: V) -> bool {
    c.r() != 0 && rt.class_of(c).starts_with("compat:stream/")
}

/// Minecraft's codec constants.
pub fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    let kind = match (owner, field) {
        ("net/minecraft/network/codec/ByteBufCodecs", f) => match f {
            "BOOL" => BOOL,
            "BYTE" => BYTE,
            "SHORT" => SHORT,
            "UNSIGNED_SHORT" => UNSIGNED_SHORT,
            "INT" => INT,
            "VAR_INT" => VAR_INT,
            "VAR_LONG" => VAR_LONG,
            "LONG" => LONG,
            "FLOAT" => FLOAT,
            "DOUBLE" => DOUBLE,
            "ROTATION_BYTE" => ROTATION,
            "STRING_UTF8" => STRING,
            "BYTE_ARRAY" => BYTES,
            "TAG" | "TRUSTED_TAG" | "COMPOUND_TAG" | "TRUSTED_COMPOUND_TAG" => TAG,
            "OPTIONAL_COMPOUND_TAG" | "TRUSTED_OPTIONAL_COMPOUND_TAG" => OPTIONAL_TAG,
            "VECTOR3F" => VECTOR3F,
            "QUATERNIONF" => QUATERNIONF,
            _ => return None,
        },
        ("net/minecraft/core/BlockPos", "STREAM_CODEC") => BLOCK_POS,
        ("net/minecraft/core/UUIDUtil", "STREAM_CODEC") => UUID,
        ("net/minecraft/resources/ResourceLocation", "STREAM_CODEC") => LOCATION,
        ("net/minecraft/core/Direction", "STREAM_CODEC") => DIRECTION,
        ("net/minecraft/world/phys/Vec3", "STREAM_CODEC") => VEC3,
        ("net/minecraft/world/item/ItemStack", "STREAM_CODEC" | "OPTIONAL_STREAM_CODEC") => STACK,
        ("net/minecraft/world/item/ItemStack", "LIST_STREAM_CODEC" | "OPTIONAL_LIST_STREAM_CODEC") => STACKS,
        ("net/minecraft/world/item/component/ItemContainerContents", "STREAM_CODEC") => CONTENTS,
        ("net/neoforged/neoforge/fluids/FluidStack", "STREAM_CODEC" | "OPTIONAL_STREAM_CODEC") | ("net/neoforged/neoforge/fluids/SimpleFluidContent", "STREAM_CODEC") => FLUID_STACK,
        ("net/minecraft/network/chat/ComponentSerialization", "STREAM_CODEC" | "TRUSTED_STREAM_CODEC") => COMPONENT,
        ("net/minecraft/core/particles/ParticleTypes", "STREAM_CODEC") => PARTICLE,
        // Its length a VarInt, as `BYTE_ARRAY`'s (only without the limit).
        (NEO_CODECS, "UNBOUNDED_BYTE_ARRAY") => BYTES,
        // A dimension's key, then a position (vanilla's `composite`).
        ("net/minecraft/core/GlobalPos", "STREAM_CODEC") => {
            return Some(rt.intern("compat:stream/global_pos".into(), |rt| {
                let ctor = crate::functions::record_constructor(rt, "net/minecraft/core/GlobalPos");
                let registry = rt.string("dimension");
                let dimension = make(rt, RESOURCE_KEY, vec![registry]);
                let (of_dimension, of_pos) = (crate::functions::component(rt, "dimension"), crate::functions::component(rt, "pos"));
                let pos = constant(rt, BLOCK_POS);
                make(rt, COMPOSITE, vec![ctor, dimension, of_dimension, pos, of_pos])
            }));
        }
        // NeoForge's own payloads: the channel each is sent on, and its type.
        (o, "ID" | "TYPE") if neo_payload_channel(o).is_some() => {
            let channel = neo_payload_channel(o)?;
            return Some(if field == "ID" {
                rt.string(channel)
            } else {
                neo_payload_type(rt, channel)
            });
        }
        // A component type travels as an entry of its registry (`ByteBufCodecs.registry`, which
        // vanilla wraps for it): MineColonies sends the types its compatibility data names.
        ("net/minecraft/core/component/DataComponentType", "STREAM_CODEC") => {
            return Some(rt.intern("compat:stream/component_type".into(), |rt| {
                let registry = rt.string("data_component_type");
                make(rt, REGISTRY, vec![registry])
            }));
        }
        // The block states' id map: block states travel by their text.
        ("net/minecraft/world/level/block/Block", "BLOCK_STATE_REGISTRY") => {
            return Some(rt.intern("compat:block-state-ids".into(), |rt| rt.new_object("net/minecraft/core/IdMapper")));
        }
        _ => return None,
    };
    Some(constant(rt, kind))
}

// ---------------------------------------------------------------------------------------------
// Buffers.

/// Throws `class` with `message`, as netty's codec exceptions carry one.
fn throw_with(rt: &mut Rt, class: &'static str, message: &str) {
    let e = rt.new_object(class);
    let m = rt.string(message);
    rt.put_field(e, "message", m);
    rt.thrown = Some(e);
}

/// A new empty buffer (`Unpooled.buffer()`).
pub fn new_buffer(rt: &mut Rt) -> V {
    rt.alloc(BUF, Data::Bytes(Vec::new(), 0))
}

/// A buffer holding `bytes`, read from the start.
pub fn buffer_with(rt: &mut Rt, bytes: Vec<u8>) -> V {
    rt.alloc(BUF, Data::Bytes(bytes, 0))
}

/// Everything a buffer holds, read or not.
pub fn bytes(rt: &Rt, buf: V) -> Vec<u8> {
    match rt.heap.get(backing(rt, buf).r() as usize).map(|o| &o.data) {
        Some(Data::Bytes(v, _)) => v.clone(),
        _ => Vec::new(),
    }
}

/// The object holding a buffer's bytes: itself, or the buffer a `new FriendlyByteBuf(inner)` wraps.
fn backing(rt: &Rt, buf: V) -> V {
    let mut b = buf;
    for _ in 0..8 {
        match rt.heap.get(b.r() as usize).map(|o| &o.data) {
            Some(Data::Fields(f)) => match f.get("$inner") {
                Some(&inner) if inner.r() != 0 => b = inner,
                _ => break,
            },
            _ => break,
        }
    }
    b
}

fn bytes_mut(rt: &mut Rt, buf: V) -> Option<(&mut Vec<u8>, &mut usize)> {
    let b = backing(rt, buf);
    match rt.heap.get_mut(b.r() as usize).map(|o| &mut o.data) {
        Some(Data::Bytes(v, at)) => Some((v, at)),
        _ => None,
    }
}

pub(crate) fn put(rt: &mut Rt, buf: V, data: &[u8]) {
    if let Some((v, _)) = bytes_mut(rt, buf) {
        v.extend_from_slice(data);
    }
}

/// The next `n` bytes. Past the end: None, with Java's `IndexOutOfBoundsException` thrown.
pub(crate) fn take(rt: &mut Rt, buf: V, n: usize) -> Option<Vec<u8>> {
    let got = bytes_mut(rt, buf).and_then(|(v, at)| {
        let end = at.checked_add(n).filter(|&e| e <= v.len())?;
        let out = v[*at..end].to_vec();
        *at = end;
        Some(out)
    });
    if got.is_none() && rt.thrown.is_none() {
        rt.throw_new("java/lang/IndexOutOfBoundsException");
    }
    got
}

pub(crate) fn take_n<const N: usize>(rt: &mut Rt, buf: V) -> Option<[u8; N]> {
    take(rt, buf, N).and_then(|v| v.try_into().ok())
}

fn var_bytes(mut u: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(5);
    loop {
        if u < 0x80 {
            out.push(u as u8);
            return out;
        }
        out.push((u as u8 & 0x7F) | 0x80);
        u >>= 7;
    }
}

/// A VarInt (`v as u32`) or a VarLong.
pub(crate) fn put_var(rt: &mut Rt, buf: V, u: u64) {
    put(rt, buf, &var_bytes(u));
}

fn read_var(rt: &mut Rt, buf: V, max_bytes: u32) -> Option<u64> {
    let mut v = 0u64;
    for i in 0..max_bytes {
        let b = take_n::<1>(rt, buf)?[0];
        v |= u64::from(b & 0x7F) << (7 * i);
        if b & 0x80 == 0 {
            return Some(v);
        }
    }
    rt.throw_new("io/netty/handler/codec/DecoderException");
    None
}

pub(crate) fn read_var_int(rt: &mut Rt, buf: V) -> Option<i32> {
    read_var(rt, buf, 5).map(|v| v as u32 as i32)
}

/// A count read ahead of that many elements: at most what the buffer still holds (each element
/// takes a byte or more), so a bad count can't make a huge collection.
fn read_count(rt: &mut Rt, buf: V) -> Option<usize> {
    let n = read_var_int(rt, buf)?;
    let left = bytes_mut(rt, buf).map_or(0, |(v, at)| v.len() - *at);
    match usize::try_from(n) {
        Ok(n) if n <= left.max(1) * 8 => Some(n),
        _ => {
            rt.throw_new("io/netty/handler/codec/DecoderException");
            None
        }
    }
}

pub(crate) fn put_utf(rt: &mut Rt, buf: V, s: &str) {
    let mut out = var_bytes(s.len() as u64);
    out.extend_from_slice(s.as_bytes());
    put(rt, buf, &out);
}

pub(crate) fn read_utf(rt: &mut Rt, buf: V) -> Option<String> {
    let n = read_count(rt, buf)?;
    let b = take(rt, buf, n)?;
    Some(String::from_utf8_lossy(&b).into_owned())
}

fn text(rt: &Rt, v: V) -> String {
    rt.str_of(v).unwrap_or("").to_owned()
}

/// A number or boolean as a codec gets it: raw (a method reference's result) or boxed.
fn unboxed(rt: &Rt, v: V) -> V {
    match v {
        V::R(n) if n != 0 => rt.get_field(v, "value"),
        other => other,
    }
}

fn as_i64(v: V) -> i64 {
    match v {
        V::I(i) => i64::from(i),
        V::J(j) => j,
        V::F(f) => f as i64,
        V::D(d) => d as i64,
        _ => 0,
    }
}

fn as_f64(v: V) -> f64 {
    match v {
        V::I(i) => f64::from(i),
        V::J(j) => j as f64,
        V::F(f) => f64::from(f),
        V::D(d) => d,
        _ => 0.0,
    }
}

fn native(rt: &Rt, v: V) -> [i64; 4] {
    match rt.heap.get(v.r() as usize).map(|o| &o.data) {
        Some(Data::Native(_, h)) => *h,
        _ => [0; 4],
    }
}

/// `BlockPos.asLong`: x in the top 26 bits, then z (26), then y (12).
fn pack_pos([x, y, z, _]: [i64; 4]) -> i64 {
    ((x & 0x3FF_FFFF) << 38) | ((z & 0x3FF_FFFF) << 12) | (y & 0xFFF)
}

fn unpack_pos(l: i64) -> [i64; 3] {
    [l >> 38, (l << 52) >> 52, (l << 26) >> 38]
}

fn put_pos(rt: &mut Rt, buf: V, pos: V) {
    let l = pack_pos(native(rt, pos));
    put(rt, buf, &l.to_be_bytes());
}

fn read_pos(rt: &mut Rt, buf: V) -> Option<V> {
    let [x, y, z] = unpack_pos(i64::from_be_bytes(take_n(rt, buf)?));
    Some(crate::api::block_pos(rt, x, y, z))
}

fn uuid_u128(rt: &Rt, v: V) -> u128 {
    let (msb, lsb) = crate::tags::uuid_bits(rt, v);
    (u128::from(msb as u64) << 64) | u128::from(lsb as u64)
}

fn put_uuid(rt: &mut Rt, buf: V, v: V) {
    let (msb, lsb) = crate::tags::uuid_bits(rt, v);
    put(rt, buf, &msb.to_be_bytes());
    put(rt, buf, &lsb.to_be_bytes());
}

fn read_uuid(rt: &mut Rt, buf: V) -> Option<V> {
    let msb = i64::from_be_bytes(take_n(rt, buf)?);
    let lsb = i64::from_be_bytes(take_n(rt, buf)?);
    Some(crate::tags::uuid(rt, msb, lsb))
}

fn byte_array(rt: &Rt, v: V) -> Vec<u8> {
    match rt.heap.get(v.r() as usize).map(|o| &o.data) {
        Some(Data::Array(a)) => a.iter().map(|x| x.i() as u8).collect(),
        _ => Vec::new(),
    }
}

fn new_byte_array(rt: &mut Rt, b: &[u8]) -> V {
    rt.alloc("[", Data::Array(b.iter().map(|&x| V::I(i32::from(x as i8))).collect()))
}

fn vec3(rt: &mut Rt, x: f64, y: f64, z: f64) -> V {
    let v = rt.new_object("net/minecraft/world/phys/Vec3");
    rt.put_field(v, "x", V::D(x));
    rt.put_field(v, "y", V::D(y));
    rt.put_field(v, "z", V::D(z));
    v
}

fn put_vec3(rt: &mut Rt, buf: V, v: V) {
    for f in ["x", "y", "z"] {
        let d = as_f64(rt.get_field(v, f));
        put(rt, buf, &d.to_be_bytes());
    }
}

fn read_vec3(rt: &mut Rt, buf: V) -> Option<V> {
    let x = f64::from_be_bytes(take_n(rt, buf)?);
    let y = f64::from_be_bytes(take_n(rt, buf)?);
    let z = f64::from_be_bytes(take_n(rt, buf)?);
    Some(vec3(rt, x, y, z))
}

/// An enum constant's ordinal: a mod's, or one of Minecraft's here (`Direction`).
fn ordinal(rt: &Rt, v: V) -> i64 {
    match rt.heap.get(v.r() as usize).map(|o| &o.data) {
        Some(Data::Native(_, h)) => h[0],
        _ => i64::from(rt.get_field(v, "$ordinal").i()),
    }
}

// NBT, as `NbtIo.writeAnyTag` writes it on the network (a type byte, no name; 0 for none).

fn put_nbt(rt: &mut Rt, buf: V, n: Option<&Nbt>) {
    let mut out = Vec::new();
    crate::nbt::write_any(&mut out, n);
    put(rt, buf, &out);
}

/// A tag read off the buffer: Some(None) for none (`TAG_End`), None (thrown) when it doesn't parse.
fn read_nbt(rt: &mut Rt, buf: V) -> Option<Option<Nbt>> {
    let parsed = bytes_mut(rt, buf).and_then(|(v, at)| {
        let mut r = crate::nbt::Reader::new(&v[*at..]);
        let out = r.any()?;
        *at += r.at;
        Some(out)
    });
    if parsed.is_none() && rt.thrown.is_none() {
        rt.throw_new("io/netty/handler/codec/DecoderException");
    }
    parsed
}

// Item and fluid stacks: the count (amount), 0 for an empty one, then the item (fluid) by name.
// An item stack's mod components and damage follow, as a compound by type name (`TAG_End` for
// none): the stack a filter's menu opens with carries its items to the client.

/// The key of a stack's damage in the compound after its item.
const STACK_DAMAGE: &str = "minecraft:damage";

pub(crate) fn put_stack(rt: &mut Rt, buf: V, s: V) {
    if crate::api_common::stack_empty(rt, s) {
        put_var(rt, buf, 0);
        return;
    }
    let count = rt.get_field(s, "$count").i();
    let item = crate::api_common::item_like_name(rt, rt.get_field(s, "$item"));
    put_var(rt, buf, u64::from(count as u32));
    put_utf(rt, buf, &item);
    let mut patch: std::collections::BTreeMap<String, Nbt> = crate::components::saved(rt, s).into_iter().collect();
    let damage = crate::components::damage(rt, s);
    if damage > 0 {
        patch.insert(STACK_DAMAGE.into(), Nbt::Int(damage));
    }
    let patch = (!patch.is_empty()).then_some(Nbt::Compound(patch));
    put_nbt(rt, buf, patch.as_ref());
}

pub(crate) fn read_stack(rt: &mut Rt, buf: V) -> Option<V> {
    let count = read_var_int(rt, buf)?;
    if count <= 0 {
        return Some(crate::api::get_static(rt, "net/minecraft/world/item/ItemStack.EMPTY"));
    }
    let item = read_utf(rt, buf)?;
    let s = crate::api_common::stack(rt, &item, count);
    if let Some(Nbt::Compound(mut patch)) = read_nbt(rt, buf)? {
        if let Some(Nbt::Int(damage)) = patch.remove(STACK_DAMAGE) {
            rt.put_field(s, "$damage", V::I(damage));
        }
        let carried: Vec<(String, Nbt)> = patch.into_iter().collect();
        crate::components::load(rt, s, &carried);
    }
    Some(s)
}

fn put_fluid(rt: &mut Rt, buf: V, s: V) {
    if s.r() == 0 || crate::fluids::stack_empty(rt, s) {
        put_var(rt, buf, 0);
        return;
    }
    let amount = crate::fluids::stack_amount(rt, s);
    let name = crate::fluids::name_of(rt, rt.get_field(s, "$fluid"));
    put_var(rt, buf, u64::from(amount as u32));
    put_utf(rt, buf, &name);
    // Its components, as a stack's patch goes (both ends are this runtime).
    let patch = crate::components::patch(rt, s);
    let patch = (!patch.is_empty()).then_some(Nbt::Compound(patch));
    put_nbt(rt, buf, patch.as_ref());
}

fn read_fluid(rt: &mut Rt, buf: V) -> Option<V> {
    let amount = read_var_int(rt, buf)?;
    if amount <= 0 {
        return Some(crate::fluids::fluid_stack_empty(rt));
    }
    let name = read_utf(rt, buf)?;
    let patch = read_nbt(rt, buf)?;
    let f = crate::fluids::fluid(rt, &name);
    if f.r() == 0 {
        return Some(crate::fluids::fluid_stack_empty(rt));
    }
    let s = crate::fluids::fluid_stack(rt, f, amount);
    if let Some(Nbt::Compound(patch)) = patch {
        crate::components::load_saved_nbt(rt, s, &patch);
    }
    Some(s)
}

/// The name of an entry of `registry` (`fluid`, `item`...): the mod's, or one of Minecraft's.
fn entry_name(rt: &Rt, registry: &str, v: V) -> String {
    if let Some(n) = rt.registered_name(v, registry) {
        return n.to_owned();
    }
    match registry {
        "fluid" => crate::fluids::name_of(rt, v),
        "item" | "block" => crate::api_common::item_like_name(rt, v),
        // (Vanilla's component types know their names.)
        "data_component_type" if rt.str_of(rt.get_field(v, "$id")).is_some() => {
            text(rt, rt.get_field(v, "$id"))
        }
        _ => text(rt, v),
    }
}

/// What `Optional` `v` holds, if anything.
fn optional_held(rt: &Rt, v: V) -> Option<V> {
    if v.r() == 0 {
        return None;
    }
    let held = rt.get_field(v, "value");
    (held != V::NULL && held != V::Void).then_some(held)
}

/// Adds `v` to a collection a codec's factory made.
fn add_to(rt: &mut Rt, coll: V, v: V) {
    match rt.heap.get_mut(coll.r() as usize).map(|o| &mut o.data) {
        Some(Data::List(l)) => l.push(v),
        Some(Data::Table(_)) => {
            crate::jutil::table_put(rt, coll, v, V::I(1));
        }
        _ => {
            rt.call_functional(ADD, coll, &[v]);
        }
    }
}

fn put_to(rt: &mut Rt, map: V, k: V, v: V) {
    if matches!(rt.heap.get(map.r() as usize).map(|o| &o.data), Some(Data::Table(_))) {
        crate::jutil::table_put(rt, map, k, v);
    } else {
        rt.call_functional(PUT, map, &[k, v]);
    }
}

// ---------------------------------------------------------------------------------------------
// Encoding and decoding.

/// Writes `value` into `buf` with codec `c`, as `c.encode(buf, value)` does.
pub fn encode(rt: &mut Rt, c: V, buf: V, value: V) {
    if rt.thrown.is_some() || c.r() == 0 {
        return;
    }
    if !is_codec(rt, c) {
        // A mod's own codec: a class implementing `StreamCodec`, or an encoder lambda.
        rt.call_functional(ENCODE, c, &[buf, value]);
        return;
    }
    let parts = rt.captured(c);
    let p = |i: usize| parts.get(i).copied().unwrap_or_default();
    match rt.class_of(c) {
        UNIT => {}
        OF => {
            rt.call_functional(ENCODE, p(0), &[buf, value]);
        }
        MEMBER => {
            rt.call_functional(MEMBER_ENCODE, p(0), &[value, buf]);
        }
        COMPOSITE => {
            for pair in parts.get(1..).unwrap_or_default().chunks(2) {
                if let [codec, getter] = *pair {
                    let field = rt.call_functional(FUNCTION, getter, &[value]);
                    encode(rt, codec, buf, field);
                }
            }
        }
        MAPPED => {
            let inner = rt.call_functional(FUNCTION, p(2), &[value]);
            encode(rt, p(0), buf, inner);
        }
        DISPATCH => {
            let key = rt.call_functional(FUNCTION, p(1), &[value]);
            encode(rt, p(0), buf, key);
            let codec = rt.call_functional(FUNCTION, p(2), &[key]);
            encode(rt, codec, buf, value);
        }
        OPTIONAL => {
            let held = optional_held(rt, value);
            put(rt, buf, &[held.is_some() as u8]);
            if let Some(v) = held {
                encode(rt, p(0), buf, v);
            }
        }
        EITHER => {
            let left = rt.get_field(value, "$isLeft").i() != 0;
            put(rt, buf, &[left as u8]);
            let v = rt.get_field(value, if left { "$left" } else { "$right" });
            encode(rt, if left { p(0) } else { p(1) }, buf, v);
        }
        COLLECTION => {
            let items = crate::jutil::elements(rt, value);
            put_var(rt, buf, items.len() as u64);
            for v in items {
                encode(rt, p(1), buf, v);
            }
        }
        MAP_OF => {
            let pairs = crate::jutil::table_pairs(rt, value);
            put_var(rt, buf, pairs.len() as u64);
            for (k, v) in pairs {
                encode(rt, p(1), buf, k);
                encode(rt, p(2), buf, v);
            }
        }
        ID_MAP => {
            let name = match rt.str_of(p(0)).map(str::to_owned) {
                Some(registry) => entry_name(rt, &registry, value),
                None => text(rt, value),
            };
            put_utf(rt, buf, &name);
        }
        ID_FN => {
            let id = rt.call_functional(TO_INT, p(1), &[value]).i();
            put_var(rt, buf, u64::from(id as u32));
        }
        REGISTRY => {
            let registry = text(rt, p(0));
            let name = entry_name(rt, &registry, value);
            put_utf(rt, buf, &name);
        }
        RESOURCE_KEY => {
            let key = text(rt, value);
            let location = key.rsplit_once('#').map_or(key.as_str(), |(_, l)| l).to_owned();
            put_utf(rt, buf, &location);
        }
        HOLDER => {
            let registry = text(rt, p(0));
            let id = rt.get_field(value, "$id");
            let name = match rt.str_of(id) {
                Some(n) => n.to_owned(),
                None => {
                    let v = crate::api::call(rt, "net/minecraft/core/Holder.value.()Ljava/lang/Object;", &[value]);
                    entry_name(rt, &registry, v)
                }
            };
            put_utf(rt, buf, &name);
        }
        BOOL => put(rt, buf, &[(as_i64(unboxed(rt, value)) != 0) as u8]),
        BYTE => put(rt, buf, &[as_i64(unboxed(rt, value)) as u8]),
        SHORT | UNSIGNED_SHORT => put(rt, buf, &(as_i64(unboxed(rt, value)) as u16).to_be_bytes()),
        INT => put(rt, buf, &(as_i64(unboxed(rt, value)) as i32).to_be_bytes()),
        VAR_INT => put_var(rt, buf, u64::from(as_i64(unboxed(rt, value)) as i32 as u32)),
        VAR_LONG => put_var(rt, buf, as_i64(unboxed(rt, value)) as u64),
        LONG => put(rt, buf, &as_i64(unboxed(rt, value)).to_be_bytes()),
        FLOAT => put(rt, buf, &(as_f64(unboxed(rt, value)) as f32).to_be_bytes()),
        DOUBLE => put(rt, buf, &as_f64(unboxed(rt, value)).to_be_bytes()),
        // `Mth.packDegrees`.
        ROTATION => put(rt, buf, &[(as_f64(unboxed(rt, value)) * 256.0 / 360.0).floor() as i32 as u8]),
        STRING | LOCATION => {
            let s = text(rt, value);
            put_utf(rt, buf, &s);
        }
        BYTES => {
            let b = byte_array(rt, value);
            put_var(rt, buf, b.len() as u64);
            put(rt, buf, &b);
        }
        TAG => {
            let n = crate::nbt::of_tag(rt, value);
            put_nbt(rt, buf, n.as_ref());
        }
        // `codec.encodeStart(NbtOps.INSTANCE, value).getOrThrow(...)`, then `writeNbt`.
        FROM_CODEC => match crate::codecs::encode(rt, p(0), value) {
            Ok(n) => put_nbt(rt, buf, Some(&n)),
            Err(e) => throw_with(rt, "io/netty/handler/codec/EncoderException", &format!("Failed to encode: {e}")),
        },
        OPTIONAL_TAG => {
            let n = optional_held(rt, value).and_then(|v| crate::nbt::of_tag(rt, v));
            put_nbt(rt, buf, n.as_ref());
        }
        BLOCK_POS => put_pos(rt, buf, value),
        UUID => put_uuid(rt, buf, value),
        DIRECTION => {
            let o = ordinal(rt, value);
            put_var(rt, buf, o as u64);
        }
        VEC3 => put_vec3(rt, buf, value),
        VECTOR3F | QUATERNIONF => {
            let fields: &[&'static str] = if rt.class_of(c) == VECTOR3F { &["x", "y", "z"] } else { &["x", "y", "z", "w"] };
            for &f in fields {
                let x = rt.get_field(value, f).f();
                put(rt, buf, &x.to_be_bytes());
            }
        }
        STACK => put_stack(rt, buf, value),
        STACKS => {
            let items = crate::jutil::elements(rt, value);
            put_var(rt, buf, items.len() as u64);
            for s in items {
                put_stack(rt, buf, s);
            }
        }
        // Every slot, empty ones too, as optional stacks.
        CONTENTS => {
            let items = crate::handlers::contents_items(rt, value);
            put_var(rt, buf, items.len() as u64);
            for s in items {
                put_stack(rt, buf, s);
            }
        }
        FLUID_STACK => put_fluid(rt, buf, value),
        // A component whole (its translation key and arguments, style and siblings), as vanilla's
        // `ComponentSerialization.STREAM_CODEC` writes it: a train's name stays translatable.
        COMPONENT => {
            let n = crate::api_common::component_nbt(rt, value);
            put_nbt(rt, buf, Some(&n));
        }
        ENUM => {
            let o = ordinal(rt, value);
            put_var(rt, buf, o as u64);
        }
        LAZY => {
            let c = lazy_codec(rt, c);
            encode(rt, c, buf, value);
        }
        REGISTRY_KEY => {
            let name = text(rt, value);
            let location = if name.contains(':') { name } else { format!("minecraft:{name}") };
            put_utf(rt, buf, &location);
        }
        PARTICLE => {
            if let Err(e) = crate::particles::write_options(rt, buf, value) {
                throw_with(rt, "io/netty/handler/codec/EncoderException", &format!("Failed to encode: {e}"));
            }
        }
        _ => {}
    }
}

/// Reads a value off `buf` with codec `c`, as `c.decode(buf)` does. Null (thrown) when the bytes
/// don't hold one.
pub fn decode(rt: &mut Rt, c: V, buf: V) -> V {
    if rt.thrown.is_some() || c.r() == 0 {
        return V::NULL;
    }
    if !is_codec(rt, c) {
        return rt.call_functional(DECODE, c, &[buf]);
    }
    decode_native(rt, c, buf).unwrap_or(V::NULL)
}

fn decode_native(rt: &mut Rt, c: V, buf: V) -> Option<V> {
    let parts = rt.captured(c);
    let p = |i: usize| parts.get(i).copied().unwrap_or_default();
    let ok = |rt: &Rt, v: V| rt.thrown.is_none().then_some(v);
    Some(match rt.class_of(c) {
        UNIT => p(0),
        OF | MEMBER => rt.call_functional(DECODE, p(1), &[buf]),
        COMPOSITE => {
            let mut fields = Vec::with_capacity(parts.len() / 2);
            for pair in parts.get(1..).unwrap_or_default().chunks(2) {
                let v = decode(rt, pair[0], buf);
                fields.push(ok(rt, v)?);
            }
            rt.call_functional(CONSTRUCT[fields.len().min(8)], p(0), &fields)
        }
        MAPPED => {
            let inner = decode(rt, p(0), buf);
            let inner = ok(rt, inner)?;
            rt.call_functional(FUNCTION, p(1), &[inner])
        }
        DISPATCH => {
            let key = decode(rt, p(0), buf);
            let key = ok(rt, key)?;
            let codec = rt.call_functional(FUNCTION, p(2), &[key]);
            decode(rt, codec, buf)
        }
        OPTIONAL => {
            let present = take_n::<1>(rt, buf)?[0] != 0;
            let v = if present {
                let v = decode(rt, p(0), buf);
                ok(rt, v)?
            } else {
                V::NULL
            };
            crate::api_common::optional(rt, v)
        }
        EITHER => {
            let left = take_n::<1>(rt, buf)?[0] != 0;
            let v = decode(rt, if left { p(0) } else { p(1) }, buf);
            let v = ok(rt, v)?;
            crate::codecs::either(rt, left, v)
        }
        COLLECTION => {
            let n = read_count(rt, buf)?;
            let coll = if p(0).r() == 0 { crate::jutil::new_list(rt, Vec::new()) } else { rt.call_functional(INT_FUNCTION, p(0), &[V::I(n as i32)]) };
            for _ in 0..n {
                let v = decode(rt, p(1), buf);
                add_to(rt, coll, ok(rt, v)?);
            }
            coll
        }
        MAP_OF => {
            let n = read_count(rt, buf)?;
            let map = rt.call_functional(INT_FUNCTION, p(0), &[V::I(n as i32)]);
            for _ in 0..n {
                let k = decode(rt, p(1), buf);
                let k = ok(rt, k)?;
                let v = decode(rt, p(2), buf);
                put_to(rt, map, k, ok(rt, v)?);
            }
            map
        }
        ID_MAP => {
            let name = read_utf(rt, buf)?;
            match rt.str_of(p(0)).map(str::to_owned) {
                Some(registry) => crate::neo::registry_get(rt, &registry, &name),
                None => crate::api::state(rt, &name),
            }
        }
        ID_FN => {
            let id = read_var_int(rt, buf)?;
            rt.call_functional(INT_FUNCTION, p(0), &[V::I(id)])
        }
        REGISTRY => {
            let registry = text(rt, p(0));
            let name = read_utf(rt, buf)?;
            crate::neo::registry_get(rt, &registry, &name)
        }
        HOLDER => {
            let registry = text(rt, p(0));
            let name = read_utf(rt, buf)?;
            let v = crate::neo::registry_get(rt, &registry, &name);
            crate::neo::registry_holder(rt, &registry, &name, v)
        }
        BOOL => V::I(i32::from(take_n::<1>(rt, buf)?[0] != 0)),
        BYTE => V::I(i32::from(take_n::<1>(rt, buf)?[0] as i8)),
        SHORT => V::I(i32::from(i16::from_be_bytes(take_n(rt, buf)?))),
        UNSIGNED_SHORT => V::I(i32::from(u16::from_be_bytes(take_n(rt, buf)?))),
        INT => V::I(i32::from_be_bytes(take_n(rt, buf)?)),
        VAR_INT => V::I(read_var_int(rt, buf)?),
        VAR_LONG => V::J(read_var(rt, buf, 10)? as i64),
        LONG => V::J(i64::from_be_bytes(take_n(rt, buf)?)),
        FLOAT => V::F(f32::from_be_bytes(take_n(rt, buf)?)),
        DOUBLE => V::D(f64::from_be_bytes(take_n(rt, buf)?)),
        ROTATION => V::F(f32::from(take_n::<1>(rt, buf)?[0] as i8) * 360.0 / 256.0),
        STRING | LOCATION => {
            let s = read_utf(rt, buf)?;
            rt.string(&s)
        }
        BYTES => {
            let n = read_count(rt, buf)?;
            let b = take(rt, buf, n)?;
            new_byte_array(rt, &b)
        }
        TAG => match read_nbt(rt, buf)? {
            Some(n) => crate::nbt::tag_of(rt, &n),
            None => V::NULL,
        },
        RESOURCE_KEY => {
            let registry = text(rt, p(0));
            let location = read_utf(rt, buf)?;
            rt.string(&format!("{registry}#{location}"))
        }
        FROM_CODEC => {
            let decoded = match read_nbt(rt, buf)? {
                Some(n) => crate::codecs::decode(rt, p(0), &crate::recipes::nbt_json(&n)),
                None => Err("no tag".to_owned()),
            };
            match decoded {
                Ok(v) => v,
                Err(e) => {
                    throw_with(rt, "io/netty/handler/codec/DecoderException", &format!("Failed to decode: {e}"));
                    V::NULL
                }
            }
        }
        OPTIONAL_TAG => {
            let v = match read_nbt(rt, buf)? {
                Some(n) => crate::nbt::tag_of(rt, &n),
                None => V::NULL,
            };
            crate::api_common::optional(rt, v)
        }
        BLOCK_POS => read_pos(rt, buf)?,
        UUID => read_uuid(rt, buf)?,
        DIRECTION => {
            let o = read_var_int(rt, buf)?;
            crate::api_common::direction(rt, i64::from(o))
        }
        VEC3 => read_vec3(rt, buf)?,
        VECTOR3F | QUATERNIONF => {
            let (class, fields): (&'static str, &[&'static str]) =
                if rt.class_of(c) == VECTOR3F { ("org/joml/Vector3f", &["x", "y", "z"]) } else { ("org/joml/Quaternionf", &["x", "y", "z", "w"]) };
            let o = rt.new_object(class);
            for &f in fields {
                let x = f32::from_be_bytes(take_n(rt, buf)?);
                rt.put_field(o, f, V::F(x));
            }
            o
        }
        STACK => read_stack(rt, buf)?,
        STACKS => {
            let n = read_count(rt, buf)?;
            let mut items = Vec::with_capacity(n);
            for _ in 0..n {
                items.push(read_stack(rt, buf)?);
            }
            crate::jutil::new_list(rt, items)
        }
        CONTENTS => {
            let n = read_count(rt, buf)?;
            if n > 256 {
                rt.throw_new("io/netty/handler/codec/DecoderException");
                return None;
            }
            let mut items = Vec::with_capacity(n);
            for _ in 0..n {
                items.push(read_stack(rt, buf)?);
            }
            crate::handlers::contents(rt, items)
        }
        FLUID_STACK => read_fluid(rt, buf)?,
        PARTICLE => crate::particles::read_options(rt, buf)?,
        ENUM => {
            let o = read_var_int(rt, buf)?;
            let class = text(rt, p(0));
            let all = crate::jlang::enum_constants(rt, &class).unwrap_or(V::NULL);
            if o < 0 || o >= rt.array_len(all) {
                throw_with(rt, "io/netty/handler/codec/DecoderException", &format!("No {class} with ordinal {o}"));
                return None;
            }
            rt.array_get(all, o)
        }
        LAZY => {
            let c = lazy_codec(rt, c);
            decode(rt, c, buf)
        }
        REGISTRY_KEY => {
            let location = read_utf(rt, buf)?;
            let name = location.strip_prefix("minecraft:").unwrap_or(&location).to_owned();
            rt.string(&name)
        }
        COMPONENT => match read_nbt(rt, buf)? {
            Some(n) => crate::api_common::component_from_nbt(rt, &n),
            None => V::NULL,
        },
        _ => V::NULL,
    })
}

// ---------------------------------------------------------------------------------------------
// Payloads.

/// `RegisterPayloadHandlersEvent`, on the mod bus once loading has set the mods up, as NeoForge's
/// network registry fires it: the mods register their payload types.
pub fn fire_registration(rt: &mut Rt) {
    let ev = rt.new_object(REGISTER_PAYLOADS);
    crate::neo::post(rt, REGISTER_PAYLOADS, ev);
}

/// The payload types registered on this runtime, by id (`create:server_speed`).
pub fn registered(rt: &Rt) -> Vec<String> {
    let mut ids: Vec<String> = rt.interned.keys().filter_map(|k| k.strip_prefix("payload/").map(str::to_owned)).collect();
    ids.sort();
    ids
}

/// The channel and bytes of `payload`: its type's id, and what its registered codec writes. None
/// when its type wasn't registered or its codec threw.
pub fn encode_payload(rt: &mut Rt, payload: V) -> Option<(String, Vec<u8>)> {
    let ty = rt.call_functional(TYPE_OF, payload, &[]);
    if rt.thrown.is_some() {
        rt.report_thrown("CustomPacketPayload.type");
        return None;
    }
    let id = rt.str_of(rt.get_field(ty, "$id"))?.to_owned();
    let Some(entry) = rt.interned.get(&format!("payload/{id}")).copied() else {
        crate::glue::trace_once(rt, &format!("payload-unregistered/{id}"), |_| format!("payload {id} sent but never registered"));
        return None;
    };
    let codec = rt.captured(entry).first().copied().unwrap_or_default();
    let buf = new_buffer(rt);
    encode(rt, codec, buf, payload);
    if rt.thrown.is_some() {
        rt.report_thrown(&format!("encoding payload {id}"));
        return None;
    }
    Some((id, bytes(rt, buf)))
}

/// Sends `payload` to `to` through the game.
fn send(rt: &mut Rt, to: PayloadTarget, payload: V) -> bool {
    let Some((channel, data)) = encode_payload(rt, payload) else { return false };
    send_raw(rt, to, &channel, &data)
}

/// A message the game took no more of for now, waiting its turn ([`send_whole`]).
struct Waiting {
    to: PayloadTarget,
    channel: String,
    data: Vec<u8>,
    ticks: u32,
    /// The ticks it was the first to go to its receiver and did not: the tick's allowance was
    /// whole then, so it is the message, or the receiver, that the game does not take.
    refused: u32,
}

thread_local! {
    /// The messages waiting on this thread's runtime (a server's for its players, a client's
    /// for its server), in the order they were sent.
    static WAITING: std::cell::RefCell<std::collections::VecDeque<Waiting>> = const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
    /// The parts that have come of each message sent in parts, by its sender and number.
    static PARTS: std::cell::RefCell<std::collections::HashMap<(Option<u128>, u32), Vec<Option<Vec<u8>>>>> = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// The number of the last message sent in parts (by any thread: a sender's are told apart by
/// it).
static SPLIT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The messages the mods' own threads sent, each with the runtime it is of ([`Rt::address`]):
/// that runtime's game thread sends them with what waits on it ([`send_waiting`]).
static HANDED: std::sync::Mutex<Vec<(usize, Waiting)>> = std::sync::Mutex::new(Vec::new());

/// Ticks a message waits at most before it is given up: half a minute (a player who left, a
/// server that carries no mod messages).
const PATIENCE: u32 = 600;
/// Messages waiting at most.
const MOST_WAITING: usize = 8192;
/// The bytes of a message's data a part carries at most: the game takes 30,000 a message.
const PART: usize = 29_000;
/// The channel a large message's parts go on: no mod's id.
const PARTS_CHANNEL: &str = "hzcompat:part";
/// Ticks a message to a player may be the first to go and be refused before it is given up:
/// what waits behind it for the same player goes on then. (The window's messages to its server
/// are refused until it has joined, and wait [`PATIENCE`].)
const HEAD_PATIENCE: u32 = 20;

/// Whether the game carries a message on `channel` as it is: `mod_id:name`, the id of
/// `[a-z0-9_]` and the name of `[a-z0-9_./-]`, each 1 to 64 long, and not the game's own
/// (`minecraft`, `horizon`), as its mod messages are documented. A payload type of another
/// name (Puzzles Lib's `puzzleslib:`, with nothing after the colon; a Fabric mod's id with a
/// dash) goes inside a message of the runtime's own channel.
fn game_takes(channel: &str) -> bool {
    let Some((id, name)) = channel.split_once(':') else {
        return false;
    };
    (1..=64).contains(&id.len())
        && (1..=64).contains(&name.len())
        && !matches!(id, "minecraft" | "horizon")
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        && name.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b'/' | b'-')
        })
}

/// Sends a payload already encoded (`data` on channel `channel`) to `to`. One a screen's input
/// sends (its host only reads) goes with the client's next tick ([`crate::screens::flush`]).
/// One larger than the game takes in a message goes in parts, put together again where they
/// arrive ([`part_received`]), as NeoForge splits a payload past a packet's size (a quest
/// book's sync, a recipe viewer's).
pub fn send_raw(rt: &mut Rt, to: PayloadTarget, channel: &str, data: &[u8]) -> bool {
    if crate::screens::defer_payload(rt, to, channel, data) {
        return true;
    }
    if data.len() <= PART && game_takes(channel) {
        return send_whole(rt, to, channel, data);
    }
    // In parts, or in one for a small payload whose channel the game would not carry.
    if channel.len() > 1024 {
        return false;
    }
    let number = SPLIT
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        .wrapping_add(1);
    let chunks: Vec<&[u8]> = if data.is_empty() {
        vec![data]
    } else {
        data.chunks(PART - channel.len() - 10).collect()
    };
    let Ok(count) = u16::try_from(chunks.len()) else {
        return false;
    };
    let mut sent = true;
    for (i, chunk) in chunks.iter().enumerate() {
        let mut part = Vec::with_capacity(chunk.len() + channel.len() + 10);
        part.extend(number.to_be_bytes());
        part.extend((i as u16).to_be_bytes());
        part.extend(count.to_be_bytes());
        part.extend((channel.len() as u16).to_be_bytes());
        part.extend(channel.as_bytes());
        part.extend(*chunk);
        sent &= send_whole(rt, to, PARTS_CHANNEL, &part);
    }
    sent
}

/// Sends one message through the game, or keeps it to go with a later tick: the game takes 32
/// a tick to one player (and from the window to its server), and at a login the pack's mods
/// send a player more than that at once (their configs, their registries, the quests, the
/// claimed chunks, the waystones: past the first 32 each was dropped). A message to one player
/// or to the server waits behind what already waits for them, so they arrive in the order
/// sent; one to several players that none of them could take waits for each of those the
/// level has.
fn send_whole(rt: &mut Rt, to: PayloadTarget, channel: &str, data: &[u8]) -> bool {
    let one = matches!(to, PayloadTarget::Server | PayloadTarget::Player(_));
    // From a thread of the mods' own a message always waits: for the game's thread, which
    // sends what waits in the order it was sent ([`send_waiting`]).
    let own = rt.thread_id() != 0;
    let behind = own || (one && WAITING.with(|w| w.borrow().iter().any(|m| m.to == to)));
    let sent = !behind
        && rt
            .host_mut()
            .is_some_and(|h| h.send_payload(to, channel, data));
    if sent {
        crate::glue::trace_some(rt, "payload-sent", channel, 40, |_| {
            format!("payload {channel} ({} bytes) to {to:?}: sent", data.len())
        });
        return true;
    }
    // Who it waits for. A host that only reads sends nothing, now or later.
    let Some(host) = rt.host().filter(|h| !h.read_only()) else {
        return false;
    };
    let for_whom: Vec<PayloadTarget> = match to {
        PayloadTarget::Server | PayloadTarget::Player(_) => vec![to],
        PayloadTarget::All => host
            .players()
            .into_iter()
            .map(|(uuid, _)| PayloadTarget::Player(uuid))
            .collect(),
        PayloadTarget::Others(but) => host
            .players()
            .into_iter()
            .filter(|&(uuid, _)| uuid != but)
            .map(|(uuid, _)| PayloadTarget::Player(uuid))
            .collect(),
        PayloadTarget::Near { x, y, z, radius } => host
            .players()
            .into_iter()
            .filter(|&(_, at)| {
                let d = [at[0] - x, at[1] - y, at[2] - z];
                d[0] * d[0] + d[1] * d[1] + d[2] * d[2] <= radius * radius
            })
            .map(|(uuid, _)| PayloadTarget::Player(uuid))
            .collect(),
    };
    if own {
        let mut handed = HANDED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if handed.len() + for_whom.len() > MOST_WAITING {
            return false;
        }
        let at = rt.address();
        for to in for_whom {
            handed.push((
                at,
                Waiting {
                    to,
                    channel: channel.to_owned(),
                    data: data.to_vec(),
                    ticks: 0,
                    refused: 0,
                },
            ));
        }
        return true;
    }
    let waits = !for_whom.is_empty()
        && WAITING.with(|w| {
            let mut w = w.borrow_mut();
            if w.len() + for_whom.len() > MOST_WAITING {
                return false;
            }
            for to in for_whom {
                w.push_back(Waiting {
                    to,
                    channel: channel.to_owned(),
                    data: data.to_vec(),
                    ticks: 0,
                    refused: 0,
                });
            }
            true
        });
    crate::glue::trace_some(rt, "payload-sent", channel, 40, |_| {
        format!(
            "payload {channel} ({} bytes) to {to:?}: {}",
            data.len(),
            if waits { "waits its turn" } else { "not sent" }
        )
    });
    waits
}

/// A tick's start on a server or a client: the messages that waited go, each receiver's in
/// the order they were sent, until the game takes no more for that one this tick. The first
/// to go to a receiver finds the tick's allowance whole: refused, it is that message the game
/// does not take (or the receiver is gone), and after [`HEAD_PATIENCE`] such ticks a player's
/// is given up, so that what waits behind it goes on. One that waited past [`PATIENCE`] is
/// given up too.
pub fn send_waiting(rt: &mut Rt) {
    // What the mods' own threads of this runtime sent since: behind what already waits.
    if rt.has_threads() {
        let mut handed = HANDED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !handed.is_empty() {
            let at = rt.address();
            let (mine, others): (Vec<_>, Vec<_>) = handed.drain(..).partition(|(of, _)| *of == at);
            *handed = others;
            WAITING.with(|w| w.borrow_mut().extend(mine.into_iter().map(|(_, m)| m)));
        }
    }
    if WAITING.with(|w| w.borrow().is_empty()) {
        return;
    }
    let waiting: Vec<Waiting> = WAITING.with(|w| w.borrow_mut().drain(..).collect());
    // The receivers the game took something for this tick, and those it takes no more for.
    let (mut took, mut full): (Vec<PayloadTarget>, Vec<PayloadTarget>) = (Vec::new(), Vec::new());
    let mut left = std::collections::VecDeque::new();
    for mut m in waiting {
        if !full.contains(&m.to) {
            if rt
                .host_mut()
                .is_some_and(|h| h.send_payload(m.to, &m.channel, &m.data))
            {
                if !took.contains(&m.to) {
                    took.push(m.to);
                }
                continue;
            }
            if took.contains(&m.to) {
                full.push(m.to);
            } else {
                m.refused += 1;
            }
        }
        m.ticks += 1;
        let lost =
            m.ticks > PATIENCE || (m.refused > HEAD_PATIENCE && m.to != PayloadTarget::Server);
        if lost {
            let (channel, to, ticks) = (m.channel.clone(), m.to, m.ticks);
            crate::glue::trace_some(rt, "payload-given-up", &channel, 8, |_| {
                format!("payload {channel} to {to:?} waited {ticks} ticks: given up")
            });
            continue;
        }
        // (One refused as the first holds the rest of its receiver's back for this tick, in
        // their order.)
        if !took.contains(&m.to) && !full.contains(&m.to) {
            full.push(m.to);
        }
        left.push_back(m);
    }
    WAITING.with(|w| {
        let mut w = w.borrow_mut();
        // What a hook sent meanwhile stays behind what already waited.
        left.extend(w.drain(..));
        *w = left;
    });
}

/// The world is gone: what waited to be sent in it, and the parts of what was still arriving,
/// are dropped (a world opened next has the same player, by the same UUID).
pub fn forget_waiting() {
    WAITING.with(|w| w.borrow_mut().clear());
    PARTS.with(|p| p.borrow_mut().clear());
}

/// A part of a large message ([`send_raw`]): kept until the others have come, then the whole
/// is received as the message it is. True when the part reads.
fn part_received(rt: &mut Rt, data: &[u8], player: Option<u128>) -> bool {
    let Some((head, rest)) = data.split_at_checked(10) else {
        return false;
    };
    let number = u32::from_be_bytes([head[0], head[1], head[2], head[3]]);
    let (index, count) = (
        usize::from(u16::from_be_bytes([head[4], head[5]])),
        usize::from(u16::from_be_bytes([head[6], head[7]])),
    );
    let Some((channel, chunk)) =
        rest.split_at_checked(usize::from(u16::from_be_bytes([head[8], head[9]])))
    else {
        return false;
    };
    let Ok(channel) = std::str::from_utf8(channel) else {
        return false;
    };
    if index >= count {
        return false;
    }
    let whole = PARTS.with(|parts| {
        let mut parts = parts.borrow_mut();
        // A sender with too much begun and never ended starts over.
        if !parts.contains_key(&(player, number))
            && parts.keys().filter(|k| k.0 == player).count() >= 16
        {
            parts.retain(|k, _| k.0 != player);
        }
        let got = parts
            .entry((player, number))
            .or_insert_with(|| vec![None; count]);
        if got.len() != count {
            return None;
        }
        got[index] = Some(chunk.to_vec());
        if got.iter().any(Option::is_none) {
            return None;
        }
        parts
            .remove(&(player, number))
            .map(|all| all.into_iter().flatten().flatten().collect::<Vec<u8>>())
    });
    match whole {
        Some(whole) => receive(rt, channel, &whole, player),
        None => true,
    }
}

/// A payload that came over the mod channel: read with its type's codec and handed to its handler.
/// `player` is who sent it, on a server's runtime; None on a client's. False when no payload type
/// has that id here.
pub fn receive(rt: &mut Rt, channel: &str, data: &[u8], player: Option<u128>) -> bool {
    if channel == PARTS_CHANNEL {
        return part_received(rt, data, player);
    }
    let Some(entry) = rt.interned.get(&format!("payload/{channel}")).copied() else { return false };
    let parts = rt.captured(entry);
    let (codec, handler) = (parts.first().copied().unwrap_or_default(), parts.get(1).copied().unwrap_or_default());
    let buf = buffer_with(rt, data.to_vec());
    let payload = decode(rt, codec, buf);
    if rt.thrown.is_some() {
        rt.report_thrown(&format!("decoding payload {channel}"));
        return true;
    }
    crate::glue::trace_some(rt, "payload-received", channel, 40, |_| format!("payload {channel} ({} bytes) received", data.len()));
    if handler.r() != 0 {
        let ctx = context(rt, player);
        rt.call_functional(HANDLE, handler, &[payload, ctx]);
        rt.report_thrown(&format!("the handler of payload {channel}"));
    }
    true
}

/// The client's own player (`Minecraft.player`, `IPayloadContext.player()` on the client).
pub fn local_player(rt: &mut Rt) -> V {
    if let Some(&p) = rt.interned.get("compat:local-player") {
        return p;
    }
    let p = rt.new_object("net/minecraft/client/player/LocalPlayer");
    crate::menus::init_player(rt, p);
    rt.interned.insert("compat:local-player".into(), p);
    // Its constructors' end, as mixins inject there.
    crate::places::constructed(rt, p, "net/minecraft/client/player/LocalPlayer", &[]);
    p
}

/// A payload handler's `IPayloadContext`: from `player` (a server's runtime), or from the server.
pub(crate) fn context(rt: &mut Rt, player: Option<u128>) -> V {
    let ctx = rt.new_object(CONTEXT);
    let p = match player {
        Some(uuid) => {
            // The same object in every hook, so what the menus keep on it (`containerMenu`) stays.
            let p = crate::menus::server_player(rt, uuid);
            // Where the sender stands, when the server knows (a packet's reach checks:
            // `canInteractWithBlock`).
            if let Some((_, at)) = rt.host().and_then(|h| h.players().into_iter().find(|(id, _)| *id == uuid)) {
                for (f, v) in ["$x", "$y", "$z"].into_iter().zip(at) {
                    rt.put_field(p, f, V::D(v));
                }
                rt.put_field(p, "$yaw", V::F(0.0));
                rt.put_field(p, "$pitch", V::F(0.0));
            }
            p
        }
        None => local_player(rt),
    };
    rt.put_field(ctx, "$player", p);
    rt.put_field(ctx, "$serverbound", V::I(i32::from(player.is_some())));
    ctx
}

/// The player a `ServerPlayer` object stands for, when it says (a payload's sender does).
pub(crate) fn player_uuid(rt: &Rt, p: V) -> Option<u128> {
    let u = rt.get_field(p, "$uuid");
    (u.r() != 0).then(|| uuid_u128(rt, u))
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, _desc: &str) -> bool {
    matches!(
        owner,
        "io/netty/buffer/ByteBuf"
            | "net/minecraft/network/FriendlyByteBuf"
            | "net/minecraft/network/RegistryFriendlyByteBuf"
            | "net/minecraft/network/VarInt"
            | "net/minecraft/network/VarLong"
            | "io/netty/buffer/Unpooled"
            | "io/netty/buffer/ByteBufUtil"
            | "net/minecraft/network/codec/StreamCodec"
            | "net/minecraft/network/codec/StreamEncoder"
            | "net/minecraft/network/codec/StreamDecoder"
            | "net/minecraft/network/codec/ByteBufCodecs"
            | "net/minecraft/network/codec/StreamCodec$CodecOperation"
            | NEO_CODECS
            | REGISTRAR
            | PAYLOAD_TYPE
            | DISTRIBUTOR
            | CONTEXT
            | CLIENT_LISTENER
            | CLIENT_COMMON_LISTENER
            | CONNECTION
            | SERVER_LISTENER
            | SERVER_COMMON_LISTENER
    ) || (owner == REGISTER_PAYLOADS && name == "registrar")
        || (matches!(owner, SERVER_PLAYER | LOCAL_PLAYER) && name == "$connection")
        || (owner == "net/minecraft/network/protocol/common/custom/CustomPacketPayload"
            && matches!(name, "createType" | "type"))
        || (owner.starts_with(NEO_PAYLOADS) && name == "type")
        || (owner == "net/minecraft/resources/ResourceKey" && name == "streamCodec")
}

/// NeoForge's own payloads.
const NEO_PAYLOADS: &str = "net/neoforged/neoforge/network/payload/";

/// The channel one of NeoForge's own payloads is sent on (its `ID`, as NeoForge 21.1 has them):
/// vanilla's channel announcements, the common ones and NeoForge's own negotiation.
fn neo_payload_channel(class: &str) -> Option<&'static str> {
    Some(match class.strip_prefix(NEO_PAYLOADS)? {
        "MinecraftRegisterPayload" => "minecraft:register",
        "MinecraftUnregisterPayload" => "minecraft:unregister",
        "CommonRegisterPayload" => "c:register",
        "CommonVersionPayload" => "c:version",
        "ModdedNetworkQueryPayload" => "neoforge:register",
        "ModdedNetworkPayload" => "neoforge:network",
        "ModdedNetworkSetupFailedPayload" => "neoforge:modded_network_setup_failed",
        _ => return None,
    })
}

/// The type of the payload sent on `channel` (a payload class's `TYPE`): one object.
fn neo_payload_type(rt: &mut Rt, channel: &'static str) -> V {
    rt.intern(format!("compat:payload-type/{channel}"), |rt| {
        let t = rt.new_object(PAYLOAD_TYPE);
        let id = rt.string(channel);
        rt.put_field(t, "$id", id);
        t
    })
}

const CLIENT_LISTENER: &str = "net/minecraft/client/multiplayer/ClientPacketListener";
const CLIENT_COMMON_LISTENER: &str = "net/minecraft/client/multiplayer/ClientCommonPacketListenerImpl";
const CONNECTION: &str = "net/minecraft/network/Connection";
const SERVERBOUND_PAYLOAD: &str = "net/minecraft/network/protocol/common/ServerboundCustomPayloadPacket";

/// `Minecraft.getConnection()`: the client's connection to its server while its player plays in a
/// world (vanilla's `player.connection`), else null. The server's thread sees it too, as
/// NeoForge's single player has it (the integrated server runs in the client's JVM, `Dist` the
/// client's): there once a player is in the server, the one acting in its own hook (whom the
/// server's list leaves out then) or another. A mod asks it for the registries from either thread
/// (Create's `GlobalRegistryAccess`, whose client way `Dist.CLIENT` picks), the server's own here.
pub(crate) fn client_connection(rt: &mut Rt) -> V {
    let connected = if crate::neo::is_client(rt) {
        crate::world_events::client_in_world(rt)
    } else {
        rt.host().is_some_and(|h| h.player_id().is_some() || !h.players().is_empty())
    };
    if !connected {
        return V::NULL;
    }
    if let Some(&connection) = rt.interned.get("compat:client-connection") {
        return connection;
    }
    let connection = rt.new_object(CLIENT_LISTENER);
    rt.interned
        .insert("compat:client-connection".into(), connection);
    // Its recipe manager, a field its constructor sets.
    let recipes = crate::api::call(
        rt,
        "net/minecraft/client/multiplayer/ClientPacketListener.getRecipeManager.()Lnet/minecraft/world/item/crafting/RecipeManager;",
        &[connection],
    );
    rt.put_field(connection, "recipeManager", recipes);
    // Its constructor's end, as mixins inject there (a mod takes the connection's recipes and
    // registries as it is made).
    if crate::places::constructors_heard(rt, CLIENT_LISTENER) {
        let mc = crate::api::call(
            rt,
            "net/minecraft/client/Minecraft.getInstance.()Lnet/minecraft/client/Minecraft;",
            &[],
        );
        crate::places::constructed(
            rt,
            connection,
            CLIENT_LISTENER,
            &[("Lnet/minecraft/client/Minecraft;", mc)],
        );
    }
    connection
}

/// The mod's payload a packet carries: the packet itself when it is one (NeoForge's
/// `send(payload)`), or what a custom-payload packet of class `wrapper` wraps (`new
/// ServerboundCustomPayloadPacket(payload)`). The game's own packets carry none: they are the game's
/// to send, and one a mod builds isn't sent. It is counted as unmapped, under its class
/// (`ClientboundSetTitleTextPacket.<sent>`), so a round tells which ones mods send.
fn mod_payload(rt: &mut Rt, packet: V, wrapper: &str) -> Option<V> {
    let class = rt.class_of(packet);
    let payload = if class == wrapper {
        rt.get_field(packet, "payload")
    } else if class.starts_with("net/minecraft/") {
        let key = rt.static_key(&format!("{class}.<sent>.()V"));
        crate::miss(rt, key);
        V::NULL
    } else {
        packet
    };
    (payload.r() != 0).then_some(payload)
}

/// A packet the client sends its server: a mod's payload goes on the mods' channel, as
/// `PacketDistributor.sendToServer` sends it.
fn send_to_server(rt: &mut Rt, packet: V) {
    if let Some(payload) = mod_payload(rt, packet, SERVERBOUND_PAYLOAD) {
        send(rt, PayloadTarget::Server, payload);
    }
}

/// The client's connection's calls: sending, and what it knows of the server (its registries).
fn client_listener_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "send" => {
            send_to_server(rt, a(1));
            V::Void
        }
        "registryAccess" => rt.intern("compat:registry-access".into(), |rt| rt.new_object("net/minecraft/core/RegistryAccess")),
        "getConnection" => rt.intern("compat:client-network-connection".into(), |rt| rt.new_object(CONNECTION)),
        "getLevel" => crate::glue::level(rt),
        // The tab list's players: the game doesn't hand it over, so no one is known (vanilla's null
        // for a UUID not on it).
        "getPlayerInfo" => V::NULL,
        "getOnlinePlayers" | "getListedOnlinePlayers" => crate::jutil::new_list(rt, Vec::new()),
        _ => return None,
    })
}

/// `Connection`: `send(packet)` as its listener's: a server player's sends to that player, the
// client's to its server, connected while the client plays.
fn connection_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let listener = rt.get_field(a(0), "$listener");
    Some(match name {
        "send" if listener.r() != 0 => {
            send_to_player(rt, listener, a(1));
            V::Void
        }
        "send" => {
            send_to_server(rt, a(1));
            V::Void
        }
        "isConnected" if listener.r() != 0 => V::I(1),
        "isConnected" => V::I(i32::from(client_connection(rt).r() != 0)),
        _ => return None,
    })
}

const SERVER_PLAYER: &str = "net/minecraft/server/level/ServerPlayer";
const LOCAL_PLAYER: &str = "net/minecraft/client/player/LocalPlayer";
const SERVER_LISTENER: &str = "net/minecraft/server/network/ServerGamePacketListenerImpl";
const SERVER_COMMON_LISTENER: &str = "net/minecraft/server/network/ServerCommonPacketListenerImpl";
const CLIENTBOUND_PAYLOAD: &str = "net/minecraft/network/protocol/common/ClientboundCustomPayloadPacket";

/// `serverPlayer.connection`: the player's connection to their client, one for each player object,
/// which knows its player (`connection.player`).
fn player_connection(rt: &mut Rt, player: V) -> V {
    if player.r() == 0 {
        return V::NULL;
    }
    let c = rt.get_field(player, "$connection");
    if c.r() != 0 {
        return c;
    }
    let c = rt.new_object(SERVER_LISTENER);
    rt.put_field(c, "player", player);
    rt.put_field(player, "$connection", c);
    c
}

/// A packet the server sends one player on their connection: a mod's payload, bare or in a
/// `ClientboundCustomPayloadPacket`, goes to that player on the mods' channel, as
/// `PacketDistributor.sendToPlayer` sends it. A fake player has no client (NeoForge's `FakePlayer`
/// drops what it is sent).
fn send_to_player(rt: &mut Rt, connection: V, packet: V) {
    let player = rt.get_field(connection, "player");
    let Some(uuid) = player_uuid(rt, player).filter(|_| !crate::fake_players::is_fake(rt, player)) else { return };
    if let Some(payload) = mod_payload(rt, packet, CLIENTBOUND_PAYLOAD) {
        send(rt, PayloadTarget::Player(uuid), payload);
    }
}

/// The id of a payload's channel, as `hasChannel` is asked it: a `CustomPacketPayload.Type`, a
/// `ResourceLocation`, or a payload.
fn channel_id(rt: &mut Rt, v: V) -> Option<String> {
    let ty = match rt.class_of(v) {
        PAYLOAD_TYPE => v,
        _ if rt.str_of(v).is_some() => return rt.str_of(v).map(str::to_owned),
        _ => rt.call_functional(TYPE_OF, v, &[]),
    };
    rt.str_of(rt.get_field(ty, "$id")).map(str::to_owned)
}

/// A server player's connection's calls: sending them packets, and what it knows of their client.
fn server_listener_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "send" => {
            send_to_player(rt, a(0), a(1));
            V::Void
        }
        "getPlayer" => rt.get_field(a(0), "player"),
        // The client runs the same mods, so it has every channel the server registered.
        "hasChannel" => {
            let id = channel_id(rt, a(1));
            V::I(i32::from(id.is_some_and(|id| registered(rt).contains(&id))))
        }
        "isAcceptingMessages" => V::I(1),
        "getConnection" => {
            let c = rt.get_field(a(0), "$connection");
            if c.r() != 0 {
                return Some(c);
            }
            let c = rt.new_object(CONNECTION);
            rt.put_field(c, "$listener", a(0));
            rt.put_field(a(0), "$connection", c);
            c
        }
        _ => return None,
    })
}

/// A networking call, or None when this module doesn't model it.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match owner {
        "io/netty/buffer/ByteBuf" | "net/minecraft/network/FriendlyByteBuf" | "net/minecraft/network/RegistryFriendlyByteBuf" => buffer_call(rt, name, desc, args),
        // `VarInt.write(buf, v)`: what `FriendlyByteBuf.writeVarInt` calls, used directly by codecs.
        "net/minecraft/network/VarInt" | "net/minecraft/network/VarLong" => Some(match name {
            "write" => {
                let u = if owner.ends_with("VarInt") { u64::from(a(1).i() as u32) } else { a(1).j() as u64 };
                put_var(rt, a(0), u);
                a(0)
            }
            "read" if owner.ends_with("VarInt") => V::I(read_var_int(rt, a(0)).unwrap_or(0)),
            "read" => V::J(read_var(rt, a(0), 10).unwrap_or(0) as i64),
            "getByteSize" if owner.ends_with("VarInt") => V::I(var_bytes(u64::from(a(0).i() as u32)).len() as i32),
            "getByteSize" => V::I(var_bytes(a(0).j() as u64).len() as i32),
            "hasContinuationBit" => V::I((a(0).i() & 0x80 != 0) as i32),
            _ => return None,
        }),
        "io/netty/buffer/Unpooled" => Some(match name {
            "buffer" | "directBuffer" => new_buffer(rt),
            "wrappedBuffer" | "copiedBuffer" if desc.starts_with("([B") => {
                let b = byte_array(rt, a(0));
                buffer_with(rt, b)
            }
            // Of a buffer, or of several one after another: what each has left to read.
            "wrappedBuffer" | "copiedBuffer" | "unreleasableBuffer" | "unmodifiableBuffer"
                if desc.starts_with("(Lio/netty/buffer/ByteBuf;)")
                    || desc.starts_with("([Lio/netty/buffer/ByteBuf;)") =>
            {
                let parts = if desc.starts_with("([") {
                    crate::jutil::elements(rt, a(0))
                } else {
                    vec![a(0)]
                };
                let mut all = Vec::new();
                for part in parts {
                    let (bytes, at) =
                        bytes_mut(rt, part).map_or((Vec::new(), 0), |(v, at)| (v.clone(), *at));
                    all.extend_from_slice(&bytes[at.min(bytes.len())..]);
                }
                buffer_with(rt, all)
            }
            _ => return None,
        }),
        // `ByteBufUtil`: a buffer's bytes as an array (what is left to read, or a stretch of
        // it by index; the buffer stays as it was), and text written as UTF-8, which answers
        // how many bytes that was (a mod checks the count against the length it wrote first).
        "io/netty/buffer/ByteBufUtil" => Some(match name {
            "getBytes" => {
                let (all, at) =
                    bytes_mut(rt, a(0)).map_or((Vec::new(), 0), |(v, at)| (v.clone(), *at));
                let (start, len) = match desc.starts_with("(Lio/netty/buffer/ByteBuf;II") {
                    true => (a(1).i(), a(2).i()),
                    false => (at as i32, (all.len() - at.min(all.len())) as i32),
                };
                match all.get(
                    start.max(0) as usize
                        ..(start.max(0) as usize).saturating_add(len.max(0) as usize),
                ) {
                    Some(part) if start >= 0 && len >= 0 => new_byte_array(rt, part),
                    _ => crate::jlang::throw_with(
                        rt,
                        "java/lang/IndexOutOfBoundsException",
                        &format!(
                            "index: {start}, length: {len} (expected: range(0, {}))",
                            all.len()
                        ),
                    ),
                }
            }
            "writeUtf8" | "writeAscii" if desc.ends_with("Ljava/lang/CharSequence;)I") => {
                let s = crate::jlang::text(rt, a(1));
                put(rt, a(0), s.as_bytes());
                V::I(s.len() as i32)
            }
            "utf8Bytes" => V::I(crate::jlang::text(rt, a(0)).len() as i32),
            _ => return None,
        }),
        "net/minecraft/network/codec/StreamCodec"
        | "net/minecraft/network/codec/StreamEncoder"
        | "net/minecraft/network/codec/StreamDecoder"
        | "net/minecraft/network/codec/ByteBufCodecs"
        | "net/minecraft/network/codec/StreamCodec$CodecOperation"
        | NEO_CODECS => codec_call(rt, owner, name, desc, args),
        "net/minecraft/resources/ResourceKey" if name == "streamCodec" => codec_call(rt, owner, name, desc, args),
        REGISTER_PAYLOADS if name == "registrar" => {
            let r = rt.new_object(REGISTRAR);
            rt.put_field(r, "$version", a(1));
            Some(r)
        }
        REGISTRAR => Some(match name {
            "playToClient" | "playToServer" | "playBidirectional" | "commonToClient" | "commonToServer" | "commonBidirectional" | "configurationToClient"
            | "configurationToServer" | "configurationBidirectional" => {
                let id = text(rt, rt.get_field(a(1), "$id"));
                let handler = if args.len() > 3 { a(3) } else { V::NULL };
                let entry = rt.alloc("compat:payload", Data::Array(vec![a(2), handler]));
                crate::glue::trace_some(rt, "payload-registered", &id, 400, |_| format!("payload {id} registered ({name})"));
                rt.interned.insert(format!("payload/{id}"), entry);
                a(0)
            }
            "versioned" | "optional" | "executesOn" => a(0),
            _ => return None,
        }),
        PAYLOAD_TYPE => Some(match name {
            "<init>" => {
                rt.put_field(a(0), "$id", a(1));
                V::Void
            }
            "id" => rt.get_field(a(0), "$id"),
            "equals" => {
                let same = rt.class_of(a(1)) == PAYLOAD_TYPE && rt.str_of(rt.get_field(a(0), "$id")) == rt.str_of(rt.get_field(a(1), "$id"));
                V::I(i32::from(same))
            }
            "hashCode" => {
                let id = rt.get_field(a(0), "$id");
                V::I(crate::jutil::hash(rt, id))
            }
            "toString" => {
                let s = format!("Type[id={}]", text(rt, rt.get_field(a(0), "$id")));
                rt.string(&s)
            }
            _ => return None,
        }),
        // `payload.type()` of one of NeoForge's own payloads (a mod's class answers for its own):
        // the Fabric API announces its channels to the server with `minecraft:register` and asks
        // the payload for its type first.
        o if name == "type"
            && (o == "net/minecraft/network/protocol/common/custom/CustomPacketPayload"
                || o.starts_with(NEO_PAYLOADS)) =>
        {
            let channel = neo_payload_channel(rt.class_of(a(0)))?;
            Some(neo_payload_type(rt, channel))
        }
        "net/minecraft/network/protocol/common/custom/CustomPacketPayload" if name == "createType" => {
            let t = rt.new_object(PAYLOAD_TYPE);
            let id = text(rt, a(0));
            let id = if id.contains(':') { id } else { format!("minecraft:{id}") };
            let id = rt.string(&id);
            rt.put_field(t, "$id", id);
            Some(t)
        }
        DISTRIBUTOR => distribute(rt, name, args),
        CONTEXT => context_call(rt, name, desc, args),
        CLIENT_LISTENER | CLIENT_COMMON_LISTENER => client_listener_call(rt, name, args),
        CONNECTION => connection_call(rt, name, args),
        SERVER_LISTENER | SERVER_COMMON_LISTENER => server_listener_call(rt, name, args),
        // The player's `connection` field.
        SERVER_PLAYER if name == "$connection" => Some(player_connection(rt, args.first().copied().unwrap_or_default())),
        LOCAL_PLAYER if name == "$connection" => Some(client_connection(rt)),
        _ => None,
    }
}

/// `PacketDistributor.sendTo*(..., payload, morePayloads...)`.
fn distribute(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let n = args.len();
    if n < 2 {
        return None;
    }
    let mut payloads = vec![args[n - 2]];
    payloads.extend(crate::jutil::elements(rt, args[n - 1]));
    let to = match name {
        "sendToServer" => PayloadTarget::Server,
        "sendToPlayer" => player_uuid(rt, args[0]).map_or(PayloadTarget::All, PayloadTarget::Player),
        // (level, excluded player, x, y, z, radius, ...)
        "sendToPlayersNear" if n >= 8 => PayloadTarget::Near { x: args[2].d(), y: args[3].d(), z: args[4].d(), radius: args[5].d() },
        // Those tracking a player: everyone but the player (a player's own client doesn't track them;
        // `...AndSelf` adds them back). Create tells the others of a rider's limb swing so.
        "sendToPlayersTrackingEntity" if crate::api_common::player_object(rt, args[0]) => {
            player_uuid(rt, args[0]).map_or(PayloadTarget::All, PayloadTarget::Others)
        }
        // Those tracking an entity or a chunk: the clients that don't have it drop what's about it.
        "sendToAllPlayers" | "sendToPlayersInDimension" | "sendToPlayersTrackingEntity" | "sendToPlayersTrackingEntityAndSelf" | "sendToPlayersTrackingChunk" => PayloadTarget::All,
        _ => return None,
    };
    for p in payloads {
        send(rt, to, p);
    }
    Some(V::Void)
}

fn context_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let ctx = a(0);
    let serverbound = rt.get_field(ctx, "$serverbound").i() != 0;
    Some(match name {
        "player" => rt.get_field(ctx, "$player"),
        // The handler runs on the game's thread already: the work runs now, and its future is done.
        "enqueueWork" => {
            let value = if desc.starts_with("(Ljava/lang/Runnable;") {
                rt.call_functional(RUN, a(1), &[]);
                V::NULL
            } else {
                rt.call_functional(SUPPLIER, a(1), &[])
            };
            crate::jutil::completed_future(rt, value)
        }
        "flow" => {
            if serverbound {
                crate::api::api_enum(rt, "net/minecraft/network/protocol/PacketFlow", "SERVERBOUND", 0)
            } else {
                crate::api::api_enum(rt, "net/minecraft/network/protocol/PacketFlow", "CLIENTBOUND", 1)
            }
        }
        "protocol" => crate::api::api_enum(rt, "net/minecraft/network/ConnectionProtocol", "PLAY", 1),
        "reply" => {
            let to = if serverbound {
                let p = rt.get_field(ctx, "$player");
                player_uuid(rt, p).map_or(PayloadTarget::All, PayloadTarget::Player)
            } else {
                PayloadTarget::Server
            };
            send(rt, to, a(1));
            V::Void
        }
        "disconnect" | "finishCurrentTask" | "handle" => V::Void,
        _ => return None,
    })
}

fn codec_call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let byte_buf_codecs = owner == "net/minecraft/network/codec/ByteBufCodecs";
    Some(match name {
        "encode" => {
            encode(rt, a(0), a(1), a(2));
            V::Void
        }
        "decode" => decode(rt, a(0), a(1)),
        // `ByteBufCodecs.list()` applied to a codec (`CodecOperation.apply`), or a codec's `apply(op)`.
        "apply" if owner.ends_with("$CodecOperation") => apply_op(rt, a(0), a(1)),
        "apply" => apply_op(rt, a(1), a(0)),
        "map" if byte_buf_codecs => make(rt, MAP_OF, vec![a(0), a(1), a(2)]),
        "map" => make(rt, MAPPED, vec![a(0), a(1), a(2)]),
        // The buffer's type changes, its bytes don't.
        "mapStream" | "cast" => a(0),
        "dispatch" => make(rt, DISPATCH, vec![a(0), a(1), a(2)]),
        "unit" => make(rt, UNIT, vec![a(0)]),
        "of" => make(rt, OF, vec![a(0), a(1)]),
        "ofMember" => make(rt, MEMBER, vec![a(0), a(1)]),
        "composite" if !args.is_empty() => {
            let mut parts = vec![args[args.len() - 1]];
            parts.extend_from_slice(&args[..args.len() - 1]);
            make(rt, COMPOSITE, parts)
        }
        "optional" if byte_buf_codecs => make(rt, OPTIONAL, vec![a(0)]),
        "either" if byte_buf_codecs => make(rt, EITHER, vec![a(0), a(1)]),
        "recursive" if owner == "net/minecraft/network/codec/StreamCodec" => {
            let wrapped = make(rt, RECURSIVE, vec![a(0)]);
            make(rt, LAZY, vec![wrapped])
        }
        // `collection(factory)` alone is an operation on a codec; with the codec, the codec.
        "collection" if byte_buf_codecs && args.len() == 1 => make(rt, COLLECTION_OP, vec![a(0)]),
        "collection" if byte_buf_codecs => make(rt, COLLECTION, vec![a(0), a(1)]),
        "list" if byte_buf_codecs => constant(rt, LIST_OP),
        "idMapper" if desc.starts_with("(Lnet/minecraft/core/IdMap;") => make(rt, ID_MAP, vec![a(0)]),
        "idMapper" => make(rt, ID_FN, vec![a(0), a(1)]),
        "registry" if byte_buf_codecs => make(rt, REGISTRY, vec![a(0)]),
        "holderRegistry" if byte_buf_codecs => make(rt, HOLDER, vec![a(0)]),
        "fromCodec" | "fromCodecWithRegistries" | "fromCodecTrusted" | "fromCodecWithRegistriesTrusted" if byte_buf_codecs => make(rt, FROM_CODEC, vec![a(0)]),
        "streamCodec" if owner == "net/minecraft/resources/ResourceKey" => make(rt, RESOURCE_KEY, vec![a(0)]),
        "enumCodec" if owner == NEO_CODECS => make(rt, ENUM, vec![a(0)]),
        "lazy" if owner == NEO_CODECS => make(rt, LAZY, vec![a(0)]),
        "registryKey" if owner == NEO_CODECS => constant(rt, REGISTRY_KEY),
        "uncheckedUnit" if owner == NEO_CODECS => make(rt, UNIT, vec![a(0)]),
        // `connectionAware(neoForgeCodec, otherCodec)`: both ends here are NeoForge's.
        "connectionAware" if owner == NEO_CODECS => a(0),
        "byteArray" if byte_buf_codecs => constant(rt, BYTES),
        "stringUtf8" if byte_buf_codecs => constant(rt, STRING),
        _ => return None,
    })
}

/// `op.apply(codec)`: `ByteBufCodecs.list()` makes a list codec; a mod's own operation runs.
fn apply_op(rt: &mut Rt, op: V, codec: V) -> V {
    if rt.class_of(op) == LIST_OP {
        make(rt, COLLECTION, vec![V::NULL, codec])
    } else if rt.class_of(op) == COLLECTION_OP {
        let factory = rt.captured(op).first().copied().unwrap_or(V::NULL);
        make(rt, COLLECTION, vec![factory, codec])
    } else if rt.cid(op) == 0 {
        // An operation of the API's this module doesn't make (`ByteBufCodecs.optional()`...):
        // asking it to run would come back here. Counted as unmapped; the codec stays as it is.
        crate::miss(rt, OPERATION);
        codec
    } else {
        rt.call_functional(OPERATION, op, &[codec])
    }
}

fn buffer_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let b = a(0);
    Some(match name {
        // `new FriendlyByteBuf(inner)`, `new RegistryFriendlyByteBuf(inner, registries[, type])`.
        "<init>" => {
            rt.put_field(b, "$inner", a(1));
            V::Void
        }
        "writeBoolean" => {
            put(rt, b, &[u8::from(a(1).i() != 0)]);
            b
        }
        "writeByte" => {
            put(rt, b, &[a(1).i() as u8]);
            b
        }
        "writeShort" | "writeChar" => {
            put(rt, b, &(a(1).i() as u16).to_be_bytes());
            b
        }
        "writeInt" => {
            put(rt, b, &a(1).i().to_be_bytes());
            b
        }
        "writeLong" => {
            put(rt, b, &a(1).j().to_be_bytes());
            b
        }
        "writeFloat" => {
            put(rt, b, &a(1).f().to_be_bytes());
            b
        }
        "writeDouble" => {
            put(rt, b, &a(1).d().to_be_bytes());
            b
        }
        "writeVarInt" => {
            put_var(rt, b, u64::from(a(1).i() as u32));
            b
        }
        "writeVarLong" => {
            put_var(rt, b, a(1).j() as u64);
            b
        }
        "writeUtf" | "writeResourceLocation" => {
            let s = text(rt, a(1));
            put_utf(rt, b, &s);
            b
        }
        "writeBlockPos" => {
            put_pos(rt, b, a(1));
            b
        }
        "writeUUID" => {
            put_uuid(rt, b, a(1));
            b
        }
        "writeNbt" => {
            let n = crate::nbt::of_tag(rt, a(1));
            put_nbt(rt, b, n.as_ref());
            b
        }
        "writeEnum" => {
            let o = ordinal(rt, a(1));
            put_var(rt, b, o as u64);
            b
        }
        "writeVec3" => {
            put_vec3(rt, b, a(1));
            V::Void
        }
        "writeByteArray" => {
            let bytes = byte_array(rt, a(1));
            put_var(rt, b, bytes.len() as u64);
            put(rt, b, &bytes);
            b
        }
        "writeBytes" if desc.starts_with("([B)") => {
            let bytes = byte_array(rt, a(1));
            put(rt, b, &bytes);
            b
        }
        // `writeBytes(array, from, length)`; from another buffer what it has left to read
        // (`writeBytes(buffer)`, or `length` of it), which that buffer has then read; or
        // `writeBytes(buffer, from, length)`, which leaves it as it was.
        "writeBytes" => {
            let params = crate::jlang::kinds(desc).len();
            let (all, at, moves) = if desc.starts_with("([B") {
                (byte_array(rt, a(1)), a(2).i().max(0) as usize, false)
            } else {
                let (all, at) =
                    bytes_mut(rt, a(1)).map_or((Vec::new(), 0), |(v, at)| (v.clone(), *at));
                if params == 3 {
                    (all, a(2).i().max(0) as usize, false)
                } else {
                    (all, at, true)
                }
            };
            let length = match params {
                1 => all.len().saturating_sub(at),
                n => a(n).i().max(0) as usize,
            };
            match all.get(at..at.saturating_add(length)) {
                Some(part) => {
                    put(rt, b, part);
                    if moves && let Some((_, read)) = bytes_mut(rt, a(1)) {
                        *read += length;
                    }
                }
                None => rt.throw_new("java/lang/IndexOutOfBoundsException"),
            }
            b
        }
        // (value, encoder): instance and static (`writeNullable(buf, value, encoder)`) alike.
        "writeNullable" => {
            put(rt, b, &[u8::from(a(1).r() != 0)]);
            if a(1).r() != 0 {
                encode(rt, a(2), b, a(1));
            }
            V::Void
        }
        "writeOptional" => {
            let held = optional_held(rt, a(1));
            put(rt, b, &[u8::from(held.is_some())]);
            if let Some(v) = held {
                encode(rt, a(2), b, v);
            }
            V::Void
        }
        "writeCollection" => {
            let items = crate::jutil::elements(rt, a(1));
            put_var(rt, b, items.len() as u64);
            for v in items {
                encode(rt, a(2), b, v);
            }
            V::Void
        }
        "writeMap" => {
            let pairs = crate::jutil::table_pairs(rt, a(1));
            put_var(rt, b, pairs.len() as u64);
            for (k, v) in pairs {
                encode(rt, a(2), b, k);
                encode(rt, a(3), b, v);
            }
            V::Void
        }
        "readBoolean" => V::I(i32::from(take_n::<1>(rt, b).is_some_and(|x| x[0] != 0))),
        "readByte" => V::I(take_n::<1>(rt, b).map_or(0, |x| i32::from(x[0] as i8))),
        "readUnsignedByte" => V::I(take_n::<1>(rt, b).map_or(0, |x| i32::from(x[0]))),
        "readShort" => V::I(take_n(rt, b).map_or(0, |x| i32::from(i16::from_be_bytes(x)))),
        "readUnsignedShort" | "readChar" => V::I(take_n(rt, b).map_or(0, |x| i32::from(u16::from_be_bytes(x)))),
        "readInt" => V::I(take_n(rt, b).map_or(0, i32::from_be_bytes)),
        "readLong" => V::J(take_n(rt, b).map_or(0, i64::from_be_bytes)),
        "readFloat" => V::F(take_n(rt, b).map_or(0.0, f32::from_be_bytes)),
        "readDouble" => V::D(take_n(rt, b).map_or(0.0, f64::from_be_bytes)),
        "readVarInt" => V::I(read_var_int(rt, b).unwrap_or(0)),
        // `readEnum(class)`: the constant at the ordinal `writeEnum` wrote (`getEnumConstants()[i]`).
        "readEnum" => {
            let class = rt.str_of(a(1)).unwrap_or("").replace('.', "/");
            let ordinal = read_var_int(rt, b).unwrap_or(0);
            match crate::jlang::enum_constants(rt, &class) {
                Some(values) if (0..rt.array_len(values)).contains(&ordinal) => {
                    rt.array_get(values, ordinal)
                }
                Some(values) => {
                    let message = format!(
                        "Index {ordinal} out of bounds for length {}",
                        rt.array_len(values)
                    );
                    crate::jlang::throw_with(
                        rt,
                        "java/lang/ArrayIndexOutOfBoundsException",
                        &message,
                    )
                }
                None => V::NULL,
            }
        }
        "readVarLong" => V::J(read_var(rt, b, 10).map_or(0, |v| v as i64)),
        "readUtf" | "readResourceLocation" => match read_utf(rt, b) {
            Some(s) => rt.string(&s),
            None => V::NULL,
        },
        "readBlockPos" => read_pos(rt, b).unwrap_or(V::NULL),
        "readUUID" => read_uuid(rt, b).unwrap_or(V::NULL),
        "readVec3" => read_vec3(rt, b).unwrap_or(V::NULL),
        "readNbt" | "readAnySizeNbt" => match read_nbt(rt, b) {
            Some(Some(n)) => crate::nbt::tag_of(rt, &n),
            _ => V::NULL,
        },
        "readByteArray" => match read_count(rt, b).and_then(|n| take(rt, b, n)) {
            Some(bytes) => new_byte_array(rt, &bytes),
            None => V::NULL,
        },
        // (decoder): instance and static (`readNullable(buf, decoder)`) alike.
        "readNullable" => match take_n::<1>(rt, b) {
            Some([0]) | None => V::NULL,
            Some(_) => decode(rt, a(1), b),
        },
        "readOptional" => {
            let v = match take_n::<1>(rt, b) {
                Some([0]) | None => V::NULL,
                Some(_) => decode(rt, a(1), b),
            };
            crate::api_common::optional(rt, v)
        }
        "readCollection" | "readList" => {
            let (factory, decoder) = if name == "readList" { (V::NULL, a(1)) } else { (a(1), a(2)) };
            let Some(n) = read_count(rt, b) else { return Some(V::NULL) };
            let coll = if factory.r() == 0 { crate::jutil::new_list(rt, Vec::new()) } else { rt.call_functional(INT_FUNCTION, factory, &[V::I(n as i32)]) };
            for _ in 0..n {
                let v = decode(rt, decoder, b);
                if rt.thrown.is_some() {
                    break;
                }
                add_to(rt, coll, v);
            }
            coll
        }
        "readMap" => {
            // (factory, keys, values) or (keys, values) into a `HashMap`.
            let (factory, keys, values) = if args.len() > 3 { (a(1), a(2), a(3)) } else { (V::NULL, a(1), a(2)) };
            let Some(n) = read_count(rt, b) else { return Some(V::NULL) };
            let map = if factory.r() == 0 {
                rt.alloc("java/util/HashMap", Data::Table(crate::Table::default()))
            } else {
                rt.call_functional(INT_FUNCTION, factory, &[V::I(n as i32)])
            };
            for _ in 0..n {
                let k = decode(rt, keys, b);
                let v = decode(rt, values, b);
                if rt.thrown.is_some() {
                    break;
                }
                put_to(rt, map, k, v);
            }
            map
        }
        "writerIndex" if desc.starts_with("()") => V::I(bytes_mut(rt, b).map_or(0, |(v, _)| v.len() as i32)),
        "writerIndex" => {
            let n = a(1).i().max(0) as usize;
            if let Some((v, at)) = bytes_mut(rt, b) {
                v.resize(n, 0);
                *at = (*at).min(n);
            }
            b
        }
        "readerIndex" if desc.starts_with("()") => V::I(bytes_mut(rt, b).map_or(0, |(_, at)| *at as i32)),
        "readerIndex" => {
            let n = a(1).i().max(0) as usize;
            if let Some((v, at)) = bytes_mut(rt, b) {
                *at = n.min(v.len());
            }
            b
        }
        "readableBytes" => V::I(bytes_mut(rt, b).map_or(0, |(v, at)| (v.len() - *at) as i32)),
        "isReadable" => V::I(i32::from(bytes_mut(rt, b).is_some_and(|(v, at)| v.len() > *at))),
        "capacity" => V::I(bytes_mut(rt, b).map_or(0, |(v, _)| v.len() as i32)),
        "clear" => {
            if let Some((v, at)) = bytes_mut(rt, b) {
                v.clear();
                *at = 0;
            }
            b
        }
        "release" => V::I(1),
        "retain" | "asReadOnly" | "touch" | "ensureWritable" => b,
        "refCnt" => V::I(1),
        "isWritable" | "hasArray" => V::I(1),
        "arrayOffset" => V::I(0),
        "isDirect" => V::I(0),
        "writableBytes" | "maxWritableBytes" => {
            V::I(i32::MAX - bytes_mut(rt, b).map_or(0, |(v, _)| v.len() as i32))
        }
        "maxCapacity" => V::I(i32::MAX),
        // The next bytes: as a buffer of their own (`readBytes(n)`, `readSlice(n)`), into an
        // array (all of it, or `length` from an index), or into another buffer.
        "readSlice" | "readRetainedSlice" => match take(rt, b, a(1).i().max(0) as usize) {
            Some(part) => buffer_with(rt, part),
            None => V::NULL,
        },
        "readBytes" if desc.starts_with("(I)") => match take(rt, b, a(1).i().max(0) as usize) {
            Some(part) => buffer_with(rt, part),
            None => V::NULL,
        },
        "readBytes" if desc.starts_with("([B") => {
            let (at, length) = if desc.starts_with("([BII)") {
                (a(2).i().max(0), a(3).i().max(0) as usize)
            } else {
                (0, rt.array_len(a(1)).max(0) as usize)
            };
            if let Some(part) = take(rt, b, length) {
                for (i, byte) in part.iter().enumerate() {
                    rt.array_set(a(1), at + i as i32, V::I(i32::from(*byte as i8)));
                }
            }
            b
        }
        "readBytes" => {
            let left = bytes_mut(rt, b).map_or(0, |(v, at)| v.len() - *at);
            let length = if crate::jlang::kinds(desc).len() >= 2 {
                a(2).i().max(0) as usize
            } else {
                left
            };
            if let Some(part) = take(rt, b, length) {
                put(rt, a(1), &part);
            }
            b
        }
        "skipBytes" => {
            take(rt, b, a(1).i().max(0) as usize);
            b
        }
        // A buffer of the same bytes: what is left to read (`slice()`, `copy()`), a stretch
        // by index, or all of it read as far (`duplicate()`). Each is a buffer of its own
        // here: what is written to one afterwards the other does not see.
        "slice" | "retainedSlice" | "copy" | "duplicate" | "retainedDuplicate" => {
            let (all, at) = bytes_mut(rt, b).map_or((Vec::new(), 0), |(v, at)| (v.clone(), *at));
            if desc.starts_with("(II)") {
                let from = a(1).i().max(0) as usize;
                match all.get(from..from.saturating_add(a(2).i().max(0) as usize)) {
                    Some(part) => buffer_with(rt, part.to_vec()),
                    None => crate::jlang::throw_with(
                        rt,
                        "java/lang/IndexOutOfBoundsException",
                        "the stretch is outside the buffer",
                    ),
                }
            } else if name.ends_with("uplicate") {
                rt.alloc(BUF, Data::Bytes(all, at))
            } else {
                buffer_with(rt, all[at.min(all.len())..].to_vec())
            }
        }
        // The indexes a buffer remembers and goes back to.
        "markReaderIndex" | "markWriterIndex" => {
            let (length, at) = bytes_mut(rt, b).map_or((0, 0), |(v, at)| (v.len(), *at));
            let inner = backing(rt, b);
            if name == "markReaderIndex" {
                rt.put_field(inner, "$markedReader", V::I(at as i32));
            } else {
                rt.put_field(inner, "$markedWriter", V::I(length as i32));
            }
            b
        }
        "resetReaderIndex" | "resetWriterIndex" => {
            let inner = backing(rt, b);
            let marked = rt
                .get_field(
                    inner,
                    if name == "resetReaderIndex" {
                        "$markedReader"
                    } else {
                        "$markedWriter"
                    },
                )
                .i()
                .max(0) as usize;
            if let Some((v, at)) = bytes_mut(rt, b) {
                if name == "resetReaderIndex" {
                    *at = marked.min(v.len());
                } else {
                    v.truncate(marked);
                    *at = (*at).min(v.len());
                }
            }
            b
        }
        "discardReadBytes" | "discardSomeReadBytes" => {
            if let Some((v, at)) = bytes_mut(rt, b) {
                v.drain(..(*at).min(v.len()));
                *at = 0;
            }
            b
        }
        // What is left to read, or a stretch by index, as text.
        "toString" if desc.ends_with("Ljava/nio/charset/Charset;)Ljava/lang/String;") => {
            let (all, at) = bytes_mut(rt, b).map_or((Vec::new(), 0), |(v, at)| (v.clone(), *at));
            let (from, length) = if desc.starts_with("(II") {
                (a(1).i().max(0) as usize, a(2).i().max(0) as usize)
            } else {
                (at, all.len().saturating_sub(at))
            };
            match all.get(from..from.saturating_add(length)) {
                Some(part) => rt.string(&String::from_utf8_lossy(part)),
                None => crate::jlang::throw_with(
                    rt,
                    "java/lang/IndexOutOfBoundsException",
                    "the stretch is outside the buffer",
                ),
            }
        }
        // `writeById(idOf, value)` and `readById(byId)`: a value as the number a function
        // gives it.
        "writeById" => {
            let id = rt.call_functional(TO_INT, a(1), &[a(2)]).i();
            put_var(rt, b, u64::from(id as u32));
            b
        }
        "readById" => {
            let id = read_var_int(rt, b).unwrap_or(0);
            rt.call_functional(INT_FUNCTION, a(1), &[V::I(id)])
        }
        // A resource key as its location; read back under the registry's key given.
        "writeResourceKey" => {
            let key = text(rt, a(1));
            put_utf(
                rt,
                b,
                key.rsplit_once('#')
                    .map_or(key.as_str(), |(_, location)| location),
            );
            V::Void
        }
        "readResourceKey" => {
            let registry = text(rt, a(1));
            match read_utf(rt, b) {
                Some(location) => rt.string(&format!("{registry}#{location}")),
                None => V::NULL,
            }
        }
        // Arrays of longs and of ints, each after its length.
        "writeLongArray" => {
            let items = crate::jutil::elements(rt, a(1));
            put_var(rt, b, items.len() as u64);
            for v in items {
                put(rt, b, &v.j().to_be_bytes());
            }
            b
        }
        "readLongArray" => {
            let mut out = Vec::new();
            for _ in 0..read_count(rt, b).unwrap_or(0) {
                match take_n(rt, b) {
                    Some(bytes) => out.push(V::J(i64::from_be_bytes(bytes))),
                    None => break,
                }
            }
            rt.alloc("[J", Data::Array(out))
        }
        "writeVarIntArray" => {
            let items = crate::jutil::elements(rt, a(1));
            put_var(rt, b, items.len() as u64);
            for v in items {
                put_var(rt, b, u64::from(v.i() as u32));
            }
            b
        }
        "readVarIntArray" => {
            let mut out = Vec::new();
            for _ in 0..read_count(rt, b).unwrap_or(0) {
                match read_var_int(rt, b) {
                    Some(i) => out.push(V::I(i)),
                    None => break,
                }
            }
            rt.alloc("[I", Data::Array(out))
        }
        // A chunk's position as one long, a block's hit as the game writes it (the place in
        // the block as three floats), a position in a dimension.
        "writeChunkPos" => {
            let packed =
                crate::api::call(rt, "net/minecraft/world/level/ChunkPos.toLong.()J", &[a(1)]).j();
            put(rt, b, &packed.to_be_bytes());
            b
        }
        "readChunkPos" => {
            let packed = take_n(rt, b).map_or(0, i64::from_be_bytes);
            crate::api_common::chunk_pos(rt, packed as i32, (packed >> 32) as i32)
        }
        "writeGlobalPos" => {
            let dimension = crate::api::call(
                rt,
                "net/minecraft/core/GlobalPos.dimension.()Lnet/minecraft/resources/ResourceKey;",
                &[a(1)],
            );
            let pos = crate::api::call(
                rt,
                "net/minecraft/core/GlobalPos.pos.()Lnet/minecraft/core/BlockPos;",
                &[a(1)],
            );
            let key = text(rt, dimension);
            put_utf(
                rt,
                b,
                key.rsplit_once('#')
                    .map_or(key.as_str(), |(_, location)| location),
            );
            put_pos(rt, b, pos);
            V::Void
        }
        "readGlobalPos" => {
            let dimension = match read_utf(rt, b) {
                Some(location) => rt.string(&format!("dimension#{location}")),
                None => return Some(V::NULL),
            };
            let pos = read_pos(rt, b).unwrap_or(V::NULL);
            crate::api::call(
                rt,
                "net/minecraft/core/GlobalPos.of.(Lnet/minecraft/resources/ResourceKey;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/core/GlobalPos;",
                &[dimension, pos],
            )
        }
        "writeBlockHitResult" => {
            let c = crate::api::call;
            let pos = c(
                rt,
                "net/minecraft/world/phys/BlockHitResult.getBlockPos.()Lnet/minecraft/core/BlockPos;",
                &[a(1)],
            );
            let direction = c(
                rt,
                "net/minecraft/world/phys/BlockHitResult.getDirection.()Lnet/minecraft/core/Direction;",
                &[a(1)],
            );
            let at = c(
                rt,
                "net/minecraft/world/phys/HitResult.getLocation.()Lnet/minecraft/world/phys/Vec3;",
                &[a(1)],
            );
            let inside = c(
                rt,
                "net/minecraft/world/phys/BlockHitResult.isInside.()Z",
                &[a(1)],
            )
            .i();
            let corner = crate::api::pos_of(rt, pos);
            put_pos(rt, b, pos);
            let o = ordinal(rt, direction);
            put_var(rt, b, o as u64);
            for (axis, field) in ["x", "y", "z"].into_iter().enumerate() {
                let within = (rt.get_field(at, field).d() - f64::from(corner[axis])) as f32;
                put(rt, b, &within.to_be_bytes());
            }
            put(rt, b, &[u8::from(inside != 0)]);
            V::Void
        }
        "readBlockHitResult" => {
            let pos = read_pos(rt, b).unwrap_or(V::NULL);
            let direction = read_var_int(rt, b).unwrap_or(0);
            let corner = crate::api::pos_of(rt, pos);
            let mut at = [0.0f64; 3];
            for (axis, place) in at.iter_mut().enumerate() {
                *place = f64::from(corner[axis])
                    + f64::from(take_n(rt, b).map_or(0.0, f32::from_be_bytes));
            }
            let inside = take(rt, b, 1).is_some_and(|v| v[0] != 0);
            let (at, direction) = (
                vec3(rt, at[0], at[1], at[2]),
                crate::api_common::direction(rt, i64::from(direction.clamp(0, 5))),
            );
            let hit = rt.new_object("net/minecraft/world/phys/BlockHitResult");
            crate::api::call(
                rt,
                "net/minecraft/world/phys/BlockHitResult.<init>.(Lnet/minecraft/world/phys/Vec3;Lnet/minecraft/core/Direction;Lnet/minecraft/core/BlockPos;Z)V",
                &[hit, at, direction, pos, V::I(i32::from(inside))],
            );
            hit
        }
        // A value through a codec, as a tag (`writeWithCodec(ops, codec, value)`) or as the
        // text of its JSON (`writeJsonWithCodec(codec, value)`).
        "writeWithCodec" | "writeJsonWithCodec" => {
            let (codec, value) = if name == "writeWithCodec" {
                (a(2), a(3))
            } else {
                (a(1), a(2))
            };
            match crate::codecs::encode(rt, codec, value) {
                Ok(n) if name == "writeWithCodec" => put_nbt(rt, b, Some(&n)),
                Ok(n) => put_utf(rt, b, &crate::recipes::nbt_json(&n).to_string()),
                Err(e) => throw_with(
                    rt,
                    "io/netty/handler/codec/EncoderException",
                    &format!("Failed to encode: {e}"),
                ),
            }
            if name == "writeWithCodec" { b } else { V::Void }
        }
        "readWithCodec" | "readWithCodecTrusted" | "readJsonWithCodec" => {
            let json = name == "readJsonWithCodec";
            let codec = if json { a(1) } else { a(2) };
            let read = if json {
                read_utf(rt, b).and_then(|t| serde_json::from_str(&t).ok())
            } else {
                read_nbt(rt, b)
                    .flatten()
                    .map(|n| crate::recipes::nbt_json(&n))
            };
            let decoded = match read {
                Some(value) => crate::codecs::decode(rt, codec, &value),
                None => Err("nothing to read".to_owned()),
            };
            match decoded {
                Ok(v) => v,
                Err(e) => {
                    throw_with(
                        rt,
                        "io/netty/handler/codec/DecoderException",
                        &format!("Failed to decode: {e}"),
                    );
                    V::NULL
                }
            }
        }
        "array" => {
            let all = bytes(rt, b);
            new_byte_array(rt, &all)
        }
        "registryAccess" => rt.intern("compat:registry-access".into(), |rt| rt.new_object("net/minecraft/core/RegistryAccess")),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use crate::host::MemoryHost;

    fn sv(rt: &mut Rt, k: &'static str) -> V {
        crate::api::get_static(rt, k)
    }

    fn round_trip(rt: &mut Rt, codec: V, value: V) -> (Vec<u8>, V) {
        let buf = new_buffer(rt);
        encode(rt, codec, buf, value);
        let bytes = bytes(rt, buf);
        let back = buffer_with(rt, bytes.clone());
        let v = decode(rt, codec, back);
        assert!(rt.thrown.is_none());
        (bytes, v)
    }

    /// A buffer hands its bytes to another and to arrays, goes back to the indexes it marked,
    /// and writes arrays, keys and positions as the game does.
    #[test]
    fn a_buffer_hands_its_bytes_on_and_goes_back_to_its_marks() {
        let mut rt = Rt::new();
        let c = |rt: &mut Rt, key: &'static str, args: &[V]| crate::api::call(rt, key, args);
        let a = new_buffer(&mut rt);
        for i in 1..=6 {
            c(
                &mut rt,
                "net/minecraft/network/FriendlyByteBuf.writeByte.(I)Lnet/minecraft/network/FriendlyByteBuf;",
                &[a, V::I(i)],
            );
        }
        // A stretch of another buffer by index leaves that buffer as it was; what it has left
        // to read, it has then read.
        let b = new_buffer(&mut rt);
        c(
            &mut rt,
            "net/minecraft/network/FriendlyByteBuf.writeBytes.(Lio/netty/buffer/ByteBuf;II)Lnet/minecraft/network/FriendlyByteBuf;",
            &[b, a, V::I(1), V::I(3)],
        );
        assert_eq!(bytes(&rt, b), [2, 3, 4]);
        assert_eq!(
            c(&mut rt, "io/netty/buffer/ByteBuf.readerIndex.()I", &[a]),
            V::I(0)
        );
        assert_eq!(
            c(&mut rt, "io/netty/buffer/ByteBuf.readByte.()B", &[a]),
            V::I(1)
        );
        c(
            &mut rt,
            "net/minecraft/network/FriendlyByteBuf.writeBytes.(Lio/netty/buffer/ByteBuf;)Lnet/minecraft/network/FriendlyByteBuf;",
            &[b, a],
        );
        assert_eq!(bytes(&rt, b), [2, 3, 4, 2, 3, 4, 5, 6]);
        assert_eq!(
            c(&mut rt, "io/netty/buffer/ByteBuf.readableBytes.()I", &[a]),
            V::I(0)
        );
        // The next bytes as a buffer of their own; a copy read without the buffer moving.
        let slice = c(
            &mut rt,
            "io/netty/buffer/ByteBuf.readRetainedSlice.(I)Lio/netty/buffer/ByteBuf;",
            &[b, V::I(2)],
        );
        assert_eq!(bytes(&rt, slice), [2, 3]);
        let copy = c(
            &mut rt,
            "io/netty/buffer/ByteBuf.copy.()Lio/netty/buffer/ByteBuf;",
            &[b],
        );
        assert_eq!(bytes(&rt, copy), [4, 2, 3, 4, 5, 6]);
        assert_eq!(
            c(&mut rt, "io/netty/buffer/ByteBuf.readByte.()B", &[copy]),
            V::I(4)
        );
        assert_eq!(
            c(&mut rt, "io/netty/buffer/ByteBuf.readerIndex.()I", &[b]),
            V::I(2)
        );
        let twin = c(
            &mut rt,
            "io/netty/buffer/ByteBuf.duplicate.()Lio/netty/buffer/ByteBuf;",
            &[b],
        );
        assert_eq!(
            c(&mut rt, "io/netty/buffer/ByteBuf.readerIndex.()I", &[twin]),
            V::I(2)
        );
        let part = c(
            &mut rt,
            "io/netty/buffer/ByteBuf.slice.(II)Lio/netty/buffer/ByteBuf;",
            &[b, V::I(6), V::I(2)],
        );
        assert_eq!(bytes(&rt, part), [5, 6]);
        // A mark, bytes skipped, and back; past the end is an error.
        c(
            &mut rt,
            "io/netty/buffer/ByteBuf.markReaderIndex.()Lio/netty/buffer/ByteBuf;",
            &[b],
        );
        c(
            &mut rt,
            "net/minecraft/network/FriendlyByteBuf.skipBytes.(I)Lnet/minecraft/network/FriendlyByteBuf;",
            &[b, V::I(3)],
        );
        assert_eq!(
            c(&mut rt, "io/netty/buffer/ByteBuf.readerIndex.()I", &[b]),
            V::I(5)
        );
        c(
            &mut rt,
            "net/minecraft/network/FriendlyByteBuf.resetReaderIndex.()Lnet/minecraft/network/FriendlyByteBuf;",
            &[b],
        );
        assert_eq!(
            c(&mut rt, "io/netty/buffer/ByteBuf.readerIndex.()I", &[b]),
            V::I(2)
        );
        c(
            &mut rt,
            "net/minecraft/network/FriendlyByteBuf.skipBytes.(I)Lnet/minecraft/network/FriendlyByteBuf;",
            &[b, V::I(99)],
        );
        let thrown = rt.thrown.take().expect("past the end");
        assert_eq!(rt.class_of(thrown), "java/lang/IndexOutOfBoundsException");
        // Into an array from an index; several buffers as one.
        let array = rt.alloc("[B", Data::Array(vec![V::I(0); 4]));
        c(
            &mut rt,
            "net/minecraft/network/FriendlyByteBuf.readBytes.([BII)Lnet/minecraft/network/FriendlyByteBuf;",
            &[b, array, V::I(1), V::I(2)],
        );
        assert_eq!(byte_array(&rt, array), [0, 4, 2, 0]);
        let both = rt.alloc("[Lio/netty/buffer/ByteBuf;", Data::Array(vec![slice, part]));
        let joined = c(
            &mut rt,
            "io/netty/buffer/Unpooled.wrappedBuffer.([Lio/netty/buffer/ByteBuf;)Lio/netty/buffer/ByteBuf;",
            &[both],
        );
        assert_eq!(bytes(&rt, joined), [2, 3, 5, 6]);
        // Arrays after their lengths, a value by its number, a key by its location.
        let d = new_buffer(&mut rt);
        let longs = rt.alloc("[J", Data::Array(vec![V::J(-1), V::J(1 << 40)]));
        c(
            &mut rt,
            "net/minecraft/network/FriendlyByteBuf.writeLongArray.([J)Lnet/minecraft/network/FriendlyByteBuf;",
            &[d, longs],
        );
        let ints = rt.alloc("[I", Data::Array(vec![V::I(300), V::I(-1)]));
        c(
            &mut rt,
            "net/minecraft/network/FriendlyByteBuf.writeVarIntArray.([I)Lnet/minecraft/network/FriendlyByteBuf;",
            &[d, ints],
        );
        let key = rt.string("dimension#minecraft:the_nether");
        c(
            &mut rt,
            "net/minecraft/network/FriendlyByteBuf.writeResourceKey.(Lnet/minecraft/resources/ResourceKey;)V",
            &[d, key],
        );
        let chunk = crate::api_common::chunk_pos(&mut rt, -3, 7);
        c(
            &mut rt,
            "net/minecraft/network/FriendlyByteBuf.writeChunkPos.(Lnet/minecraft/world/level/ChunkPos;)Lnet/minecraft/network/FriendlyByteBuf;",
            &[d, chunk],
        );
        assert_eq!(
            bytes(&rt, d).len(),
            1 + 16 + 1 + 2 + 5 + 1 + "minecraft:the_nether".len() + 8
        );
        let back = c(
            &mut rt,
            "net/minecraft/network/FriendlyByteBuf.readLongArray.()[J",
            &[d],
        );
        assert_eq!(crate::jutil::elements(&rt, back), [V::J(-1), V::J(1 << 40)]);
        let back = c(
            &mut rt,
            "net/minecraft/network/FriendlyByteBuf.readVarIntArray.()[I",
            &[d],
        );
        assert_eq!(crate::jutil::elements(&rt, back), [V::I(300), V::I(-1)]);
        let registry = rt.string("dimension");
        let back = c(
            &mut rt,
            "net/minecraft/network/FriendlyByteBuf.readResourceKey.(Lnet/minecraft/resources/ResourceKey;)Lnet/minecraft/resources/ResourceKey;",
            &[d, registry],
        );
        assert_eq!(rt.str_of(back), Some("dimension#minecraft:the_nether"));
        let back = c(
            &mut rt,
            "net/minecraft/network/FriendlyByteBuf.readChunkPos.()Lnet/minecraft/world/level/ChunkPos;",
            &[d],
        );
        assert_eq!(
            c(
                &mut rt,
                "net/minecraft/world/level/ChunkPos.toLong.()J",
                &[back]
            ),
            c(
                &mut rt,
                "net/minecraft/world/level/ChunkPos.toLong.()J",
                &[chunk]
            )
        );
        assert!(
            rt.thrown.is_none() && rt.unmapped.is_empty(),
            "{:?}",
            rt.unmapped
        );
    }

    /// Create's packets write lengths with `VarInt.write(buf, n)` itself, not through a codec.
    #[test]
    fn var_int_helpers_write_and_read_as_friendly_byte_buf_does() {
        let mut rt = Rt::new();
        let b = new_buffer(&mut rt);
        call(&mut rt, "net/minecraft/network/VarInt.write.(Lio/netty/buffer/ByteBuf;I)Lio/netty/buffer/ByteBuf;", &[b, V::I(300)]);
        call(&mut rt, "net/minecraft/network/VarLong.write.(Lio/netty/buffer/ByteBuf;J)Lio/netty/buffer/ByteBuf;", &[b, V::J(-1)]);
        assert_eq!(call(&mut rt, "net/minecraft/network/VarInt.read.(Lio/netty/buffer/ByteBuf;)I", &[b]), Some(V::I(300)));
        assert_eq!(call(&mut rt, "net/minecraft/network/VarLong.read.(Lio/netty/buffer/ByteBuf;)J", &[b]), Some(V::J(-1)));
        assert_eq!(call(&mut rt, "net/minecraft/network/VarInt.getByteSize.(I)I", &[V::I(-1)]), Some(V::I(5)));
        assert_eq!(call(&mut rt, "net/minecraft/network/VarLong.getByteSize.(J)I", &[V::J(-1)]), Some(V::I(10)));
    }

    #[test]
    fn numbers_write_as_minecraft_writes_them() {
        let mut rt = Rt::new();
        let var_int = sv(&mut rt, "net/minecraft/network/codec/ByteBufCodecs.VAR_INT");
        // VarInt: 7 bits a byte, low first, the top bit saying more follow; a negative takes 5.
        assert_eq!(round_trip(&mut rt, var_int, V::I(300)), (vec![0xAC, 0x02], V::I(300)));
        assert_eq!(round_trip(&mut rt, var_int, V::I(-1)).0, vec![0xFF, 0xFF, 0xFF, 0xFF, 0x0F]);
        let int = sv(&mut rt, "net/minecraft/network/codec/ByteBufCodecs.INT");
        // A boxed `Integer` (a lambda's `Integer.valueOf`) writes as its value.
        let boxed = crate::api_common::integer(&mut rt, 258);
        assert_eq!(round_trip(&mut rt, int, boxed), (vec![0, 0, 1, 2], V::I(258)));
        let float = sv(&mut rt, "net/minecraft/network/codec/ByteBufCodecs.FLOAT");
        assert_eq!(round_trip(&mut rt, float, V::F(1.5)), (vec![0x3F, 0xC0, 0, 0], V::F(1.5)));
        let b = sv(&mut rt, "net/minecraft/network/codec/ByteBufCodecs.BOOL");
        let yes = crate::api_common::boolean(&mut rt, true);
        assert_eq!(round_trip(&mut rt, b, yes), (vec![1], V::I(1)));
        let s = sv(&mut rt, "net/minecraft/network/codec/ByteBufCodecs.STRING_UTF8");
        let text = rt.string("hé");
        let (bytes, back) = round_trip(&mut rt, s, text);
        assert_eq!(bytes, vec![3, b'h', 0xC3, 0xA9]);
        assert_eq!(rt.str_of(back), Some("hé"));
    }

    /// A component type in a packet (`DataComponentType.STREAM_CODEC`): vanilla's and a mod's
    /// own come back as the type they were, by their registry's names.
    #[test]
    fn a_component_type_travels_by_its_registrys_name() {
        let mut rt = Rt::new();
        crate::install();
        let codec = sv(
            &mut rt,
            "net/minecraft/core/component/DataComponentType.STREAM_CODEC",
        );
        let name = sv(
            &mut rt,
            "net/minecraft/core/component/DataComponents.CUSTOM_NAME",
        );
        let own = rt.new_object("net/minecraft/core/component/DataComponentType");
        rt.registered
            .insert(("data_component_type".into(), "t:charge".into()), own);
        for ty in [name, own] {
            assert_eq!(round_trip(&mut rt, codec, ty).1, ty);
        }
        assert!(
            rt.unmapped.is_empty() && rt.thrown.is_none(),
            "{:?}",
            rt.unmapped
        );
    }

    /// A stream codec of the game's not modelled here (`DyeColor.STREAM_CODEC`) sends what its
    /// class's codec writes and reads it back; JOML's vectors go as their floats.
    #[test]
    fn a_classs_stream_codec_falls_back_to_its_codec() {
        let mut rt = Rt::new();
        crate::install();
        fn vcall(rt: &mut Rt, key: &'static str, _short: &'static str, args: &[V]) -> V {
            crate::api::call(rt, key, args)
        }
        rt.vcall = Some(vcall);
        let codec = sv(&mut rt, "net/minecraft/world/item/DyeColor.STREAM_CODEC");
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        let red = sv(&mut rt, "net/minecraft/world/item/DyeColor.RED");
        assert_eq!(round_trip(&mut rt, codec, red).1, red);
        // Without a codec of its class either, it is a miss of its own name.
        sv(&mut rt, "net/minecraft/world/item/JukeboxSong.STREAM_CODEC");
        assert_eq!(rt.unmapped.keys().copied().collect::<Vec<_>>(), ["net/minecraft/world/item/JukeboxSong.STREAM_CODEC"]);

        let v3 = sv(&mut rt, "net/minecraft/network/codec/ByteBufCodecs.VECTOR3F");
        let v = rt.new_object("org/joml/Vector3f");
        for (f, x) in [("x", 1.0f32), ("y", -2.5), ("z", 0.25)] {
            rt.put_field(v, f, V::F(x));
        }
        let (bytes, back) = round_trip(&mut rt, v3, v);
        assert_eq!(bytes[..4], 1.0f32.to_be_bytes());
        assert_eq!(bytes.len(), 12);
        assert_eq!([rt.get_field(back, "x"), rt.get_field(back, "y"), rt.get_field(back, "z")], [V::F(1.0), V::F(-2.5), V::F(0.25)]);
    }

    /// `ResourceKey.streamCodec(registry)` as a mod's class file calls it, through the dispatcher:
    /// a key goes by its location and comes back a key of the same registry.
    #[test]
    fn a_resource_key_travels_by_its_location() {
        let mut rt = Rt::new();
        crate::install();
        let dimension = rt.string("dimension");
        let codec = crate::api::call(&mut rt, "net/minecraft/resources/ResourceKey.streamCodec.(Lnet/minecraft/resources/ResourceKey;)Lnet/minecraft/network/codec/StreamCodec;", &[dimension]);
        assert!(is_codec(&rt, codec), "the network module's codec, not a stand-in");
        let id = rt.string("minecraft:the_nether");
        let nether = crate::api::call(&mut rt, "net/minecraft/resources/ResourceKey.create.(Lnet/minecraft/resources/ResourceKey;Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/resources/ResourceKey;", &[dimension, id]);
        let (bytes, back) = round_trip(&mut rt, codec, nether);
        assert_eq!(bytes, [&[20u8][..], b"minecraft:the_nether"].concat(), "a UTF-8 string after its length");
        assert_eq!(rt.str_of(back), rt.str_of(nether));
    }

    /// A `GlobalPos` (Lodestone-like trackers, Max Henkel's mods): the dimension's location, then the
    /// position as a long (vanilla's `composite` of `ResourceKey.streamCodec` and `BlockPos`'s);
    /// `GlobalPos.of` builds it as its constructor does.
    #[test]
    fn a_global_position_travels_as_vanillas_does() {
        let mut rt = Rt::new();
        crate::install();
        // What generated code does for an object no mod class is: the API call by the method's key.
        fn vcall(rt: &mut Rt, key: &'static str, _short: &'static str, args: &[V]) -> V {
            crate::api::call(rt, key, args)
        }
        rt.vcall = Some(vcall);
        let codec = sv(&mut rt, "net/minecraft/core/GlobalPos.STREAM_CODEC");
        let nether = crate::api::get_static(&mut rt, "net/minecraft/world/level/Level.NETHER");
        let pos = crate::api::block_pos(&mut rt, 1, 2, 3);
        let at = crate::api::call(&mut rt, "net/minecraft/core/GlobalPos.of.(Lnet/minecraft/resources/ResourceKey;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/core/GlobalPos;", &[nether, pos]);
        let (bytes, back) = round_trip(&mut rt, codec, at);
        // BlockPos.asLong(1, 2, 3): x << 38 | z << 12 | y, as 8 bytes.
        let long: i64 = (1i64 << 38) | (3i64 << 12) | 2;
        assert_eq!(bytes, [&[20u8][..], b"minecraft:the_nether", &long.to_be_bytes()].concat());
        let dimension = crate::api::call(&mut rt, "net/minecraft/core/GlobalPos.dimension.()Lnet/minecraft/resources/ResourceKey;", &[back]);
        assert_eq!(rt.str_of(dimension), Some("dimension#minecraft:the_nether"));
        let p = crate::api::call(&mut rt, "net/minecraft/core/GlobalPos.pos.()Lnet/minecraft/core/BlockPos;", &[back]);
        assert_eq!(crate::api::pos_of(&rt, p), [1, 2, 3]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// Particles in a packet (`ParticleTypes.STREAM_CODEC`): a plain type, a block's and an item's
    /// options and a dust's colour come back as they went.
    #[test]
    fn particle_options_come_back_as_they_went() {
        let mut rt = Rt::new();
        crate::install();
        let codec = sv(&mut rt, "net/minecraft/core/particles/ParticleTypes.STREAM_CODEC");
        let flame = sv(&mut rt, "net/minecraft/core/particles/ParticleTypes.FLAME");
        let (_, back) = round_trip(&mut rt, codec, flame);
        assert_eq!(back, flame, "vanilla's type, the one object");

        let block_type = sv(&mut rt, "net/minecraft/core/particles/ParticleTypes.BLOCK");
        let stone = crate::api::state(&mut rt, "minecraft:stone");
        let block = rt.new_object("net/minecraft/core/particles/BlockParticleOption");
        crate::api::call(&mut rt, "net/minecraft/core/particles/BlockParticleOption.<init>.(Lnet/minecraft/core/particles/ParticleType;Lnet/minecraft/world/level/block/state/BlockState;)V", &[block, block_type, stone]);
        let (_, back) = round_trip(&mut rt, codec, block);
        let ty = crate::api::call(&mut rt, "net/minecraft/core/particles/ParticleOptions.getType.()Lnet/minecraft/core/particles/ParticleType;", &[back]);
        assert_eq!(ty, block_type);
        let state = crate::api::call(&mut rt, "net/minecraft/core/particles/BlockParticleOption.getState.()Lnet/minecraft/world/level/block/state/BlockState;", &[back]);
        assert_eq!(rt.str_of(state), Some("minecraft:stone"));

        let item_type = sv(&mut rt, "net/minecraft/core/particles/ParticleTypes.ITEM");
        let sheets = crate::api_common::stack(&mut rt, "create:iron_sheet", 3);
        let item = rt.new_object("net/minecraft/core/particles/ItemParticleOption");
        crate::api::call(&mut rt, "net/minecraft/core/particles/ItemParticleOption.<init>.(Lnet/minecraft/core/particles/ParticleType;Lnet/minecraft/world/item/ItemStack;)V", &[item, item_type, sheets]);
        let (_, back) = round_trip(&mut rt, codec, item);
        let stack = crate::api::call(&mut rt, "net/minecraft/core/particles/ItemParticleOption.getItem.()Lnet/minecraft/world/item/ItemStack;", &[back]);
        assert_eq!(crate::api_common::item_like_name(&rt, rt.get_field(stack, "$item")), "create:iron_sheet");
        assert_eq!(rt.get_field(stack, "$count"), V::I(3));

        let redstone = sv(&mut rt, "net/minecraft/core/particles/DustParticleOptions.REDSTONE");
        let (_, back) = round_trip(&mut rt, codec, redstone);
        assert_eq!(["$r", "$g", "$b", "$scale"].map(|f| rt.get_field(back, f).f()), [1.0, 0.0, 0.0, 1.0]);
        let dust = sv(&mut rt, "net/minecraft/core/particles/ParticleTypes.DUST");
        assert_eq!(rt.get_field(back, "$type"), dust);
    }

    /// NeoForge's own codecs (`NeoForgeStreamCodecs`): an enum by its ordinal, a lazy codec its
    /// supplier gives once, a registry's key by its location, a unit that writes nothing.
    #[test]
    fn neoforges_codecs_write_what_neoforge_writes() {
        let mut rt = Rt::new();
        crate::install();
        const NEO: &str = "net/neoforged/neoforge/network/codec/NeoForgeStreamCodecs";
        let axis = crate::jlang::class_object(&mut rt, "net/minecraft/core/Direction$Axis");
        let enums = crate::api::call(&mut rt, "net/neoforged/neoforge/network/codec/NeoForgeStreamCodecs.enumCodec.(Ljava/lang/Class;)Lnet/minecraft/network/codec/StreamCodec;", &[axis]);
        assert!(is_codec(&rt, enums), "{NEO}'s, not a stand-in");
        let z = sv(&mut rt, "net/minecraft/core/Direction$Axis.Z");
        let (bytes, back) = round_trip(&mut rt, enums, z);
        assert_eq!((bytes, back), (vec![2], z));
        let keys = crate::api::call(&mut rt, "net/neoforged/neoforge/network/codec/NeoForgeStreamCodecs.registryKey.()Lnet/minecraft/network/codec/StreamCodec;", &[]);
        let blocks = sv(&mut rt, "net/minecraft/core/registries/Registries.BLOCK");
        let (bytes, back) = round_trip(&mut rt, keys, blocks);
        assert_eq!(bytes, [&[15u8][..], b"minecraft:block"].concat());
        assert_eq!(rt.str_of(back), Some("block"));
        let one = rt.string("only");
        let unit = crate::api::call(&mut rt, "net/neoforged/neoforge/network/codec/NeoForgeStreamCodecs.uncheckedUnit.(Ljava/lang/Object;)Lnet/minecraft/network/codec/StreamCodec;", &[one]);
        let other = rt.string("other");
        let (bytes, back) = round_trip(&mut rt, unit, other);
        assert_eq!((bytes, back), (vec![], one));
    }

    #[test]
    fn a_block_pos_packs_into_one_long() {
        let mut rt = Rt::new();
        let codec = sv(&mut rt, "net/minecraft/core/BlockPos.STREAM_CODEC");
        let pos = crate::api::block_pos(&mut rt, -36, -60, 1_000_001);
        let (bytes, back) = round_trip(&mut rt, codec, pos);
        // `BlockPos.asLong` of (-36, -60, 1000001), as Java computes it.
        let x = (-36i64 & 0x3FF_FFFF) << 38;
        let z = (1_000_001i64 & 0x3FF_FFFF) << 12;
        let y = -60i64 & 0xFFF;
        assert_eq!(bytes, (x | z | y).to_be_bytes().to_vec());
        assert_eq!(native(&rt, back)[..3], [-36, -60, 1_000_001]);
    }

    /// A component in a packet (`ComponentSerialization.STREAM_CODEC`, a Create train's name):
    /// its NBT as vanilla's `NbtOps` writes it (plain text a string tag; a translation a compound
    /// whose `with` of mixed tags holds each under `""`), and back whole, so the client reads
    /// "Unnamed Train" in its own language, not the server's key.
    #[test]
    fn a_component_travels_whole_and_stays_translatable() {
        let mut rt = Rt::new();
        crate::install();
        let codec = sv(
            &mut rt,
            "net/minecraft/network/chat/ComponentSerialization.STREAM_CODEC",
        );
        let plain = rt.string("Train");
        let plain = crate::api_common::component(&mut rt, plain, false, V::NULL);
        let (bytes, back) = round_trip(&mut rt, codec, plain);
        // `TAG_String` (8), then the UTF: two bytes of length and the text.
        assert_eq!(bytes, [&[8u8, 0, 5][..], b"Train"].concat());
        assert_eq!(
            crate::api_common::component_json(&rt, back),
            serde_json::json!({ "text": "Train" })
        );
        // A translation of a string and a styled component, a bold sibling after it.
        let id = rt.string("fbfc9");
        let station = rt.string("Station");
        let station = crate::api_common::component(&mut rt, station, false, V::NULL);
        let gold = rt.string("GOLD,");
        rt.put_field(station, "$style", gold);
        let with = rt.alloc("[Ljava/lang/Object;", Data::Array(vec![id, station]));
        let key = rt.string("create.train.arrived_at");
        let name = crate::api_common::component(&mut rt, key, true, with);
        let tail = rt.string("!");
        let tail = crate::api_common::component(&mut rt, tail, false, V::NULL);
        let bold = rt.string("BOLD,");
        rt.put_field(tail, "$style", bold);
        let extra = crate::jutil::new_list(&mut rt, vec![tail]);
        rt.put_field(name, "$extra", extra);
        let (bytes, back) = round_trip(&mut rt, codec, name);
        let sent = crate::nbt::Reader::new(&bytes)
            .any()
            .flatten()
            .expect("a tag");
        let expected = serde_json::json!({
            "translate": "create.train.arrived_at",
            "with": [{ "": "fbfc9" }, { "": { "text": "Station", "color": "gold" } }],
            "extra": [{ "text": "!", "bold": 1 }],
        });
        assert_eq!(crate::recipes::nbt_json(&sent), expected);
        let whole = serde_json::json!({
            "translate": "create.train.arrived_at",
            "with": ["fbfc9", { "text": "Station", "color": "gold" }],
            "extra": [{ "text": "!", "bold": true }],
        });
        assert_eq!(crate::api_common::component_json(&rt, back), whole);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn a_compound_tag_survives_the_trip() {
        let mut rt = Rt::new();
        let codec = sv(&mut rt, "net/minecraft/network/codec/ByteBufCodecs.COMPOUND_TAG");
        let mut inner = BTreeMap::new();
        inner.insert("Items".to_owned(), Nbt::List(vec![Nbt::Int(1), Nbt::Int(2)]));
        let mut m = BTreeMap::new();
        m.insert("Speed".to_owned(), Nbt::Float(32.0));
        m.insert("Id".to_owned(), Nbt::Long(-9_895_604_654_016));
        m.insert("Name".to_owned(), Nbt::String("create:belt".into()));
        m.insert("Inv".to_owned(), Nbt::Compound(inner));
        let tag = crate::nbt::to_runtime(&mut rt, &m);
        let (_, back) = round_trip(&mut rt, codec, tag);
        assert_eq!(crate::nbt::from_runtime(&rt, back), m);
    }

    /// `StreamCodec.composite(BlockPos.STREAM_CODEC, Packet::pos, ByteBufCodecs.FLOAT, Packet::speed,
    /// Packet::new)` over a packet the test's lambdas stand for: lambda 1 reads `pos`, 2 reads
    /// `speed` (raw, as a method reference gives it), 3 builds a packet from both.
    fn lambdas(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
        Some(match id {
            1 => rt.get_field(args[1], "pos"),
            2 => rt.get_field(args[1], "speed"),
            3 => {
                let p = rt.new_object("test/Packet");
                rt.put_field(p, "pos", args[1]);
                rt.put_field(p, "speed", args[2]);
                p
            }
            // `HashSet::new` as an `IntFunction`.
            4 => crate::jutil::new_set_of(rt, &[]),
            // `StreamCodec.recursive`'s function: an optional of the codec itself.
            5 => call(
                rt,
                "net/minecraft/network/codec/ByteBufCodecs.optional.(Lnet/minecraft/network/codec/StreamCodec;)Lnet/minecraft/network/codec/StreamCodec;",
                &[args[1]],
            )?,
            _ => return None,
        })
    }

    /// `codec.apply(ByteBufCodecs.collection(HashSet::new))` (Mekanism's attachments,
    /// SecurityCraft's components): a count, then each element; read back into what the factory
    /// makes.
    #[test]
    fn a_collection_operation_reads_into_the_factorys_collection() {
        let mut rt = Rt::new();
        rt.lambda_call = Some(lambdas);
        let int = sv(&mut rt, "net/minecraft/network/codec/ByteBufCodecs.VAR_INT");
        let factory = rt.lambda_id("test/Sets", 4, Vec::new());
        let op = call(&mut rt, "net/minecraft/network/codec/ByteBufCodecs.collection.(Ljava/util/function/IntFunction;)Lnet/minecraft/network/codec/StreamCodec$CodecOperation;", &[factory]).unwrap();
        let codec = call(&mut rt, "net/minecraft/network/codec/StreamCodec.apply.(Lnet/minecraft/network/codec/StreamCodec$CodecOperation;)Lnet/minecraft/network/codec/StreamCodec;", &[int, op]).unwrap();
        let set = crate::jutil::new_set_of(&mut rt, &[V::I(5), V::I(300)]);
        let (bytes, back) = round_trip(&mut rt, codec, set);
        assert_eq!(bytes, [2, 5, 0xac, 0x02]);
        assert_eq!(rt.class_of(back), rt.class_of(set));
        assert_eq!(crate::jutil::elements(&rt, back), [V::I(5), V::I(300)]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `ByteBufCodecs.either(left, right)` writes which side, then that side's value;
    /// `StreamCodec.recursive(wrapped)` is the codec its function makes of it, here an optional of
    /// itself: one byte for each level, the last empty.
    #[test]
    fn either_and_recursive_codecs_read_back_what_they_wrote() {
        let mut rt = Rt::new();
        rt.lambda_call = Some(lambdas);
        let int = sv(&mut rt, "net/minecraft/network/codec/ByteBufCodecs.VAR_INT");
        let string = sv(
            &mut rt,
            "net/minecraft/network/codec/ByteBufCodecs.STRING_UTF8",
        );
        let either = call(&mut rt, "net/minecraft/network/codec/ByteBufCodecs.either.(Lnet/minecraft/network/codec/StreamCodec;Lnet/minecraft/network/codec/StreamCodec;)Lnet/minecraft/network/codec/StreamCodec;", &[int, string]).unwrap();
        let name = rt.string("ok");
        let right = crate::codecs::either(&mut rt, false, name);
        let (bytes, back) = round_trip(&mut rt, either, right);
        assert_eq!(bytes, [0, 2, b'o', b'k']);
        assert_eq!(rt.get_field(back, "$isLeft"), V::I(0));
        assert_eq!(rt.str_of(rt.get_field(back, "$right")), Some("ok"));
        let left = crate::codecs::either(&mut rt, true, V::I(300));
        let (bytes, _) = round_trip(&mut rt, either, left);
        assert_eq!(bytes, [1, 0xac, 0x02]);

        let wrapped = rt.lambda_id("java/util/function/UnaryOperator", 5, Vec::new());
        let nested = call(&mut rt, "net/minecraft/network/codec/StreamCodec.recursive.(Ljava/util/function/UnaryOperator;)Lnet/minecraft/network/codec/StreamCodec;", &[wrapped]).unwrap();
        let none = crate::api_common::optional(&mut rt, V::NULL);
        let one = crate::api_common::optional(&mut rt, none);
        let two = crate::api_common::optional(&mut rt, one);
        let (bytes, _) = round_trip(&mut rt, nested, two);
        assert_eq!(bytes, [1, 1, 0]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A number a packet read (`ByteBufCodecs.VAR_INT`) is the `Integer` a mod boxes of it: Create's
    /// controls look for the keys a player holds with `heldControls.contains(0)` in the list
    /// `ControlsInputPacket` read, and its server kept the train still while W was held.
    #[test]
    fn a_number_a_packet_read_is_found_by_a_mods_box_of_it() {
        const VALUE_OF: &str = "java/lang/Integer.valueOf.(I)Ljava/lang/Integer;";
        let mut rt = Rt::new();
        rt.lambda_call = Some(lambdas);
        let int = sv(&mut rt, "net/minecraft/network/codec/ByteBufCodecs.VAR_INT");
        let list_op = call(&mut rt, "net/minecraft/network/codec/ByteBufCodecs.list.()Lnet/minecraft/network/codec/StreamCodec$CodecOperation;", &[]).unwrap();
        let codec = call(&mut rt, "net/minecraft/network/codec/StreamCodec.apply.(Lnet/minecraft/network/codec/StreamCodec$CodecOperation;)Lnet/minecraft/network/codec/StreamCodec;", &[int, list_op]).unwrap();
        let w = crate::api::call(&mut rt, VALUE_OF, &[V::I(0)]);
        let held = crate::jutil::new_list(&mut rt, vec![w]);
        let (_, read) = round_trip(&mut rt, codec, held);
        for (key, held) in [(0, 1), (1, 0)] {
            let boxed = crate::api::call(&mut rt, VALUE_OF, &[V::I(key)]);
            let found = crate::api::call(
                &mut rt,
                "java/util/List.contains.(Ljava/lang/Object;)Z",
                &[read, boxed],
            );
            assert_eq!(found, V::I(held), "key {key}");
            let first = crate::api::call(
                &mut rt,
                "java/util/List.get.(I)Ljava/lang/Object;",
                &[read, V::I(0)],
            );
            let same = crate::api::call(
                &mut rt,
                "java/lang/Integer.equals.(Ljava/lang/Object;)Z",
                &[boxed, first],
            );
            assert_eq!(
                same,
                V::I(held),
                "Integer.valueOf({key}).equals(the key read)"
            );
        }
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn a_composite_codec_reads_back_what_it_wrote() {
        let mut rt = Rt::new();
        rt.lambda_call = Some(lambdas);
        let pos_codec = sv(&mut rt, "net/minecraft/core/BlockPos.STREAM_CODEC");
        let float = sv(&mut rt, "net/minecraft/network/codec/ByteBufCodecs.FLOAT");
        let (pos_of, speed_of, new) = (rt.lambda_id("test/Packet", 1, Vec::new()), rt.lambda_id("test/Packet", 2, Vec::new()), rt.lambda_id("test/Packet", 3, Vec::new()));
        let composite = call(
            &mut rt,
            "net/minecraft/network/codec/StreamCodec.composite.(Lnet/minecraft/network/codec/StreamCodec;Ljava/util/function/Function;Lnet/minecraft/network/codec/StreamCodec;Ljava/util/function/Function;Ljava/util/function/BiFunction;)Lnet/minecraft/network/codec/StreamCodec;",
            &[pos_codec, pos_of, float, speed_of, new],
        )
        .unwrap();
        // A list of them, as `ByteBufCodecs.list()` applies.
        let list_op = call(&mut rt, "net/minecraft/network/codec/ByteBufCodecs.list.()Lnet/minecraft/network/codec/StreamCodec$CodecOperation;", &[]).unwrap();
        let codec = call(&mut rt, "net/minecraft/network/codec/StreamCodec.apply.(Lnet/minecraft/network/codec/StreamCodec$CodecOperation;)Lnet/minecraft/network/codec/StreamCodec;", &[composite, list_op]).unwrap();
        let mut packets = Vec::new();
        for (i, speed) in [(1, 8.0f32), (2, -16.0)] {
            let p = rt.new_object("test/Packet");
            let pos = crate::api::block_pos(&mut rt, i, 64, -i);
            rt.put_field(p, "pos", pos);
            rt.put_field(p, "speed", V::F(speed));
            packets.push(p);
        }
        let list = crate::jutil::new_list(&mut rt, packets);
        let (bytes, back) = round_trip(&mut rt, codec, list);
        assert_eq!(bytes.len(), 1 + 2 * (8 + 4));
        let items = crate::jutil::elements(&rt, back);
        assert_eq!(items.len(), 2);
        assert_eq!(native(&rt, rt.get_field(items[1], "pos"))[..3], [2, 64, -2]);
        assert_eq!(rt.get_field(items[1], "speed"), V::F(-16.0));
    }

    /// `ByteBufUtil`: text goes in as UTF-8 and the count of its bytes comes back (a mod writes
    /// the length first and checks the two agree); a buffer's bytes come out as an array, those
    /// left to read or a stretch by index, with the buffer left where it was.
    #[test]
    fn a_buffers_bytes_and_text_through_netty_s_helper() {
        let mut rt = Rt::new();
        let buf = new_buffer(&mut rt);
        let text = rt.string("d\u{e9}j\u{e0} vu");
        let wrote = call(
            &mut rt,
            "io/netty/buffer/ByteBufUtil.writeUtf8.(Lio/netty/buffer/ByteBuf;Ljava/lang/CharSequence;)I",
            &[buf, text],
        );
        assert_eq!(
            wrote,
            Some(V::I(9)),
            "two of the seven characters take two bytes"
        );
        call(&mut rt, "io/netty/buffer/ByteBuf.readByte.()B", &[buf]);
        let rest = call(
            &mut rt,
            "io/netty/buffer/ByteBufUtil.getBytes.(Lio/netty/buffer/ByteBuf;)[B",
            &[buf],
        )
        .unwrap();
        assert_eq!(byte_array(&rt, rest), "\u{e9}j\u{e0} vu".as_bytes());
        let part = call(
            &mut rt,
            "io/netty/buffer/ByteBufUtil.getBytes.(Lio/netty/buffer/ByteBuf;II)[B",
            &[buf, V::I(0), V::I(3)],
        )
        .unwrap();
        assert_eq!(byte_array(&rt, part), "d\u{e9}".as_bytes());
        assert_eq!(
            call(&mut rt, "io/netty/buffer/ByteBuf.readableBytes.()I", &[buf]),
            Some(V::I(8)),
            "reading its bytes moved nothing"
        );
        call(
            &mut rt,
            "io/netty/buffer/ByteBufUtil.getBytes.(Lio/netty/buffer/ByteBuf;II)[B",
            &[buf, V::I(5), V::I(9)],
        );
        assert_eq!(
            rt.thrown.map(|e| rt.class_of(e)),
            Some("java/lang/IndexOutOfBoundsException")
        );
    }

    #[test]
    fn a_short_buffer_throws_instead_of_reading_garbage() {
        let mut rt = Rt::new();
        let int = sv(&mut rt, "net/minecraft/network/codec/ByteBufCodecs.INT");
        let buf = buffer_with(&mut rt, vec![0, 1]);
        assert_eq!(decode(&mut rt, int, buf), V::NULL);
        assert_eq!(rt.thrown.map(|e| rt.class_of(e)), Some("java/lang/IndexOutOfBoundsException"));
    }

    /// The handler of the test payload (lambda 7): records the context's player and the payload.
    fn handler(rt: &mut Rt, id: u32, args: &[V]) -> Option<V> {
        match id {
            7 => {
                let ctx = args[2];
                let player = call(rt, "net/neoforged/neoforge/network/handling/IPayloadContext.player.()Lnet/minecraft/world/entity/player/Player;", &[ctx]).unwrap();
                rt.interned.insert("test/got".into(), args[1]);
                rt.interned.insert("test/player".into(), player);
                Some(V::Void)
            }
            _ => lambdas(rt, id, args),
        }
    }

    /// The test payload's `type()`: one `Type` per runtime, `test:speed`.
    fn vcall(rt: &mut Rt, _owner: &'static str, key: &'static str, _args: &[V]) -> V {
        if key.starts_with("type.") {
            rt.intern("test/type".into(), |rt| {
                let t = rt.new_object(PAYLOAD_TYPE);
                let id = rt.string("test:speed");
                call(rt, "net/minecraft/network/protocol/common/custom/CustomPacketPayload$Type.<init>.(Lnet/minecraft/resources/ResourceLocation;)V", &[t, id]);
                t
            })
        } else {
            V::Void
        }
    }

    /// Registers the test payload as a mod does in `RegisterPayloadHandlersEvent`.
    fn register(rt: &mut Rt) {
        rt.lambda_call = Some(handler);
        rt.vcall = Some(vcall);
        let ev = rt.new_object(REGISTER_PAYLOADS);
        let version = rt.string("1");
        let registrar = call(rt, "net/neoforged/neoforge/network/event/RegisterPayloadHandlersEvent.registrar.(Ljava/lang/String;)Lnet/neoforged/neoforge/network/registration/PayloadRegistrar;", &[ev, version]).unwrap();
        let ty = vcall(rt, "", "type.()", &[]);
        let pos_codec = sv(rt, "net/minecraft/core/BlockPos.STREAM_CODEC");
        let float = sv(rt, "net/minecraft/network/codec/ByteBufCodecs.FLOAT");
        let (pos_of, speed_of, new) = (rt.lambda_id("test/Packet", 1, Vec::new()), rt.lambda_id("test/Packet", 2, Vec::new()), rt.lambda_id("test/Packet", 3, Vec::new()));
        let codec = call(
            rt,
            "net/minecraft/network/codec/StreamCodec.composite.(Lnet/minecraft/network/codec/StreamCodec;Ljava/util/function/Function;Lnet/minecraft/network/codec/StreamCodec;Ljava/util/function/Function;Ljava/util/function/BiFunction;)Lnet/minecraft/network/codec/StreamCodec;",
            &[pos_codec, pos_of, float, speed_of, new],
        )
        .unwrap();
        let handle = rt.lambda_id("test/Handler", 7, Vec::new());
        call(rt, "net/neoforged/neoforge/network/registration/PayloadRegistrar.playToClient.(Lnet/minecraft/network/protocol/common/custom/CustomPacketPayload$Type;Lnet/minecraft/network/codec/StreamCodec;Lnet/neoforged/neoforge/network/handling/IPayloadHandler;)Lnet/neoforged/neoforge/network/registration/PayloadRegistrar;", &[registrar, ty, codec, handle]);
    }

    /// NeoForge's own payloads have their types: the Fabric API (through Connector) makes a
    /// `MinecraftRegisterPayload` to announce its channels and refuses a payload whose `type()`
    /// is null ("CustomPayload#getId() cannot return null").
    #[test]
    fn neoforges_own_payloads_name_their_channels() {
        fn rt_key(key: &str) -> &'static str {
            Box::leak(key.to_owned().into_boxed_str())
        }
        const TYPE: &str = "net/minecraft/network/protocol/common/custom/CustomPacketPayload.type.()Lnet/minecraft/network/protocol/common/custom/CustomPacketPayload$Type;";
        crate::install();
        let mut rt = Rt::new();
        for (class, channel) in [
            (
                "net/neoforged/neoforge/network/payload/MinecraftRegisterPayload",
                "minecraft:register",
            ),
            (
                "net/neoforged/neoforge/network/payload/MinecraftUnregisterPayload",
                "minecraft:unregister",
            ),
        ] {
            let payload = rt.new_object(class);
            let ty = crate::api::call(&mut rt, TYPE, &[payload]);
            // (The class's own statics name the same channel and are that one type.)
            let named = crate::api::get_static(&mut rt, rt_key(&format!("{class}.ID")));
            assert_eq!(rt.str_of(named), Some(channel));
            assert_eq!(
                crate::api::get_static(&mut rt, rt_key(&format!("{class}.TYPE"))),
                ty
            );
            let id = crate::api::call(
                &mut rt,
                "net/minecraft/network/protocol/common/custom/CustomPacketPayload$Type.id.()Lnet/minecraft/resources/ResourceLocation;",
                &[ty],
            );
            assert_eq!(rt.str_of(id), Some(channel));
        }
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// A client mod sends its payload on its connection (`Minecraft.getConnection().send(new
    /// ServerboundCustomPayloadPacket(payload))`): it goes to the server on the mods' channel, as
    /// `PacketDistributor.sendToServer` sends it. A game packet a mod builds isn't sent.
    #[test]
    fn a_payload_sent_on_the_clients_connection_goes_to_the_server() {
        let mut rt = Rt::new();
        register(&mut rt);
        rt.interned.insert("side:client".into(), V::I(1));
        let packet = rt.new_object("test/Packet");
        let pos = crate::api::block_pos(&mut rt, 1, 2, 3);
        rt.put_field(packet, "pos", pos);
        rt.put_field(packet, "speed", V::F(2.0));
        let send = "net/minecraft/client/multiplayer/ClientPacketListener.send.(Lnet/minecraft/network/protocol/Packet;)V";
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| {
            let mc = crate::api::call(rt, "net/minecraft/client/Minecraft.getInstance.()Lnet/minecraft/client/Minecraft;", &[]);
            let conn = crate::api::call(rt, "net/minecraft/client/Minecraft.getConnection.()Lnet/minecraft/client/multiplayer/ClientPacketListener;", &[mc]);
            assert_ne!(conn, V::NULL);
            let wrapped = rt.new_object(SERVERBOUND_PAYLOAD);
            crate::api::call(rt, "net/minecraft/network/protocol/common/ServerboundCustomPayloadPacket.<init>.(Lnet/minecraft/network/protocol/common/custom/CustomPacketPayload;)V", &[wrapped, packet]);
            crate::api::call(rt, send, &[conn, wrapped]);
            let swing = rt.new_object("net/minecraft/network/protocol/game/ServerboundSwingPacket");
            crate::api::call(rt, send, &[conn, swing]);
        });
        assert_eq!(host.payloads.len(), 1);
        assert_eq!((host.payloads[0].0, host.payloads[0].1.as_str()), (PayloadTarget::Server, "test:speed"));
        // The game packet left unsent is counted under its class.
        assert_eq!(rt.unmapped.keys().copied().collect::<Vec<_>>(), ["net/minecraft/network/protocol/game/ServerboundSwingPacket.<sent>.()V"]);
    }

    /// The server's thread sees the client's connection as NeoForge's single player does: null
    /// until a player is in the server (the integrated server starting), then there, whether the
    /// player acts in its own hook (an item's use: the server's list leaves it out) or is listed,
    /// and the registries it gives (Create's `GlobalRegistryAccess`:
    /// `Minecraft.getInstance().getConnection().registryAccess()`) are the level's.
    #[test]
    fn the_server_thread_sees_the_clients_connection_once_a_player_is_in() {
        let mut rt = Rt::new();
        let connection = |rt: &mut Rt| {
            let mc = crate::api::call(rt, "net/minecraft/client/Minecraft.getInstance.()Lnet/minecraft/client/Minecraft;", &[]);
            crate::api::call(rt, "net/minecraft/client/Minecraft.getConnection.()Lnet/minecraft/client/multiplayer/ClientPacketListener;", &[mc])
        };
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| assert_eq!(connection(rt), V::NULL, "nobody in the server yet"));
        let mut acting = MemoryHost { player_id: Some(0xa), ..MemoryHost::default() };
        rt.with_host(&mut acting, |rt| assert_ne!(connection(rt), V::NULL, "a player using an item"));
        host.players.push((0xa, [0.5, 64.0, 0.5]));
        rt.with_host(&mut host, |rt| {
            let conn = connection(rt);
            assert_ne!(conn, V::NULL);
            let access = crate::api::call(rt, "net/minecraft/client/multiplayer/ClientPacketListener.registryAccess.()Lnet/minecraft/core/RegistryAccess$Frozen;", &[conn]);
            let level = crate::glue::level(rt);
            let levels = crate::api::call(rt, "net/minecraft/world/level/Level.registryAccess.()Lnet/minecraft/core/RegistryAccess;", &[level]);
            assert_ne!(access, V::NULL);
            assert_eq!(access, levels, "the level's registries");
        });
    }

    /// A server mod sends a player a payload on the player's connection (`player.connection.send(new
    /// ClientboundCustomPayloadPacket(payload))`): it goes to that player alone, on the mods'
    /// channel. The connection is the player's own (`connection.player`) and knows the channels.
    #[test]
    fn a_payload_sent_on_a_players_connection_goes_to_that_player() {
        let mut rt = Rt::new();
        register(&mut rt);
        let uuid = 0x0123_4567_89ab_cdef_0011_2233_4455_6677_u128;
        let player = crate::menus::server_player(&mut rt, uuid);
        let packet = rt.new_object("test/Packet");
        let pos = crate::api::block_pos(&mut rt, 4, 5, 6);
        rt.put_field(packet, "pos", pos);
        rt.put_field(packet, "speed", V::F(1.0));
        let field = "net/minecraft/server/level/ServerPlayer.$connection.()Lnet/minecraft/server/network/ServerGamePacketListenerImpl;";
        let listener = "net/minecraft/server/network/ServerGamePacketListenerImpl";
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| {
            let conn = crate::api::game_field(rt, player, "connection", "$connection", field);
            assert_eq!(rt.class_of(conn), listener);
            assert_eq!(crate::api::game_field(rt, player, "connection", "$connection", field), conn);
            assert_eq!(rt.get_field(conn, "player"), player);
            let wrapped = rt.new_object(CLIENTBOUND_PAYLOAD);
            crate::api::call(rt, "net/minecraft/network/protocol/common/ClientboundCustomPayloadPacket.<init>.(Lnet/minecraft/network/protocol/common/custom/CustomPacketPayload;)V", &[wrapped, packet]);
            crate::api::call(rt, "net/minecraft/server/network/ServerGamePacketListenerImpl.send.(Lnet/minecraft/network/protocol/Packet;)V", &[conn, wrapped]);
            let ty = vcall(rt, "", "type.()", &[]);
            let other = rt.string("test:other");
            let has = |rt: &mut Rt, key: &'static str, v: V| crate::api::call(rt, key, &[conn, v]);
            assert_eq!(has(rt, "net/minecraft/server/network/ServerGamePacketListenerImpl.hasChannel.(Lnet/minecraft/network/protocol/common/custom/CustomPacketPayload$Type;)Z", ty), V::I(1));
            assert_eq!(has(rt, "net/minecraft/server/network/ServerGamePacketListenerImpl.hasChannel.(Lnet/minecraft/resources/ResourceLocation;)Z", other), V::I(0));
        });
        assert_eq!(host.payloads.len(), 1);
        assert_eq!((host.payloads[0].0, host.payloads[0].1.as_str()), (PayloadTarget::Player(uuid), "test:speed"));
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    #[test]
    fn a_payload_goes_from_the_servers_runtime_to_the_clients_handler() {
        // The server's runtime sends one to every player.
        let mut server = Rt::new();
        register(&mut server);
        assert_eq!(registered(&server), ["test:speed"]);
        let packet = server.new_object("test/Packet");
        let pos = crate::api::block_pos(&mut server, 3, 70, -9);
        server.put_field(packet, "pos", pos);
        server.put_field(packet, "speed", V::F(0.5));
        let more = server.new_array(0, V::NULL);
        let mut host = MemoryHost::default();
        server.with_host(&mut host, |rt| {
            call(rt, "net/neoforged/neoforge/network/PacketDistributor.sendToAllPlayers.(Lnet/minecraft/network/protocol/common/custom/CustomPacketPayload;[Lnet/minecraft/network/protocol/common/custom/CustomPacketPayload;)V", &[packet, more]);
        });
        assert_eq!(host.payloads.len(), 1);
        let (to, channel, data) = host.payloads[0].clone();
        assert_eq!((to, channel.as_str()), (PayloadTarget::All, "test:speed"));

        // The client's runtime, a different heap, decodes it and runs the handler.
        let mut client = Rt::new();
        register(&mut client);
        let mut client_host = MemoryHost::default();
        assert!(client.with_host(&mut client_host, |rt| receive(rt, &channel, &data, None)));
        let got = client.interned["test/got"];
        assert_eq!(native(&client, client.get_field(got, "pos"))[..3], [3, 70, -9]);
        assert_eq!(client.get_field(got, "speed"), V::F(0.5));
        assert_eq!(client.class_of(client.interned["test/player"]), "net/minecraft/client/player/LocalPlayer");
        // Nothing of this mod's has another id.
        assert!(!client.with_host(&mut client_host, |rt| receive(rt, "test:other", &data, None)));
    }

    /// The game takes so many messages a tick for one player, and so many bytes a message. What
    /// it takes no more of waits and goes with the next ticks, each player's in the order sent
    /// (a pack's mods send a player dozens at a login: past the game's 32 they were dropped);
    /// what is larger than a message goes in parts and is one message again where it arrives.
    #[test]
    fn what_the_game_takes_no_more_of_waits_and_a_large_payload_goes_in_parts() {
        /// A game that takes `left` more messages this tick, of 30,000 bytes at most.
        struct Capped {
            took: Vec<(PayloadTarget, String, Vec<u8>)>,
            left: u32,
        }
        impl crate::host::Host for Capped {
            fn block_state(&self, _: [i32; 3]) -> String {
                "minecraft:air".into()
            }
            fn set_block_state(&mut self, _: [i32; 3], _: &str) -> bool {
                false
            }
            fn is_server(&self) -> bool {
                true
            }
            fn players(&self) -> Vec<(u128, [f64; 3])> {
                vec![(7, [0.0; 3]), (9, [0.0; 3])]
            }
            fn send_payload(&mut self, to: PayloadTarget, channel: &str, data: &[u8]) -> bool {
                // (A channel the game does not carry, and one it never takes for this test.)
                if self.left == 0
                    || data.len() > 30_000
                    || !game_takes(channel)
                    || channel == "t:never"
                {
                    return false;
                }
                self.left -= 1;
                self.took.push((to, channel.to_owned(), data.to_vec()));
                true
            }
        }
        let (seven, nine) = (PayloadTarget::Player(7), PayloadTarget::Player(9));
        let mut rt = Rt::new();
        let mut host = Capped {
            took: Vec::new(),
            left: 2,
        };
        rt.with_host(&mut host, |rt| {
            for i in 0..5u8 {
                assert!(send_raw(rt, seven, "t:sync", &[i]));
            }
            // To everyone, when none can take it now: for each player the level has.
            assert!(send_raw(rt, PayloadTarget::All, "t:all", &[9]));
        });
        let went = |host: &Capped| {
            host.took
                .iter()
                .map(|(to, channel, data)| (*to, channel.clone(), data[0]))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            went(&host),
            [
                (seven, "t:sync".to_owned(), 0),
                (seven, "t:sync".to_owned(), 1)
            ]
        );
        // The next tick takes three more, the one after the rest: in the order sent.
        host.left = 3;
        rt.with_host(&mut host, send_waiting);
        assert_eq!(
            went(&host)[2..],
            [
                (seven, "t:sync".to_owned(), 2),
                (seven, "t:sync".to_owned(), 3),
                (seven, "t:sync".to_owned(), 4)
            ]
        );
        host.left = 9;
        rt.with_host(&mut host, |rt| {
            // One sent while others wait for the same player goes behind them.
            assert!(send_raw(rt, seven, "t:sync", &[5]));
            send_waiting(rt);
            send_waiting(rt);
        });
        assert_eq!(
            went(&host)[5..],
            [
                (seven, "t:all".to_owned(), 9),
                (nine, "t:all".to_owned(), 9),
                (seven, "t:sync".to_owned(), 5)
            ]
        );
        assert_eq!(host.left, 6, "nothing goes twice");

        // A payload of 70,000 bytes: three messages the game takes, one payload where they come.
        let mut server = Rt::new();
        register(&mut server);
        let packet = server.new_object("test/Packet");
        let pos = crate::api::block_pos(&mut server, 3, 70, -9);
        server.put_field(packet, "pos", pos);
        server.put_field(packet, "speed", V::F(0.5));
        let (channel, mut data) = encode_payload(&mut server, packet).unwrap();
        data.resize(70_000, 0xab);
        let mut host = Capped {
            took: Vec::new(),
            left: 9,
        };
        assert!(server.with_host(&mut host, |rt| send_raw(rt, seven, &channel, &data)));
        assert_eq!(host.took.len(), 3);
        assert!(host.took.iter().all(|(to, channel, part)| *to == seven
            && channel == PARTS_CHANNEL
            && part.len() <= 30_000));
        let mut client = Rt::new();
        register(&mut client);
        let mut client_host = MemoryHost::default();
        // In any order: nothing is handled until the last has come, then the handler runs once.
        for i in [2, 0] {
            assert!(client.with_host(&mut client_host, |rt| receive(
                rt,
                PARTS_CHANNEL,
                &host.took[i].2,
                None
            )));
            assert!(!client.interned.contains_key("test/got"));
        }
        assert!(client.with_host(&mut client_host, |rt| receive(
            rt,
            PARTS_CHANNEL,
            &host.took[1].2,
            None
        )));
        let got = client.interned["test/got"];
        assert_eq!(
            native(&client, client.get_field(got, "pos"))[..3],
            [3, 70, -9]
        );
        assert_eq!(client.get_field(got, "speed"), V::F(0.5));
        // A part that says nothing of the sort is refused.
        assert!(!client.with_host(&mut client_host, |rt| receive(
            rt,
            PARTS_CHANNEL,
            &[0, 1, 2],
            None
        )));

        // A payload type whose name the game carries no message under (nothing after the
        // colon, as Puzzles Lib's; a dash in the mod's id) goes inside one of the runtime's own.
        let mut host = Capped {
            took: Vec::new(),
            left: 9,
        };
        let mut rt = Rt::new();
        rt.with_host(&mut host, |rt| {
            assert!(send_raw(rt, seven, "puzzleslib:", &[1, 2, 3]));
            assert!(send_raw(rt, seven, "fabric-api:hello", &[]));
        });
        assert_eq!(host.took.len(), 2);
        assert!(
            host.took
                .iter()
                .all(|(_, channel, _)| channel == PARTS_CHANNEL)
        );
        let inner: Vec<String> = host
            .took
            .iter()
            .map(|(_, _, part)| {
                let named = usize::from(u16::from_be_bytes([part[8], part[9]]));
                String::from_utf8_lossy(&part[10..10 + named]).into_owned()
            })
            .collect();
        assert_eq!(inner, ["puzzleslib:", "fabric-api:hello"]);
        // One the game refuses with its allowance whole is that message's own trouble: it
        // holds the player's others back for a second, no longer, and they go on without it.
        let mut host = Capped {
            took: Vec::new(),
            left: 0,
        };
        rt.with_host(&mut host, |rt| {
            assert!(send_raw(rt, nine, "t:never", &[1]));
            assert!(send_raw(rt, nine, "t:after", &[2]));
        });
        for _ in 0..HEAD_PATIENCE {
            host.left = 5;
            rt.with_host(&mut host, send_waiting);
        }
        assert!(host.took.is_empty(), "held back: {:?}", went(&host));
        host.left = 5;
        rt.with_host(&mut host, send_waiting);
        assert_eq!(went(&host), [(nine, "t:after".to_owned(), 2)]);
    }

    /// A payload to those tracking a player goes to every player but that one (a player's own client
    /// doesn't track them: Create tells the others of a rider's limb swing so); to those tracking a
    /// mod's entity, or a player and the player, it goes to everyone.
    #[test]
    fn a_payload_to_those_tracking_a_player_leaves_the_player_out() {
        const PAYLOADS: &str = "(Lnet/minecraft/world/entity/Entity;Lnet/minecraft/network/protocol/common/custom/CustomPacketPayload;[Lnet/minecraft/network/protocol/common/custom/CustomPacketPayload;)V";
        let tracking: &'static str = format!("net/neoforged/neoforge/network/PacketDistributor.sendToPlayersTrackingEntity.{PAYLOADS}").leak();
        let and_self: &'static str = format!("net/neoforged/neoforge/network/PacketDistributor.sendToPlayersTrackingEntityAndSelf.{PAYLOADS}").leak();
        let mut rt = Rt::new();
        register(&mut rt);
        let uuid = 0x0123_4567_89ab_cdef_0011_2233_4455_6677_u128;
        let player = crate::menus::server_player(&mut rt, uuid);
        let carriage = rt.new_object("test/Carriage");
        let packet = rt.new_object("test/Packet");
        let pos = crate::api::block_pos(&mut rt, 4, 5, 6);
        rt.put_field(packet, "pos", pos);
        rt.put_field(packet, "speed", V::F(1.0));
        let more = rt.new_array(0, V::NULL);
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| {
            for (key, about) in [(tracking, player), (and_self, player), (tracking, carriage)] {
                call(rt, key, &[about, packet, more]);
            }
        });
        let to: Vec<PayloadTarget> = host.payloads.iter().map(|p| p.0).collect();
        assert_eq!(
            to,
            [
                PayloadTarget::Others(uuid),
                PayloadTarget::All,
                PayloadTarget::All
            ]
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    /// `readEnum(class)` gives back the constant `writeEnum` wrote (a vanilla enum's, from its
    /// `values()`), on either buffer; an ordinal past the constants throws, as the array does.
    #[test]
    fn read_enum_gives_back_the_constant_write_enum_wrote() {
        const WRITE: &str = "net/minecraft/network/FriendlyByteBuf.writeEnum.(Ljava/lang/Enum;)Lnet/minecraft/network/FriendlyByteBuf;";
        const WRITE_VAR_INT: &str = "net/minecraft/network/FriendlyByteBuf.writeVarInt.(I)Lnet/minecraft/network/FriendlyByteBuf;";
        const READ: &str =
            "net/minecraft/network/FriendlyByteBuf.readEnum.(Ljava/lang/Class;)Ljava/lang/Enum;";
        const READ_REGISTRY: &str = "net/minecraft/network/RegistryFriendlyByteBuf.readEnum.(Ljava/lang/Class;)Ljava/lang/Enum;";
        let mut rt = Rt::new();
        crate::install();
        let b = new_buffer(&mut rt);
        let (east, negative) = (
            sv(&mut rt, "net/minecraft/core/Direction.EAST"),
            sv(
                &mut rt,
                "net/minecraft/core/Direction$AxisDirection.NEGATIVE",
            ),
        );
        crate::api::call(&mut rt, WRITE, &[b, east]);
        crate::api::call(&mut rt, WRITE_VAR_INT, &[b, V::I(1)]);
        crate::api::call(&mut rt, WRITE_VAR_INT, &[b, V::I(6)]);
        let (direction, axis_direction) = (
            rt.class_literal("net/minecraft/core/Direction"),
            rt.class_literal("net/minecraft/core/Direction$AxisDirection"),
        );
        assert_eq!(
            crate::api::call(&mut rt, READ_REGISTRY, &[b, direction]),
            east
        );
        assert_eq!(
            crate::api::call(&mut rt, READ, &[b, axis_direction]),
            negative
        );
        assert_eq!(crate::api::call(&mut rt, READ, &[b, direction]), V::NULL);
        let thrown = rt.thrown.take().expect("the ordinal is past the constants");
        assert_eq!(
            rt.class_of(thrown),
            "java/lang/ArrayIndexOutOfBoundsException"
        );
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }
}
