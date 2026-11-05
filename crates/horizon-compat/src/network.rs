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
use std::collections::BTreeMap;

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
/// NeoForge's `lazy(supplier)`: [supplier]; the codec it gives, asked once, does the work.
const LAZY: &str = "compat:stream/lazy";
/// NeoForge's `registryKey()`: a registry's own key, by its location.
const REGISTRY_KEY: &str = "compat:stream/registry_key";
const NEO_CODECS: &str = "net/neoforged/neoforge/network/codec/NeoForgeStreamCodecs";
/// `ByteBufCodecs.list()`, a `CodecOperation` (`codec.apply(list())`).
const LIST_OP: &str = "compat:stream-op/list";

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
    let made = rt.call_functional(SUPPLIER, supplier, &[]);
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
        ("net/neoforged/neoforge/fluids/FluidStack", "STREAM_CODEC" | "OPTIONAL_STREAM_CODEC") => FLUID_STACK,
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

fn nbt_id(n: &Nbt) -> u8 {
    match n {
        Nbt::Byte(_) => 1,
        Nbt::Short(_) => 2,
        Nbt::Int(_) => 3,
        Nbt::Long(_) => 4,
        Nbt::Float(_) => 5,
        Nbt::Double(_) => 6,
        Nbt::ByteArray(_) => 7,
        Nbt::String(_) => 8,
        Nbt::List(_) => 9,
        Nbt::Compound(_) => 10,
        Nbt::IntArray(_) => 11,
        Nbt::LongArray(_) => 12,
    }
}

fn put_nbt_string(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    let n = b.len().min(usize::from(u16::MAX));
    out.extend_from_slice(&(n as u16).to_be_bytes());
    out.extend_from_slice(&b[..n]);
}

fn put_nbt_payload(out: &mut Vec<u8>, n: &Nbt) {
    match n {
        Nbt::Byte(b) => out.push(*b as u8),
        Nbt::Short(s) => out.extend_from_slice(&s.to_be_bytes()),
        Nbt::Int(i) => out.extend_from_slice(&i.to_be_bytes()),
        Nbt::Long(l) => out.extend_from_slice(&l.to_be_bytes()),
        Nbt::Float(f) => out.extend_from_slice(&f.to_be_bytes()),
        Nbt::Double(d) => out.extend_from_slice(&d.to_be_bytes()),
        Nbt::ByteArray(a) => {
            out.extend_from_slice(&(a.len() as i32).to_be_bytes());
            out.extend(a.iter().map(|&b| b as u8));
        }
        Nbt::String(s) => put_nbt_string(out, s),
        Nbt::List(items) => {
            out.push(items.first().map_or(0, nbt_id));
            out.extend_from_slice(&(items.len() as i32).to_be_bytes());
            for i in items {
                put_nbt_payload(out, i);
            }
        }
        Nbt::Compound(c) => {
            for (k, v) in c {
                out.push(nbt_id(v));
                put_nbt_string(out, k);
                put_nbt_payload(out, v);
            }
            out.push(0);
        }
        Nbt::IntArray(a) => {
            out.extend_from_slice(&(a.len() as i32).to_be_bytes());
            for i in a {
                out.extend_from_slice(&i.to_be_bytes());
            }
        }
        Nbt::LongArray(a) => {
            out.extend_from_slice(&(a.len() as i32).to_be_bytes());
            for l in a {
                out.extend_from_slice(&l.to_be_bytes());
            }
        }
    }
}

fn put_nbt(rt: &mut Rt, buf: V, n: Option<&Nbt>) {
    let mut out = Vec::new();
    match n {
        Some(n) => {
            out.push(nbt_id(n));
            put_nbt_payload(&mut out, n);
        }
        None => out.push(0),
    }
    put(rt, buf, &out);
}

struct NbtReader<'a> {
    b: &'a [u8],
    at: usize,
}

impl NbtReader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let end = self.at.checked_add(n)?;
        let s = self.b.get(self.at..end)?;
        self.at = end;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn i16(&mut self) -> Option<i16> {
        Some(i16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }
    fn i64(&mut self) -> Option<i64> {
        Some(i64::from_be_bytes(self.take(8)?.try_into().ok()?))
    }
    fn len(&mut self) -> Option<usize> {
        let n = usize::try_from(self.i32()?).ok()?;
        (n <= self.b.len() - self.at).then_some(n)
    }
    fn string(&mut self) -> Option<String> {
        let n = usize::from(u16::from_be_bytes(self.take(2)?.try_into().ok()?));
        Some(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn tag(&mut self, id: u8, depth: u32) -> Option<Nbt> {
        if depth > 512 {
            return None;
        }
        Some(match id {
            1 => Nbt::Byte(self.u8()? as i8),
            2 => Nbt::Short(self.i16()?),
            3 => Nbt::Int(self.i32()?),
            4 => Nbt::Long(self.i64()?),
            5 => Nbt::Float(f32::from_bits(self.i32()? as u32)),
            6 => Nbt::Double(f64::from_bits(self.i64()? as u64)),
            7 => {
                let n = self.len()?;
                Nbt::ByteArray(self.take(n)?.iter().map(|&b| b as i8).collect())
            }
            8 => Nbt::String(self.string()?),
            9 => {
                let t = self.u8()?;
                let n = self.len()?;
                let mut items = Vec::with_capacity(n);
                for _ in 0..n {
                    items.push(self.tag(t, depth + 1)?);
                }
                Nbt::List(items)
            }
            10 => {
                let mut m = BTreeMap::new();
                loop {
                    let t = self.u8()?;
                    if t == 0 {
                        break;
                    }
                    let k = self.string()?;
                    let v = self.tag(t, depth + 1)?;
                    m.insert(k, v);
                }
                Nbt::Compound(m)
            }
            11 => {
                let n = self.len()?;
                let mut a = Vec::with_capacity(n);
                for _ in 0..n {
                    a.push(self.i32()?);
                }
                Nbt::IntArray(a)
            }
            12 => {
                let n = self.len()?;
                let mut a = Vec::with_capacity(n);
                for _ in 0..n {
                    a.push(self.i64()?);
                }
                Nbt::LongArray(a)
            }
            _ => return None,
        })
    }
}

/// A tag read off the buffer: Some(None) for none (`TAG_End`), None (thrown) when it doesn't parse.
fn read_nbt(rt: &mut Rt, buf: V) -> Option<Option<Nbt>> {
    let parsed = bytes_mut(rt, buf).and_then(|(v, at)| {
        let mut r = NbtReader { b: &v[*at..], at: 0 };
        let out = match r.u8()? {
            0 => Some(None),
            t => Some(Some(r.tag(t, 0)?)),
        };
        *at += r.at;
        out
    });
    if parsed.is_none() && rt.thrown.is_none() {
        rt.throw_new("io/netty/handler/codec/DecoderException");
    }
    parsed
}

/// A runtime tag (a `CompoundTag`, or any tag object) as NBT.
fn tag_nbt(rt: &Rt, tag: V) -> Option<Nbt> {
    if tag.r() == 0 {
        return None;
    }
    match rt.heap.get(tag.r() as usize).map(|o| &o.data) {
        Some(Data::Map(_)) => Some(Nbt::Compound(crate::nbt::from_runtime(rt, tag))),
        _ => crate::nbt::element_from_runtime(rt, tag),
    }
}

fn nbt_tag(rt: &mut Rt, n: &Nbt) -> V {
    match n {
        Nbt::Compound(c) => crate::nbt::to_runtime(rt, c),
        other => crate::nbt::element_to_runtime(rt, other).unwrap_or(V::NULL),
    }
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
}

fn read_fluid(rt: &mut Rt, buf: V) -> Option<V> {
    let amount = read_var_int(rt, buf)?;
    if amount <= 0 {
        return Some(crate::fluids::fluid_stack_empty(rt));
    }
    let name = read_utf(rt, buf)?;
    let f = crate::fluids::fluid(rt, &name);
    Some(if f.r() == 0 { crate::fluids::fluid_stack_empty(rt) } else { crate::fluids::fluid_stack(rt, f, amount) })
}

/// The name of an entry of `registry` (`fluid`, `item`...): the mod's, or one of Minecraft's.
fn entry_name(rt: &Rt, registry: &str, v: V) -> String {
    if let Some(n) = rt.registered_name(v, registry) {
        return n.to_owned();
    }
    match registry {
        "fluid" => crate::fluids::name_of(rt, v),
        "item" | "block" => crate::api_common::item_like_name(rt, v),
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
            let n = tag_nbt(rt, value);
            put_nbt(rt, buf, n.as_ref());
        }
        // `codec.encodeStart(NbtOps.INSTANCE, value).getOrThrow(...)`, then `writeNbt`.
        FROM_CODEC => match crate::codecs::encode(rt, p(0), value) {
            Ok(n) => put_nbt(rt, buf, Some(&n)),
            Err(e) => throw_with(rt, "io/netty/handler/codec/EncoderException", &format!("Failed to encode: {e}")),
        },
        OPTIONAL_TAG => {
            let n = optional_held(rt, value).and_then(|v| tag_nbt(rt, v));
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
        COMPONENT => {
            let s = text(rt, rt.get_field(value, "$text"));
            put_utf(rt, buf, &s);
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
            Some(n) => nbt_tag(rt, &n),
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
                Some(n) => nbt_tag(rt, &n),
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
        COMPONENT => {
            let s = read_utf(rt, buf)?;
            let t = rt.new_object("net/minecraft/network/chat/MutableComponent");
            let s = rt.string(&s);
            rt.put_field(t, "$text", s);
            t
        }
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

/// Sends a payload already encoded (`data` on channel `channel`) to `to`. One a screen's input
/// sends (its host only reads) goes with the client's next tick ([`crate::screens::flush`]).
pub fn send_raw(rt: &mut Rt, to: PayloadTarget, channel: &str, data: &[u8]) -> bool {
    if crate::screens::defer_payload(rt, to, channel, data) {
        return true;
    }
    let sent = rt.host_mut().is_some_and(|h| h.send_payload(to, channel, data));
    crate::glue::trace_some(rt, "payload-sent", channel, 40, |_| {
        format!("payload {channel} ({} bytes) to {to:?}: {}", data.len(), if sent { "sent" } else { "not sent" })
    });
    sent
}

/// A payload that came over the mod channel: read with its type's codec and handed to its handler.
/// `player` is who sent it, on a server's runtime; None on a client's. False when no payload type
/// has that id here.
pub fn receive(rt: &mut Rt, channel: &str, data: &[u8], player: Option<u128>) -> bool {
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
    rt.intern("compat:local-player".into(), |rt| {
        let p = rt.new_object("net/minecraft/client/player/LocalPlayer");
        crate::menus::init_player(rt, p);
        p
    })
}

/// A payload handler's `IPayloadContext`: from `player` (a server's runtime), or from the server.
fn context(rt: &mut Rt, player: Option<u128>) -> V {
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
    ) || (owner == REGISTER_PAYLOADS && name == "registrar")
        || (owner == "net/minecraft/network/protocol/common/custom/CustomPacketPayload" && name == "createType")
        || (owner == "net/minecraft/resources/ResourceKey" && name == "streamCodec")
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
            let n = tag_nbt(rt, a(1));
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
        "writeBytes" if desc.starts_with("([B") => {
            let bytes = byte_array(rt, a(1));
            put(rt, b, &bytes);
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
        "readVarLong" => V::J(read_var(rt, b, 10).map_or(0, |v| v as i64)),
        "readUtf" | "readResourceLocation" => match read_utf(rt, b) {
            Some(s) => rt.string(&s),
            None => V::NULL,
        },
        "readBlockPos" => read_pos(rt, b).unwrap_or(V::NULL),
        "readUUID" => read_uuid(rt, b).unwrap_or(V::NULL),
        "readVec3" => read_vec3(rt, b).unwrap_or(V::NULL),
        "readNbt" | "readAnySizeNbt" => match read_nbt(rt, b) {
            Some(Some(n)) => nbt_tag(rt, &n),
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
        "retain" | "copy" | "slice" | "duplicate" | "asReadOnly" => b,
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
            _ => return None,
        })
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
}
