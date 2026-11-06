#!/usr/bin/env python3
"""Writes horizon-compat's table of vanilla blocks' and items' classes (`vanilla_classes.rs`) from
NeoForge's merged jar.

A mod asks what kind of block or item it holds by its class: `state.getBlock() instanceof
LeavesBlock`, `instanceof BonemealableBlock`, `stack.getItem() instanceof SwordItem`. The runtime
stands for a vanilla block or item by its id, so it needs the class Java has for each id:

- `Blocks`' static initialiser registers each block, `register("ice", new IceBlock(...))`, or
  through one of `Blocks`' own helpers (`log(...)` makes a `RotatedPillarBlock`);
- `Items`' registers each item by name (`registerItem("diamond_sword", new SwordItem(...))`) or by
  its block (`registerBlock(Blocks.TORCH)` is a `BlockItem`, `registerBlock(new
  StandingAndWallBlockItem(Blocks.TORCH, ...))` that class), the block's field naming the id;
- the classes' own superclasses and interfaces, up to `Object`.

A mod is built against the jar's version, so the tables are that version's. Blocks of class `Block`,
items of class `Item` and blocks' plain `BlockItem`s are left out: they are what the runtime makes
anyway.

Needs `javap` (a JDK). Usage: python3 tools/vanilla_classes.py path/to/neoforge-<version>-merged.jar
"""
import re
import subprocess
import sys

OUT = "crates/horizon-compat/src/vanilla_classes.rs"
BLOCK = "net/minecraft/world/level/block/Block"
ITEM = "net/minecraft/world/item/Item"
BLOCK_ITEM = "net/minecraft/world/item/BlockItem"

STRING = re.compile(r"// String (\S+)$")
NEW_DONE = re.compile(r'invokespecial .*// Method (\S+)\."<init>":')
OWN_CALL = re.compile(r"invokestatic .*// Method (\w+):(\(\S*\)\S+)$")
PUT_OWN = re.compile(r"putstatic .*// Field (\w+):")
GET_BLOCK = re.compile(r"getstatic .*// Field net/minecraft/world/level/block/Blocks\.(\w+):")
# A factory of another class (`SmithingTemplateItem.createNetheriteUpgradeTemplate()`): what it returns.
FACTORY = re.compile(r"invokestatic .*// Method net/minecraft/\S+\.\w+:\(\S*\)L(net/minecraft/world/(?:item/\w+Item|level/block/\w+Block));$")
GET_KEY = re.compile(r"getstatic .*// Field net/minecraft/references/\w+\.(\w+):Lnet/minecraft/resources/ResourceKey;")


def javap(jar, *classes, code=False):
    args = ["javap", "-p"] + (["-c"] if code else []) + ["-cp", jar] + list(classes)
    return subprocess.run(args, capture_output=True, text=True, check=True).stdout


def methods(text):
    """Each method's name, descriptor-less header and its instruction lines."""
    out, name, body = {}, None, []
    for line in text.splitlines():
        if line.startswith("  ") and not line.startswith("   ") and line.rstrip().endswith(";"):
            if name is not None:
                out.setdefault(name, []).append(body)
            head = line.strip()
            name = "<clinit>" if head == "static {};" else re.search(r"(\w+)\(", head).group(1) if "(" in head else None
            body = []
        elif name is not None and re.match(r"\s+\d+: ", line):
            body.append(line)
    if name is not None:
        out.setdefault(name, []).append(body)
    return out


def helper_classes(bodies):
    """The class a static helper returns: the last object it constructs (`log` a `RotatedPillarBlock`)."""
    made = {}
    for name, overloads in bodies.items():
        if name == "<clinit>":
            continue
        for body in overloads:
            done = [m.group(1) for line in body if (m := NEW_DONE.search(line))]
            if done and body and body[-1].strip().endswith("areturn"):
                made.setdefault(name, set()).add(done[-1])
    return {n: c.pop() for n, c in made.items() if len(c) == 1}


def segments(body):
    """The static initialiser cut at each write of the class's own fields: one field's expression each."""
    seg = []
    for line in body:
        seg.append(line)
        if m := PUT_OWN.search(line):
            yield m.group(1), seg
            seg = []


def keys(jar, cls):
    """`net.minecraft.references`' registry keys of class `cls` (`Blocks.PUMPKIN`), by field: the id each names."""
    bodies = methods(javap(jar, f"net.minecraft.references.{cls}", code=True))
    out = {}
    for field, seg in segments(bodies["<clinit>"][0]):
        names = [m.group(1) for line in seg if (m := STRING.search(line))]
        if len(names) != 1:
            sys.exit(f"references.{cls}.{field}: {names}")
        out[field] = names[0]
    return out


def blocks(jar):
    text = javap(jar, "net.minecraft.world.level.block.Blocks", code=True)
    bodies = methods(text)
    helpers = helper_classes(bodies)
    by_key = keys(jar, "Blocks")
    by_field, classes = {}, {}
    for field, seg in segments(bodies["<clinit>"][0]):
        name, key, made = None, None, None
        for line in seg:
            if name is None and (m := STRING.search(line)):
                name = m.group(1)
            elif key is None and (m := GET_KEY.search(line)):
                key = m.group(1)
            elif m := NEW_DONE.search(line) or FACTORY.search(line):
                made = m.group(1)
            elif m := OWN_CALL.search(line):
                call, desc = m.groups()
                if call == "register":
                    if desc.startswith("(Lnet/minecraft/resources/ResourceKey;") and key in by_key:
                        name = by_key[key]
                    elif not desc.startswith("(Ljava/lang/String;"):
                        sys.exit(f"Blocks.{field}: register{desc}")
                    if name is None or made is None:
                        sys.exit(f"Blocks.{field}: no name or block")
                    by_field[field] = f"minecraft:{name}"
                    classes[f"minecraft:{name}"] = made
                elif call in helpers:
                    made = helpers[call]
    return by_field, classes


def items(jar, block_ids):
    text = javap(jar, "net.minecraft.world.item.Items", code=True)
    bodies = methods(text)
    by_key = keys(jar, "Items")
    classes = {}
    for field, seg in segments(bodies["<clinit>"][0]):
        name, key, block, made, reg = None, None, None, None, None
        for line in seg:
            if name is None and (m := STRING.search(line)):
                name = m.group(1)
            if key is None and (m := GET_KEY.search(line)):
                key = m.group(1)
            if block is None and (m := GET_BLOCK.search(line)):
                block = m.group(1)
            if m := NEW_DONE.search(line) or FACTORY.search(line):
                made = m.group(1)
            if (m := OWN_CALL.search(line)) and m.group(1) in ("registerBlock", "registerItem"):
                reg = m.groups()
        if reg is None:
            continue
        call, desc = reg
        if call == "registerItem" and desc.startswith("(Lnet/minecraft/resources/ResourceKey;") and key in by_key:
            name = by_key[key]
        if call == "registerItem":
            if made is None or name is None or not desc.startswith(("(Ljava/lang/String;", "(Lnet/minecraft/resources/ResourceKey;")):
                sys.exit(f"Items.{field}: registerItem{desc}")
            classes[f"minecraft:{name}"] = made
            continue
        if block is None or block not in block_ids:
            sys.exit(f"Items.{field}: no block for registerBlock{desc}")
        if desc in ("(Lnet/minecraft/world/level/block/Block;)Lnet/minecraft/world/item/Item;",
                    "(Lnet/minecraft/world/level/block/Block;Ljava/util/function/UnaryOperator;)Lnet/minecraft/world/item/Item;"):
            made = BLOCK_ITEM
        elif desc == "(Lnet/minecraft/world/level/block/Block;[Lnet/minecraft/world/level/block/Block;)Lnet/minecraft/world/item/Item;":
            # `Items$1`: a `BlockItem` that also places other blocks (the infested ones' items).
            made = BLOCK_ITEM
        elif made is None:
            sys.exit(f"Items.{field}: registerBlock{desc} without an item")
        classes[block_ids[block]] = made
    return classes


def strip_generics(s):
    out, depth = [], 0
    for ch in s:
        if ch == "<":
            depth += 1
        elif ch == ">":
            depth -= 1
        elif depth == 0:
            out.append(ch)
    return "".join(out)


HEADER = re.compile(r"^(?:[a-z-]+ )*(class|interface|enum) (\S+?)(?: extends (.+?))?(?: implements (.+?))?(?: permits .+?)? \{$")


def hierarchy(jar, start):
    """Each class reachable from `start` by superclass and interface: (superclass or "", interfaces)."""
    known, todo = {}, sorted(set(start))
    while todo:
        text = javap(jar, *[t.replace("/", ".") for t in todo])
        for line in text.splitlines():
            m = HEADER.match(strip_generics(line.strip()))
            if not m:
                continue
            kind, name, ext, impl = m.groups()
            name = name.replace(".", "/")
            names = lambda s: [x.strip().replace(".", "/") for x in s.split(",")] if s else []
            if kind == "interface":
                sup, ifs = "", names(ext)
            else:
                sup = (names(ext) or [""])[0]
                ifs = names(impl)
            if sup == "java/lang/Object" or kind == "enum":
                sup = ""
            known[name] = (sup, ifs)
        missing = [t for t in todo if t not in known]
        if missing:
            sys.exit(f"no header for {missing[:5]}")
        todo = sorted({c for s, i in known.values() for c in [s, *i] if c and c not in known})
    return known


def main():
    jar = sys.argv[1]
    block_ids, block_classes = blocks(jar)
    item_classes = items(jar, block_ids)
    classes = hierarchy(jar, set(block_classes.values()) | set(item_classes.values()))
    for c in [*block_classes.values(), *item_classes.values()]:
        k = c
        while k and k not in (BLOCK, ITEM):
            k = classes[k][0]
        if not k:
            sys.exit(f"{c} is neither a block nor an item")
    blocks_out = sorted((i, c) for i, c in block_classes.items() if c != BLOCK)
    # A block's plain `BlockItem` is left out too: the runtime makes one where the game says the
    # item places a block.
    items_out = sorted((i, c) for i, c in item_classes.items() if c not in (ITEM, BLOCK_ITEM))
    lines = [
        "//! Generated by `tools/vanilla_classes.py` from NeoForge's merged jar: the class Java has for",
        "//! each vanilla block and item (`minecraft:ice` an `IceBlock`, `minecraft:diamond_sword` a",
        "//! `SwordItem`), plain `Block`s, `Item`s and `BlockItem`s left out, and those classes'",
        "//! superclasses and interfaces. Sorted by id and by class for binary search.",
        "",
        "/// 1.21.1's blocks whose class isn't `Block`, by id: the class each is made of.",
        f"pub(crate) const BLOCKS: [(&str, &str); {len(blocks_out)}] = [",
        *[f'    ("{i}", "{c}"),' for i, c in blocks_out],
        "];",
        "",
        "/// 1.21.1's items whose class is neither `Item` nor `BlockItem`, by id.",
        f"pub(crate) const ITEMS: [(&str, &str); {len(items_out)}] = [",
        *[f'    ("{i}", "{c}"),' for i, c in items_out],
        "];",
        "",
        "/// Each class: its superclass (\"\" past `Object`, and for an interface) and its interfaces.",
        f"pub(crate) const CLASSES: [(&str, &str, &[&str]); {len(classes)}] = [",
        *[f'    ("{c}", "{s}", &[{", ".join(chr(34) + i + chr(34) for i in ifs)}]),' for c, (s, ifs) in sorted(classes.items())],
        "];",
    ]
    with open(OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    print(f"{len(blocks_out)} of {len(block_classes)} blocks, {len(items_out)} of {len(item_classes)} items, {len(classes)} classes -> {OUT}")


main()
