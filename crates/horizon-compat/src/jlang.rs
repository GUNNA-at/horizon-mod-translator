//! `java.lang` in native Rust: `String` (indexes and hashes in UTF-16 code units, as Java
//! counts them), `StringBuilder`, the boxed number classes, `Character`, `Math` and `Object`.
use crate::{Data, Rt, V};

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn from16(u: &[u16]) -> String {
    String::from_utf16_lossy(u)
}

fn text(rt: &Rt, v: V) -> String {
    rt.str_of(v).map(str::to_owned).unwrap_or_else(|| rt.to_java_string(v, 'L'))
}

fn new_str(rt: &mut Rt, s: String) -> V {
    rt.alloc("java/lang/String", Data::Str(s))
}

/// The JVM's kind letter of each parameter in `desc`.
fn kinds(desc: &str) -> Vec<char> {
    let mut out = Vec::new();
    let params = desc.strip_prefix('(').and_then(|d| d.split(')').next()).unwrap_or("");
    let mut it = params.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            'L' => {
                for d in it.by_ref() {
                    if d == ';' {
                        break;
                    }
                }
                out.push('L');
            }
            '[' => {
                while it.peek() == Some(&'[') {
                    it.next();
                }
                if it.next() == Some('L') {
                    for d in it.by_ref() {
                        if d == ';' {
                            break;
                        }
                    }
                }
                out.push('L');
            }
            c => out.push(c),
        }
    }
    out
}

fn string_hash(s: &str) -> i32 {
    s.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

/// A `java.lang` call (`owner.name.desc`), or None when this module doesn't model it.
pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    if let Some(v) = libs(rt, key, args) {
        return Some(v);
    }
    let (owner, rest) = key.split_once('.')?;
    let (name, desc) = rest.split_once('.')?;
    if !owner.starts_with("java/lang/") {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let ks = kinds(desc);
    match owner {
        "java/lang/String" | "java/lang/CharSequence" => string(rt, name, desc, &ks, args),
        "java/lang/StringBuilder" | "java/lang/StringBuffer" | "java/lang/AbstractStringBuilder" => builder(rt, name, &ks, args),
        "java/lang/Integer" | "java/lang/Long" | "java/lang/Short" | "java/lang/Byte" | "java/lang/Double" | "java/lang/Float" | "java/lang/Number" => number(rt, owner, name, desc, &ks, args),
        "java/lang/Boolean" => Some(match name {
            "parseBoolean" => V::I(rt.str_of(a(0)).is_some_and(|s| s.eq_ignore_ascii_case("true")) as i32),
            "toString" if desc.starts_with("(Z)") => {
                let s = (a(0).i() != 0).to_string();
                rt.string(&s)
            }
            "compare" => V::I(a(0).i().cmp(&a(1).i()) as i32),
            "hashCode" if desc.starts_with("(Z)") => V::I(if a(0).i() != 0 { 1231 } else { 1237 }),
            "logicalAnd" => V::I(a(0).i() & a(1).i()),
            "logicalOr" => V::I(a(0).i() | a(1).i()),
            "logicalXor" => V::I(a(0).i() ^ a(1).i()),
            _ => return None,
        }),
        "java/lang/Character" => {
            let c = char::from_u32(a(0).i() as u32).unwrap_or('\0');
            Some(match name {
                "isDigit" => V::I(c.is_ascii_digit() as i32),
                "isLetter" | "isAlphabetic" => V::I(c.is_alphabetic() as i32),
                "isLetterOrDigit" => V::I(c.is_alphanumeric() as i32),
                "isWhitespace" | "isSpaceChar" => V::I(c.is_whitespace() as i32),
                "isUpperCase" => V::I(c.is_uppercase() as i32),
                "isLowerCase" => V::I(c.is_lowercase() as i32),
                "toUpperCase" => V::I(c.to_uppercase().next().unwrap_or(c) as i32),
                "toLowerCase" => V::I(c.to_lowercase().next().unwrap_or(c) as i32),
                "getNumericValue" => V::I(c.to_digit(36).map_or(-1, |d| d as i32)),
                "valueOf" => {
                    let o = rt.new_object("java/lang/Character");
                    rt.put_field(o, "value", a(0));
                    o
                }
                "charValue" => rt.get_field(a(0), "value"),
                "toString" => {
                    let c = if ks.is_empty() { char::from_u32(rt.get_field(a(0), "value").i() as u32).unwrap_or('\0') } else { c };
                    rt.string(&c.to_string())
                }
                _ => return None,
            })
        }
        "java/lang/Math" | "java/lang/StrictMath" => math(name, &ks, args),
        "java/lang/Object" => Some(match name {
            "getClass" => {
                let c = rt.class_of(a(0));
                rt.alloc("java/lang/Class", Data::Str(c.into()))
            }
            "hashCode" => V::I(crate::jutil::hash(rt, a(0))),
            "equals" => V::I(crate::jutil::equals(rt, a(0), a(1)) as i32),
            "toString" => {
                let s = text(rt, a(0));
                new_str(rt, s)
            }
            _ => return None,
        }),
        "java/lang/System" => Some(match name {
            "currentTimeMillis" => V::J(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)),
            "nanoTime" => {
                static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
                V::J(START.get_or_init(std::time::Instant::now).elapsed().as_nanos() as i64)
            }
            "identityHashCode" => V::I(a(0).r() as i32),
            "arraycopy" => {
                let (src, sp, dst, dp, n) = (a(0), a(1).i().max(0) as usize, a(2), a(3).i().max(0) as usize, a(4).i().max(0) as usize);
                let part: Vec<V> = match rt.heap.get(src.r() as usize).map(|o| &o.data) {
                    Some(Data::Array(v)) => v.iter().skip(sp).take(n).copied().collect(),
                    _ => return Some(V::Void),
                };
                if let Some(Data::Array(d)) = rt.heap.get_mut(dst.r() as usize).map(|o| &mut o.data) {
                    for (i, v) in part.into_iter().enumerate() {
                        if let Some(slot) = d.get_mut(dp + i) {
                            *slot = v;
                        }
                    }
                }
                V::Void
            }
            "getProperty" | "getenv" => V::NULL,
            "lineSeparator" => rt.string("\n"),
            _ => return None,
        }),
        "java/lang/Enum" => Some(match name {
            "name" | "toString" => rt.get_field(a(0), "$name"),
            "compareTo" => V::I(rt.get_field(a(0), "$ordinal").i() - rt.get_field(a(1), "$ordinal").i()),
            "hashCode" => V::I(a(0).r() as i32),
            "equals" => V::I((a(0) == a(1)) as i32),
            "getDeclaringClass" => {
                let c = rt.class_of(a(0));
                rt.alloc("java/lang/Class", Data::Str(c.into()))
            }
            _ => return None,
        }),
        "java/lang/Class" => Some(match name {
            "isInstance" => {
                let target = rt.str_of(a(0)).unwrap_or("").to_owned();
                V::I(rt.instance_of(a(1), &target) as i32)
            }
            "isAssignableFrom" => {
                let (this, other) = (rt.str_of(a(0)).unwrap_or("").to_owned(), rt.str_of(a(1)).unwrap_or("").to_owned());
                V::I(rt.is_a(&other, &this) as i32)
            }
            "cast" => a(1),
            "desiredAssertionStatus" => V::I(0),
            "hashCode" => V::I(string_hash(rt.str_of(a(0)).unwrap_or(""))),
            "equals" => V::I((rt.str_of(a(0)) == rt.str_of(a(1))) as i32),
            "isEnum" | "isInterface" | "isArray" | "isRecord" => V::I(0),
            _ => return None,
        }),
        "java/lang/Thread" => Some(match name {
            "currentThread" => rt.new_object("java/lang/Thread"),
            "getName" => rt.string("Server thread"),
            "sleep" | "yield" | "onSpinWait" => V::Void,
            _ => return None,
        }),
        "java/lang/Runtime" => Some(match name {
            "getRuntime" => rt.new_object("java/lang/Runtime"),
            "availableProcessors" => V::I(std::thread::available_parallelism().map_or(4, |n| n.get() as i32)),
            "maxMemory" | "totalMemory" => V::J(4 << 30),
            "freeMemory" => V::J(1 << 30),
            _ => return None,
        }),
        _ => None,
    }
}

fn string(rt: &mut Rt, name: &str, desc: &str, ks: &[char], args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    // Statics first.
    match name {
        "valueOf" | "copyValueOf" => {
            let s = match ks.first() {
                Some('L') if rt.class_of(a(0)).starts_with('[') => {
                    let chars: Vec<u16> = crate::jutil::elements(rt, a(0)).iter().map(|v| v.i() as u16).collect();
                    from16(&chars)
                }
                Some(&k) => rt.to_java_string(a(0), k),
                None => String::new(),
            };
            return Some(new_str(rt, s));
        }
        "join" => {
            let sep = text(rt, a(0));
            let parts: Vec<V> = if desc.contains("Iterable") { crate::jutil::elements(rt, a(1)) } else if desc.contains("[Ljava/lang/CharSequence;") { crate::jutil::elements(rt, a(1)) } else { args[1..].to_vec() };
            let s = parts.iter().map(|&p| text(rt, p)).collect::<Vec<_>>().join(&sep);
            return Some(new_str(rt, s));
        }
        "format" => {
            let (fmt, rest) = if desc.starts_with("(Ljava/util/Locale;") { (a(1), a(2)) } else { (a(0), a(1)) };
            let f = text(rt, fmt);
            let vals = crate::jutil::elements(rt, rest);
            let s = format_java(rt, &f, &vals);
            return Some(new_str(rt, s));
        }
        _ => {}
    }
    let this = a(0);
    let s = rt.str_of(this)?.to_owned();
    let u = utf16(&s);
    let arg_s = |rt: &Rt, i: usize| text(rt, a(i));
    Some(match name {
        "length" => V::I(u.len() as i32),
        "isEmpty" => V::I(u.is_empty() as i32),
        "isBlank" => V::I(s.trim().is_empty() as i32),
        "hashCode" => V::I(string_hash(&s)),
        "equals" | "contentEquals" => V::I((Some(s.as_str()) == rt.str_of(a(1))) as i32),
        "equalsIgnoreCase" => V::I(rt.str_of(a(1)).is_some_and(|o| o.to_lowercase() == s.to_lowercase()) as i32),
        "compareTo" => {
            let o = utf16(&arg_s(rt, 1));
            let n = u.len().min(o.len());
            V::I((0..n).find(|&i| u[i] != o[i]).map_or(u.len() as i32 - o.len() as i32, |i| u[i] as i32 - o[i] as i32))
        }
        "compareToIgnoreCase" => {
            let (x, y) = (s.to_lowercase(), arg_s(rt, 1).to_lowercase());
            V::I(x.cmp(&y) as i32)
        }
        "charAt" | "codePointAt" => V::I(u.get(a(1).i().max(0) as usize).copied().unwrap_or(0) as i32),
        "startsWith" => {
            let p = utf16(&arg_s(rt, 1));
            let from = if ks.len() > 1 { a(2).i().max(0) as usize } else { 0 };
            V::I(u.get(from..).is_some_and(|t| t.starts_with(&p)) as i32)
        }
        "endsWith" => V::I(s.ends_with(&arg_s(rt, 1)) as i32),
        "contains" => V::I(s.contains(&arg_s(rt, 1)) as i32),
        "indexOf" | "lastIndexOf" => {
            let needle: Vec<u16> = match ks.first() {
                Some('I') => char::from_u32(a(1).i() as u32).map(|c| utf16(&c.to_string())).unwrap_or_default(),
                _ => utf16(&arg_s(rt, 1)),
            };
            let from = ks.get(1).map(|_| a(2).i());
            let positions: Vec<usize> = (0..=u.len().saturating_sub(needle.len())).filter(|&i| u.len() >= needle.len() && u[i..i + needle.len()] == needle[..]).collect();
            let found = if name == "indexOf" {
                positions.into_iter().find(|&i| from.is_none_or(|f| i as i32 >= f))
            } else {
                positions.into_iter().rev().find(|&i| from.is_none_or(|f| i as i32 <= f))
            };
            V::I(found.map_or(-1, |i| i as i32))
        }
        "substring" | "subSequence" => {
            let lo = (a(1).i().max(0) as usize).min(u.len());
            let hi = if ks.len() > 1 { (a(2).i().max(0) as usize).min(u.len()) } else { u.len() };
            new_str(rt, from16(&u[lo..hi.max(lo)]))
        }
        "toLowerCase" => new_str(rt, s.to_lowercase()),
        "toUpperCase" => new_str(rt, s.to_uppercase()),
        "trim" | "strip" => new_str(rt, s.trim().to_owned()),
        "stripLeading" => new_str(rt, s.trim_start().to_owned()),
        "stripTrailing" => new_str(rt, s.trim_end().to_owned()),
        "intern" | "toString" => {
            let v = rt.string(&s);
            v
        }
        "concat" => {
            let t = format!("{s}{}", arg_s(rt, 1));
            new_str(rt, t)
        }
        "repeat" => new_str(rt, s.repeat(a(1).i().max(0) as usize)),
        "replace" => {
            let (from, to) = if ks.first() == Some(&'C') {
                (char::from_u32(a(1).i() as u32).unwrap_or('\0').to_string(), char::from_u32(a(2).i() as u32).unwrap_or('\0').to_string())
            } else {
                (arg_s(rt, 1), arg_s(rt, 2))
            };
            new_str(rt, if from.is_empty() { s } else { s.replace(&from, &to) })
        }
        "replaceAll" | "replaceFirst" => {
            // Literal patterns only (what mods pass in practice: `_`, ` `, `\\.`).
            let pat = unescape_regex(&arg_s(rt, 1))?;
            let to = arg_s(rt, 2);
            new_str(rt, if name == "replaceAll" { s.replace(&pat, &to) } else { s.replacen(&pat, &to, 1) })
        }
        "split" => {
            let pat = unescape_regex(&arg_s(rt, 1))?;
            let mut parts: Vec<String> = if pat.is_empty() { s.chars().map(String::from).collect() } else { s.split(pat.as_str()).map(str::to_owned).collect() };
            // Java drops trailing empty strings (limit 0).
            if ks.len() < 2 || a(2).i() == 0 {
                while parts.last().is_some_and(String::is_empty) && parts.len() > 1 {
                    parts.pop();
                }
            }
            let items: Vec<V> = parts.into_iter().map(|p| new_str(rt, p)).collect();
            rt.alloc("[Ljava/lang/String;", Data::Array(items))
        }
        "toCharArray" => rt.alloc("[C", Data::Array(u.iter().map(|&c| V::I(c as i32)).collect())),
        "chars" => rt.alloc("java/util/stream/Stream", Data::Stream(u.iter().map(|&c| V::I(c as i32)).collect())),
        "matches" => V::I((unescape_regex(&arg_s(rt, 1)).as_deref() == Some(s.as_str())) as i32),
        _ => return None,
    })
}

/// A regex that is a plain string (`_`, `\\.`, `\\s` excepted), unescaped; None for real patterns.
fn unescape_regex(p: &str) -> Option<String> {
    let mut out = String::new();
    let mut it = p.chars();
    while let Some(c) = it.next() {
        match c {
            '\\' => {
                let n = it.next()?;
                if n.is_ascii_alphanumeric() {
                    return None;
                }
                out.push(n);
            }
            '.' | '*' | '+' | '?' | '[' | ']' | '(' | ')' | '{' | '}' | '^' | '$' | '|' => return None,
            c => out.push(c),
        }
    }
    Some(out)
}

/// `String.format` for the conversions mods use: `%s %d %f %.Nf %x %c %b %%` and `%n`.
fn format_java(rt: &Rt, f: &str, vals: &[V]) -> String {
    let mut out = String::new();
    let mut it = f.chars().peekable();
    let mut next = 0;
    while let Some(c) = it.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut spec = String::new();
        while let Some(&d) = it.peek() {
            if d.is_ascii_digit() || d == '.' || d == '-' || d == '0' || d == ',' || d == '+' {
                spec.push(d);
                it.next();
            } else {
                break;
            }
        }
        let Some(conv) = it.next() else { break };
        let v = vals.get(next).copied().unwrap_or_default();
        let unbox = |v: V| match v {
            V::R(_) if rt.class_of(v).starts_with("java/lang/") && rt.str_of(v).is_none() => rt.get_field(v, "value"),
            v => v,
        };
        let piece = match conv {
            '%' => "%".to_owned(),
            'n' => "\n".to_owned(),
            's' | 'S' => {
                next += 1;
                let t = text(rt, v);
                if conv == 'S' { t.to_uppercase() } else { t }
            }
            'd' => {
                next += 1;
                match unbox(v) {
                    V::J(j) => j.to_string(),
                    x => x.i().to_string(),
                }
            }
            'x' | 'X' => {
                next += 1;
                let t = match unbox(v) {
                    V::J(j) => format!("{j:x}"),
                    x => format!("{:x}", x.i()),
                };
                if conv == 'X' { t.to_uppercase() } else { t }
            }
            'f' | 'e' | 'g' => {
                next += 1;
                let x = match unbox(v) {
                    V::F(x) => x as f64,
                    V::D(x) => x,
                    V::I(i) => i as f64,
                    V::J(j) => j as f64,
                    _ => 0.0,
                };
                let prec = spec.split_once('.').and_then(|(_, p)| p.parse().ok()).unwrap_or(6);
                fixed_half_up(x, prec)
            }
            'c' => {
                next += 1;
                char::from_u32(unbox(v).i() as u32).map(String::from).unwrap_or_default()
            }
            'b' => {
                next += 1;
                (unbox(v).i() != 0).to_string()
            }
            other => format!("%{spec}{other}"),
        };
        // Width (`%5d`, `%-5s`, `%05d`).
        let width: usize = spec.split('.').next().unwrap_or("").trim_start_matches(['-', '0', '+', ',']).parse().unwrap_or(0);
        if piece.chars().count() < width {
            let pad = width - piece.chars().count();
            if spec.starts_with('-') {
                out.push_str(&piece);
                out.push_str(&" ".repeat(pad));
            } else if spec.starts_with('0') {
                out.push_str(&"0".repeat(pad));
                out.push_str(&piece);
            } else {
                out.push_str(&" ".repeat(pad));
                out.push_str(&piece);
            }
        } else {
            out.push_str(&piece);
        }
    }
    out
}

/// `x` with `prec` decimals as Java's `Formatter` writes it: the shortest decimal that reads
/// back as `x`, rounded half up.
fn fixed_half_up(x: f64, prec: usize) -> String {
    if !x.is_finite() {
        return if x.is_nan() { "NaN".into() } else if x > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    let neg = x.is_sign_negative() && x != 0.0;
    let shortest = format!("{}", x.abs());
    let (int, frac) = shortest.split_once('.').unwrap_or((&shortest, ""));
    let mut digits: Vec<u8> = int.bytes().chain(frac.bytes()).map(|b| b - b'0').collect();
    let point = int.len();
    let keep = point + prec;
    if digits.len() > keep {
        let up = digits[keep] >= 5;
        digits.truncate(keep);
        if up {
            let mut i = keep;
            loop {
                if i == 0 {
                    digits.insert(0, 1);
                    break;
                }
                i -= 1;
                if digits[i] == 9 {
                    digits[i] = 0;
                } else {
                    digits[i] += 1;
                    break;
                }
            }
        }
    }
    while digits.len() < keep {
        digits.push(0);
    }
    let int_len = digits.len() - prec;
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    out.extend(digits[..int_len].iter().map(|d| (d + b'0') as char));
    if prec > 0 {
        out.push('.');
        out.extend(digits[int_len..].iter().map(|d| (d + b'0') as char));
    }
    out
}

fn builder(rt: &mut Rt, name: &str, ks: &[char], args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    let cur = |rt: &Rt| match rt.heap.get(this.r() as usize).map(|o| &o.data) {
        Some(Data::Str(s)) => s.clone(),
        _ => String::new(),
    };
    let set = |rt: &mut Rt, s: String| {
        if let Some(o) = rt.heap.get_mut(this.r() as usize) {
            o.data = Data::Str(s);
        }
    };
    Some(match name {
        "<init>" => {
            let s = match ks.first() {
                Some('L') => text(rt, a(1)),
                _ => String::new(),
            };
            set(rt, s);
            V::Void
        }
        "append" => {
            let piece = match ks.first() {
                Some('L') if rt.class_of(a(1)) == "[C" => from16(&crate::jutil::elements(rt, a(1)).iter().map(|v| v.i() as u16).collect::<Vec<_>>()),
                Some(&k) => rt.to_java_string(a(1), k),
                None => String::new(),
            };
            let s = cur(rt) + &piece;
            set(rt, s);
            this
        }
        "insert" => {
            let mut u = utf16(&cur(rt));
            let at = (a(1).i().max(0) as usize).min(u.len());
            let piece = utf16(&rt.to_java_string(a(2), ks.get(1).copied().unwrap_or('L')));
            u.splice(at..at, piece);
            set(rt, from16(&u));
            this
        }
        "toString" => {
            let s = cur(rt);
            new_str(rt, s)
        }
        "length" => V::I(utf16(&cur(rt)).len() as i32),
        "isEmpty" => V::I(cur(rt).is_empty() as i32),
        "charAt" => V::I(utf16(&cur(rt)).get(a(1).i().max(0) as usize).copied().unwrap_or(0) as i32),
        "setLength" => {
            let mut u = utf16(&cur(rt));
            u.resize(a(1).i().max(0) as usize, 0);
            set(rt, from16(&u));
            V::Void
        }
        "deleteCharAt" => {
            let mut u = utf16(&cur(rt));
            let i = a(1).i().max(0) as usize;
            if i < u.len() {
                u.remove(i);
            }
            set(rt, from16(&u));
            this
        }
        "delete" | "replace" => {
            let mut u = utf16(&cur(rt));
            let lo = (a(1).i().max(0) as usize).min(u.len());
            let hi = (a(2).i().max(0) as usize).min(u.len()).max(lo);
            let with = if name == "replace" { utf16(&text(rt, a(3))) } else { Vec::new() };
            u.splice(lo..hi, with);
            set(rt, from16(&u));
            this
        }
        "reverse" => {
            let s: String = cur(rt).chars().rev().collect();
            set(rt, s);
            this
        }
        "indexOf" => V::I(cur(rt).find(&text(rt, a(1))).map_or(-1, |i| utf16(&cur(rt)[..i]).len() as i32)),
        _ => return None,
    })
}

fn number(rt: &mut Rt, owner: &str, name: &str, desc: &str, ks: &[char], args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let simple = owner.rsplit('/').next().unwrap_or(owner);
    let boxed = |rt: &mut Rt, v: V| {
        let class: &'static str = match simple {
            "Long" => "java/lang/Long",
            "Double" => "java/lang/Double",
            "Float" => "java/lang/Float",
            "Short" => "java/lang/Short",
            "Byte" => "java/lang/Byte",
            _ => "java/lang/Integer",
        };
        let o = rt.new_object(class);
        rt.put_field(o, "value", v);
        o
    };
    let parse = |s: &str| -> V {
        let t = s.trim();
        match simple {
            "Long" => V::J(t.parse().unwrap_or(0)),
            "Double" => V::D(t.parse().unwrap_or(0.0)),
            "Float" => V::F(t.parse().unwrap_or(0.0)),
            _ => V::I(t.parse().unwrap_or(0)),
        }
    };
    // `xValue()` on any boxed number, with Java's narrowing.
    if let Some(kind) = name.strip_suffix("Value") {
        let v = rt.get_field(a(0), "value");
        let as_f64 = match v {
            V::I(i) => i as f64,
            V::J(j) => j as f64,
            V::F(f) => f as f64,
            V::D(d) => d,
            _ => 0.0,
        };
        return Some(match (kind, v) {
            ("int" | "short" | "byte", V::I(i)) => V::I(i),
            ("int", V::J(j)) => V::I(j as i32),
            ("int" | "short" | "byte", _) => V::I(crate::d2i(as_f64)),
            ("long", V::J(j)) => V::J(j),
            ("long", V::I(i)) => V::J(i as i64),
            ("long", _) => V::J(as_f64 as i64),
            ("float", _) => V::F(as_f64 as f32),
            ("double", _) => V::D(as_f64),
            _ => return None,
        });
    }
    Some(match name {
        "valueOf" if ks.first() == Some(&'L') => {
            let s = text(rt, a(0));
            let v = parse(&s);
            boxed(rt, v)
        }
        "valueOf" => boxed(rt, a(0)),
        "parseInt" | "parseLong" | "parseDouble" | "parseFloat" | "parseShort" | "parseByte" => {
            let s = text(rt, a(0));
            if ks.len() > 1 && simple == "Integer" {
                V::I(i32::from_str_radix(s.trim(), a(1).i() as u32).unwrap_or(0))
            } else {
                parse(&s)
            }
        }
        "toString" if ks.is_empty() => {
            let v = rt.get_field(a(0), "value");
            let k = match v {
                V::J(_) => 'J',
                V::F(_) => 'F',
                V::D(_) => 'D',
                _ => 'I',
            };
            let s = rt.to_java_string(v, k);
            new_str(rt, s)
        }
        "toString" => {
            let s = rt.to_java_string(a(0), ks[0]);
            new_str(rt, s)
        }
        "toHexString" | "toBinaryString" => {
            let s = match (a(0), name) {
                (V::J(j), "toHexString") => format!("{:x}", j as u64),
                (V::J(j), _) => format!("{:b}", j as u64),
                (v, "toHexString") => format!("{:x}", v.i() as u32),
                (v, _) => format!("{:b}", v.i() as u32),
            };
            new_str(rt, s)
        }
        "hashCode" if ks.is_empty() => {
            let v = rt.get_field(a(0), "value");
            V::I(rt.java_hash(v))
        }
        "hashCode" => V::I(rt.java_hash(a(0))),
        "equals" => V::I(crate::jutil::equals(rt, a(0), a(1)) as i32),
        "compare" | "compareTo" => {
            let (x, y) = if name == "compareTo" { (rt.get_field(a(0), "value"), rt.get_field(a(1), "value")) } else { (a(0), a(1)) };
            V::I(match (x, y) {
                (V::I(p), V::I(q)) => p.cmp(&q) as i32,
                (V::J(p), V::J(q)) => p.cmp(&q) as i32,
                (V::F(p), V::F(q)) => p.total_cmp(&q) as i32,
                (V::D(p), V::D(q)) => p.total_cmp(&q) as i32,
                _ => 0,
            })
        }
        "max" | "min" | "sum" => math(if name == "sum" { "addExact" } else { name }, ks, args)?,
        "signum" => V::I(match a(0) {
            V::J(j) => j.signum() as i32,
            v => v.i().signum(),
        }),
        "bitCount" => V::I(match a(0) {
            V::J(j) => j.count_ones() as i32,
            v => v.i().count_ones() as i32,
        }),
        "numberOfLeadingZeros" => V::I(match a(0) {
            V::J(j) => j.leading_zeros() as i32,
            v => v.i().leading_zeros() as i32,
        }),
        "numberOfTrailingZeros" => V::I(match a(0) {
            V::J(j) => j.trailing_zeros() as i32,
            v => v.i().trailing_zeros() as i32,
        }),
        "highestOneBit" => V::I(match a(0).i() {
            0 => 0,
            i => 1i32.wrapping_shl(31 - (i as u32).leading_zeros()),
        }),
        "isNaN" => V::I(match a(0) {
            V::F(f) => f.is_nan(),
            V::D(d) => d.is_nan(),
            _ => false,
        } as i32),
        "isInfinite" | "isFinite" => {
            let x = match a(0) {
                V::F(f) => f as f64,
                V::D(d) => d,
                _ => 0.0,
            };
            V::I(if name == "isFinite" { x.is_finite() } else { x.is_infinite() } as i32)
        }
        "floatToIntBits" | "floatToRawIntBits" => V::I(a(0).f().to_bits() as i32),
        "intBitsToFloat" => V::F(f32::from_bits(a(0).i() as u32)),
        "doubleToLongBits" | "doubleToRawLongBits" => V::J(a(0).d().to_bits() as i64),
        "longBitsToDouble" => V::D(f64::from_bits(a(0).j() as u64)),
        _ => {
            let _ = desc;
            return None;
        }
    })
}

fn math(name: &str, ks: &[char], args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let d = |i: usize| match a(i) {
        V::I(x) => x as f64,
        V::J(x) => x as f64,
        V::F(x) => x as f64,
        V::D(x) => x,
        _ => 0.0,
    };
    let k = ks.first().copied().unwrap_or('D');
    Some(match name {
        "max" | "min" => {
            let pick_max = name == "max";
            match (a(0), a(1)) {
                (V::I(x), V::I(y)) => V::I(if pick_max { x.max(y) } else { x.min(y) }),
                (V::J(x), V::J(y)) => V::J(if pick_max { x.max(y) } else { x.min(y) }),
                (V::F(x), V::F(y)) => V::F(if x.is_nan() || y.is_nan() { f32::NAN } else if pick_max { x.max(y) } else { x.min(y) }),
                (V::D(x), V::D(y)) => V::D(if x.is_nan() || y.is_nan() { f64::NAN } else if pick_max { x.max(y) } else { x.min(y) }),
                _ => return None,
            }
        }
        "abs" => match a(0) {
            V::I(x) => V::I(x.wrapping_abs()),
            V::J(x) => V::J(x.wrapping_abs()),
            V::F(x) => V::F(x.abs()),
            V::D(x) => V::D(x.abs()),
            _ => return None,
        },
        "addExact" => match (a(0), a(1)) {
            (V::I(x), V::I(y)) => V::I(x.wrapping_add(y)),
            (V::J(x), V::J(y)) => V::J(x.wrapping_add(y)),
            (V::F(x), V::F(y)) => V::F(x + y),
            (V::D(x), V::D(y)) => V::D(x + y),
            _ => return None,
        },
        "floor" => V::D(d(0).floor()),
        "ceil" => V::D(d(0).ceil()),
        "rint" => V::D(d(0).round_ties_even()),
        "round" => match k {
            'F' => V::I((a(0).f() + 0.5).floor() as i32),
            _ => V::J((d(0) + 0.5).floor() as i64),
        },
        "floorDiv" | "floorMod" => match (a(0), a(1)) {
            (V::I(x), V::I(y)) if y != 0 => V::I(if name == "floorDiv" { x.div_euclid(y) - if (x.rem_euclid(y) != 0) && y < 0 { 1 } else { 0 } } else { ((x % y) + y) % y }),
            (V::J(x), V::J(y)) if y != 0 => V::J(if name == "floorDiv" { (x as f64 / y as f64).floor() as i64 } else { ((x % y) + y) % y }),
            _ => return None,
        },
        "clamp" => match (a(0), a(1), a(2)) {
            (V::J(v), V::I(lo), V::I(hi)) => V::I((v.clamp(lo as i64, hi as i64)) as i32),
            (V::J(v), V::J(lo), V::J(hi)) => V::J(v.clamp(lo, hi)),
            (V::F(v), V::F(lo), V::F(hi)) => V::F(v.clamp(lo, hi)),
            (V::D(v), V::D(lo), V::D(hi)) => V::D(v.clamp(lo, hi)),
            (V::I(v), V::I(lo), V::I(hi)) => V::I(v.clamp(lo, hi)),
            _ => return None,
        },
        "sqrt" => V::D(d(0).sqrt()),
        "cbrt" => V::D(d(0).cbrt()),
        "pow" => V::D(d(0).powf(d(1))),
        "exp" => V::D(d(0).exp()),
        "log" => V::D(d(0).ln()),
        "log10" => V::D(d(0).log10()),
        "sin" => V::D(d(0).sin()),
        "cos" => V::D(d(0).cos()),
        "tan" => V::D(d(0).tan()),
        "asin" => V::D(d(0).asin()),
        "acos" => V::D(d(0).acos()),
        "atan" => V::D(d(0).atan()),
        "atan2" => V::D(d(0).atan2(d(1))),
        "toRadians" => V::D(d(0).to_radians()),
        "toDegrees" => V::D(d(0).to_degrees()),
        "hypot" => V::D(d(0).hypot(d(1))),
        "signum" => match k {
            'F' => V::F(if a(0).f() == 0.0 { 0.0 } else { a(0).f().signum() }),
            _ => V::D(if d(0) == 0.0 { 0.0 } else { d(0).signum() }),
        },
        "random" => V::D(0.5),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(rt: &mut Rt, key: &'static str, args: &[V]) -> V {
        call(rt, key, args).unwrap_or_else(|| panic!("unmodelled {key}"))
    }

    #[test]
    fn strings_count_utf16_and_hash_as_java() {
        let mut rt = Rt::new();
        let s = rt.string("com.simibubi.create.Create");
        let p = rt.string("com.simibubi.create");
        assert_eq!(c(&mut rt, "java/lang/String.startsWith.(Ljava/lang/String;)Z", &[s, p]), V::I(1));
        // "Fixture".hashCode() on the JVM.
        let f = rt.string("Fixture");
        assert_eq!(c(&mut rt, "java/lang/String.hashCode.()I", &[f]), V::I(821203433));
        let e = rt.string("a\u{1F600}b");
        assert_eq!(c(&mut rt, "java/lang/String.length.()I", &[e]), V::I(4));
        let sub = c(&mut rt, "java/lang/String.substring.(II)Ljava/lang/String;", &[s, V::I(4), V::I(12)]);
        assert_eq!(rt.str_of(sub), Some("simibubi"));
        let dot = rt.string("\\.");
        let parts = c(&mut rt, "java/lang/String.split.(Ljava/lang/String;)[Ljava/lang/String;", &[s, dot]);
        assert_eq!(crate::jutil::elements(&rt, parts).len(), 4);
    }

    #[test]
    fn fixed_rounds_half_up_like_java() {
        assert_eq!(fixed_half_up(0.125, 2), "0.13");
        assert_eq!(fixed_half_up(2.5, 0), "3");
        assert_eq!(fixed_half_up(9.995, 2), "10.00");
        assert_eq!(fixed_half_up(-1.005, 2), "-1.01");
        assert_eq!(fixed_half_up(1e21, 1), "1000000000000000000000.0");
        assert_eq!(fixed_half_up(3.0, 3), "3.000");
    }

    #[test]
    fn builders_and_format() {
        let mut rt = Rt::new();
        let b = rt.new_object("java/lang/StringBuilder");
        c(&mut rt, "java/lang/StringBuilder.<init>.()V", &[b]);
        let x = rt.string("x=");
        c(&mut rt, "java/lang/StringBuilder.append.(Ljava/lang/String;)Ljava/lang/StringBuilder;", &[b, x]);
        c(&mut rt, "java/lang/StringBuilder.append.(I)Ljava/lang/StringBuilder;", &[b, V::I(-3)]);
        c(&mut rt, "java/lang/StringBuilder.append.(D)Ljava/lang/StringBuilder;", &[b, V::D(1.5)]);
        let s = c(&mut rt, "java/lang/StringBuilder.toString.()Ljava/lang/String;", &[b]);
        assert_eq!(rt.str_of(s), Some("x=-31.5"));
        let f = rt.string("%s has %03d rpm (%.2f)");
        let n = rt.string("shaft");
        let i = c(&mut rt, "java/lang/Integer.valueOf.(I)Ljava/lang/Integer;", &[V::I(7)]);
        let d = c(&mut rt, "java/lang/Double.valueOf.(D)Ljava/lang/Double;", &[V::D(0.125)]);
        let arr = rt.alloc("[Ljava/lang/Object;", Data::Array(vec![n, i, d]));
        let out = c(&mut rt, "java/lang/String.format.(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;", &[f, arr]);
        assert_eq!(rt.str_of(out), Some("shaft has 007 rpm (0.13)"));
    }
}

/// Small libraries mods lean on: loggers (dropped), commons-lang `Pair`, `Record`'s constructor.
fn libs(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, rest) = key.split_once('.')?;
    let (name, desc) = rest.split_once('.')?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let logger = matches!(owner, "org/apache/logging/log4j/Logger" | "org/slf4j/Logger" | "java/util/logging/Logger");
    Some(match (owner, name) {
        (_, n) if logger && n.starts_with("is") && n.ends_with("Enabled") => V::I(0),
        (_, _) if logger => {
            if desc.ends_with(")V") { V::Void } else { V::NULL }
        }
        ("org/apache/logging/log4j/LogManager" | "org/slf4j/LoggerFactory" | "com/mojang/logging/LogUtils", "getLogger") => {
            rt.intern("compat:logger".into(), |rt| rt.new_object("org/apache/logging/log4j/Logger"))
        }
        ("org/apache/logging/log4j/MarkerManager", "getMarker") => a(0),
        ("org/apache/commons/lang3/tuple/Pair" | "org/apache/commons/lang3/tuple/ImmutablePair" | "com/mojang/datafixers/util/Pair", "of" | "<init>") => {
            let (p, l, r) = if name == "<init>" { (a(0), a(1), a(2)) } else { (rt.new_object("org/apache/commons/lang3/tuple/Pair"), a(0), a(1)) };
            rt.put_field(p, "$left", l);
            rt.put_field(p, "$right", r);
            if name == "<init>" { V::Void } else { p }
        }
        (o, "getLeft" | "getKey" | "getFirst" | "left") if o.ends_with("/Pair") || o.ends_with("ImmutablePair") => rt.get_field(a(0), "$left"),
        (o, "getRight" | "getValue" | "getSecond" | "right") if o.ends_with("/Pair") || o.ends_with("ImmutablePair") => rt.get_field(a(0), "$right"),
        ("java/lang/Record", "<init>") => V::Void,
        _ => return None,
    })
}
