#!/usr/bin/env python3
"""Universal Mod Translator's progress: the pack's measurements and the README's figures.

    python3 tools/translator_progress.py record <run dir> [--run N] [--date YYYY-MM-DDTHH:MM]
                                                [--commit SHA] [--part N]
    python3 tools/translator_progress.py render

`record` reads one measurement of the pack: the translator's `coverage` report of every jar,
`<run dir>/out/<jar>.txt`, written with `MISSING=all` (and `LIBS=1` for the library half). It adds
the run to docs/translator/progress.json, and the newest run's per-jar numbers and commonest gaps.

`render` draws the figures from that file, in light and dark and in English and Russian
(docs/translator/*.svg), and rewrites what stands between each `<!-- progress:<name> -->` and the
`<!-- /progress -->` after it in .github/README.md and .github/README.ru.md.

The totals leave out the datagen-only APIs (data providers, model generators, GatherDataEvent): a
player's game never runs them. The figures embed subsets of Horizon's own fonts
(crates/horizon-theme/fonts, SIL OFL 1.1) when fontTools is installed, and name them otherwise.
"""

from __future__ import annotations

import argparse
import base64
import datetime as dt
import io
import json
import math
import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "docs" / "translator" / "progress.json"
FIGURES = ROOT / "docs" / "translator"
READMES = {"en": ROOT / ".github" / "README.md", "ru": ROOT / ".github" / "README.ru.md"}
FONT_DIR = ROOT / "crates" / "horizon-theme" / "fonts"

MAPPED = re.compile(r"mapped: (\d+) of (\d+) API methods \([\d.]+%\), (\d+) of (\d+) class uses")
MISSING = re.compile(r"^\s*missing\s+(\d+) (\S+)")
LIBS = re.compile(r"libraries: mapped (\d+) of (\d+) library methods \([\d.]+%\), (\d+) of (\d+) class uses")
GAME = re.compile(r"by the game's own code: (\d+) methods of classes translated with the mod \((\d+) class uses\), "
                  r"(\d+) standing for unmapped ones \((\d+) class uses\)")
LIB_MISSING = re.compile(r"^\s*lib-missing\s+(\d+) (\S+)")
DATAGEN = (
    "net/minecraft/data/",
    "net/neoforged/neoforge/data/",
    "net/neoforged/neoforge/common/data/",
    "net/neoforged/neoforge/client/model/generators/",
)
# LWJGL is the window and the GPU under the game: Horizon draws with wgpu instead, so its calls are
# counted but left out of the gap lists.
UNLISTED_LIBS = ("org/lwjgl/",)
CREATE = "create-1.21.1-"
TOP = 12


# ---------------------------------------------------------------------------------------------
# Measurements


def parse_report(text):
    """One jar's `coverage` report: API (methods, of, uses, of) with the datagen-only calls taken
    out, the missing API calls by method, and the same for the libraries. None when the report has
    no totals (the jar stopped the translator)."""
    m = MAPPED.search(text)
    if not m:
        return None
    x, y, a, b = map(int, m.groups())
    missing, lib_missing = {}, {}
    for line in text.splitlines():
        mm = MISSING.match(line)
        if mm:
            n, key = int(mm.group(1)), mm.group(2)
            if key.startswith(DATAGEN):
                y -= 1
                b -= n
            else:
                missing[key] = n
            continue
        mm = LIB_MISSING.match(line)
        if mm:
            lib_missing[mm.group(2)] = int(mm.group(1))
    lm = LIBS.search(text)
    libs = tuple(map(int, lm.groups())) if lm else None
    # Of the mapped ones, those the game's own code answers (translated from the player's install).
    gm = GAME.search(text)
    game = (int(gm.group(1)) + int(gm.group(3)), int(gm.group(2)) + int(gm.group(4))) if gm else None
    return {"api": (x, y, a, b), "missing": missing, "libs": libs, "lib_missing": lib_missing, "game": game}


def top_gaps(by_key, skip=()):
    rows = [(mods, uses, key) for key, (mods, uses) in by_key.items() if not key.startswith(skip)]
    rows.sort(key=lambda r: (-r[0], -r[1], r[2]))
    return [[key, mods, uses] for mods, uses, key in rows[:TOP]]


def measure(run_dir):
    out = run_dir / "out"
    jars, by_key, by_lib = [], {}, {}
    api = [0, 0, 0, 0]
    libs = [0, 0, 0, 0]
    have_libs = False
    create = create_libs = game = None
    for name in sorted(os.listdir(out)):
        if not name.endswith(".txt"):
            continue
        rep = parse_report((out / name).read_text(encoding="utf-8", errors="replace"))
        if rep is None:
            continue
        jar = name[:-4]
        jars.append([jar, *rep["api"]])
        api = [s + v for s, v in zip(api, rep["api"])]
        for key, n in rep["missing"].items():
            mods, uses = by_key.get(key, (0, 0))
            by_key[key] = (mods + 1, uses + n)
        if rep["game"]:
            game = [s + v for s, v in zip(game or [0, 0], rep["game"])]
        if rep["libs"]:
            have_libs = True
            libs = [s + v for s, v in zip(libs, rep["libs"])]
            for key, n in rep["lib_missing"].items():
                mods, uses = by_lib.get(key, (0, 0))
                by_lib[key] = (mods + 1, uses + n)
        if jar.startswith(CREATE):
            create = rep["api"]
            create_libs = rep["libs"]
    pair = lambda t: {"methods": [t[0], t[1]], "uses": [t[2], t[3]]}
    entry = {
        "jars": len(jars),
        "api": {**pair(api), "missing": len(by_key)},
        "create": pair(create) if create else None,
        "libs": {**pair(libs), "missing": len(by_lib)} if have_libs else None,
        "libs_create": pair(create_libs) if create_libs else None,
    }
    if game:
        entry["game"] = {"methods": [game[0], api[1]], "uses": [game[1], api[3]]}
    latest = {
        "jars": jars,
        "missing": top_gaps(by_key),
        "lib_missing": top_gaps(by_lib, UNLISTED_LIBS) if have_libs else [],
    }
    return entry, latest


SCALARS = re.compile(r'\[(?:\s*(?:-?\d+|"(?:[^"\\]|\\.)*"|null)\s*,)*\s*(?:-?\d+|"(?:[^"\\]|\\.)*"|null)\s*\]')


def dump(data):
    """JSON with each array of plain values on one line: a jar, a gap, a pair of counts."""
    text = json.dumps(data, indent=1, ensure_ascii=False)
    one_line = lambda m: json.dumps(json.loads(m.group(0)), ensure_ascii=False)
    return SCALARS.sub(one_line, text) + "\n"


def run_number(run_dir):
    m = re.search(r"(\d+)$", run_dir.name)
    return int(m.group(1)) if m else 1


def record(args):
    run_dir = Path(args.run_dir)
    entry, latest = measure(run_dir)
    if not entry["jars"]:
        sys.exit(f"{run_dir}: no coverage reports in out/")
    n = args.run or run_number(run_dir)
    date = args.date or dt.datetime.fromtimestamp((run_dir / "out").stat().st_mtime).strftime("%Y-%m-%dT%H:%M")
    commit = args.commit
    if not commit and (run_dir / "commit.txt").exists():
        first = (run_dir / "commit.txt").read_text().split()
        commit = first[0] if first else None
    probe = "hosted"
    if (run_dir / "run.sh").exists() and "PROBE=hosted" not in (run_dir / "run.sh").read_text():
        probe = "bare"
    data = json.loads(DATA.read_text()) if DATA.exists() else {"runs": []}
    runs = [r for r in data["runs"] if r["run"] != n]
    runs.append({"run": n, "id": run_dir.name, "date": date, "commit": commit, "part": args.part,
                 "probe": probe, **entry})
    runs.sort(key=lambda r: r["run"])
    data["runs"] = runs
    if n >= max(r["run"] for r in runs):
        data["latest"] = {"run": n, **latest}
    DATA.parent.mkdir(parents=True, exist_ok=True)
    DATA.write_text(dump(data))
    api = entry["api"]
    print(f"run {n} ({date}): {entry['jars']} jars, API methods {pct(*api['methods']):.1f}%, "
          f"call sites {pct(*api['uses']):.1f}%")


def pct(a, b):
    return 100 * a / b if b else 0.0


# ---------------------------------------------------------------------------------------------
# Words


def fmt_int(n, lang):
    s = f"{n:,}"
    return s.replace(",", " ") if lang == "ru" else s


def fmt_pct(a, b, lang, digits=1):
    v = f"{pct(a, b):.{digits}f}"
    return v.replace(".", ",") + " %" if lang == "ru" else v + "%"


MONTHS = {"en": ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"],
          "ru": ["янв", "фев", "мар", "апр", "мая", "июн", "июл", "авг", "сен", "окт", "ноя", "дек"]}
MONTHS_LONG = {"en": ["January", "February", "March", "April", "May", "June", "July", "August",
                      "September", "October", "November", "December"],
               "ru": ["января", "февраля", "марта", "апреля", "мая", "июня", "июля", "августа",
                      "сентября", "октября", "ноября", "декабря"]}


def ru_jars(n):
    if n % 10 == 1 and n % 100 != 11:
        return "джар"
    if 2 <= n % 10 <= 4 and not 12 <= n % 100 <= 14:
        return "джара"
    return "джаров"


def day_label(date, lang, with_month):
    d = dt.date.fromisoformat(date[:10])
    return f"{d.day} {MONTHS[lang][d.month - 1]}" if with_month else str(d.day)


def date_span(a, b, lang):
    """`4 to 8 November 2026`, `с 4 по 8 ноября 2026`; the month and year once when they agree."""
    x, y = dt.date.fromisoformat(a[:10]), dt.date.fromisoformat(b[:10])
    first = str(x.day) if (x.month, x.year) == (y.month, y.year) else long_date(a, lang)
    return f"с {first} по {long_date(b, lang)}" if lang == "ru" else f"{first} to {long_date(b, lang)}"


def long_date(date, lang):
    d = dt.date.fromisoformat(date[:10])
    return f"{d.day} {MONTHS_LONG[lang][d.month - 1]} {d.year}"


WORDS = {
    "en": {
        "title": "Universal Mod Translator",
        "subtitle": "NeoForge 1.21.1 mods and modpacks, rebuilt as native Rust for Horizon",
        "uses": "call sites answered",
        "methods": "API methods answered",
        "pack": "All {n} jars of the pack",
        "create": "Create 6.0.10",
        "run": "run",
        "runs_axis": "Runs {first} to {last} over the same {n} jars, {span}",
        "jars_head": "Each jar of the pack, by the share of its game calls answered natively",
        "bins": ["100%", "95 to 99.9%", "90 to 95%", "80 to 90%", "60 to 80%", "below 60%", "no game calls"],
        "jars_count": "{n} jars",
        "jar_count": "1 jar",
        "with_create": "Create is here, at {v}",
        "stages": [
            ("Read", ["A mod's classes and", "bytecode, with the", "jars it bundles"], "class.rs · insn.rs"),
            ("Weave", ["Mixins join their", "targets: other mods'", "classes and the game's"], "weave.rs"),
            ("Translate", ["Every opcode to Rust:", "lambdas, exceptions,", "records, switches"], "codegen.rs"),
            ("Answer", ["Game, NeoForge and", "Java library calls,", "answered natively"], "horizon-compat"),
            ("Build", ["Bundled into Horizon's", "tree and compiled", "with Cargo"], "bundle.rs · horizon-mixins"),
        ],
        "input": "mod jars",
        "output": "Horizon",
        "no_jvm": "No JVM at any step: what runs in the game is Rust.",
        "alt_progress": "Coverage over {n} runs: call sites answered rose from {u0} to {u1}, API methods from {m0} to {m1}; Create's call sites from {c0} to {c1}.",
        "alt_jars": "Of {n} jars, {full} have every game call answered and {high} at least 95%.",
    },
    "ru": {
        "title": "Universal Mod Translator",
        "subtitle": "Моды и модпаки NeoForge 1.21.1, пересобранные в нативный Rust для Horizon",
        "uses": "отвеченные вызовы",
        "methods": "отвеченные методы API",
        "pack": "Все {n} джаров пака",
        "create": "Create 6.0.10",
        "run": "прогон",
        "runs_axis": "Прогоны с {first} по {last} на тех же {n} джарах, {span}",
        "jars_head": "Каждый джар пака по доле его вызовов игры, которым отвечает Rust",
        "bins": ["100 %", "от 95 до 99,9 %", "от 90 до 95 %", "от 80 до 90 %", "от 60 до 80 %",
                 "меньше 60 %", "без вызовов игры"],
        "jars_count": "{n} {jars}",
        "jar_count": "1 джар",
        "with_create": "здесь Create: {v}",
        "stages": [
            ("Чтение", ["Классы и байткод", "мода и вложенных", "в него джаров"], "class.rs · insn.rs"),
            ("Вплетение", ["Миксины встают", "в свои цели: классы", "других модов и игры"], "weave.rs"),
            ("Перевод", ["Каждый опкод в Rust:", "лямбды, исключения,", "записи, switch"], "codegen.rs"),
            ("Ответ", ["Rust отвечает", "на вызовы игры,", "NeoForge и Java"], "horizon-compat"),
            ("Сборка", ["Бандл встаёт", "в дерево Horizon", "и собирается Cargo"], "bundle.rs · horizon-mixins"),
        ],
        "input": "джары модов",
        "output": "Horizon",
        "no_jvm": "Ни на одном шаге нет JVM: в игре работает Rust.",
        "alt_progress": "Покрытие за {n} прогонов: доля отвеченных вызовов выросла с {u0} до {u1}, методов API с {m0} до {m1}; у Create вызовов с {c0} до {c1}.",
        "alt_jars": "Из {n} джаров у {full} отвечены все вызовы игры, у {high} не меньше 95 %.",
    },
}


# ---------------------------------------------------------------------------------------------
# Themes: Horizon's tokens (crates/horizon-theme): the night sky and dawn of the launcher's Night look
# for the masthead, its ink, rules and violet for the figures, which sit on GitHub's own ground.

LIGHT = {
    "ground": "#ffffff", "ink": "#1b1530", "ink_soft": "#3b3354", "muted": "#585071",
    "rule": "#e4ddf4", "accent": "#5333c9", "wash": 0.10, "context": "#8f86ad",
    "sky": ["#c9d3f2", "#dcd8f3", "#ead5ec", "#f5d3d8", "#f9d6bd", "#fbe4c6"],
    "sun": ["#fffbea", "#ffeac2", "#ffd3a1"], "far": "#baa8de", "near": "#8c79c6", "star": "#ffffff",
}
DARK = {
    "ground": "#0d1117", "ink": "#f4f2ff", "ink_soft": "#cfc9ea", "muted": "#a59fc6",
    "rule": "#262838", "accent": "#a08cff", "wash": 0.08, "context": "#77718f",
    "sky": ["#07061a", "#140f38", "#2a1656", "#5b2a78", "#b0477d", "#f08a5d"],
    "sun": ["#ffe28c", "#ffbf66", "#ff944f"], "far": "#3a1f5e", "near": "#150f2e", "star": "#f4f2ff",
}
THEMES = {"light": LIGHT, "dark": DARK}


def over(top, alpha, under):
    """`top` over `under` at `alpha`, as horizon_theme::night::over mixes it."""
    t = [int(top[i:i + 2], 16) for i in (1, 3, 5)]
    u = [int(under[i:i + 2], 16) for i in (1, 3, 5)]
    return "#" + "".join(f"{round(a * alpha + b * (1 - alpha)):02x}" for a, b in zip(t, u))


# ---------------------------------------------------------------------------------------------
# Type: measured with the fonts themselves, so labels never collide, and embedded as subsets.

FONTS = {
    "display": ("BricolageGrotesqueDisplay-Bold.ttf", "HZ Bricolage Display", 700),
    "ui": ("BricolageGrotesque-Medium.ttf", "HZ Bricolage", 500),
    "sans": ("Geist-Regular.ttf", "HZ Geist", 400),
    "medium": ("Geist-Medium.ttf", "HZ Geist Medium", 500),
    "mono": ("GeistMono-Regular.ttf", "HZ Geist Mono", 400),
}
FALLBACK = {"display": "sans-serif", "ui": "sans-serif", "sans": "sans-serif", "medium": "sans-serif",
            "mono": "ui-monospace, monospace"}

try:
    import logging

    from fontTools import subset as _subset
    from fontTools.ttLib import TTFont

    logging.getLogger("fontTools").setLevel(logging.ERROR)
except ImportError:  # measured roughly, and named rather than embedded
    TTFont = None


class Type:
    def __init__(self):
        self.fonts = {}

    def font(self, key):
        if TTFont is None:
            return None
        if key not in self.fonts:
            self.fonts[key] = TTFont(FONT_DIR / FONTS[key][0], recalcTimestamp=False, lazy=True)
        return self.fonts[key]

    def width(self, text, key, size, tracking=0.0):
        f = self.font(key)
        if f is None:
            return len(text) * size * (0.6 if key == "mono" else 0.52)
        cmap, hmtx, upm = f.getBestCmap(), f["hmtx"], f["head"].unitsPerEm
        adv = sum(hmtx[cmap[ord(c)]][0] if ord(c) in cmap else upm * 0.5 for c in text)
        return adv * size / upm + tracking * size * max(len(text) - 1, 0)

    def face(self, key, chars):
        """An @font-face for `key` holding only `chars`, or None without fontTools."""
        if TTFont is None:
            return None
        path, family, weight = FONTS[key]
        font = TTFont(FONT_DIR / path, recalcTimestamp=False)
        opts = _subset.Options()
        opts.layout_features = ["kern", "tnum", "case"]
        opts.hinting = False
        opts.desubroutinize = True
        opts.name_IDs = [0, 1, 2, 13, 14]
        try:
            import brotli  # noqa: F401
            opts.flavor, mime = "woff2", "font/woff2"
        except ImportError:
            opts.flavor, mime = "woff", "font/woff"
        sub = _subset.Subsetter(opts)
        sub.populate(unicodes=sorted({ord(c) for c in chars} | {0x20}))
        sub.subset(font)
        buf = io.BytesIO()
        font.flavor = opts.flavor
        font.save(buf)
        data = base64.b64encode(buf.getvalue()).decode()
        return (f"@font-face{{font-family:'{family}';font-weight:{weight};"
                f"src:url(data:{mime};base64,{data}) format('{opts.flavor}')}}")


TYPE = Type()


def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace('"', "&quot;")


class Svg:
    def __init__(self, w, h, title, desc):
        self.w, self.h, self.title, self.desc = w, h, title, desc
        self.body, self.defs = [], []
        self.chars = {}

    def add(self, s):
        self.body.append(s)

    def rect(self, x, y, w, h, fill, extra=""):
        self.add(f'<rect x="{x:g}" y="{y:g}" width="{w:g}" height="{h:g}" fill="{fill}"{extra}/>')

    def text(self, x, y, s, key, size, fill, anchor="start", tracking=0.0, extra=""):
        self.chars.setdefault(key, set()).update(s)
        family, weight = FONTS[key][1], FONTS[key][2]
        ls = f' letter-spacing="{tracking * size:.2f}"' if tracking else ""
        an = f' text-anchor="{anchor}"' if anchor != "start" else ""
        self.add(f'<text x="{x:.1f}" y="{y:.1f}" font-family="\'{family}\', {FALLBACK[key]}" '
                 f'font-weight="{weight}" font-size="{size:g}" fill="{fill}"{ls}{an}{extra}>{esc(s)}</text>')
        return TYPE.width(s, key, size, tracking)

    def render(self):
        faces = [TYPE.face(k, c) for k, c in sorted(self.chars.items())]
        style = "".join(f for f in faces if f)
        head = (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {self.w:g} {self.h:g}" '
                f'width="{self.w:g}" height="{self.h:g}" role="img" aria-labelledby="t d">'
                f'<title id="t">{esc(self.title)}</title><desc id="d">{esc(self.desc)}</desc>{PROVENANCE}')
        defs = "".join(self.defs)
        if style or defs:
            head += "<defs>" + (f"<style>{style}</style>" if style else "") + defs + "</defs>"
        return head + "".join(self.body) + "</svg>\n"


# Where every figure comes from, and the licence of the type subsets it carries.
PROVENANCE = ("<!-- Made by tools/translator_progress.py from docs/translator/progress.json. Type: subsets of "
              "Geist and Geist Mono (Copyright 2024 The Geist Project Authors) and Bricolage Grotesque (Copyright "
              "2022 The Bricolage Grotesque Project Authors), under the SIL Open Font License 1.1: "
              "crates/horizon-theme/fonts/OFL-Geist.txt, OFL-BricolageGrotesque.txt. -->")

# ---------------------------------------------------------------------------------------------
# The masthead: the launcher's sky (launcher/src/sky.rs), its skyline the pack itself, one column
# per jar: its height the jar's size in game calls (a log scale), the scaffold on top the share of
# those calls not answered yet.

TEXEL, LAND = 4, 6
SHARES = [0.2, 0.18, 0.17, 0.15, 0.15, 0.15]
SUN_RADIUS, SUN_RISE, SUN_X = 15, 8, 0.72
FAR_RUNS = [(18, 8), (10, 10), (6, 13), (14, 11), (22, 9), (8, 12), (12, 14), (16, 11), (20, 8), (10, 10),
            (14, 12), (24, 9), (12, 11), (18, 8)]
FAR_ROWS, FAR_LIFT = 20, 2
TOWER = ["#.#", "###", "###", "###", "###", "###", "###"]
PINE = ["..#..", ".###.", "..#..", ".###.", "#####", "..#.."]
FAR_PROPS = [(32, TOWER), (118, PINE), (176, TOWER)]
STAR_LEVELS = [0.95, 0.6, 0.3, 0.6]


def sky_bands(sky, h):
    rows = math.ceil(h / TEXEL)
    out, at = [], 0
    for i, share in enumerate(SHARES):
        span = max(round_half_away(share * rows), 4)
        if i + 1 == len(sky):
            out.append((at * TEXEL, max(h / TEXEL - at, span) * TEXEL, sky[i]))
            break
        frm = at
        for k in range(4):
            to = round_half_away(at + span * (k + 1) / 4)
            out.append((frm * TEXEL, (to - frm) * TEXEL, over(sky[i + 1], k / 4, sky[i])))
            frm = to
        at += span
    return out


def round_half_away(v):
    return math.floor(v + 0.5) if v >= 0 else -math.floor(-v + 0.5)


def stars():
    rng = 0x2545F491

    def nxt():
        nonlocal rng
        rng ^= (rng << 13) & 0xFFFFFFFF
        rng ^= rng >> 17
        rng ^= (rng << 5) & 0xFFFFFFFF
        return (rng >> 8) / (1 << 24)

    out = []
    for i in range(240):
        x, y = math.floor(nxt() * 960), nxt()
        keep = nxt() > y * y
        big = nxt() < 0.08
        if keep:
            out.append((x, y, i % 4, big))
    return out


def sun_rows(colors):
    r = SUN_RADIUS
    for y in range(r):
        dy = r - y - 0.5
        half = round_half_away(math.sqrt(r * r - dy * dy))
        depth = y / r
        if depth > 0.5 and (r - y) % 3 == 0:
            continue
        c = colors[0 if depth < 0.4 else 1 if depth < 0.7 else 2]
        yield y, r - half, r + half, c


def jar_sort_key(name):
    return re.sub(r"^[^0-9A-Za-z]+", "", re.sub(r"^\[[^\]]*\]\s*", "", name)).lower()


def masthead(data, theme, lang):
    t, w = THEMES[theme], WORDS[lang]
    jars = sorted(data["latest"]["jars"], key=lambda j: jar_sort_key(j[0]))
    W, H = 1468, 440
    foot = H
    svg = Svg(W, H, w["title"], w["subtitle"])
    for y, h, c in sky_bands(t["sky"], H):
        svg.rect(0, y, W, h, c)
    text_w = max(TYPE.width(w["title"], "display", 92, -0.035), TYPE.width(w["subtitle"], "ui", 30, -0.005))
    title_box = (40, 40, 64 + text_w + 40, 230)
    for x, y, group, big in stars():
        px, py = x * TEXEL, round(y * H * 0.5)
        size = 4 if big else 2
        if px >= W or (title_box[0] <= px <= title_box[2] and title_box[1] <= py <= title_box[3]):
            continue
        svg.rect(px, py, size, size, t["star"], f' opacity="{STAR_LEVELS[group]}"')
    # The sun, behind both skylines.
    sx = round((W * SUN_X - SUN_RADIUS * LAND) / 2) * 2
    sy = foot - (SUN_RISE + SUN_RADIUS) * LAND
    for row, x0, x1, c in sun_rows(t["sun"]):
        svg.rect(sx + x0 * LAND, sy + row * LAND, (x1 - x0) * LAND, LAND, c)
    # The far skyline: Horizon's hills, half a period in.
    period = sum(r for r, _ in FAR_RUNS)
    tops = []
    for run, height in FAR_RUNS:
        tops += [FAR_ROWS - height] * run
    top_y = foot - (FAR_ROWS + FAR_LIFT) * LAND
    parts = []
    ox = -period * LAND / 2
    while ox < W:
        x = 0
        for run, height in FAR_RUNS:
            parts.append(f"M{ox + x * LAND:g} {top_y + (FAR_ROWS - height) * LAND:g}h{run * LAND:g}V{foot}h{-run * LAND:g}Z")
            x += run
        for px, sprite in FAR_PROPS:
            ground = min(tops[px:px + len(sprite[0])])
            y0 = ground - len(sprite)
            for dy, row in enumerate(sprite):
                for dx, mark in enumerate(row):
                    if mark == "#":
                        parts.append(f"M{ox + (px + dx) * LAND:g} {top_y + (y0 + dy) * LAND:g}h{LAND}v{LAND}h{-LAND}Z")
        ox += period * LAND
    svg.add(f'<path fill="{t["far"]}" d="{"".join(parts)}"/>')
    # The pack: one column per jar.
    umax = max(j[4] for j in jars) or 1
    col = W / len(jars)
    solid, scaffold = [], []
    for i, (_, x, y, a, b) in enumerate(jars):
        if b > 0:
            rows = 3 + round_half_away(15 * math.log10(1 + b) / math.log10(1 + umax))
            done = round_half_away(rows * a / b)
        else:
            rows = done = 2
        cx = i * col
        solid.append(f"M{cx:g} {foot - done * TEXEL}h{col:g}V{foot}h{-col:g}Z")
        if rows > done:
            scaffold.append(f"M{cx:g} {foot - rows * TEXEL}h{col:g}v{(rows - done) * TEXEL}h{-col:g}Z")
    svg.defs.append(f'<pattern id="scaffold" width="{2 * TEXEL}" height="{2 * TEXEL}" patternUnits="userSpaceOnUse">'
                    f'<rect width="{TEXEL}" height="{TEXEL}" fill="{t["near"]}"/>'
                    f'<rect x="{TEXEL}" y="{TEXEL}" width="{TEXEL}" height="{TEXEL}" fill="{t["near"]}"/></pattern>')
    svg.add(f'<path fill="url(#scaffold)" d="{"".join(scaffold)}"/>')
    svg.add(f'<path fill="{t["near"]}" d="{"".join(solid)}"/>')
    svg.text(64, 150, w["title"], "display", 92, t["ink"], tracking=-0.035)
    svg.text(66, 206, w["subtitle"], "ui", 30, t["ink_soft"], tracking=-0.005)
    return svg.render()


# ---------------------------------------------------------------------------------------------
# Figure 1: coverage over the runs, the pack and Create side by side on one scale.


def progress(data, theme, lang):
    t, w = THEMES[theme], WORDS[lang]
    runs = data["runs"]
    n_jars = data["latest"] and len(data["latest"]["jars"])
    W, H = 960, 440
    top, bottom = 104, 372
    panels = [(w["pack"].format(n=n_jars), lambda r: r["api"]), (w["create"], lambda r: r["create"])]
    values = [pct(*s[k]) for _, get in panels for r in runs if (s := get(r)) for k in ("methods", "uses")]
    lo = math.floor(min(values) / 10) * 10
    yv = lambda v: bottom - (v - lo) / (100 - lo) * (bottom - top)
    first, last = runs[0], runs[-1]
    u0, u1 = fmt_pct(*first["api"]["uses"], lang), fmt_pct(*last["api"]["uses"], lang)
    m0, m1 = fmt_pct(*first["api"]["methods"], lang), fmt_pct(*last["api"]["methods"], lang)
    c_first = next(r for r in runs if r["create"])
    c0, c1 = fmt_pct(*c_first["create"]["uses"], lang), fmt_pct(*last["create"]["uses"], lang)
    svg = Svg(W, H, w["uses"], w["alt_progress"].format(n=len(runs), u0=u0, u1=u1, m0=m0, m1=m1, c0=c0, c1=c1))
    # Legend, once for both panels.
    x = 56
    for key, color in (("uses", t["accent"]), ("methods", t["context"])):
        svg.add(f'<line x1="{x}" y1="26" x2="{x + 22}" y2="26" stroke="{color}" stroke-width="2.4" stroke-linecap="round"/>')
        x += 32 + svg.text(x + 32, 31, w[key], "sans", 15, t["ink_soft"]) + 28
    pw, gap = 344, 136
    for p, (title, get) in enumerate(panels):
        x0 = 56 + p * (pw + gap)
        x1 = x0 + pw
        svg.text(x0, 76, title, "medium", 17, t["ink"])
        for v in range(lo, 101, 10):
            y = yv(v)
            svg.add(f'<line x1="{x0}" y1="{y:.1f}" x2="{x1}" y2="{y:.1f}" stroke="{t["rule"]}" stroke-width="1"/>')
            label = (f"{v} %" if lang == "ru" else f"{v}%")
            svg.text(x0 - 10, y + 5, label, "mono", 13, t["muted"], anchor="end")
        pts = [(i, r, s) for i, r in enumerate(runs) if (s := get(r))]
        xv = lambda i: x0 + i / max(len(runs) - 1, 1) * pw
        # Run numbers under the axis.
        numbers = [r["run"] for r in runs]
        for want in sorted({numbers[0], *range(10, numbers[-1] - 4, 10), numbers[-1]}):
            i = min(range(len(runs)), key=lambda k: abs(numbers[k] - want))
            xx = xv(i)
            svg.add(f'<line x1="{xx:.1f}" y1="{bottom}" x2="{xx:.1f}" y2="{bottom + 6}" stroke="{t["muted"]}" stroke-width="1"/>')
            svg.text(xx, bottom + 26, str(numbers[i]), "mono", 13, t["muted"], anchor="middle")
        for key, color in (("methods", t["context"]), ("uses", t["accent"])):
            line = " ".join(f"{xv(i):.1f},{yv(pct(*s[key])):.1f}" for i, _, s in pts)
            if key == "uses":
                area = f"M{xv(pts[0][0]):.1f},{bottom} L" + line.replace(" ", " L") + f" L{xv(pts[-1][0]):.1f},{bottom} Z"
                svg.add(f'<path d="{area}" fill="{color}" opacity="{t["wash"]}"/>')
            svg.add(f'<polyline points="{line}" fill="none" stroke="{color}" stroke-width="2.4" '
                    f'stroke-linejoin="round" stroke-linecap="round"/>')
        ends = []
        for key, color in (("uses", t["accent"]), ("methods", t["context"])):
            i, _, s = pts[-1]
            ex, ey = xv(i), yv(pct(*s[key]))
            svg.add(f'<circle cx="{ex:.1f}" cy="{ey:.1f}" r="5" fill="{color}" stroke="{t["ground"]}" stroke-width="2"/>')
            ends.append([ey, fmt_pct(*s[key], lang)])
        # End labels, kept apart without leaving their lines.
        if abs(ends[0][0] - ends[1][0]) < 18:
            mid = (ends[0][0] + ends[1][0]) / 2
            ends[0][0], ends[1][0] = sorted([mid - 9, mid + 9])
        for ey, label in ends:
            svg.text(x1 + 12, ey + 5, label, "medium", 15, t["ink"])
    svg.text(56, H - 12, w["runs_axis"].format(first=runs[0]["run"], last=runs[-1]["run"], n=n_jars,
                                              span=date_span(runs[0]["date"], runs[-1]["date"], lang)),
             "sans", 13, t["muted"])
    return svg.render()


# ---------------------------------------------------------------------------------------------
# Figure 2: the jars by the share of their game calls answered.

BINS = [(100.0, 100.0), (95.0, 100.0), (90.0, 95.0), (80.0, 90.0), (60.0, 80.0), (0.0, 60.0)]


def jar_bin(a, b):
    if b == 0:
        return 6
    v = pct(a, b)
    if a == b:
        return 0
    for i, (lo, hi) in enumerate(BINS[1:], 1):
        if v >= lo:
            return i
    return 5


def jars_figure(data, theme, lang):
    t, w = THEMES[theme], WORDS[lang]
    jars = data["latest"]["jars"]
    counts = [0] * 7
    create_bin = None
    for name, x, y, a, b in jars:
        k = jar_bin(a, b)
        counts[k] += 1
        if name.startswith(CREATE):
            create_bin, create_v = k, fmt_pct(a, b, lang)
    full, high = counts[0], counts[0] + counts[1]
    W, row = 960, 40
    H = 64 + row * 7 + 8
    svg = Svg(W, H, w["jars_head"], w["alt_jars"].format(n=len(jars), full=full, high=high))
    svg.text(0, 26, w["jars_head"], "sans", 15, t["ink_soft"])
    label_w = max(TYPE.width(s, "mono", 13) for s in w["bins"])
    x0 = label_w + 20
    span = W - x0 - 260
    most = max(counts)
    for k, n in enumerate(counts):
        y = 64 + k * row
        svg.text(label_w, y + 16, w["bins"][k], "mono", 13, t["muted"], anchor="end")
        length = max(span * n / most, 2) if n else 0
        color = t["context"] if k == 6 else t["accent"]
        if length:
            r = min(4, length / 2)
            svg.add(f'<path d="M{x0} {y}h{length - r:.1f}a{r} {r} 0 0 1 {r} {r}v{22 - 2 * r}a{r} {r} 0 0 1 {-r} {r}'
                    f'h{-(length - r):.1f}Z" fill="{color}"/>')
        count = w["jar_count"] if n == 1 else w["jars_count"].format(n=fmt_int(n, lang), jars=ru_jars(n))
        tx = x0 + length + 10
        tw = svg.text(tx, y + 16, count, "medium", 15, t["ink"])
        if k == create_bin:
            svg.text(tx + tw + 14, y + 16, w["with_create"].format(v=create_v), "sans", 14, t["muted"])
    return svg.render()


# ---------------------------------------------------------------------------------------------
# Figure 3: how a jar becomes part of the game.

MARK = [((4, 3, 2, 4), 1.0), ((10, 3, 2, 4), 1.0), ((4, 8, 2, 4), 0.36), ((10, 8, 2, 4), 0.36)]


def pipeline(theme, lang):
    t, w = THEMES[theme], WORDS[lang]
    W, H = 960, 284
    line_y, left, colw = 92, 24, 168
    svg = Svg(W, H, w["output"], w["no_jvm"])
    stages = w["stages"]
    mx = left + len(stages) * colw + 12
    s = 3
    svg.add(f'<line x1="{left - 6}" y1="{line_y}" x2="{mx + s}" y2="{line_y}" stroke="{t["accent"]}" stroke-width="2"/>')
    for i, (name, lines, src) in enumerate(stages):
        x = left + i * colw
        svg.rect(x - 6, line_y - 6, 12, 12, t["accent"])
        svg.text(x - 6, line_y - 22, name, "medium", 19, t["ink"])
        for k, line in enumerate(lines):
            width = svg.text(x - 6, line_y + 38 + k * 21, line, "sans", 14, t["ink_soft"])
            if width > colw - 16:
                print(f"warning: '{line}' is {width:.0f} wide, over {colw - 16}", file=sys.stderr)
        svg.text(x - 6, line_y + 38 + len(lines) * 21 + 10, src, "mono", 12, t["muted"])
    # Horizon's mark closes the line: its horizon is the pipeline's.
    my = line_y - 7.5 * s
    for (x, y, ww, hh), op in MARK:
        extra = f' opacity="{op}"' if op < 1 else ""
        svg.rect(mx + x * s, my + y * s, ww * s, hh * s, t["ink"], extra)
    svg.rect(mx + 1 * s, my + 7 * s, 14 * s, 1 * s, t["accent"])
    svg.text(mx + 8 * s, line_y - 22, w["output"], "medium", 19, t["ink"], anchor="middle")
    svg.add(f'<line x1="{left - 6}" y1="{H - 44}" x2="{W}" y2="{H - 44}" stroke="{t["rule"]}" stroke-width="1"/>')
    svg.text(left - 6, H - 16, w["no_jvm"], "sans", 14, t["ink_soft"])
    return svg.render()


# ---------------------------------------------------------------------------------------------
# The README's numbers.


def simple(owner):
    return owner.rsplit("/", 1)[-1].replace("$", ".")


def desc_params(desc):
    params, i = [], 1
    while desc[i] != ")":
        dims = 0
        while desc[i] == "[":
            dims, i = dims + 1, i + 1
        c = desc[i]
        if c == "L":
            j = desc.index(";", i)
            name, i = simple(desc[i + 1:j]), j + 1
        else:
            name = {"Z": "boolean", "B": "byte", "C": "char", "S": "short", "I": "int", "J": "long",
                    "F": "float", "D": "double"}[c]
            i += 1
        params.append(name + "[]" * dims)
    return params


def java_name(key):
    """`net/minecraft/world/entity/ai/goal/GoalSelector.addGoal.(ILnet/.../Goal;)V` as
    `GoalSelector.addGoal(int, Goal)`; constructors as `new Goal()`."""
    m = re.match(r"(.+)\.([^.]+)\.(\(.*)$", key)
    if not m:
        return key
    owner, name, desc = m.groups()
    args = ", ".join(desc_params(desc))
    return f"new {simple(owner)}({args})" if name == "<init>" else f"{simple(owner)}.{name}({args})"


README_WORDS = {
    "en": {
        "headline": ("**{api_m}** of the Minecraft and NeoForge methods that the pack's {jars} jars call are "
                     "answered natively, and **{api_u}** of their call sites. Java's libraries: **{lib_u}** of "
                     "the calls. Create alone: **{cr_u}** of its call sites. Run {run}, {date}."),
        "table_head": "| Run {run}, {jars} jars | Methods answered | Call sites answered |\n|---|---:|---:|",
        "rows": [("api", "Minecraft and NeoForge API"),
                 ("game", "…of it by the game's own code<br><sub>translated from the player's NeoForge install</sub>"),
                 ("libs", "Java libraries<br><sub>JDK, Guava, Gson, fastutil, JOML and the rest</sub>"),
                 ("create", "Create 6.0.10, its API calls"), ("libs_create", "Create 6.0.10, its library calls")],
        "cell": "**{p}**<br><sub>{a} of {b}</sub>",
        "gaps_head": "| Missing API method | Jars | Call sites |\n|---|---:|---:|",
        "lib_gaps_head": "| Missing library method | Jars | Call sites |\n|---|---:|---:|",
        "history_head": "| Run | Date | API methods | Call sites | Create, call sites |\n|---:|---|---:|---:|---:|",
    },
    "ru": {
        "headline": ("Rust отвечает на **{api_m}** методов Minecraft и NeoForge, которые вызывают {jars} джаров "
                     "пака, это **{api_u}** мест вызова. Библиотеки Java: **{lib_u}** вызовов. Create: **{cr_u}** "
                     "его вызовов. Прогон {run}, {date}."),
        "table_head": "| Прогон {run}, {jars} джаров | Отвечено методов | Отвечено вызовов |\n|---|---:|---:|",
        "rows": [("api", "API Minecraft и NeoForge"),
                 ("game", "…из них собственным кодом игры<br><sub>переведён из установки NeoForge у игрока</sub>"),
                 ("libs", "Библиотеки Java<br><sub>JDK, Guava, Gson, fastutil, JOML и другие</sub>"),
                 ("create", "Create 6.0.10, вызовы API"), ("libs_create", "Create 6.0.10, вызовы библиотек")],
        "cell": "**{p}**<br><sub>{a} из {b}</sub>",
        "gaps_head": "| Метод API, которого нет | Джаров | Вызовов |\n|---|---:|---:|",
        "lib_gaps_head": "| Метод библиотеки, которого нет | Джаров | Вызовов |\n|---|---:|---:|",
        "history_head": "| Прогон | Дата | Методы API | Вызовы | Create, вызовы |\n|---:|---|---:|---:|---:|",
    },
}


def blocks(data, lang):
    rw = README_WORDS[lang]
    last = data["runs"][-1]
    latest = data["latest"]
    jars = fmt_int(last["jars"], lang)
    out = {}
    out["headline"] = rw["headline"].format(
        api_m=fmt_pct(*last["api"]["methods"], lang), api_u=fmt_pct(*last["api"]["uses"], lang),
        lib_u=fmt_pct(*last["libs"]["uses"], lang), cr_u=fmt_pct(*last["create"]["uses"], lang),
        jars=jars, run=last["run"], date=long_date(last["date"], lang))
    rows = [rw["table_head"].format(run=last["run"], jars=jars)]
    for key, label in rw["rows"]:
        s = last.get(key)
        if not s:
            continue
        cells = [rw["cell"].format(a=fmt_int(s[k][0], lang), b=fmt_int(s[k][1], lang), p=fmt_pct(*s[k], lang))
                 for k in ("methods", "uses")]
        rows.append(f"| {label} | {cells[0]} | {cells[1]} |")
    out["table"] = "\n".join(rows)
    gap = lambda head, items: "\n".join([head] + [f"| `{java_name(k)}` | {fmt_int(m, lang)} | {fmt_int(u, lang)} |"
                                                  for k, m, u in items[:8]])
    out["gaps"] = gap(rw["gaps_head"], latest["missing"])
    out["lib_gaps"] = gap(rw["lib_gaps_head"], latest["lib_missing"])
    hist = [rw["history_head"]]
    for r in reversed(data["runs"]):
        cu = fmt_pct(*r["create"]["uses"], lang) if r["create"] else "–"
        hist.append(f"| {r['run']} | {r['date'][:10]} | {fmt_pct(*r['api']['methods'], lang)} | "
                    f"{fmt_pct(*r['api']['uses'], lang)} | {cu} |")
    out["history"] = "\n".join(hist)
    return out


def fill_readme(path, values):
    if not path.exists():
        return
    text = path.read_text()
    for name, body in values.items():
        # The body between the markers, empty or not, never past the first closing marker.
        pat = re.compile(rf"(<!-- progress:{name} -->\n)(?:(?:(?!<!-- /progress -->).)*?\n)?(<!-- /progress -->)", re.S)
        text, n = pat.subn(lambda m: m.group(1) + body + "\n" + m.group(2), text)
        if n == 0:
            print(f"note: {path.name} has no progress:{name} block", file=sys.stderr)
    path.write_text(text)


def render(_args):
    data = json.loads(DATA.read_text())
    FIGURES.mkdir(parents=True, exist_ok=True)
    for lang in ("en", "ru"):
        suffix = "" if lang == "en" else "-ru"
        for theme in ("light", "dark"):
            for name, svg in (("masthead", masthead(data, theme, lang)), ("progress", progress(data, theme, lang)),
                              ("jars", jars_figure(data, theme, lang)), ("pipeline", pipeline(theme, lang))):
                (FIGURES / f"{name}{suffix}-{theme}.svg").write_text(svg)
        fill_readme(READMES[lang], blocks(data, lang))
    last = data["runs"][-1]
    print(f"rendered run {last['run']}: API methods {pct(*last['api']['methods']):.1f}%, "
          f"call sites {pct(*last['api']['uses']):.1f}%")


def main():
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = p.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("record", help="add a pack run to docs/translator/progress.json")
    r.add_argument("run_dir")
    r.add_argument("--run", type=int)
    r.add_argument("--date")
    r.add_argument("--commit")
    r.add_argument("--part", type=int)
    r.set_defaults(fn=record)
    sub.add_parser("render", help="draw the figures and fill the README's numbers").set_defaults(fn=render)
    args = p.parse_args()
    args.fn(args)


if __name__ == "__main__":
    main()
