//! Text a mod reads one character or one piece at a time, the way vanilla hands it out: the
//! `StringDecomposer` walks (a string's code points, its `§` codes turned into styles), a
//! `FormattedCharSequence`'s `accept(sink)`, and a text's `visit(consumer)`. Each calls the mod's
//! sink or consumer in order and stops where it says so.

use crate::host::TextRun;
use crate::{Rt, V};

const DECOMPOSER: &str = "net/minecraft/util/StringDecomposer";
const SEQUENCE: &str = "net/minecraft/util/FormattedCharSequence";
const FORMATTED: &str = "net/minecraft/network/chat/FormattedText";
const COMPONENT: &str = "net/minecraft/network/chat/Component";
const MUTABLE: &str = "net/minecraft/network/chat/MutableComponent";
const SINK: &str =
    "net/minecraft/util/FormattedCharSink.accept.(ILnet/minecraft/network/chat/Style;I)Z";
const STYLED: &str = "net/minecraft/network/chat/FormattedText$StyledContentConsumer.accept.(Lnet/minecraft/network/chat/Style;Ljava/lang/String;)Ljava/util/Optional;";
const CONTENT: &str = "net/minecraft/network/chat/FormattedText$ContentConsumer.accept.(Ljava/lang/String;)Ljava/util/Optional;";
/// What a broken surrogate pair reads as (`StringDecomposer.REPLACEMENT_CHAR`).
const REPLACEMENT: i32 = 0xFFFD;
/// `§`, which starts a legacy format code.
const SECTION: u16 = 0xA7;

pub(crate) fn claims(owner: &str, name: &str) -> bool {
    owner == DECOMPOSER
        || (owner == SEQUENCE && name == "accept")
        || (matches!(owner, FORMATTED | COMPONENT | MUTABLE) && name == "visit")
}

pub(crate) fn call(rt: &mut Rt, owner: &str, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match (owner, name) {
        // `iterateFormatted(FormattedText, style, sink)`: each piece of the text in its style over
        // `style`, its `§` codes applied, a `§r` going back to the piece's own style.
        (DECOMPOSER, "iterateFormatted")
            if desc.starts_with("(Lnet/minecraft/network/chat/FormattedText;") =>
        {
            let base = crate::gui::style_tokens(rt, a(1));
            let pieces = crate::gui::runs_under(rt, a(0), &base);
            V::I(sequence_accept(rt, &pieces, a(2)) as i32)
        }
        (DECOMPOSER, "iterateFormatted") => {
            let text = units(rt, a(0));
            let (skip, current, default, sink) = match args.len() {
                3 => (0, a(1), a(1), a(2)),
                4 => (a(1).i(), a(2), a(2), a(3)),
                _ => (a(1).i(), a(2), a(3), a(4)),
            };
            V::I(formatted(rt, &text, skip, current, default, sink) as i32)
        }
        (DECOMPOSER, "iterate") => {
            let text = units(rt, a(0));
            V::I(forward(rt, &text, 0, a(1), a(2)) as i32)
        }
        (DECOMPOSER, "iterateBackwards") => {
            let text = units(rt, a(0));
            V::I(backward(rt, &text, a(1), a(2)) as i32)
        }
        // A string here holds no broken surrogate pair to filter.
        (DECOMPOSER, "filterBrokenSurrogates") => a(0),
        (DECOMPOSER, "getPlainText") => {
            let text = crate::gui::plain(rt, a(0));
            rt.string(&text)
        }
        (SEQUENCE, "accept") => {
            let pieces = crate::gui::runs(rt, a(0));
            V::I(sequence_accept(rt, &pieces, a(1)) as i32)
        }
        // `visit(consumer[, style])`: each piece to the consumer until it answers with a value,
        // which `visit` returns; empty when none does.
        (_, "visit") => {
            let styled = desc
                .starts_with("(Lnet/minecraft/network/chat/FormattedText$StyledContentConsumer;");
            let base = if styled {
                crate::gui::style_tokens(rt, a(2))
            } else {
                String::new()
            };
            for piece in crate::gui::runs_under(rt, a(0), &base) {
                let text = rt.string(&piece.text);
                let answer = if styled {
                    let style = style_of(rt, &piece);
                    rt.call_functional(STYLED, a(1), &[style, text])
                } else {
                    rt.call_functional(CONTENT, a(1), &[text])
                };
                if rt.thrown.is_some() {
                    return Some(V::NULL);
                }
                if answer.r() != 0 && rt.get_field(answer, "value") != V::NULL {
                    return Some(answer);
                }
            }
            rt.intern("compat:optional-empty".into(), |rt| {
                crate::api_common::optional(rt, V::NULL)
            })
        }
        _ => return None,
    })
}

/// A Java string's UTF-16 units (a null one has none).
fn units(rt: &Rt, v: V) -> Vec<u16> {
    rt.str_of(v)
        .map(|s| s.encode_utf16().collect())
        .unwrap_or_default()
}

fn style_of(rt: &mut Rt, piece: &TextRun) -> V {
    let tokens = crate::gui::tokens_of(piece);
    crate::gui::style_object(rt, &tokens)
}

/// The code point at `j` and how many units it takes; a surrogate without its pair is U+FFFD.
fn code_point(text: &[u16], j: usize) -> (i32, usize) {
    let c = i32::from(text[j]);
    match (text[j], text.get(j + 1)) {
        (0xD800..=0xDBFF, Some(&low @ 0xDC00..=0xDFFF)) => (
            0x10000 + ((c - 0xD800) << 10) + (i32::from(low) - 0xDC00),
            2,
        ),
        (0xD800..=0xDFFF, _) => (REPLACEMENT, 1),
        _ => (c, 1),
    }
}

/// `sink.accept(index, style, codePoint)`; false when the sink stops the walk or throws.
fn accept(rt: &mut Rt, sink: V, index: usize, style: V, code_point: i32) -> bool {
    let go = rt.call_functional(SINK, sink, &[V::I(index as i32), style, V::I(code_point)]);
    rt.thrown.is_none() && go.i() != 0
}

/// `StringDecomposer.iterate`: each code point from `from` on, at its index.
fn forward(rt: &mut Rt, text: &[u16], from: usize, style: V, sink: V) -> bool {
    let mut j = from;
    while j < text.len() {
        let (c, width) = code_point(text, j);
        if !accept(rt, sink, j, style, c) {
            return false;
        }
        j += width;
    }
    true
}

/// `StringDecomposer.iterateBackwards`: from the last code point to the first, a pair at the
/// index of its first unit.
fn backward(rt: &mut Rt, text: &[u16], style: V, sink: V) -> bool {
    let mut j = text.len();
    while j > 0 {
        j -= 1;
        let unit = text[j];
        let (at, c) = match unit {
            0xDC00..=0xDFFF if j == 0 => (0, REPLACEMENT),
            0xDC00..=0xDFFF => match text[j - 1] {
                high @ 0xD800..=0xDBFF => {
                    j -= 1;
                    (
                        j,
                        0x10000 + ((i32::from(high) - 0xD800) << 10) + (i32::from(unit) - 0xDC00),
                    )
                }
                _ => (j, REPLACEMENT),
            },
            0xD800..=0xDBFF => (j, REPLACEMENT),
            _ => (j, i32::from(unit)),
        };
        if !accept(rt, sink, at, style, c) {
            return false;
        }
    }
    true
}

/// `StringDecomposer.iterateFormatted(text, skip, current, default, sink)`: each code point from
/// `skip` on, in the style the `§` codes before it make (`Style.applyLegacyFormat`; `§r` goes back
/// to `default`). A code that names no format is skipped with its `§`, and so is a last `§`.
fn formatted(rt: &mut Rt, text: &[u16], skip: i32, current: V, default: V, sink: V) -> bool {
    let mut style = current;
    let mut j = usize::try_from(skip).unwrap_or(0);
    while j < text.len() {
        if text[j] == SECTION {
            let Some(&code) = text.get(j + 1) else { break };
            match char::from_u32(u32::from(code)).and_then(crate::gui::legacy_format) {
                Some("RESET") => style = default,
                Some(format) => {
                    let tokens =
                        crate::gui::legacy_applied(&crate::gui::style_tokens(rt, style), format);
                    style = crate::gui::style_object(rt, &tokens);
                }
                None => {}
            }
            j += 2;
            continue;
        }
        let (c, width) = code_point(text, j);
        if !accept(rt, sink, j, style, c) {
            return false;
        }
        j += width;
    }
    true
}

/// A sequence's pieces to `sink`, each piece's code points indexed from its start (as
/// `FormattedCharSequence.forward` and `composite` feed them).
fn sequence_accept(rt: &mut Rt, pieces: &[TextRun], sink: V) -> bool {
    for piece in pieces {
        let style = style_of(rt, piece);
        let text: Vec<u16> = piece.text.encode_utf16().collect();
        if !forward(rt, &text, 0, style, sink) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const DECOMPOSE: &str = "net/minecraft/util/StringDecomposer.iterateFormatted.(Ljava/lang/String;Lnet/minecraft/network/chat/Style;Lnet/minecraft/util/FormattedCharSink;)Z";
    const BACKWARDS: &str = "net/minecraft/util/StringDecomposer.iterateBackwards.(Ljava/lang/String;Lnet/minecraft/network/chat/Style;Lnet/minecraft/util/FormattedCharSink;)Z";
    const LITERAL: &str = "net/minecraft/network/chat/Component.literal.(Ljava/lang/String;)Lnet/minecraft/network/chat/MutableComponent;";
    const VISIT: &str = "net/minecraft/network/chat/Component.visit.(Lnet/minecraft/network/chat/FormattedText$StyledContentConsumer;Lnet/minecraft/network/chat/Style;)Ljava/util/Optional;";
    const IS_BOLD: &str = "net/minecraft/network/chat/Style.isBold.()Z";
    const COLOR_VALUE: &str = "net/minecraft/network/chat/TextColor.getValue.()I";
    const GET_COLOR: &str =
        "net/minecraft/network/chat/Style.getColor.()Lnet/minecraft/network/chat/TextColor;";

    /// A sink that keeps what it is fed (index, code point, bold, colour) and stops after `limit`.
    fn recorder(rt: &mut Rt, limit: i32) -> V {
        let sink = rt.new_object("test/Recorder");
        rt.put_field(sink, "limit", V::I(limit));
        let seen = crate::jutil::new_list(rt, Vec::new());
        rt.put_field(sink, "seen", seen);
        sink
    }

    fn recorder_overrides(class: &str, short: &str) -> bool {
        class == "test/Recorder" && short == "accept.(ILnet/minecraft/network/chat/Style;I)Z"
            || class == "test/Pieces"
                && short
                    == "accept.(Lnet/minecraft/network/chat/Style;Ljava/lang/String;)Ljava/util/Optional;"
    }

    fn recorder_code(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        let this = args[0];
        let seen = rt.get_field(this, "seen");
        if short.starts_with("accept.(I") {
            let bold = crate::api::call(rt, IS_BOLD, &[args[2]]).i();
            let color = crate::api::call(rt, GET_COLOR, &[args[2]]);
            let color = if color.r() == 0 {
                "-".to_owned()
            } else {
                format!("{:x}", crate::api::call(rt, COLOR_VALUE, &[color]).i())
            };
            let entry = format!(
                "{}:{}:{bold}:{color}",
                args[1].i(),
                char::from_u32(args[3].i() as u32).unwrap_or('?')
            );
            let entry = rt.string(&entry);
            crate::jutil::list_add(rt, seen, entry);
            let n = crate::jutil::elements(rt, seen).len() as i32;
            return V::I((n < rt.get_field(this, "limit").i()) as i32);
        }
        if short.starts_with("accept.(Lnet/minecraft/network/chat/Style;") {
            let text = rt.str_of(args[2]).unwrap_or("").to_owned();
            let bold = crate::api::call(rt, IS_BOLD, &[args[1]]).i();
            let entry = rt.string(&format!("{text}:{bold}"));
            crate::jutil::list_add(rt, seen, entry);
            // Stops at the piece that holds a "!".
            let found = if text.contains('!') { args[2] } else { V::NULL };
            return crate::api_common::optional(rt, found);
        }
        crate::api::call(rt, key, args)
    }

    fn seen(rt: &Rt, sink: V) -> Vec<String> {
        crate::jutil::elements(rt, rt.get_field(sink, "seen"))
            .into_iter()
            .map(|e| rt.str_of(e).unwrap_or("").to_owned())
            .collect()
    }

    fn runtime() -> Rt {
        let mut rt = Rt::new();
        crate::install();
        rt.overrides = Some(recorder_overrides);
        rt.vcall = Some(recorder_code);
        rt
    }

    /// `§` codes style what follows them (a colour clears bold, `§r` goes back to the style the
    /// walk started from) and aren't fed themselves; a pair of surrogates is one code point at the
    /// index of its first unit; a sink that answers false stops the walk, which then says false.
    #[test]
    fn iterate_formatted_feeds_code_points_in_the_styles_their_codes_make() {
        let mut rt = runtime();
        let text = rt.string("a§lb§cc§r\u{1F600}d§");
        let empty = crate::gui::style_object(&mut rt, "");
        let sink = recorder(&mut rt, 100);
        let all = crate::api::call(&mut rt, DECOMPOSE, &[text, empty, sink]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        assert_eq!(all, V::I(1));
        assert_eq!(
            seen(&rt, sink),
            [
                "0:a:0:-",
                "3:b:1:-",
                "6:c:0:ff5555",
                "9:\u{1F600}:0:-",
                "11:d:0:-"
            ]
        );

        let stopping = recorder(&mut rt, 2);
        let all = crate::api::call(&mut rt, DECOMPOSE, &[text, empty, stopping]);
        assert_eq!(all, V::I(0));
        assert_eq!(seen(&rt, stopping), ["0:a:0:-", "3:b:1:-"]);
    }

    #[test]
    fn iterate_backwards_feeds_the_last_code_point_first() {
        let mut rt = runtime();
        let text = rt.string("x\u{1F600}y");
        let empty = crate::gui::style_object(&mut rt, "");
        let sink = recorder(&mut rt, 100);
        assert_eq!(
            crate::api::call(&mut rt, BACKWARDS, &[text, empty, sink]),
            V::I(1)
        );
        assert_eq!(seen(&rt, sink), ["3:y:0:-", "1:\u{1F600}:0:-", "0:x:0:-"]);
    }

    /// `Component.visit(consumer, style)`: the pieces in order, each in its own style over the one
    /// given, until the consumer answers with something, which `visit` returns.
    #[test]
    fn visit_hands_out_pieces_until_the_consumer_answers() {
        let mut rt = runtime();
        let hello = rt.string("hello ");
        let root = crate::api::call(&mut rt, LITERAL, &[hello]);
        let world = rt.string("world!");
        let sibling = crate::api::call(&mut rt, LITERAL, &[world]);
        let bold = crate::gui::style_object(&mut rt, "BOLD,");
        crate::api::call(
            &mut rt,
            "net/minecraft/network/chat/MutableComponent.setStyle.(Lnet/minecraft/network/chat/Style;)Lnet/minecraft/network/chat/MutableComponent;",
            &[sibling, bold],
        );
        crate::api::call(
            &mut rt,
            "net/minecraft/network/chat/MutableComponent.append.(Lnet/minecraft/network/chat/Component;)Lnet/minecraft/network/chat/MutableComponent;",
            &[root, sibling],
        );
        let tail = rt.string(" and more");
        crate::api::call(
            &mut rt,
            "net/minecraft/network/chat/MutableComponent.append.(Ljava/lang/String;)Lnet/minecraft/network/chat/MutableComponent;",
            &[root, tail],
        );
        let consumer = rt.new_object("test/Pieces");
        let seen_list = crate::jutil::new_list(&mut rt, Vec::new());
        rt.put_field(consumer, "seen", seen_list);
        let empty = crate::gui::style_object(&mut rt, "");
        let found = crate::api::call(&mut rt, VISIT, &[root, consumer, empty]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        assert_eq!(seen(&rt, consumer), ["hello :0", "world!:1"]);
        assert_eq!(rt.str_of(rt.get_field(found, "value")), Some("world!"));
    }
}
