//! Guava's `com.google.common.io`, as far as mods use it: `Files`' whole-file helpers, the
//! sinks and sources over a file (`asByteSink`, `asCharSource`...), a mod's own `ByteSource`
//! (whose `openStream` is the mod's code), and `ByteStreams`. Text is UTF-8, the charset mods
//! name. JourneyMap copies its themes out of its jar through these.
use crate::{Rt, V};

const FILES: &str = "com/google/common/io/Files";
const BYTE_SINK: &str = "com/google/common/io/ByteSink";
const BYTE_SOURCE: &str = "com/google/common/io/ByteSource";
const CHAR_SINK: &str = "com/google/common/io/CharSink";
const CHAR_SOURCE: &str = "com/google/common/io/CharSource";
const BYTE_STREAMS: &str = "com/google/common/io/ByteStreams";
const CHAR_STREAMS: &str = "com/google/common/io/CharStreams";
/// The file a sink or a source of this module is over, and whether a sink adds to it.
const FILE: &str = "$file";
const APPEND: &str = "$append";
const OPEN_STREAM: (&str, &str) = (
    "com/google/common/io/ByteSource.openStream.()Ljava/io/InputStream;",
    "openStream.()Ljava/io/InputStream;",
);

pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    matches!(
        owner,
        FILES | BYTE_SINK | BYTE_SOURCE | CHAR_SINK | CHAR_SOURCE | BYTE_STREAMS | CHAR_STREAMS
    )
}

/// A file's bytes, or the `IOException` Java throws for it (None then).
fn read(rt: &mut Rt, path: &str) -> Option<Vec<u8>> {
    let disk = crate::files::on_disk(rt, path);
    match std::fs::read(&disk) {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            crate::files::throw_io(rt, &e, path);
            None
        }
    }
}

/// The parameter types of a descriptor, each as it is written (`Ljava/io/File;`, `[B`, `I`).
fn params(desc: &str) -> Vec<&str> {
    let mut rest = desc
        .strip_prefix('(')
        .and_then(|d| d.split(')').next())
        .unwrap_or("");
    let mut out = Vec::new();
    while !rest.is_empty() {
        let dims = rest.bytes().take_while(|b| *b == b'[').count();
        let len = match rest.as_bytes().get(dims) {
            Some(b'L') => rest[dims..]
                .find(';')
                .map_or(rest.len(), |end| dims + end + 1),
            Some(_) => dims + 1,
            None => rest.len(),
        };
        out.push(&rest[..len]);
        rest = &rest[len..];
    }
    out
}

/// `bytes` written to a file, over what it held or after it.
fn write(rt: &mut Rt, path: &str, bytes: &[u8], append: bool) {
    use std::io::Write;
    let disk = crate::files::on_disk(rt, path);
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .append(append)
        .truncate(!append)
        .open(&disk)
        .and_then(|mut f| f.write_all(bytes));
    if let Err(e) = written {
        crate::files::throw_io(rt, &e, path);
    }
}

/// A sink or a source of `class` over `file`; `modes` (a sink's `FileWriteMode...`) names
/// `APPEND` or nothing.
fn over(rt: &mut Rt, class: &'static str, file: V, modes: V) -> V {
    let o = rt.new_object(class);
    let path = crate::jlang::text(rt, file);
    let path = rt.string(&path);
    rt.put_field(o, FILE, path);
    let append = !crate::jutil::elements(rt, modes).is_empty();
    rt.put_field(o, APPEND, V::I(i32::from(append)));
    o
}

/// What a source gives: its file's bytes, or what the stream a mod's own source opens holds.
fn source_bytes(rt: &mut Rt, source: V) -> Option<Vec<u8>> {
    match rt.str_of(rt.get_field(source, FILE)).map(str::to_owned) {
        Some(path) => read(rt, &path),
        None => {
            let stream = crate::api::virtual_call(rt, OPEN_STREAM, &[source]);
            if rt.thrown.is_some() {
                return None;
            }
            let bytes = crate::streams::drain(rt, stream).unwrap_or_default();
            crate::api::call(rt, "java/io/InputStream.close.()V", &[stream]);
            Some(bytes)
        }
    }
}

fn sink_write(rt: &mut Rt, sink: V, bytes: &[u8]) {
    if let Some(path) = rt.str_of(rt.get_field(sink, FILE)).map(str::to_owned) {
        let append = rt.get_field(sink, APPEND).i() != 0;
        write(rt, &path, bytes, append);
    }
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    // Guava checks what it is handed (`checkNotNull`): a null file, or a null text to write, is
    // the caller's error and touches no file. (Read as the text "null", a null file was a file
    // of that name: the translator, asking each answer with nothing in hand, left one behind.)
    let files = owner == FILES
        && params(desc)
            .iter()
            .enumerate()
            .any(|(i, kind)| *kind == "Ljava/io/File;" && a(i).is_null());
    let written = matches!(name, "write" | "append") && owner == FILES && a(0).is_null();
    if files || written {
        rt.throw_new("java/lang/NullPointerException");
        return Some(V::NULL);
    }
    let text = |rt: &Rt, i: usize| crate::jlang::text(rt, a(i));
    let string = |rt: &mut Rt, bytes: &[u8]| rt.string(&String::from_utf8_lossy(bytes));
    Some(match (owner, name) {
        (FILES, "asByteSink") => over(rt, BYTE_SINK, a(0), a(1)),
        (FILES, "asCharSink") => over(rt, CHAR_SINK, a(0), a(2)),
        (FILES, "asByteSource") => over(rt, BYTE_SOURCE, a(0), V::NULL),
        (FILES, "asCharSource") => over(rt, CHAR_SOURCE, a(0), V::NULL),
        (FILES, "createParentDirs") => {
            let path = text(rt, 0);
            let disk = crate::files::on_disk(rt, &path);
            if let Some(parent) = disk.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                crate::files::throw_io(rt, &e, &path);
            }
            V::Void
        }
        (FILES, "toByteArray") => {
            let bytes = read(rt, &text(rt, 0))?;
            crate::resources::byte_array(rt, &bytes)
        }
        (FILES, "toString") => {
            let bytes = read(rt, &text(rt, 0))?;
            string(rt, &bytes)
        }
        (FILES, "readLines") => {
            let bytes = read(rt, &text(rt, 0))?;
            let lines: Vec<V> = String::from_utf8_lossy(&bytes)
                .lines()
                .map(|l| rt.string(l))
                .collect();
            crate::jutil::new_list(rt, lines)
        }
        // `write(bytes, file)`, `write(text, file, charset)`, `append(text, file, charset)`.
        (FILES, "write" | "append") => {
            let bytes = if desc.starts_with("([B") {
                crate::resources::array_bytes(rt, a(0))
            } else {
                text(rt, 0).into_bytes()
            };
            write(rt, &text(rt, 1), &bytes, name == "append");
            V::Void
        }
        // `copy(from, to)` to a file, or `copy(from, stream)`.
        (FILES, "copy") => {
            let bytes = read(rt, &text(rt, 0))?;
            if desc.contains("Ljava/io/OutputStream;") {
                crate::streams::write_to(rt, a(1), &bytes);
            } else {
                write(rt, &text(rt, 1), &bytes, false);
            }
            V::Void
        }
        (FILES, "move") => {
            let (from, to) = (text(rt, 0), text(rt, 1));
            let (from_disk, to_disk) = (
                crate::files::on_disk(rt, &from),
                crate::files::on_disk(rt, &to),
            );
            if let Err(e) = std::fs::rename(&from_disk, &to_disk) {
                crate::files::throw_io(rt, &e, &from);
            }
            V::Void
        }
        (FILES, "touch") => {
            write(rt, &text(rt, 0), &[], true);
            V::Void
        }
        // A file name's extension (after its last dot, none for none) and what is before it.
        (FILES, "getFileExtension" | "getNameWithoutExtension") => {
            let path = text(rt, 0);
            let file = path.rsplit('/').next().unwrap_or("");
            let (stem, extension) = match file.rfind('.') {
                Some(dot) => (&file[..dot], &file[dot + 1..]),
                None => (file, ""),
            };
            rt.string(if name == "getFileExtension" {
                extension
            } else {
                stem
            })
        }
        (FILES, "newReader") => {
            let bytes = read(rt, &text(rt, 0))?;
            let stream = crate::resources::input_stream(rt, bytes);
            let reader = rt.new_object("java/io/BufferedReader");
            let inner = rt.new_object("java/io/InputStreamReader");
            crate::api::call(
                rt,
                "java/io/InputStreamReader.<init>.(Ljava/io/InputStream;)V",
                &[inner, stream],
            );
            crate::api::call(
                rt,
                "java/io/BufferedReader.<init>.(Ljava/io/Reader;)V",
                &[reader, inner],
            );
            reader
        }
        (FILES, "newWriter") => {
            let writer = rt.new_object("java/io/BufferedWriter");
            let inner = rt.new_object("java/io/FileWriter");
            crate::api::call(
                rt,
                "java/io/FileWriter.<init>.(Ljava/io/File;)V",
                &[inner, a(0)],
            );
            crate::api::call(
                rt,
                "java/io/BufferedWriter.<init>.(Ljava/io/Writer;)V",
                &[writer, inner],
            );
            writer
        }
        // A mod's own source or sink: nothing of this module's to set up.
        (BYTE_SOURCE | BYTE_SINK | CHAR_SOURCE | CHAR_SINK, "<init>") => V::Void,
        (BYTE_SOURCE, "openStream" | "openBufferedStream") => {
            let bytes = source_bytes(rt, a(0))?;
            crate::resources::input_stream(rt, bytes)
        }
        (BYTE_SOURCE, "read") => {
            let bytes = source_bytes(rt, a(0))?;
            crate::resources::byte_array(rt, &bytes)
        }
        (BYTE_SOURCE, "size") => V::J(source_bytes(rt, a(0))?.len() as i64),
        (BYTE_SOURCE, "isEmpty") => V::I(i32::from(source_bytes(rt, a(0))?.is_empty())),
        // `copyTo(stream)` or `copyTo(sink)`: how many bytes went.
        (BYTE_SOURCE, "copyTo") => {
            let bytes = source_bytes(rt, a(0))?;
            if desc.contains("Ljava/io/OutputStream;") {
                crate::streams::write_to(rt, a(1), &bytes);
            } else {
                sink_write(rt, a(1), &bytes);
            }
            V::J(bytes.len() as i64)
        }
        (BYTE_SINK, "write") => {
            let bytes = crate::resources::array_bytes(rt, a(1));
            sink_write(rt, a(0), &bytes);
            V::Void
        }
        (BYTE_SINK, "writeFrom") => {
            let bytes = crate::streams::drain(rt, a(1)).unwrap_or_default();
            sink_write(rt, a(0), &bytes);
            V::J(bytes.len() as i64)
        }
        (BYTE_SINK, "openStream" | "openBufferedStream") => {
            let path = rt.get_field(a(0), FILE);
            let out = rt.new_object("java/io/FileOutputStream");
            let append = V::I(rt.get_field(a(0), APPEND).i());
            crate::api::call(
                rt,
                "java/io/FileOutputStream.<init>.(Ljava/lang/String;Z)V",
                &[out, path, append],
            );
            out
        }
        (CHAR_SINK, "write") => {
            let text = text(rt, 1);
            sink_write(rt, a(0), text.as_bytes());
            V::Void
        }
        (CHAR_SINK, "writeLines") => {
            let lines: Vec<String> = crate::jutil::elements(rt, a(1))
                .into_iter()
                .map(|l| crate::jlang::text(rt, l))
                .collect();
            let all: String = lines.iter().map(|l| format!("{l}\n")).collect();
            sink_write(rt, a(0), all.as_bytes());
            V::Void
        }
        (CHAR_SOURCE, "read") => {
            let bytes = source_bytes(rt, a(0))?;
            string(rt, &bytes)
        }
        (CHAR_SOURCE, "readLines") => {
            let bytes = source_bytes(rt, a(0))?;
            let lines: Vec<V> = String::from_utf8_lossy(&bytes)
                .lines()
                .map(|l| rt.string(l))
                .collect();
            crate::jutil::new_list(rt, lines)
        }
        (CHAR_SOURCE, "readFirstLine") => {
            let bytes = source_bytes(rt, a(0))?;
            match String::from_utf8_lossy(&bytes).lines().next() {
                Some(line) => rt.string(line),
                None => V::NULL,
            }
        }
        (BYTE_STREAMS, "copy")
            if desc.starts_with("(Ljava/io/InputStream;Ljava/io/OutputStream;)") =>
        {
            let bytes = crate::streams::drain(rt, a(0)).unwrap_or_default();
            crate::streams::write_to(rt, a(1), &bytes);
            V::J(bytes.len() as i64)
        }
        (BYTE_STREAMS, "toByteArray") => {
            let bytes = crate::streams::drain(rt, a(0)).unwrap_or_default();
            crate::resources::byte_array(rt, &bytes)
        }
        (BYTE_STREAMS, "exhaust") => {
            V::J(crate::streams::drain(rt, a(0)).unwrap_or_default().len() as i64)
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        crate::api::call(rt, key, args)
    }

    /// A null file or a null text is the caller's error, and no file comes of it: asked with
    /// nothing in hand (as the translator asks every answer once), `Files.write` left a file
    /// named `null` holding "null" where it ran.
    #[test]
    fn a_null_file_writes_nothing() {
        crate::install();
        let dir = std::env::temp_dir().join(format!("hz-guava-null-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut rt = Rt::new();
        let (text, file) = (
            rt.string("kept"),
            rt.string(&dir.join("kept.txt").to_string_lossy()),
        );
        for (key, args) in [
            (
                "com/google/common/io/Files.write.(Ljava/lang/CharSequence;Ljava/io/File;Ljava/nio/charset/Charset;)V",
                [V::NULL, V::NULL, V::NULL],
            ),
            (
                "com/google/common/io/Files.write.(Ljava/lang/CharSequence;Ljava/io/File;Ljava/nio/charset/Charset;)V",
                [text, V::NULL, V::NULL],
            ),
            (
                "com/google/common/io/Files.write.(Ljava/lang/CharSequence;Ljava/io/File;Ljava/nio/charset/Charset;)V",
                [V::NULL, file, V::NULL],
            ),
            (
                "com/google/common/io/Files.touch.(Ljava/io/File;)V",
                [V::NULL, V::NULL, V::NULL],
            ),
        ] {
            let n = if key.contains("touch") { 1 } else { 3 };
            crate::api::call(&mut rt, key, &args[..n]);
            let thrown = rt.thrown.take().expect(key);
            assert_eq!(rt.class_of(thrown), "java/lang/NullPointerException");
        }
        assert!(!std::path::Path::new("null").exists() && !dir.join("kept.txt").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Guava's file helpers as JourneyMap uses them to copy a file out of its jar: the parent
    /// folders made, a stream written through a sink, text written and read back, a name's
    /// extension.
    #[test]
    fn guavas_sinks_and_sources_write_and_read_files() {
        let dir = std::env::temp_dir().join(format!("hz-guava-io-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut rt = Rt::new();
        crate::install();
        let target = dir.join("icon/theme/flat/icon.png");
        let path = rt.string(target.to_str().unwrap());
        let file = rt.new_object("java/io/File");
        c(
            &mut rt,
            "java/io/File.<init>.(Ljava/lang/String;)V",
            &[file, path],
        );
        c(
            &mut rt,
            "com/google/common/io/Files.createParentDirs.(Ljava/io/File;)V",
            &[file],
        );
        assert!(target.parent().unwrap().is_dir());
        let modes = rt.alloc(
            "[Lcom/google/common/io/FileWriteMode;",
            crate::Data::Array(Vec::new()),
        );
        let sink = c(
            &mut rt,
            "com/google/common/io/Files.asByteSink.(Ljava/io/File;[Lcom/google/common/io/FileWriteMode;)Lcom/google/common/io/ByteSink;",
            &[file, modes],
        );
        let stream = crate::resources::input_stream(&mut rt, b"png bytes".to_vec());
        assert_eq!(
            c(
                &mut rt,
                "com/google/common/io/ByteSink.writeFrom.(Ljava/io/InputStream;)J",
                &[sink, stream]
            ),
            V::J(9)
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"png bytes");

        let notes = dir.join("notes.txt");
        let notes_path = rt.string(notes.to_str().unwrap());
        let notes_file = rt.new_object("java/io/File");
        c(
            &mut rt,
            "java/io/File.<init>.(Ljava/lang/String;)V",
            &[notes_file, notes_path],
        );
        let utf8 = crate::api::get_static(&mut rt, "java/nio/charset/StandardCharsets.UTF_8");
        let chars = c(
            &mut rt,
            "com/google/common/io/Files.asCharSink.(Ljava/io/File;Ljava/nio/charset/Charset;[Lcom/google/common/io/FileWriteMode;)Lcom/google/common/io/CharSink;",
            &[notes_file, utf8, modes],
        );
        let line = rt.string("caf\u{e9}\nau lait");
        c(
            &mut rt,
            "com/google/common/io/CharSink.write.(Ljava/lang/CharSequence;)V",
            &[chars, line],
        );
        let back = c(
            &mut rt,
            "com/google/common/io/Files.toString.(Ljava/io/File;Ljava/nio/charset/Charset;)Ljava/lang/String;",
            &[notes_file, utf8],
        );
        assert_eq!(rt.str_of(back), Some("caf\u{e9}\nau lait"));
        let lines = c(
            &mut rt,
            "com/google/common/io/Files.readLines.(Ljava/io/File;Ljava/nio/charset/Charset;)Ljava/util/List;",
            &[notes_file, utf8],
        );
        assert_eq!(crate::jutil::elements(&rt, lines).len(), 2);
        let name = rt.string("maps/theme.flat.json");
        let extension = c(
            &mut rt,
            "com/google/common/io/Files.getFileExtension.(Ljava/lang/String;)Ljava/lang/String;",
            &[name],
        );
        let stem = c(
            &mut rt,
            "com/google/common/io/Files.getNameWithoutExtension.(Ljava/lang/String;)Ljava/lang/String;",
            &[name],
        );
        assert_eq!(
            (rt.str_of(extension), rt.str_of(stem)),
            (Some("json"), Some("theme.flat"))
        );
        assert!(
            rt.thrown.is_none() && rt.unmapped.is_empty(),
            "{:?}",
            rt.unmapped
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
