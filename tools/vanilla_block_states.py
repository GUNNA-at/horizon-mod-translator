#!/usr/bin/env python3
"""Writes horizon-compat's table of Minecraft's block classes' states (`vanilla_block_states.rs`) from the game's jars.

A translated mod's block that extends one of Minecraft's (a sign, a rail, a liquid, a redstone
torch) has the states that class declares: the properties its `createBlockStateDefinition` adds to
the builder, with those of the classes above it where it calls theirs, and the default state its
constructors register. Both are read here from the classes' bytecode:

- the definition: every `BlockStateProperties` field the method hands the builder (a class's own
  constant is followed to the field it was set from in its static initialiser);
- the default: the `setValue(property, value)` calls on the state a constructor gives
  `registerDefaultState`, from `stateDefinition.any()` (every other property keeps its first
  value) or from the default the constructors above left.

A method this cannot read (a loop over faces, a property asked of a virtual method) is told on
stderr and written as given in `BY_HAND`, or left out.

Usage: python3 tools/vanilla_block_states.py <jar>...   (the NeoForge install's client jar first:
its patched classes stand before Minecraft's own)
"""
import os
import struct
import sys
import zipfile

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "crates", "horizon-compat", "src", "vanilla_block_states.rs")
PKG = "net/minecraft/world/level/block/"
BLOCK = PKG + "Block"
PROPS = PKG + "state/properties/BlockStateProperties"

# Definitions the bytecode does not spell out: (properties, None to keep the defaults read).
BY_HAND = {
    # `getShapeProperty()`: the straight shapes.
    "DetectorRailBlock": ["RAIL_SHAPE_STRAIGHT", "POWERED", "WATERLOGGED"],
    "PoweredRailBlock": ["RAIL_SHAPE_STRAIGHT", "POWERED", "WATERLOGGED"],
    # `SLOT_OCCUPIED_PROPERTIES.forEach(builder::add)`.
    "ChiseledBookShelfBlock": ["HORIZONTAL_FACING"] + [f"CHISELED_BOOKSHELF_SLOT_{i}_OCCUPIED" for i in range(6)],
    # A face's property for each direction it supports (all of them, unless a subclass says), in
    # `Direction`'s order.
    "MultifaceBlock": ["DOWN", "UP", "NORTH", "SOUTH", "WEST", "EAST"],
}


def parse(data):
    """A class file: name, superclass, interfaces, fields, methods with code, and its constants."""
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
        elif tag == 3:
            cp[i] = ("int", struct.unpack_from(">i", data, pos)[0])
            pos += 4
        elif tag == 4:
            pos += 4
        elif tag in (5, 6):
            pos += 8
            i += 1
        elif tag in (7, 8):
            cp[i] = ("class" if tag == 7 else "string", struct.unpack_from(">H", data, pos)[0])
            pos += 2
        elif tag in (16, 19, 20):
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

    def const(n):
        if cp[n] is None:
            return None
        kind, v = cp[n][0], cp[n][1]
        return utf(v) if kind == "string" else v if kind == "int" else None

    (access, this, sup, nif) = struct.unpack_from(">HHHH", data, pos)
    interfaces = [cls(struct.unpack_from(">H", data, pos + 8 + 2 * k)[0]) for k in range(nif)]
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
    return {"access": access, "name": cls(this), "super": cls(sup) if sup else None, "interfaces": interfaces, "fields": fields, "methods": methods, "ref": ref, "const": const}


def instructions(code):
    """(pc, opcode, operand) over a method's code: the operand is a constant pool index, a pushed
    integer, or None."""
    one = {0x10, 0x12, 0x15, 0x16, 0x17, 0x18, 0x19, 0x36, 0x37, 0x38, 0x39, 0x3A, 0xA9, 0xBC}
    two = {0x11, 0x13, 0x14, 0x84, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xBB, 0xBD, 0xC0, 0xC1, 0xC6, 0xC7} | set(range(0x99, 0xA9))
    p = 0
    while p < len(code):
        op = code[p]
        if 0x02 <= op <= 0x08:
            arg = op - 0x03
        elif op == 0x10:
            arg = struct.unpack_from(">b", code, p + 1)[0]
        elif op == 0x11:
            arg = struct.unpack_from(">h", code, p + 1)[0]
        elif op == 0x12:
            arg = code[p + 1]
        elif op in two or op in (0xB9, 0xBA):
            arg = struct.unpack_from(">H", code, p + 1)[0]
        else:
            arg = None
        yield p, op, arg
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


GETSTATIC, PUTSTATIC, INVOKEVIRTUAL, INVOKESPECIAL, INVOKESTATIC, INVOKEINTERFACE, INVOKEDYNAMIC = 0xB2, 0xB3, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA
LDC, LDC_W, AASTORE, AALOAD = 0x12, 0x13, 0x53, 0x32
PUSHES = set(range(0x02, 0x09)) | {0x10, 0x11}
BRANCHES = set(range(0x99, 0xA9)) | {0xC6, 0xC7, 0xAA, 0xAB}


def main():
    classes = {}
    for jar in sys.argv[1:]:
        with zipfile.ZipFile(jar) as z:
            for name in z.namelist():
                if name.endswith(".class") and name.startswith("net/minecraft/"):
                    stem = name[: -len(".class")]
                    if stem in classes or not (stem.startswith(PKG) or stem.startswith("net/minecraft/core/Direction") or stem.startswith("net/minecraft/core/FrontAndTop")):
                        continue
                    c = parse(z.read(name))
                    if c:
                        classes[c["name"]] = c

    def is_block(name):
        seen = 0
        while name and seen < 30:
            if name == BLOCK:
                return True
            name = classes[name]["super"] if name in classes else None
            seen += 1
        return False

    # A class's own constant set from another's in its static initialiser, and those it makes
    # itself (an array of them: the brewing stand's bottles).
    alias, arrays = {}, {}
    for c in classes.values():
        for _, mname, _, code in c["methods"]:
            if mname != "<clinit>" or code is None:
                continue
            ins = list(instructions(code))
            held = []
            for i, (p, op, arg) in enumerate(ins):
                if op == GETSTATIC:
                    held.append(c["ref"](arg)[:2])
                elif op == PUTSTATIC:
                    owner, f, d = c["ref"](arg)
                    if i > 0 and ins[i - 1][1] == GETSTATIC:
                        alias[(owner, f)] = c["ref"](ins[i - 1][2])[:2]
                    elif d.startswith("[") and d.endswith("Property;"):
                        arrays[(owner, f)] = list(held)
                    held = []

    def declares(owner, field):
        c = classes.get(owner)
        return c is not None and any(n == field for _, n, _, _ in c["fields"])

    def resolve(owner, field):
        """The `BlockStateProperties` field behind a class's constant (its own, one it inherits
        from a class or an interface above, set from another's), or None."""
        for _ in range(30):
            if owner == PROPS:
                return field
            if not declares(owner, field):
                c = classes.get(owner)
                if c is None:
                    return None
                above = [c["super"]] + c["interfaces"]
                found = None
                todo = [a for a in above if a]
                while todo and found is None:
                    a = todo.pop(0)
                    if declares(a, field):
                        found = a
                    elif a in classes:
                        todo += [x for x in [classes[a]["super"]] + classes[a]["interfaces"] if x]
                if found is None:
                    return None
                owner = found
            if (owner, field) not in alias:
                return None
            owner, field = alias[(owner, field)]
        return None

    # Each enum's constants as its states spell them: the first text its constructor is given
    # after the constant's own name, else the name in lower case.
    spelled = {}
    for c in classes.values():
        if not c["access"] & 0x4000:
            continue
        for _, mname, _, code in c["methods"]:
            if mname != "<clinit>" or code is None:
                continue
            texts = []
            for p, op, arg in instructions(code):
                if op in (LDC, LDC_W):
                    v = c["const"](arg)
                    if isinstance(v, str):
                        texts.append(v)
                elif op == INVOKESPECIAL and c["ref"](arg)[1] == "<init>":
                    pass
                elif op == PUTSTATIC:
                    owner, f, d = c["ref"](arg)
                    if owner == c["name"] and d == f"L{owner};" and texts and texts[0] == f:
                        spelled[(owner, f)] = texts[1] if len(texts) > 1 else f.lower()
                    texts = []

    definitions, odd = {}, []
    for name, c in sorted(classes.items()):
        if not is_block(name):
            continue
        for _, mname, _, code in c["methods"]:
            if mname != "createBlockStateDefinition" or code is None:
                continue
            props, calls_super, unread = [], False, []
            for p, op, arg in instructions(code):
                if op == GETSTATIC:
                    owner, f, d = c["ref"](arg)
                    if d.endswith("Property;") and not d.startswith("["):
                        r = resolve(owner, f)
                        props.append(r) if r else unread.append(f"{owner.rsplit('/', 1)[-1]}.{f}")
                    elif (owner, f) in arrays:
                        for o2, f2 in arrays[(owner, f)]:
                            r = resolve(o2, f2)
                            if r and r not in props:
                                props.append(r)
                    else:
                        unread.append(f"{owner.rsplit('/', 1)[-1]}.{f}")
                elif op == INVOKESPECIAL:
                    if c["ref"](arg)[1] == "createBlockStateDefinition":
                        calls_super = True
                elif op in (INVOKEVIRTUAL, INVOKEINTERFACE, INVOKESTATIC):
                    owner, f, d = c["ref"](arg)
                    if not (f == "add" and owner.endswith("StateDefinition$Builder")):
                        unread.append(f"{f}()")
                elif op == INVOKEDYNAMIC or op in BRANCHES:
                    unread.append("a loop or a lambda")
            simple = name.rsplit("/", 1)[-1]
            if unread and simple not in BY_HAND:
                odd.append(f"{simple}: definition not read ({', '.join(sorted(set(unread)))})")
                continue
            definitions[name] = (BY_HAND.get(simple, props), calls_super and simple not in BY_HAND)

    def definition(name):
        """A class's whole definition: its own method's, after those of the classes above where
        it calls theirs; a class without the method has the one above's."""
        seen = 0
        while name in classes and name not in definitions and seen < 30:
            name = classes[name]["super"]
            seen += 1
        if name not in definitions:
            return []
        props, calls_super = definitions[name]
        if not calls_super:
            return list(props)
        above = definition(classes[name]["super"])
        return above + [p for p in props if p not in above]

    def registered(c):
        """What a class's constructors give `registerDefaultState`: (from the default left by
        those above?, {property: value}), None where they register none, "?" where unread."""
        found = None
        for _, mname, _, code in c["methods"]:
            if mname != "<init>" or code is None:
                continue
            ins = list(instructions(code))
            ends = [i for i, (_, op, arg) in enumerate(ins) if op == INVOKEVIRTUAL and c["ref"](arg)[1] == "registerDefaultState"]
            for end in ends:
                start = None
                for i in range(end - 1, -1, -1):
                    _, op, arg = ins[i]
                    if op in (INVOKEVIRTUAL, INVOKEINTERFACE) and c["ref"](arg)[1] in ("any", "defaultBlockState"):
                        start = i
                        break
                if start is None:
                    return "?"
                keeps = c["ref"](ins[start][2])[1] == "defaultBlockState"
                values, prop, value = {}, None, None
                for _, op, arg in ins[start + 1 : end]:
                    if op == GETSTATIC:
                        owner, f, d = c["ref"](arg)
                        if d.endswith("Property;"):
                            prop = resolve(owner, f)
                            if prop is None:
                                return "?"
                        elif (owner, f) in spelled:
                            value = spelled[(owner, f)]
                        else:
                            return "?"
                    elif op in PUSHES:
                        value = arg
                    elif op == INVOKESTATIC:
                        owner, f, d = c["ref"](arg)
                        if (owner, f) == ("java/lang/Boolean", "valueOf"):
                            value = "true" if value else "false"
                        elif (owner, f) == ("java/lang/Integer", "valueOf"):
                            value = str(value)
                        else:
                            return "?"
                    elif op in (INVOKEVIRTUAL, INVOKEINTERFACE):
                        if c["ref"](arg)[1] != "setValue" or prop is None or value is None:
                            return "?"
                        values[prop] = value
                        prop, value = None, None
                    elif op in (0xC0, 0x2A):  # checkcast, aload_0
                        pass
                    else:
                        return "?"
                this = (keeps, values)
                if found is not None and found != this:
                    return "?"
                found = this
        return found

    defaults_of = {name: registered(c) for name, c in classes.items() if is_block(name)}

    def default(name):
        """The values a class's constructors leave its properties at, from `Block`'s down (every
        property at its first value there), or None where one of them is unread."""
        chain = []
        while name in classes and name != BLOCK:
            chain.append(name)
            name = classes[name]["super"]
        values = {}
        for n in reversed(chain):
            r = defaults_of.get(n)
            if r == "?":
                return None
            if r is not None:
                keeps, own = r
                values = {**values, **own} if keeps else dict(own)
        return values

    rows, simple_names = [], {}
    for name in sorted(classes):
        if not is_block(name):
            continue
        props = definition(name)
        if not props:
            continue
        simple = name.rsplit("/", 1)[-1]
        if simple in simple_names:
            sys.exit(f"two block classes named {simple}: {simple_names[simple]} and {name}")
        simple_names[simple] = name
        values = default(name)
        if values is None:
            odd.append(f"{simple}: default not read")
            given = "?"
        else:
            given = ",".join(f"{p}={values[p]}" for p in props if p in values)
        rows.append((simple, ",".join(props), given))
    rows.sort()
    lines = [
        "//! Generated by `tools/vanilla_block_states.py` from the game's jars (Minecraft 1.21.1 as the",
        "//! NeoForge install patches it): what each of Minecraft's block classes puts in its state",
        "//! definition, its own `createBlockStateDefinition`'s and those above it that it calls, as",
        "//! `BlockStateProperties` fields in order; and the values its constructors register for them",
        "//! (`FIELD=value`; a property not named keeps its first value, as `StateDefinition.any()` has",
        "//! it; `?` where the constructors' code was not read). Sorted by class.",
        "",
        f"pub(crate) const BLOCK_STATES: [(&str, &str, &str); {len(rows)}] = [",
    ]
    lines += [f'    ("{c}", "{p}", "{d}"),' for c, p, d in rows]
    lines += ["];"]
    with open(OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    for line in odd:
        print(line, file=sys.stderr)
    print(f"{len(rows)} block classes with states ({sum(1 for r in rows if r[2] == '?')} with an unread default) -> {os.path.normpath(OUT)}")


if __name__ == "__main__":
    main()
