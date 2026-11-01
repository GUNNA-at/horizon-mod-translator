# Mod translator: NeoForge jars to native Rust mods

Goal (owner request, 2026-11-01): drop a NeoForge mod or modpack in, get native Rust Horizon mods out.
No JVM at runtime: performance is the point of Horizon.

## Design

```
mod.jar ──► class parser ──► IR (stack → SSA-ish locals) ──► Rust codegen ──► <mod>.hnmod crate
                                                     │
                                calls to net.minecraft.* / net.neoforged.*
                                                     ▼
                               horizon-compat: Rust implementations of the
                               Java API surface mods use, mapped onto Horizon
```

1. **Content carry-over** (exists: `horizon-port-pack install`): assets, data, recipes, loot, tags, worldgen.
2. **Class parser** (`crates/horizon-mod-translator/src/class.rs`): constant pool, fields, methods, code.
3. **Registration extraction**: interpret static initialisers symbolically (`DeferredRegister`,
   `BlockBehaviour.Properties` chains, `Item.Properties`) into Horizon registry entries. Covers the code
   of most data/logic-tier mods without generating Rust at all.
4. **Bytecode → Rust codegen** for behaviour methods (use, tick, place, events), against `horizon-compat`.
5. **horizon-compat**: the Java API surface (Block, Item, Level, BlockPos, ItemStack, events) mapped onto
   Horizon's native hooks (`native-hooks.json`). Grows by coverage reports: the translator lists every
   unmapped Java method per mod, so we implement the most-called first.
6. **Create and friends** need block entities, kinetic networks, contraptions and custom renderers in
   Horizon's native mod API first; they are the last milestone.

## Progress (update after every step; commit on branch `mod-translator`)

- [x] `install` command: jar/folder/pack → `horizon-mods/` (commit f1dbd01). Tested on Macaw's Fences
      (150 blocks, 222 recipes) and Create (content refused: code). Not yet checked in game: needs
      `versions/26.3/26.3.jar` in the Minecraft folder (play 26.3 once) to run `horizon-prepare`.
- [x] Class parser + API coverage report (`cargo run -p horizon-mod-translator -- coverage <jar>`).
      Measured: Macaw's Fences 19 classes / 89 API methods; Create 2692 classes, 24833 methods,
      3579 distinct API methods. Top Create calls: BlockState.getValue/getBlock/setValue,
      Direction, Level.getBlockState/getBlockEntity, ItemStack, BlockPos, Vec3, CompoundTag.
- [x] Instruction decoder (`insn.rs`, tested) and registration extraction from code
      (DeferredRegister, Registry.register, Registrate). Macaw's Fences: 180 blocks + 180 items (matches
      carry). Create: 187 blocks, 114 block entity types, 53 items.
- [ ] Feed code registrations into the carry (blocks the data alone misses)
- [x] Codegen core + horizon-compat runtime: `codegen.rs` translates all non-exception opcodes;
      differential test (`cargo test -p horizon-mod-translator --test differential`) matches the JVM on 47
      results (loops, long/float math, switch, arrays, objects, virtual + super dispatch, statics).
      Gaps: invokedynamic (lambdas, string concat), try/catch, multianewarray, dup2_x2, instanceof over
      superclasses.
- [x] invokedynamic: lambdas (captures, method refs, ctor refs) and string concat, Java number formatting;
      differential now 54 results.
- [x] Exceptions: handler tables, propagation through calls (`rt.thrown`), hierarchy-aware catch and
      instanceof (`init(rt)` registers supers), ArithmeticException; differential 59 results.
- [x] `translate <jar> <out>` CLI: writes a crate (split into part*.rs). Macaw's Fences: 16.7k lines,
      compiles clean in 9 s. Create: 1.16M lines in 69 files; unsupported: record ObjectMethods (487),
      SwitchBootstraps.typeSwitch (1), multianewarray (2). Create's translated crate compiles with 0 errors (debug, 136 s).
- [x] Records (ObjectMethods equals/hashCode/toString) and multianewarray; differential 65 results.
- [x] Runtime ↔ game: `horizon_compat::host::Host` (MemoryHost for tests), `glue::{tick_block, random_tick,
      neighbor_changed}`, generated `init` (callbacks, `overrides` table) and `start` (runs registering
      classes' static init, resolves DeferredRegister suppliers), lazy `<clinit>`, API for DeferredRegister,
      Level get/setBlock, BlockState, Blocks.*, builder chains. Test `--test hook`: a NeoForge-style mod
      compiled against our stubs registers `testmod:grow` and its tick changes the world; 0 unmapped.
- [x] Native-mod bundle: `horizon-mod-translator bundle <jar> <out>` writes horizon-mixin.toml (HEAD
      injections on tick_block, random_tick), helpers implementing Host over GameLevel, and `engine/` (the
      translated code + horizon-compat inlined as `engine::compat`: horizon-mixins allows bundle crates
      only registry deps, no path/workspace/package). New GameLevel helpers (API 81):
      `native_state_text`, `native_set_state_text`. horizon-mixins (pinned ddd9700) applies the test
      bundle to a symlink-free copy of the tree: status ok. Modded game builds (fast profile).
- [x] IN GAME: the bundle step also writes a content mod for code-registered blocks (template
      grass_block for random ticks; this build has no `random_ticks` field). Test mod jar → bundle →
      horizon-mixins apply → modded horizon-client → launched offscreen (horizon-ctl, fresh HORIZON_HOME,
      `--dev-mod content`, resources prepared from the official 26.3 client/server jars + libraries,
      SHA-1 checked, kept in scratch): `/setblock 0 120 0 testmod:grow`, random_tick_speed 1000 → the
      translated randomTick/tick put stone above, then dirt. Repro: see "In-game test" below.
- [x] `horizon-port-pack install` translates every jar's code too (single jars, folders, packs via
      inventory jar paths) into `<game>/horizon-native-mods/<id>_translated/`, plus the content mod for
      code-registered blocks. Drop of Fences + Create: both translated; Create's only gap is one typeSwitch.
- [x] API coverage metric in `coverage` (each API method called once through the runtime). api_common:
      state properties (vanilla + mod bool/int/enum, interned identity), CompoundTag, Vec3, ItemStack/Item,
      Mth, RandomSource, Component, Enum, Optional. Create 28.1% → 46.0% of class uses mapped
      (594/3579 methods); Fences 56% → 67%. `MISSING=1 ... coverage <jar>` lists the next ones.
- [x] Block entities in the runtime: `glue::{block_entity_id, block_entity_ticks, block_entity_tick,
      block_entity_removed}`, NBT bridge (`compat::nbt::Nbt`, typed puts), BlockEntity/BlockEntityType.Builder/
      createTickerHelper/Level.getBlockEntity, inherited-member resolution up mod superclasses (javac names
      the mod class for inherited statics), API base constructors. Hook test: counter BE ticks, saves
      `count`, survives losing its live object, acts on tick 3.
- [ ] Next: wire block-entity hooks in the bundle (Game-level Host helpers + Nbt<->Compound); a `horizon-native-mods` bundle per mod whose hooks
      (tick_block, use_item_on, neighbor_changed, place_block, block_entity_tick...) call the mod's overrides
      through `vcall`; (2) grow `horizon-compat/src/api.rs` by the coverage list (Level, BlockState,
      ItemStack, CompoundTag, Vec3 first); (3) API callbacks into lambdas (DeferredRegister suppliers);
      (4) in-game test (needs versions/26.3/26.3.jar for horizon-prepare).
- [ ] Old: then a `translate` CLI that emits a
      native-mod crate wired to hooks.
- [ ] (old plan item) Plan: `crates/horizon-compat` = runtime (V enum: I/J/F/D/Ref,
      object heap, static fields) + API impls keyed by mangled `owner.name.desc`; unmapped → logging stub.
      Codegen (`horizon-mod-translator/src/codegen.rs`): one Rust fn per Java method, body = `loop { match pc }`
      state machine over decoded insns; invokes on mod classes → generated fns with a class-name vtable;
      invokes on API → `horizon_compat::api::call`. Hooks: wire overridden Block/BlockEntity methods to
      native hooks (tick_block, block_entity_tick, use_item_on, neighbor_changed, update_shape, place_block).
      Typed locals from descriptors later (perf).
- [ ] In-game verification

## Resume

`git checkout mod-translator`, read this file, take the first unchecked item.

## In-game test

```sh
cargo run -p horizon-mod-translator -- bundle testmod.jar out/          # out/{horizon-mixin.toml,*.rs,engine,content}
horizon-mixins apply --workspace <symlink-free copy of this tree> --mods <dir with out/ minus content> --out stage
(cd stage && cargo build --profile fast -p horizon-client --bin horizon-client)
horizon-ctl launch --game stage/target/fast/horizon-client --world T --creative --offscreen --wait \
  --resources <prepared> --dev-mod out/content --game-dir <scratch>
```
Pass `--game-dir`: the game folder defaults to the working directory.
