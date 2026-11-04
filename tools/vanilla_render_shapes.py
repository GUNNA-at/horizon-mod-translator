#!/usr/bin/env python3
"""Writes horizon-compat's table of vanilla render shapes (`vanilla_shapes.rs`) from NeoForge's merged jar.

A mod asks a state how it is drawn (`state.getRenderShape()`): Create tesselates the blocks of a
contraption whose shape is `MODEL` and leaves the others to their block entity renderers. A mod is
built against the jar's version, so the answers are that version's: each block's class, from
`Blocks`' static initialiser, and the `RenderShape` that class's `getRenderShape` (or the nearest
superclass's) returns. Only the blocks and classes that aren't `MODEL` are listed.

Needs `javap` (a JDK). Usage: python3 tools/vanilla_render_shapes.py path/to/neoforge-<version>-merged.jar
"""
import re
import subprocess
import sys
import zipfile

OUT = "crates/horizon-compat/src/vanilla_shapes.rs"
BLOCK_PKG = "net/minecraft/world/level/block/"
# `RenderShape`'s constants in the jar's version, by ordinal.
SHAPES = ["INVISIBLE", "ENTITYBLOCK_ANIMATED", "MODEL"]


def javap(jar, classes):
    """Each class's disassembly, from one `javap` run over all of them."""
    r = subprocess.run(["javap", "-c", "-p", "-cp", jar, *[c.replace("/", ".") for c in classes]], capture_output=True, text=True)
    out, cur = {}, None
    for line in r.stdout.splitlines():
        if line.startswith("Compiled from"):
            cur = None
            continue
        m = re.search(r"(?:class|interface|enum) (\S+)", plain(line))
        if cur is None and m:
            cur = m.group(1).replace(".", "/")
            out[cur] = []
        if cur is not None:
            out[cur].append(line)
    return {c: "\n".join(lines) for c, lines in out.items()}


def methods(text):
    """The class's methods: header line -> body lines."""
    out, cur = {}, None
    for line in text.splitlines():
        if line.startswith("  ") and not line.startswith("   ") and line.rstrip().endswith(";"):
            cur = line.strip()
            out[cur] = []
        elif cur is not None and line.startswith("   "):
            out[cur].append(line)
        elif not line.strip():
            cur = None
    return out


def plain(line):
    """A class header without its type parameters and arguments (`ChestBlock<E extends X>`)."""
    while True:
        stripped = re.sub(r"<[^<>]*>", "", line)
        if stripped == line:
            return line
        line = stripped


def superclass(text):
    head = next((plain(line) for line in text.splitlines() if re.search(r"\bclass ", line)), "")
    m = re.search(r"class \S+ extends (\S+)", head)
    return m.group(1).replace(".", "/") if m else None


def main():
    jar = sys.argv[1]
    cache = {}

    # Every class of the block package and below, in one pass: the blocks' own and the ones a mod's
    # blocks extend.
    with zipfile.ZipFile(jar) as z:
        names = [n[:-6] for n in z.namelist() if n.startswith(BLOCK_PKG) and n.endswith(".class")]
    for i in range(0, len(names), 200):
        cache.update(javap(jar, names[i : i + 200]))

    def load(cls):
        if cls not in cache:
            cache.update(javap(jar, [cls]))
        return cache.get(cls, "")

    shape_of = {}

    def shape(cls):
        """The shape `cls`'s blocks have: its own `getRenderShape`, else its superclass's."""
        if cls in shape_of:
            return shape_of[cls]
        if cls in ("net/minecraft/world/level/block/state/BlockBehaviour", "java/lang/Object", None):
            return "MODEL"
        text = load(cls)
        found = None
        for head, body in methods(text).items():
            if "getRenderShape(net.minecraft.world.level.block.state.BlockState)" in head:
                consts = sorted({m for line in body for m in re.findall(r"RenderShape\.([A-Z_]+)", line)})
                if len(consts) == 1:
                    found = consts[0]
                else:
                    print(f"warning: {cls}.getRenderShape answers {consts or 'by a call'}; kept as MODEL", file=sys.stderr)
                    found = "MODEL"
        if found is None:
            found = shape(superclass(text))
        shape_of[cls] = found
        return found

    blocks = load("net/minecraft/world/level/block/Blocks")
    ms = methods(blocks)

    def built_in(head):
        """The block class a `Blocks` helper (`bed(DyeColor)`, `shulkerBox(...)`) makes."""
        for line in ms.get(head, []):
            m = re.search(r"// class (\S+)", line)
            if " new " in line and m and m.group(1).startswith(BLOCK_PKG):
                return m.group(1)
        return None

    helper_heads = {re.search(r" (\w+)\(", h).group(1): h for h in ms if re.search(r" (\w+)\(", h)}
    init = next(body for head, body in ms.items() if head.startswith("static {}"))
    ids, name, made, helper = {}, None, None, None
    for line in init:
        if (m := re.search(r"// String (\S+)$", line)) and name is None:
            name = m.group(1)
        elif " new " in line and (m := re.search(r"// class (\S+)", line)) and m.group(1).startswith(BLOCK_PKG) and made is None:
            made = m.group(1)
        elif "invokestatic" in line and (m := re.search(r"// Method (\w+):\(.*\)Lnet/minecraft/world/level/block/Block;", line)):
            if m.group(1) != "register" and helper is None:
                helper = m.group(1)
        elif "putstatic" in line and line.rstrip().endswith(":Lnet/minecraft/world/level/block/Block;"):
            cls = made or (built_in(helper_heads[helper]) if helper in helper_heads else None)
            if name and cls:
                ids[name] = cls
            name, made, helper = None, None, None

    for c in names:
        shape(c)
    listed_ids = sorted((n, SHAPES.index(shape(c))) for n, c in ids.items() if shape(c) != "MODEL")
    listed_classes = sorted((c, SHAPES.index(s)) for c, s in shape_of.items() if s != "MODEL" and c.startswith("net/minecraft/"))
    lines = [
        "//! Generated by `tools/vanilla_render_shapes.py` from NeoForge's merged jar: the vanilla blocks",
        "//! (by path) and block classes whose `getRenderShape` isn't `MODEL`, with `RenderShape`'s ordinal",
        "//! in that version (0 `INVISIBLE`, 1 `ENTITYBLOCK_ANIMATED`). Sorted.",
        "",
        f"pub(crate) const BLOCKS: [(&str, u8); {len(listed_ids)}] = [",
    ]
    lines += [f'    ("{n}", {o}),' for n, o in listed_ids]
    lines.append("];")
    lines.append("")
    lines.append(f"pub(crate) const CLASSES: [(&str, u8); {len(listed_classes)}] = [")
    lines += [f'    ("{c}", {o}),' for c, o in listed_classes]
    lines.append("];")
    with open(OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    print(f"{len(ids)} blocks read, {len(listed_ids)} blocks and {len(listed_classes)} classes not drawn as models -> {OUT}")


main()
