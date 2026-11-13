//! `java.io`'s writers of text: `Writer`, `PrintWriter`, `FileWriter`, `BufferedWriter`,
//! `OutputStreamWriter`, `StringWriter`. What a mod writes goes straight to where the writer
//! leads (a file, a stream, another writer, a string kept), with no buffer of the writer's own:
//! a config or a log a mod forgot to close is whole on disk all the same.
use crate::{Rt, V};

const PRINT: &str = "java/io/PrintWriter";
const FILE: &str = "java/io/FileWriter";
const BUFFERED: &str = "java/io/BufferedWriter";
const STREAM: &str = "java/io/OutputStreamWriter";
const STRING: &str = "java/io/StringWriter";
const CHARS: &str = "java/io/CharArrayWriter";

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key
/// only to the modules that claim it ([`crate::api`]'s chain).
pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    matches!(owner, "java/io/Writer" | PRINT | FILE | BUFFERED | STREAM | STRING | CHARS)
}

/// A descriptor's parameter types (`Ljava/lang/String;`, `I`, `[C`), in order.
fn params(desc: &str) -> Vec<&str> {
    let list = desc.strip_prefix('(').and_then(|d| d.split(')').next()).unwrap_or("");
    let (mut out, mut at) = (Vec::new(), 0);
    while at < list.len() {
        let rest = &list[at..];
        let dims = rest.len() - rest.trim_start_matches('[').len();
        let len = dims + if rest[dims..].starts_with('L') { rest[dims..].find(';').map_or(rest.len() - dims, |i| i + 1) } else { 1 };
        out.push(&rest[..len]);
        at += len;
    }
    out
}

/// A writer of class `class` onto the file at `path` (its text), made empty now unless it is
/// appended to, as `new FileWriter(path)` and `Files.newBufferedWriter(path)` make theirs; the
/// exception thrown where it can't be.
pub(crate) fn to_file(rt: &mut Rt, class: &'static str, path: &str, append: bool) -> V {
    let w = rt.new_object(class);
    open_file(rt, w, path, append);
    w
}

fn open_file(rt: &mut Rt, w: V, path: &str, append: bool) {
    let disk = crate::files::on_disk(rt, path);
    match std::fs::OpenOptions::new().write(true).create(true).append(append).truncate(!append).open(&disk) {
        Ok(_) => {
            let p = rt.string(path);
            rt.put_field(w, "$path", p);
        }
        Err(e) => {
            crate::files::throw_io(rt, &e, path);
        }
    }
}

/// `text` written to writer `w`: on to its file, its stream or the writer it wraps, or kept (a
/// `StringWriter`'s own text).
fn put(rt: &mut Rt, w: V, text: &str) {
    use std::io::Write;
    if text.is_empty() {
        return;
    }
    if let Some(path) = rt.str_of(rt.get_field(w, "$path")).map(str::to_owned) {
        let disk = crate::files::on_disk(rt, &path);
        let written = std::fs::OpenOptions::new().append(true).open(&disk).and_then(|mut f| f.write_all(text.as_bytes()));
        if let Err(e) = written {
            crate::files::throw_io(rt, &e, &path);
        }
        return;
    }
    let sink = rt.get_field(w, "$sink");
    if sink.r() == 0 {
        // The writer's own text, which nothing else holds, added to where it lies: a new
        // string of it all for each piece written made a megabyte of JSON written in small
        // pieces gigabytes of copies, each one garbage at the next.
        let held = rt.get_field(w, "$text");
        match rt
            .heap
            .get_mut(held.r() as usize)
            .filter(|_| held.r() != 0)
            .map(|o| &mut o.data)
        {
            Some(crate::Data::Str(all)) => all.push_str(text),
            _ => {
                let all = rt.alloc("java/lang/String", crate::Data::Str(text.to_owned()));
                rt.put_field(w, "$text", all);
            }
        }
    } else if rt.get_field(w, "$bytes").i() != 0 {
        let bytes = crate::files::byte_array(rt, text.as_bytes());
        crate::api::call(rt, "java/io/OutputStream.write.([B)V", &[sink, bytes]);
    } else {
        let s = rt.string(text);
        crate::api::call(rt, "java/io/Writer.write.(Ljava/lang/String;)V", &[sink, s]);
    }
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let types = params(desc);
    // An argument's text, by its type: a `char[]`, a `char`, a number, a boolean, an object.
    let text_of = |rt: &mut Rt, i: usize| match types.get(i - 1).copied().unwrap_or("L") {
        "[C" => String::from_utf16_lossy(&crate::jutil::elements(rt, a(i)).iter().map(|c| c.i() as u16).collect::<Vec<_>>()),
        t if t.len() == 1 => rt.to_java_string(a(i), t.chars().next().unwrap_or('I')),
        _ => crate::jlang::text(rt, a(i)),
    };
    // The part `(offset, count)` names of a text, in UTF-16 units as Java counts them.
    let part = |s: String, offset: i32, count: i32| {
        let u: Vec<u16> = s.encode_utf16().collect();
        let from = (offset.max(0) as usize).min(u.len());
        String::from_utf16_lossy(&u[from..(from + count.max(0) as usize).min(u.len())])
    };
    Some(match name {
        "<init>" => {
            match (owner, types.first().copied()) {
                (STRING | CHARS, _) | (_, None) => {}
                // Onto a file, by its name or its `File`; `(…, true)` appends.
                (_, Some("Ljava/lang/String;" | "Ljava/io/File;")) => {
                    let path = crate::jlang::text(rt, a(1));
                    let append = types.get(1) == Some(&"Z") && a(2).i() != 0;
                    open_file(rt, this, &path, append);
                }
                // Around a stream (its bytes are the text's UTF-8) or another writer.
                (_, Some(first)) => {
                    rt.put_field(this, "$sink", a(1));
                    rt.put_field(this, "$bytes", V::I((first == "Ljava/io/OutputStream;") as i32));
                }
            }
            V::Void
        }
        "write" => {
            let text = match types.as_slice() {
                // `write(int)`: one character.
                ["I"] => char::from_u32(a(1).i() as u32 & 0xffff).map(String::from).unwrap_or_default(),
                [_, "I", "I"] => part(text_of(rt, 1), a(2).i(), a(3).i()),
                _ => text_of(rt, 1),
            };
            put(rt, this, &text);
            V::Void
        }
        "append" => {
            let text = match types.as_slice() {
                ["C"] => char::from_u32(a(1).i() as u32 & 0xffff).map(String::from).unwrap_or_default(),
                // `append(text, start, end)`.
                [_, "I", "I"] => part(text_of(rt, 1), a(2).i(), a(3).i() - a(2).i()),
                _ if a(1).r() == 0 => "null".to_owned(),
                _ => text_of(rt, 1),
            };
            put(rt, this, &text);
            this
        }
        "print" | "println" => {
            let mut text = if types.is_empty() { String::new() } else { text_of(rt, 1) };
            if name == "println" {
                text.push('\n');
            }
            put(rt, this, &text);
            V::Void
        }
        "printf" | "format" => {
            let (f, rest) = if types.first() == Some(&"Ljava/util/Locale;") { (a(2), a(3)) } else { (a(1), a(2)) };
            let (f, vals) = (crate::jlang::text(rt, f), crate::jutil::elements(rt, rest));
            let vals = crate::jlang::format_values(rt, vals);
            let text = crate::jlang::format_java(rt, &f, &vals);
            put(rt, this, &text);
            this
        }
        "newLine" => {
            put(rt, this, "\n");
            V::Void
        }
        // Nothing is held back here: the stream or the writer below is told all the same.
        "flush" | "close" => {
            let sink = rt.get_field(this, "$sink");
            if sink.r() != 0 {
                let below = if rt.get_field(this, "$bytes").i() != 0 { "java/io/OutputStream" } else { "java/io/Writer" };
                let key = rt.static_key(&format!("{below}.{name}.()V"));
                crate::api::call(rt, key, &[sink]);
            }
            V::Void
        }
        "checkError" => V::I(0),
        "toString" if matches!(rt.class_of(this), STRING | CHARS) => {
            let s = rt.str_of(rt.get_field(this, "$text")).unwrap_or("").to_owned();
            rt.string(&s)
        }
        "getBuffer" => {
            let b = rt.new_object("java/lang/StringBuffer");
            let s = rt.get_field(this, "$text");
            let s = if s.r() == 0 { rt.string("") } else { s };
            crate::api::call(rt, "java/lang/StringBuffer.<init>.(Ljava/lang/String;)V", &[b, s]);
            b
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

    /// A config written the way mods write theirs: a `PrintWriter` over a `BufferedWriter` over a
    /// `FileWriter`, then lines appended by a second writer, and a `StringWriter` read back.
    #[test]
    fn writers_put_their_text_where_they_lead() {
        let dir = std::env::temp_dir().join(format!("hz-writers-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("mod.cfg");
        let mut rt = Rt::new();
        let path = rt.string(file.to_str().unwrap());
        let (fw, bw, pw) = (rt.new_object(FILE), rt.new_object(BUFFERED), rt.new_object(PRINT));
        c(&mut rt, "java/io/FileWriter.<init>.(Ljava/lang/String;)V", &[fw, path]);
        c(&mut rt, "java/io/BufferedWriter.<init>.(Ljava/io/Writer;)V", &[bw, fw]);
        c(&mut rt, "java/io/PrintWriter.<init>.(Ljava/io/Writer;)V", &[pw, bw]);
        let (speed, value, chars) = (rt.string("speed="), rt.string("# made by a mod"), rt.alloc("[C", crate::Data::Array("xyz!".encode_utf16().map(|u| V::I(i32::from(u))).collect())));
        c(&mut rt, "java/io/PrintWriter.println.(Ljava/lang/String;)V", &[pw, value]);
        c(&mut rt, "java/io/PrintWriter.print.(Ljava/lang/String;)V", &[pw, speed]);
        c(&mut rt, "java/io/PrintWriter.println.(I)V", &[pw, V::I(7)]);
        c(&mut rt, "java/io/BufferedWriter.write.([CII)V", &[bw, chars, V::I(1), V::I(2)]);
        c(&mut rt, "java/io/BufferedWriter.newLine.()V", &[bw]);
        assert_eq!(c(&mut rt, "java/io/Writer.append.(C)Ljava/io/Writer;", &[bw, V::I('!' as i32)]), bw);
        c(&mut rt, "java/io/PrintWriter.close.()V", &[pw]);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "# made by a mod\nspeed=7\nyz\n!");
        // A second writer appends; a plain one would start the file again.
        let more = rt.new_object(FILE);
        c(&mut rt, "java/io/FileWriter.<init>.(Ljava/lang/String;Z)V", &[more, path, V::I(1)]);
        let (fmt, vals, on) = (rt.string("%s=%d%n"), rt.new_array(2, V::NULL), rt.string("on"));
        let one = c(&mut rt, "java/lang/Integer.valueOf.(I)Ljava/lang/Integer;", &[V::I(1)]);
        rt.array_set(vals, 0, on);
        rt.array_set(vals, 1, one);
        let printer = rt.new_object(PRINT);
        c(&mut rt, "java/io/PrintWriter.<init>.(Ljava/io/Writer;)V", &[printer, more]);
        c(&mut rt, "java/io/PrintWriter.printf.(Ljava/lang/String;[Ljava/lang/Object;)Ljava/io/PrintWriter;", &[printer, fmt, vals]);
        assert!(std::fs::read_to_string(&file).unwrap().ends_with("!on=1\n"));
        // Kept text, and a writer over a stream of bytes.
        let (sw, bytes, osw) = (rt.new_object(STRING), rt.new_object("java/io/ByteArrayOutputStream"), rt.new_object(STREAM));
        c(&mut rt, "java/io/StringWriter.<init>.()V", &[sw]);
        c(&mut rt, "java/io/StringWriter.write.(Ljava/lang/String;)V", &[sw, speed]);
        c(&mut rt, "java/io/StringWriter.write.(I)V", &[sw, V::I('9' as i32)]);
        let kept = c(&mut rt, "java/io/StringWriter.toString.()Ljava/lang/String;", &[sw]);
        assert_eq!(rt.str_of(kept), Some("speed=9"));
        // What it gave is a string of its own: more written after is not in it, and a long
        // text written in small pieces is all there.
        let buffer = c(
            &mut rt,
            "java/io/StringWriter.getBuffer.()Ljava/lang/StringBuffer;",
            &[sw],
        );
        for _ in 0..20_000 {
            c(
                &mut rt,
                "java/io/StringWriter.write.(I)V",
                &[sw, V::I('!' as i32)],
            );
        }
        assert_eq!(rt.str_of(kept), Some("speed=9"));
        assert_eq!(
            c(&mut rt, "java/lang/StringBuffer.length.()I", &[buffer]),
            V::I(7)
        );
        let longer = c(
            &mut rt,
            "java/io/StringWriter.toString.()Ljava/lang/String;",
            &[sw],
        );
        assert_eq!(rt.str_of(longer).map(str::len), Some(20_007));
        c(&mut rt, "java/io/ByteArrayOutputStream.<init>.()V", &[bytes]);
        c(&mut rt, "java/io/OutputStreamWriter.<init>.(Ljava/io/OutputStream;)V", &[osw, bytes]);
        c(&mut rt, "java/io/OutputStreamWriter.write.(Ljava/lang/String;)V", &[osw, on]);
        c(&mut rt, "java/io/OutputStreamWriter.flush.()V", &[osw]);
        assert_eq!(c(&mut rt, "java/io/ByteArrayOutputStream.size.()I", &[bytes]), V::I(2));
        assert!(rt.thrown.is_none() && rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        // A file that can't be made is the exception Java throws.
        let nowhere = rt.string(dir.join("none/there.txt").to_str().unwrap());
        let bad = rt.new_object(FILE);
        c(&mut rt, "java/io/FileWriter.<init>.(Ljava/lang/String;)V", &[bad, nowhere]);
        assert!(rt.thrown.take().is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
