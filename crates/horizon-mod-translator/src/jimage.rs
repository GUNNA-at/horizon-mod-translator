//! A Java runtime's own classes, read from its image (`lib/modules`, where every runtime since
//! Java 9 keeps them): the player's copy of the JDK's code, which the translator takes as a
//! library where a class is plain code ([`crate::jdk_code`]), as it takes the game's own from
//! their NeoForge install.
//!
//! The image is a table of resources by module, package and name, each stored whole or packed
//! (the launchers' runtimes pack with zlib). Only the image of a runtime built for a
//! little-endian machine is read: every one a launcher installs.
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const MAGIC: u32 = 0xCAFE_DADA;
/// Before a packed resource: this, its sizes packed and whole, and its packer's name.
const PACKED: u32 = 0xCAFE_FAFA;
const PACKED_HEADER: usize = 29;

/// A resource's attributes: which string or number follows ([`Image::open`]).
const MODULE: usize = 1;
const PARENT: usize = 2;
const BASE: usize = 3;
const EXTENSION: usize = 4;
const OFFSET: usize = 5;
const COMPRESSED: usize = 6;
const UNCOMPRESSED: usize = 7;

pub struct Image {
    file: File,
    /// Where the resources start in the file.
    start: u64,
    strings: Vec<u8>,
    /// Each class by name (`java/awt/Color`): where its bytes are, how many as stored (none: not
    /// packed) and how many whole.
    classes: Vec<(String, u64, u64, u64)>,
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// The string at `at` of the image's strings: bytes up to a zero.
fn string(strings: &[u8], at: u64) -> Option<&str> {
    let s = strings.get(usize::try_from(at).ok()?..)?;
    std::str::from_utf8(&s[..s.iter().position(|&b| b == 0)?]).ok()
}

impl Image {
    pub fn open(path: &Path) -> Result<Image, String> {
        let bad = |what: &str| format!("{}: {what}", path.display());
        let mut file = File::open(path).map_err(|e| bad(&e.to_string()))?;
        let mut header = [0u8; 28];
        file.read_exact(&mut header).map_err(|e| bad(&e.to_string()))?;
        let word = |i: usize| u32_at(&header, i * 4).unwrap_or(0) as usize;
        if word(0) != MAGIC as usize {
            return Err(bad("not a Java runtime image of a little-endian machine"));
        }
        // After the header: a redirect table and the resources' offsets into the locations (a
        // word each per table entry), the locations, then the strings.
        let (entries, locations_size, strings_size) = (word(4), word(5), word(6));
        let mut tables = vec![0u8; entries * 8 + locations_size + strings_size];
        file.read_exact(&mut tables).map_err(|e| bad(&e.to_string()))?;
        let (offsets, rest) = tables[entries * 4..].split_at(entries * 4);
        let (locations, strings) = rest.split_at(locations_size);
        let mut classes = Vec::new();
        for i in 0..entries {
            // A location: attributes until a zero byte, each a byte of its kind (the high five
            // bits) and length (the low three, less one), then its value, big-endian.
            let mut at = u32_at(offsets, i * 4).unwrap_or(0) as usize;
            let mut attrs = [0u64; 8];
            while let Some(&b) = locations.get(at) {
                let (kind, len) = (usize::from(b >> 3), usize::from(b & 7) + 1);
                if kind == 0 || kind >= attrs.len() {
                    break;
                }
                let Some(value) = locations.get(at + 1..at + 1 + len) else {
                    break;
                };
                attrs[kind] = value.iter().fold(0, |v, &b| v << 8 | u64::from(b));
                at += 1 + len;
            }
            let text = |kind: usize| string(strings, attrs[kind]).unwrap_or("");
            if text(EXTENSION) != "class" || text(PARENT).is_empty() || text(MODULE).is_empty() {
                continue;
            }
            classes.push((
                format!("{}/{}", text(PARENT), text(BASE)),
                attrs[OFFSET],
                attrs[COMPRESSED],
                attrs[UNCOMPRESSED],
            ));
        }
        classes.sort();
        classes.dedup_by(|a, b| a.0 == b.0);
        let start = (header.len() + tables.len()) as u64;
        let strings = strings.to_vec();
        Ok(Image {
            file,
            start,
            strings,
            classes,
        })
    }

    /// The names of the image's classes (`java/awt/Color`), in order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.classes.iter().map(|c| c.0.as_str())
    }

    /// The Java version of the runtime (21, 25): its classes' file version, less 44.
    pub fn java_version(&mut self) -> Option<u16> {
        let object = self.class("java/lang/Object")?;
        Some(u16::from_be_bytes(object.get(6..8)?.try_into().ok()?).saturating_sub(44))
    }

    /// A class's file, unpacked. None when the image has none of the name, or packs it some way
    /// other than zlib.
    pub fn class(&mut self, name: &str) -> Option<Vec<u8>> {
        let i = self
            .classes
            .binary_search_by(|c| c.0.as_str().cmp(name))
            .ok()?;
        let (_, offset, packed, size) = self.classes[i];
        let stored = usize::try_from(if packed == 0 { size } else { packed }).ok()?;
        let mut data = vec![0u8; stored];
        self.file.seek(SeekFrom::Start(self.start + offset)).ok()?;
        self.file.read_exact(&mut data).ok()?;
        // Packed once per packer the image was built with, the last one outermost.
        while u32_at(&data, 0) == Some(PACKED) {
            let (len, packer) = (u64_at(&data, 4)?, u32_at(&data, 20)?);
            if string(&self.strings, u64::from(packer))? != "zip" {
                return None;
            }
            let body = data.get(PACKED_HEADER..PACKED_HEADER + usize::try_from(len).ok()?)?;
            data = miniz_oxide::inflate::decompress_to_vec_zlib(body).ok()?;
        }
        (data.len() as u64 == size).then_some(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An image of two resources, one stored whole and one packed with zlib, as `jlink` lays them
    /// out.
    #[test]
    fn an_images_classes_are_read_whole_or_packed() {
        let mut strings = vec![0u8];
        let mut intern = |s: &str| {
            let at = strings.len() as u64;
            strings.extend(s.as_bytes());
            strings.push(0);
            at
        };
        let (module, parent, ext, zip) = (intern("java.desktop"), intern("java/awt"), intern("class"), intern("zip"));
        let (color, point, info) = (intern("Color"), intern("Point"), intern("module-info"));
        let whole = b"\xca\xfe\xba\xbe color".to_vec();
        let plain = b"\xca\xfe\xba\xbe point point point point".to_vec();
        let deflated = miniz_oxide::deflate::compress_to_vec_zlib(&plain, 6);
        let mut packed = PACKED.to_le_bytes().to_vec();
        packed.extend((deflated.len() as u64).to_le_bytes());
        packed.extend((plain.len() as u64).to_le_bytes());
        packed.extend((zip as u32).to_le_bytes());
        packed.extend([0u8; 5]);
        packed.extend(&deflated);
        let mut locations = Vec::new();
        let mut location = |attrs: &[(usize, u64)]| {
            let at = locations.len() as u32;
            for &(kind, v) in attrs {
                locations.push((kind as u8) << 3 | 3);
                locations.extend((v as u32).to_be_bytes());
            }
            locations.push(0);
            at
        };
        let offsets = [
            location(&[(MODULE, module), (PARENT, parent), (BASE, color), (EXTENSION, ext), (UNCOMPRESSED, whole.len() as u64)]),
            location(&[
                (MODULE, module),
                (PARENT, parent),
                (BASE, point),
                (EXTENSION, ext),
                (OFFSET, whole.len() as u64),
                (COMPRESSED, packed.len() as u64),
                (UNCOMPRESSED, plain.len() as u64),
            ]),
            // A module's descriptor is no class of a package.
            location(&[(MODULE, module), (BASE, info), (EXTENSION, ext)]),
        ];
        let mut image = Vec::new();
        for word in [MAGIC, 1 << 16, 0, 3, 3, locations.len() as u32, strings.len() as u32] {
            image.extend(word.to_le_bytes());
        }
        image.extend([0u8; 12]);
        for o in offsets {
            image.extend(o.to_le_bytes());
        }
        image.extend(&locations);
        image.extend(&strings);
        image.extend(&whole);
        image.extend(&packed);
        let path = std::env::temp_dir().join(format!("horizon-jimage-{}", std::process::id()));
        std::fs::write(&path, &image).unwrap();
        let mut read = Image::open(&path).unwrap();
        assert_eq!(read.names().collect::<Vec<_>>(), ["java/awt/Color", "java/awt/Point"]);
        assert_eq!(read.class("java/awt/Color"), Some(whole));
        assert_eq!(read.class("java/awt/Point"), Some(plain));
        assert_eq!(read.class("java/awt/Toolkit"), None);
        std::fs::write(&path, b"PK\x03\x04 a jar, not an image, of some length").unwrap();
        assert!(Image::open(&path).is_err());
        std::fs::remove_file(&path).unwrap();
    }
}
