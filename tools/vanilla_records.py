#!/usr/bin/env python3
"""Writes horizon-compat's tables of vanilla's plain data (`vanilla_records.rs`) from NeoForge's merged jar.

A translated mod builds vanilla records (`StructureTemplate.StructureBlockInfo`) with their canonical
constructor and reads them back through their accessors; the runtime answers both from the components
each record declares (its class file's `Record` attribute).

Their static factories that only call a constructor with their own arguments (`GlobalPos.of`) too.

The same for vanilla's plain data classes (`Tuple`, `LevelResource`, `ModelLayerLocation`): a class
extending `Object` whose constructor does nothing but store its arguments in its fields, read back by
getters that only return a field and written by setters that only set one, as their bytecode shows.
Their `equals` and `hashCode` are `Object`'s when the class doesn't override them, and field by field
when both overrides read exactly the instance fields the constructor sets. Classes the runtime models
by hand (named in horizon-compat's sources) are left out: their own model answers.

And the enums, whose `equals` and `hashCode` are `Enum`'s (final: identity), with the constants of
those the runtime doesn't model by hand, in ordinal order.

Usage: python3 tools/vanilla_records.py path/to/neoforge-<version>-merged.jar
"""
import glob
import os
import struct
import sys
import zipfile


def parse(data):
    if data[:4] != b"\xca\xfe\xba\xbe":
        return None
    pos = 8
    (count,) = struct.unpack_from(">H", data, pos)
    pos += 2
    cp = [None] * count
    i = 1
    while i < count:
        tag = data[pos]
        pos += 1
        if tag == 1:
            (n,) = struct.unpack_from(">H", data, pos)
            pos += 2
            cp[i] = ("utf8", data[pos : pos + n].decode("utf-8", "replace"))
            pos += n
        elif tag in (3, 4):
            pos += 4
        elif tag in (5, 6):
            pos += 8
            i += 1
        elif tag == 7:
            (n,) = struct.unpack_from(">H", data, pos)
            cp[i] = ("class", n)
            pos += 2
        elif tag in (8, 16, 19, 20):
            pos += 2
        elif tag in (9, 10, 11, 12, 17, 18):
            pos += 4
        elif tag == 15:
            pos += 3
        else:
            return None
        i += 1

    def utf(n):
        return cp[n][1]

    def cls(n):
        return utf(cp[n][1])

    pos += 2  # access
    (this, sup, nif) = struct.unpack_from(">HHH", data, pos)
    pos += 6 + 2 * nif
    if sup == 0 or cls(sup) != "java/lang/Record":
        return None

    def skip_members(pos):
        (n,) = struct.unpack_from(">H", data, pos)
        pos += 2
        for _ in range(n):
            pos += 6
            (na,) = struct.unpack_from(">H", data, pos)
            pos += 2
            for _ in range(na):
                (_, ln) = struct.unpack_from(">HI", data, pos)
                pos += 6 + ln
        return pos

    pos = skip_members(pos)  # fields
    pos = skip_members(pos)  # methods
    (na,) = struct.unpack_from(">H", data, pos)
    pos += 2
    for _ in range(na):
        (name, ln) = struct.unpack_from(">HI", data, pos)
        body = pos + 6
        pos = body + ln
        if utf(name) != "Record":
            continue
        (nc,) = struct.unpack_from(">H", data, body)
        p = body + 2
        comps = []
        for _ in range(nc):
            (cn, cd, cna) = struct.unpack_from(">HHH", data, p)
            p += 6
            for _ in range(cna):
                (_, ln2) = struct.unpack_from(">HI", data, p)
                p += 6 + ln2
            comps.append((utf(cn), utf(cd)))
        return cls(this), comps
    return cls(this), []


PREFIXES = ("net/minecraft/", "com/mojang/", "net/neoforged/")
OUT = "crates/horizon-compat/src/vanilla_records.rs"
ACC_STATIC, ACC_INTERFACE, ACC_ANNOTATION, ACC_ENUM = 0x0008, 0x0200, 0x2000, 0x4000


def parse_class(data):
    """A class file's name, access, superclass, fields and methods (with their code), and a
    resolver of its field and method references, or None."""
    if data[:4] != b"\xca\xfe\xba\xbe":
        return None
    pos = 8
    (count,) = struct.unpack_from(">H", data, pos)
    pos += 2
    cp = [None] * count
    i = 1
    while i < count:
        tag = data[pos]
        pos += 1
        if tag == 1:
            (n,) = struct.unpack_from(">H", data, pos)
            pos += 2
            cp[i] = ("utf8", data[pos : pos + n].decode("utf-8", "replace"))
            pos += n
        elif tag in (3, 4):
            pos += 4
        elif tag in (5, 6):
            pos += 8
            i += 1
        elif tag == 7:
            (n,) = struct.unpack_from(">H", data, pos)
            cp[i] = ("class", n)
            pos += 2
        elif tag in (8, 16, 19, 20):
            pos += 2
        elif tag in (9, 10, 11):
            cp[i] = ("ref",) + struct.unpack_from(">HH", data, pos)
            pos += 4
        elif tag == 12:
            cp[i] = ("nt",) + struct.unpack_from(">HH", data, pos)
            pos += 4
        elif tag in (17, 18):
            pos += 4
        elif tag == 15:
            pos += 3
        else:
            return None
        i += 1

    def utf(n):
        return cp[n][1]

    def cls(n):
        return utf(cp[n][1])

    def ref(n):
        _, c, nt = cp[n]
        _, nn, dd = cp[nt]
        return cls(c), utf(nn), utf(dd)

    (access, this, sup, nif) = struct.unpack_from(">HHHH", data, pos)
    pos += 8 + 2 * nif

    def members(pos):
        (n,) = struct.unpack_from(">H", data, pos)
        pos += 2
        out = []
        for _ in range(n):
            (acc, nm, ds, na) = struct.unpack_from(">HHHH", data, pos)
            pos += 8
            code = None
            for _ in range(na):
                (an, ln) = struct.unpack_from(">HI", data, pos)
                if utf(an) == "Code":
                    (clen,) = struct.unpack_from(">I", data, pos + 10)
                    code = data[pos + 14 : pos + 14 + clen]
                pos += 6 + ln
            out.append((acc, utf(nm), utf(ds), code))
        return pos, out

    pos, fields = members(pos)
    pos, methods = members(pos)
    return {"access": access, "name": cls(this), "super": cls(sup) if sup else None, "fields": fields, "methods": methods, "ref": ref, "cls_ref": cls}


def params(desc):
    """A method descriptor's parameter descriptors."""
    out, i = [], 1
    while desc[i] != ")":
        j = i
        while desc[j] == "[":
            j += 1
        if desc[j] == "L":
            j = desc.index(";", j)
        out.append(desc[i : j + 1])
        i = j + 1
    return out


# iload..aload with an index byte, then their _0.._3 forms.
LOADS = {0x15, 0x16, 0x17, 0x18, 0x19}
SHORT_LOADS = {base + k: k for base in (0x1A, 0x1E, 0x22, 0x26, 0x2A) for k in range(4)}
RETURNS = {0xAC, 0xAD, 0xAE, 0xAF, 0xB0}
ALOAD_0, INVOKESPECIAL, GETFIELD, PUTFIELD, RETURN = 0x2A, 0xB7, 0xB4, 0xB5, 0xB1


def load(code, p):
    """(local slot, next pc) when the instruction at `p` loads a local."""
    if p < len(code) and code[p] in SHORT_LOADS:
        return SHORT_LOADS[code[p]], p + 1
    if p + 1 < len(code) and code[p] in LOADS:
        return code[p + 1], p + 2
    return None


def pure_init(c, desc, code):
    """The fields a constructor stores its parameters in (in parameter order) when that is all it
    does after `Object()`, or None."""
    if code is None or len(code) < 5 or code[0] != ALOAD_0 or code[1] != INVOKESPECIAL:
        return None
    if c["ref"](struct.unpack_from(">H", code, 2)[0]) != ("java/lang/Object", "<init>", "()V"):
        return None
    ps = params(desc)
    slots, s = {}, 1
    for i, d in enumerate(ps):
        slots[s] = i
        s += 2 if d in ("J", "D") else 1
    stored = [None] * len(ps)
    p = 4
    while True:
        if p == len(code) - 1 and code[p] == RETURN:
            break
        if p >= len(code) or code[p] != ALOAD_0:
            return None
        r = load(code, p + 1)
        if r is None or r[1] + 2 >= len(code) or code[r[1]] != PUTFIELD:
            return None
        slot, p = r
        owner, field, fdesc = c["ref"](struct.unpack_from(">H", code, p + 1)[0])
        p += 3
        if owner != c["name"] or slot not in slots or stored[slots[slot]] is not None or fdesc != ps[slots[slot]]:
            return None
        stored[slots[slot]] = field
    return stored if None not in stored else None


def getter(c, desc, code):
    """The field a method returns, when that is all it does."""
    if code is None or len(code) != 5 or code[0] != ALOAD_0 or code[1] != GETFIELD or code[4] not in RETURNS:
        return None
    owner, field, fdesc = c["ref"](struct.unpack_from(">H", code, 2)[0])
    return field if owner == c["name"] and desc == "()" + fdesc else None


def setter(c, desc, code):
    """The field a one-argument void method sets, when that is all it does."""
    if code is None or not desc.endswith(")V") or len(params(desc)) != 1 or not code or code[0] != ALOAD_0:
        return None
    r = load(code, 1)
    if r is None or r[0] != 1 or r[1] + 4 != len(code) or code[r[1]] != PUTFIELD or code[-1] != RETURN:
        return None
    owner, field, fdesc = c["ref"](struct.unpack_from(">H", code, r[1] + 1)[0])
    return field if owner == c["name"] and fdesc == params(desc)[0] else None


def instructions(code):
    """(pc, opcode) over a method's code."""
    one = {0x10, 0x12, 0x15, 0x16, 0x17, 0x18, 0x19, 0x36, 0x37, 0x38, 0x39, 0x3A, 0xA9, 0xBC}
    two = {0x11, 0x13, 0x14, 0x84, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xBB, 0xBD, 0xC0, 0xC1, 0xC6, 0xC7} | set(range(0x99, 0xA9))
    p = 0
    while p < len(code):
        op = code[p]
        yield p, op
        if op in one:
            p += 2
        elif op in two:
            p += 3
        elif op in (0xB9, 0xBA, 0xC8, 0xC9):
            p += 5
        elif op == 0xC5:
            p += 4
        elif op == 0xC4:
            p += 6 if code[p + 1] == 0x84 else 4
        elif op in (0xAA, 0xAB):
            q = (p + 4) & ~3
            if op == 0xAA:
                lo, hi = struct.unpack_from(">ii", code, q + 4)
                p = q + 12 + 4 * (hi - lo + 1)
            else:
                (n,) = struct.unpack_from(">i", code, q + 4)
                p = q + 8 + 8 * n
        else:
            p += 1


NEW, DUP, ARETURN = 0xBB, 0x59, 0xB0


def factory(c, desc, code):
    """The constructor descriptor a static method only calls with its own arguments, in order, and
    returns (`GlobalPos.of(dimension, pos)` is `new GlobalPos(dimension, pos)`), or None."""
    if code is None or len(code) < 8 or code[0] != NEW or code[3] != DUP:
        return None
    if c["cls_ref"](struct.unpack_from(">H", code, 1)[0]) != c["name"] or not desc.endswith(")L" + c["name"] + ";"):
        return None
    ps = params(desc)
    p, slot = 4, 0
    for d in ps:
        r = load(code, p)
        if r is None or r[0] != slot:
            return None
        p = r[1]
        slot += 2 if d in ("J", "D") else 1
    if p + 4 != len(code) or code[p] != INVOKESPECIAL or code[p + 3] != ARETURN:
        return None
    owner, name, init = c["ref"](struct.unpack_from(">H", code, p + 1)[0])
    if owner != c["name"] or name != "<init>" or params(init) != ps:
        return None
    return init


def factories(c):
    """`<name><desc>=<constructor desc>` for each of a class's static factories."""
    out = []
    for acc, name, desc, code in c["methods"]:
        if acc & ACC_STATIC and name != "<clinit>" and (init := factory(c, desc, code)) is not None:
            out.append(f"{name}{desc}={init}")
    return out


def own_fields_read(c, code):
    return {c["ref"](struct.unpack_from(">H", code, p + 1)[0])[1] for p, op in instructions(code) if op == GETFIELD and c["ref"](struct.unpack_from(">H", code, p + 1)[0])[0] == c["name"]}


def data_class(c):
    """A plain data class's members (`i<desc>=<fields>`, `g<name><desc>=<field>`,
    `s<name><desc>=<field>`, then `o` for `Object`'s equality or `e=<fields>` for field by field,
    when one constructor sets every instance field), or None."""
    if c["access"] & (ACC_INTERFACE | ACC_ANNOTATION | ACC_ENUM) or c["super"] != "java/lang/Object":
        return None
    instance = sorted(n for acc, n, _, _ in c["fields"] if not acc & ACC_STATIC)
    out, overrides, whole = [], {}, None
    for acc, name, desc, code in c["methods"]:
        if acc & ACC_STATIC:
            continue
        if name == "<init>":
            fields = pure_init(c, desc, code)
            if fields is not None:
                out.append(f"i{desc}=" + ",".join(fields))
                if whole is None and sorted(fields) == instance:
                    whole = fields
        elif (name, desc) in (("equals", "(Ljava/lang/Object;)Z"), ("hashCode", "()I")):
            overrides[name] = code
        elif (f := getter(c, desc, code)) is not None:
            out.append(f"g{name}{desc}={f}")
        elif (f := setter(c, desc, code)) is not None:
            out.append(f"s{name}{desc}={f}")
    if not any(m.startswith("i") and not m.endswith("=") for m in out):
        return None
    if not overrides:
        out.append("o")
    elif (
        len(overrides) == 2
        and all(code and sorted(own_fields_read(c, code)) == instance for code in overrides.values())
        and whole
    ):
        # In the constructor's order: the order the overrides usually combine them in.
        out.append("e=" + ",".join(whole))
    return "|".join(out)


def modelled_by_hand(classes):
    """The classes horizon-compat's own modules name (their code, not their tests; past the
    generated tables and `records.rs`, which answers from them)."""
    text = ""
    for path in glob.glob("crates/horizon-compat/src/*.rs") + glob.glob("crates/horizon-compat-core/src/*.rs"):
        if os.path.basename(path) not in (os.path.basename(OUT), "records.rs"):
            text += open(path, encoding="utf-8").read().split("#[cfg(test)]")[0]
    return {c for c in classes if f'"{c}"' in text or f'"{c}.' in text or f"{c}\"" in text}


def main():
    jar = sys.argv[1]
    found, data, enums, constants, made_by = [], {}, [], {}, {}
    with zipfile.ZipFile(jar) as z:
        for name in z.namelist():
            if not (name.endswith(".class") and name.startswith(PREFIXES)):
                continue
            raw = z.read(name)
            r = parse(raw)
            c = parse_class(raw)
            if r:
                found.append(r)
                if c is not None and (f := factories(c)):
                    made_by[c["name"]] = f
                continue
            if c is None:
                continue
            if c["access"] & ACC_ENUM:
                enums.append(c["name"])
                # Its constants: the fields flagged as enum constants, in declaration (ordinal) order.
                constants[c["name"]] = [n for acc, n, _, _ in c["fields"] if acc & ACC_ENUM]
            elif (m := data_class(c)) is not None:
                data[c["name"]] = m
                if f := factories(c):
                    made_by[c["name"]] = f
    found.sort()
    enums.sort()
    for c in modelled_by_hand(data):
        del data[c]
    # Factories of the records and data classes answered here (a record the runtime models by hand
    # answers its own: it comes first).
    made_by = {c: f for c, f in made_by.items() if c in data or c not in modelled_by_hand(made_by)}
    # An enum the runtime models by hand keeps its own constants (directions are natives).
    for c in modelled_by_hand(constants):
        del constants[c]
    lines = [
        "//! Generated by `tools/vanilla_records.py` from NeoForge's merged jar: vanilla's and NeoForge's",
        "//! records, each with its components (`name:descriptor`, in order); plain data classes, with",
        "//! the members that only store and read their fields; enums. Sorted by class.",
        "",
        f"pub(crate) const RECORDS: [(&str, &str); {len(found)}] = [",
    ]
    for c, comps in found:
        lines.append(f'    ("{c}", "' + ",".join(f"{n}:{d}" for n, d in comps) + '"),')
    lines.append("];")
    lines.append("")
    lines.append("/// (class, members): `i<desc>=<fields>` a constructor storing its arguments, `g<name><desc>=<field>` a")
    lines.append("/// getter, `s<name><desc>=<field>` a setter, then `o` (`Object`'s `equals` and `hashCode`) or")
    lines.append("/// `e=<fields>` (both field by field), or neither (overridden otherwise); `|` between members.")
    lines.append(f"pub(crate) const DATA_CLASSES: [(&str, &str); {len(data)}] = [")
    for c in sorted(data):
        lines.append(f'    ("{c}", "{data[c]}"),')
    lines.append("];")
    lines.append("")
    lines.append(f"pub(crate) const ENUMS: [&str; {len(enums)}] = [")
    for c in enums:
        lines.append(f'    "{c}",')
    lines.append("];")
    lines.append("")
    lines.append("/// (class, factories): the static methods of the records and data classes above that only build")
    lines.append("/// one from their arguments (`<name><desc>=<constructor desc>`, `|` between them).")
    lines.append(f"pub(crate) const FACTORIES: [(&str, &str); {len(made_by)}] = [")
    for c in sorted(made_by):
        lines.append(f'    ("{c}", "' + "|".join(made_by[c]) + '"),')
    lines.append("];")
    lines.append("")
    lines.append("/// The constants of the enums the runtime doesn't model by hand, in ordinal order.")
    kept = sorted(c for c in constants if constants[c])
    lines.append(f"pub(crate) const ENUM_CONSTANTS: [(&str, &str); {len(kept)}] = [")
    for c in kept:
        lines.append(f'    ("{c}", "' + ",".join(constants[c]) + '"),')
    lines.append("];")
    with open(OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    print(f"{len(found)} records, {len(data)} data classes, {len(made_by)} with factories, {len(enums)} enums ({len(kept)} with their constants) -> {OUT}")


main()
