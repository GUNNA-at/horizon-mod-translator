#!/usr/bin/env python3
"""Writes horizon-compat's tables of vanilla mob effects and potions (`vanilla_effects.rs`) from NeoForge's merged jar.

A mod names vanilla's effects by `MobEffects` field (`MobEffects.DIG_SPEED`), and the id the field
holds (`haste`) isn't its name: `MobEffects`' static initialiser registers each effect under its id
(`register("haste", ...)`, the first string after the field before it) and stores the holder in the
field. The effect it registers is made there too: its category and colour are the first
`MobEffectCategory` constant and the first int after the id (`new MobEffect(MobEffectCategory.BENEFICIAL,
14270531)`), and its class says whether it is instantaneous (an `InstantenousMobEffect`).

`Potions`' static initialiser registers each potion the same way, `new Potion([name,] effects...)`,
each effect `new MobEffectInstance(MobEffects.FIELD, duration[, amplifier])`; the name, when there,
is what the potion's descriptions use instead of its id (`Potion.getName`).

A mod is built against the jar's version, so the tables are that version's.

Needs `javap` (a JDK). Usage: python3 tools/vanilla_mob_effects.py path/to/neoforge-<version>-merged.jar [out.rs]
"""
import re
import subprocess
import sys

OUT = "crates/horizon-compat/src/vanilla_effects.rs"
MOB_EFFECT = "net/minecraft/world/effect/MobEffect"
INSTANT = "net/minecraft/world/effect/InstantenousMobEffect"
STRING = re.compile(r"// String (\S+)$")
HOLDER_FIELD = re.compile(r"putstatic .*// Field (\w+):Lnet/minecraft/core/Holder;$")


def javap(jar, cls, code=True):
    args = ["javap", *(["-c"] if code else []), "-p", "-cp", jar, cls.replace("/", ".")]
    return subprocess.run(args, capture_output=True, text=True, check=True).stdout


def static_init(jar, cls):
    text = javap(jar, cls)
    return text[text.index("static {};") :].splitlines()


def int_constant(line):
    """The int an instruction pushes (`iconst_2`, `bipush 20`, `sipush 3600`, `ldc ... // int 9154528`), else None."""
    if m := re.search(r"\biconst_(m1|\d)\b", line):
        return -1 if m.group(1) == "m1" else int(m.group(1))
    if m := re.search(r"\b[bs]ipush\s+(-?\d+)", line):
        return int(m.group(1))
    if m := re.search(r"// int (-?\d+)$", line):
        return int(m.group(1))
    return None


def instantaneous(jar, cls):
    """Whether the effect class `cls` is an `InstantenousMobEffect`, by its superclasses."""
    while cls not in (MOB_EFFECT, INSTANT):
        m = re.search(r"\bclass \S+ extends (\S+)", javap(jar, cls, code=False))
        if not m:
            sys.exit(f"{cls} is no MobEffect")
        cls = m.group(1).replace(".", "/")
    return cls == INSTANT


def effects(jar):
    """(field, id, category, colour, instantaneous) of each `MobEffects` field, sorted by field."""
    rows, cur = [], None
    for line in static_init(jar, "net/minecraft/world/effect/MobEffects"):
        if cur is None:
            if m := STRING.search(line):
                cur = {"id": m.group(1)}
        elif "class" not in cur and (m := re.search(r"\bnew\s.*// class (\S+)$", line)):
            cur["class"] = m.group(1)
        elif "category" not in cur and (m := re.search(r"getstatic .*// Field net/minecraft/world/effect/MobEffectCategory\.(\w+):", line)):
            cur["category"] = m.group(1)
        elif "category" in cur and "color" not in cur and (n := int_constant(line)) is not None:
            cur["color"] = n
        elif m := HOLDER_FIELD.search(line):
            if missing := [k for k in ("class", "category", "color") if k not in cur]:
                sys.exit(f"MobEffects.{m.group(1)}: no {', '.join(missing)}")
            rows.append((m.group(1), cur["id"], cur["category"], cur["color"], instantaneous(jar, cur["class"])))
            cur = None
    return sorted(rows)


def potions(jar):
    """(id, name, effects as (`MobEffects` field, duration, amplifier)) of each `Potions` field, in order."""
    rows, cur, effect = [], None, None
    for line in static_init(jar, "net/minecraft/world/item/alchemy/Potions"):
        if cur is None:
            if m := STRING.search(line):
                cur = {"id": m.group(1), "name": None, "effects": []}
        elif m := re.search(r"getstatic .*// Field net/minecraft/world/effect/MobEffects\.(\w+):", line):
            effect = (m.group(1), [])
        elif effect is not None and (n := int_constant(line)) is not None:
            effect[1].append(n)
        elif effect is not None and (m := re.search(r'invokespecial .*MobEffectInstance\."<init>":\((.*)\)V', line)):
            ints = {"Lnet/minecraft/core/Holder;I": 1, "Lnet/minecraft/core/Holder;II": 2}.get(m.group(1))
            if ints != len(effect[1]):
                sys.exit(f"Potions {cur['id']}: MobEffectInstance({m.group(1)}) of {effect[1]}")
            duration, amplifier = (effect[1] + [0])[:2]
            cur["effects"].append((effect[0], duration, amplifier))
            effect = None
        elif effect is None and not cur["effects"] and cur["name"] is None and (m := STRING.search(line)):
            cur["name"] = m.group(1)
        elif m := HOLDER_FIELD.search(line):
            # The runtime finds a field's potion by the field's name (`Potions.WATER`: `water`).
            if m.group(1).lower() != cur["id"]:
                sys.exit(f"Potions.{m.group(1)} registers {cur['id']}")
            rows.append((cur["id"], cur["name"], cur["effects"]))
            cur = None
    return rows


def main():
    jar = sys.argv[1]
    out = sys.argv[2] if len(sys.argv) > 2 else OUT
    effs = effects(jar)
    ids = {field: id for field, id, *_ in effs}
    pots = potions(jar)
    lines = [
        "//! Generated by `tools/vanilla_mob_effects.py` from NeoForge's merged jar: vanilla's mob effects and",
        "//! potions, as `MobEffects` and `Potions` register them (in `minecraft`).",
        "",
        "/// Each `MobEffects` field: the effect id it registers, the effect's category (a `MobEffectCategory`",
        "/// constant), its colour, and whether it is instantaneous (an `InstantenousMobEffect`). Sorted by field.",
        f"pub(crate) const MOB_EFFECTS: [(&str, &str, &str, i32, bool); {len(effs)}] = [",
    ]
    lines += [f'    ("{f}", "{i}", "{c}", {color}, {str(inst).lower()}),' for f, i, c, color, inst in effs]
    lines += [
        "];",
        "",
        "/// Each `Potions` field's potion, in registration order: its id, the name its descriptions use instead",
        "/// of its id (`Potion.getName`: a long or strong potion's base), and its effects as (effect id,",
        "/// duration in ticks, amplifier).",
        f"pub(crate) const POTIONS: [(&str, Option<&str>, &[(&str, i32, i32)]); {len(pots)}] = [",
    ]
    for id, name, es in pots:
        name = f'Some("{name}")' if name else "None"
        es = ", ".join(f'("{ids[f]}", {d}, {a})' for f, d, a in es)
        lines.append(f'    ("{id}", {name}, &[{es}]),')
    lines.append("];")
    with open(out, "w") as f:
        f.write("\n".join(lines) + "\n")
    print(f"{len(effs)} mob effects, {len(pots)} potions -> {out}")


main()
