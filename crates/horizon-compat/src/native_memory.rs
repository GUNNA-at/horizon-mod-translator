//! Memory by its address: LWJGL's `MemoryUtil` on what the runtime gives an address of.
//!
//! A mod reaches an image's pixels by their address (`NativeImage.pixels`, a field of the
//! game's): it reads a pixel with `memGetInt(pixels + (x + y * width) * 4)`, copies an image
//! with `memCopy`, lays a buffer over the pixels (`memByteBuffer`), or allocates memory of its
//! own to work in (`nmemAlloc`, `memAlloc`). Measured over a pack of 367 mods: nine classes do
//! so with an image's pixels (two mods read their leaves' colours out of a texture, a map mod
//! copies a region's image before it writes it to disk, another keeps an animated texture's
//! frames), and eight jars name `MemoryUtil` at all. Unanswered, the map mod's copy stayed
//! empty and every tile it wrote was blank.
//!
//! An address here names an object of the runtime's heap in its upper half and a byte of that
//! object's memory in its lower ([`address`]): an open image (its RGBA bytes), a block a mod
//! allocated by address ([`BLOCK`], kept until it is freed), or the array under a byte buffer.
//! An address of anything else (the window system's memory, a buffer the game's renderer
//! gave) is not answered here, and the call stays one nothing maps. Values are read and
//! written in the machine's byte order, little-endian on every machine this runs on.
use crate::{Data, Rt, V};

const MEMORY_UTIL: &str = "org/lwjgl/system/MemoryUtil";

/// A block a mod allocated by its address (`nmemAlloc`): its bytes.
const BLOCK: &str = "compat/NativeMemory";

/// The blocks allocated and not yet freed. An address is no reference: without this list the
/// collector would take a block nothing else names.
const BLOCKS: &str = "compat:native-memory";

/// The largest block given: more is refused as `malloc` refuses it, with 0.
const LARGEST: i64 = 1 << 30;

/// Whether this module answers `owner.name desc`.
pub fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    owner == MEMORY_UTIL
}

/// The address of byte `at` of `memory`.
pub(crate) fn address(memory: V, at: i32) -> i64 {
    (i64::from(memory.r()) << 32) | i64::from(at.max(0))
}

/// The memory `address` points into, and how many bytes in; None for an address of nothing
/// held here (0 among them), and for an image that was closed.
fn place(rt: &Rt, address: i64) -> Option<(V, i32)> {
    let memory = V::R(u32::try_from(address >> 32).ok().filter(|&r| r != 0)?);
    let at = i32::try_from(address & 0xffff_ffff).ok()?;
    let o = rt.heap.get(memory.r() as usize)?;
    let held = match &o.data {
        Data::Bytes(..) => o.class == BLOCK || crate::resources::open_image(rt, memory),
        Data::Array(_) => o.class == "[B",
        _ => false,
    };
    held.then_some((memory, at))
}

/// The `n` bytes at `address`; None where they are not all memory held here.
fn read(rt: &Rt, address: i64, n: usize) -> Option<Vec<u8>> {
    let (memory, at) = place(rt, address)?;
    crate::nio::read(rt, memory, at, n)
}

/// Writes `bytes` at `address`; false where they would not all land in memory held here.
fn write(rt: &mut Rt, address: i64, bytes: &[u8]) -> bool {
    match place(rt, address) {
        Some((memory, at)) => crate::nio::write(rt, memory, at, bytes),
        None => false,
    }
}

/// A new block of `size` zero bytes and its address; 0 for a size no block is given for.
fn allocate(rt: &mut Rt, size: i64) -> i64 {
    if !(0..=LARGEST).contains(&size) {
        return 0;
    }
    // A block of no bytes still has an address of its own, as `malloc(0)` may give one.
    let block = rt.alloc(BLOCK, Data::Bytes(vec![0; size as usize], 0));
    let kept = rt.intern(BLOCKS.into(), |rt| crate::jutil::new_list(rt, Vec::new()));
    if let Some(Data::List(blocks)) = rt.heap.get_mut(kept.r() as usize).map(|o| &mut o.data) {
        blocks.push(block);
    }
    address(block, 0)
}

/// Frees the block at `address` (`nmemFree`): the address of its first byte, as `free` takes
/// it. Anything else is left alone.
fn free(rt: &mut Rt, address: i64) {
    let Some((block, 0)) = place(rt, address) else {
        return;
    };
    let Some(&kept) = rt.interned.get(BLOCKS) else {
        return;
    };
    if let Some(Data::List(blocks)) = rt.heap.get_mut(kept.r() as usize).map(|o| &mut o.data) {
        blocks.retain(|b| *b != block);
    }
}

/// How many bytes a block has, where `address` is one's first byte.
fn block_len(rt: &Rt, address: i64) -> Option<usize> {
    let Some((block, 0)) = place(rt, address) else {
        return None;
    };
    match rt.heap.get(block.r() as usize).map(|o| (o.class, &o.data)) {
        Some((BLOCK, Data::Bytes(bytes, _))) => Some(bytes.len()),
        _ => None,
    }
}

/// The kind of value a `memGet...`/`memPut...` moves, by the name after the verb.
fn kind_of(name: &str) -> Option<char> {
    Some(match name.get(6..)? {
        "Byte" => 'B',
        "Short" => 'S',
        "Int" => 'I',
        "Long" | "Address" => 'J',
        "Float" => 'F',
        "Double" => 'D',
        _ => return None,
    })
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if owner != MEMORY_UTIL {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (name, args.len()) {
        // A value at an address.
        (n, 1) if n.starts_with("memGet") && desc.starts_with("(J)") => {
            let kind = kind_of(n)?;
            let bytes = read(rt, a(0).j(), crate::nio::width(kind) as usize)?;
            crate::nio::decode(kind, &bytes, true)
        }
        (n, 2) if n.starts_with("memPut") && desc.starts_with("(J") => {
            let kind = kind_of(n)?;
            let bytes = crate::nio::encode(kind, a(1), true);
            write(rt, a(0).j(), &bytes).then_some(V::Void)?
        }
        // `memCopy(from, to, bytes)`; the two may lie in one piece of memory.
        ("memCopy", 3) if desc == "(JJJ)V" => {
            let n = usize::try_from(a(2).j()).ok()?;
            let bytes = read(rt, a(0).j(), n)?;
            write(rt, a(1).j(), &bytes).then_some(V::Void)?
        }
        // `memSet(at, value, bytes)`: each byte the value's low byte, as `memset` has it.
        ("memSet", 3) if desc == "(JIJ)V" => {
            let n = usize::try_from(a(2).j()).ok()?;
            let bytes = vec![a(1).i() as u8; n];
            write(rt, a(0).j(), &bytes).then_some(V::Void)?
        }
        // A buffer's remaining bytes filled.
        ("memSet", 2) if desc.starts_with("(Ljava/nio/ByteBuffer;I)") => {
            let (memory, offset, position, limit) = crate::nio::memory(rt, a(0))?;
            let bytes = vec![a(1).i() as u8; (limit - position).max(0) as usize];
            crate::nio::write(rt, memory, offset + position, &bytes).then_some(V::Void)?
        }
        // A buffer over the memory at an address.
        ("memByteBuffer", 2) if desc == "(JI)Ljava/nio/ByteBuffer;" => {
            let (memory, at) = place(rt, a(0).j())?;
            crate::nio::view(rt, memory, at, a(1).i().max(0))
        }
        // Blocks by their address. `calloc`'s is zeroed, and so is every block here.
        ("nmemAlloc" | "nmemAllocChecked", 1) => V::J(allocate(rt, a(0).j())),
        ("nmemCalloc" | "nmemCallocChecked", 2) => {
            V::J(allocate(rt, a(0).j().saturating_mul(a(1).j())))
        }
        // `nmemAlignedAlloc(alignment, size)`.
        ("nmemAlignedAlloc" | "nmemAlignedAllocChecked", 2) => V::J(allocate(rt, a(1).j())),
        // `nmemRealloc(block, size)`: a block of the new size with the old one's bytes, as
        // many as fit; the old one is freed. Of no block (0), a new one.
        ("nmemRealloc" | "nmemReallocChecked", 2) => {
            let (old, size) = (a(0).j(), a(1).j());
            if old == 0 {
                return Some(V::J(allocate(rt, size)));
            }
            let had = block_len(rt, old)?;
            let new = allocate(rt, size);
            if new != 0 {
                let kept = read(rt, old, had.min(size as usize)).unwrap_or_default();
                write(rt, new, &kept);
                free(rt, old);
            }
            V::J(new)
        }
        ("nmemFree" | "nmemAlignedFree", 1) => {
            free(rt, a(0).j());
            V::Void
        }
        // Buffers of LWJGL's making, in the machine's byte order. One is freed when nothing
        // holds it any more, as every object here is.
        ("memAlloc" | "memCalloc", 1) if desc == "(I)Ljava/nio/ByteBuffer;" => {
            crate::nio::allocate_native(rt, a(0).i().max(0))
        }
        ("memAlignedAlloc", 2) if desc == "(II)Ljava/nio/ByteBuffer;" => {
            crate::nio::allocate_native(rt, a(1).i().max(0))
        }
        // `memRealloc(buffer, size)`: a buffer of the new size with the old one's bytes, as
        // many as fit, at the old one's position (the new size where that is past it).
        ("memRealloc", 2) if desc.starts_with("(Ljava/nio/ByteBuffer;I)") => {
            let size = a(1).i().max(0);
            let new = crate::nio::allocate_native(rt, size);
            if let Some((memory, offset, position, _)) = crate::nio::memory(rt, a(0)) {
                let had = crate::nio::capacity(rt, a(0)).min(size);
                let kept = crate::nio::read(rt, memory, offset, had as usize).unwrap_or_default();
                let (to, to_offset, ..) = crate::nio::memory(rt, new)?;
                crate::nio::write(rt, to, to_offset, &kept);
                crate::nio::set_position(rt, new, position.min(size));
            }
            new
        }
        ("memFree" | "memAlignedFree", 1) => V::Void,
        // A buffer's address: of the byte at its position, of the byte at an index, or of its
        // first byte (`memAddress0`). A null buffer's is 0 where the call allows one.
        ("memAddress" | "memAddressSafe" | "memAddress0", n)
            if desc.starts_with("(Ljava/nio/ByteBuffer;")
                || desc.starts_with("(Ljava/nio/Buffer;") =>
        {
            if a(0).r() == 0 && name == "memAddressSafe" {
                return Some(V::J(0));
            }
            let (memory, offset, position, _) = crate::nio::memory(rt, a(0))?;
            let at = match (name, n) {
                ("memAddress0", _) => 0,
                (_, 2) => a(1).i(),
                _ => position,
            };
            V::J(address(memory, offset + at))
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMAGE: &str = "com/mojang/blaze3d/platform/NativeImage";

    fn new_rt() -> Rt {
        let rt = Rt::new();
        crate::install();
        rt
    }

    fn c(rt: &mut Rt, member: &str, args: &[V]) -> V {
        api(rt, &format!("{MEMORY_UTIL}.{member}"), args)
    }

    fn api(rt: &mut Rt, key: &str, args: &[V]) -> V {
        let key = rt.static_key(key);
        crate::api::call(rt, key, args)
    }

    /// A `width` by `height` image, and the address its `pixels` field reads as.
    fn image(rt: &mut Rt, width: i32, height: i32) -> (V, i64) {
        let image = rt.new_object(IMAGE);
        api(
            rt,
            &format!("{IMAGE}.<init>.(IIZ)V"),
            &[image, V::I(width), V::I(height), V::I(0)],
        );
        let pixels = api(rt, &format!("{IMAGE}.$pixels.()J"), &[image]).j();
        (image, pixels)
    }

    fn pixel(rt: &mut Rt, image: V, x: i32, y: i32) -> i32 {
        api(
            rt,
            &format!("{IMAGE}.getPixelRGBA.(II)I"),
            &[image, V::I(x), V::I(y)],
        )
        .i()
    }

    fn set_pixel(rt: &mut Rt, image: V, x: i32, y: i32, colour: i32) {
        api(
            rt,
            &format!("{IMAGE}.setPixelRGBA.(III)V"),
            &[image, V::I(x), V::I(y), V::I(colour)],
        );
    }

    /// A pixel read and written by its address is the pixel `getPixelRGBA` and `setPixelRGBA`
    /// see: vanilla's own two are `memGetInt(pixels + (x + y * width) * 4)` and `memPutInt` of
    /// the same place (NativeImage.getPixelRGBA, 1.21.1), which is how a mod reads a leaf
    /// texture's colours.
    #[test]
    fn a_pixel_by_its_address_is_the_images_pixel() {
        let mut rt = new_rt();
        let (image, pixels) = image(&mut rt, 4, 3);
        set_pixel(&mut rt, image, 2, 1, 0x7f33_2211);
        let at = pixels + (2 + 4) * 4;
        assert_eq!(c(&mut rt, "memGetInt.(J)I", &[V::J(at)]).i(), 0x7f33_2211);
        // Its bytes are R, G, B, A in memory: the low byte first.
        assert_eq!(c(&mut rt, "memGetByte.(J)B", &[V::J(at)]).i(), 0x11);
        assert_eq!(c(&mut rt, "memGetByte.(J)B", &[V::J(at + 3)]).i(), 0x7f);
        c(
            &mut rt,
            "memPutInt.(JI)V",
            &[V::J(pixels + 3 * 4), V::I(-2)],
        );
        assert_eq!(pixel(&mut rt, image, 3, 0), -2);
        c(&mut rt, "memPutByte.(JB)V", &[V::J(pixels), V::I(-1)]);
        assert_eq!(pixel(&mut rt, image, 0, 0), 0xff);
    }

    /// The copy a map mod writes to disk: a new image of the same size filled with
    /// `memCopy(from.pixels, to.pixels, width * height * 4)` holds every pixel of the first
    /// (JourneyMap's `ImageHolder.writeImageToFile`; every tile was blank while nothing
    /// answered the copy). A part of an image copies by an address inside it.
    #[test]
    fn a_copy_by_address_carries_an_images_pixels() {
        let mut rt = new_rt();
        let (from, from_pixels) = image(&mut rt, 8, 8);
        for i in 0..64 {
            set_pixel(&mut rt, from, i % 8, i / 8, 0x0100_0000 + i);
        }
        let (to, to_pixels) = image(&mut rt, 8, 8);
        c(
            &mut rt,
            "memCopy.(JJJ)V",
            &[V::J(from_pixels), V::J(to_pixels), V::J(8 * 8 * 4)],
        );
        for i in 0..64 {
            assert_eq!(
                pixel(&mut rt, to, i % 8, i / 8),
                0x0100_0000 + i,
                "pixel {i}"
            );
        }
        // The second row over the first, within the one image.
        c(
            &mut rt,
            "memCopy.(JJJ)V",
            &[V::J(to_pixels + 8 * 4), V::J(to_pixels), V::J(8 * 4)],
        );
        assert_eq!(pixel(&mut rt, to, 3, 0), 0x0100_0000 + 11);
        assert_eq!(pixel(&mut rt, to, 3, 1), 0x0100_0000 + 11);
    }

    /// A buffer laid over an image's pixels is those pixels: bytes put through it are in the
    /// image, and bytes got through it are the image's (JourneyMap's `LodRegionCache` moves a
    /// whole image in and out of a `byte[]` so). It is in the machine's byte order and has no
    /// array, as LWJGL's `memByteBuffer` gives it on a JVM.
    #[test]
    fn a_buffer_over_an_images_pixels_is_the_pixels() {
        let mut rt = new_rt();
        let (image, pixels) = image(&mut rt, 2, 2);
        let buffer = c(
            &mut rt,
            "memByteBuffer.(JI)Ljava/nio/ByteBuffer;",
            &[V::J(pixels), V::I(16)],
        );
        assert_eq!(
            api(&mut rt, "java/nio/ByteBuffer.capacity.()I", &[buffer]).i(),
            16
        );
        assert_eq!(
            api(&mut rt, "java/nio/ByteBuffer.isDirect.()Z", &[buffer]).i(),
            1
        );
        assert_eq!(
            api(&mut rt, "java/nio/ByteBuffer.hasArray.()Z", &[buffer]).i(),
            0
        );
        let order = api(
            &mut rt,
            "java/nio/ByteBuffer.order.()Ljava/nio/ByteOrder;",
            &[buffer],
        );
        assert!(crate::nio::is_little(&rt, order));
        let bytes: Vec<u8> = (1..=16).collect();
        let array = crate::resources::byte_array(&mut rt, &bytes);
        api(
            &mut rt,
            "java/nio/ByteBuffer.put.([B)Ljava/nio/ByteBuffer;",
            &[buffer, array],
        );
        assert_eq!(pixel(&mut rt, image, 0, 0), 0x0403_0201);
        assert_eq!(pixel(&mut rt, image, 1, 1), 0x100f_0e0d);
        // And back out: what the image holds now.
        set_pixel(&mut rt, image, 1, 0, 0x0a0b_0c0d);
        let read = c(
            &mut rt,
            "memByteBuffer.(JI)Ljava/nio/ByteBuffer;",
            &[V::J(pixels), V::I(16)],
        );
        let out = crate::resources::byte_array(&mut rt, &[0; 16]);
        api(
            &mut rt,
            "java/nio/ByteBuffer.get.([B)Ljava/nio/ByteBuffer;",
            &[read, out],
        );
        let out = crate::resources::array_bytes(&rt, out);
        assert_eq!(&out[..8], &[1, 2, 3, 4, 0x0d, 0x0c, 0x0b, 0x0a]);
        assert_eq!(
            api(&mut rt, "java/nio/ByteBuffer.getInt.(I)I", &[read, V::I(4)]).i(),
            0x0a0b_0c0d
        );
    }

    /// A block allocated by address holds what is put in it until it is freed, apart from every
    /// other block, zeroed where `calloc` made it; a grown block keeps its bytes. A closed
    /// image's address, a freed block's and 0 are memory of nothing: a call there is answered
    /// by nothing (the stand-in a missing call gets).
    #[test]
    fn a_block_by_address_is_memory_until_freed() {
        let mut rt = new_rt();
        let a = c(&mut rt, "nmemAlloc.(J)J", &[V::J(16)]).j();
        let b = c(&mut rt, "nmemCalloc.(JJ)J", &[V::J(4), V::J(8)]).j();
        assert!(a != 0 && b != 0 && a != b);
        c(
            &mut rt,
            "memPutLong.(JJ)V",
            &[V::J(a + 8), V::J(0x0102_0304_0506_0708)],
        );
        c(&mut rt, "memPutFloat.(JF)V", &[V::J(a), V::F(1.5)]);
        assert_eq!(
            c(&mut rt, "memGetLong.(J)J", &[V::J(a + 8)]).j(),
            0x0102_0304_0506_0708
        );
        assert_eq!(c(&mut rt, "memGetFloat.(J)F", &[V::J(a)]).f(), 1.5);
        assert_eq!(
            c(&mut rt, "memGetInt.(J)I", &[V::J(a + 8)]).i(),
            0x0506_0708
        );
        assert_eq!(c(&mut rt, "memGetLong.(J)J", &[V::J(b + 24)]).j(), 0);
        // `memSet(at, value, bytes)`: the value's low byte in each.
        c(&mut rt, "memSet.(JIJ)V", &[V::J(b), V::I(0x1ab), V::J(3)]);
        assert_eq!(c(&mut rt, "memGetInt.(J)I", &[V::J(b)]).i(), 0x00ab_abab);
        // Grown: the same bytes at a new address, and the old one is gone.
        let grown = c(&mut rt, "nmemRealloc.(JJ)J", &[V::J(a), V::J(64)]).j();
        assert_eq!(
            c(&mut rt, "memGetLong.(J)J", &[V::J(grown + 8)]).j(),
            0x0102_0304_0506_0708
        );
        assert_eq!(c(&mut rt, "memGetLong.(J)J", &[V::J(grown + 56)]).j(), 0);
        // Kept while allocated, whatever the collector does: an address is no reference.
        rt.collect();
        assert_eq!(c(&mut rt, "memGetInt.(J)I", &[V::J(b)]).i(), 0x00ab_abab);
        c(&mut rt, "nmemFree.(J)V", &[V::J(b)]);
        c(&mut rt, "nmemFree.(J)V", &[V::J(0)]);
        rt.collect();
        let missed = |rt: &mut Rt, address: i64| {
            call(
                rt,
                "org/lwjgl/system/MemoryUtil.memGetInt.(J)I",
                &[V::J(address)],
            )
        };
        assert_eq!(missed(&mut rt, b), None);
        assert_eq!(missed(&mut rt, 0), None);
        let (image, pixels) = image(&mut rt, 2, 2);
        assert!(missed(&mut rt, pixels).is_some());
        // Past its end is no memory of the image's either.
        assert_eq!(missed(&mut rt, pixels + 16), None);
        api(&mut rt, &format!("{IMAGE}.close.()V"), &[image]);
        assert_eq!(missed(&mut rt, pixels), None);
    }

    /// LWJGL's own buffers: `memAlloc` gives a direct one in the machine's byte order,
    /// `memAddress` the address of the byte at its position (or at an index), and what is
    /// written at that address is in the buffer. A sprite's pixels are flipped through such a
    /// buffer by one of the pack's mods (`memAlloc`, `memAddress`, `memCopy` to and from the
    /// image). The answers are LWJGL 3.3.3's on a JVM for the same calls, as are the byte
    /// orders, the fill and the grown block's bytes in the tests above.
    #[test]
    fn a_buffers_address_is_its_bytes() {
        let mut rt = new_rt();
        let buffer = c(&mut rt, "memAlloc.(I)Ljava/nio/ByteBuffer;", &[V::I(16)]);
        let order = api(
            &mut rt,
            "java/nio/ByteBuffer.order.()Ljava/nio/ByteOrder;",
            &[buffer],
        );
        assert!(crate::nio::is_little(&rt, order));
        assert_eq!(
            api(&mut rt, "java/nio/ByteBuffer.isDirect.()Z", &[buffer]).i(),
            1
        );
        assert_eq!(
            api(&mut rt, "java/nio/ByteBuffer.hasArray.()Z", &[buffer]).i(),
            0
        );
        let base = c(&mut rt, "memAddress.(Ljava/nio/ByteBuffer;)J", &[buffer]).j();
        api(
            &mut rt,
            "java/nio/ByteBuffer.position.(I)Ljava/nio/ByteBuffer;",
            &[buffer, V::I(4)],
        );
        let at = c(&mut rt, "memAddress.(Ljava/nio/ByteBuffer;)J", &[buffer]).j();
        assert_eq!(at - base, 4);
        assert_eq!(
            c(
                &mut rt,
                "memAddress.(Ljava/nio/ByteBuffer;I)J",
                &[buffer, V::I(12)]
            )
            .j() - base,
            12
        );
        assert_eq!(
            c(&mut rt, "memAddress0.(Ljava/nio/Buffer;)J", &[buffer]).j(),
            base
        );
        assert_eq!(
            c(
                &mut rt,
                "memAddressSafe.(Ljava/nio/ByteBuffer;)J",
                &[V::NULL]
            )
            .j(),
            0
        );
        c(&mut rt, "memPutInt.(JI)V", &[V::J(at), V::I(0x0102_0304)]);
        assert_eq!(
            api(
                &mut rt,
                "java/nio/ByteBuffer.getInt.(I)I",
                &[buffer, V::I(4)]
            )
            .i(),
            0x0102_0304
        );
        assert_eq!(
            api(&mut rt, "java/nio/ByteBuffer.get.(I)B", &[buffer, V::I(4)]).i(),
            4
        );
        // An image's row through the buffer and back to another place of the image.
        let (image, pixels) = image(&mut rt, 2, 2);
        set_pixel(&mut rt, image, 0, 0, 7);
        set_pixel(&mut rt, image, 1, 0, 9);
        c(
            &mut rt,
            "memCopy.(JJJ)V",
            &[V::J(pixels), V::J(base + 8), V::J(8)],
        );
        c(
            &mut rt,
            "memCopy.(JJJ)V",
            &[V::J(base + 8), V::J(pixels + 8), V::J(8)],
        );
        assert_eq!(
            (pixel(&mut rt, image, 0, 1), pixel(&mut rt, image, 1, 1)),
            (7, 9)
        );
        // The remaining bytes filled (`memSet(buffer, value)`): from the position on.
        c(
            &mut rt,
            "memSet.(Ljava/nio/ByteBuffer;I)V",
            &[buffer, V::I(0x55)],
        );
        assert_eq!(
            api(&mut rt, "java/nio/ByteBuffer.get.(I)B", &[buffer, V::I(3)]).i(),
            0
        );
        assert_eq!(
            api(&mut rt, "java/nio/ByteBuffer.get.(I)B", &[buffer, V::I(15)]).i(),
            0x55
        );
        // Grown: the old bytes, the old position.
        api(
            &mut rt,
            "java/nio/ByteBuffer.put.(IB)Ljava/nio/ByteBuffer;",
            &[buffer, V::I(0), V::I(0x21)],
        );
        let grown = c(
            &mut rt,
            "memRealloc.(Ljava/nio/ByteBuffer;I)Ljava/nio/ByteBuffer;",
            &[buffer, V::I(32)],
        );
        assert_eq!(
            api(&mut rt, "java/nio/ByteBuffer.capacity.()I", &[grown]).i(),
            32
        );
        assert_eq!(
            api(&mut rt, "java/nio/ByteBuffer.position.()I", &[grown]).i(),
            4
        );
        assert_eq!(
            api(&mut rt, "java/nio/ByteBuffer.position.()I", &[buffer]).i(),
            4
        );
        assert_eq!(
            api(&mut rt, "java/nio/ByteBuffer.get.(I)B", &[grown, V::I(0)]).i(),
            0x21
        );
        assert_eq!(
            api(&mut rt, "java/nio/ByteBuffer.get.(I)B", &[grown, V::I(31)]).i(),
            0
        );
    }
}
