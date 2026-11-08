# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

- **People the owner shows the project to.** They need to see in a minute what it is, why it matters, what
  already works, how to try it, and the numbers that prove it.
- **Horizon's maintainers.** They need the architecture, where the translator stops and Horizon begins,
  and how every claim is checked: the measurements, the tests, and the NeoForge and JDK oracles.

## Product Purpose

Universal Mod Translator turns NeoForge 1.21.1 mods and whole modpacks into native Rust mods for Horizon,
the Rust rewrite of Minecraft: Java Edition. You drop in a mod or a pack, and it translates the mods' Java
bytecode into Rust and builds it into the game. There is no JVM at runtime, because performance is why
Horizon exists. The headline goal is universality: any mod, not a list of supported ones. Create, with its
addons, is the proving case: it must run.

## Positioning

The translator never runs Java. It rebuilds each mod's bytecode, its mixins and its bundled libraries as
Rust. A compat runtime (`horizon-compat`) answers the Minecraft, NeoForge and Java library calls natively
against Horizon's engine. Its coverage is measured, not claimed: every call that every jar of a real
367-jar modpack makes is probed against the runtime, and the result is recorded per run.

## Operating Context

- CLI: `horizon-mod-translator coverage | mixins | weaves | translate | bundle`. `horizon-port-pack`
  carries a pack's content (assets, data, recipes). `horizon-mixins` applies the bundles to a copy of
  Horizon's tree, which is then built with Cargo.
- The measurement pack is CurseForge's "Create Ultimate Selection 2 + Create Aeronautics + addons"
  (367 jars), read in place, never copied into the repo.
- Reference sources: NeoForge 21.1.256's and Minecraft 1.21.1's bytecode (`javap`), the libraries' own
  jars, and JDK 21 oracles for java.* behaviour.
- The work is a log of numbered Parts in `docs/mod-translator.md` (126 as of 2026-11-08), each committed
  and pushed to the private repo `GUNNA-at/horizon-mod-translator`, branch `mod-translator`.

## Capabilities and Constraints

- Native Rust only: no JVM, no bridge, no embedded Java.
- A general translator: fixes are made at the level of the API or library, never per mod.
- Mojang's files (jars, assets, decompiled sources) are never committed. Translations are built from the
  user's own jars.
- Not yet true, and never to be implied: that a whole pack runs end to end, or that Create works fully.
  Trains, mob AI goals, part of rendering and config screens are still open.
- The fork's root `README.md` is Horizon's own. The translator's README lives in `.github/README.md`.

## Brand Commitments

- Name: **Universal Mod Translator**.
- README in English, with a Russian translation (`README.ru.md`) linked at the top.
- It lives inside Horizon, so Horizon's name, mark and design system (`docs/design-system.md`,
  `crates/horizon-theme`) are Horizon's, used as Horizon uses them.

## Evidence on Hand

- 43 full-pack measurement runs (packcov), from 2026-11-04 to 2026-11-08: API methods answered, call
  sites answered, and Create's share; Java library coverage from run 39 on.
- Latest (run 43, Part 126): API methods 109046/130223 (83.7%), call sites 341371/372742 (91.6%);
  Create 3292/3971 methods (82.9%), 24391/25240 call sites (96.6%); libraries 61858/74081 methods
  (83.5%), 336963/357517 uses (94.3%).
- `docs/mod-translator.md`: every Part, with its measurements and what it checked against.
- Tests: `horizon-compat` 693, `horizon-mod-translator` 23, plus the differential test against the JVM.
- In game: Create's blocks place, render with Create's own models and tick. A recorded video exists (the
  owner's), not in the repo.
- Absent, and not to be invented: frame-rate or performance benchmarks, users, testimonials, release
  dates.

## Product Principles

1. Measure, don't guess: every claim of progress is a number from a run over the same pack.
2. Universal before particular: a gap is closed for every mod that has it.
3. Native or nothing: speed is the reason the project exists.
4. Vanilla, NeoForge and the JDK are the specification: behaviour is checked against their bytecode.
5. Show the gaps as plainly as the progress.
