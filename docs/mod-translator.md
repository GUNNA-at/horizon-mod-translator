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
- [x] Block-entity hooks in bundles (block_entity, block_entity_ticks, block_entity_tick on Game; Game helpers
      `native_state_text_at` / `native_set_state_text_at`, API 81; Compound<->Nbt keeping unmodelled tags).
      IN GAME: `testmod:counter`'s translated block entity ticks each server tick and places stone on its
      3rd tick; random ticks of mod blocks are always the mod's (template behaviour suppressed).
- [ ] Next: a `horizon-native-mods` bundle per mod whose hooks
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

The scratchpad (`/private/tmp/...`) is cleaned by macOS of files nobody read for about three days: on 6 November it
took the prepared resources, the horizon-mixins clone's working tree and parts of the build folders. After a pause,
run `scratchpad/keepalive.sh` (the round scripts run it); `git checkout -- .` restores a clone, and
`target/fast/horizon-prepare --prepare --minecraft-dir "$HOME/Library/Application Support/minecraft"` the resources
(17 s with `.local` cached).

Before a PR: `tools/check_public_tree.py` rejects every tracked `.java` outside `mods/horizon/src/`,
and the translator's test fixtures (`crates/horizon-mod-translator/tests/fixtures/`: original test
sources and NeoForge API stubs the tests compile) are `.java`. The maintainer decides how to allow
them (a path in the checker's allowlist, or the fixtures built from another form).

## Fast loop: the harness (use this first)

The runtime is two crates: `horizon-compat-core` (the object model: `Rt`, values, heap, dispatch
tables) and `horizon-compat` (the API mapping). The game bundle inlines both (direct calls). For
tools, `bundle <jar> <out> --runtime <path to crates/horizon-compat-core>` makes an engine that
depends on the core crate only and reaches the mapping through `Fns` (`horizon_compat::install()`).
A harness that links that engine plus `horizon-compat` rebuilds only the mapping crate and relinks
when a mapping changes; the engine recompiles only when generated code or the core changes
(~10 min for Create at opt-level 1). The bundler rewrites only changed files, so don't delete the
bundle directory between runs.

```sh
horizon-mod-translator bundle create.jar cb-h --runtime $PWD/crates/horizon-compat-core
# harness/Cargo.toml: engine = { package = "create-translated-engine", path = "../cb-h/engine" },
#                     horizon-compat = { path = ".../crates/horizon-compat" }, [profile.release] opt-level = 1
# main: horizon_compat::install(); engine::with_runtime(|rt| ...) ; drive glue::* with host::MemoryHost
CARGO_INCREMENTAL=0 cargo build --release && HORIZON_COMPAT_TRACE=1 target/release/create-harness
```

Keep changes in the mapping (`horizon-compat`) where possible: a change to `horizon-compat-core`
rebuilds every engine. Minecraft's class families for `instanceof` are the mapping's
(`api_common::api_is_a`) for that reason.

## In-game test

```sh
cargo run -p horizon-mod-translator -- bundle testmod.jar out/          # out/{horizon-mixin.toml,*.rs,engine,content}
horizon-mixins apply --workspace <symlink-free copy of this tree> --mods <dir with out/ minus content> --out stage
(cd stage && cargo build --profile fast -p horizon-client --bin horizon-client)
horizon-ctl launch --game stage/target/fast/horizon-client --world T --creative --offscreen --wait \
  --resources <prepared> --dev-mod out/content --game-dir <scratch>
```
Pass `--game-dir`: the game folder defaults to the working directory.

### 2026-11-01: Create in game
- Game built with Create's full translated engine (`--profile fast`, 13.5 min, 170 MB binary): builds and runs.
- Create's content mod loads (fix: blocks whose properties sort before `snowy` use the `minecraft:stone` template).
- `/setblock` works for shaft, cogwheel, andesite_casing, gearbox and hand_crank, and the game keeps ticking.
- [x] Create blocks render with Create's own models (shaft, cogwheel, large cogwheel, casing, gearbox, hand crank, verified
  by screenshot). The earlier "invisible" was a misread: the player had fallen below the blocks.
- [x] Every block uses template `minecraft:stone` with `properties.random_ticks = true` (grass_block made casings spread and
  turn to dirt).
- [x] Every Create blockstate loads: `props.rs` follows mod-to-mod property aliases, fields inherited from vanilla
  superclasses (PipeBlock.NORTH...), and vanilla superclass definitions (RotatedPillarBlock gives axis...). Verified in
  game: fluid pipe, radial chassis, whistle extension, girder shaft, funnel, press and basin render.
- [x] Bundles inject `right_click_block` → `glue::use_block` (main hand; a click the block takes cancels the game's own use).
  Verified in game with `horizon-ctl interact`: testmod:switch toggles powered=false→true→false and places stone above.
- [x] `HORIZON_COMPAT_TRACE=1` prints each unmapped API call once, from a real game. Create's first run showed
  Java collections, its bundled libraries (Registrate) and NeoForge loading were missing, so no Create block got a
  block entity.
- [x] `jutil.rs`: java.util collections, iterators, streams, atomics, Guava Table/Multimap, natively.
- [x] `neo.rs` + codegen: the `@Mod` class is constructed with a native mod bus and `RegisterEvent` fires per registry;
  `META-INF/jarjar` libraries translate with the mod (Create: 1.39M lines with Registrate, Flywheel, Ponder).
- [x] Method resolution follows the JVM: superclasses, then inherited interface default methods (Registrate's builders).
- [x] Enums: `array.clone()` (so `values()`), `name()`/`compareTo` read `$name`/`$ordinal`; string concat,
  `String.valueOf(Object)` and `append(Object)` call a mod object's own `toString()` (generated `java_string`).
- Memory: compiling Create's engine at opt-level 3 swapped for hours on this 24 GB Mac; the harness builds with
  `CARGO_PROFILE_RELEASE_OPT_LEVEL=1`. Stop leftover offscreen games before a build.
- [x] 2026-11-02 harness: Create registers 515 blocks, 550 items, 114 block entities, 8 fluids, no uncaught
  exceptions; shaft/creative_motor/cogwheel resolve to Create's classes with block entities that tick. Fixes: the
  `DeferredHolder` super constructor (Registrate's `RegistryEntry`), `Lazy`, `Registry.register`, loggers, `Pair`.
- [x] Runtime split into horizon-compat-core + horizon-compat; `bundle --runtime` for the harness (see Fast loop).
- [x] `instanceof`/`isAssignableFrom` follow interfaces (generated init registers them) and Minecraft's families.
- [x] Annotations parsed; `@EventBusSubscriber` classes and `bus.register(Class|obj)` listen with `@SubscribeEvent`
  methods; every `@Mod` class is constructed (bundled mods first); `NeoForge.EVENT_BUS`; `LevelEvent.Load` fires
  when the level is first used. Hook test covers a static subscriber and a registered object.
- [x] Block entities: `BlockEntityType.create` (the factory), the Java fields (`level`, `worldPosition`,
  `blockState`), the glue puts each in the level with its current state; `ModConfigSpec` values are their defaults.
- [x] Reflection (commit f3eec22): a generated `reflect(rt, class, what, args)` table initialises classes
  (`Class.forName`), runs no-argument constructors (`newInstance`, `getConstructor().newInstance()`, Flywheel's
  `DependencyInjection`), returns enum constants (`getEnumConstants`, Create's `readEnum`) and lists
  `META-INF/services` providers of the jar and its jarjar (`ServiceLoader`, catnip's platform helpers).
  `Class.forName` of a class neither the mod's nor the platform's throws `ClassNotFoundException` (optional
  dependencies). `Object.hashCode/equals` are Object's own (identity), so `super.hashCode()` in an override no
  longer recurses; `Rt::call_functional` dispatches mod objects by `name.desc`. Differential: 78 results.
- [x] Dispatch by class id (commit 59f833f): objects carry their mod class's id (`Object::cid`, generated
  `CLASS_IDS`); each instance method some mod class implements gets a dispatcher `d_<name>_<hash>(rt, cid,
  owner_key, args)` that jumps on the id (runs of ids as ranges), plus `o_...(cid)` (overrides). Lambdas carry
  `LAMBDA | id` and `lambda_call` matches on the id. `instanceof` answers are cached by name addresses.
  Calls through a mod class to an API-only method name the API class at translation time.
- [x] Static fields resolve to the declaring class (JVMS 5.4.3.2): `CreativeMotorBlock.FACING` read null before,
  so the motor turned the wrong way and no shaft connected. Differential case `staticFields`.
- [x] Registries read back (`getKey`, `get`, `containsKey`, `getHolder`, `keySet`, `holders`...), `registryAccess`,
  `Holder` reads, `ChatFormatting` constants, `CompoundTag.isEmpty/size/getAllKeys/copy`.
- [x] `HORIZON_COMPAT_TRACE=1`: an uncaught exception prints the methods it left (`at Class.method`).
- Harness numbers before the static-field fix: 40 ticks of 4 kinetic block entities in ~30 ms (unoptimised
  string dispatch); motor Speed -16 (null FACING), shafts 0.
- [x] 2026-11-02 harness kinetic test PASSES: creative motor east + 3 shafts along x, 40 ticks: motor Speed 16.0,
  every shaft Speed 16.0 with a Network id (Create's own RotationPropagator, KineticNetwork, stress values). 40
  ticks of the 4 block entities take 7.8 ms (class-id dispatch; 30 ms before), ~13 objects allocated per block
  entity tick. Engine compile 5 min (9 before). Load (all registrations) 0.2 s.
- [x] Garbage collector (commit 9774764, `horizon-compat-core/src/gc.rs`): mark and sweep from the runtime's roots
  (statics, registries, interned values, strings, block entities, listeners, suppliers, `Rt::pinned`), freed slots
  reused. It runs at a hook's outermost entry (`Rt::safe_point`, no translated frame live) once the live count passes
  `collect_at`. Stress check in the harness (`GC_STRESS=1`: a collection at every hook entry): after load 33,641 of
  83,701 objects stay (3 ms per collection); 40 kinetic ticks later 33,730 are live and every Speed is still 16.
- [x] NBT tag model (`tags.rs`): `CompoundTag` keeps each value's kind, `ListTag`, numeric/string/array tags,
  `NbtUtils` (block pos as an int array, UUIDs, block states); the block-entity hook carries lists and arrays to the
  game's compound both ways, so `Source` (a shaft's int-array source position) is saved.
- [x] Level services while ticking: `tickRateManager` (never frozen), `getChunkSource().blockChanged`,
  `dimensionType` (the overworld's), `getServer` (`getTickCount`, `overworld`), `sendParticles`, `Vec3i.ZERO`.
- [x] 2026-11-02 IN-GAME kinetic test PASSES (`scratchpad/kinetic.sh`, game built with the regenerated bundle,
  8.5 min when only the engine and horizon-client recompile): `/setblock` a creative motor (facing east) and three
  shafts, then `data get block`: motor `{Speed: 16.0f, Network: {Capacity: 262144.0f, AddedCapacity: 16384.0f,
  Id: -7971459268543L, Stress: 0.0f, Size: 1}}`, shaft `{Speed: 16.0f, Network: {Size: 3, Id: (the same)},
  Source: [I; -28, 65, 8]}`. Create's own kinetic code runs natively in the Rust game's server thread.
  Staging tip: apply mixins into a fresh folder, then `rsync -r --checksum` (no `-t`) into the old stage, so
  unchanged files keep their times and cargo rebuilds only what changed.
- [ ] Next: the client half. Rotating parts are drawn by Create's block entity renderers
  (`KineticBlockEntityRenderer`, catnip's `SuperByteBuffer`), which need: a client runtime (the render thread's own
  `with_runtime`, `FMLClientSetupEvent` registering renderers), block entity update tags to the client
  (`native_block_entity_update_tag`, `blockChanged` → `native_block_entity_sync`), the client tick
  (`ClientTickEvent`, catnip's `AnimationTickHolder`), and drawing through `native_block_entity_submit`
  (PoseStack, JOML, catnip's buffers as natives: one draw call per buffer, no per-vertex translated code).
  - [x] Runtime side (commit 18dff8f): setup events (common, client, `RegisterRenderers`), `enqueueWork` and
    `CompletableFuture`, PoseStack/JOML/Axis natives (`horizon-compat/src/render.rs`), catnip's
    `SuperByteBuffer` as intrinsics drawn through `Host::draw_model` (one call per buffer), and the glue:
    `glue::render_block_entity`, `glue::block_entity_update_tag`, `glue::client_tick`, client `level`.
  - [x] Translator intrinsics: a method whose key is in `render::INTRINSICS` is emitted as
    `api::call(rt, KEY, args)` (codegen `method()`).
  - [x] Harness client test passes: the motor and three shafts draw (4 of 4) on the client's own runtime,
    each turning between frames at Create's rate (speed × 0.3 degrees a tick, plus its 22.5° offset on
    alternate shafts); the motor's `shaft_half` partial model faces east. 4 draws take ~130 µs a frame.
    Fixes on the way: Guava caches (`CacheBuilder`, `Cache.get(key, Callable)`, `LoadingCache`) and
    `MapMaker`; fastutil maps' `defaultReturnValue` (its absence made Flywheel's layout builder throw in
    `CreateClient`'s constructor); catnip's real buffer is `ShadeSeparatingSuperByteBuffer` (the legacy
    `DefaultSuperByteBuffer.rotate` returns null); `Quaternionf.setAngleAxis`, `Direction.toYRot`,
    Flywheel's `TransformStack.of` (a mixin method on `PoseStack`), `Collections.binarySearch`.
  - [x] Bundle hooks (`bundle.rs`, commit 4393432): `block_entity_submit` (Host: `partial_tick`, `draw_model`:
    push, multiply the pose, draw, pop; light from `LightTexture.pack` to Horizon's `sky16 | block16 << 8`),
    `block_entity_update_tag` (live tag, else the saved nbt), `tick` (client tick when `native_window()`),
    and `block_changed` → `native_block_entity_sync` in the server hosts.
  - [x] Horizon: `BlockEntityDraw::transform([f32; 16])` and `Game::native_draw_model(draw, name, tint)` (named
    models baked once per model library; fragment `block_entity_draw_model.json`).
  - [x] 2026-11-02 IN-GAME render test PASSES (`scratchpad/render.sh`): a creative motor and four shafts on a
    cleared stone floor, two screenshots 0.35 s apart: the shafts turn (their stripes move) and the motor
    draws its `shaft_half` partial facing east. Create's renderer code runs natively on the window thread.
    Test tips: the world's seed changes per run, so clear an area and lay a floor; stand the camera on that
    floor (a teleport into the air falls during the wait); wait ~11 s so the chat fades.
  - [x] Static models under rotating parts (harness-verified; in-game check pending): content property
    `render_shape` (`model`/`entityblock_animated`/`invisible`, all states or by one own property), set
    at startup by the hook `native_content_block` from the mod's own code (`glue::render_shape`, on a
    runtime of its own, `with_probe_runtime`, since it fires the client's setup): `getRenderShape` of
    each state (one property varied at a time), and for models swapped in `ModelEvent.ModifyBakingResult`
    (probe models at every block's location) a wrapper with no quads under empty model data whose block
    entity renderer draws something. Result for Create: shaft, cogwheel, large cogwheel, hand crank,
    valve handle, flywheel, crushing wheel, gantry shaft, powered shaft, turntable, large water wheel
    → `entityblock_animated`; water wheel structure → `invisible`; doors, copycats, factory gauge keep
    their models (a door's `visible` isn't a property of the content block: an answer read from a
    property the state lacks is unknown; data-driven models have no quads without their data).
    Fixes found on the way, each silently breaking much more than models:
    - `Dist` values were fresh strings, so catnip's `FMLLoader.getDist() == Dist.CLIENT` was false and
      every `executeOnClientOnly` block was skipped. Now one enum object per constant.
    - Pairs compared by identity, so Registrate never found the `onRegister` callbacks it files under
      `Pair.of(name, registry)`: none of Create's register callbacks ran (model swaps, casing
      connectivity, movement/interaction behaviours...). Pairs are values now (equals, hashCode).
    - Constructor references (`BeltBlock::new`, how Registrate makes every block) didn't run the
      class's static initialiser, and a class's own methods skip it, so `BeltBlock.CASING` read null.
      `new` through a reference initialises as `new` does, and initialising a class initialises its
      mod superclasses first (JVMS 5.5).
    - Guava `MultimapBuilder`, `ImmutableList/Set/Map.builder()`.
    - Block entities were loaded from the game's compound even when empty (a block just placed):
      once Create's behaviours were found (their `TYPE` keys had been null), the creative motor's
      speed setting read 0 from it. Now only saved data is loaded, as vanilla.
    - `DimensionDataStorage` (Create's train and logistics networks): in memory, one object per
      name. Gap: not written to the world's `data/` folder yet.
    Harness (`scratchpad/harness`): 531 blocks registered; motor + 3 shafts at 16 RPM with a
    network; 0 unmapped calls while ticking; the client's 4 parts turn between frames; the shape
    probe runs on its own runtime (it makes that runtime a client).
  - [x] Every block and item from the mod's own registration (commits 83dfd35, bd499b9; harness
    `DECLARE=<content dir>`): `blocks.rs` records `BlockBehaviour.Properties` / `Item.Properties` calls and
    each block's state definition, and the bundle's `native_content` hook declares them
    (`Content::declare_block/declare_item`). Create: 465 blocks (186 from files alone) and 107 items, none
    refused, with Create's hardness, sounds, map colours, stack sizes, states and defaults. Registrate
    composes property callbacks with `NonNullFunction.andThen`, whose body is a private interface lambda:
    private methods are now called directly however they are reached (commit 16e6333).
  - [x] Shapes (`shapes.rs`): `VoxelShape`/`Shapes`/`Block.box`/joins/`AABB`/`CollisionContext`; each block
    with its own `getShape` is asked for every state's outline (empty context, origin, no level) and
    declares it (content `shapes`: one list, by one property, or by state against a default; up to 64
    boxes). Full opaque cubes keep `minecraft:stone`, the rest take `minecraft:glass` (light reaches the
    block entity renderer's parts). Float noise from Create's rotated shapes rounds to 1/256.
  - [x] A block whose own property sorts before its vanilla template's (Create's glass pipe: `alt` before a
    pillar's `axis`) takes a stateless template. The game refused all of Create's content for that one
    block, so now a code-declared block the game can't build is left out with a warning and the rest
    loads (`Content::leave_out`, retried in `game/mods.rs`).
  - [x] 2026-11-02 IN GAME (`scratchpad/video.sh`, game built 21:25): Create's content loads (572 blocks and
    items declared, none refused, shapes per state), shafts are lit and textured on glass templates, a motor
    drives shafts into a cogwheel meshed with a second one: 16 RPM, then -16 RPM past the mesh (read with
    `data get block`). Frames differ on the gears and shafts, not on static blocks. Video:
    https://claude.ai/artifact/6ReCVHYJmEUnpjW2BxLhVs (ffmpeg from Homebrew is broken here;
    `scratchpad/encode.swift` writes the MP4 with AVFoundation). `scratchpad/play-create.sh` opens the
    same scene in a window for a person to play.
  - [x] Hand placement (harness `PLACE=1`): the bundle's `state_for_placement` hook builds a `BlockPlaceContext`
    (clicked face and position, look directions as vanilla's `Direction.orderedByNearest`, sneaking, item) and
    asks the block's own `getStateForPlacement`; `super` calls reaching vanilla get its rule (pillar axis,
    face-attached). Harness: shaft takes the clicked face's axis, motor faces a neighbouring shaft (else away
    from the look), cogwheel follows a shaft's axis, press and fan face as in Create. Gaps: every answer was
    `waterlogged=true` (no `getFluidState` yet; fixed by fluids below), and blocks inheriting vanilla's rule
    without overriding it (analog lever) aren't asked. New game helpers `native_state_id_text`,
    `native_state_from_text` (API 81).
  - [x] 2026-11-03 Fluids (`horizon-compat/src/fluids.rs`, 5 tests): vanilla's fluids (`Fluids.WATER`, one object
    per name, also from registry lookups), NeoForge's `BaseFlowingFluid` built from its `Properties` (the
    suppliers of source, flowing fluid, block, bucket and type; tick rate, slope, resistance), `FluidState` (one
    object per fluid, level and falling flag; its calls reach the fluid's own method when the mod's class has
    one, as `getType().isSource(state)` does in Java), `getFluidState` of a block state (the mod block's own
    override, else its vanilla class: a `LiquidBlock`'s level, `waterlogged`, kelp and the other always-wet
    plants; `Block` itself holds none) and of the level at a position, `LiquidBlock`/`BucketItem.getFluid`,
    fluid `TagKey`s (`FluidTags`, `Tags.Fluids`, `TagKey.create`; water and lava hold vanilla's members).
    Harness: `PLACE=1` now gives `waterlogged=false` in air; Create's common setup no longer stops on
    `Tried to register duplicate values for object null` (Potion and Tea fluids' `getSource()` was null), and
    kinetics still read 16 RPM. Gaps: fluid tags from data files, `FluidType` properties (density, viscosity),
    `FluidStack`, tanks and pipes; `create:lectern` display target fails to register (seen since 2026-11-02).
  - [x] 2026-11-03 Creative tabs (`horizon-compat/src/creative.rs`): `CreativeModeTab.builder()...build()` keeps the
    title, icon supplier, `displayItems` generator and `withTabsBefore/After`; the content hook runs each
    registered tab's generator into an `Output`, posts `BuildCreativeModeTabContentsEvent` for it, and declares
    the result as the content mod's `creative_tab` entries (search-only items left out). `Block.asItem()` finds
    the block item made for the block (`BlockItem` keeps `$block`), and `new ItemStack(itemLike)` now holds
    `itemLike.asItem()` (it held the block's name). Needed on the way, for every mod: fastutil's
    `ReferenceArrayList` and the other typed lists and queues are lists, `Predicate.or/and/negate/not`,
    `Function.andThen/compose/identity`, `Consumer.andThen` and Guava's `Predicates`/`Suppliers.memoize` are
    combinator objects (`functions.rs`), and a registry's `getTagOrEmpty` is empty (no data tags yet).
    Harness `TABS=1`: Create's `create:base` (icon cogwheel, 201 items) then `create:palettes` (ornate iron
    window, 308), with Create's own exclusions (incomplete items, placeable backtanks, contraption carts,
    crushed ores of absent metals). Not yet: items a mod adds to vanilla's tabs (they stay in search).
    The bundle now carries `fluids.rs`, `creative.rs` and `functions.rs` (the fluids commit left `fluids.rs` out).
  - [x] 2026-11-03 Seen in game (offscreen, 1600x900, `scratchpad/tabs.sh`): page 2 of the creative screen has
    "Create" and "Create's Building Blocks", filled from Create's code; a cogwheel placed from the hotbar lands as
    `create:cogwheel[axis=y,waterlogged=false]`; framed glass, tiled glass and the iron windows draw cut out (the
    wall behind shows through), with no render-layer call needed. Two bugs found there and fixed:
    - No Create item drew an icon: a 1.21.1 jar has no client item definitions (`assets/<ns>/items/`, 1.21.4+).
      The bundle now writes `{"model": {"type": "minecraft:model", "model": "<ns>:item/<name>"}}` for each
      `models/item/` model the jar lacks a definition for, as the port-pack's carry-over does (Create: 751).
    - Create's wooden windows all registered as `create:null_window`: `WoodType.OAK.name()` was unmapped.
      `WoodType`'s twelve constants now answer their names (test `wood_types_answer_their_names`).
    Still to do here: Create's connected glass textures (its own CT system), the cogwheel's item icon drawn
    small, one icon-less slot in the building-blocks tab.
  - [x] 2026-11-03 The wrench (`Item.useOn`), harness `WRENCH=1`: the bundle wires Horizon's `use_item_on`
    hook (a right click with one of the mod's items, after the block let it through) to `glue::use_item_on`,
    which builds a `UseOnContext` (class `UseOnContext`, `getClickedPos` = the block clicked) and runs the
    item's own `useOn`; anything but `PASS` claims the click. On the way: `Player.mayBuild` (true),
    `BlockState.canSurvive` and `updateShape` and `Block.updateFromNeighbourShapes` reach the mod block's own
    overrides (vanilla's order: west, east, north, south, down, up), `Item.useOn` on vanilla's `Item` is
    `PASS`, and `Direction.getClockWise/getCounterClockWise(Axis)` follow vanilla's X/Y/Z tables (the
    one-argument form fell to the builder rule and returned the direction unchanged, so nothing turned;
    test `directions_turn_about_each_axis`). Create's results: a shaft along x clicked on top turns to z, on
    its north face to y; a cogwheel along y clicked east turns to z; a motor facing east clicked on top faces
    south; a gearbox along y clicked north turns to x; stone passes.
  - [x] 2026-11-03 Both seen in game (offscreen, `scratchpad/wrench.sh`): a creative motor drives five shafts along x;
    the wrench on the third shaft's north face stands it up (`axis=x` to `axis=y`), and `Speed` reads 16, 0, 0
    along the line (16, 16, 16 before): the network ends at the turned shaft. Create's ten wooden windows now
    place as `create:oak_window` ... `create:warped_window`. horizon-ctl's `interact` aims at three points of
    the face; on a thin shaft's top face all three rays hit its side first, so test a side face.
  - [x] 2026-11-03 A dropped mod with machines keeps its translated blocks and gets its data
    (`horizon-port-pack install`): before, the carry-over (Create: "Partly", 542 blocks from vanilla templates,
    101 left out) replaced the translated content mod. Now a mod with block entity types keeps the content
    mod its code declares (a template would turn Create's smart chute into a pressure plate), with the
    carry-over's data merged in (`install::merge_data`: `data/<id>/` but `horizon/`, and additions to
    vanilla's and `c` tags; the content mod's own files stay). Mods without machines keep the carry-over as
    before. In the pack flow such a mod gets a carry-over of its own for the data. Create now installs with
    1810 recipes, 640 loot tables, 181 tag files and 1150 advancements.
    NeoForge's convention tags (`c:`), which Minecraft lacks and NeoForge's jar ships, are built in:
    `horizon-compat/src/neoforge_tags.txt` (NeoForge 1.21.1's generated `data/c/tags` for items, blocks and
    fluids, 370 tags, optional `forge:` entries left out; LGPL-2.1 data, check before upstreaming),
    `horizon_compat::convention`. The carry-over fills each `c:` item tag a recipe names (and those it names)
    into the mod's file for it. Create's andesite alloy names `#c:nuggets/iron`, which only NeoForge defines.
    Seen in game (`scratchpad/craft.sh`, survival): andesite and iron nuggets in the inventory's 2x2 grid make
    `create:andesite_alloy` (with the recipe-unlocked toast), and an andesite casing mined with an iron
    pickaxe drops itself through Create's loot table (by hand it drops nothing: it copies andesite's
    properties, as in Java).
  - [x] 2026-11-04 Recipes read by the mod's own codecs, natively (harness `RECIPES=<content mod>`): a
    serializer's `codec()` is the mod's own DFU code, and `horizon-compat/src/codecs.rs` gives it native
    codec objects that read JSON as DFU does (fields strict, `orElse`, `either`, `dispatch` handing the
    whole object to a map codec, records built through the mod's own constructor lambdas, `DataResult`,
    `Either`, `Pair`), with Minecraft's and NeoForge's codecs (`Ingredient`, `ItemStack`, `FluidStack`,
    `SizedFluidIngredient`, `ResourceLocation`...) read by `recipes.rs`. The game hands over a type's files
    (`Host::recipes_of_type`, load conditions held) and its tags (`Host::ingredient_items`); each recipe
    is read once per runtime and kept per type; `RecipeManager.getRecipeFor/getAllRecipesFor/byKey` answer
    from them, and an `Ingredient` tests a stack against the game's tags. Measured on Create: of its 879
    files of its own 17 types, 488 are off by their `neoforge:conditions` (other mods' compat recipes),
    and 387 of the other 391 read (all 8 pressing, 47 milling, 82 crushing, 167 deploying, 4 mechanical
    crafting, 3 sequenced assembly...), in 0.3 s. `getRecipeFor(PRESSING, SingleRecipeInput(iron ingot))`
    finds `create:pressing/iron_ingot` and Create's `rollResults` makes `create:iron_sheet x1` (gold ingot:
    golden sheet, sugar cane: paper, stone: none). Left: 3 filling recipes with NeoForge's components
    fluid ingredient (a potion fluid of given contents; fluid stacks have no components yet) and one
    mixing recipe with a block-tag ingredient (`neoforge:block_tag`, needs the game's block tags).
    Also new natively:
    - Vanilla's `ShapedRecipe` and `ShapedRecipePattern` (`crafting.rs`): read by vanilla's codec (pattern
      shrunk and checked as vanilla does, as large as NeoForge's `setCraftingSize` allows), matching a
      `CraftingInput` as written or mirrored, a mod's subclass of it included (Create's mechanical
      crafting); `SimpleCraftingRecipeSerializer` (special recipes: the mod's factory given the category);
      `CraftingBookCategory`; `CraftingInput`.
    - Mixin accessors and invokers, for any mod: the translator reads `@Mixin`'s target (kept only in the
      class file) and each method's `@Accessor`/`@Invoker`, and the call site reads the game object's
      field or calls the game's method directly (Create's `ShapedRecipeAccessor.create$getPattern`).
    - NeoForge's `ItemStackHandler` (`handlers.rs`), a mod's subclass's own `onContentsChanged`,
      `isItemValid` and `getSlotLimit` answering, `ItemHandlerHelper.insertItem/insertItemStacked`;
      `ItemStack.save/saveOptional/parse/parseOptional` in `ItemStack.CODEC`'s form.
    - NeoForge's `FluidStack`, `NonNullList`, and the ingredient kinds as the game reads them
      (`children` or `ingredients`, intersection and difference computed).
    - The player's hand: `Player.setItemInHand`, `getInventory().placeItemBackInInventory/add`, through
      `Host::take_player_items` and `give_player_items` (the game's `native_take_items`/`native_give_items`).
    Harness `PRESS=<content mod>` (motor, shaft, press two above a depot): the press turns at 16 rpm and a
    right click with an iron ingot puts it on the depot (the hand empties), but the press doesn't start:
    Create's `BeltProcessingBehaviour.isBlocked` asks the collision shape above the depot, unmapped.
  - [x] 2026-11-04 Create's press makes an iron sheet (harness `PRESS=<content mod>`): a creative motor
    turns a shaft and a mechanical press two above a depot (speed 16); a right click puts an iron ingot on
    the depot; the press starts when the ingot reaches the depot's centre and runs 2 cycle ticks per game
    tick, as Create computes for that speed. At half cycle (game tick 66) the depot holds
    `create:iron_sheet x1`. What it took:
    - Collisions: `BlockState.getCollisionShape/getShape(level, pos[, context])` answer from the game's
      own collision boxes for the block there (`Host::collision_boxes`, the game's
      `block_collision_boxes`), a mod block from its own `getShape`, air empty. Create's `isBlocked`
      asks what is above the depot.
    - Interface calls on API objects dispatch on the receiver's class, as on the JVM: a missed
      `Supplier.get` on a DeferredHolder answers as `DeferredHolder.get`. This holds for any interface
      and any API object: `api::call` tries the receiver's class and its supers after the named class.
    - `DeferredRegister.register`'s holder now keeps its registry. `create:pressing` is both a recipe
      type and a serializer, and `AllRecipeTypes.PRESSING.getType()` came back null because the holder
      knew only its name.
    - NeoForge's components fluid ingredient (`neoforge:components`): Create's 3 potion filling recipes
      now read. They match no fluid stack until fluid stacks carry components.
    Correction to the entry above: `isBlocked` was not the whole cause. Once it was answered, the press
    still found no recipe, because the recipe type was null.
    Checked in game too (`scratchpad/press.sh`, survival, the content mod `horizon-port-pack install` made):
    the press turns at 16 rpm; a right click with an iron ingot puts it on the depot (`HeldItem` in the
    depot's data) and takes it from the hand; 130 ticks later the depot holds `create:iron_sheet` and the
    press reports `Finished: 1b, Running: 0b` after its 240-tick cycle; an empty-handed right click puts
    the sheet in the inventory. Not drawn yet: the item on the depot and the press head's stroke
    (Create's block entity renderers, client side). Seen in the log: `MatchException` in two blocks'
    `getShape` (a pattern switch on a value the runtime doesn't give), to look at.
  - [x] 2026-11-04 Item data components, natively (`horizon-compat/src/components.rs`). A mod's
    component types (`DataComponentType.builder().persistent(codec).build()`, registered through NeoForge's
    `DeferredRegister.createDataComponents` or `registerComponentType`) and vanilla's `DataComponents`
    (one object per type). A stack's `has/get/getOrDefault/set/remove/update`, in vanilla's forms and in
    NeoForge's forms that take the type's holder. A stack keeps what was set on it, as vanilla's patch,
    and `copy/copyWithCount/split` carry it. `isSameItemSameComponents` and `matches` compare it: strings
    and boxed values by value, a mod's records by their own `equals`. Measured: all 57 of Create's types
    register with their codecs; before this they were null. Not yet: an item's default components
    (vanilla's prototype), and components in saved stacks or in stacks handed to the game (both need
    encoding through the type's codec).
  - [x] 2026-11-04 NeoForge's capabilities, natively (`horizon-compat/src/capabilities.rs`). The
    constants (`Capabilities.ItemHandler/FluidHandler/EnergyStorage.BLOCK/ITEM/ENTITY`, a mod's own
    `BlockCapability.create`). `RegisterCapabilitiesEvent`, fired once the registries are full, with
    its `registerBlockEntity/registerBlock/registerItem/registerEntity`. The lookups
    `Level.getCapability` (both forms), `BlockCapabilityCache` and `ItemStack.getCapability`: a block's
    own providers first, then its block entity's, in NeoForge's order. Measured on Create: 31
    registrations; `getCapability(ItemHandler.BLOCK, depot, UP)` is Create's `DepotItemHandler`, and an
    iron ingot inserted through it (what a funnel or chute does) lands on the depot, and the press
    above makes it a sheet. Not yet: vanilla containers (a chest or hopper as an `IItemHandler`, and the
    game's hoppers into a mod's block), which need the game's container slots through the Host.
  - [x] 2026-11-04 Items drawn by a mod's block entity renderer (harness `PRESS`, client step).
    `ItemRenderer.render/renderStatic` (both forms) go to `Host::draw_item`: the item, the
    `ItemDisplayContext` (its constants are mapped now), the pose stack's pose and the light. The game
    draws the item with its own item renderer (`native_draw_item`), as it draws an item in that
    context. `getModel(...).isGui3d()` asks `Host::item_is_block`, and `ItemStack.getMaxStackSize`
    asks `Host::item_stack_limit`; both come from the game's item facts (`native_item_facts`), where
    the stack limit was 64 for every item before. On the client runtime, Create's `DepotRenderer` draws
    `create:iron_sheet` (FIXED) at the depot's centre shifted by the item's side offset, and
    `MechanicalPressRenderer` draws the shaft and `create:block/mechanical_press/head`. Also: an API enum
    constant answers `Enum`'s methods through the receiver retry (`DyeColor.ordinal()`).
  - [x] 2026-11-04 Vanilla enum properties as enum constants, and vanilla state definitions for more
    classes (harness `DECLARE`, `blocks::tests`). A vanilla enum property's value (`BellBlock.ATTACHMENT`,
    `SlabType`, `ChestType`, `AttachFace`...) is now the interned enum constant of its class, the one its
    static field reads, with `name()`, `ordinal()` in vanilla's order and `values()`; it was the value's
    text, so a `switch` over it (`AbstractBellBlock.getShape` through `$SwitchMap$...BellAttachType`)
    threw `MatchException` and the bell had no shape. A mod class extending a vanilla block that doesn't
    write its own `createBlockStateDefinition` gets that class's properties: bells, lecterns,
    scaffolding, chests, barrels, hoppers, furnaces, anvils, grindstones, panes, dispensers, repeaters,
    comparators, snow, leaves, crops, saplings, beds, cakes, candles (classes whose constructor
    defaults aren't the property's first value, a campfire's `lit=true`, are left out until defaults
    are read from the constructor). Create's haunted and peculiar bells now declare
    `facing/attachment/powered` with `facing=north,attachment=floor,powered=false`, as vanilla's.
  - [x] 2026-11-04 Item entities (`entities.rs`; tests `entities::tests`; in-game check pending).
    The server level's item entities are objects for translated code: `Level.getEntitiesOfClass` /
    `getEntities` over a box (one object per entity while queries keep returning it, with its
    NeoForge persistent data), `getItem/setItem`, `discard/kill/remove`, motion and position,
    `new ItemEntity(...)` + `addFreshEntity`, `Block.popResource`, `Containers.dropItemStack`. The
    Host gained `item_entities`, `set_item_entity`, `move_item_entity`, `spawn_item_entity`; the game
    gained `native_move_item_entity`, `native_spawn_item_entity` and `native_game_time` (the hooks
    name the level with the existing `native_dimension`), and `NativeItemEntity` carries motion,
    `on_ground` and age. Contacts: a mod
    block with a block entity that overrides `updateEntityAfterFallOn` or `entityInside` is told
    of item entities resting on it or in its cell from its block entity's tick (vanilla's
    `Entity.move` reports them to the block; Create's basin, belt, depot, chute, saw, millstone,
    crushing wheel and funnel take items this way). An item resting in the cell counts as on the
    block, since the game holds it on the general collision box where Create's item-only shape
    (a basin's raised floor) would put it on the block itself.
    Also traced (with `HORIZON_COMPAT_TRACE`): the first update tags the server sends and the
    client loads for each block entity, with the items they hold.
  - [x] 2026-11-04 Fluid tanks, whole `Optional`, combined item handlers, the mod list (`tanks.rs`;
    tests `tanks::tests`). `FluidTank` natively as NeoForge writes it (`fluid`, `capacity`,
    `validator` fields; `fill/drain` both forms with `FluidAction.EXECUTE/SIMULATE`,
    `getFluidInTank/getTankCapacity/getTanks/isFluidValid`, `getSpace`, `readFromNBT/writeToNBT` as
    `{Fluid: {id, amount}}`), a mod's subclass's `onContentsChanged/isFluidValid` called through its
    code; calls through `IFluidHandler`/`IFluidTank` on a tank answer too. `java.util.Optional` is
    whole: `of/ofNullable/empty` made one (they fell to the builder-chain fallback and came back
    empty), `ifPresent/ifPresentOrElse/map/flatMap/filter/or/orElseGet/stream`, and `get/orElseThrow`
    on an empty one throw as the JDK does. That strictness showed an unmapped `ModList`: the
    translator now lists the engine's `@Mod` ids (`Rt::mods`), and `ModList.isLoaded`,
    `getModContainerById` (catnip's packet registration takes the mod bus from it) and
    `ModContainer.getModId/getEventBus` answer from it. Also `CombinedInvWrapper` (a basin's input and
    output as one handler), `Mth.equal`, `ItemStack.isComponentsPatchEmpty`,
    `BlockPos.betweenClosed/betweenClosedStream` (x fastest, as vanilla walks it). Found from the
    mixer harness (`MIX`): Create's basin takes items through `withBlockEntityDo` →
    `getBlockEntityOptional(...).ifPresent(...)`, which did nothing.
  - [x] 2026-11-04 `Direction.Plane` walks its faces (`for (Direction d : Plane.HORIZONTAL)`: north,
    east, south, west; `stream`, `forEach`, `getRandomDirection`, `shuffledCopy`); such loops did
    nothing before (`Iterable.iterator` on a plane was unmapped). `Vec3.equals/hashCode` as vanilla's.
    fastutil's `synchronize/unmodifiable` wrappers hand back the collection: Create's
    `SimpleRegistry` (stress values, display targets, spouting and movement behaviours) is built on
    `Object2ReferenceMaps.synchronize(...)`, which came back void, so nothing registered in it.
    `values()` of `DyeColor`, `WeatheringCopper.WeatherState`, `ArmorItem.Type`; `Direction.name()`.
    Guava's `ImmutableList.of(e1, ..., e12, E... others)` takes the rest from its array (the array
    was one element: Create's package styles, 14 of them, iterated an array as a style).
  - [x] 2026-11-04 The collector's iterator pass ends (`gc::tests`): a live iterator's source was
    pushed again on every round even when already marked, so a collection with one live iterator
    never finished (the mixer harness hung in `Rt::collect` once `Plane` loops ran). Also
    `ArrayUtils.contains/indexOf/isEmpty` (Create's `CopperBlockSet.hasVariant`: the copper sets
    threw once `WeatherState.values()` answered, and `AllBlocks` stopped there), Guava's
    `HashBiMap.create` and `inverse`, `DyeColor`'s methods (name, id, texture/firework/text colours,
    map colour, `byId/byName/byFireworkColor`), `ArmorItem.Type.getName/getDurability`.
    Mapping an enum's `values()` makes loops run that never ran: each such step needs the calls
    inside the loop mapped too, or the loop throws where it used to do nothing.
    Vanilla's `Util.make(supplier)` / `make(object, consumer)` / `memoize` / `getMillis`: all of
    `Util` was unmapped, so `Util.make(...)` came back void (Create builds `SimpleRegistry`s and
    display sources this way).
  - In game (2026-11-04 build): a dropper over a depot drops an iron ingot, the item entity lands on
    the depot and the depot takes it (`HeldItem: {Item: iron_ingot}`, the server sends the update tag,
    the client loads it). The client's depot renderer doesn't draw it yet (no `draw_item` call);
    harness `DROP` repeats that path.
    Harness `DROP` (a depot with an ingot resting on it, 30 server ticks, then the client renders from
    the update tag): the depot holds the ingot and the client draws it at the depot's top
    (`items drawn [iron_ingot at 0.5, 0.84, 0.5]`), so the game-side gap is in the game's path.
  - [x] 2026-11-04 Primitive fields nothing wrote read as their type's zero (`getfield`/`getstatic`
    on an `int`, `long`, `float`, `double`, `boolean`... field), as the JVM's defaults. Before, they
    read as unset: `0` in arithmetic, but saved to NBT they vanished (`putInt("Angle", angle)` wrote
    nothing). Reference fields stay unset (null to the code).
  - [x] 2026-11-04 `java.util.Arrays`: `sort/parallelSort` (whole or a range; primitives by value,
    doubles and floats in `Double.compare`'s order, objects stable through `compareTo` or the
    comparator), `fill`, `copyOf/copyOfRange` (padded with the type's zero), `equals`, `hashCode`
    (Java's values). `Arrays.copyOf` gave a list before. `Collections.sort/List.sort` use the same
    stable merge sort (was insertion sort). Found from the mixer: Create's recipe trie sorts each
    recipe's ingredient ids (`Arrays.sort(int[])`) before it inserts them.
    `Level.potionBrewing()`: a `PotionBrewing` that knows its containers (potion, splash, lingering);
    its mixes aren't modelled (no mix matches; NeoForge's brewing recipes and Create's accessor
    `create$getPotionMixes` give empty lists), so the mixer doesn't brew yet.
    NeoForge's `CompoundIngredient.of(children)`: every child's items.
  - [x] 2026-11-04 A map's computed primitive value is kept: `computeIfAbsent` (and `putIfAbsent`,
    `computeIfPresent`, `merge`) took a fastutil map's `int` value for null, so it never stored it and
    computed it again on every call. Create's recipe trie numbers ingredient variants with
    `Object2IntOpenHashMap.computeIfAbsent(v, k -> next++)`: its variant map stayed empty, so the
    trie found no recipe for the basin's items and the mixer never started (`MIX` probes:
    `matchBasinRecipe` of `mixing/andesite_alloy` was already true).
    `java.util.Random` natively: the JDK's 48-bit LCG (`nextInt/(bound)/(origin, bound)`, `nextLong`,
    `nextBoolean`, `nextFloat`, `nextDouble`, `nextGaussian`, `nextBytes`, `setSeed`;
    `ThreadLocalRandom.current()`), checked against the JDK's numbers. Vanilla's `Mth` pure
    functions (`log2`, `ceillog2`, `wrapDegrees*`, `rotLerp*`, `approach*`, `atan2` with its table,
    `fastInvSqrt`, `hsvToRgb/Argb`, `getSeed`, `murmurHash3Mixer`, `frac`, `floorDiv`,
    `positiveCeilDiv`, `lerpInt/Discrete`, `inverseLerp`, `map/clampedMap/clampedLerp`, `smoothstep`,
    `triangleWave`, `catmullrom`, `length*`, `square/cube`...), checked against the 26.3 jar's own
    `Mth`; `clamp` of ints and longs as vanilla's `min(max(value, min), max)`.
    The game's client hosts (block entity drawing, client tick) answer `ingredient_items`: without it
    every item read from an update tag was unknown (`ItemStack.parseOptional` gave EMPTY), so the
    depot held nothing on the client and drew nothing (the harness host knew the items).
  - [x] 2026-11-04 **Create's mixer mixes** (harness `MIX`: creative motor at 64 RPM, cogwheel,
    mechanical mixer over a basin holding andesite and an iron nugget): it starts on tick 3 and by
    tick 70 the basin's output holds `create:andesite_alloy` and its inputs are gone; it stops on
    tick 90. The last two gaps: `Integer.intValue()` (and `Number.xValue()`) on an int a collection
    holds raw read nothing (fastutil's `IntIterator.next()` boxes it in Java; here it is the int
    itself), so the trie's walk looked every ingredient up as id 0; and fastutil's typed entry sets
    (`int2ObjectEntrySet`, `object2IntEntrySet`...) and entry accessors (`getIntKey`...) were
    unmapped.
  - [x] 2026-11-04 Block properties as 26.3 has them, for Create's common setup (it threw
    `NoSuchElementException`: its spout behaviours take the top of `LayeredCauldronBlock.LEVEL` and
    `FarmBlock.MOISTURE` with `getPossibleValues().stream().max(Integer::compareTo).orElseThrow()`).
    `Property.getPossibleValues()` lists the values as Java holds them, `Stream.max/min(Comparator)`
    keep the first of equal ones (`BinaryOperator.maxBy`), and `Integer.compareTo` reads an int a
    collection holds raw. The table of which `BlockStateProperties` field a vanilla class's constant
    holds is now read from the 26.3 jar, own fields and inherited: `LayeredCauldronBlock.LEVEL` is
    `LEVEL_CAULDRON` (1 to 3, was 0 to 15), `ComposterBlock.LEVEL` `LEVEL_COMPOSTER`, `HopperBlock.FACING`
    `FACING_HOPPER` (no `up`), `WallBlock.NORTH` `NORTH_WALL` (low/tall, was a boolean), the rails, banners,
    skulls and signs' `ROTATION`, `NoteBlock.INSTRUMENT` and 30 more; `LEVEL_HONEY` is named
    `honey_level`, and 26.3's new fields (`SPELEOTHEM_THICKNESS`, `SIDE_CHAIN_PART`, the shelf's
    `SLOT_n_OCCUPIED`...) are known.
  - [x] 2026-11-04 **Create's mixer mixes in game** (`scratchpad/mixgame.sh`: creative motor at 64
    RPM, cogwheel, mechanical mixer two above a basin; a dropper over a depot beside it). Andesite and
    an iron nugget thrown into the basin go in on the first second; the basin's output then holds
    `create:andesite_alloy`, its inputs are gone and the client draws the alloy in the basin. The
    dropper's iron ingot lands on the depot, which holds it and draws it. (The first run threw the
    items beside the basin: the script's `$X.5` is X - 0.5 for a negative X.)
  - [x] 2026-11-04 Mod block entities tick on the client too, as vanilla's `ClientLevel` ticks a block
    entity whose block gives a ticker there: `glue::client_tick` runs `ClientTickEvent.Pre`, then
    every block entity the client holds (one whose block is gone is let go), then `.Post`. Without
    it the mixer's head stayed down after mixing: the server sends `Running`/`Ticks` when it starts
    and when the recipe is done, and the client counts the head's way down and up itself. Harness
    `MIX` now replays the server's update tags on a client runtime: the head goes from 7/16 down to
    1 7/16 by tick 21, waits for the recipe, and is back at 7/16 on tick 88.
  - [x] 2026-11-04 Vanilla's block entity types (`BlockEntityType.LECTERN`, `SIGN`... and 26.3's
    `BlockEntityTypes`) are one object each, registered under their ids (26.3's 49, each its field's
    name lower-cased, read from `BlockEntityTypeIds`). They were null: Create's display-target
    registry (`SimpleRegistryImpl.register`, `requireNonNull`) threw on `create:lectern`, which ended
    the whole `RegisterEvent` for display targets. Piston classes' property constants
    (`PistonHeadBlock.TYPE`, in `block/piston/`) resolve too.
  - [x] 2026-11-04 In game the mixer's head now goes down into the basin, mixes and comes back up
    (`mixgame.sh` shots at ticks 20, 60 and 200: down, down, up), and the setup runs with no
    `NoSuchElementException`.
  - [x] 2026-11-04 A mod block entity is made when something asks for it, as vanilla's
    `LevelChunk.getBlockEntity` makes one for a block that has one (`glue::block_entity_at`, used by
    `Level.getBlockEntity` and by capability lookups). They were made only at a block's first tick, so
    Create's belt broke itself: the start segment's `BeltBlock.initBelt` walks the chain and destroys it
    when one segment's `getBlockEntity` is not a `BeltBlockEntity`, and the later segments had not
    ticked yet. A block entity made this way loads the block's saved data at its first tick.
    Harness `BELT` (a creative motor and a three-block belt): the belt keeps its three segments,
    each knows its index and its controller, and an ingot dropped on it is taken onto the belt.
  - [x] 2026-11-04 **Create's belt runs** (harness `BELT`): the motor turns it, and the ingot rides it
    at 16 RPM / 480 = 1/30 block a tick, to the end it moves toward (west: a belt along x turns the
    other way to its speed's sign), and goes off it as an item entity on tick 30. What held it: the
    belt never took the motor's rotation. Its start answered "no shaft" to the motor:
    `BeltBlock.hasShaftTowards` is `getBlockEntityOptional(..).map(BeltBlockEntity::hasPulley)
    .orElse(false)`, and a method reference used as a `Function` gives the method's raw boolean back
    (Java boxes it). `Optional` dropped a value that was not a reference, and `Boolean.booleanValue()`
    read nothing from a raw boolean. Both now take a raw primitive as its own value, as collections
    already hold numbers raw.
  - [x] 2026-11-04 Block tags: `BlockState.is(TagKey)` asks the game's loaded tags through a new host
    call, `Host::block_in_tag` (`native_block_name_in_tag` on `Game` and `GameLevel`, API 81), and
    vanilla's tag constants resolve: `vanilla_tags.rs` holds 26.3's 284 `BlockTags` and 219
    `ItemTags` fields with the tags they name, read from the jar (`BlockTags.STANDING_SIGNS` is
    `standing_signs`, `ItemTags.SIGNS` `signs`: 26.3 shares them through `BlockItemTags`).
    `Block.isFaceFull(shape, direction)` and `isShapeFullBlock` work on the shape's boxes as
    `getFaceShape` slices them. Create's belt asks all three before it lets an item off its end
    (`BlockHelper.hasBlockSolidSide`). `List.removeAll/retainAll` are mapped too.
  - [x] 2026-11-04 What Create's belt draws besides its model. The moving surface: catnip shifts a
    buffer's texture (`SuperByteBuffer.shiftUVScrolling`, `shiftUVtoSheet`, `shiftUV`) between two
    sprites it loads when the block atlas is stitched (`StitchedSprite`, on
    `TextureAtlasStitchedEvent`, which `fire_client_setup` now posts). Compat's sprites span 0..1, so
    the mod's own math leaves a fraction of the target sprite, and the game redraws the model with
    that move through a new host call, `Host::draw_model_shifted` (`SpriteShift` in
    `native_block_entity_draw.rs`). The items on it: the renderer culls each one with
    `Frustum.isVisible`, which was unmapped and said "not visible"; the game culls the block entity
    before it asks, so it now says yes. And `BlockPos$MutableBlockPos` is a position moved in place
    (`set`, `move`, `setWithOffset`, `clamp`, its constructors), whose inherited calls climb to
    `BlockPos` and `Vec3i` as a mod's own classes climb (`api_super`); before, `set` and `move` fell
    into the builder fallback and handed back the receiver unmoved. `closerThan` and
    `BlockPos.mutable()` are mapped too.
  - [x] 2026-11-04 A mod's own registries. Create makes twelve (`CreateBuiltInRegistries`: fan
    processing types, arm interaction points, display sources, contraption types...) with
    `new RegistryBuilder<>(KEY).sync(true).onBake(cb).create()`, and `create()` was unmapped, so each
    was null and what `AllFanProcessingTypes` registered to it went nowhere. A mod registry is now its
    name, as vanilla's are in compat; `NewRegistryEvent.create(builder)` and
    `DeferredRegister.makeRegistry(..)` make one too, `NewRegistryEvent` is posted before the
    `RegisterEvent`s, and the bake callbacks run once registration is done, on the full registry
    (`FanProcessingTypeRegistry.init` reads it with `Registry.forEach`, now mapped). Also
    `RegisterEvent.getRegistry()` (Create registers its advancement triggers when it is
    `BuiltInRegistries.TRIGGER_TYPES`) and `WritableRegistry.register(key, value, info)`.
  - [x] 2026-11-04 **Create's encased fan washes** (harness `FAN`: a creative motor, a fan facing
    east, a water source in front, a block of ice 3.5 blocks out): the current covers the water and
    on (`create:splashing` from offset 0 to 5), catches the ice, and on tick 160 it is packed ice
    (`create:splashing/ice`, 150 ticks). Three things held it. Fluid tags: the game loads them as it
    loads block and item tags (`Items::fluid_tags`, vanilla's and the content mods' data, the packs'),
    and `FluidState.is(TagKey)` asks through a new host call, `Host::fluid_in_tag`
    (`native_fluid_name_in_tag` on `Game` and `GameLevel`); Create finds its catalysts by the tag
    `create:fan_processing_catalysts/splashing`. Class initialisation: a static method reference
    (`FanProcessingTypeRegistry::init`, which Create hands to its registry's bake callback) now
    initialises the class that declares it, as `invokestatic` does; through the lambda table it ran
    on the uninitialised class, so the sorted processing types stayed empty (differential case
    `staticRefs` pins it against the JVM). And `Vec3i.get(Axis)`. The harness host now loads the
    mod's block and fluid tags too.
  - [x] 2026-11-04 In game the belt's segments are drawn turned as in the harness (a trace of every
    model draw's pose, `HORIZON_COMPAT_TRACE`, shows the same matrices), and its moving surface no
    longer shows other sprites at the seams: the GUI atlas cut every non-square block or item texture
    without an `.mcmeta` to its top square as if it were an animation strip, so Create's 16x32
    `belt_scroll` was half there and the scroll read past it. Vanilla keeps such a texture whole
    (`SpriteLoader.loadSprite`), and vanilla 26.3 has none.
  - [x] 2026-11-04 **Mod networking** (`network.rs`): a mod's payloads go over the game's mod message
    channel (`native_net_send_to_player`, `native_net_send_to_server`). `RegisterPayloadHandlersEvent`
    is posted after common setup, and each `PayloadRegistrar.playToClient` (`playToServer`,
    `playBidirectional`, `common*`, `configuration*`) keeps its type's codec and handler under the
    type's id. `PacketDistributor.sendToPlayer`, `sendToAllPlayers`, `sendToPlayersNear` and
    `sendToServer` encode the payload with its `StreamCodec` and hand the bytes to the host
    (`Host::send_payload`, a `PayloadTarget`); the other side's runtime decodes them with the same
    codec and runs the handler with an `IPayloadContext` (`player`, `enqueueWork` run at once,
    `reply`, `flow`). Codecs are compat objects: `ByteBufCodecs`' constants and builders
    (`collection`, `map`, `optional`, `idMapper`, `registry`, `holder`), `StreamCodec.composite`,
    `unit`, `of`, `ofMember`, `.map`, `.apply`, `.dispatch`, and the stream codecs of item and fluid
    stacks, positions, UUIDs, components and NBT. Buffers write as Minecraft's (`FriendlyByteBuf`:
    varints, a `BlockPos` packed in a long, network NBT); a short buffer throws, as netty's does.
    `ServerTickEvent.Pre`/`.Post` are posted from the game's `server_tick` hook, and the bundle wires
    `net_from_server` (the window) and `net_from_client` (the server) to the runtime. Harness `NET`:
    Create registers 117 payload types; its server tick sends `create:server_speed` every 21 ticks,
    and the client's runtime takes it.
  - [x] 2026-11-04 The client has a local player: `Minecraft.getInstance().player`, a `LocalPlayer` in
    the client's level that maps as a player does. Without it Create's whole client tick
    (`ClientEvents.onTick`, which first checks `isGameActive`) returned early,
    `ServerSpeedProvider.clientTick` among it. Harness `NET`, the client ticking once a server tick:
    the speed the client moves belt items at is 0 until the second speed packet (Create's first only
    starts the count), 0.88 ten ticks later and 1.000 from tick 80, as in Java. A client tick of
    Create's costs 0.24 ms.
  - [x] 2026-11-04 **In game the ingot rides the belt on the client too** (`beltgame.sh`, shots at
    ticks 10, 13 and 25: it moves toward the belt's start each frame, the other lane's into the
    depot). Create moves belt items on the client by `speed * ServerSpeedProvider.get()`, which
    starts at 0 and rises as the server's speed packets arrive: the integrated server's tick sends
    `create:server_speed` to the window's player over the mod message channel, and the window's
    runtime takes it. Before, the client showed the ingot where the last update tag put it.
  - [x] 2026-11-04 **Create's water wheel turns in flowing water** (harness `WHEEL`: a small wheel along
    x, water above it flowing south and below it flowing north, a shaft on its east side): flow score
    2, the wheel and the shaft at 8 RPM from the first tick; in still water it stays still.
    `FluidState.getFlow(level, pos)` asks the game through a new host call, `Host::fluid_flow`
    (`native_fluid_flow_at` on `Game`: vanilla's `FlowingFluid.getFlow` over the current level,
    `FluidResources::flow_in`, the flow the game already pushes entities with).
  - [x] 2026-11-04 Food: `Player.getFoodData()` (`getFoodLevel`, `needsFood`, `addExhaustion`) and
    `Player.causeFoodExhaustion` reach the acting player through `Host::player_food` and
    `Host::exhaust_player` (`native_food_level`, `native_exhaust` on `Game`; none in creative, as
    vanilla's abilities keep it off). **Create's hand crank turns** (harness `CRANK`, a crank facing
    up on a shaft): a right click (`useItemOn`, empty hand) drives it and the shaft at 32 RPM for 10
    ticks, then they stop; sneaking, at -32. It tires the player by 0.32 (32 RPM times the config's
    `crankHungerMultiplier`, 0.01), and read a null `FoodData` before.
  - [x] 2026-11-04 **In game the water wheel turns** (`wheelgame.sh`: a glass channel with a source at
    its north end, the wheel under it): `FlowScore: 1`, `Speed: 8.0f`, `AddedCapacity: 32.0f`, the
    shaft beside it `Speed: 8.0f` with `Source` the wheel; dry, 0. It drew nothing: its wheel is an
    OBJ model, retextured in code.
  - [x] 2026-11-04 Models with NeoForge's OBJ loader (`"loader": "neoforge:obj"`, 43 of Create's: the
    water wheels, crushing wheels, blaze burner...) load: `horizon_assets::obj` reads the mesh and its
    `.mtl` libraries as NeoForge's `ObjLoader` does (libraries beside the mesh or by namespace,
    `mtl_override`, triangles repeating their last corner, `flip_v`, `emissive_ambient` light from
    `Ka`, tint indices, faces flat on a side culled there), the model resolves its texture slots
    through its parents and bakes with the block state's rotation like elements. The game's model
    library reads the mesh beside each such model (both its loads); translated mods carry `.obj` and
    `.mtl` (the jar has 39 meshes).
  - [x] 2026-11-04 NeoForge's composite models (`"loader": "neoforge:composite"`, 7 of Create's: the
    display link, stock links, nixie tube): each child resolves through its own textures and
    parents (`ResolvedCuboidModel::children`) and they bake together under the block state's
    rotation; `visibility` hides a child. The game's model library loads the children's parents
    (`CuboidModel::parents`) and their OBJ meshes.
  - [x] 2026-11-04 **Baked quads are real** (compat `quads.rs`): `BakedModel.getQuads` on a model the
    game has (a block state's, a partial model) gives the game's faces as `BakedQuad`s, their
    vertices in `DefaultVertexFormat.BLOCK` (32 bytes: position, colour, uv in the atlas, light,
    NeoForge's filled normal), their sprite the atlas's one object per sprite (mods key maps by
    them). A mod reads, copies and changes them and builds `SimpleBakedModel`s (and
    NeoForge's `SimpleBakedModel.Builder`) of them; catnip's `createForBlock` reads such a model's
    faces once (`getQuads` per side, as `ModelBlockRenderer` asks) and the game draws them
    (`Host::model_quads`, `Model::Quads`, `native_draw_faces`). Create's water wheel: its template's
    faces on oak planks and logs copied onto the wheel's wood (`BakedModelHelper.swapSprites`).
  - [x] 2026-11-04 **Sprites have places in an atlas** (compat `render.rs`, `CELLS`): the wheel drew
    oak (pixels measured at (153,125,75), oak planks average (162,130,78), spruce (114,84,48)).
    Create's `swapSprites` keeps the quad's sprite (`BakedQuadHelper.clone`) and moves only its UVs
    into the new sprite's part of the atlas (`getU(getUnInterpolatedU(old, u))`), and every sprite
    spanned 0 to 1, so the move did nothing. Now each sprite gets a cell 1/256 of the atlas wide
    (`getU0`..`getV1`, `getU`, `getUOffset` in it), quads the game hands over are encoded into
    their sprite's cell, and a quad read back shows the sprite whose cell its UVs are in
    (`render::locate`: its own sprite while they stay in its cell), with UVs 0 to 1 across that
    sprite again for the game. catnip's scroll offsets come in atlas units and are divided by a
    cell. The draw trace lists the sprites a mod-built model shows.
  - [x] 2026-11-04 **Codecs write** (compat `codecs.rs`, `encode`): `encodeStart(NbtOps, value)`
    and `encode(value, ops, prefix)` walk a codec into NBT as DFU does: a record's fields from the
    mod's getters (`forGetter`), `xmap`'s way back (`from`, kept with the codec now), optional
    fields left out at their default, dispatch's type key then the type's codec, lists, maps,
    either, pairs, enums by serialized name, registries by id, and Minecraft's own (positions as
    int arrays, stacks `{id, count}`, fluid stacks, block states `{Name, Properties}`, compounds).
    In game the display link and nixie tube threw `NoSuchElementException` in every save
    (`FactoryPanelSupportBehaviour.write`: `CatnipCodecUtils.encode(...).orElseThrow()`); Create
    writes 24 things this way (chain conveyors, packagers, clipboards, contraptions' storage).
    Reading takes any tag now (a list, a number), not only compounds and strings.
  - [x] 2026-11-04 `/data merge block` reaches a mod's block entity: each tick its live object
    saved over the game's data, so a change the game made was lost (the water wheel stayed spruce
    after `{Material:{Name:"minecraft:birch_planks"}}`). The glue keeps the hash of what it last
    wrote (`nbt::data_hash`, without `x`/`y`/`z`/`id`) and loads the game's data when it differs,
    as vanilla's `BlockDataAccessor.setData` does (`hook.rs`: count 10 merged in, 11 a tick later),
    and players are sent it (`sendBlockUpdated`: the client drew the old wood until then).
  - [x] 2026-11-04 A mod class extending a JDK collection keeps its own fields (compat-core
    `Rt::side_fields`, kept and freed with the object by the collector): its constructor turns the
    object into the list or table, which wiped them, and fields set after `super()` went nowhere.
    Create's `UniqueLinkedList` (a `LinkedList` with a `HashSet` of what it holds) is the frontier of
    every contraption's block search: `contained.add` on nothing answered false, so nothing was
    queued and a bearing assembled no blocks (harness `BEARING`).
  - [x] 2026-11-04 In game (`objgame.sh`) Create's OBJ and composite models draw: crushing wheel,
    flywheel, valve handle, display link, nixie tube, water wheel and large water wheel (spruce, as
    their default material: the draw trace lists `spruce_planks x1024, waterwheel_metal x224,
    spruce_log x16`), track, chain conveyor. The blaze burner draws its cage but no blaze.
  - [x] 2026-11-04 `LevelTickEvent.Pre` and `.Post` around each server level's tick: the translator
    hooks the game's `native_level_tick_pre`/`_post` (the server's game holding that level, with
    the host block entity ticks get, now one generator: `game_host`), and `glue::level_tick` posts
    the event with the level. Create's `onServerWorldTick` ticks contraptions, minecart couplings,
    linked controllers, train controls, railways and logistics there.
  - [x] 2026-11-04 A state's destroy time, push reaction and replaceability (`getDestroySpeed`,
    `getPistonPushReaction`, `canBeReplaced`): a vanilla block's from the game (`Host::block_facts`,
    `native_state_facts` over horizon-world's `StateFacts`), a mod block's from its `Properties`
    (`strength`, `pushReaction`, `replaceable`, or the block it copied); a mod block overriding
    NeoForge's `getPistonPushReaction(state)` answers itself. Create's `BlockMovementChecks` read
    all three before a block may move: every block answered "unbreakable" and nothing moved.
  - [x] 2026-11-04 NeoForge's tag constants (`Tags.Blocks`, `.Items`, `.Fluids`, `.EntityTypes`: 382,
    `neo_tags.rs`, read from NeoForge's jar) name the tags its jar gives them (`c:relocation_not_supported`).
  - [x] 2026-11-04 Components as JSON (`Component.Serializer.toJson`/`fromJson`, text, translation
    and its arguments) and blocks' names (`getName`, `getDescriptionId`).
  - [x] 2026-11-04 Mod entities (`mod_entities.rs`): vanilla's `Entity` modelled in the runtime (id,
    UUID, position and its old values, rotation, motion, bounding box from the type's size,
    `SynchedEntityData` with ids per class as vanilla counts them, removal), `EntityType.Builder`
    and `EntityType.create`. A level's mod entities tick with it (`LevelTickEvent.Pre`, then each
    `tick()`), `getEntitiesOfClass` finds them, and a server sends each to the players in its
    tracking range on channel `horizon_compat:entity` (spawn with NeoForge's
    `IEntityWithComplexSpawn` data, moves batched, dirty synched data, gone; a message over 29 KB
    goes in parts). Unit tests; harness `BEARING`: the bearing's `ControlledContraptionEntity` is
    made, joins the level and turns 4.8° a tick.
  - [x] 2026-11-04 Vanilla's records (`records.rs`): 1013 records and their components
    (`vanilla_records.rs`, written by `tools/vanilla_records.py` from NeoForge's merged jar); a mod
    builds one with its canonical constructor and reads it back by accessor, `equals`, `hashCode`.
    Create keeps a contraption's blocks as `StructureTemplate.StructureBlockInfo`, which came back
    empty, so a disassembled bearing put nothing back. `ImmutableMap.copyOf` copies (the builder
    rule had answered with an empty object of the return type, uncounted). `Rotation`/`Mirror` and
    `BlockState.rotate`/`mirror`: the mod block's own, else its vanilla base's rule on `facing`,
    `axis`, `rotation`, the side arms and rail shapes (`Block` itself leaves the state). The level's
    `getChunkAt`, `markAndNotifyBlock`, `removeBlockEntity`, `dimension()`, `Level.OVERWORLD`...
  - [x] 2026-11-04 **A bearing assembles, turns, disassembles and assembles again** (harness
    `BEARING`, server side): the click at tick 5 puts the plank back at (0, 65, 0), the one at tick
    10 lifts it into a new contraption. The plank had landed at (0, 0, 0): `BlockPos.containing`
    wasn't mapped and the builder rule (an unmapped method returning its own class hands back the
    receiver, or a new object for a static) answered with a zero position, uncounted. That rule now
    runs only once every class up the receiver's has missed, and counts as a miss: the reports show
    what it stood in for (Guava's `Sets.union`, `Comparator.comparing`, advancements' builders...).
  - [x] 2026-11-04 **The client gets the contraption** (harness `BEARING`, client step: the spawn
    message read by a client runtime on its own thread): a `ControlledContraptionEntity` with its
    `BearingContraption` of 1 block at the server's angle, alive after a client tick. It needed
    `Class.getEnumConstants` for vanilla enums (`NBTHelper.readEnum(.., Direction.Axis.class)`),
    `Enum.name()`/`ordinal()` of the natives (directions, axes, planes; `toString` as 26.3's
    overrides), `WeakReference`, `VarInt`/`VarLong`, intrusive holders (`createIntrusiveHolder`: the
    contraption type's key was `minecraft:`), `Registry.registerForHolder` giving a holder, and
    `HashMapPalette` with its `CrudeIncrementalIntIdentityHashBiMap` (`palette.rs`: the palette of
    states was empty). Also `Comparator`'s combinators (`comparing`, `thenComparing`, `reversed`,
    `nullsFirst`...), `ThreadLocal`, `Triple`, `RandomSource.create`, `NbtAccounter`.
  - [x] 2026-11-04 Drawing mod entities (step C, built, not yet seen in game): the game's new hook
    `native_level_submit` (once a frame, camera and partial tick) runs `glue::render_mod_entities`,
    which makes each entity type's renderer once (`EntityRendererProvider.Context`) and calls its
    `shouldRender` and `render` with a pose stack at the entity's lerped place. Create draws a
    contraption by tesselating its blocks into catnip's `ShadedBlockSbbBuilder`: each block kept as
    a part (its model's quads and the pose it was drawn at), drawn in one `SuperByteBuffer`.
  - [x] 2026-11-04 What the contraption renderer asks (harness `BEARING`, client frames): its client
    side is made lazily behind `AtomicReference.compareAndExchangeRelease` (mapped, with the weak
    compare-and-sets), `state.getRenderShape()` answers as 1.21.1 does (a mod block's own, else its
    vanilla class's: `vanilla_shapes.rs`, written by `tools/vanilla_render_shapes.py` from the merged
    jar's `Blocks` and block classes: 140 blocks and 34 classes aren't models, chests and beds
    animated, signs, banners, heads and air invisible), `java.util.BitSet` (which block entities to
    draw; hash checked against the JDK) and `Matrix4f.setTranslation`.
  - [x] 2026-11-04 **The client draws the contraption** (harness `BEARING`, client frames 0 and 1):
    Create's renderer, translated again with the `ShadedBlockSbbBuilder` intrinsics, tesselates the
    plank and the game is asked for `minecraft:oak_planks` under the turned pose (the corner at
    -0.54, -1.62, -3.46 from the camera: 4.8° about the block's centre). Lit by the entity's own
    light for now (catnip's `useLevelLight` per vertex isn't read). Also: a client level's
    `getEntitiesOfClass` (contraption collisions ask it each frame), fastutil's
    `toIntArray`/`toLongArray`..., and fastutil's `add(int)` on a primitive list, which was taken
    for `add(index, element)` and kept a null (any `IntArrayList` a mod filled was empty values).
  - [x] 2026-11-04 **In game: a mechanical bearing turns its contraption** (`scratchpad/bearinggame.sh`,
    build of 785a983): a creative motor under a bearing under an oak stair; the server assembles the
    stair (the block above reads air, the bearing `Running: 1b`, angle 62.4° at tick 10), the client
    gets the entity and Create's `ContraptionEntityRenderer` draws the stair turning on the bearing's
    rotating top (screenshots t10/t20/t30 show the step on the left, side-on, then on the right).
    Lit by the entity's light. No uncaught throws in the log.
  - [x] 2026-11-04 Sticky blocks (NeoForge's `isSlimeBlock`, `isStickyBlock`, `canStickTo`: a mod
    block's override, else slime and honey stick to anything but each other, as `IBlockExtension`'s
    bytecode says). Harness `BEARING` with `BEARING_SLIME=1` (a slime block on the bearing, planks
    on its four sides): a contraption of 5 blocks, put back and lifted again by the clicks, and the
    client draws the 5 under the turned pose.
  - [x] 2026-11-04 What the counted builder rule showed at load (harness `RECIPES`, the unmapped
    list): `new BlockPos(vec3i)` (42656 calls left at the origin), `Vec3.ZERO` (2677), `EnumSet`
    (`noneOf`, `of`, `copyOf`, `range`, `complementOf`, `allOf`: a set in its enum's order that knows
    its type; 930 `Set.contains` on a stand-in object followed), `Vec3`'s whole 1.21.1 API
    (`reverse`, `cross`, `xRot`/`yRot`/`zRot`, `lerp`, `align`, `relative`, `closerThan`,
    `directionFromRotation`, `toString`...) and `Mth.sin`/`cos` of a float as 1.21.1 indexes its
    table (float arithmetic). Checked against a JDK running the merged jar's own `Vec3`, `Mth` and
    `EnumSet` (`scratchpad/jdkcheck/vec`, with stand-ins for the two authlib classes whose shape
    changed after 1.21.1).
  - [x] 2026-11-04 Every `BlockBehaviour.Properties` call (1.21.1 has 41) is kept on the object as a
    `$` field instead of falling to the counted builder rule; the predicates (`isRedstoneConductor`,
    `isSuffocating`, `isViewBlocking`...) and flags the game doesn't read yet are listed in
    `blocks.rs`. `SoundSource` constants. Found on the way: a level's `playSound`, `addParticle` and
    `levelEvent` are no-ops for mods (no host hook yet): Create's sounds and particles are silent.
  - [x] 2026-11-04 In game, the slime cross (sticky blocks above) assembles and turns on the bearing
    (`scratchpad/bearinggif.sh` with `SCENE=slime`, 40 frames).
  - [x] 2026-11-04 Mod sounds (`horizon-compat/src/sounds.rs`, the host's `play_sound` with a
    `host::Sound`). The `SoundEvent`s a mod makes (`createVariableRangeEvent`,
    `createFixedRangeEvent`, `getLocation`, `getRange`); vanilla's `SoundEvents` fields as the ids
    they register (a field's name isn't its id: `WOODEN_TRAPDOOR_OPEN` is
    `block.wooden_trapdoor.open`), 1603 of them, 22 as holders, in `vanilla_sounds.rs`, which
    `tools/vanilla_sound_events.py` writes from the merged jar's `SoundEvents` initialiser; a level's
    `playSound`, `playSeededSound` and `playLocalSound` and `Entity.playSound`, every 1.21.1
    overload. As in vanilla, a server's level plays `playSound` for the players near it and a
    client's plays `playLocalSound` and its own player's `playSound`. The server's sounds go to the
    game through `native_play_sound` (a block's hook) or the new `native_level_sound` (a block
    entity's tick, a click, a payload: API fragment `native_level_sound`), queued as native mods'
    sounds are and sent by name, so clients play Create's own events from its `sounds.json` in the
    content root; a client's through `native_play_local_sound`. One difference from vanilla: the
    player named in `playSound` hears it too, because only the server runs a mod's interactions here.
    Harness `BEARING`: the click plays `create:contraption_assemble` at the bearing (volume 0.5, pitch
    0.5) and `contraption_assemble_compounded_1` (0.045, 0.74) under blocks, as Create's
    `playExisting` sets them; the next click plays `contraption_disassemble` at the bearing and at the
    contraption. A sound at the player a hook acts for plays nothing yet (that player has no
    position in hooks).
  - [x] 2026-11-04 Guava's `Sets.union`, `intersection`, `difference` and `symmetricDifference` in
    Guava 33's iteration order, checked with the JDK (Create's mounted storages write a union of
    their item and fluid positions; its iterator was a stand-in, so nothing was written); `Level`'s
    constructor for a mod's own level (Create's `ContraptionWorld` keeps its side) and
    `dimensionTypeRegistration`. The bearing's click leaves nothing unmapped now.
  - [x] 2026-11-04 Mod particles (`horizon-compat/src/particles.rs`, the host's `particles` with a
    `host::Particles`). Vanilla's `ParticleTypes` fields (the name lowercased is the id but for
    the trial spawner's two, checked against the 1.21.1 jar's 109 fields), `BlockParticleOption`,
    `ItemParticleOption`, `DustParticleOptions` (and `REDSTONE`), a mod's own types by the id they
    are registered under; a level's `addParticle` and `addAlwaysVisibleParticle` (with or without
    `force`) and `ServerLevel.sendParticles` (to all near, or to one player). As in vanilla, a
    client's level draws `addParticle` (the game's `native_add_particle`, the options from
    `native_particle_options`: a block's particle sprite and tint, an item's model layer sprites)
    and a server's sends `sendParticles` (`native_level_particles`, as `native_send_particles` does:
    item particles don't go over the network yet); `addParticle` on a server is nothing. API
    fragment `native_level_particles`. What Create draws, measured over its jar: `ITEM` and
    `BLOCK` options most (15 and 14 call sites), dust, end rods, smoke, flames; 68 classes add
    particles on the client, 6 send them from the server. Harness `PRESS` (the press's syncs replayed
    on a client): the sync made as it presses carries the ingot, and the client makes 15
    `minecraft:item` particles of `minecraft:iron_ingot` at the depot (0.5, 64.75, 0.5), each moving
    up 0.125 and out up to 0.125 (the press's `getParticleAmount` is 15). Create's own particle
    classes (air flow, steam jets, rotation indicators) aren't drawn yet: their options carry no
    registered type.
  - [x] 2026-11-04 In game (`scratchpad/fxgame.sh`, `foodgame.sh`: offscreen, on a platform in the
    sky): the press over a depot presses the player's iron ingot into a sheet, and the window
    hears Create's own sounds: `create:depot_slide`, `create:mechanical_press_activation` and its
    `_compounded_1`, the bearing's `create:contraption_assemble` (and `_compounded_1`). The
    press's item particles first weren't made: a client loaded a mod block entity's update tags
    only when drawing it, from what it keeps of all of them, and the press sends the pressed items
    in one tag and clears them in the next, so two packets between frames left none. Now the
    client has the block entity load each packet's own tag as it arrives (hook
    `native_block_entity_data`, as vanilla's `handleBlockEntityData`), in view or not; drawing
    loads only what no packet did. Measured after: the window makes 15 `item` particles as the
    press comes down (`status.particles`), as the harness said.
  - [x] 2026-11-04 Food (`horizon-compat/src/food.rs`, tests `food::tests`). `FoodProperties.Builder`
    (`nutrition`, `saturationModifier`, `alwaysEdible`, `fast`, `effect` with an instance or
    NeoForge's supplier, `usingConvertsTo`, `build` as vanilla's: saturation is nutrition ×
    modifier × 2 in floats), `Item.Properties.food`, `MobEffectInstance` (its constructors,
    copies, getters), `MobEffects` (each field's id from the 1.21.1 jar's static initialiser,
    `tools/vanilla_mob_effects.py`: 39, `DIG_SPEED` is `haste`) and `UseAnim`'s constants. An
    edible item declares 26.3's three components as vanilla's own items have them: `food`,
    `consumable` (seconds, animation, sound, particles, `apply_effects`: the sure effects
    together, each chance one alone, as 1.21.1 rolls them) and `use_remainder`. Its class's
    overrides of `getUseDuration`, `getUseAnimation` (1.21.1's `SPEAR` is 26.3's `trident`),
    `getEatingSound`/`getDrinkingSound` are asked once. Checked against items.json: the builder
    makes mushroom stew (7.2000003, a bowl back), the golden apple and raw chicken (hunger at
    30 %) as vanilla declares them. Create's foods (harness `DECLARE`): bar of chocolate
    6/3.6000001, sweet roll 6/9.6, chocolate glazed berries 7/11.2, honeyed apple 8/12.8,
    builder's tea 1/1.2 always edible, drunk in 2.1 s (its 42 ticks), `drink`, the generic drink
    sound, no crumbs, haste for 3 minutes without particles or icon. In game: a hungry survival
    player (food 6) eats a bar of chocolate (food 12, saturation 3.6000001), then drinks
    builder's tea (food 13, saturation 4.8, haste with 3571 ticks left). Gap: the tea's glass
    bottle comes from its `finishUsingItem` override, which nothing runs yet (the game's
    `living_entity_use_item_finish` hook can).
  - [x] 2026-11-04 A mod item's own `use` (hook `native_use_item`, `glue::use_item`): a right
    click in the air (or on a block that let it through) runs the item class's `Item.use`; a
    result that consumes the action takes the click, and the hand holds the stack it answered
    (only the difference of the same item moves, so its data stays). `InteractionResultHolder`,
    `Player.getAbilities` (creative builds at once), cooldowns (none kept yet), and the acting
    player's place: the server hooks for a player's click share one host
    (`bundle::player_host`), which now says where that player stands and looks
    (`Host::player_place`, from `native_acting_player`), so `getX`, `position`, `blockPosition`,
    `getEyePosition`, `getLookAngle` (vanilla's `calculateViewVector` with `Mth`'s table) answer.
    Harness `USE=1` over Create's 18 items with a `use` (survival, 3 in hand): the experience
    nugget uses the whole stack (orbs not made yet: `ExperienceOrb`, `addFreshEntity`), filters
    and the schedule take the click (no menu yet: `Player.openMenu`), the wand of symmetry and the
    clipboard take it with nothing unmapped; goggles want `Item.swapWithEquipmentSlot`, the
    sandpaper `getPlayerPOVHitResult`, the potato cannon and worldshaper `startUsingItem` (use
    over time). In-game check pending.
  - [x] 2026-11-04 Items used over time (`horizon-compat/src/item_use.rs`, tests `item_use::tests`).
    1.21.1's way: the item's `use` calls `startUsingItem`, the use counts `getUseDuration` down
    with `onUseTick` each tick, and ends in `finishUsingItem` or, let go, `releaseUsing`. The game
    counts every use itself (`game/item_use.rs`); the bundle now answers its hooks with the mod's
    parts: `native_use_duration` (both sides) and `native_use_animation` (the client's pose) from
    the class's overrides, `native_living_entity_use_item_start` lets a use go on on the server
    only when the click's `use` called `startUsingItem` (a food's eating stays the game's), and
    `native_on_use_tick`, `native_living_entity_use_item_finish` and `native_release_using` run
    the overrides on the server. `glue::use_item` no longer takes a click whose `use` started a
    use, so the game starts it. Vanilla's own part of `finishUsingItem` (eating) is the game's: an
    override calling `super` gets the stack the game left. A hook's player keeps one stack object
    for its hand (`getItemInHand` answers the same one, as vanilla's), `instanceof Player` and
    `LivingEntity` know the hook's `ServerPlayer`. Harness `HOLD=1`: builder's tea (42 ticks,
    `drink`) leaves a glass bottle in the inventory, and the last one leaves it in the hand; a
    cardboard package (72000, `bow`) starts on its `use` and leaves the hand on release.
    Gaps: a client predicts a mod item's use whenever its duration is above 0, so where the
    server refuses it (a sandpaper with nothing to polish) the client holds the pose until let
    go; the thrown package is a mod `LivingEntity` (`PackageEntity`), not made yet, so it is lost.
  - [x] 2026-11-04 Messages, experience orbs, what the player looks at. `Player.displayClientMessage`
    and `sendSystemMessage` reach the acting player (`Host::player_message`: vanilla's JSON text,
    chat or over the hotbar, through `native_tell`/`native_action_bar`); components keep styles
    (`withStyle` of `ChatFormatting`s) and siblings (`append`), `plainCopy`, `copy`, `empty`.
    `new ExperienceOrb(level, x, y, z, value)` with `setDeltaMovement`, added by
    `Level.addFreshEntity`, and `ExperienceOrb.award` (vanilla's orb sizes) spawn orbs
    (`Host::spawn_experience_orb`, new game helper `native_spawn_experience_orb`).
    `Item.getPlayerPOVHitResult` asks the block the player looks at (`Host::player_pov_hit`, new
    helper `native_player_pov_hit`, the game's own ray within block reach), else a miss where the
    reach ends; `ClipContext`'s modes. Harness `USE=1`: three experience nuggets throw orbs worth
    9 points; the worldshaper with nothing set says `create.terrainzapper.shiftRightClickToSet`
    in red over the hotbar; the sandpaper asks what the player looks at (a miss: nothing to
    polish).
  - [x] 2026-11-04 In game (offscreen, seed 12345, survival on a platform, `usegame.sh`), the two
    entries above: a right click with 4 experience nuggets uses all 4 and the player ends with 12
    points (level 1), orbs flying; hungry, the last builder's tea drunk (held 3 s) leaves a glass
    bottle in the hand, food up and haste on; one of two drunk leaves one tea in the hand and the
    bottle in the inventory (`clear` finds 1); the worldshaper says "Shift-Right-Click to Select a
    Shape" in red over the hotbar. No panic, nothing uncaught.
  - [x] 2026-11-04 Item data on the game's stacks (`horizon-compat/src/hand.rs`, `components.rs`;
    tests `hand::tests`, `components::tests`). A mod's persistent component types (built with a
    codec) are registered with the game as native components (`native_component_types`, answered
    by the probe runtime: `components::persistent_types`, names a resource location allows, never
    `minecraft:`), so a game stack carries their values as NBT, each written and read by its
    type's own codec. The hand: a hook acting for a player (`use`, `useOn`, a block's
    `useItemOn`, a use over time's tick, finish and release) has one main-hand stack for the whole
    hook (`hand::with_hand`), loaded from the game with its components the first time the mod
    reaches it (`getItemInHand`, a use context's `getItemInHand`), and what the mod did to it
    reaches the game's stack when the hook ends: its count and components changed in place
    (vanilla's own data on it stays), or the stack the mod put there instead (`setItemInHand`, a
    `use` answering another stack, `finishUsingItem`'s answer). The runtime's `Nbt` moved into the
    host's module; new Host methods `player_held_components`, `update_player_held`,
    `set_player_held` (`MemoryHost` keeps them), new game helpers `native_held_stack`,
    `native_set_held_stack`, `native_registered_components`. A saved stack (`ItemStack.save`,
    `parseOptional`, `ItemStack.CODEC`: what a block entity keeps, a recipe's result) carries them
    as vanilla's `components` map. Harness `COMPONENTS=1`: Create registers 50 persistent types
    (`belt_first_shaft`, `clipboard_pages`, `package_address`, `shaper_*`, `train_schedule`...);
    its belt connector clicked on a shaft keeps `create:belt_first_shaft` = [0, 64, 0] on the
    stack in hand, clicked on a second shaft 4 blocks along lays a belt (start, three middles,
    end, facing south) and the hand holds 4 of 5, the component gone. `HOLD=1` and `USE=1` as
    before. Gaps: payloads a client sends (`ConfigureWorldshaperPacket`...) run without the
    sender's hand; vanilla's own components (damage, a name) aren't read from the game's stack;
    the off hand is empty to mods; stacks given to the player lose their components. In game
    (build of 129aba2, `beltgame.sh`): the connector's first click on a shaft keeps
    `create:belt_first_shaft` = `[I; 98, 184, -23]` on the game's stack (`data get entity @s
    Inventory`); the second click stopped the server thread (the runtime entered twice, see
    "Interaction events" below).
  - [x] 2026-11-04 Both hands, and wear by the game's rules (`horizon-compat/src/hand.rs`,
    `components.rs`; tests `hand::tests`). A hook's player holds one stack object per hand: the
    off hand too now (`getOffhandItem`, `getItemInHand(OFF_HAND)`, `setItemInHand` by its hand),
    each loaded from the game the first time the mod reaches it, and each flushed when the hook
    ends. A stack carries its damage (`minecraft:damage`) and the damage it breaks at (the game's
    `max_damage` for a stack from a hand, else the mod item's `durability`): `getDamageValue`,
    `setDamageValue`, `isDamaged`, `getMaxDamage`, `isDamageableItem`, the `DAMAGE` and
    `MAX_DAMAGE` component types, a copy, `split` and `isSameItemSameComponents` keep or compare
    it, and a saved stack writes it in `components`. `hurtAndBreak` on a stack in hand is the
    game's own wear (`Host::hurt_player_hand` → new helper `native_hurt_hand_stack`: none in
    creative, Unbreaking, breaking with its sound, told for the hand that held it), and the mod's
    stack then holds what the game left; on any other stack it wears where it is.
    `hurtWithoutBreaking` stops one short of breaking. Host methods `player_hand`,
    `update_player_hand`, `set_player_hand`, `hurt_player_hand` take a hand (`HandStack`: item,
    count, damage, max damage, components) and replace last step's held ones; game helpers
    `native_hand_stack`, `native_set_hand_stack`, `native_hurt_hand_stack`,
    `native_item_max_damage` replace `native_held_stack`/`native_set_held_stack` (whose
    `remote` check didn't compile: `remote` is an `Option`). The game's own `damage_held` now
    tells a broken off-hand item as the off hand's (entity event 48, vanilla's
    `onEquippedItemBroken` slot), not the main hand's.
    Crafting remainders: `Item.Properties.craftRemainder` is kept, and `hasCraftingRemainingItem`/
    `getCraftingRemainingItem` (stack and item forms) answer it, else vanilla's five (buckets, honey,
    dragon's breath); `isStackable` is `getMaxStackSize() > 1` and not damaged. Harness
    `SANDPAPER=1` (new): a survival player holds Create's sandpaper (durability 8) with 3 rose
    quartz in the off hand; the click's `use` takes one from the off hand (2 left) and keeps it on
    the paper as `create:sand_paper_polishing` = `{item: {id: create:rose_quartz, count: 1}}`,
    starting a 32-tick use; its `finishUsingItem` gives polished rose quartz, removes the
    component and wears the paper to 1; a paper at 7 breaks (the hand empties). No unmapped calls.
    `COMPONENTS`, `HOLD`, `USE` print the same as before. Gaps: a mod item's hooks run from the
    main hand only (an item used from the off hand is the game's); a mod's `Consumer` passed to
    `hurtAndBreak` isn't called on a break. In game (build of 129aba2, `sandgame.sh`): a survival
    player with the sandpaper and rose quartz in the off hand polishes one (2 left in the off hand),
    the paper wears to 1, and a worn paper breaks.
  - [x] 2026-11-04 A mod item's hooks from either hand. The game asks its hooks once per hand, as
    vanilla does (`ServerPlayerGameMode.useItemOn` and `useItem` per `InteractionHand`); the bundle
    now passes the acting hand (`game.hand`) instead of skipping the off hand: a block's
    `useItemOn` gets that hand's stack and constant (`useWithoutItem` stays the main hand's),
    `useOn`'s context (`Placement.hand`: `getHand`, `getItemInHand`) and `use` act for it, a use
    over time's `onUseTick`, `finishUsingItem` and `releaseUsing` get the hand that used it
    (`getUsedItemHand`), and what they do reaches that hand. `glue::use_block`, `glue::use_item`,
    `item_use::{finish_using_item, release_using, use_tick}` take the hand; `hand::constant`.
    Harness `SANDPAPER=1` round 2: the paper in the off hand, 3 rose quartz in the main hand:
    `use` takes one from the main hand (2 left), the component goes on the off-hand paper, and
    the finish gives polished rose quartz and wears the off-hand paper to 1. Rounds 0 and 1,
    `COMPONENTS`, `HOLD`, `USE`, and the `regress.sh` set print as before (object ids and hash
    order aside; FAN, PRESS and MIX reach one unmapped call fewer: `hasCraftingRemainingItem`).
  - [x] 2026-11-04 Interaction events, and hooks that don't enter the runtime twice
    (`horizon-compat/src/events.rs`, tests `events::tests`). A right click on any block posts
    NeoForge's `PlayerInteractEvent.RightClickBlock` to the mod's listeners before the block's own
    use and the item's (as `CommonHooks.onRightClickBlock` in `ServerPlayerGameMode.useItemOn`), and
    a right click with any item posts `RightClickItem` before its `use`. Listeners of the classes
    above (`PlayerInteractEvent`, `PlayerEvent`, ... `Event`) hear them too, in the order they were
    added, until one cancels (`setCanceled`): the game's own handling then stops, and the click was
    used when the result set (`setCancellationResult`, `PASS` unless set) consumes it.
    `setUseBlock` and `setUseItem` both `FALSE` refuse the click. The event answers `getEntity`,
    `getLevel`, `getPos`, `getHand`, `getFace`, `getHitVec` (where the look hits the block,
    `native_player_pov_hit`), `getItemStack` (the hand's one stack: what a listener does to it
    reaches the game), `getSide` (`SERVER`), and `TriState`'s calls. A click nobody listens to makes
    no event. Create applies its manual recipes this way (`ManualApplicationRecipe`), and sets
    values and filters on its blocks. Entering twice: the runtime is one per thread, and the game
    may run a hook while a mod's call runs (a block the mod places asks for its block entity), which
    panicked the server thread ("RefCell already borrowed", the belt connector's second click in
    game). The bundle's hooks now look first (`runtime_busy`): the questions about registrations
    alone (a block's block entity type and ticker, an item's use animation and duration, a block's
    state for a placement) are answered by the probe runtime then, and any other hook leaves the
    work to the game and says so once (`hook_runtime`). `events.rs` joins the bundle's runtime
    files (the test `bundles_carry_every_runtime_source_file` caught it missing). Harness
    `CASING=1` (new): Create's item application recipes and the `c:stripped_logs`/`c:stripped_woods`
    tags as the mod carries them, 16 `RightClickBlock` listeners; andesite alloy on a stripped oak
    log: the event is canceled as used, the log becomes `create:andesite_casing`, one sound, the hand
    3 → 2; on stone, or with an empty hand, nothing happens. The casing reaches Create's
    `AllAdvancements` (its advancement builder is unmapped: no advancement is awarded). The
    `regress.sh` set prints the same (hash order aside). Gaps: `LeftClickBlock`, `EntityInteract`
    and the client's own posting (Create's client listeners: its value boxes, the wrench's
    preview) aren't posted yet; listeners' priorities aren't kept (the order they were added).
  - [x] 2026-11-04 Checked in game (build of 4ec8610; the game build of 184baf2 had stopped at the
    generated placement hook, whose `Placement` lacked the new `hand`: 4ec8610 gives it the main
    hand's). `casegame.sh` (new): a survival player's andesite alloy on a stripped oak log makes
    andesite casing (alloy 3 → 2); on stone nothing happens (2 stay); an empty hand on a stripped
    birch log leaves it. `sandgame.sh` round 3 (new): the paper in the off hand, 3 rose quartz in
    the main hand: holding use leaves 2 rose quartz and gives polished rose quartz, and the
    off-hand paper shows its wear bar. `beltgame.sh`: the second click no longer stops the server
    thread; the hand holds 4 of 5 and the belt is laid, but its start and end (where the shafts
    were) turned to air and the item on it stayed put.
  - [x] 2026-11-04 A block entity goes with its block (`glue::block_entity_at`). The belt's ends
    were lost because the runtime kept the shafts' block entities (made when the game ticked them)
    after the belt replaced the shafts: `getBlockEntity` on the new belt start answered the old
    shaft's, so the belt's first tick (`BeltBlock.initBelt`, `beltLength == 0`) found no
    `BeltBlockEntity` there and broke the start, and the end's tick then broke the end. Now the
    runtime's block entity at a position stands only while the block it was made for is there; a
    changed block lets it go (as vanilla's chunk removes it) and a new one is made for the new
    block. Harness `COMPONENTS=1` with `TICKED=1` (new: the shafts' block entities tick before the
    clicks, and the belt's three times after, each with its saved compound, as the game does):
    before the fix the ticks leave air, middle, middle, middle, air (the game's result); after it
    start, middle × 3, end, each with `Length` 5. In game (build of 68d5473, `scratchpad/beltgame.sh`):
    the two clicks leave `create:belt` start, middle × 3, end between the shafts, the hand 5 → 4;
    a creative motor on the first pulley, and an iron ingot summoned over it is taken off the
    ground into the belt (`BeltBlock.entityInside`: no item entity left, as in Java) and drawn
    riding it, from the motor's end to the far end within 120 ticks (`shots-belt/carry-1..6.png`).
  - [x] 2026-11-04 Mods' HUD layers (`gui.rs`, new). `RegisterGuiLayersEvent` is posted with the
    client's setup and keeps NeoForge's ordered list: vanilla's 25 layers (`VanillaGuiLayers.*` are
    their ids), `registerAbove/Below/AboveAll/BelowAll`, `replaceLayer` (`wrapLayer` is traced and
    drawn unwrapped). Each mod layer is drawn at the Horizon hook matching its place in that list
    (`native_gui_layer_pre`/`_post`, API gui_layers; layers Horizon draws in one pass, such as the
    health, armour, food and air rows, share one hook), by its own `render(GuiGraphics,
    DeltaTracker)`. `GuiGraphics` draws through new `Host` methods (`gui_fill`, `gui_blit`,
    `gui_text` with styled runs, `gui_item`, `gui_size`, `gui_char_width`, `translate`): the pose
    stack, fills and gradients, blits and GUI sprites, strings with their styles (section codes,
    translations with their arguments, vanilla's line breaking for `Font.split`), items, and
    tooltips as vanilla lays them out (`ClientTooltipComponent`, `RenderTooltipEvent.Pre`/`Color`,
    `GatherComponents`). Also `NumberFormat`/`DecimalFormat` (root locale, half even on the exact
    value, tested against the JDK's answers), `EquipmentSlot`, `Player.getItemBySlot` (armour
    through the new `native_worn`, API player_worn), `Player.getPersistentData`, the texture
    manager and blend factors as no-ops. Harness `GOGGLES=<dir>` (new; `GOGGLES_AT=motor`,
    `NO_GOGGLES=1`): a creative motor turns a shaft on the server; on the client the player wears
    goggles and looks at the motor, and Create's `goggle_info` layer draws as in Java: the goggles'
    icon at (270, 119) over the corner of a tooltip box (background, gradient border) beside the
    crosshair, with "   Generator Stats:", "   Kinetic Stress Capacity:" (grey) and
    "    262,144su " (aqua) "at current speed" (dark grey), 0 unmapped calls. Looking at the shaft
    draws nothing (a shaft adds no stress, so Create shows nothing either), and so does the motor
    without goggles. Bundle: `gui_layer_pre`/`gui_layer_post` helpers, whose host draws with
    `native_quad`, `native_sprite`, `native_text` and `native_item` into the frame's `HudBatch`.
    Checked in game (offscreen, survival, goggles put on with `item replace ... armor.head`): at the
    creative motor the HUD shows the same tooltip beside the crosshair (the goggles' icon, the box
    and its border, "Generator Stats:", "Kinetic Stress Capacity:", "262,144su at current speed" in
    their colours); at the shaft nothing, and at the motor bare-headed nothing; no exception in the
    log.
  - [x] 2026-11-04 An `Equipable` mod item is declared with 26.3's `equippable` component, its slot
    from its own `getEquipmentSlot` (`blocks::equippable`): Create's goggles are
    `{"minecraft:equippable": {"slot": "head"}}` (harness `DECLARE`, `SHOW=` lists every item: the
    goggles are the only one; Create's armour extends vanilla's `ArmorItem`, which the API table
    doesn't know as `Equipable` yet), so the game puts them on by use and in the head slot. Their
    `Item.use` (`swapWithEquipmentSlot`, unmapped) answers nothing, which leaves the click to the
    game's own use of an equippable item.
  - [x] 2026-11-04 Mods' own screens (`crates/horizon-compat/src/screens.rs`): `Minecraft.setScreen`
    with a mod's `Screen` opens it on the game (`Host::open_screen`, the game's `native_mod_screen_open`),
    freeing the pointer before the screen's `init` as vanilla does, so a screen that moves the pointer
    there (`GLFW.glfwSetCursorPos`) moves a free one. The screen ticks with the client; it draws in the
    game's `screen_render_post` (vanilla `Screen.render`: the in-world menu background tiled,
    renderables, the tesselator's quads as blits of the shader texture) and hears the pointer, the
    wheel, keys and characters through the screen input hooks (`Screen`/`ContainerEventHandler`
    defaults: focus, dragging, children). Those hooks only read the game (`Host::read_only`), so what
    a screen does there (payloads, opening, closing, the pointer, sounds) waits in an outbox for the
    next client tick (`screens::flush`); Escape closes it through its own `keyPressed`, and a screen the
    game closed itself is let go at the next hook. Key mappings (`Options.keyUse` and 25 others,
    `KeyMapping.isDown`/`matchesMouse`/`getTranslatedKeyMessage`, `InputConstants`) read the game's
    bindings (`Host::mapping`, `key_held`); `Component.keybind` shows the bound key as the Controls
    screen names it ("Right Button", "E"). The client's use key posts `RightClickBlock` on the client
    (`glue::client_use`, from the `interaction_key` hook) as `MultiPlayerGameMode.useItemOn` does,
    the use itself still going to the server. Also: `Vec2`, `Pose`, `HitResult.Type` (plain vanilla
    enums: statics, `values`, `valueOf`), the local player's place, look and reach
    (`blockInteractionRange`, `canInteractWithBlock`, which Create's packets check on the server, so
    a payload's sender now carries its position), `isHolding`, `getPose`, `Inventory.getItem` for the
    hands and armour, `ItemInHandRenderer`'s hand stacks, the client's `SoundManager.play` of vanilla
    instances (`SimpleSoundInstance.forUI`, one at a place). Harness `VSETTINGS=<dir>`
    (`VSETTINGS_DX`): a creative motor turns a shaft at 16 rpm; on the client the player holds the
    use key on the motor's value box: `ValueSettingsScreen` opens after 5 held ticks with the pointer
    on 16 (row ⟲), draws "Generated Speed in RPM", both rows' milestones and the value tag; releasing
    48 GUI pixels to the right picks 60, plays `create:scroll_value`, and sends `create:value_settings`
    (15 bytes) at the next tick; the server takes it and the motor and the shaft turn at 60 rpm.
    Bundle: one client host for every client hook (`GameRef`: mutable for the tick, payloads and the
    use key; shared for the screen hooks), new helpers for `screen_render_post`, the 7 screen input
    hooks and `interaction_key`; Horizon natives `native_mapping_binding`, `native_key_held`,
    `native_gui_size`, `native_set_cursor`, `native_local_player` (fragment `mod_screen_input`).
    - [x] 2026-11-04 In game (offscreen, a 960 x 540 window: GUI 480 x 270 at the automatic scale 4, the
      harness's layout): holding right-click on a creative motor's value box opens Create's
      `ValueSettingsScreen` with the pointer on 16; a drag of 48 GUI pixels and the release set the
      motor's `ScrollValue` to 60, as in the harness. On the way: bundles now carry the dispatcher's
      helpers from `horizon-compat`'s root (`parts`, the miss counters), which their engine lacked.
    - Not yet: a mod's own looping, ticking sound instances (Create's `SoundScape` machine hum),
      `AbstractWidget`/`Button`/`EditBox` natives, container screens, `ScreenEvent`s, mods' own key
      mappings and `consumeClick`.
  - [x] 2026-11-04 The runtime's costs, measured (harness `NET=1 NET_TICKS=20000`, Create's client tick
    at the harness's opt-level 1). Once the client tick ran Create's and Ponder's handlers for real it
    took 991 µs; `HORIZON_COMPAT_PROFILE=1` (new) times each listener of each event
    (`neo::profile()`, printed by the harness) and showed `ClientEvents.onTickPost` and Ponder's
    `onTickPre`, and macOS `sample` where inside them: SipHash, a block's item found by walking every
    registered item (`asItem`, which Create's placement helpers ask each tick), and the API key split
    again by each of the ~30 modules a call is offered to (a third of the tick in `memchr`). Now: the
    runtime's maps hash with FxHash (`horizon_compat_core::FxHasher`, inline: no new dependency;
    `Data::Map`, a Java map's iteration order, keeps std's), a block's item comes from an index made
    once per registry size (`creative::block_item`), a call's key is split once (`crate::parts`),
    a call that missed is told by a miss counter instead of two lookups of the key, and an event goes
    to the listeners indexed for its class (`Rt::listeners_by_event`) instead of a copy of every
    listener checked against it. A call no literal arm of `api::call` takes was then offered to each of
    the 32 modules of its chain in turn, and the hot ones (`Iterator.hasNext`, `ItemStack.getItem`,
    `Minecraft.getInstance`) are answered by the 26th to the 30th. Each module now says which keys it may
    answer from the key alone (`claims(owner, name, desc)` beside its `call`: every arm, guards aside; a
    module that answers by the receiver's class lists the method names it answers that way), and a key
    is offered only to the modules that claim it, worked out once per key (`api::claimed`).
    `HORIZON_COMPAT_CHECK_DISPATCH=1` (always on in the unit tests) offers every call to every module and
    says when one answers a key it doesn't claim: none in the harness's checks (belt, fan, press, mixer,
    drop, wheel, crank, recipes, network, value settings) or in 20 000 client ticks. 991 → 514 (hash,
    index) → 320 (split once) → 302 (listener index) → 174 µs a tick (298 with the check on).
  - [x] 2026-11-04 Every inventory slot through the Host (the base for menus): `Host::player_slot`,
    `set_player_slot` and `update_player_slot` reach slots 0 to 40 (hotbar 0-8, inventory 9-35, armour
    36-39 feet to head, off hand 40) when the host says `player_inventory()`, and the hands and armour
    otherwise (the default methods); `player_selected`, `player_id`. A slot's `ItemStack` object stays the
    same object while the game keeps that slot's stack (`hand::slot_stack`, interned per player and
    slot), as a Java inventory's would: Create's `HeldItemGhostItemMenu.stillValid` compares
    `getSelected()` with the stack it opened on. What a mod does to it (count, components, damage) is
    written back after the hook. Stacks on the wire (`network::put_stack`/`read_stack`, shared by mod
    payloads and menus) carry their components and damage.
  - [x] 2026-11-04 Container menus and their screens, natively (`menus.rs`, `container_screens.rs`,
    `widgets.rs`). Server: `Player.openMenu` (vanilla's, NeoForge's with extra data or a `BlockPos`) closes
    the old menu, makes the provider's (`createMenu`), sends OPEN (type, id, title JSON, extra data) and
    then the contents over `horizon_compat:menu`, and keeps them in step as vanilla's synchronizer does
    (`broadcastChanges`: listeners, then what the client doesn't have, one UPDATE message for all of it,
    split above 24 000 bytes; data slots as shorts). Clicks are vanilla's `doClick`, ported whole:
    pickup and place, right-click halves, shift with the mod's `quickMoveStack` and `moveItemStackTo`,
    drags (quick craft), double-click gather, hotbar and off-hand swaps, clone, throw with drops from the
    eyes. `Slot`, NeoForge's `SlotItemHandler`, the player's `Inventory` (each of the 41 slots through
    `hand::slot_stack`, vanilla's `add` / `placeItemBackInInventory`), `SimpleContainer`, `DataSlot`,
    `ContainerLevelAccess`, `MenuType` (NeoForge's `IMenuTypeExtension.create`), `SimpleMenuProvider`.
    What a client sends waits for its player's tick (`glue::player_tick`, new binding), where the host
    acts for that player: CLICK runs as `handleContainerClick` (stale state id: full state back),
    BUTTON, CLOSED; then `broadcastChanges` and `stillValid` (closes for both when false). Before
    anything runs, every inventory slot is reached, so `removed()`'s `saveData` into the held stack
    reaches the game. Client: OPEN makes the menu with the type's factory and the screen the mod
    registered (`RegisterMenuScreensEvent`, posted after client setup), clicks are predicted and sent
    with the changed slots (`MultiPlayerGameMode.handleInventoryMouseClick`), UPDATE/CLOSE apply.
    `AbstractContainerScreen` is vanilla's: layout, `renderBg`, slots, highlight, labels, the carried
    stack, tooltips, mouse and keys (inventory key, hotbar keys, pick, drop). `AbstractWidget`,
    `AbstractButton`, `Button` (+ builder), `Tooltip`. `ServerPlayer` objects are now one per player
    (`menus::server_player`), also for payload contexts and `acting_player`. Dispatch chain: 35 modules,
    claim bits u64. Tests: two runtimes (server and client hosts) open Create's filter shape (inventory
    + 2 ghost slots over an `ItemStackHandler`), draw it, click, place, sync a server-side change,
    close with Escape (saved into the hand), close when the stack leaves the hand; vanilla click counts
    (right-click 10 → 5/5, drag 5 over 3 → 1 each + 2, gather, shift onto 60 → 64 + 6, throw 64, swap).
    Gaps: vanilla's own menus (`ChestMenu`...) don't open yet (no menu type of a mod's); `Player.drop`
    loses components; no `ItemStackedOnOtherEvent` / `PlayerContainerEvent`; touchscreen mode;
    `Inventory.items` / `armor` lists aren't fields.
  - [x] 2026-11-04 Create's own filter, both sides, with the real classes (harness `FILTER=<content mod>`,
    `scratchpad/harness/src/filter.rs`; server and client each on their own runtime and thread, as in the
    game). The server's player holds `create:filter` (5 diamonds in slot 1) and uses it:
    `FilterItem.use` → `openMenu` → OPEN + UPDATE (45 + 171 bytes); the client makes `FilterMenu` (54
    slots: 36 of the inventory, 18 ghost `SlotItemHandler`s) and opens `FilterScreen` (214 x 211 at
    122, 34 in a 480 x 270 GUI), which draws the inventory's stacks and labels. Clicks on the diamonds
    (carried 5), on the first filter slot (a ghost copy, the 5 still carried), back on the diamonds'
    slot, then Escape: each CLICK reaches the server and runs in the player's tick (the diamonds leave
    and come back to slot 1 there too), CLOSED runs `removed()` → `saveData`, and the filter in the
    server's hand then holds `create:filter_items: [{slot: 0, item: {id: "minecraft:diamond", count:
    1}}]` with `filter_items_blacklist` and `filter_items_respect_nbt` 0, vanilla's own form.
    New: `ItemContainerContents` (`handlers.rs`: `fromItems`, `copyInto`, `copyOne`, `stream`,
    `nonEmptyStream`, `nonEmptyItems(Copy)`, NeoForge's `getSlots` / `getStackInSlot`, `equals`,
    `hashCode`, `EMPTY`; `CODEC` as `[{slot, item}]` in NBT and JSON, slots 0..255; `STREAM_CODEC` as a
    list of optional stacks; test `container_contents_keep_their_slots_through_the_codec`). Game side:
    natives `native_slot_stack` / `native_set_slot_stack` / `native_selected_slot` (fragment
    `slot_stack.json`: every slot by vanilla's numbering, the armour feet up, 40 the off hand; a client
    reads its own and changes none), the bundle's player host answers every slot (`player_id`,
    `player_selected`, `player_inventory`, `player_slot`, `set_player_slot`, `update_player_slot`) and
    now sends payloads (a menu opened from `use_item` reaches the client), the client host reads every
    slot, and a new `player_tick` helper (binding 17) calls `glue::player_tick` from the game's
    `native_player_tick`, inside `as_guest`. `coverage` takes `MISSING=all`.
  - [x] 2026-11-04 Drops keep their data: `Host::spawn_stack_entity` (whole stack: damage, mod components;
    default falls back to item and count), used by `Player.drop` (a menu's throw, a stack carried out
    of it) and `addFreshEntity` of a new `ItemEntity`. The bundle's player host had no item spawning at
    all, so a throw from a mod's menu lost the stack in game: it now spawns both forms. Coverage
    probes each method again with an object of its owner's class as the receiver when null arguments
    find no answer (a handler that checks its receiver answered nothing before): Create 1770 of 4244
    API methods (41.7%), 19 844 of 25 840 class uses (76.8%); still a lower bound (codecs, lists and
    handlers answer only for their own runtime objects).
  - [x] 2026-11-04 Create's filter works in game, end to end (`scratchpad/filtergame.sh`: survival, a
    filter and 5 diamonds, `use-item`, three clicks, Escape): FilterScreen opens on the client, the
    diamonds go on the cursor, a click on the first filter slot sets a ghost diamond, the stack goes
    back to its slot, and after Escape the held filter reads `create:filter_items: [{item: {count: 1,
    id: "minecraft:diamond"}, slot: 0}]` (vanilla's form) with the 5 diamonds still in slot 1. The
    fault was the bundle's server `send_payload`: inside a player hook (`as_guest`) `self.lan` is
    None, so `native_server_players()` is empty and a payload to that player was "not sent". It now
    sends `PayloadTarget::Player(uuid)` straight to `native_net_send_to_player`, and `All`/`Near`
    count the acting player too. Clipboard: `Host::clipboard`/`set_clipboard`, natives
    `native_clipboard` (reads the system clipboard through a fresh connection, so a read-only screen
    host can paste) and `native_set_clipboard` (fragment `native_clipboard.json`), the client host
    answers both, `Minecraft.keyboardHandler` (`KeyboardHandler.getClipboard`/`setClipboard`), and a
    write from a screen's input waits for the next client tick (defer kind 5).
  - [x] 2026-11-04 Text fields: `EditBox` natively (`horizon-compat/src/edit_box.rs`), with 1.21.1's
    API and behaviour, for vanilla fields and mods' subclasses (Create's `HintableTextFieldWidget`,
    station and schedule names, `AddressEditBox`): the value with cursor and selection in UTF-16
    units, the filter, the responder (told of every change and cursor move, twice for `setValue`, as
    1.21.1 does), the formatter, typing (`StringUtil.filterText`, the maximum length, a surrogate pair
    kept whole), backspace/delete by character or word, the arrows, home/end, shift extending,
    select-all/copy/paste/cut through the clipboard, a click placing the cursor (no click sound), and
    `renderWidget` line by line (sprite, text scrolled to the cursor, blinking `_` or bar, suggestion,
    hint, selection). The widget dispatcher routes `EditBox` instances there first, so inherited
    `AbstractWidget` calls still work. 4 tests. Item names: `Item.getDescriptionId` (a block item's is
    its block's unless it is an `ItemNameBlockItem`; vanilla's by the language, as the game does),
    `getDescription`, `getName(stack)`, `ItemStack.getHoverName` (custom name, item name, then the
    item's), `getDescriptionId`, `getDisplayName` (brackets, italic for a custom name; no rarity
    colour or hover yet), each step a virtual call so a mod's override answers. Create's filter screen
    had an empty title for want of `Item.getDescription`.
  - [x] 2026-11-04 Addons with the mod they build on: a mod group is one engine. Create: New Age
    extends Create's classes (`GeneratorCoilBlockEntity extends KineticBlockEntity`) and registers
    through Create's `CreateRegistrate`, so it must share Create's runtime, as both share one JVM.
    The installer (`horizon-port-pack/src/install.rs`, `groups`) puts each jar with the jars that
    require it (manifests' required dependencies within the pack; a library a jar carries in
    jar-in-jar is its own), in loading order, and translates each group once
    (`bundle::write_group`, CLI `bundle <jar> <out> --with <addon jar>...`): the classes of all of
    them (`classes_of`, the first jar's class winning), their services, one engine
    `<first id>_translated`; FML's order for the `@Mod` constructors (bundled libraries, the mod,
    then its addons, `Program::addons`). Every hook asks `<engine>::ours(id)` (the engine's
    `NAMESPACES`) instead of one namespace; the `content` hook declares, for each content mod, its
    own namespace's blocks, items and tabs; each mod keeps a content mod of its own (`content/` for
    the first, `content-<id>/` for each addon, with the blocks its own classes register,
    `class_names`), and the installer removes an addon's old standalone bundle. Each
    `DeferredRegister` names in the mod id it was made with (`$ns`). NeoForge's jar-in-jar is read
    from `META-INF/jarjar/metadata.json` (New Age keeps its jars in `META-INF/jars/`; MixinExtras is
    left out). Measured: Create + New Age install as one bundle; the harness on that engine declares
    New Age's 34 blocks and 18 items under `create_new_age` and Create's filter test still passes.
    New Age's tab was empty: Registrate's `CreativeModeTabModifier` implements `CreativeModeTab.Output`
    and gets `accept(ItemLike)` from the interface; a missed call now retries on the interfaces
    the receiver's classes implement (`retry_classes`), and a mod's own `Output` takes the default
    methods (a stack and the default visibility to its own `accept(stack, visibility)`). The world
    around a block, for any mod (`horizon-compat/src/level_env.rs`, Host `light`, `day_time`,
    `weather`, `world_height`, `neighbor_signal`, `schedule_block_tick`, from the game's natives
    `native_light`, `native_weather`, `native_day_time`, `native_world_height`,
    `native_neighbor_signal`, and new ones for level hooks and for code outside them, fragment
    `native_level_reads.json`): `getBrightness` (`LightLayer`), `getRawBrightness`,
    `getMaxLocalRawBrightness`, `canSeeSky`, `getSkyDarken`/`isDay`/`isNight` (1.21.1's formula:
    beds work from 12542 to 23459, 12010 to 23991 in rain, as the wiki says), `getTimeOfDay`,
    `getMoonPhase`, rain and thunder, `hasNeighborSignal`/`getBestNeighborSignal` (Create's clutch
    and gearshift), `scheduleTick` (to the mod block's `tick` through the `tick_block` hook),
    `getBlockTicks`, `getRandom`, the world's height. 3 tests.
    Measured on the group's engine (harness built `WITH=` New Age's jar): `TABS=1` gives Create's
    203 + 377 items and New Age's tab `create_new_age:tab` 48 items, generator coil first;
    `COIL=<New Age's content mod>` (`scratchpad/harness/src/coil.rs`): Create's creative motor
    (facing east, 16 rpm) beside New Age's generator coil (axis x), twelve layered magnets round it:
    the coil (`GeneratorCoilBlockEntity`, a subclass of Create's `KineticBlockEntity`) turns at 16,
    applies 72 stress (its own 24 and the magnets' 48) and makes 22 energy a tick (48 × 16 ×
    0.029296875, the config's `suToEnergy`, cut to an int); `COIL_MAGNETS=0`: stress 24, energy 0.
    `FILTER=` on the same engine: Create's filter test unchanged.
  - [x] 2026-11-04 In game, Create + New Age as one engine (`scratchpad/cnagame.sh`, build with
    `WITH=` New Age's jar; the stage copies the bundle without its content folders only): New Age's
    32 blocks placed in two rows draw with their models, its tab "Create: New Age" lists its items
    with their icons, Create's two tabs still there. The game's log showed New Age's carbon brushes
    throwing on every tick: ESL's transactions (`NeoForgeTransaction`) keep the `Thread` they were
    opened on and check `this.thread != Thread.currentThread()`, and the runtime made a new
    `Thread` on each call, so the first commit threw and left its transaction open for good.
    `Thread.currentThread()` is now one object per runtime (one runtime is one game thread; the
    client's is the render thread). Harness `COIL` has the brushes beside the coil: with the
    trace on, no exception; they take the coil's 22 a tick and store up to 440 (their capacity,
    20 ticks of output), as New Age's code says. Test `jlang::tests::a_runtime_is_one_thread`.
  - [x] 2026-11-04 Measured over a real pack (CurseForge "Create Ultimate Selection 2", 367 jars,
    read-only). `coverage` over every jar, summed (datagen-only APIs left out): 53.5% of the API
    methods each jar calls answer a probe, 68.7% of the call sites; a lower bound, as the probe
    calls with null or bare receivers. 200 of the jars require another mod of the pack (61 require
    Create). New `mixins <jar>...` (`mixin_injections`; the class parser now keeps class-file-only
    method annotations and a nested annotation's values): 248 jars have mixins, 9595 members:
    4001 `@Inject`, 2060 methods added to a target, 1053 accessors and 249 invokers (applied),
    650 `@Redirect`, 504 `@WrapOperation`, 172 `@Overwrite`, the rest smaller injectors. By
    target: client rendering first (`LevelRenderer`, `GameRenderer`, `Minecraft`), then entities
    (`LivingEntity`, `Entity`, `Player`), and ~950 injections into Create's own classes (by its
    addons): those targets are translated too, so their mixins can be woven by the translator
    itself. Test `mixin_census_tests` (a class file built by hand: `@Mixin`, `@Inject` with
    `@At(INVOKE, target)`, an added method).
  - [x] 2026-11-04 Loading the pack's own mods (`scratchpad/lt`: `one.sh <jar> <name>` bundles a
    jar on the runtime core, builds a load harness against its engine (debug, no debug info: a
    1M-line engine builds in ~70 s) and runs the mod's loading with `HORIZON_COMPAT_TRACE=1`:
    registrations, `@Mod` constructors, common and client setup; `round.sh <list>`, `summary.py`
    tabulates). 28 jars picked by size and kind: FarmersDelight, Storage Drawers, Refined Storage,
    Mekanism, SecurityCraft, JEI, Jade, FramedBlocks, Macaw's, Curios, GeckoLib, Lootr, Comforts,
    Max Henkel's mods... First round: 21 got through loading; now all 28 do. Every fix is general:
    - mods.toml values with a comment after them (`modId="torchmaster" #mandatory`) read as the value
      (`bundle::toml_string`; 4 jars didn't build).
    - API calls nested past 512 throw `StackOverflowError` (`api::MAX_DEPTH`) as the JVM would,
      instead of overflowing the game's stack; a codec operation on an API object (`apply_op`) is a
      miss instead of a loop (Mekanism and SecurityCraft crashed).
    - Exceptions made from a cause (`new RuntimeException(e)`) carry its text; `getCause`, `initCause`.
    - FML: a mod's container is FML's Java one (`FMLModContainer`, Comforts threw "not a FML one"),
      its own while its constructor runs (the generated `start` names the active mod), the same
      through `ModList` and `ModLoadingContext.get().getActiveContainer()`. `getModInfo()` has the
      version and name the translator reads from each `[[mods]]` table (`mod_metadata`,
      `${file.jarVersion}` from the manifest, else FML's `1`). `ModConfig.Type` constants.
    - Reflection: every constructor of a mod class answers `getConstructor(Class...)`/`newInstance`
      (the table had the no-argument ones only), primitives unboxed (Max Henkel's configs: Corpse,
      Easy Villagers, Pipez).
    - `ServiceLoader.stream()` and its `Provider`: Storage Drawers' Chameleon finds its NeoForge
      services, 0 → 152 blocks and 159 items registered.
    - `PacketFlow`'s constants, `getOpposite`, `id` (Max Henkel's messages); Guava's `Splitter`,
      `Joiner`, `ObjectArrays.concat` (SpectreLib's configs).
    Left: FramedBlocks' state cache ("Invalid shape"), Mekanism's own registry objects and its saved
    data asking for the server, SecurityCraft's tab (an empty optional). Groundwork for weaving
    mixins into translated classes: Mixin's callback objects (`horizon-compat/src/mixin.rs`), the
    weaving plan (`weave.rs`, used by the codegen since the next entry). Tests:
    `neo::tests::a_mod_gets_its_own_fml_container_with_its_version`,
    `mixin_census_tests::mod_metadata_is_read_as_fml_reads_it`,
    `jutil::tests::guava_splits_joins_and_concatenates_as_guava_does`,
    `mixin::tests::a_handler_cancels_with_a_value_the_method_then_returns`,
    `weave::tests::selectors_pick_methods_as_mixin_does`, api's recursion and cause tests, bundle's
    toml test.
  - [x] 2026-11-04 Mixins into translated classes, woven by the translator. An addon's mixin whose
    target is a class being translated (Create's) is applied as the target's Rust is written, as
    Mixin applies it at class load: its methods join the target (added ones, `@Overwrite`s), calls
    and field reads through the mixin go to the target, its interfaces become the target's, its
    static initialiser runs after the target's and its field initialisers in the target's
    constructors, right after their super call. Each injector is a direct call to its handler at
    its place, nothing looked up at run time: `@Inject` at HEAD, RETURN, TAIL or a call (INVOKE,
    before or `shift = AFTER`, `ordinal`), cancellable, with a `CallbackInfoReturnable` for a value;
    `@Redirect`, `@ModifyArg`, MixinExtras' `@ModifyExpressionValue`, `@WrapWithCondition`,
    `@ModifyReturnValue`, and `@WrapOperation`/`@WrapMethod`, which get the wrapped call or method
    as an `Operation` (a generated lambda) they may call. The plan checks that an `@At(INVOKE)` names
    a call the method makes (its owner as the call instruction writes it, as Mixin compares), so a
    wildcard (`method = "*"`, Sable's basin mixin) goes only where the call is.
    `weaves <mod jar> <addon jar>...` prints the plan (`HORIZON_WEAVE_DEBUG=1`: each handler left
    and why). Over the pack's 20 largest Create addons (`scratchpad/weaves.sh`): 820 handlers
    woven, 178 left (82.2%; 645 before the two wrappers). Left: 88 `@Inject`s capturing locals,
    37 `@At(FIELD)`, 17 `@ModifyVariable`, 10 methods not found (Railways' common mixins written for
    other Create versions; their config's `defaultRequire: 0` makes Mixin skip them too), 6
    `@Slice`, 5 `@ModifyReceiver`, 3 `@At(NEW)`, 3 `@At(CONSTANT)`, 3 `@WrapMethod`s with `@Share`d
    locals, 2 `shift = BY`, 2 calls not in their methods, 1 `@ModifyConstant`. Create + New Age:
    no mixins between them, the generated engine is byte for byte the same; harness TABS, COIL,
    FILTER unchanged. Test `tests/weave.rs`: `tests/fixtures/weave` (a `Machine`, an addon's
    `MachineMixin` using every kind, stubs of Mixin's and MixinExtras' annotations) compiled with
    javac, translated and run; 21 answers worked out by hand from what Mixin's weaving does.
  - [x] 2026-11-04 Weaving, continued: a real addon woven into Create and run, and most of what
    was left. Create + New Age + Create: Connected as one engine (`WITH=` both): the harness's
    TABS lists Connected's tab (47 items) beside Create's (203, 377) and New Age's (48); COIL and
    FILTER as before. Its first run found two bugs the one-file test could not: the generated
    `Operation` functions were private to their part of the engine (the lambda table calls them
    from `lib.rs`; the test now emits one class per file, as a bundle does), and a static
    `@Invoker` (Connected adds `SequencerInstructions` constants through the enum's constructor:
    `@Invoker("<init>")` from its mixin's static initialiser) ran its own body, javac's `throw
    new AssertionError()`, which stopped Create's constructor: an accessor or invoker is now one
    by its annotation, body or not, and a constructor invoker of a translated class makes the
    object with its own constructor. A `@WrapMethod` handler also ran a second time as an
    injection at its method's head (now in the test: a wrapper with a side effect, run once).
    New: mixin configurations (`mixin_configs_of`: the mods.toml's `[[mixins]]`, the manifest's
    `MixinConfigs`): only the mixins they list are applied, in their order (then by priority),
    and an injector that finds no place but may (`require`, else the config's
    `defaultRequire`, 0 by default) is skipped as Mixin skips it. The class parser keeps the local
    variable tables, parameter annotations and generic signatures, so a handler gets the target's
    locals: MixinExtras' `@Local` (by type, `ordinal` counting the arguments, `index`, `name`, with
    Mixin's `argN` names for arguments), its `Local*Ref`s (written back into the local after the
    handler), `@Share` (a ref per run of the method), and `@Inject(locals = ...)`'s captured
    locals; `@At("RETURN", ordinal)` (was every return), `INVOKE_ASSIGN` after the store of the
    call's result, `@At("FIELD")` (a read or write woven as a call `()T`/`(T)V`: `@Redirect`,
    `@ModifyExpressionValue`, `@WrapOperation`, `@Inject`), `@ModifyVariable` (head, `STORE`,
    calls, returns), `@ModifyReceiver`. Over the 20 addons: 969 of 990 handlers woven (97.9%; 12
    more skipped as Mixin skips them); left 6 `@Slice`, 4 `@At(NEW)`, 3 `@WrapMethod`s with
    `@Share`, 3 `@At(CONSTANT)`, 2 `shift = BY`, 1 `@ModifyConstant`, 2 others. Plugins
    (`IMixinConfigPlugin.shouldApplyMixin`, which may turn a listed mixin off by the mods present)
    are not run yet: every listed mixin applies. Tests: `tests/weave.rs` now 43 answers (locals,
    refs, shares, a captured local at the third return, `INVOKE_ASSIGN`, field reads and writes,
    `@ModifyVariable`, `@ModifyReceiver`, an enum constant added by a constructor invoker; the
    fixture compiled with `-g` as Gradle compiles mods, and plans with and without a
    configuration), `mixin_census_tests::mixin_configs_are_found_as_fml_finds_them`.
  - [x] 2026-11-04 Weaving: slices, constants, `new`s, `instanceof`s. Five mods as one engine
    (Create, New Age, Create: Connected, Create: Vibrant Vaults, Create: Pattern Schematics; the
    harness's `WITH=` all four): TABS lists Vibrant Vaults' tab (165 items) and Create's base tab
    with 205 items, not 203, because Pattern Schematics' `@Redirect` into Create's
    `RegistrateDisplayItemsGenerator.outputAll` puts its `empty_pattern_schematic` and
    `pattern_schematic_and_quill` before Create's `empty_schematic` and `schematic_and_quill`, as
    its handler does (it accepts the inserted item first, then the original); COIL and FILTER as
    before. New: a mixin's nested annotations are kept as annotations (`Annotation::nested`), so
    `@Slice` works (`from`/`to`, the first place each names or the last with `:LAST`; a `from` that
    finds nothing starts at the method's start, a `to` that finds nothing ends at the slice's start,
    as Mixin's `MethodSlice`; ordinals count inside the slice); `@At("CONSTANT")` (`args` with
    `intValue`, `floatValue`, `longValue`, `doubleValue`, `stringValue`, `classValue`, `nullValue`)
    and `@ModifyConstant` (its `@Constant`s, or every constant of its handler's type without one);
    `@At("NEW")` (a class, or a constructor's descriptor) for `@Inject` and `@ModifyVariable`;
    MixinExtras' `@WrapOperation` of an `instanceof` (`constant = @Constant(classValue = ...)`: the
    handler gets the object and the check as an `Operation`); handlers that take the target
    method's first arguments after their own, as many as they have room for (Mixin's
    `captureTargetArgs`: Railways' `@ModifyExpressionValue` of a constant takes 5 of
    `tryConnect`'s 7). Over the 20 addons: 983 of 990 woven (99.3%; was 969). Left: 3
    `@WrapMethod`s with a `@Share` (Create Big Cannons), 2 `shift = BY` (counts ASM's label, line
    and frame nodes, which the translator does not have), 1 `@Redirect` of a `new` (Sable), 1
    `@Local` by a name the local variable table doesn't have there. Mixins into the game's own
    classes (not translated) are still the hooks' (`horizon-compat`'s). Config plugins of the kind
    Create's addons share are run as they decide (`plugin_turns_off`): a mixin annotated with mods
    (strings, or an enum's constants: the string each is made with, else its lowercased name) and
    `applyIfPresent` applies when any of those mods is among the jars translated together exactly
    when `applyIfPresent` says (Create: Connected's and Copycats+' `@ModMixin`, Railways'
    `@ConditionalMixin`; `@DevEnvMixin`s never). With Create alone, Copycats+ turns 6 of its
    compat mixins off (Sodium's, Radium's, Diagonal Blocks'...) and Railways 5 (Simple Voice
    Chat's, MaLiLib's); Create: Connected's 3 that apply without Railways stay on (its
    `@ModMixin(mods = "railways", applyIfPresent = false)`). Other plugins (73 in the pack, most
    deciding by mods or configs, some rewriting classes with ASM) apply every mixin still. Tests:
    `tests/weave.rs` 56 answers (`DialMixin`: slices on both ends, a `@ModifyConstant` by value and
    by type with the method's argument, a string constant, a constant in a slice, before `new`s, an
    `instanceof`, an argument prefix) and a plugin's choices (`@ModMixin` with an enum's mods: one
    there, one not, one by the string its constant is made with, `applyIfPresent = false`).
  - 2026-11-04 Weaving: `shift = BY`. The translator now has ASM's node list as Mixin sees it
    (`weave::nodes`): at each instruction's offset a label when anything points there (jumps,
    switches, the exception table, where lines and local variables start and variables end,
    frames, and the `new`s ASM's frame scan finds), its line numbers, its frame, then the
    instruction; the class reader keeps `LineNumberTable` and `StackMapTable` offsets for it. A
    place `by` nodes on lands on an instruction (or the line number or frame before one): the
    handler goes before that instruction. One that lands on a label goes after the instruction
    before it, so it runs only as the code falls through there, not when a jump lands on the
    label (Mixin inserts before the label). `INVOKE_ASSIGN`'s own node is Mixin's `AfterInvoke`'s:
    after the call, its result's cast and store. Both in the pack weave now: Create: Connected's
    `SequencedGearshiftScreen.updateParamsOfRow` (`by = 2` from `standardStep()`) and Railways'
    `ScheduleRuntime.estimateStayDuration` (`INVOKE_ASSIGN` of `List.get`, `by = 2`, with a
    `@Local` by name). Tests: `tests/weave.rs` 60 answers (after a store, after a cast and store,
    and on a label an `if` jumps to: the handler runs on the fall-through only).
  - 2026-11-04 Ten addons as one engine: too big for one crate. Create with New Age, Connected,
    Vibrant Vaults, Pattern Schematics, Copycats+, Power Loader, Encased, Dragons Plus,
    Enchantable Machinery and Electro Energetics translated (all their mixins woven) into 218 MB of
    Rust (2.35M lines, 73.7K functions); rustc ran out of memory on the 24 GB Mac (swap took the
    disk down to 265 MB, then rustc was killed). Create with 4 addons (161 MB) still builds. Next:
    a leaner codegen (basic blocks instead of a `match` arm per instruction, no heap `Vec` per
    call's arguments), measured, then splitting a group's engine if that is not enough.
  - 2026-11-04 A leaner codegen: the ten-addon engine builds. A `match` arm per basic block (an
    instruction only the one before reaches runs in that one's arm), and the operand stack a fixed
    array on the Rust stack (`Stack<max_stack>`; a call takes its arguments from it, no heap `Vec`).
    The same ten addons: 2.35M → 1.32M lines, 1.22M → 194K arms, 218 → 173 MB; the harness built
    it in 7 min 53 s with rustc at 10.1 GB at most (swap +1.7 GB). In the harness: 11 creative tabs
    (Copycats+' two, Encased's 189 items, Electro Energetics' 81, Power Loader's 4...), New Age's
    coil (16 rpm, 72 su, 22 FE/t stored to 440) and Create's filter (a diamond in `filter_items`)
    as before. Java's pattern `switch` (`SwitchBootstraps.typeSwitch`/`enumSwitch`: Dragons Plus,
    Electro Energetics, Create's clipboard) translated, checked against the JVM (`patterns`).
  - 2026-11-04 Libraries as far as the mods reach them; Kotlin mods. Enchantable Machinery (Kotlin)
    threw in its constructor: Kotlin's `Intrinsics.areEqual` wasn't there, an unmapped call answers
    nothing, and its own config check failed. Kotlin's library comes with Kotlin for Forge, the jar
    of its `modLoader` (`kotlinforforge`): the installer now gives a group the jar of each language
    loader its mods name, and a library (that jar, or a jar-in-jar that declares no mod) is
    translated only as far as the mods' code reaches it (`reach.rs`, rapid type analysis): a static
    or `super` call runs its method; a virtual call runs that signature's method in the library
    classes a created class has above it; a created class handed to code that isn't translated
    keeps all its methods. Kotlin for Forge carries 4 757 classes; behind Enchantable Machinery 120
    of them are kept (class-level reach kept 3 097: kotlin-reflect, which Kotlin's `Reflection`
    looks up by name; a library's own lookups by name now stay out, and Kotlin falls back to its
    basic reflection as without kotlin-reflect). Create with it: 115 MB of Rust (Create alone 110).
    Kotlin for Forge's loading context (`KotlinModLoadingContext`, which a Kotlin mod asks for its
    bus) is the runtime's. Checked against the JVM with a library inside the fixture's jar
    (`library`): what is left out, and that what is kept runs the same.
  - 2026-11-04 `FMLConstructModEvent`, which NeoForge fires once the mods are built and before the
    registries: Dragons Plus makes all its blocks, fluids and items there, so it had registered
    none (its tab was empty). `LoadingModList.getModFileById`: a mod's file, null for one that
    isn't there. Measured in the harness (the ten addons with Kotlin for Forge as a library: 172
    MB, 198 library classes kept of 4 771): Dragons Plus registers 19 blocks, 34 fluids, 21 items
    and its block entity; Enchantable Machinery its 12 blocks and 12 block entities. Its
    constructor then stopped on Kotlin's `component2(...) must not be null`: a destructured entry of
    a fastutil `Object2DoubleMap`, whose numbers the runtime keeps unboxed, and `ifnull` tested
    `.r() == 0`. Null is now the null reference or an unmapped call's nothing (`V::is_null`), never
    a number (`rawBoxes` against the JVM, with `IntStream.range`, which was unmapped).
  - 2026-11-04 Measured with the leaner engine (ten addons, Kotlin for Forge as a library):
    Enchantable Machinery's constructor runs through (its 12 blocks and 12 block entities), Dragons
    Plus' tab lists its 18 items (fluid hatch, dye buckets): 11 tabs. New Age's coil (16 rpm, 72 su,
    22 FE/t) and Create's filter as before.
  - 2026-11-04 Blocks before their names. Registrate makes a block, then registers it, and Copycats+'
    constructors cache their face shapes from `defaultBlockState()` meanwhile: a state is its
    block's name and properties here, and the name was empty, so `state.getBlock()` gave a stand-in
    (`registered "" block 1`) and the copycat's own methods weren't there (1 542 misses each). A mod
    block not registered yet now has a name of its own (`horizon_unregistered:block_<n>`), and
    registering it renames the states made under that name in place: the same objects, so what
    kept them sees the registered name, and the game asking by name gets them. Also mapped, by
    count at load: `ObfuscationReflectionHelper` field access (1 578: Registrate clears each
    block's copied `Properties.drops`), `DeferredHolder.equals`/`hashCode` (636), `Registry.addAlias`
    (622; lookups follow it), `ResourceLocation.tryBuild` (351), 1.21.1's `ItemTags.STAIRS`, `SLABS`,
    `DOORS`... (112; 26.3 keeps them in `BlockItemTags`, the block tag's id), `Vec3i`'s own
    arithmetic (432), a block-state enum's `getSerializedName`. In the harness: misses at load
    193 → 187 kinds, the stand-in gone, Copycats+' parts found. What the face shapes need next is in
    the runtime and tested, measured next: `BlockState.getOcclusionShape`, Guava's
    `DoubleMath.fuzzyCompare`, a `VoxelShape`'s cells (`DiscreteVoxelShape.getSize`, `isFull`: a
    shape on power-of-two grids has `2^bits` cells an axis, as `Shapes.box` finds them; any other
    one between each two coordinates) and `SliceShape` (a face's section). Copycats+ reads the
    cells through a mixin accessor of `VoxelShape.shape`; an accessor of a field the runtime doesn't
    keep as one now asks the runtime (`VoxelShape.$shape`) instead of reading nothing.
  - 2026-11-04 Measured (the ten addons, Kotlin for Forge as a library): with the face shapes'
    calls, misses at load 7 700 → 2 608, the four 1 542-count ones gone. Then:
    `Block.getShapeForEachState` (Copycats+ caches every state's shape by state) over
    `StateDefinition.getPossibleStates`, which now lists every combination of a mod block's
    properties in vanilla's order (the property first by name varies slowest; past 65 536 states
    the default alone), and `cycle` over any property's values. Filling the cache ran Copycats+'
    `CCShapes`, which tests `instanceof Map` on a `HashMap` it made: the runtime's `instanceof`
    knew Minecraft's families but not Java's, threw "Unsupported type", and Copycats+ registered 4
    of its 46 blocks. `instanceof` and `catch` now know Java's own classes as the JDK declares them
    (collections by kind, numbers, strings, the exceptions: `catch (IllegalArgumentException)`
    takes `Integer.parseInt`'s `NumberFormatException`). Also mapped: cauldrons
    (`CauldronInteraction.newInteractionMap`, `EMPTY`/`WATER`/`LAVA`/`POWDER_SNOW`, `INTERACTIONS`;
    `AbstractCauldronBlock(Properties, map)` and its `useItemOn` through the map, so a mod's cauldron
    that keeps vanilla's `useItemOn` answers clicks in game: Dragons Plus' dragon's breath
    cauldron), NeoForge's `FluidType` (its `Properties`, NeoForge's defaults, `SoundActions`,
    `NeoForgeMod.WATER_TYPE`...), `simpleCodec` on any block class, `defaultMapColor`,
    `FastColor.ARGB32`, `DyeItem.byColor`, JOML's `Vector2i`, `Collectors.joining` and `toMap`
    (a key twice without a merge throws, as Java's), `Math.ceilDiv`, `NeoForgeRegistries.Keys`.
    Misses at load 2 608 → 418 (128 kinds), no uncaught exception, Copycats+' 46 blocks; tabs (11),
    New Age's coil and Create's filter as before. Not yet: a mod's entries in vanilla's maps
    (`CauldronInteraction.EMPTY.map().put(dragon breath bucket, ...)`) — the game's own cauldron
    doesn't ask them.
  - 2026-11-04 Next by count at load. Kotlin for Forge's bus calls were mapped but never claimed
    (a module answers only what it claims), so a Kotlin mod's listeners didn't subscribe; they do
    now. `ModList.getMods` answers each loaded mod's info, and with it a mod went on to ask every
    mod's `getModProperties` (misses 418 → 511 at 93 kinds), now mapped with the rest of the mod
    info. Also mapped: `Util.makeDescriptionId`; vanilla's value codecs (`Vec3i`, `Vec3`,
    `UUIDUtil`'s two, `Direction`, `Direction.Axis`, `Mirror`, `Rotation`, `Codec.INT_STREAM`),
    written and read in vanilla's forms; `SoundType`'s 113 constants and `BlockSetType`'s 16 as
    1.21.1's classes declare them (`METAL` at pitch 1.5, `CROP` sounds planted, twisting vines like
    weeping ones at half pitch), a mod's own sound types and NeoForge's `DeferredSoundType` (sounds
    through suppliers); `DispenserBlock.registerBehavior` and `registerProjectileBehavior`, kept by
    item; `ArmorMaterial.Layer` (its texture path as vanilla builds it) and `ArmorMaterials`;
    `FluidInteractionRegistry.addInteraction`; NeoForge's data maps (`NeoForgeDataMaps`, the
    `DataMapType` builders), `RegistrationInfo.BUILT_IN`, `RegistrySetBuilder`, command argument
    types by class (Ponder's), `Holder.direct`, the data generator check, more of vanilla's plain
    enums (`AttributeModifier.Operation`, `MobCategory`, `MobEffectCategory`...). Misses at load
    418 → 134 (83 kinds), the same registrations plus Ponder's argument type, no uncaught
    exception; tabs (11), New Age's coil (16 rpm, 72 su, 22 FE/t) and Create's filter as before.
    Kept but not yet asked by the game: the dispenser behaviours, the fluid interactions, armour
    materials' stats.
  - 2026-11-04 Load order, and the rest of what loading missed (Create + 10 addons, harness).
    A lambda is an instance of its functional interface, so its default methods run (`Function`'s
    `andThen` on a method reference...): `Function.apply` (23) and Registrate's `asProvider` (5)
    went. Mapped (cd88f31): java.nio's `Path`, `Paths`, `Files` and java.io's `File` over the game
    folder, FML's `FMLPaths` (`GAMEDIR`, `MODSDIR`, `CONFIGDIR`, `FMLCONFIG`, folders made as FML
    makes them), Java's `IOException` family; NeoForge's data attachments (`AttachmentType`
    builders, a holder's `getData`/`setData`/`hasData`/`removeData`; the default made once per
    holder), so Create's minecart controller registers; `Tiers` and `SimpleTier` with vanilla's
    numbers, `SwordItem`/`DiggerItem.createAttributes`, more of `Item.Properties`, `MobEffect`'s
    methods, `ItemAbilities`; codecs of components, tag and resource keys and vanilla enums, `and`
    on record products; stream codecs from codecs. An unknown vanilla `CODEC` is a counted miss
    with a codec that fails when used, instead of null. Misses at load 134 → 27 (25 kinds).
    The rest (f168990):
    - FML constructs each `@Mod` class first; a class holding registers initialises when the
      constructor reaches it (or after, if none does). `start` used to initialise every such class
      before the constructors, so Create's `AllArmorMaterials` read its sound entries before
      `AllSoundEvents.prepare()` filled them (`Record.event` on null: the armour's equip sound).
      The hook test's mod has a main class now; the order test fails on the old `start`.
    - Bundles carry `attachments.rs` and `files.rs` (the bundle test caught them missing: the
      game's build would not have compiled).
    - `ParticleTypes.STREAM_CODEC` (New Age's wire particle packets): the type by name, then a
      block's state, an item's stack, a dust's colour, or what a mod's own type's `streamCodec()`
      writes. `ResourceKey.streamCodec` was claimed but never routed to the codec functions; a
      test now goes through the dispatcher.
    - Vanilla's 41 `Foods` from 1.21.1's initialiser, built by the builder (`GOLDEN_APPLE`...);
      `Stream.close`; `ArmorItem.DISPENSE_ITEM_BEHAVIOR`.
    Misses at load 27 → 19 (19 kinds), registrations as before plus Create's attachment type, no
    uncaught exception; tabs (11), New Age's coil (16 rpm, 72 su, 22 FE/t, brushes to 440) and
    Create's filter as before. Left: milk (the game has no milk fluid), item property overrides
    (`ItemProperties.register`, client models), recipe stream codecs (recipe sync, which the
    game does itself), Gson (`Create.GSON`), `Pattern.compile` (Flywheel's shader compiler),
    Copycats' cat variant (`Holder.Reference.createStandAlone`), `ItemStack.getComponentsPatch`,
    `FoodProperties.DIRECT_CODEC`, `OreConfiguration.TargetBlockState.CODEC`, Create Connected's
    server config read on the client before it loads (Java throws a `NullPointerException` and its
    `safeGetter` catches it; the runtime throws none: reading a field of null gives no value).
    Build note: the engine's rustc peaks at ~6.6 GB and macOS
    swap (on the disk) grows ~5 GB, so the watchdog killed one build at < 4 GB free: keep 15 GB.
  - 2026-11-04 The pack's 28 mods again (`scratchpad/lt`, round 5, on a snapshot of the runtime
    at f168990): all 28 load to the end; distinct misses 350 → 239, kinds summed per mod 757 →
    454. Calls went up (10K → 158K) because FramedBlocks now gets as far as its state cache, run
    once per block state: vanilla's `Tuple` (48K `getA`/`getB`) and `framedblocks$initCache`, a
    method FramedBlocks' mixin adds to vanilla's `BlockStateBase` (38K). By breadth (mods hitting
    it): `ItemProperties.register` 7, `LootItemConditionalFunction.commonFields` 5,
    `GlobalPos.STREAM_CODEC` 4, `Minecraft.getResourceManager` 4, then at 3: NeoForge's 7-field
    `composite`, `FMLConfig.defaultConfigPath`, `LevelResource`, `Ingredient.CONTENTS_STREAM_CODEC`,
    `FeatureFlags.VANILLA_SET`, `BlockTags`/`ItemTags.create`, `GlobalPos.CODEC`,
    `ModelLayerLocation`, `SimpleCriterionTrigger`'s constructor, `Path.resolve`,
    `Class.getClassLoader`. Mapped now:
    - NeoForge's stream codecs: `enumCodec` (the ordinal as a var int; an ordinal past the
      constants throws `DecoderException` as NeoForge's does), `lazy` (the supplier asked once,
      on first use), `registryKey`, `uncheckedUnit`, `connectionAware` (the codec itself: both
      ends are the same runtime), `UNBOUNDED_BYTE_ARRAY`. Test
      `network::tests::neoforges_codecs_write_what_neoforge_writes`.
    - A mod class's `super()` into an API class's no-argument constructor that the runtime has
      nothing to set up for (`SimpleCriterionTrigger`...) is done, not a miss; the object's later
      API calls still count if unmapped.
    - `DeferredRegister.getNamespace`, `addAlias` (kept as `alias/<registry>/<from>`),
      `FMLConfig.defaultConfigPath` (`defaultconfigs`, FML's default).
    - Vanilla's plain data classes, read from their bytecode (`tools/vanilla_records.py`, now also
      writing `DATA_CLASSES` and `ENUMS` beside `RECORDS`): 682 classes extending `Object` whose
      constructor only stores its arguments in fields, with the getters and setters that only read
      or write one (`Tuple`, `LevelResource`, `ModelLayerLocation`...). Their `equals`/`hashCode`
      are `Object`'s when not overridden, field by field (in the constructor's order) when both
      overrides read exactly the fields one constructor sets (31 classes), else unanswered. The 51
      classes the runtime models by hand are left out (a test checks the table never shadows one).
      An enum constant's `equals`/`hashCode` are `Enum`'s: identity (FarmersDelight's 36
      `Direction.Axis.equals`). Tests `records::tests::a_plain_vanilla_data_class_keeps_what_it_was_given`,
      `a_data_class_that_compares_its_fields_is_equal_to_its_twin`,
      `enum_constants_are_equal_only_to_themselves`,
      `the_data_class_table_leaves_out_what_the_runtime_models_by_hand`.
    - Vanilla enums' constants from their classes (`ENUM_CONSTANTS`, the fields flagged as enum
      constants in declaration order): 365 enums the runtime doesn't model by hand get one object
      per constant with its name and ordinal (`NoteBlockInstrument.BASS`, `EquipmentSlotGroup.MAINHAND`),
      `values()` (a new array each call) and `valueOf` (or `IllegalArgumentException` with Java's
      text). The 66 enums modelled by hand (directions are natives) keep their own. Test
      `records::tests::a_vanilla_enums_constants_come_from_its_class`.
    - Static factories that only call a constructor with their own arguments, read from the
      bytecode too (36 classes: `GlobalPos.of`...), make the object as that constructor does. The
      table generator now reads the runtime's code without its tests (a test naming a class made it
      look modelled by hand).
    - `GlobalPos.CODEC` (`{dimension, pos}`) and `STREAM_CODEC` (the dimension's location, then the
      position as a long), built from a record constructor and component accessors as function
      objects (`compat:fn/record`, `compat:fn/component`). Tests
      `codecs::tests::a_global_position_reads_and_writes_as_vanillas_codec`,
      `network::tests::a_global_position_travels_as_vanillas_does` (bytes written out by hand).
    - New module `attributes.rs`: vanilla's 31 `Attributes` holders under the ids 1.21.1's
      initialiser registers (`minecraft:generic.attack_damage`), the 42 `Enchantments` keys,
      `Item.BASE_ATTACK_DAMAGE_ID`/`BASE_ATTACK_SPEED_ID`, `ItemAttributeModifiers.builder()`,
      `add`, `build`, `withModifierAdded`, `EMPTY` (vanilla's record; the main hand's attack damage
      and speed kept beside it as `SwordItem.createAttributes` keeps them). Tests
      `attributes::tests::a_weapons_modifiers_read_back_as_built`,
      `enchantment_constants_are_the_keys_of_vanillas_enchantments`.
    - NeoForge's public `BlockTags.create`/`ItemTags.create` (FarmersDelight's 48 calls),
      `FMLEnvironment.production` (FramedBlocks' 527: a player's game is FML's production),
      `FeatureFlags.VANILLA_SET`/`DEFAULT_FLAGS`.
    - Loot codecs' shared starts: `LootItemConditionalFunction.commonFields(instance)` (5 of the
      28 mods) is the group of an optional `conditions` list (empty by default), NeoForge's
      `LootModifier.codecStart` a required one. Loot conditions themselves aren't modelled: a list
      of them fails to read. Test `codecs::tests::a_loot_functions_shared_fields_read_as_vanillas`.
  - [x] 2026-11-05 The pack's 28 mods, round 6 (snapshot at e88c88b): all 28 load to the end;
    distinct misses 239 → 207, kinds summed per mod 454 → 344, unmapped calls 158K → 44K
    (`Tuple`'s getters are a vanilla data class now). Widest: `MapCodec.forGetter` 8 and
    `Codec.fieldOf` 5 (each left by a miss before it: a mod's own `Codec` class,
    `Codec.stringResolver`, `xmap` on nothing), `ItemProperties.register` 7,
    `Minecraft.getResourceManager` 4, `Ingredient.CONTENTS_STREAM_CODEC` 3, `Class.getClassLoader` 3.
  - [x] 2026-11-05 Mixins into the game's classes (`weave::Plan::vanilla`; the dispatchers in
    `codegen.rs`). Mixin merges such a mixin into a class the translator leaves to the runtime, so
    what it adds now joins the game's objects. A call through the mixin, through an interface it
    adds (a duck interface: FramedBlocks' `IStateCacheAccessor.framedblocks$initCache`, called on
    every block state) or through its target runs the mixin's method on a game object. A call
    through the game's own interfaces, or through the target for an `@Overwrite`, runs it only on
    an object of the target. Its `@Shadow`s are the game's: calls become API calls on the target,
    and a `@Shadow` static is the target's static. Its own fields are stored beside the game
    object. Its field initialisers run once on each object before the first method call (Mixin
    runs them in the target's constructors), and its static initialiser before the first call
    too. The target is an instance of the interfaces the mixin adds (`instanceof`). The runtime
    answers `BlockStateBase.asState()` (the state itself). Class files' field annotations are read
    now. Injectors into the game's own code are still not woven: they need hooks in the game.
    Test: the weave fixture's `Vec3Mixin`, a mixin into the game's `Vec3`. Each object gets its own
    counter (initialised to 10); `lengthSqr()` and `x` come through `@Shadow`s; each object keeps
    its own label next to the mixin's static; `instanceof Measured` holds.
    Measured on FramedBlocks alone: its 38,128 `framedblocks$initCache` misses are gone, and
    nothing else changed (the same 26 other misses, the same one uncaught exception).
  - [x] 2026-11-05 Registration through a mod's own classes, and NeoForge's shorthands.
    - A mod's register class (Mekanism's `MekanismDeferredRegister`, made with
      `super(registryKey, modid)`) keeps its registry and namespace: NeoForge's constructors of
      `DeferredRegister` and of its `Blocks`, `Items` and `DataComponents` set what `create` sets.
      Before this, the holders' constructor arm read the registry's key as an entry's.
    - `register` makes each holder through the register's `createHolder`, as NeoForge does. A mod
      class overriding it gets its own holders (Mekanism's `TileEntityTypeRegistryObject`, whose
      `capabilities`, `tickers`, `getName` were 154/154/87 misses on the runtime's holder).
      `getEntries` lists them; `getRegistryKey`/`getRegistryName` answer.
      `register(name, Function)` gets the entry's location, as NeoForge's supplier passes it.
    - `registerSimpleItem`, `registerSimpleBlock`, all four `registerSimpleBlockItem`s (the
      block's holder names the item), and NeoForge's default properties where none are given
      (`registerBlock(name, func)` had passed null). SecurityCraft and elevatorid registered
      nothing through them.
    - A vanilla enum's `ordinal()`, `name()` and `compareTo` named through its own class
      (`WallSide.ordinal()`, the read in every `switch` on a vanilla enum) are `Enum`'s.
    Tests `api::tests::a_mods_own_register_makes_its_own_holders`,
    `neoforges_simple_registrations_make_what_neoforge_makes`,
    `records::tests::a_vanilla_enums_own_class_answers_enums_final_methods`.
  - [x] 2026-11-05 Smaller general misses from round 6:
    - `Stream.reduce` in its three forms (`reduce(op)` an `Optional`, an `OptionalInt` for an
      `IntStream`; `reduce(identity, op)`; `reduce(identity, accumulator, combiner)`), left to
      right (SecurityCraft, FarmersDelight; Mekanism's `IntStream.reduce`).
    - Guava's other collection factories: `Sets.newLinkedHashSet` (FarmersDelight's 184 `add`s
      went to nothing), `newTreeSet`, the `WithExpectedSize`/`WithCapacity` ones (empty, where the
      list factory had taken the size for an element), `Maps.newHashMap(map)` a copy (it was
      empty).
    - `SignBlock.getWoodType(block)` (a sign keeps the wood it was made with, vanilla's `type`;
      other blocks oak) and `sign.type()`; `BedBlock.getConnectedDirection(state)` (Comforts' 512).
    Tests `jutil::tests::streams_reduce_and_guavas_factories_make_their_collections`,
    `blocks::tests::signs_keep_their_wood_and_beds_find_their_other_half`.
  - [x] 2026-11-05 The pack's 28 mods, round 7 (snapshot at 0c931d6, before the round-6 small
    fixes): all 28 load to the end; distinct misses 207 → 190, kinds summed per mod 344 → 337,
    unmapped calls 43,758 → 5,740. FramedBlocks 38,202 → 61 (mixins into the game's classes).
    Mekanism's `capabilities`/`tickers` (154 each), `getName` (87), `getItemHolder`,
    `addAttachedContainerCapabilities` and `getEntries` are gone (its own holders). It now gets
    further into its blocks: 846 → 994 calls. A run with `HORIZON_COMPAT_BACKTRACE` named the new
    ones: `BlockStateHelper.getDefaultState` asks `state.getBlockHolder().value()` (170 each), and
    `applyLightLevelAdjustments` reads the properties' `lightEmission` field (an access
    transformer opens it), which no one wrote (182 `applyAsInt` on null). `BlockBase.getShape`
    calls `super.getShape` (46). SecurityCraft 100 → 91, elevatorid 32 → 16 (the simple
    registrations).
  - [x] 2026-11-05 A game class's fields read by a mod itself (`codegen::game_field_read`,
    `horizon_compat_core::api::game_field`). Reading a field that a game class declares (no
    translated class on the way up does) looks first for what was written under its name, then
    for the state the runtime keeps for it (`$name`, as a mixin's accessor reads it), then for the
    runtime's answer to `Owner.$name.()T`. Such a read went to nothing before, and nothing counted
    it; now it is a miss by that key when nothing answers. The runtime answers
    `BlockBehaviour$Properties.$lightEmission`: what `lightLevel` set, else `Properties.of()`'s
    `state -> 0` (a `compat:fn/constant`; function objects now also answer `applyAsInt`, `getAsInt`
    and the other primitive forms). `BlockState.getBlockHolder()` is the block's registry entry
    (a block not registered yet learns its name when it is). `super.getShape(state, level, pos,
    context)` to a block class with vanilla's default is a full block. Those classes are the 99
    whose 1.21.1 class files override `getShape` nowhere above them, read from NeoForge's merged
    1.21.1 jar. `SlabBlock` and the other classes with shapes of their own stay misses.
    Test `blocks::tests::a_mod_blocks_reads_of_vanillas_answer_as_vanilla`.
  - [x] 2026-11-05 Codec factories that round 6 left as the roots of codec misses:
    - `Codec.stringResolver(toString, fromString)` (Mekanism's `FrequencyType`, StorageDrawers'
      `KeyType`): the string codec mapped by the mod's functions. A name they answer null for is
      an error both ways, as DFU's.
    - `ExtraCodecs.optionalEmptyMap(codec)`: `{}` is an empty Optional, written back as `{}`.
    - `Codec.checkRange(min, max)`: a function answering a `DataResult`, an error with the value
      as its partial outside the range. Whole numbers compare exactly (`Long.MAX_VALUE`).
    - The static `Codec.optionalField(name, codec, lenient)`, as `optionalFieldOf`.
    - NeoForge's `aliasedFieldOf(codec, names...)` (the first name present, written under the
      first) and `xor(first, second)` of map codecs (exactly one reads; an `Either`).
    Tests `codecs::tests::a_string_resolver_reads_only_the_names_its_functions_know`,
    `an_empty_map_is_an_empty_optional_and_a_checked_range_refuses_outside`,
    `optional_aliased_and_exclusive_fields_read_as_neoforge_builds_them`.
  - [x] 2026-11-05 Small ones from round 7: NeoForge's `DeferredSpawnEggItem(type, background,
    highlight, properties)` is made as the game's items are, keeping its type supplier and colours
    under `SpawnEggItem`'s names. `Integer.TYPE` and the other primitives' classes (`int.class`)
    and `StandardCharsets`' charsets are answered.
  - [x] 2026-11-05 Codec classes of a mod's own, and `DynamicOps` over NBT (`ops.rs`). A mod's
    class that is a codec (SophisticatedCore's `SetCodec`, Mekanism's `SequencedCollectionCodec`
    and `SerializerHelper`'s anonymous ones, SecurityCraft's `NullableListCodec`, Curios'
    modifier codec) gets DFU's combinators: `fieldOf`, `listOf`, `xmap`, `validate`,
    `optionalFieldOf`, `orElse`, `parse`, `encodeStart` and the rest wrap it
    (`compat:codec/java`, `java_map` for a map codec). Reading runs its own
    `decode(NbtOps, tag)` (a map codec's `decode(NbtOps, MapLike)`); writing runs its
    `encode(value, NbtOps, EndTag)` (a map codec's into a `RecordBuilder`). `NbtOps` is modelled
    over the runtime's tags as Minecraft's is: `empty` (`EndTag`), the `create*`, `get*Value`,
    `getList` (a consumer of consumers), `getStream`, `createList` (bytes, ints or longs alone an
    array tag), `getMap` (a `MapLike`: `get`, `entries`), `getMapValues`/`getMapEntries`,
    `createMap`, `mergeToList`/`mergeToMap`/`mergeToPrimitive`, `get`/`set`/`remove`,
    `listBuilder` and `mapBuilder` (their `add` forms, with an encoder too, `withErrorsFrom`,
    `build`). The runtime's own map codecs read a `MapLike` and write into a `RecordBuilder`, so a
    mod's codec can hold them. `Lifecycle`'s constants answer. DFU's abstract classes'
    constructors (`MapCodec()`, `MapDecoder.Implementation()`) are plain supers, and a criterion
    trigger made by the mod itself (`new PlayerTrigger()`) is a plain object.
    Tests `ops::tests::a_mods_own_codec_reads_and_writes_through_the_combinators`,
    `nbt_ops_build_and_read_tags_as_minecrafts`.
  - [x] 2026-11-05 Load round 8 (the 28 jars, snapshot `38040f6`): 28 load, exit 0. 180 distinct
    misses (190), 312 kinds (337); calls 7,940 (5,740). The calls grew because reads of game
    fields now count: `Block.stateDefinition` read in mods' constructors (`this.stateDefinition
    .any()`: 2,731 in Macaw's furniture and roofs, Create Furnitures, Easy Villagers and others)
    and `Properties.destroyTime` (644 in SecurityCraft) were silent nulls before. Down: Comforts
    566 → 87, FarmersDelight 269 → 66, Mekanism 994 → 513 (kinds 129 → 103).
  - [x] 2026-11-05 What round 8 showed:
    - The field `stateDefinition` of a block is the object `getStateDefinition()` answers.
    - `BlockBehaviour.Properties`' fields read directly (`destroyTime`, `explosionResistance`,
      `requiresCorrectToolForDrops`, `friction`, `speedFactor`, `jumpFactor`, `hasCollision`,
      `canOcclude`, `isRandomlyTicking`, `replaceable`, `soundType`, `pushReaction`,
      `lightEmission` and the flags): what the builder calls stored, else what the vanilla block
      a copy came from has, else `Properties.of()`'s value.
    - `BlockEntityType(supplier, blocks, type)` built by a mod (Refined Storage) keeps its factory;
      `DataComponentType.Builder()` (SophisticatedCore) is a plain constructor;
      `Util.fetchChoiceType` answers null (no datafixer types), `References`' type names answer.
    - `CropBlock.getAgeProperty()`/`getMaxAge()` reached from a mod crop: `AGE_7` and 7.
    - fastutil's `Int2ObjectMaps.fastIterator(map)`, `fastIterable` and `fastForEach` (any
      `X2YMaps`) walk the map's typed entry set; `Collections.singletonMap`.
    Tests `blocks::tests::a_mod_blocks_reads_of_vanillas_answer_as_vanilla`,
    `jutil::tests::fastutil_int_maps_and_sets_walk_as_in_java`.
  - [x] 2026-11-05 Stream codecs of the game's that aren't modelled (`DyeColor.STREAM_CODEC`,
    `CustomData.STREAM_CODEC`, `ItemEnchantments.STREAM_CODEC`...): the class's codec over the wire
    (`OPTIONAL_STREAM_CODEC` takes `OPTIONAL_CODEC`, else `CODEC`), as
    `ByteBufCodecs.fromCodecWithRegistries` sends it. Both ends of Horizon run the same runtime,
    so they read it alike; it is not vanilla's byte layout, which only a vanilla peer would need
    (and it can't run the mod anyway). A class with no modelled codec either stays a miss under
    the stream codec's name. `ByteBufCodecs.VECTOR3F` and `QUATERNIONF` write their floats.
    Small ones: `MapColor.MATERIAL_COLORS` (each colour at its id), `FastColor.as8BitChannel`,
    `TranslatableContents.isAllowedPrimitiveArgument`, NeoForge's
    `ModelResourceLocation.standalone`, `BuiltInLootTables.register`, `LazyLoadedValue`,
    `RegisterCapabilitiesEvent.setProxyable` (queries always reach the block's own provider).
    Tests `network::tests::a_classs_stream_codec_falls_back_to_its_codec`,
    `blocks::tests::properties_keep_what_a_declaration_needs`.
  - [x] 2026-11-05 FML's scan data (`ModList.getAllScanData()`), which 16 of the pack's 367 mods
    read: JEI and Jade find their plugins with it (`@JeiPlugin`, `@WailaPlugin`), Framework and
    SuperMartijn642's core library what they register, Mekanism the fields its menus sync,
    JourneyMap, Iron's Spellbooks and others their own annotations. The translator writes, per
    jar, a line per annotation on a class, field or method (type, `TYPE`/`FIELD`/`METHOD`, class,
    member as FML names it, values as JSON with their types: string, enum constant, class,
    primitive, array, annotation); annotations only compilers and tools read (nullability,
    Kotlin's metadata, mixins) are left out. The engine registers the text
    (`Rt::scan_data`); `getAnnotations()` reads it the first time it is asked. Answered:
    `ModFileScanData.getAnnotations`, `getAnnotatedBy(type, elementType)`, `getIModInfoData`
    (the jar's mods); `AnnotationData`'s accessors; values as FML holds them (`EnumHolder`,
    ASM `Type`, boxes, lists, `ModAnnotation`); ASM's `Type` (`getType`, `getObjectType`,
    `getClassName`, `getInternalName`, `getDescriptor`, `equals`); `ElementType`.
    `Class.asSubclass` (JEI checks its plugins so). Nobody in the pack asks for
    `getClasses()`, so the class lines aren't written (it stays a miss).
    Tests `codegen::scan_tests::the_scan_keeps_what_fml_would_find` (javac),
    `scan::tests::a_scan_lists_annotations_as_fml_finds_them`.
    The runtime a bundle carries (built without `--runtime`, as the game's are) missed `ops.rs`
    since the `DynamicOps` step; `bundle::tests::bundles_carry_every_runtime_source_file`
    caught it, and it carries `ops.rs` and `scan.rs` now.
  - [x] 2026-11-05 fastutil's custom-hash collections (`ObjectOpenCustomHashSet(strategy)`, the
    linked ones and maps): keys hash and compare by the strategy's `hashCode`/`equals` (`Table`'s
    `strategy`). Vanilla's `ItemStackLinkedSet.TYPE_AND_TAG` (`TYPE_AND_COMPONENTS`) and NeoForge's
    `FluidStackLinkedSet.TYPE_AND_COMPONENTS` are answered natively (same item or fluid, same
    components), with `createTypeAndComponentsSet()` and the `hashStackAndComponents`/
    `hashItemAndComponents`/`hashFluidAndComponents` helpers; a mod's own strategy runs as
    translated code. `clear()` keeps how a collection orders and compares its keys: a tree's
    comparator was dropped by a set's `clear()`, and now the strategy is kept too.
    `Class.getClassLoader()` answers one loader object (`ServiceLoader.load` ignores it).
    Tests `api_common::tests::a_custom_hash_set_keeps_stacks_by_their_type`,
    `jutil::tests::tree_map_sorts_keys_and_sets_ignore_duplicates`.
  - [x] 2026-11-05 Load round 9 (the 28 jars, snapshot `945def5`): 28 load, exit 0. 150 distinct
    misses (180), 256 kinds (312), calls 6,967 (7,940). Mekanism registers 200 blocks, as many as
    its code declares; rounds 7 and 8 counted 201, one registration with no block behind it. Every
    block it registers has its blockstate file (`advanced_bounding_block`'s is a leftover no code
    registers; `LT_NAMES=block` makes the load harness list the names). New in Mekanism now that
    its scan data answers: its container sync reads its `@ContainerSync` fields by reflection
    (`getDeclaredField`, 70, then `MethodHandles.Lookup.unreflectGetter` and `LambdaMetafactory`).
  - [x] 2026-11-05 What round 9 showed:
    - `MapColor.col` (2,928 reads in one mod): each constant has the colour vanilla builds it with.
    - A mod's own `PrimitiveCodec` (Curios' attribute codec, an anonymous class with `read` and
      `write`): it is a `Codec`, `Decoder` and `Encoder` to `instanceof`, and its field codecs read
      and write through its `read` and `write`, as DFU's `decode` and `encode` do.
    Tests `blocks::tests::properties_keep_what_a_declaration_needs`,
    `ops::tests::a_mods_primitive_codec_reads_and_writes_through_read_and_write`.
  - [x] 2026-11-05 The game's build of the Create + ten addons bundle failed in 3 minutes: a bundle
    built without `--runtime` (as the game's are) carries the runtime inlined, without the core's
    entry points, and `api::game_field` (round 8's game field reads) called the core crate's. It
    reads the field itself now, and `bundle::tests::bundles_carry_every_runtime_source_file` checks
    that no carried source names the core crate outside its tests.
  - [x] 2026-11-05 Item model overrides (1.21.1), step 1 of 3: horizon-assets reads a translated
    mod's `overrides` as `horizon:overrides`, Horizon's own item model type: the last override whose
    predicates all hold, each property asked once, one nothing answers -inf
    (`ItemOverrides.resolve`). Vanilla's generic properties come from the stack and its holder
    (`damage` and `damaged` on a stack that can break, `lefthanded`, `cooldown`,
    `custom_model_data` as 26.3's first float, `trim_type` by 1.21.1's material indices); the rest
    from the mod (`ItemEnv::item_property`). Next: the runtime keeps `ItemProperties.register` (7 of
    the 28 mods) and the game asks it through a client hook; the translator writes the
    definitions. Test `item_model::tests::translated_overrides_pick_as_item_overrides_resolve`.
  - [x] 2026-11-05 Item model overrides, steps 2 and 3. The runtime keeps `ItemProperties.register`
    and `registerGeneric` (by item and property, a generic one first, as `getProperty` looks) and
    calls a function as `ItemOverrides.resolve` does: a `ClampedItemPropertyFunction`'s
    `unclampedCall` held to 0..1 (`Mth.clamp`, NaN kept), a plain one as it answers. A new client
    hook, `native_item_property(stack, property, owner, seed, value)` (fragment
    `item_property.json`), asks the translated mods for each property vanilla's generic ones
    don't answer; the glue gives the function the client's level and the local player when they
    hold the stack, and the stack they use is their hand's own object, so a bow's
    `entity.getUseItem() == stack` holds (its use ticks and hand set on the player). The
    translator writes `horizon:overrides` for each item model with `overrides` of its own (bare
    model locations are vanilla's); the rest keep the plain definition. Create has five: the
    clipboard (`create:clipboard_type`, the ordinal of its content's type) and its armour
    (`trim_type`). Tests `glue::tests::item_properties_the_mods_register_answer_for_the_stack_in_use`
    (vanilla's bow `pull`: drawn half a second, held to 1 past a second, 0 not in use or not held),
    `bundle::tests::item_models_with_overrides_get_an_overrides_definition`. In game (Create and its
    ten addons as one engine, `scratchpad/clipgame.sh`): a clipboard in the hotbar and in hand
    draws `create:item/empty_clipboard` (its base model has no texture of its own, so a property
    left unknown would draw the missing texture). The build (177 MB engine) needed the watchdog's
    limits raised: macOS compresses rustc's pages and then moves the compressor to swap in
    seconds, so swap rose 15 GB above its base and fell back (peak footprint ~44 GB); the
    watchdog now reads `SWAP_LIMIT` and `FREE_MIN`, and the build ran with 6 jobs.
  - [x] 2026-11-05 The engine split into crates (`split.rs`). One crate can't hold more addons:
    rustc's footprint grows with it (~44 GB for 177 MB). horizon-mixins now lets a mod's bundle
    crates depend on each other, `<package> = { path = "../<crate id>" }` (the stage puts each at
    `native-crates/<mod>/<crate id>`; the plan checks the id names another of the mod's crates, the
    name is its package's, and there is no cycle; scratchpad clone, branch `sibling-crates`,
    eb84389, test `a_bundle_crate_depends_on_another_crate_of_its_mod`). A bundle
    (`bundle::write_group`, as the port pack and the game's builds write it) is now `runtime/`
    (horizon-compat's sources as a crate), `code-<mod>-<n>/` (each mod's classes, a mod at a time,
    in crates of about 8 MB; `HORIZON_TRANSLATOR_CRATE_MB`; classes no member jar holds go to
    `code-lib-*`) and `engine/` (the tables, dispatchers, `init` and what the hooks call, over them
    all). A code crate calls a function of another crate, a dispatcher, `vcall_miss` or
    `java_string` through a shim of its own (`links.rs`): the engine's `init` links each crate by
    name first (`link_crates`, once per process), so a crate's text depends only on its own code,
    and calls inside a crate stay direct. A tools bundle (`--runtime`) stays one crate. Create + ten
    addons: 29 code crates of 0.5-8.5 MB, a 17 MB engine crate, 3,558 linked functions, 8,188
    dispatchers. The game's build (6 jobs) took 6 min 53 s instead of 11 min 32 s; the
    translated crates' rustcs peaked at 15.9 GB RSS together (each code crate 0.3-1.8 GB, the
    engine crate ~9 GB) and swap grew 5.2 GB instead of 15. In game (`addonsgame.sh`,
    `clipgame.sh`): the creative tabs, the addons' blocks and the clipboard's icon as before; of the
    blocks placed, 56 of 62 read back after a wait (the script's first read, right after
    `setblock`, often sees air: the client hasn't heard yet), the other 6 Dragons Plus's dye
    fluids, for compat mods the pack lacks. Test
    `split::tests::calls_between_crates_go_through_links_and_calls_within_stay_direct`. The
    launcher builds with horizon-mixins at `MIXINS_REV` from horizon-rust/horizon-mixins, so a
    split bundle needs the sibling change upstream and the pin moved.
  - [x] 2026-11-05 Enchantments (`horizon-compat/src/enchantments.rs`). Create Enchantable
    Machinery's machines read `ItemEnchantments.EMPTY` in their block entities' constructors, and it
    was null: 32,490 `NullPointerException`s (`EMPTY must not be null`) in one load round, and none
    of its machines got a block entity. `ItemEnchantments` is native now: one object per content
    (levels by name, sorted; equal ones are the same object, as `equals` has them), `EMPTY`,
    `getLevel`, `keySet`, `entrySet` (fastutil's entries), `Mutable` (`set`, `upgrade`, `removeIf`,
    `toImmutable`), `CODEC` (`{levels, show_in_tooltip}` or the levels alone, 1.21.1's
    `withAlternative`), the stack's (`enchant`, `isEnchanted`, `getEnchantmentLevel`,
    `getTagEnchantments`), `EnchantmentHelper`'s (`getEnchantmentsForCrafting`,
    `set/updateEnchantments`, `getItemEnchantmentLevel`...) and `Enchantment.getFullname` (1.21.1's
    descriptions and maximum levels, read from its data: the level unless the enchantment has one,
    red for a curse). Every item carries `enchantments`, `EMPTY` by default (1.21.1's common
    components), an enchanted book `stored_enchantments`. Holders: `HolderLookup.getOrThrow(key)`
    (34 calls in the eleven mods) missed, and `HolderLookup$RegistryLookup`'s and `HolderGetter`'s
    gave the value; they give the entry's holder now, `get(key)` it in an optional, and a
    registry's holders are one per entry (vanilla's compare by identity). Tests
    `enchantments::tests` (3). Next: the game's stacks carry their enchantments to the runtime and
    back.
  - [x] 2026-11-05 The game's stacks carry their enchantments to the runtime and back. A stack the
    runtime reads (a hand, a slot, `stack_of`) takes the game's own levels as its
    `minecraft:enchantments` and `minecraft:stored_enchantments` components
    (`ItemData::enchantments_tag`, the game's `native_vanilla_components`), and what a mod sets on
    them (`ItemStack.enchant`, `EnchantmentHelper.setEnchantments`) goes back to the game's levels
    (`native_set_vanilla_component`, `ItemData::set_enchantments_tag`), not to a mod component. A
    stack the runtime saves or loads (a mod's inventory) keeps them by the component's codec. In game
    (Create and ten addons, `cemgame.sh`): Create Enchantable Machinery's drill and millstone get their
    block entities (no `EMPTY must not be null`, which was 32,490 in round 9); a drill placed with
    `{Enchantments:{levels:{"minecraft:efficiency":5,"minecraft:fortune":3}}}` reads back the same,
    a millstone placed with the levels alone (`{Enchantments:{"minecraft:efficiency":2}}`) reads back
    as `{levels:{...}}`, and a plain drill saves none. No exception or panic in the log. Tests
    `item_data::tests::enchantments_cross_as_their_component_and_back`,
    `hand::tests::a_stack_in_hand_carries_its_enchantments_both_ways`. CEM's own mechanic (a machine
    item takes its enchantments into the placed block) needs its mixins into `BlockItem` and `Item`
    (`getPlacementState`, `isEnchantable`), the vanilla hook points still to come.
  - [x] 2026-11-05 Small ones from round 9. Vanilla's armour materials answer their values
    (`ArmorMaterial.defense()` by `ArmorItem.Type`, `enchantmentValue`, `toughness`,
    `knockbackResistance`, `equipSound`; 1.21.1's `ArmorMaterials`): Mekanism copies netherite's for
    its own armour. `new KeyMapping(...)` in each of its forms (vanilla's, NeoForge's with a conflict
    context, a modifier or an `InputConstants.Key`) keeps its name, its default key and its category:
    a mod's own mapping, which the game's controls don't list, answers `isDown`, `getKey` and
    `isUnbound` on that key as the game holds it (Create's tool menu on Left Alt). Tests
    `blocks::tests::a_vanilla_armour_material_answers_its_values`,
    `screens::tests::a_mods_own_key_mapping_answers_on_the_key_it_was_made_with`.
  - [x] 2026-11-05 Load round 10 (the 28 jars, snapshot `bb82114`): 28 load, exit 0. Kinds summed
    per mod 256 → 238, distinct 237 → 227 (counted by key), calls 6,967 → 3,970: `MapColor.col`'s
    2,928, `ItemProperties.register` and the armour materials' reads are gone. Uncaught: FramedBlocks'
    `Invalid shape` (1) and Mekanism's `Current server is null` (2), as before. Widest now:
    resources (`Minecraft.getResourceManager` in 4 mods: GeckoLib and Immersive Aircraft register
    reload listeners, StorageDrawers reads 40 block models with `BlockModel.fromStream` and their
    elements 1,568 times, Mekanism reads 5 textures with `NativeImage.read` and their pixels), then
    codec roots (`MapCodec.forGetter` 3, `StreamCodec$CodecOperation.apply`, `Biome.LIST_CODEC`,
    `Codec.fieldOf`, `MapCodec.xmap`, `SimpleFluidContent.CODEC` 2 each). Results in
    `scratchpad/lt/r` (round 9's in `r9-all`).
  - [x] 2026-11-05 The game's resources, read as a mod reads them (`resources.rs`). Of 367 jars in
    the pack, 57 call `getResourceManager`, 48 `registerReloadListener`, 45 `NativeImage.read`,
    39 `listResources`, 34 handle `RegisterClientReloadListenersEvent` (Gson: 122 use `JsonObject`,
    103 `Gson`, 47 `GsonHelper`, 43 `JsonParser`: next). `Minecraft.getResourceManager()` is the
    client's manager (`ReloadableResourceManager`), `MinecraftServer.getResourceManager()` the
    server's (`data/`): `getResource` (an `Optional`), `getResourceOrThrow` and `open` (a missing file
    throws `FileNotFoundException`), `openAsReader`, `getResourceStack`, `listResources` and
    `listResourceStacks` per namespace with the mod's filter (sorted by location, as vanilla's
    `TreeMap`), `getNamespaces`. A `Resource` opens its file, or the supplier a mod made it with.
    `java.io`'s streams and readers over the bytes (`InputStream.read` in its forms, `readAllBytes`,
    `readNBytes`, `skip`, `available`; `InputStreamReader`, `BufferedReader.readLine` and `lines`,
    `StringReader`, `ByteArrayInputStream`), commons-io's `IOUtils.toString`/`toByteArray`/
    `readLines`. `BlockModel.fromStream`/`fromString` parse a model's own elements (`from`, `to`,
    faces by direction with `cullface`, `tintindex`, `texture` and `uv`, `rotation`, `shade`) and its
    parent's location (StorageDrawers' drawer geometry). `NativeImage.read` decodes a PNG with the
    game's decoder and answers `getWidth`, `getHeight`, `getPixelRGBA` (1.21.1's ABGR; out of bounds
    throws), `setPixelRGBA`, `getPixelsRGBA` (Mekanism's texture colours). Reload listeners: NeoForge's
    `RegisterClientReloadListenersEvent` is posted before the client's setup and the manager's own
    `registerReloadListener` adds to the same list; after the setup each listener reloads in order,
    once, with the game as host: its own `reload` (a lambda's: GeckoLib's), else
    `SimplePreparableReloadListener`'s `prepare` then `apply`, or `onResourceManagerReload`
    (Create's two). The files come from the host: `native_resource(path)`,
    `native_resource_paths(prefix)` and `native_resource_namespaces(root)` (fragment
    `native_resources`) read the game's resource root as it loaded it (packs, the mods' content and
    vanilla layered); without a host, from the folders in `HORIZON_COMPAT_RESOURCES`. `Resource`
    and `BlockFaceUV` left the data-class table (modelled here now; only those two lines, since the
    tool would also drop `GlobalPos.of`'s factory, which `codecs.rs` names). Tests
    `resources::tests::{a_mod_reads_the_games_resources_through_the_manager,
    readers_and_listing_read_as_javas_do, a_registered_reload_listener_prepares_then_applies}`;
    233 compat tests pass. Not yet in game (next build, with Gson).
  - [x] 2026-11-05 Gson's tree (`gson.rs`), from a census of the pack's method references (287
    distinct; `JsonElement.getAsJsonObject` in 92 jars, `GsonBuilder.create` 89, `JsonObject.get` 85,
    `new JsonObject()` 78, `getAsString` 78...). `JsonParser.parseString`/`parseReader`/`parse` read
    leniently as Gson does (comments, single quotes, unquoted names and words, `;`, `=`, a trailing
    comma as a null), an empty document is `JsonNull`, text after the value an error; the parser is
    the runtime's own, so objects keep their members' order (Gson's `LinkedTreeMap`) and a repeated
    name keeps the last value. Objects are maps and arrays lists of elements shared by reference (a
    member added through a nested object shows in its parent); a primitive keeps its kind and value
    (integers that fit a `long` as one, other numbers as doubles; `getAsInt` of a long wraps as
    `LazilyParsedNumber.intValue` does, a string parses as `Integer.parseInt` would, with its
    `NumberFormatException`). Every `JsonElement`, `JsonObject`, `JsonArray` and `JsonPrimitive` call
    of the census (`add`, `addProperty` of each type, `remove`, `has`, `entrySet`, `keySet`, `asMap`,
    `iterator`, `forEach`, `deepCopy`, `equals`/`hashCode` as Gson's, also as map keys; a one-element
    array answering for its element; Gson's `IllegalStateException`s and `ClassCastException`s).
    Writing is Gson's: `toString` compact with nulls; a `Gson` from `new Gson()` or a `GsonBuilder`
    escapes HTML (`\u003c`) unless `disableHtmlEscaping`, leaves null members out unless
    `serializeNulls`, and pretty prints with two spaces and `": "`; `toJson` into a `StringBuilder`;
    `fromJson(reader, string or tree, JsonObject.class...)` for the tree's classes with Gson's
    `Expected a com.google.gson.JsonObject but was ...` error. `GsonHelper`, 1.21.1's: the
    `is*Value`/`is*Node` checks, `getAsX`/`convertToX` for every type with and without a fallback and
    its messages word for word (`Missing count, expected to find a Int`, `Expected tags to be a
    string, was an array (["a"])`, `getType`'s `abbreviateMiddle`), `parse`, `parseArray`,
    `fromJson`, `toStableString`. Codecs read a tree (`codec.parse(JsonOps.INSTANCE, element)`).
    `FileToIdConverter` (`json`, `idToFile`, `fileToId`, `listMatchingResources` and its stack and
    namespace forms) and `SimpleJsonResourceReloadListener` (its `prepare` reads the folder's JSON
    files by id, skipping one that isn't JSON as vanilla logs and skips it; `scanDirectory`,
    `getPreparedPath`): Create's train hat info and the 26 jars' JSON listeners. `FileToIdConverter`
    left the data-class table. Not modelled: mapping JSON onto a mod's classes (`fromJson(reader,
    MyConfig.class)`, type adapters) and the streaming `JsonReader`/`JsonWriter`. Tests
    `gson::tests::{a_parsed_file_reads_as_gsons_tree, a_built_tree_is_written_as_gson_writes_it,
    gson_helper_reads_and_complains_as_vanillas, the_parser_is_lenient_as_gsons}`,
    `resources::tests::json_files_list_by_id_and_reach_a_json_listener`; 237 compat tests pass.
  - [x] 2026-11-05 The server's data reload listeners (45 of 367 jars use `AddReloadListenerEvent`). On
    `native_server_data_loaded` (a world's first open, each `/reload`) the engine's new
    `server_data_loaded` hook gathers the listeners anew as each `ReloadableServerResources` load does:
    the list is cleared, `AddReloadListenerEvent` posted (`addListener`, `getListeners`,
    `getServerResources`, `getRegistryAccess`), and each listener reloads from the `data/` manager in
    the order added (`resources::server_reload`, `glue::server_data_loaded`; bundle helper and binding
    `server_data_loaded`). The game's resources root (`options.resources`) is the layered one the mods'
    content joined (`mods::content_root`), so `data/create/...` reads as vanilla's. Not yet: the
    world's own data packs (`datapacks/`). After the listeners `TagsUpdatedEvent` is posted
    (`SERVER_DATA_LOAD`, as `updateRegistryTags` does; `getUpdateCause`, `shouldUpdateStaticData`,
    `UpdateCause`'s constants, `values`, `ordinal` for a switch), and after `/reload`
    `OnDatapackSyncEvent` to every player. `ReloadableServerResources.getRegistryLookup` answers.
    Also: with `HORIZON_COMPAT_TRACE` a mod's `info`/`warn`/`error` log lines print, `{}` filled as
    SLF4J and Log4j fill them (`jlang::mod_log`): what a mod reports (Create's `Loaded N train hat
    configurations.`) shows in the game's log. Test
    `resources::tests::data_listeners_gather_anew_on_each_server_load` (two loads run the listener
    twice, not three times, and it reads `data/`, not `assets/`).
    In game (the build before this step, `cemgame.sh`): the client's reload listeners run at the
    first client hook; Create's raise nothing, Flywheel's shader compiler throws
    `"flywheel:internal/components_header.frag" was not found` (its GL backend, which Horizon's
    renderer replaces; reported, the next listener goes on). CEM's machines read back as before.
  - [x] 2026-11-05 A server player's own events (49 of 367 jars listen for `PlayerLoggedInEvent`, 28
    for `PlayerLoggedOutEvent`, 14 for `PlayerRespawnEvent`, 15 for `Clone`, 15 for
    `OnDatapackSyncEvent`). The engine's `player_logged_in`, `player_logged_out` and
    `player_respawned` hooks (bundle helpers and bindings, on the server's game with its writes and
    payload sends) post them as a server does (`events::{logged_in, logged_out, respawned,
    datapack_sync}`): a join sends the server's data to the new player alone (`OnDatapackSyncEvent`,
    `PlayerList.placeNewPlayer`), then `PlayerLoggedInEvent`; a respawn after a death is
    `PlayerEvent.Clone` (`isWasDeath`, `getOriginal` the same player object, so what a mod keeps on
    it stays) then `PlayerRespawnEvent` (`isEndConquered` false); `/reload` syncs every player.
    `getEntity`/`getPlayer` give the player's one object (`menus::server_player`); the server's
    `PlayerList` (`MinecraftServer.getPlayerList`, `getPlayers`, `getPlayerCount`, `getPlayer(UUID)`)
    lists the host's players, and `getRelevantPlayers` streams them. Test
    `events::tests::a_players_join_respawn_and_leave_reach_listeners_in_a_servers_order`; 239
    compat tests pass.
  - [x] 2026-11-05 `java.util.regex`, a player's identity, `ChunkPos`, and the gaps the last game run
    listed. `jregex` (new module, on the `regex` crate Horizon's OptiFine reader already locks):
    `Pattern` (`compile` with `CASE_INSENSITIVE`, `COMMENTS`, `MULTILINE`, `LITERAL`, `DOTALL`,
    `matcher`, `matches`, `split` with Java's own rules for limits and leading empty strings,
    `splitAsStream`, `quote`, `flags`, `pattern`), `Matcher` (`matches`, `lookingAt`, `find` and
    `find(int)`, groups by number and name with UTF-16 offsets, `replaceAll`/`replaceFirst` with
    Java's `$n`/`${name}`/`\x` replacement and its errors, `appendReplacement`/`appendTail`, `reset`,
    `results`, `toMatchResult`, `quoteReplacement`, `hitEnd`) and `PatternSyntaxException`. Java's
    syntax is rewritten to the crate's: `\Q…\E`, `\w \d \s` as ASCII (Java's default), `\h \v \R
    \Z`, `\uXXXX`, octal, POSIX `\p{Alpha}`, atomic groups and possessive quantifiers as plain ones;
    back references and lookaround (which the crate lacks) throw `PatternSyntaxException`, so the
    mod's own handling runs. `String.split`, `replaceAll`, `replaceFirst` and `matches` take the same
    path (a literal pattern stays a plain replace). 64 of the 367 jars (with what they bundle) use
    `Pattern`/`Matcher`, 154 the `String` forms. A player's identity: `getUUID`, `getStringUUID`,
    `getGameProfile`, `getScoreboardName`, `getName`/`getDisplayName` (the name the game gave at
    login, passed through the player hooks: `native_player_logged_in(player, name)`), `getServer`;
    their place (`blockPosition`, `position`) from the host's players for the server's players in
    events (the player hooks' host lists every player; `server_data_loaded`'s too, for `/reload`'s
    sync). 59 jars read a player's profile. `ChunkPos` modelled by hand from vanilla's source (87
    jars): `asLong` both ways, `getX`/`getZ` of a long, the three constructors, `toLong`,
    `hashCode` (`hash`), `equals`, `toString`, block bounds (`getMiddleBlockX` is `+ 8`), regions,
    `getBlockAt`, distances, `ZERO`, `INVALID_CHUNK_POS`; it left the data-class table, and
    `tools/vanilla_records.py` now keeps every factory (one only calls its class's constructor
    through the runtime, so `GlobalPos.of` stays though its codec is modelled by hand).
    `ModConfig.Type.values()` and `CreativeModeTab.Type` as plain enums: catnip's enum codec reads
    the config type by its constants, so Copycats' config sync no longer reads `null` ("Received data
    for non-synchronized config, ignoring null"). Also: `EffectiveSide.get`, `LogicalSide.isClient`/
    `isServer`, `NeoForgeMod.enableMilkFluid`, a registry's `holderOwner`,
    `Holder.Reference.createStandAlone`, `java.lang.ref.Cleaner` (`register`, `clean` once),
    `FaceAttachedHorizontalDirectionalBlock.getConnectedDirection`, `CreativeModeTab.getType`,
    `BuildCreativeModeTabContentsEvent.getParentEntries`/`getSearchEntries`. Tests
    `jregex::tests::{split_is_javas, a_matcher_walks_and_replaces_as_javas,
    javas_syntax_compiles_or_throws_as_javas}`, `neo::tests::effective_side_and_stand_alone_holders`,
    `api_common::tests::{a_chunk_pos_packs_and_spans_as_vanillas, config_types_are_listed_as_flms}`,
    the player events test now reads each player's name, UUID text and block; 245 compat tests and
    13 translator tests pass.
  - [x] 2026-11-05 In game (the build of the step before, `cemgame.sh`): the regex, side, holder,
    `ChunkPos`, config-type and player-identity calls are gone (54 kinds of unmapped call to 32),
    Copycats' config sync no longer reads `null`, CEM's machines read back as before. The next
    batch, from that run's list: `ItemStack.getComponentsPatch` (the stack's set components as
    `ItemStack.CODEC` writes them; `EMPTY` when none), `Registry.holderOwner` on a registry as the
    game's statics give it (its path), a level's `players()` (a server's in that dimension, a
    client's own) and its public `random` field, `Minecraft.getLanguageManager` (`getSelected`,
    `getJavaLocale`), `hasSingleplayerServer` (false: the client's runtime holds no server object,
    so a mod syncs over the network as to a remote server), the accessors `Minecraft.screen` and
    `particleEngine` (its `providers` map), `ChunkRenderTypeSet.of`/`none`/`all` holding their
    types, `GL.getCapabilities` (nothing reads as supported: Horizon doesn't draw with GL),
    `PanoramaRenderer` (Create's and catnip's menus: kept, drawn by nothing), a library interface's
    default `forEach` on a mod's own iterable (electroenergetics' `LongIterable`: its `iterator()`
    walked with its own `hasNext`/`nextLong`), `CriterionTrigger.createCriterion`, and inert
    advancement and item-predicate builders (advancements built in code are for data generators;
    the game reads the shipped files). Left: `super.getShape` of a vanilla class (Create
    Connected's lever, Copycats' ladder and door, read at registration: needs the game's shape of
    an equivalent vanilla state), `BlockState.getProperties`, `LocalPlayer.getLookAngle` before the
    world. Tests: `jlang::tests::a_mods_iterable_walks_its_own_iterator_for_for_each`, the saved
    stack test reads its patch, the player events test reads `ServerLevel.players`; 246 compat
    tests pass.
  - [x] 2026-11-05 Load round 11 (05a5b75's runtime, the 28-jar set): all 28 done, blocks,
    items and declarations as round 10 (3651 / 2893 / 3647), kinds of unmapped call 238 to 225
    (StorageDrawers 11 to 4, immersive_aircraft 10 to 6; Jade now runs further, to 2). Vanilla
    kinds as templates: a mod block whose first vanilla class is a fence, fence gate, wall,
    button, lever, pressure plate, weighted plate, chain, lantern, torch, wall torch, stained pane,
    cake, farmland or flower copies the vanilla block of that kind (`oak_fence`,
    `cobblestone_wall`, `iron_chain` as 26.3 names the chain...), so the engine gives it that
    block's states, shapes and behaviour (a fence joins fences and gates by the class chain and
    tags, a button springs back) where it was a full cube of glass or stone. Counted in the pack's
    367 jars: 9 extend `FenceBlock`, 9 `FenceGateBlock`, 8 `WallBlock`, 6 `ButtonBlock`, 6
    `PressurePlateBlock`, 3 `LeverBlock`, 3 `CakeBlock`, 4 `FarmBlock`, 4 `FlowerBlock`, 2 each
    of chains and torches. Where vanilla's behaviour follows what the constructor was given, so
    does the template: the `BlockSetType` (kept as vanilla's field `type`) of a door or trapdoor
    that doesn't open by hand makes it iron's, of a button arrows don't press stone's, of a plate
    that senses mobs alone stone's; a weighted plate counting past 15 is the heavy one. A wall
    starts with its post up, as its constructor sets it. Codecs mods build theirs on: a field
    named `*CODEC*` (not `STREAM_CODEC`) of the game's that isn't modelled is still a codec (was:
    `Ingredient.LIST_CODEC_NONEMPTY` and `X.CODEC_NONEMPTY` read as null, so Farmer's Delight's
    cooking recipe codec and every codec after it in its class broke); `PrimitiveCodec.INT` is
    `Codec.INT` (Lootr); NeoForge's `Ingredient.LIST_CODEC`/`LIST_CODEC_NONEMPTY` read lists of
    ingredients, the second refusing an empty one with vanilla's message;
    `RegistryFileCodec.create` reads an entry by id, `RegistryCodecs.homogeneousList` is a codec
    that can't read yet. The translator resolves a constant the mod's classes reach through an
    API interface they implement (`RecipeSerializer.SHAPED_RECIPE` through sophisticatedcore's and
    Farmer's Delight's serializers was `java/lang/Object.SHAPED_RECIPE`), and vanilla's items
    inherit `Item.BASE_ATTACK_DAMAGE_ID`. Also: `UUID.nameUUIDFromBytes` (MD5, version 3: offline
    player and fake player UUIDs), `String.getBytes` (UTF-8, Latin-1, ASCII, UTF-16),
    `ChestBlock.AABB` and its sides, `WallHangingSignBlock`'s shapes, `BrushableBlock`'s fields
    and getters, `StateDefinition.Builder.properties` (FramedBlocks' accessor), `TicketType`
    (by hand, out of the data-class table), `MinMaxBounds.Ints`/`Doubles` (factories, `ANY`,
    `matches`, `isAny`), `Stats` (stat types, custom stat ids, `StatType.get`, formatters),
    `LootContextParamSets`, `BlockItemStateProperties.EMPTY`, `PotionContents.withEffectAdded`,
    `Tool.Rule`'s factories, `IngredientType(mapCodec)`, `ConcatenatedListView.of`,
    `ModLoader.postEvent` (a mod's own event to every mod's bus), `DeferredItem.createItem`,
    `DeferredBlock.createBlock`, `Registry.asLookup`, `ResourceLocation.Serializer` (Gson),
    `Minecraft.getEntityModels`, `BlockEntityWithoutLevelRenderer`'s constructor, `Mth.color`,
    `LocalDate` (UTC) and `Month`, `TimeUnit`'s conversions, Guava's `Stopwatch`, fastutil's
    `ObjectOpenHashSet.of` and empty maps, `Map.ofEntries`, `Maps.immutableEnumMap`. Tests
    `blocks::tests::{vanilla_kinds_copy_the_vanilla_block_that_behaves_as_they_do,
    vanilla_statics_and_fields_mods_read_are_vanillas}`, `codecs::tests::codecs_mods_build_on_are_codecs`,
    `tags::tests::a_name_uuid_is_javas`, `api_common::tests::{bounds_match_as_vanillas_records_do,
    dates_and_time_units_are_javas}`, `codegen::field_tests::an_inherited_constant_is_the_api_interfaces_the_chain_implements`;
    252 compat tests and 14 translator tests pass.
  - [x] 2026-11-05 In game with 05a5b75 and 295bf8a (Create with the ten addons): kinds of
    unmapped call 32 to 28, none new; the rest are Flywheel probing OpenGL (its backend stays
    off), two `super.getShape` to vanilla kinds, `BlockState.getProperties` and the local
    player's look before the world. Enchantable machinery keeps its enchantments through
    `/data get block` (a drill with efficiency 5 and fortune 3, a millstone with efficiency 2).
    Then round 11's roots: a mod's own property class (StorageDrawers' `IntegerSetProperty extends
    Property<Integer>`: `super(name, Integer.class)` was unmapped, so its drawers lost their
    `slots` states and the mod read 1568 elements of nothing) now takes the values its
    `getPossibleValues` gives, spelled by its `getName` and read back by its `getValue`; vanilla's
    kinds subclassed by a mod (`EnumProperty(name, class, values)`, `IntegerProperty(name, min,
    max)`) take theirs as `create` does, and `Property.getName(value)` spells a value (it gave
    the property's name). An `EnumProperty` of a vanilla enum filtered by a predicate
    (FramedBlocks' rail slopes: `RailShape::isAscending`) has the constants it keeps (it had
    none, and FramedBlocks threw "Invalid shape" in common setup). `super.getShape(...)` to a
    vanilla kind whose shape follows its state answers vanilla 1.21.1's boxes for the state, read
    from its classes: doors, trapdoors, ladders, levers, buttons, pressure plates, slabs, stairs,
    panes and iron bars, fences, fence gates, torches and wall torches, lanterns, chains, cakes,
    farmland. `ServerLifecycleHooks.getCurrentServer()` is the server once the world's level is
    open, in the server's runtime (Mekanism's saved data threw "Current server is null" from
    `LevelEvent.Load`). Tests `blocks::tests::{a_mods_own_property_class_gives_a_block_its_states,
    an_enum_property_filters_a_vanilla_enums_constants,
    super_get_shape_to_a_vanilla_kind_follows_the_state}`,
    `neo::tests::the_current_server_is_there_once_the_world_is`; 256 compat tests pass.
  - [x] 2026-11-05 Load round 12 (the 28 jars, snapshot `7afb7f7`): 28 load, exit 0; 3651 blocks
    and 2893 items, as in round 11. Distinct unmapped calls 217 to 155: Farmer's Delight 23 to
    10, FramedBlocks 17 to 8, Mekanism 74 to 50, lootr 9 to 2, immersive_aircraft 6 to 1, Jade and
    geckolib to none. Three uncaught exceptions were left, each traced to its root and fixed here:
    FramedBlocks' "Invalid shape" (`RailShape.isAscending` was unmapped, so the property's filter
    kept no rail slope), its item model source error (`BlockState.is(Holder)` was unmapped), and
    Mekanism's "Current server is null" (`Level.isClientSide()` answered by the host's side, not
    by the level's own field, so the server's level read as the client's). StorageDrawers' 1568
    reads of nothing come from the load test, not the runtime: its drawers' label, count and
    indicator areas are elements of JSON models in its own jar
    (`DrawerModelGeometry.populateGeometryData` reads `models/block/geometry/*.json` through
    `BlockModel.fromStream`), and a load test has resources only from the folders
    `HORIZON_COMPAT_RESOURCES` names, so the read throws and the mod indexes a null model; in game
    the host gives them. The next round passes each jar's own files. Fixes from round 12 and the
    game run: `StateHolder.getProperties()` (a mod block's own property objects, else vanilla's
    `BlockStateProperties` constants by name, so a mod copying a vanilla state's values onto its
    own block keeps them; it was unmapped in game); the local player's look and eyes before the
    world is there (`getLookAngle`, `getViewVector`, `getEyePosition`); a mod's own render types
    (`RenderType.create` with the `CompositeState` its builder made, a type by its name that keeps
    that state; `RenderStateShard`'s constants by name, shards of a mod's own or of vanilla's kinds
    keep what they were given: SecurityCraft's overlay lines); `GLCapabilities` fields Flywheel
    reads are false; NeoForge's `SimpleFluidContent` (`CODEC`, `STREAM_CODEC`, `EMPTY`, `copyOf`,
    its fluid stack calls); `Level.isClientSide()` by the level's field; `BlockState.is(Holder)`;
    `RailShape.isAscending`. Tests `api_common::tests::a_states_properties_carry_its_values_onto_another`,
    `api::tests::a_state_is_its_blocks_holder_and_a_level_its_own_side`,
    `render::tests::a_mods_own_render_type_is_one_by_its_name`, and the rail filter now through
    `RailShape.isAscending` itself; 259 compat tests pass.
  - [x] 2026-11-05 Load round 13 (snapshot `8701863`), each load test now given the jar's own
    `assets/` and `data/` (and its addons') and then vanilla 1.21.1's as `HORIZON_COMPAT_RESOURCES`,
    as the game's resource root gives them: 28 load, exit 0, and no uncaught exception in any of
    them (3 in round 12). FramedBlocks gets through its common setup now that its rail slopes are
    whole: 238 blocks and 213 items registered (114 and 100). StorageDrawers reads its drawers'
    geometry from its models (its 1568 reads of nothing are gone). Distinct unmapped calls 155 to
    136. Then: reflection over a mod's own classes (`Class.getDeclaredField`/`getField`,
    `Field.get`/`set` and the typed ones, a static when given no object after the class is
    initialised, boxed through `get`; `getDeclaredMethod` and `Method.invoke` on a mod object by
    name and parameter types; `MethodHandles.lookup()`, `unreflectGetter`/`unreflectSetter`/
    `unreflect`, `findGetter`/`findStaticGetter`, `MethodHandle.invoke`/`invokeExact` typed as
    the call site: Mekanism's computer methods take 70 getters this way). A vanilla enum's `CODEC`
    reads and writes its constants by serialized name (`EquipmentSlot`,
    `AttributeModifier.Operation`, `GenerationStep.Decoration`...); `EquipmentSlot` and
    `ChatFormatting` have `values()`. `LocalTime` (now in UTC, `MIDNIGHT`, `until` in whole
    units, signed), `ChronoUnit` and its durations, `Duration` (seconds rounded down and the
    nanoseconds past them, as Java keeps it), executors that run what they are given at once
    while a delayed or periodic task never runs (its future not done until cancelled), Guava's
    `ThreadFactoryBuilder`, `ConcurrentHashMap.newKeySet`, `RoundingMode`, `DecimalFormatSymbols`
    (the root locale's), Log4j's `Level` and `Configurator`, SLF4J's `MarkerFactory`.
    `FeatureFlags.REGISTRY.allFlags()`, `Mth.SQRT_OF_TWO`, `DimensionType.MIN_Y`/`MAX_Y`,
    `ChiseledBookShelfBlock.SLOT_OCCUPIED_PROPERTIES` (1.21.1's `CHISELED_BOOKSHELF_SLOT_n_OCCUPIED`
    are `slot_n_occupied`), and vanilla 1.21.1's shapes behind `super.getShape` of chests (a
    double one's half reaching to the side it joins), ender chests, decorated pots, hoppers (hollow,
    the spout where it faces) and scaffolding. Tests
    `jlang::tests::reflection_reaches_a_mods_fields_and_methods_by_name`,
    `codecs::tests::a_vanilla_enums_codec_goes_by_its_serialized_name`,
    `api_common::tests::times_of_day_durations_and_executors_are_javas`, chests and hoppers in
    `blocks::tests::super_get_shape_to_a_vanilla_kind_follows_the_state`; 262 compat tests pass.
  - [x] 2026-11-05 Game rules as mods read them (`level.getGameRules().getBoolean(GameRules.RULE_MOBGRIEFING)`;
    SecurityCraft, Create's contraptions and many mobs' griefing checks ask). `gamerules.rs` answers
    1.21.1's 51 `GameRules.RULE_*` keys, each the game's own rule of its 26.3 name through the new
    `Host::game_rule` (the engine's `native_game_rule` reads the world's rules; the bundle bridges
    it): 1.21.11 renamed them and turned some around, so `disableRaids` reads `raids` negated,
    `disableElytraMovementCheck` `elytra_movement_check` negated, `doFireTick` whether
    `fire_spread_radius_around_player` isn't 0, `doDaylightCycle` `advance_time`; the two portal
    delays and `spawnChunkRadius`, which 26.3 doesn't have, read their 1.21.1 defaults. A rule a mod
    registers (`GameRules.register(id, category, BooleanValue.create(default))`) reads its default,
    then what the mod sets it to (`getRule(key).set(value, server)`); keys answer `getId`,
    `getCategory`, `getDescriptionId`, `equals` and `hashCode` by id. Also `OptionalLong` and
    `OptionalDouble` as `OptionalInt` (with `orElseGet`, `ifPresentOrElse`), and vanilla's
    `Util.toMap()` (entries back into a map, through a new method-as-function combinator).
    Tests `gamerules::tests::a_mod_reads_the_games_rules_by_their_old_keys` (a host with
    `mob_griefing` off, `raids` off, fire spread 0, `random_tick_speed` 7),
    `menus::tests::optional_doubles_and_longs_hold_their_kind`, `Util.toMap` in
    `jutil::tests::streams_join_and_collect_to_maps_as_java_does`; 264 compat and 14 translator
    tests pass. In game (the build of the step before, e41df47, `cemgame.sh`): 14 kinds of unmapped
    calls, from 28; door, ladder and lever shapes, `BlockState.getProperties`, the local player's eye
    position and look angle and Flywheel's GL capability reads no longer show. Left: Flywheel's
    shader compiler (its GL backend, which Horizon's renderer replaces) and 3 `Object.get` on null.
  - [x] 2026-11-05 Tag constants of every tag class, vanilla entity types, and collection
    operations on stream codecs. `tools/vanilla_tag_keys.py` reads 1.21.1's tag classes from
    NeoForge's merged jar with `javap -c` (each constant's factory, its registry and namespace) into
    `vanilla_tag_keys.rs`: 316 constants of `BiomeTags`, `EntityTypeTags`, `DamageTypeTags`,
    `EnchantmentTags`, `StructureTags`, `PoiTypeTags`, `GameEventTags` and the rest, and NeoForge's
    `Tags.Biomes`, `Tags.DamageTypes`, `Tags.Enchantments`, `Tags.Structures` (`BiomeTags.IS_END` is
    biome tag `minecraft:is_end`, `HAS_ANCIENT_CITY` `minecraft:has_structure/ancient_city`,
    `Tags.Biomes.IS_VOID` `c:is_void`); `neo_tags` answers them after its own. Vanilla's
    `EntityType.<NAME>` are one object per id (`minecraft:illusioner`), the same the entity type
    registry gives for that id, named by it (`getKey`, `getDescriptionId`): guardvillagers' illusioner
    spawn egg. `ByteBufCodecs.collection(factory)` alone is a `CodecOperation` (Mekanism's
    attachments, SecurityCraft's components apply it to a codec), reading into what the factory
    makes. Tests `neo_tags::tests::other_tag_classes_name_their_registrys_tags`,
    `mod_entities::tests::vanilla_entity_types_are_one_object_by_id`,
    `network::tests::a_collection_operation_reads_into_the_factorys_collection`; 267 compat and 14
    translator tests pass.
  - [x] 2026-11-05 Potions, loot parameters, ModLauncher's environment, the tools' attributes and
    a tab's constructor (round 13's misses of SecurityCraft, guardvillagers, Aquaculture). Vanilla's
    `Potions.<NAME>` are the potion registry's holders, one per id (all 46 of 1.21.1's fields name
    their own id, checked with `javap`), the same the registry gives; `new PotionContents(holder)`
    and `PotionContents.EMPTY`. `LootContextParams.<NAME>` are one parameter per id (all 12), and a
    mod's own `LootContextParamSet.Builder` builds a set that requires and allows what it was given.
    `Launcher.INSTANCE.environment().getProperty(key)` answers nothing, as a player's game
    (Aquaculture's dev check). `AxeItem`, `HoeItem`, `PickaxeItem` and `ShovelItem.createAttributes`
    are `DiggerItem`'s; `new CreativeModeTab.Builder(row, column)` builds as `builder()`'s. Tests
    `food::tests::potions_are_holders_and_contents_hold_them`,
    `api_common::tests::a_mods_loot_parameter_set_holds_its_parameters`,
    `neo::tests::modlaunchers_environment_tells_no_properties`,
    `blocks::tests::a_tools_attributes_add_its_tiers_damage`,
    `creative::tests::a_tab_built_from_its_constructor_keeps_its_title`; 272 compat and 14
    translator tests pass.
  - [x] 2026-11-05 Round 14 (snapshot 8aee561, the 28 mods): unmapped calls at load, counted per
    mod, 140 to 74; no uncaught exception, every mod's blocks and items as before (SecurityCraft
    1215/81, mcw-furniture 662/654, Mekanism 200/381). Then its misses of comforts, Mekanism,
    FarmersDelight and immersive_aircraft: Night Config in memory (`nightconfig.rs`: `Config.of`,
    `inMemory`, `wrap`, `CommentedConfig`, values by dotted path or path list, `getEnum` by name,
    name ignoring case or ordinal as `EnumGetMethod` says, comments, sub-configs; comforts' config
    spec); vanilla's attributes with 1.21.1's defaults and ranges (all 31, read with `javap`) and a
    mod's own `Attribute`/`RangedAttribute` (`sanitizeValue` clamps, NaN is the minimum);
    `Villager.FOOD_POINTS` and `WANTED_ITEMS` as 1.21.1's initialiser fills them; and a game
    class's static a mod writes (FarmersDelight's `Villager.FOOD_POINTS = ImmutableMap.copyOf(...)`,
    opened by its access transformer) is what later reads get. `CauldronInteraction.DYED_ITEM` and
    the other five interaction constants are the lambdas vanilla's initialiser makes them from;
    `DYED_ITEM`'s is modelled (a dyeable stack with a colour loses it on the server, the cauldron a
    level, an empty cauldron after the last; `LayeredCauldronBlock.lowerFillLevel`). A call nothing
    answers on one of the runtime's functions, through whatever interface of the game's holds it
    (`interact`), runs it. `tools/vanilla_tag_keys.py` now keeps 1.21.1's `BlockTags` and
    `ItemTags` too (647 constants), read after 26.3's: `ItemTags.DYEABLE`, which 26.3 dropped, is
    `minecraft:dyeable`. Tests `nightconfig::tests::a_config_keeps_values_by_path_and_reads_enums_by_name`,
    `attributes::tests::attributes_answer_their_defaults_and_keep_values_in_range`,
    `api_common::tests::villager_food_is_vanillas_until_a_mod_replaces_it`,
    `blocks::tests::vanillas_dyed_item_interaction_washes_a_dyed_stack`; 276 compat and 14
    translator tests pass.
  - [x] 2026-11-05 Class objects are one per class (`rt.class_literal`: a class literal, `getClass`,
    `Integer.TYPE` and the other primitives' `TYPE`), so `type == int.class` holds as in Java
    (Mekanism's `PropertyType.getFromType` compares by identity). The translator writes each class's
    declared fields into the bundle's reflection table (`access name descriptor`), and the runtime
    answers `Field.getType`, `getModifiers`, `Class.getDeclaredFields`, `getFields` (public ones,
    superclasses' too), `isArray`, `getComponentType` and `isPrimitive` for the mod's classes:
    Mekanism's `SyncMapper` reads its synced fields' types (70 `Field.getType` misses in round 14).
    Test `jlang::tests::a_mod_classs_fields_tell_their_types`; 277 compat and 14 translator tests pass.
  - [x] 2026-11-05 NeoForge's `SizedIngredient`, which Mekanism's machine recipes are made of
    (`ItemStackIngredient` holds one): `FLAT_CODEC` and `NESTED_CODEC` (the ingredient's keys beside
    `count`, or under `ingredient`; one when no count is written, a count below one refused),
    `new SizedIngredient(ingredient, count)`, `of(item or tag, count)`, `test` (as many of its items
    or more), `getItems` (stacks of that many), `equals`, `hashCode`, `toString`. Test
    `recipes::tests::a_sized_ingredient_wants_as_many_of_its_items`; 278 compat tests pass.
  - [x] 2026-11-05 Round 15 (snapshot fdc19df): 79 calls unanswered across the 28 mods (74 in round
    14). FarmersDelight 9 to 7, comforts 5 to 0, immersive_aircraft 1 to 0. Mekanism went from 35 to 48
    because its field reflection now works and its `SyncMapper` gets further: it now asks
    `Field.isAnnotationPresent` 2607 times and builds getters through `LambdaMetafactory` and
    `MethodHandle`s. No uncaught exceptions, same blocks and items. Also in this batch:
    `StringRepresentable.EnumCodec` (vanilla's `fromEnum` makes one: reads a constant by its serialized
    name, `byName` with or without a default) and `createNameLookup`. `Codec.LONG_STREAM` beside
    `INT_STREAM` (a `long[]` in NBT), and `IntStream`/`LongStream`/`DoubleStream.of` and `toArray`
    with primitive arrays. NeoForge's `NeoForgeExtraCodecs.dispatchMapOrElse`: dispatched by `type`
    when the object has one, else the fallback map codec. The load harness no longer fires
    `FMLCommonSetupEvent` a second time (the bundle's init already does). The second run caused
    StorageDrawers' `List.iterator` on null. Tests
    `codecs::tests::an_enum_codec_reads_and_finds_constants_by_serialized_name`,
    `codecs::tests::dispatch_or_else_reads_by_type_and_falls_back_without_one`, and
    `codecs::tests::vanilla_value_codecs_write_and_read_as_vanillas` (now with `LONG_STREAM`). 280 compat
    and 14 translator tests pass.
  - [x] 2026-11-05 Annotations as reflection sees them, and `java.lang.invoke` called by hand.
    The translator now records which annotations are `RUNTIME` ones (`RuntimeVisibleAnnotations`)
    and each annotation type's element defaults (`AnnotationDefault`). It writes them into the
    bundle's reflection table (`ANNOTATIONS`: `TYPE`, `FIELD name`, `METHOD name+descriptor`,
    `DEFAULT element` lines). The runtime (`annotations.rs`, new) answers `isAnnotationPresent`,
    `getAnnotation`, `getDeclaredAnnotation`, `getAnnotations` and `getAnnotationsByType` on a mod's
    classes (`@Inherited` ones from superclasses), fields (from the declaring class) and methods.
    Each annotation is an object of its own type: its elements come from the class file's values,
    else the type's defaults (enum constants, classes, arrays typed by the element's return type,
    nested annotations), and `annotationType()` is answered. SecurityCraft registers its content's
    block items from `@Reinforced` and `@RegisterItemBlock` on `SCContent`'s fields (2607
    `isAnnotationPresent` misses in round 15). `java.lang.invoke` moved to its own module
    (`invoke.rs`, new):
    - `MethodType` (the `methodType` factories, `wrap`, `unwrap`, `erase`, `generic`, `changeReturnType`,
      `dropParameterTypes`, `insert`/`appendParameterTypes`, descriptors, `equals`).
    - `MethodHandle.type()`, `asType`, `bindTo`, `MethodHandles.exactInvoker`/`invoker`, `constant`,
      `identity`, `insertArguments`.
    - `Lookup.findVirtual`, `findStatic` (the game's), `findConstructor`, `unreflectConstructor`.
    - `LambdaMetafactory.metafactory`/`altMetafactory` with `CallSite.getTarget`: the target, given what
      the lambda captures, makes a function (`compat:fn/handle`) that invokes the implementation through
      whatever interface holds it.
    Mekanism's `LambdaMetaFactoryUtil` builds its menus' field getters and setters this way. Tests
    `annotations::tests::a_fields_annotation_answers_its_values_and_its_types_defaults`,
    `annotations::tests::classes_and_methods_carry_theirs_and_only_inherited_ones_pass_down`,
    `invoke::tests::method_types_change_as_javas_do`,
    `invoke::tests::a_hand_made_metafactory_lambda_reads_and_writes_a_field`,
    `invoke::tests::bound_and_constant_handles_answer_as_javas`, and
    `codegen::annotation_tests::reflection_sees_runtime_annotations_and_element_defaults`
    (javac-compiled: a `CLASS` annotation stays out). 285 compat and 15 translator tests pass.
  - [x] 2026-11-05 Round 16 (snapshot c37ebab): 48 calls unanswered across the 28 mods (79 in round
    15), no uncaught exceptions. SecurityCraft registers 671 items (81 in round 15): the block items
    its `@Reinforced` and `@RegisterItemBlock` fields name, and its one miss left is
    `EnumProxy.<init>` (NeoForge's enum extensions, which the translator doesn't apply yet). Mekanism
    went from 48 to 20: every `java.lang.invoke` and `LambdaMetafactory` miss is gone. StorageDrawers
    went from 2 to 0. Also in this batch:
    - Records' `dependent` fields (`Codec.dependent`, `RecordCodecBuilder`'s field whose codec the
      field before it picks): read with the decoder the earlier value names, written with the codec
      the getter's value gives.
    - A mod's `STREAM_CODEC` with no stream codec of its own falls back to `FLAT_CODEC` as well as
      `CODEC`.
    - Stream codecs write NeoForge's ingredients: `Ingredient` (one entry as `{"item"}` or
      `{"tag"}`, several as a list), `SizedIngredient`, `FluidIngredient` (single, tag, compound,
      components) and `SizedFluidIngredient` in the form their codecs read.
    - `BlockItemStateProperties.CODEC`: a map of property names to values.
    - JOML's `Vector3f.hashCode` and `equals` as JOML computes them (float bits; `0.0` and `-0.0`
      differ).
    Tests `codecs::tests::a_dependent_field_reads_with_the_decoder_the_field_before_picks`,
    `codecs::tests::block_item_state_properties_read_and_write_as_a_map_of_names`,
    `render::tests::vectors_hash_and_compare_as_jomls` (values from JOML 1.10.5),
    `recipes::tests::a_sized_fluid_ingredient_written_back_takes_the_same_stacks`, and
    `recipes::tests::a_sized_ingredient_wants_as_many_of_its_items` (now through `STREAM_CODEC` and
    back). 289 compat and 15 translator tests pass.
  - [x] 2026-11-05 Round 17 (snapshot 71ef340): 42 calls unanswered across the 28 mods (48 in round
    16), no uncaught exceptions. Mekanism went from 20 to 16, FramedBlocks from 5 to 3. What's left
    at load is mostly codecs of vanilla's data types (worldgen's `PlacedFeature.CODEC`,
    `Biome.LIST_CODEC`, `OreConfiguration$TargetBlockState.CODEC`; `MinMaxBounds$Doubles.CODEC`,
    `IntProvider.CODEC`, `ICondition.CODEC`), NeoForge's client model builders, and calls into other
    mods' APIs (AE2's, FramedBlocks').
  - [x] 2026-11-05 Mods' worldgen code in Horizon's generator, stage 1: placement modifiers. The
    generator refused a whole placed feature whose chain names a mod's modifier type, so the feature
    never generated: Create's zinc ore and striated ores (`create:config_filter`), Mekanism's ores
    (`DisableableFeaturePlacement`), Farmer's Delight's wild crops (`BiomeTagFilter`). Now:
    - `horizon-world` `worldgen/foreign.rs` (new): a translated mod registers a `Handler`; the
      placement decoder asks the handlers for a type vanilla doesn't have
      (`PlacementModifier::Foreign`), and the chain runs what it returns on the region and the
      feature's own random stream, so the mod's code draws in vanilla's order. The mod's code sees
      the world through `World`: blocks as text, block tags, heightmaps by vanilla's name, the
      height range, the seed, sea level, the biome at a position and the biome tags (the
      generator's, vanilla's and the packs').
    - `horizon-compat` `worldgen.rs` (new): reads a modifier with the codec its type was registered
      with (`PlacementModifierType.codec()`), and answers `getPositions` as vanilla does around the
      mod's `shouldPlace` (`PlacementFilter`) or `count` (`RepeatingPlacement`). It also answers
      `PlacementContext` (`getLevel`, `getHeight`, `getMinBuildHeight`, `getBlockState`), the
      level's `getHeight(Heightmap.Types, x, z)`, `getSeed` and `getBiome` (a holder whose tags the
      host answers), and a `WorldgenRandom` that draws from the stream the host lends
      (`Host::random`).
    - The bundle's content hook registers the handler. The mod's code runs on a runtime of its own
      for worldgen (`with_worldgen_runtime`, behind a mutex because the generator's workers are
      threads of their own; a call from inside it is refused rather than deadlocking).
    Harness (`WORLDGEN=1`, Create's real bytecode): `create:config_filter` reads as
    `ConfigPlacementFilter`, the same object both times, and keeps its position (Create's config
    `worldgen.disable` is false by default). Another mod's type isn't Create's to read. The
    worldgen runtime starts in 0.19 s and 144 MB. Tests:
    `foreign::tests::a_mods_modifier_places_through_its_handler_with_the_features_random`,
    `worldgen::tests::a_mods_filter_keeps_the_positions_its_should_place_takes_from_the_hosts_random`
    (a `java.util.Random` oracle), `worldgen::tests::a_mods_repeating_modifier_gives_its_position_count_times`,
    `worldgen::tests::a_mods_filter_reads_the_biome_and_its_tags_from_the_host`. 292 compat tests pass.
    Next: zinc ore in new chunks in game; then stage 2, mods' feature types (`create:layered_ore`,
    Mekanism's `ResizableOreFeature`, Farmer's Delight's `WildCropFeature`): the mod's
    `Feature.place` on a `WorldGenLevel` that writes blocks.
  - [x] 2026-11-05 IN GAME: Create's zinc ore generates. The placed feature was skipped because of
    its feature, not its modifier. Mods' worldgen files (NeoForge 1.21.1's shape, which is 26.2's:
    `config` wrapped) went into the game's feature catalog unchanged, and 26.3's catalog reads
    features with `config` unwrapped (`invalid world generation field: targets`). Fixes:
    - `horizon-port-pack`'s install writes a mod's configured and placed features in 26.3's form,
      through the upgrade 26.2 data packs get (`upgrade::feature_file`).
    - The game logs each placed feature it skips and why. Before, it logged only a count.
    - The runtime's `BlockState.CODEC` also reads 26.3's state shapes (`{id, properties}`, or the id
      alone), since a mod's installed data has them.
    Fresh world, the four chunks around spawn, y -63 to 70: 81 `create:zinc_ore` and 196
    `create:deepslate_zinc_ore` (0 before); `find-block` sees 786 within 48 blocks. Still skipped:
    `create:striated_ores_overworld` (`create:layered_ore`, a mod's feature type: stage 2).
    Stage 2 so far: `horizon-world` decodes a feature type a mod registered through its handler
    (`FeatureDefinition::Foreign`) and places it with the mod's code. That code writes states named
    as text through `setBlock` or the chunk section (`World::set_block`, `set_section_block`), reads
    block tags by name, and asks `ensureCanWrite`. Tests
    `foreign::tests::a_mods_feature_writes_the_states_it_names_the_way_it_asks` and
    `recipes::tests::block_states_read_in_every_shape_the_data_has`. 293 compat and 110 port-pack
    tests pass.
  - [x] 2026-11-05 Stage 2, runtime side: a mod's own feature type runs on the worldgen runtime.
    Create's striated ores (`create:layered_ore`, `LayeredOreFeature`) place their layers in the
    harness, from Create's real bytecode.
    - `horizon-compat` `worldgen.rs`: `feature(kind, json)` reads a configured feature with the
      codec its type was made with (`Feature(Codec)`), from 1.21.1's shape (`config`) or 26.3's
      (fields inline). `place(configured, origin)` runs the mod's `place(FeaturePlaceContext)` on
      the host's region and random stream, after `ensureCanWrite(origin)` as vanilla's
      `Feature.place` asks. What features use of vanilla's worldgen is answered there too:
      - `BulkSectionAccess` and `LevelChunkSection` (raw section writes, as `Host::set_section_state`);
      - `SectionPos`;
      - `SimplexNoise`, checked bit for bit against vanilla 1.21.1's class;
      - the rule tests ore targets are made of (`RuleTest.CODEC`: tag, block, state, random block,
        random state, always true) and their `test`;
      - `OreConfiguration` (codec, constructors, `target`) and `TargetBlockState`;
      - `FeaturePlaceContext`;
      - `Feature`'s helpers (`setBlock`, `safeSetBlock`, `isStone`, `isDirt`, `checkNeighbors`,
        `isAdjacentToAir`);
      - `Mth`'s draws (`randomBetween`, `randomBetweenInclusive`, `nextInt/Float/Double`);
      - the level's `ensureCanWrite` and its build height from the host.
      These classes are modelled by hand, so `tools/vanilla_records.py` leaves them out of the
      data-class table (rerun against neoforge-21.1.225's merged jar: 3 classes removed, nothing
      else changed).
    - The bundle's worldgen hook registers `Handler::feature` (`ModFeature`). The region the mod
      sees now writes (`setBlock`, section writes), answers `ensureCanWrite` and block tags by name.
      Each call's outermost entry is a safe point for garbage: what the generator keeps of the
      mod's code is interned by its text.
    - Translator fix: a lambda's implementation taking primitives now gets its arguments as
      `LambdaMetafactory` adapts them: unboxed from the interface method's `Object`s, widened
      (`Rt::adapt_args`). Create's `LayeredOreConfiguration::new` in its codec took the boxed
      `Integer` and `Float` as they were, so `size` read as 0 and its layer loop never ended. The
      differential test's new `adapted` case gives 4 without the fix and 208 (the JVM's) with it.
    - Speed: one placement (size 32, ~9000 blocks written) took 94 ms. The profile showed 70% in
      the API dispatch: `call_exact`'s string match and five `exception_class` guards on every key,
      the generated `overrides` table asked per comparison in `Collections.binarySearch`, and key
      splitting that missed on nested calls. These are now kept per key: keys that go straight to
      the modules, `Rt::class_overrides`, and a 256-slot `parts` cache. Now 46 ms, with the same
      blocks written. Create's translated `place` itself is the top of the profile now.
    Harness (`WORLDGEN=1`): both shapes give the same layers from the same seed. At y 30:
    limestone 4858, calcite 1432, diorite 2006, stone 874 (9170 section writes, no `setBlock`).
    Veridium, asurine and scoria at other heights. Nothing above the ocean floor (y 70), nothing
    below the world's bottom. No unmapped worldgen calls. Tests:
    `worldgen::tests::a_mods_feature_places_by_its_rule_tests_through_sections`,
    `worldgen::tests::rule_tests_test_states_as_vanillas` (a `java.util.Random` oracle for the
    random test), `worldgen::tests::simplex_noise_draws_and_reads_as_vanillas` (vanilla 1.21.1's
    `SimplexNoise` through `LegacyRandomSource`: offsets, values and the stream after, by bits).
    The values come from a Java oracle run on the Mojmapped 1.21.1 classes (NeoForm's
    `sourcesAndCompiledWithNeoForge` jar). Also
    `worldgen::tests::ore_configurations_read_and_made_as_vanillas`. 297 compat tests pass.
  - [x] 2026-11-05 IN GAME: Create's striated ores generate. Fresh world, game built with the
    translator above (`scratchpad/striatedgame.sh`). `find-block` within 64 blocks of spawn:
    asurine 773, crimsite 181, ochrum 1730, veridium 1411, limestone 108 (scoria 0 there: one
    pattern of six, picked at random). Zinc ore is still there (2037). The log lists no skipped
    features any more: `create:striated_ores_overworld` ran Create's own `LayeredOreFeature`.
    Next: the other mods' feature types (Mekanism's `ResizableOreFeature`,
    `ResizableDiskReplaceFeature`, `OreRetrogenFeature`; Farmer's Delight's `WildCropFeature` and
    `WildRiceFeature`), and speeding up translated code in general (the dispatch above).
  - [x] 2026-11-05 Stage 3: mods' own biome modifier types (Farmer's Delight's
    `farmersdelight:add_features_by_filter`, which puts its wild crops in vanilla's biomes), and a
    mod's feature placing vanilla placed features (FD's `WildCropFeature` places inline ones).
    - Runtime (`horizon-compat/src/worldgen.rs`): `biome_modifier` reads a modifier file with the
      codec the mod registered in `NeoForgeRegistries.BIOME_MODIFIER_SERIALIZERS`. `modify_biome`
      runs the mod's `BiomeModifier.modify(biome, phase, builder)`. The biome is a
      `Holder$Reference`; its `Biome` answers `getBaseTemperature` and `hasPrecipitation` from the
      biome's file. The phase is `BiomeModifier$Phase`'s constant. The builder is NeoForge's
      `BiomeInfo$Builder`; its `BiomeGenerationSettingsBuilder.addFeature`/`getFeatures` grow the
      step lists up to the step used, as NeoForge's do.
    - Holder sets (`Biome.LIST_CODEC`, `PlacedFeature.LIST_CODEC`, `RegistryCodecs.homogeneousList`)
      decode to sets the host resolves (`Host::holder_set`). They answer `contains`, `size`, `get`,
      `stream`, `iterator`, `forEach`, `getRandomElement` and `unwrapKey`. Also `PlacedFeature.CODEC`,
      `DIRECT_CODEC`, and `PlacedFeature.place` (`Host::place_feature`).
    - Generator (`horizon-world`): a mod feature's `PlacedFeature.place` places a vanilla or inline
      placed feature through the generator (`PlacementContext::place_feature_json`). It draws from
      the mod feature's own random stream (`CellRandom`), as in vanilla. Placed features made from
      text are cached (`placed_json`).
    - Content root (`horizon-client/src/mod_content_biomes.rs`): a modifier file whose type a
      translated mod registers is read by that mod's codec and run by its code. Biome by biome, the
      phases run in NeoForge's order (before everything, add, remove, modify, after everything),
      with NeoForge's own `add_features`/`remove_features` types in the same pass. A tag the root
      lacks is empty to the mod's code, as NeoForge's missing tags are, and is warned about. A placed
      feature the root lacks (one that didn't port: 26.3 has no `random_patch`) is left out, also
      with a warning. Biomes the code doesn't change keep their text.
    - Port pack (`carry.rs`): the loader's convention biome tags a biome modifier names
      (`#c:is_mushroom`) come along with the mod, as the structures' did.
    - `ModifiableBiomeInfo$BiomeInfo$Builder` and `BiomeModifier$Phase` are modelled by hand now
      (`tools/vanilla_records.py` rerun: data classes 664 → 663, enum constants 363 → 362).
    Tests: `worldgen::tests::a_mods_biome_modifier_edits_the_biomes_its_code_picks` (a modifier made
    by the mod's codec with temperature bounds and a denied set; 298 compat tests pass);
    `worldgen::foreign::tests::a_mods_feature_places_vanilla_features_on_its_own_random_stream`;
    `mod_content::biomes::tests::a_mods_own_modifier_type_runs_its_code_between_neoforges_phases`
    (the old biome modifier tests still pass); `carry::tests::biome_modifiers_get_the_loaders_biome_tags`
    (111 port-pack tests pass); the translator's tests pass. Not checked yet: FD's real bytecode
    through a bundle (the bundle template's new code compiles only inside a bundle), and in game.
  - [x] 2026-11-05 Farmer's Delight's worldgen on its real bytecode (`scratchpad/fdh`: FD bundled on
    the runtime core, a harness linking its engine; 292 classes, 61k lines, debug build).
    - Its 9 biome modifier files read with FD's own codec (`AddFeaturesByFilterBiomeModifier`).
      Run on 26.3's 67 biomes, with vanilla's biome tags and NeoForge 21.1.256's convention tags:
      3015 `modify` calls, no errors, 120 µs each unoptimised. The features land where FD's files
      say: cabbages and beetroots on the two beaches, both mushroom colonies in mushroom fields,
      tomatoes in 10 hot biomes, potatoes in 7 cold ones, carrots and onions in 26, rice in 6.
    - Its placement modifier `farmersdelight:biome_tag` keeps a beach position and drops a
      Nether one.
    - `WildCropFeature` places its inline placed features through the host: 64 primary and 64
      secondary for cabbages.
    - `WildRiceFeature` places 24 rice plants in a marsh, each a lower half
      (`waterlogged=true`) in the water and an upper half (`waterlogged=false`) above.
    It needed:
    - `RandomPatchConfiguration.CODEC` (counts default as `orElse` does, the placed feature
      required). The record's accessors come from the records table.
    - `WorldGenLevel.getHeightmapPos` (a column's top as a position).
    - NeoForge 1.21.1's `BushBlock.canSurvive` and `DoublePlantBlock.canSurvive` for a mod plant's
      `super.canSurvive`. The soil's `canSustainPlant` decides first (a mod soil's override; vanilla
      soils leave it to the plant), then the plant's own `mayPlaceOn`, else the bush's dirt or
      farmland. An upper half needs its lower half, except in the check made before the block is
      set. Kept for the plant classes that inherit these unchanged, as 1.21.1's sources show.
    - `BushBlock.mayPlaceOn` and `DoublePlantBlock.placeAt` (both halves, `copyWaterloggedFrom`).
    - A single dropped jar now gets the loader's convention biome tags too
      (`check::check_with`, `loader_biome_tags(<work>)`); only packs did before. Without them,
      FD's `#c:` modifiers find empty tags, and only cabbages and beetroots generate.
    Tests: `worldgen::tests::random_patch_configurations_read_as_vanillas`,
    `blocks::tests::a_mod_plants_super_survival_is_vanillas` (300 compat tests pass),
    `install::tests::a_dropped_jar_gets_the_loaders_biome_tags` (112 port-pack tests pass).
    Still unanswered at FD's load: `ItemAbility.CODEC`, `EnchantmentValueEffect.CODEC`,
    `SoundEvent.DIRECT_CODEC`.
  - [x] 2026-11-05 A mod adds to another mod's tags without taking its namespace. FD ships
    `data/create/tags/{item/upright_on_belt,block/brittle,block/fan_transparent,block/passive_boiler_heaters}.json`
    and names `create:` ids, so its single-jar install claimed `namespaces = ["create"]`, and the game
    would then refuse FD next to Create ("both use the namespace create"). NeoForge lets any data pack
    add to any tag, so the content fence now takes tag additions in every namespace
    (`data/<any>/tags/`), not only `minecraft` and `c`; the game's tag merge already joins them across
    mods in load order (a replace that drops another mod's entries warns). The installer carries them as
    shared tags (`carry::shared_tag`), leaves a namespace the jar only has tags in out of the claim
    candidates, and keeps `replace` in another mod's tag (only the mod's own tags lose it).
    Tests: `content_only_mod_round_trips`, `refuses_content_outside_its_fence` (31 horizon-mods tests
    pass), `claims_the_namespaces_its_files_name` (112 port-pack tests pass).
  - [x] 2026-11-05 Two codecs Farmer's Delight's cutting board recipes are read with, natively:
    `ItemAbility.CODEC` (`ItemAbilityIngredient`'s `action`: the ability by name, one object per
    name as `ItemAbility.get` gives) and `SoundEvent.DIRECT_CODEC` (`{sound_id, range}`, the range
    lenient, written back only when fixed). All 72 of FD's cutting recipes name an item ability and
    21 a sound, so without them the `farmersdelight:cutting` serializer's codec had nulls in it.
    Test: `codecs::tests::item_abilities_and_direct_sound_events_read_and_write_as_vanillas`
    (301 compat tests pass). `EnchantmentValueEffect.CODEC` (FD's backstabbing enchantment effect
    component) is still unanswered: it needs the game to read a mod's enchantment effect component.
  - [x] 2026-11-05 1.21.1's vanilla tags that 26.3 renamed, in a mod's code and in its data.
    Farmer's Delight's wild crops never generated. A trace of each placed feature FD's code hands
    over showed why: its patches put a crop where `matching_block_tag minecraft:dirt` holds below,
    and that was false on every grass block. 26.3 narrowed `#minecraft:dirt` to dirt, coarse dirt and
    rooted dirt; the set 1.21.1 called `dirt` (grass, podzol, mycelium, moss, mud too) is
    `substrate_overworld`, which takes `dirt` in. The same rename kept every mod plant that keeps
    `BushBlock.mayPlaceOn` (`BlockTags.DIRT` or farmland) off grass.
    Compared the two versions' vanilla tag data, each tag resolved to the blocks or items 1.21.1 has
    (1.21.1's from the CurseForge install's jar, 26.3's from the prepared resources). 10 block tags
    and `enchantable/sword` hold under another 26.3 name the set they held: `bamboo_plantable_on` is
    `supports_bamboo`, `mushroom_grow_block` `overrides_mushroom_light_requirement`,
    `dead_bush_may_place_on` `supports_dry_vegetation` (which lets farmland in too). `tall_flowers` is
    gone with nothing holding its set. `horizon_compat::vanilla_tags::CHANGED` lists them, each with
    where a mod's addition goes: there, the 26.3 tags that take the entry in are the ones that took
    it in under 1.21.1, renamed. So `dirt` additions stay in `dirt` and join `supports_big_dripleaf`
    (it lists its blocks where 1.21.1's `big_dripleaf_placeable` took `dirt` in); `enchantable/sword`'s
    go to `melee_weapon` (knockback, looting, sharpness' primary items) and `sweeping`;
    `tall_flowers`' stay and join `flowers` (and `bee_food`, for items).
    - Runtime: every `TagKey` the mod's code makes is the 26.3 tag (`fluids::tag_key`: `BlockTags`
      and `ItemTags` constants, `TagKey.create`, keys its codecs read), and so are
      `BushBlock.mayPlaceOn`'s dirt and the item tags ingredients name.
    - Installer: the mod's data names the 26.3 tags (`carry::renamed_tags`: `#` strings and `tag`
      fields in tag files, worldgen and recipes, and under `blocks` and `items` fields anywhere),
      and its additions to a renamed tag are written where 26.3 takes them in, merged where two meet
      (`carry::write_tag`). FD: `bamboo_plantable_on` to `supports_bamboo`, `mushroom_grow_block`
      to `overrides_mushroom_light_requirement`, `enchantable/sword` to `melee_weapon` and
      `sweeping`, `#minecraft:dirt` in `farmersdelight:terrain` and its wild crop patches to
      `substrate_overworld`.
    - Left: `dyeable`. 1.21.1 dyed any `#dyeable` item in the crafting grid; 26.3 dyes by one
      `crafting_dye` recipe per item and washes `cauldron_can_remove_dye` in a cauldron, so a mod's
      dyeable items need recipes written. Item `dirt`, `trim_templates` and the carver tags have no
      26.3 tag of their set.
    Tests: `vanilla_tags::tests::a_1_21_1_mods_vanilla_tags_hold_their_1_21_1_blocks`,
    `additions_to_renamed_tags_go_where_26_3_takes_them_in`,
    `blocks::tests::a_mod_plants_super_survival_is_vanillas` (now against 26.3's tags; 303 compat
    tests pass), `carry::tests::a_mods_vanilla_tags_are_26_3s` (113 port-pack tests pass).
  - [x] 2026-11-05 A mod's feature code places its inline placed features on the live level too:
    `/place feature farmersdelight:patch_wild_carrots`, a mod sapling's tree. Before, the live
    level answered "this world places no features". `LevelFeatureWorld::place_feature_json` reads
    the JSON with the live registry's catalog (`LiveFeatures::placed_json`, kept once per text) and
    places it from the stream the mod draws from. A feature that fails while decorating is now said
    once in the log (`generator_decoration::report_failure`); before, the failure only went to
    `DecorateStats`, which nothing reads, so a mod feature failing everywhere looked like a rare one.
    291 horizon-world tests pass.
  - [x] 2026-11-05 Vanilla's value providers, natively (`horizon-compat/src/value_providers.rs`):
    `IntProvider` (constant, uniform, biased to bottom, clamped, clamped normal, weighted list) and
    `FloatProvider` (constant, uniform, clamped normal, trapezoid), as 1.21.1 has them. A mod makes
    them in code (`UniformInt.of(2, 5)`) or reads them with `IntProvider.CODEC`,
    `NON_NEGATIVE_CODEC`, `POSITIVE_CODEC`, `IntProvider.codec(min, max)` and `FloatProvider`'s, in
    its feature configurations. `sample` draws from the random the mod holds (the generator's in
    worldgen) with vanilla's draws in vanilla's order, so the stream goes on where vanilla's would.
    `toString`, `getMinValue`/`getMaxValue`, `ConstantInt.ZERO` (`of(0)` is it), `UniformFloat.of`
    refusing an empty range. The codecs write back what they read (a constant as its number) and
    refuse with 1.21.1's messages (`Failed to parse either. First: Not a number: ...; Second: Max
    must be at least min, ...`, `Value provider too high: 10 [11-11]`). Checked against a Java
    oracle run on the vanilla Mojmapped 1.21.1 jar (`scratchpad/oracle-vp`: the installer's
    `client-1.21.1-…-srg.jar` plus `-extra.jar` for `Bootstrap`; NeoForge's patched classes can't
    bootstrap outside FML): three seeds of `LegacyRandomSource`, each provider's samples and the
    stream's next long after them, bit for bit, the clamped normal ones through
    `MarsagliaPolarGaussian`'s cached second value too. Tests:
    `value_providers::tests::{int,float}_providers_draw_as_vanillas`,
    `{int,float}_provider_codecs_read_and_write_as_vanillas`. 307 compat tests pass.
  - [x] 2026-11-05 A mod block's survival is its own code's, in the world generator and on the live
    level. Farmer's Delight's beach cabbages and beetroots never generated: FD's
    `WildCropBlock.mayPlaceOn` (it extends `FlowerBlock`) takes `BlockTags.DIRT || BlockTags.SAND`,
    but survival was the rule of the block's template (a vanilla plant: no sand), both where the
    generator decorates (`PlacementContext::block_survives`, the template's `SurvivalRule`) and on
    the live level (`Sim::can_survive`: the hooks `native_can_survive` and `native_may_place_on`
    existed, no translated mod answered them).
    - Runtime (`horizon-compat`): `blocks::mod_survives` is the block's own `canSurvive`, else its
      vanilla plant base's as NeoForge 1.21.1 has it (`BushBlock`'s, `DoublePlantBlock`'s: the
      soil's `canSustainPlant`, then the block's own `mayPlaceOn`). `blocks::survival_blocks` lists
      the mod's blocks it decides (own `canSurvive`, or a plant base kept); others stay their
      template's. A mod's code asking `state.canSurvive(level, pos)` gets the same answer. Entry
      points: `glue::can_survive` and `glue::may_place_on` (the level), `worldgen::survives` (the
      generator).
    - Generator (`horizon-world/src/worldgen/foreign.rs`): `Handler::survival_blocks` and
      `Handler::survives`. The decoration world asks the handler first, for the states of the
      blocks it claims only: a bitset per registry, built on the first question; one atomic load
      when no mod claims any. The mod's code reads the region and writes nothing.
    - Translator: the engine's worldgen handler answers on the worldgen runtime, or, while the mod's
      own feature is in a call on that thread (FD's `WildCropFeature` places the inline
      `simple_block` that asks), on that thread's probe runtime (`with_worldgen_query`). The level
      hooks `can_survive` and `may_place_on` are wired. They are asked about every block, so they
      pass over the others by name: `GameLevel::native_state_block_name` (new, fragment
      `state_block_name`) reads a state's block without building its text.
    Tests: `blocks::tests::a_mod_blocks_survival_is_its_own_codes` (a wild crop on sand, grass and
    stone through `BlockTags.DIRT`'s 26.3 name; a block with its own `canSurvive`; one with
    neither left to its template; the claims), `foreign::tests::a_mods_block_survives_where_its_code_says`.
    308 compat and 292 horizon-world tests pass.
  - [x] 2026-11-05 A block copying a vanilla block's properties gets them; a block nothing collides
    with lets things through; a mod's own `waterlogged=true` holds water. Farmer's Delight's rice
    drew black: `Properties.ofFullCopy(Blocks.TALL_GRASS)` named a block the runtime's `COPIED`
    table (14 blocks) didn't know, so the declaration kept its stone template's occlusion,
    hardness, tool and collision. The pack's mods copy 358 distinct vanilla blocks
    (`scratchpad/copycensus.py`: `getstatic Blocks.X` then `ofFullCopy`/`ofLegacyCopy`).
    - Declarations (`horizon-mods`): a block's properties take `copy` (a `minecraft:` block, the one
      `ofFullCopy` names) and `collision` (`false`: `noCollission()`).
    - Runtime (`horizon-compat`): `block_declaration` names the copy and a `noCollission()`.
      `Blocks.X` and `Items.X` give their 26.3 ids (`api::field_id`). Checked against every static
      field of 1.21.1's two classes (1060 blocks, 1333 items): four differ, `CHAIN` (26.3's
      `iron_chain`), `POTTED_AZALEA` and `POTTED_FLOWERING_AZALEA` (`*_bush`) and
      `Items.CUT_STANDSTONE_SLAB` (`cut_sandstone_slab`).
    - Content root (`mod_content.rs`): the copied block's values carry over under the block's own.
      Gameplay rows: hardness, resistance, tool, friction, speed and jump factors, map colour,
      sound, light, lava, push, replaceable, random ticks, offset. A copy of a block that doesn't
      occlude hides nothing behind it and lets light through. Also worldgen's definition flags
      (`block-properties.nbt`), the movement factors (`movement-properties.nbt`; a bounce stays
      the template class's) and the note block instrument. No collision: the empty shape in both
      collision caches, an open row for mobs (water where it holds water), no sturdy face, not
      solid, no conductor, no suffocation, `full_collision` false. A copy without its own
      `collision` collides as the copied block does. `waterlogged=true`: the fluid flag in
      `facts.json` and worldgen's `block_flags.json`, a water source in `block-behaviours.json`,
      light dimmed as water dims it.
    Tests: `mod_content::tests::a_block_copying_a_vanilla_blocks_properties_takes_the_games_rows_for_them`,
    `content::tests::blocks_copy_a_vanilla_blocks_properties_and_may_not_collide`,
    `blocks::tests::copied_properties_take_the_vanilla_blocks_values` (a tall grass copy, a
    `noCollission()` reed), the field ids in `api_common`'s runtime test.
  - [x] 2026-11-05 A translated mod keeps the recipes for items its code declares at start. The
    installer moved a recipe whose result was a mod item it didn't declare to `outside/`: for
    Farmer's Delight 24, among them the cooking pot, skillet, rope, rice, tatami and the pies,
    whose blocks only the translated code declares. The game now does what vanilla does with a
    recipe for an unknown item (`ItemStackTemplate.CODEC` reads the id through the item
    registry, so the file doesn't load): `crafting::result_known` leaves it out of crafting,
    cooking, smithing and the stonecutter. The installer keeps such recipes and lists them
    (`recipes_unregistered`). An item table of tags alone (`Items::tags_only`) calls nothing
    unknown.
    Tests: `crafting::tests::a_recipe_for_an_unknown_item_is_not_loaded`,
    `check::tests::recipes_for_left_out_blocks_are_left_out_too` (the file stays).
  - [x] 2026-11-05 A mod's dyeable items dye in the crafting grid. 1.21.1 dyes anything in
    `#minecraft:dyeable` (`ArmorDyeRecipe`); 26.3 has no such tag and gives each dyeable item a
    `crafting_dye` recipe (`leather_helmet_dyed`). The installer writes that recipe for each item
    the mod adds to the tag (`dye_recipes`), one marked `required: false` too: the game leaves it
    out while its item is unknown. Test: `carry::tests::a_mods_vanilla_tags_are_26_3s`.
    113 port-pack, 54 crafting, content and station tests pass.
  - [x] 2026-11-05 `/recipe` and the recipe book know a content mod's recipes. The recipe index
    (`RecipeIndex::load`, which `/recipe` resolves and suggests from) and the server's recipe list
    for its clients (`lan::recipes`, also the advancements' recipe displays) read only
    `data/minecraft/recipe`: in game `/recipe give @s farmersdelight:cooking_pot` said "Unknown
    recipe" while the pot crafted. They read every content namespace now
    (`crafting::root_recipe_files`), leave out a recipe for an item the game doesn't have, as
    crafting does, and list the stonecutter's recipes in the registry's order
    (`crafting::identifier_order`: the path, then the namespace, as `Identifier.compareTo`).
    Tests: `recipe_book::tests::the_index_holds_the_content_mods_recipes`,
    `crafting::tests::recipe_ids_sort_by_path_then_namespace`.
  - [x] 2026-11-05 A mod's configs load as NeoForge loads them (`neo::register_config`).
    `ModContainer.registerConfig(type, spec[, file])` and `ModLoadingContext`'s keep a `ModConfig`
    (its type, spec, mod id, and file, `<modid>-<type>.toml` unless named). A `STARTUP` config
    loads at once; the others after the registries, before common setup, in the order registered.
    Each loading posts `ModConfigEvent.Loading` with its config, which mods hear to copy the values
    into their fields (the MDK's `Config.onLoad`). The values are the spec's defaults: Horizon keeps
    no config files for translated mods. A server's configs load with the rest (NeoForge loads them
    as its server starts). `ModConfigSpec.isLoaded` is true.
    Test: `neo::tests::a_mods_configs_load_after_the_registries`.
  - [x] 2026-11-05 A mod's crops grow (`crops.rs`). In game Farmer's Delight's cabbages, onions
    and budding tomatoes stayed at age 0 on wet farmland at random tick speed 300 while the wheat
    beside them grew. A mod crop without a `randomTick` of its own never ran `CropBlock`'s, and the
    tomatoes' own growth asked three things the runtime didn't answer. A run with
    `HORIZON_COMPAT_TRACE` named them: `BlockState.canSustainPlant`, `BlockState.isFertile` and
    `CommonHooks.canCropGrow`, whose missing answer (false) stopped every growth.
    - `CropBlock`'s methods as NeoForge 1.21.1 has them, for a crop that inherits them:
      `randomTick` (light 9, then a draw of 1 in `25 / speed + 1`), `getGrowthSpeed` (the soil's
      say, wet farmland, rows), `getAge`, `getStateForAge`, `isMaxAge`, `isRandomlyTicking`,
      `hasSufficientLight`, and bone meal's `growCrops`, `getBonemealAgeIncrease`,
      `isValidBonemealTarget`, `performBonemeal`. Each calls the crop's own overrides where its
      class has them: the rice panicles count to 3 with their `AGE_3`. A mod block whose first
      vanilla class is `CropBlock` (or carrots', potatoes') and has no `randomTick` of its own runs
      it on the game's random tick, in native code; it keeps its own shapes and template.
    - NeoForge's `canCropGrow` and `fireCropGrowPost` post `CropGrowEvent.Pre` and `Post`
      (`BlockEvent`'s getters). A listener's `GROW` or `DO_NOT_GROW` decides; with no listener,
      the crop's own draw does.
    - The soil: `canSustainPlant` is a mod soil's own answer, else no decision (`DEFAULT`; none of
      vanilla's soils decide); `isFertile` a mod soil's own, else a `FarmBlock` while wet.
    Tests: `crops::tests::a_mod_crop_without_a_random_tick_grows_as_vanillas`,
    `the_growth_speed_counts_the_soil_and_the_rows`,
    `a_mods_own_growth_asks_neoforges_hooks_and_the_soil`. 312 compat, 206 client tests pass.
    In game (Farmer's Delight alone, wet farmland, 200 ticks at random tick speed 300): cabbages
    and onions 0 to 3, budding tomatoes 0 to 1 (their own slower draw), wheat and carrots beside
    them 0 to 4. `/recipe give` unlocks the cooking pot, skillet, rope and tatami; a player walks
    through wild cabbages and is stopped by stone.
  - [x] 2026-11-05 Bone meal on a translated mod's blocks. The game had three bone meal hooks for
    mod blocks (API 72) that the bundle didn't wire; it does now (`is_valid_bonemeal_target`,
    `is_bonemeal_success`, `perform_bonemeal`, through `glue`'s functions of those names). A
    block takes bone meal by its own `BonemealableBlock` methods, or, for a crop that inherits
    them, `CropBlock`'s (`crops.rs`): while short of its last age, growing 2 to 5 ages
    (`getBonemealAgeIncrease`, the crop's own where it has one: the rice panicles' third of it),
    up to its last. Vanilla asks the first question on both sides, so the level is only read
    there. The block hooks' hosts are one text each now (`level_host` and `level_read_host` in
    `bundle.rs`). Test: `crops::tests::bone_meal_grows_a_mod_crop_as_vanillas`.
    In game (`fdbm.sh`, random ticks off): bone meal on Farmer's Delight's cabbages takes them
    from age 0 to 3 (wheat beside them 0 to 3); on rice, its own `performBonemeal` grows it
    0 to 1, then to 3 with its panicles above at 3 after three more. Ripe rice left to random
    ticks (speed 1000, 200 ticks) grows its panicles by its own tick (to age 2). The ctl's
    `interact` can't aim at a crop's low shape (it aims at the block's upper half); `look-at` a
    point inside the shape and `mouse right click` do.
  - [x] 2026-11-05 Rice grows its panicles. In the same run the rice reached age 3 and stopped:
    its random tick grows the panicles above only into an empty block, and
    `LevelReader.isEmptyBlock` had no answer (false). It is the block there being air of any kind
    now. The tick's first line, `super.tick(...)` to `BushBlock`, was unmapped too: vanilla's
    scheduled tick there does nothing, and so does a `super.tick` to any of the 189 block classes
    of 1.21.1 whose class files declare no tick down to `BlockBehaviour` (`DEFAULT_TICK_BLOCKS`
    in `blocks.rs`, read from the 1.21.1 classes); one to a class with its own (a falling
    block's) stays unanswered. Test: `blocks::tests::a_rices_tick_and_its_empty_block_test_are_vanillas`.
    314 compat, 15 translator tests pass.
  - [x] 2026-11-05 NeoForge's finish phase (`neo::fire_registrations`). Loading stopped after the
    capabilities: no mod heard `InterModEnqueueEvent` (21 of the pack's mods listen),
    `InterModProcessEvent` (11) or `FMLLoadCompleteEvent` (33), and the payload types registered
    before the capabilities. The order is NeoForge 21.1's `CommonModLoader` now: common setup,
    "Registration events" (capabilities), "Enqueue IMC", "Process IMC", "Complete loading", then
    "Network registry lock" (`RegisterPayloadHandlersEvent`). The client's own setup still fires on
    the drawing runtime's first hook, after these (NeoForge's comes before them).
    - `InterModComms`: `sendTo([sender,] modId, method, supplier)` queues a message for a loaded mod
      and answers false for another; `getMessages(modId[, filter])` and a lifecycle event's
      `getIMCStream([filter])` take the mod's messages off its queue in the order they were sent,
      leaving the ones the method filter refuses; `IMCMessage`'s accessors. A lifecycle event's
      `getContainer` is the runtime's mod. Messages reach the mods of the same engine only: a mod
      in another translated jar isn't loaded as `ModList` sees it here.
    Test: `neo::tests::the_mods_message_each_other_then_complete_loading`.
  - [x] 2026-11-05 `ItemStack.canPerformAction(ability)`, unmapped in the same run: Farmer's
    Delight asks it of the held stack to tell a knife (`ItemUtils.isKnife`, from its cake
    listener on every right click on a block). It is the item's `canPerformAction(stack,
    ability)`: a mod item's own, else the abilities of the item's vanilla class as NeoForge
    1.21.1's patched classes give them (`ItemAbilities.DEFAULT_*_ACTIONS` in the 13 classes whose
    class files declare it: axes, hoes, shovels, pickaxes, swords, shears, shields, fishing rods,
    tridents, brushes, flint and steel, fire charges, spyglasses). A vanilla item is of its class
    by its name; a mod item by its first vanilla class. Test:
    `neo::tests::a_stacks_abilities_are_its_items`. 316 compat tests pass.
  - [x] 2026-11-05 The server's lifecycle. Mods heard none of NeoForge's server events, though 29
    of the pack's mods listen to `ServerAboutToStartEvent`, 21 to `ServerStartingEvent`, 22 to
    `ServerStartedEvent`, 23 to `ServerStoppingEvent`, 24 to `ServerStoppedEvent` and 26 to
    `LevelEvent.Unload`. The game's server hooks post them now, in NeoForge's order:
    `native_server_about_to_start` gives `ServerAboutToStartEvent`, before anything loads (the
    level's `LevelEvent.Load` comes after); the world open (`native_world_lifecycle`,
    `open_ready`) gives `ServerStartingEvent`, then `ServerStartedEvent`;
    `native_server_stopping` gives `ServerStoppingEvent`, the worlds still loaded; the world
    closed or failed to open gives the level's `LevelEvent.Unload`, then `ServerStoppedEvent`.
    Each event's `getServer()` is the server; `ServerLifecycleHooks.getCurrentServer()` is it from
    `ServerAboutToStartEvent` until it stopped, null before and after (the next world has a
    server and level of its own). Test: `neo::tests::the_server_starts_and_stops_as_neoforge_posts_it`.
    In game (Create and Farmer's Delight, traced): `ServerAboutToStartEvent` reaches 1 listener,
    `ServerStoppingEvent` 1 and `LevelEvent.Unload` 4 (these two mods have none for the others),
    and the world closes with no error and no unmapped call.
  - [x] 2026-11-05 `IEventBus.unregister(x)`, unmapped 4 times in the same run: Create's
    `OneTimeEventReceiver` unregisters itself after its one event. What `register(x)` or
    `addListener(x)` added stops listening, as NeoForge's bus keys it: the listener itself, the
    `@SubscribeEvent` methods of an object (its class's and its superclasses', bound to it), or a
    class's static ones; another object of the same class still hears. Test:
    `neo::tests::unregistered_listeners_stop_hearing`.
  - [x] 2026-11-05 Mods' commands (`crate::commands`). 82 of the pack's mods register Brigadier
    commands (69 through `RegisterCommandsEvent.getDispatcher()`); none could run.
    - The tree: Brigadier's builders (`Commands.literal`/`argument`, `then`, `executes`,
      `requires`, `redirect`/`fork`, `suggests`, `build`) make nodes, and the dispatcher merges each
      root it registers as Brigadier does (one registered again merges its children: Create's
      `/create`, its `/c` shortcut).
    - Registering: when a world opens to players (`native_register_commands`), the server's
      runtime posts `RegisterCommandsEvent` with a new dispatcher, and every runnable path of its
      tree becomes a native usage with the operator level its `requires` needs (the predicates
      asked at levels 0 to 4 with a silent source; one no level passes gets 4). A path with a
      literal that isn't a command word, or an argument type the runtime doesn't know (a mod's
      own `ArgumentType`), is left out, traced. Players get the usages after vanilla's tree, so
      the chat box completes and colours them.
    - Running (`native_perform_command`; the console's `native_perform_console_command`): the line
      is parsed as Brigadier parses it (a literal the next word names is the only way on, else
      each argument the source may use, in order, backtracking; a redirect goes on at its target
      with its modifier's sources), then the command runs with a `CommandContext` for each source.
      A `CommandSyntaxException` it throws is red feedback, anything else vanilla's `command.failed`.
    - Arguments, as vanilla's parsers read them: Brigadier's numbers with their bounds, booleans,
      words, strings (quoted, with escapes) and the rest of the line; messages; entity selectors
      (`@s @p @a @r @e @n`, names, UUIDs; `name=` and `limit=`; players only); block, column and
      world positions (`~`, `^`); `vec2`, rotations, angles; ids and dimensions; items and block
      states (with `[...]` and `{...}`); times (`d`, `s`, `t`); UUIDs; colours. Their getters
      (`IntegerArgumentType.getInteger`, `BlockPosArgument.getLoadedBlockPos`,
      `EntityArgument.getPlayer`...) read the context, with vanilla's errors (no such player, a
      dimension the source isn't in).
    - `CommandSourceStack`: position, rotation, level, server, the player or entity (else the
      syntax error `permissions.requires.player`), `hasPermission`, `sendSuccess`, `sendFailure`
      and `sendSystemMessage` (to whoever ran the command: chat for a player, the console's
      output), the `with*` copies, and one a mod makes itself. It, `BlockInput`, `ItemInput` and
      `WorldCoordinates` left the data-class table (`vanilla_records.rs`, 659 classes).
    - The game's native usages take vanilla's other argument types (`string`, `long`, `message`,
      `players`, `entity`, `entities`, `game_profile`, `block`, `column_pos`, `vec3`, `vec2`,
      `rotation`, `angle`, `id`, `dimension`, `uuid`, `color`, `time`), and words with capitals
      and dots (Create's `clearRenderBuffers`) ([native-mods.md](native-mods.md)).
    Tests: `commands::tests::a_mods_commands_register_and_run_as_brigadier_parses_them`,
    `registering_a_root_again_merges_its_children`, `arguments_read_as_vanillas_parsers`.
    In game (`fdcmd.sh`: Create and Farmer's Delight, the owner's console at level 4):
    `/create trains` and `/c trains` print Create's train summary, `/create clone` alone its
    help, and a line no path reads answers red.
  - [x] 2026-11-05 `/create clone` copies blocks. It ran but copied 1 block where it should copy 18:
    `BoundingBox` and `BlockInWorld` had no model. `BoundingBox` is 1.21.1's now (`shapes.rs`):
    mutable as vanilla's, corners in any order, sizes counting both corners in Java's wrapping
    arithmetic, `orientBox` for each facing, `intersects`, `isInside`, `encapsulate`, `move`,
    `moved`, `inflatedBy`, `getCenter`, `getLength`, `equals`, `hashCode` and the toString
    (24 of the pack's mods use it). `BlockInWorld` reads its level's block (a mod's own level
    too) once, when first asked; `Clearable.tryClear` empties a container by its `clearContent`;
    `LevelTicks.copyArea` copies no queued tick, as the game's own `/clone` doesn't. Tests:
    `shapes::tests::a_bounding_box_is_vanillas`, `level_env::tests::a_block_in_world_reads_its_level_once`.
    In game: a 3x2x3 box of planks with glowstone inside copies 7 blocks east ("Successfully cloned
    18 Blocks"), its corners, the glowstone and the air beside it as they were.
    Found in the same runs and modelled (tests only so far; the next game build checks them):
    `Dynamic2CommandExceptionType` (and 3, 4, N: 6 mods; Create's "too many blocks" answered
    `command.failed`), `new CommandSyntaxException(...)`, its `getMessage`/`getContext`/`getInput`/
    `getCursor`/`toString` as Brigadier writes them, `LiteralMessage`; Guava's `Lists.reverse`
    (the clone's barrier step was skipped) and `Lists.transform`; `SharedConstants.getCurrentVersion()`
    as 1.21.1's `version.json` (23 mods: name, protocol 767, pack formats 34 and 48, data version
    3955); `Objects.toString` (14 mods); `Stream.generate` until its `limit`;
    `ModList.forEachModContainer`, `forEachModInOrder`, `applyForEachModContainer`, `getSortedMods`.
    Tests: `api_common::tests::command_exceptions_say_what_brigadiers_do`,
    `the_game_is_1_21_1_to_its_mods`, `jutil::tests::objects_print_and_generated_streams_stop_at_their_limit`,
    `neo::tests::the_mod_list_walks_its_mods_in_order`. 327 compat, 15 translator and the game's 7
    `native_server` tests pass.
    Not yet: client commands (`RegisterClientCommandsEvent`, 19 mods: Create's `/create
    rainbowDebug`...), `Commands.performPrefixedCommand` (14 mods), mods' own argument types (16
    mods, 28 classes: catnip's `ConfigPathArgument` leaves `/create config` out), suggestion
    providers (the chat box completes by kind), selector options past `name` and `limit`,
    entities other than players, and vanilla's parse error with its context line for a line no
    path reads (it answers as an unknown command). `/create debuginfo` reaches its client-side
    parts (`GlUtil`, `Options.graphicsMode`) and `SystemReport`, unmodelled.
  - [x] 2026-11-05 Commands parse and run as Brigadier 1.3.10 and vanilla 1.21.1 do, mods' own
    argument types included. Three modules now:
    - `brigadier.rs`: `StringReader` over the line's UTF-16 units, each read failing with
      1.21.1's error where Brigadier leaves the cursor (`parsing.int.expected`,
      `parsing.quote.escape`...); a reader object (one a mod makes, or the one its parser is
      handed) runs on it. The exception types, `CommandSyntaxException` with or without a reader's
      place, and `BUILT_IN_EXCEPTIONS` (1.21.1's `BrigadierExceptions`: one type object per key, so
      a mod comparing `getType()` finds it).
    - `command_args.rs` (new): vanilla's argument parsers on that reader, each with its errors at
      vanilla's cursor: numbers with bounds (`argument.integer.big` [64, 65]), strings, messages,
      entity selectors as `EntitySelectorParser` reads them (every 1.21.1 option and its errors;
      selection by name, UUID, `x y z`, `dx dy dz`, distance, sort, limit; `@` needs level 2 at
      parse and at selection), world and local coordinates (`Mth`'s float sin/cos for `^`),
      angles, ids, dimensions, items (`[components]`, overstacked), block states (`[props]`,
      `{nbt}` checked as `TagParser` reads SNBT), times, UUIDs (`UUID.fromString`'s leniency),
      colours. What a type reads resolves later with the source the command runs for.
    - `commands.rs`: `parseNodes` as Brigadier's (relevant nodes, `canUse`, a copy of the
      context and reader per child, the separator, a redirect parsed in a context of its own,
      potentials ordered by what they read and their errors); a mod's own `ArgumentType` reads
      with its own `parse` (the one taking the source when it overrides it), a `RuntimeException`
      it throws being `command.exception`. `Commands.performCommand` as NeoForge's: `CommandEvent`
      first (cancelled, its exception said; architectury, balm and KubeJS listen), then
      `finishParsing` (the error, then the line's last ten characters before its place in grey,
      the rest red and underlined, `<--[HERE]`, clicking it suggesting the line), then
      `ExecutionContext`'s run: each redirect's modifier gives the sources (`redirect`'s
      single modifier wrapped as Brigadier's), a fork's syntax errors silent, a modifier's ending
      the command, the fork limit; the command per source, its syntax errors said to that source,
      anything else `command.failed` with the message on hover. A mod's own
      `dispatcher.parse`/`execute` (2 mods each) gets `ParseResults`, `CommandContextBuilder`,
      `ParsedCommandNode`, `StringRange`, and Brigadier's `executeAll` (forks count 1 each, errors
      thrown out unless forked). `EntitySelectorParser` on a mod's reader (FTB Teams'
      `TeamArgument`, KubeJS, FancyMenu: 3 mods). A mod's own type is shown in the chat box as what
      its `getExamples` look like (words, ids, else strings); its parser reads on the server.
    `LocalCoordinates` and `AngleArgument$SingleAngle` left the data-class table (657 classes).
    Tests: `brigadier::tests::*` (3), `command_args::tests::selectors_read_and_fail_as_vanillas_parser`,
    `coordinates_read_and_resolve_as_vanillas`, `the_other_types_read_and_fail_as_vanillas`,
    `commands::tests::a_mods_commands_register_and_run_as_brigadier_parses_them` (vanilla's
    errors and context lines, a mod's own colour type with its own error),
    `redirects_run_their_modifier_for_each_source`, `a_mods_parser_reads_selectors_with_vanillas`.
    333 compat tests pass. Not yet: client commands, `Commands.performPrefixedCommand`, suggestion
    providers, entities other than players in selectors.
    In game (FD + Create bundle, `scratchpad/fdcmd.sh`): `/create trains` and its alias `/c trains`,
    `/create clone` (usage line, a 3x2x3 copy, `commands.clone.toobig`, relative `~`), a line no
    path reads (`argument.pos3d.incomplete` plus the context line), an int too big
    (`parsing.int.invalid`, as vanilla's `readInt`), an unknown subcommand
    (`command.unknown.argument`) all answer as vanilla's. `/catnip config client` failed with
    `command.expected.separator`: catnip's `ConfigPathArgument.parse` reads over
    `ResourceLocation.isAllowedInResourceLocation`, which nothing mapped (so it read nothing).
- [x] 2026-11-05 Ids are checked, read and keyed as 1.21.1's `ResourceLocation` does (`neo.rs`): the
  character checks (`isAllowedInResourceLocation` 6 mods, `validPathChar` 7, `validNamespaceChar`,
  `isValidPath` 5, `isValidNamespace` 6); `parse` and `bySeparator` throw `ResourceLocationException`
  with vanilla's message for a character an id can't have, `tryParse`/`tryBySeparator` give null
  (they returned the string as it was), `read(String)` an error `DataResult` (7); the separator
  given to `bySeparator` is used (it was ignored); `read`/`readNonEmpty(StringReader)` (6) read a
  command line's id with `argument.id.invalid` where it began; `compareTo` sorts by path, then
  namespace (11), `compareNamespaced` (5); `toDebugFileName` (6); `toLanguageKey(type[, key])` and
  `toShortLanguageKey` (dropped `minecraft.`) make vanilla's keys (they ignored their arguments).
  `jlang::java_compare` is `String.compareTo` for both. Test:
  `neo::tests::ids_are_checked_read_compared_and_keyed_as_vanilla_does`. 334 compat tests pass.
  Still the string's: `hashCode` and a sort through `Comparable` (ids are strings in the runtime).
- [x] 2026-11-05 Mods run command lines as `Commands.performPrefixedCommand` does (14 mods: Create's
  `HighlightCommand`, AE2's spatial storage, Carry On, FramedBlocks' signs, MCreator procedures...),
  with the sources they make: `server.getCommands()` (20), the server's own source
  `createCommandSourceStack()` (10: level 4, at the world's spawn in the overworld, what it is
  told goes to the server's log, never to the chat of the player whose command is running; on the
  console it is the console's answer), a player's own `createCommandSourceStack()` (9: where they
  stand and look, at their operator level), `performCommand(results, line)` for results a mod
  parsed. A line one of the bundle's roots begins runs on the mods' tree with that source; any
  other goes to the host, `Host::run_command(CommandRun)` (line, place or the server's, look,
  level, player, silence) → the game's new `native_run_command_as` (API fragment
  `run_command_as`): vanilla's commands, or another bundle's registered one for the player in
  place (`perform_native_command`), at the source's level. A player not in place runs it in their
  own command turn (`guest_command_turn`, taken out of `run_guest_command`: their state swapped in,
  the hub back in place, their lines and admin feedback sent after), so its answer reaches them;
  inside a hook only the acting player, or no entity, can be the source. `Host::player_permission`
  gives a player's level (`native_acting_player`, `native_server_players`). Test:
  `commands::tests::a_mods_lines_run_as_their_source` (the mods' own line as the player and as the
  server, whose failure goes to the log, then to the console with no player in place; vanilla
  lines reaching the host with each source's place, look, level, player and silence; a parsed
  results object). 335 compat, 15 translator, 16 game (`native_server`, `lan_actions`) tests pass.
  Another entity's `createCommandSourceStack` stays unmapped.
- [x] 2026-11-05 `java.text.BreakIterator` (`breaks.rs`), and a stop for hooks stuck on what isn't
  translated. Found in game: `/catnip config client` (working since the ids above) opens catnip's
  config screen, which wraps its text with a line iterator; `next()` was unmapped, answered 0 and
  never `DONE`, and the screen's loop hung the client. The pack's users: Create's `TooltipHelper`
  and catnip's `FontHelper`/`ClientFontHelper` (item summaries), guideme's `LineBuilder`
  (`preceding`), the Lucene copies in AE2's guide and Ars Nouveau (word, sentence, `isBoundary`,
  `setText(CharacterIterator)`). Line, word and sentence iterators follow JDK 21's own rule sets
  (`sun.text.resources.BreakIteratorRules`: line and word as a state machine over the rules'
  character classes, longest match, format characters passed over; sentences by their lazy rules
  with the JDK's lookahead point), characters the extended grapheme clusters of Unicode 15.0 (JDK
  21's). An iterator finds its text's boundaries once, at `setText`, and walks them (`first`,
  `last`, `next`, `next(n)`, `previous`, `following`, `preceding`, `isBoundary`, `current`, `clone`,
  `getText`; vanilla's `IllegalArgumentException` for an offset off the text); a
  `CharacterIterator` text keeps its own indexes (a mod's is read through its methods), and
  `StringCharacterIterator` is mapped. The expected boundaries are the JDK's own output:
  `tools/jdk_break_iterator_oracle.py --diff` runs the test's 194 cases through the JDK given and
  prints the rows it breaks otherwise (0 with JDK 21; JDK 25 differs on one Devanagari conjunct,
  Unicode 15.1). The guard: an unmapped call answers a stand-in, so any loop waiting for another
  answer never ends. `api::call_nested` counts the calls nothing maps in the running hook (the
  outermost `Rt::with_host`, which resets the count); past a million over ten seconds each throws
  `java.lang.Error` naming the call, which `catch (Exception e)` doesn't stop, so the hook unwinds
  and the game goes on. Code outside hooks (loading) is never stopped; with `HORIZON_COMPAT_TRACE`
  a hook that made over 100,000 such calls says so, to measure how close real hooks come. Tests:
  `breaks::tests::text_breaks_where_the_jdks_iterators_break_it` (194 rows: lines, words,
  sentences, graphemes), `an_iterator_walks_its_boundaries_as_the_jdks_does`,
  `text_given_as_a_character_iterator_breaks_in_its_indexes`,
  `api::tests::a_hook_looping_on_an_unmapped_call_is_stopped`. 339 compat, 15 translator tests
  pass. Still unmapped on that screen: `ModConfigs.getModConfigs`, `ModConfigSpec.getValues` (its
  list of settings).
- [x] 2026-11-05 Client commands, as NeoForge's `ClientCommandHandler` runs them
  (`RegisterClientCommandsEvent`, 19 mods: Flywheel's `/flywheel`, JEI, Xaero's, catnip...). The
  window's game asks the bundle through its existing hooks (API 35): `native_commands` (when a
  connection starts and when the server sends its tree) posts the event on a new tree of the
  client's own, apart from the server's (`commands::register_client`), and gives back its roots so
  the chat box colours and completes them; `native_command` (a typed or clicked line, before the
  server sees it) runs a line one of those roots begins on that tree (`commands::perform_client`).
  Brigadier's unknown command or unknown argument goes on to the server, as NeoForge's does (so
  `/create trains` still reaches the server's `/create`); any other syntax error is answered in the
  window's chat (red message, then the context line, which puts the line back without its slash
  when clicked), another exception as `command.failed`. The source is
  `ClientCommandSourceStack`'s: the local player's name, operator level and place, its entity the
  local player (no server's player); `getServer`/`getLevel` throw vanilla's
  `UnsupportedOperationException` ("Attempted to get server in client command").
  `ClientCommandHandler.getDispatcher`, `getSource`, `runCommand` are mapped too. New:
  `Host::client_player`, the game's `native_client_player` (API fragment `client_player`: name and
  `LocalPlayer.getPermissionLevel` as the server last said it), and the client host's
  `command_feedback` (a line in the window's chat). Also fixed: five lines in hook points of
  `native_hooks.rs` (`native_state_id_text`, `native_state_text_at`, `native_resource_paths`, from
  earlier commits on this branch) used `format!`/`vec!`, which horizon-mixins refuses in a hook
  point's body; `hook_points_stay_hookable` passes again. Test:
  `commands::tests::client_commands_run_on_the_clients_tree_and_leave_the_rest_to_the_server`
  (a `/glass` mod: its root only on the client's tree, `who` sees the local player as the source,
  an unknown subcommand and other roots left to the server, an out-of-range argument answered
  with its context, `getServer` → `command.failed`). 340 compat, 15 translator, 36 game hook tests
  pass.
- [x] 2026-11-05 Client commands in game; mods' key mappings; NeoForge's raw input events; the look
  of the mods a jar bundles (jar-in-jar).
  - Client commands, checked in game (`scratchpad/fdcmd.sh`): Create's `/create util
    clearRenderBuffers` answers "Cleared rendering buffers.", its `/c` shortcut too,
    `/create rainbowDebug` and Flywheel's `/flywheel limitUpdates`, `/flywheel backend` answer on
    the client, a bad float is answered there with its context line, `/create trains` (not on the
    client's tree) still reaches the server; the trace lists the client roots `flywheel, create, c`.
    It took: horizon-ctl's `chat` on a game with a server (the default) now goes through
    `handle_chat_input` as a typed line does (it sent the line to the server, past the client's
    commands), and the client host's `command_feedback` draws the whole component
    (`horizon_client::component::to_text`: translations, siblings, styles), not its first `text`.
    Typed in the chat box (t, `/flywheel limitUpdates`, Enter) the line runs on the client too.
  - Key mappings (`RegisterKeyMappingsEvent`, 60 mods of the pack register some; new
    `horizon-compat/src/keys.rs`): `KeyMapping` (both constructors with a context and a modifier,
    `consumeClick`, `isDown`, `same`, `getKey`, `setKey`, `getTranslatedKeyMessage`, `matches`,
    `matchesMouse`...), `KeyModifier` (`SHIFT`, `CONTROL`, `ALT`, `NONE`, `isActive`), the contexts
    (`KeyConflictContext.IN_GAME`, `GUI`, `UNIVERSAL`: `isActive`, `conflicts`), and the event is
    posted where `ClientHooks.initClientHooks` posts it (after the renderers, before
    `FMLClientSetupEvent`). A key goes down with nothing over the world: the mappings on it count a
    click (`consumeClick` in the mod's client tick), as `KeyMappingLookup.getAll` finds them (with
    a modifier held that isn't the key itself, those made with that modifier, else those with
    none). New host calls: `mod_key` (the player's key
    for a mod's mapping, by its name, else its default) and `playing` (no screen or menu over the
    world: `IN_GAME`'s `isActive`); the bundle's `key` helper gives the mods the window's keys, and
    `mods` puts the translated mods' row on Options > Mods (title: the bundle's own mod first, its
    version, `+ N` for the others; description: all of them) with the mappings as its keys page.
    `LocalPlayer.displayClientMessage` reaches the window (`Host::player_message`: the chat, or
    over the hotbar). In game, hzcheck's two mappings on K (alone and with `KeyModifier.SHIFT`):
    K → "hzcheck: ping on K", SHIFT + K → "hzcheck: shifted on SHIFT + K" (not ping), K with the
    chat open → nothing; the Mods page has "Create 6.0.10 + 3" with a Settings button, whose page
    lists the bundle's mappings by their translated names and keys ("Access Nearby Toolboxes: Left
    Alt", "Open Block Rotation Menu: Not Bound", Ponder's "Ponder: W"...).
  - Raw input (`InputEvent`, new `horizon-compat/src/input.rs`): `Key` (20 mods listen),
    `MouseButton.Pre` (cancellable: the game then does nothing with the press, no `Post`),
    `MouseButton.Post` (7 mods with `Pre`), `MouseScrollingEvent` (15 mods; cancelled, the hotbar
    stays), from the game's existing hooks `native_input_key` (after the window handled a key, on
    any screen, repeats too), `native_mouse_button_pre`/`_post` and `native_mouse_scrolling`, with
    GLFW's key codes, actions and modifier bits; no mod hears input before the client's setup.
    Test `input::tests::input_reaches_the_mods_as_neoforges_input_events`. In game (hzcheck's
    listeners, `fdcmd.sh`): the wheel with a stick in hand → "hzcheck: scroll 1.0 kept from the
    hotbar" and the hotbar stays on slot 1; the right button → "right button kept from the game";
    the left button → "left button after the game" (its `Post`); with nothing in hand the wheel
    moves the hotbar (1 → 9). horizon-ctl's keys aren't window key events, so `InputEvent.Key` is
    checked by the unit test alone.
  - The mods a jar bundles for jar-in-jar (Create's Flywheel and Ponder) now get their look:
    `write_group` writes `content-nested-<id>` for each nested jar that declares a mod (Registrate
    declares none), and `install` puts it in `horizon-mods/<id>`; their names and versions (and
    every content mod's: `mods.toml`'s `displayName`, `version`, with a basic string's escapes now
    read) go on the Mods page, not `create 0.1.0`. A content mod carries more of a jar's assets:
    measured, the old filter (json, png, mcmeta, ogg...) left out 433 `.nbt` files in 34 jars of
    the pack (Ponder's scenes), Flywheel's 71 shader sources (`.glsl`, `.vert`, `.frag`) and
    more; it now takes `nbt`, `snbt`, `glsl`, `vert`, `frag`, `vsh`, `fsh`, `txt` too, and leaves
    out a file whose path isn't a resource location (23 in the pack; one made the game refuse the
    whole content mod). In game Flywheel's answers are its own text ("Update limiting is currently
    enabled", "The current Flywheel backend is 'flywheel:off'"), not lang keys.
  - Still unmapped in this run: catnip's config screen (`ModConfigs.getModConfigs`,
    `ModConfigSpec.getValues`, `ObjectSelectionList`), Flywheel's GL (`GL20`, `MemoryStack`),
    `SystemReport`, `LevelLightEngine.<init>`, `BlockAndTintGetter.getModelData`.
  - 343 compat, 17 translator (`a_mods_look_and_the_mods_it_bundles_become_content_mods` reads
    the manifests back with `horizon_mods::Manifest::parse`, a dev-dependency), 5 port-pack
    install, 40 game hook and control tests pass.
- [x] 2026-11-05 Item tooltips, as 1.21.1's `ItemStack.getTooltipLines` builds them for mods
  (new `horizon-compat/src/tooltips.rs`). Measured in the pack: 95 jars override `appendHoverText`
  (872 classes), 35 listen to `ItemTooltipEvent` (50 classes); Create 10 and 6. The game's
  `native_tooltip` hook (each frame a tooltip shows, its lines built) gives the bundle the stack and
  the lines; the mods see each line as a literal of its text: a mod item's own `appendHoverText`
  (or, for a block item, its block's: `BlockItem.appendHoverText` asks `Block.appendHoverText`)
  adds lines right after the name, then `ItemTooltipEvent` (stack, the list, the flags, the
  context, the local player) lets mods add, change or take out lines. The game keeps its own
  styled line where a mod left it, takes a mod's new one as a component
  (`component::to_text`), and is told nothing when no mod changed a line. The flags are NeoForge's
  `ClientTooltipFlag` (advanced from F3 + H through the new `native_advanced_tooltips`, creative,
  Shift, Control, Alt held); `Item.TooltipContext` (`of`, `EMPTY`, `registries`, `tickRate`),
  `TooltipFlag.NORMAL`/`ADVANCED`, `asCreative` are mapped. Test
  `tooltips::tests::a_tooltip_gets_the_mods_lines_as_get_tooltip_lines_builds_it` (a mod item whose
  override calls its super, a mod block through its block item, a listener putting a Shift hint
  after the name and taking out a line, a vanilla item nobody changes). In game (`fdcmd.sh`,
  `scroll-at X Y 0` to hover a slot): Create's wrench in the creative inventory shows "Hold [Shift]
  for Summary" under its name; with Shift held, Create's own summary in its palette ("Multi-purpose
  tool for working with your kinetic contraptions.", "When used on Blocks: Rotates components
  around the clicked face...", "When used on Contraptions..."), wrapped at Create's 200 pixels
  (`Font.width`, `BreakIterator`); the goggles the same. That run also showed Ponder's listener
  (`PonderTooltipHandler.updateHovered`, `RegisteredObjectsHelper.getKeyOrThrow(item)`) throwing
  for every vanilla item hovered (72 × "Could not get key for value minecraft:spruce_door!"):
  `Registry.getKey` knew mods' registrations only. Vanilla's items and blocks now answer their
  names (test `neo::tests::vanilla_blocks_and_items_have_their_keys`); `Minecraft.getLocale` is
  mapped. 345 compat, 17 translator tests pass.
- [x] 2026-11-05 Common world events: a block broken or placed, a player's tick, the client joining
  and leaving (new `horizon-compat/src/world_events.rs`). Measured in the pack (methods taking the
  event): `BlockEvent.BreakEvent` 26 jars (33 methods, Create 2), `EntityPlaceEvent` 17 (21, Create
  3), `PlayerTickEvent.Post` 24 (35, Create 3) and `.Pre` 10 (16), `ClientPlayerNetworkEvent.LoggingIn`
  25 (26, Create 1), `.LoggingOut` 34 (47, Create 1), `.Clone` 4 (20). The runtime posted none of
  them. Now, through hooks the game already had (bundle bindings `destroy_block`, `place_block`,
  `client_level_load`, `client_level_unload`; `player_tick` and `client_tick` extended):
  - `BreakEvent` as `ServerPlayerGameMode.destroyBlock` posts it first (`CommonHooks.fireBlockBreak`):
    level, position, state, the breaking player; a listener that cancels it keeps the block (the
    game's `native_destroy_block` cancel).
  - `EntityPlaceEvent` after a player's `useOn` placed a block (`onPlaceItemIntoWorld`): the placed
    state, `placedAgainst` (the block on the clicked face's other side, from the right click
    `events::right_click_block` notes), and the replaced block's `BlockSnapshot` (`create`,
    `getPos`, `getLevel`, `getState`, `getCurrentState`, `restore`...). A mod's own `new
    BreakEvent(...)` / `new EntityPlaceEvent(snapshot, against, entity)` and subclasses are made too.
  - `PlayerTickEvent.Pre` and `.Post` for each player on the server (with the menus' tick) and for
    the client's own player each client tick.
  - On the client: `LoggingIn` once its first level is made (after that level's `LevelEvent.Load`),
    `Clone` for each later one (a change of dimension), `LoggingOut` when the last goes, then that
    level's `LevelEvent.Unload`; the client's block entities are dropped and `Minecraft.level` /
    `.player` are null until the next server's (`Minecraft.getInstance` fills them only in a world).
  Test `world_events::tests::{breaking_and_placing_reach_the_mods_as_block_events,
  a_mods_own_break_event_and_snapshot, a_players_tick_and_the_client_joining_and_leaving}`. In game
  (`fdcmd.sh` with `hzcheck.jar`, whose new `World` listeners cancel a break of diamond ore and tell
  the rest): joining says "hzcheck: logged in"; "100 server ticks of the player" and "100 client
  ticks of the player"; `c break` of a stone: "hzcheck: break minecraft:stone at -28 70 5", the stone
  gone; of diamond ore: "..., kept", the ore still there; `c place` of oak planks on it: "hzcheck:
  placed minecraft:oak_planks against minecraft:diamond_ore"; Save and Quit to Title: "hzcheck:
  logged out" and the client's level unloaded. The earlier checks (commands, keys, raw input,
  tooltips) still pass; Ponder's 72 "Could not get key" are gone. 348 compat, 17 translator tests
  pass.
  - Gaps: the game's placing hook can't be undone, so a canceled `EntityPlaceEvent` keeps the block;
    `BreakEvent`'s pre-canceled cases (spectator, adventure mode) aren't posted; `PlayerTickEvent.Pre`
    comes with `.Post` at the tick's end; a change of dimension keeps the client's one level object.
- [x] 2026-11-05 A mod's entity joining and leaving a level (`world_events.rs`, `mod_entities.rs`).
  Measured: `EntityJoinLevelEvent` 31 jars (41 methods, Create 4: `CommonEvents.onEntityAdded`
  puts a new contraption entity on `ContraptionHandler`'s list for collisions, `attachData`,
  `CapabilityMinecartController.attach`, Flywheel's visuals), `EntityLeaveLevelEvent` 11 (15,
  Create 3). The runtime posted neither. Now `EntityJoinLevelEvent` (entity, the level it was made
  in, `loadedFromDisk` false) as a mod entity joins: `Level.addFreshEntity` on a server, the copy a
  client is sent; a listener that cancels it keeps it out (`addFreshEntity` answers false, the
  client drops the copy). `EntityLeaveLevelEvent` as it leaves: removed at a level's tick, its copy
  let go on a client (the server's removal, or a new copy under its id). Tests
  `mod_entities::tests::a_mod_entitys_joining_and_leaving_reach_the_mods` (two carts, the second
  kept out; the first leaves when it discards itself) and the client half in
  `a_client_is_sent_the_entities_in_its_range_and_follows_them_until_they_go`. Not yet checked in
  game (it needs a contraption assembled); vanilla entities (Create's minecart controllers) aren't
  posted yet. 349 compat, 17 translator tests pass.
- [x] 2026-11-05 The game's own entities for mods, and a player attacking or right-clicking one
  (new `horizon-compat/src/vanilla_entities.rs`; `events.rs`). Measured in the pack (369 jars,
  methods taking the event): `AttackEntityEvent` 16 methods, `PlayerInteractEvent.EntityInteract`
  23, `.EntityInteractSpecific` 10, in 23 jars; Create 11 of them (the wrench breaking minecarts,
  the cardboard sword sparing arthropods, minecart coupling, a schedule handed to a train's
  conductor, the stock ticker's keeper, the extendo grip's reach). The runtime posted none, and a
  mod had no object for a cow, a minecart or any entity the game itself runs.
  - An entity object for each of the game's entities, by id (`vanilla_entities::of`): a player is
    the runtime's server player standing there, an item entity `entities.rs`' own, the rest one
    object each, kept while the entity lasts and brought up to date from the game (new `Host::entity`:
    type, place, facing, a mob's health and max health). Its class is 1.21.1's (`Cow` an `Animal`, a
    `PathfinderMob`, a `LivingEntity`; a minecart an `AbstractMinecart`), from generated tables of the
    130 vanilla entity types (class, width, height, eye height) and the 218 classes' supertypes
    (`TYPES`, `CLASSES`), so `instanceof` and `getType() == EntityType.COW` answer as in Java.
    It reads its id, type, place, block position, rotation, bounding box, size, eye height, level,
    whether it's alive, health, name (translated through the game's language, now on the server's
    hosts too), UUID and persistent data; `hurt`, `kill`, `discard`, `setHealth`, `heal`, `push`,
    `setDeltaMovement` and `teleportTo` go to the game (new host methods, bound to the native mob
    helpers: `native_set_mob_health`, `native_kill_mob`, `native_remove_mob`, `native_push_mob`,
    `native_teleport_mob`, and `kill_target`).
  - `AttackEntityEvent` where `Player.attack` starts (the game's `native_attack_entity`, bundle
    binding `attack_entity`): the player and the target; a listener that cancels it stops the blow.
    `EntityInteractSpecific`, then `EntityInteract`, where `Player.interactOn` starts, once per hand
    (`native_interact_entity`, binding `interact_entity`): target, hand, held stack, position; one
    that cancels it takes the click.
  - Small ones the checks met: `EntityType.is(TagKey)` through the game's entity type tags (new
    `Host::entity_type_in_tag`; Create's cardboard sword asks `EntityTypeTags.ARTHROPOD`), and a
    player's `distanceTo` / `distanceToSqr` of an entity, a point or a `Vec3` (the extendo grip).
  Tests `vanilla_entities::tests::{the_tables_are_sorted_for_their_searches,
  vanilla_entities_are_their_1_21_1_classes, a_mod_reads_and_hurts_the_games_entity}`. In game
  (`fdcmd.sh` with `hzcheck.jar`, whose new `Entities` listeners describe what was hit, cancel a
  pig's blow and set a right-clicked cow's health to 4): `c attack` on a cow: "hzcheck: attacked
  minecraft:cow (Cow, an animal, health 10.0/10.0)", the cow then at 9; on a pig: "..., kept",
  the pig still at 10; `c interact-entity` on the cow: "hzcheck: interact minecraft:cow (Cow, an
  animal, health 4.0/10.0) with the main hand", and the game's cow at 4. The earlier checks
  (commands, keys, raw input, world events, tooltips) still pass. 352 compat, 17 translator tests
  pass.
  - Gaps: a mod's `hurt` takes the health off (no armour, knockback or attacker credit), and only a
    mob's (Create's wrench can't break a minecart yet); an entity's UUID is a stable stand-in, its
    motion, pitch, equipment and effects aren't read; a player is found by place; the
    `EntityInteractSpecific` click point is the entity's feet.
- [x] 2026-11-05 Damage for mods: the game's damage types and sources, and NeoForge's events of a
  hit (new `horizon-compat/src/damage.rs`; new hooks `native_living_damage_pre`,
  `native_living_damage_post`, `native_living_death`). Measured in the pack (367 jars, methods
  taking the event): `LivingDeathEvent` 24 jars (36 methods), `LivingIncomingDamageEvent` 16 (29,
  Create 1: the extendo grip notes a blow its reach dealt), `LivingDamageEvent.Pre` 14 (23), `.Post`
  7 (11), `LivingKnockBackEvent` 5 (5, Create 1: the extendo grip's stronger knockback), 38 jars in
  all; and `DamageSource`'s own API: `getEntity` in 42 jars, `is(TagKey)` 25, `new
  DamageSource(holder)` 22, `is(ResourceKey)` 17, `DamageSources.playerAttack` 13, `.fall` 13,
  `.mobAttack` 12. The runtime posted none of the events, and a mod could neither make nor read a
  damage source (Create hurts with its own nine types from its data: `create:crush` for the
  crushing wheels, `mechanical_saw`, `mechanical_drill`, `mechanical_roller`, `fan_fire`,
  `fan_lava`, `run_over`, `potato_cannon`, `cuckoo_surprise`).
  - Damage types are data: the game's (new `Host::damage_types`: id, message id, exhaustion and
    tags, from the game's damage catalog) and the mods' own (`data/<ns>/damage_type/*.json` of the
    bundled jars, with their `tags/damage_type` files, whose `#` references resolve through both).
    `DamageTypes.IN_FIRE` and the rest of 1.21.1's 47 fields are their keys;
    `registryAccess().registryOrThrow(Registries.DAMAGE_TYPE).getHolderOrThrow(key)` gives the
    type's holder and `Holder.is(TagKey)` reads its tags.
  - `DamageSource`s a mod makes (`new DamageSource(holder[, entity[, entity]][, pos])`) or asks
    `level.damageSources()` for (`fall()`, `inFire()`, `playerAttack(player)`, `mobAttack(mob)`,
    `arrow(arrow, owner)`, `explosion(...)`, `source(key, ...)`...), with 1.21.1's rules (one entity
    is both the direct and the causing one; an explosion of two entities is `player_explosion`; a
    fireball of nobody `unattributed_fireball`). They read `getEntity`, `getDirectEntity`,
    `isDirect`, `type()`, `typeHolder()`, `getMsgId`, `getFoodExhaustion`, `is(TagKey)`,
    `is(ResourceKey)`, `getSourcePosition`, `getWeaponItem` and `isCreativePlayer`.
  - The events, posted from the game's damage hooks where NeoForge posts them:
    `LivingIncomingDamageEvent` once `LivingEntity.hurt` is past its guards (the game's
    `native_living_incoming_damage`: a listener changes the amount or cancels the hit);
    `LivingDamageEvent.Pre` in `actuallyHurt` after armour, enchantments and Resistance, before
    absorption (new `native_living_damage_pre`, from horizon-world's `DamageWorld::damage_pre` for
    players and the mobs' `Mob::reach_health`: a listener sets what goes on), and `.Post` once the
    health took it (`native_living_damage_post`); `LivingKnockBackEvent` (`native_living_knockback`:
    strength and direction changed, or the push canceled); a mob's `LivingDeathEvent` where
    `LivingEntity.die` starts (new `native_living_death`, asked by `creatures::mob_died`): a
    listener that cancels it keeps the mob, which lives on if given health back. A player's death
    is heard (`native_player_died`), not cancelable. One `DamageContainer` follows a hit from its
    incoming event to its `Post`, as NeoForge's stack of them on the entity does
    (`getOriginalDamage`, `getNewDamage`, `setNewDamage`, `getReduction`, `getBlockedDamage`...).
  - Who is hit, and by whom: the hurt entity is the runtime's object for it
    (`vanilla_entities::living`), brought up to date from the game's mob as the hit finds it (new
    `health::hit_entity`: health, max health, place, facing; the hit's hooks have no world to ask);
    a player is the server's player of that entity id (new `Host::player_entities`, read each
    server tick); a player's blow or arrow now names that player (new `health::acting_player`, set
    by `as_guest`), where its source had no entity before.
  Tests `damage::tests::{a_mods_damage_sources_read_as_vanillas,
  a_hit_reaches_the_mods_and_their_changes_come_back, a_death_the_mods_cancel_keeps_the_mob_alive,
  a_mods_own_damage_events_and_containers}`, `living_damage::tests::the_damage_goes_on_as_the_pre_step_leaves_it`,
  `mobs::native_damage_tests::a_native_hook_changes_the_damage_that_reaches_a_mobs_health`,
  `native_hooks::tests::the_damage_hook_names_the_source`. In game (`fdcmd.sh` with `hzcheck.jar`,
  whose new `Damage` listeners halve fire on a cow, triple a player's blow on one, log the rest and
  keep a cow's first death, its health set back to 6): the player's blow on a cow at 10 leaves it
  at 7 ("hzcheck: incoming player 1.0 on minecraft:cow (health 10.0) ..., tripled", "pre 3.0 of 1.0",
  "post 3.0 of 1.0 ... (player)", "knockback 0.4"); `/damage` with `minecraft:in_fire` 4 on it at 4
  leaves it at 2 ("incoming inFire 4.0 ... by none, fire halved", "pre 2.0 of 4.0"); the next blow
  kills it and "death of minecraft:cow (player) ..., kept with health 6" is heard; the wolves'
  bites on the sheep nearby come through the same way ("incoming mob 4.0 on minecraft:sheep ... by
  minecraft:wolf", "death of minecraft:sheep (mob) by minecraft:wolf"). The earlier checks
  (commands, keys, raw input, world events, entities, tooltips) still pass. That round found three
  faults, fixed since: the game refused a dying mob's health (`native_set_mob_health` took living
  mobs only), so the kept cow died anyway; a death named no player, the player's turn being over by
  then (`player_killer_id` now reads the game's `last_hurt_by_player`); and a player's `getType`
  was nobody's (now `EntityType.PLAYER`). 356 compat, 17 translator tests pass.
  - Gaps: what a listener asks of the world in the hit's own hooks (incoming, pre, post, knockback)
    doesn't reach the game, which is in the middle of the hit: a message to the player, healing the
    attacker; a container's reduction functions (`addModifier`) and the invulnerable time a listener
    sets aren't applied (the game computes its own); a player's death can't be canceled; a hit a
    mod's own code deals (`entity.hurt` from a listener or a block entity) posts no events, the
    runtime being busy in that call; a player's held item isn't known in the hit's hooks (Create's
    extendo grip and Farmer's Delight's backstabbing ask `getMainHandItem`); Horizon pushes after
    Resistance where NeoForge's knockback event comes before; `LivingDropsEvent` (13 jars, Create 2:
    the deployer and the crushing wheels collect a kill's drops) isn't posted yet.
  - Measured again over the pack (367 jars, `coverage` with this step's translator, datagen-only
    APIs left out): 63.8% of the API methods the jars call answer a probe (83313 of 130683; 63.3%
    before this step, 60.2% this morning, 53.5% on Nov 4), 75.2% of the call sites (277698 of
    369268); Create 57.8% of its methods (2308 of 3994), 84.2% of its call sites (21366 of 25380);
    14188 distinct methods missing across the pack (14291 before, 16305 on Nov 4). Still a lower
    bound: the probe calls each method with null arguments, or with a bare object of its class as
    the receiver, and the most-missed ones answer only an object the runtime made: `Holder.value`
    (136 jars) a registry's holder, `Codec.fieldOf` (113) and `MapCodec.forGetter` (120) a codec,
    `NonNullList.get` (102) a list, `Level.addFreshEntity` (85) an entity.
- [x] 2026-11-05 A dead mob's drops for mods: NeoForge's `LivingDropsEvent` (`damage.rs`; new hook
  `native_living_drops`). Measured in the pack (methods taking the event): 13 jars, 17 methods;
  Create 2 (its crushing wheels set the drops of a mob they crushed still, `setDeltaMovement(Vec3.ZERO)`
  on each when the source `is(AllDamageTypes.CRUSH)`, so they fall into the wheels; its deployer's
  fake player collects what it kills), Mystical Agriculture and Agradditions (their mob drops), FTB
  Quests, KubeJS, Iron's Spellbooks, and for a player's death Curios, Sophisticated Backpacks,
  Corpse, Cosmetic Armor and Ars Elemental. The runtime posted none.
  - The game's `drop_death_loot` now makes a death's drops first (the loot table's, after
    `native_loot_drops`, then what an enderman carried), each with its place, motion and pickup
    delay as its item entity would have them (`NativeDrop`), hands them to the new hook, and adds
    to the level what comes back, nothing when it is canceled; the orbs follow as before. The hook
    names the death's source as `native_living_death` does (`death_source`, read before the game
    forgets who hit the mob) and whether a player hurt it in the last 100 ticks.
  - The mods see the drops as `ItemEntity` objects not yet in the level (`entities::unspawned`):
    `getDrops()` is a list they read, change (`setDeltaMovement`, `setItem`, `setPickUpDelay`, a
    stack's count), add to (`new ItemEntity(level, x, y, z, stack)`) or take from, `getSource`,
    `isRecentlyHit`, `setCanceled`; a mod's own `new LivingDropsEvent(...)` too. What they leave
    goes back to the game; a drop whose stack they didn't change keeps the game's own stack (its
    vanilla components the runtime doesn't model stay).
  Test `damage::tests::a_mods_drops_listener_changes_the_drops` (a listener stills a cow's drops,
  takes its leather and adds a diamond; one canceling a death in fire leaves none; with no listener
  nothing is asked). In game (Farmer's Delight translated; `hzcheck`'s new drops listener stills a
  cow's drops and adds `new ItemEntity(level, x, y, z, diamond)`; `scratchpad/fdcmd.sh`): the cow
  `hzcheck` had kept alive died of two more blows; its leather and two beef lay still where it died
  (the game's own scatter) and a diamond beside them, moved a little by the constructor's random
  motion as vanilla's is. The log: `drops of minecraft:cow (player) by minecraft:player, recently
  hit true: minecraft:leather x1 minecraft:beef x2; stilled, a diamond added`, and for a sheep a
  wolf killed nearby `by minecraft:wolf, recently hit false: minecraft:mutton x2
  minecraft:white_wool x1`. The same round confirmed the step before's three fixes: the cow kept
  alive lived on with health 6, and its death and each of the player's hits named
  `minecraft:player`. 357 compat, 17 translator tests pass.
  - Gaps: a player's death drops aren't posted yet (Curios, backpacks and Corpse listen for them);
    Create's deployer collects its kills' drops only when its fake player's blow is known as the
    killer, which a mod's own `hurt` doesn't post (the gap of the hit's own hooks above). Seen in
    game: a message a listener sends the killer from a mob's death (`displayClientMessage`) reaches
    no chat, the hooks' player being the one acting and a mob dying in the mobs' tick; in the hit's
    own hooks a mob that attacks (the wolf) stands as an entity of its id alone, of no type
    (`minecraft:`).
- [x] 2026-11-05 A hit's world for mods: what a listener does in a hit's own hooks reaches the game, and the
  hooks read the player whose blow it is (`damage.rs`, `bundle.rs`; game: `native_hits.rs`, new API
  `native_hit_player` and `native_after_hit`). The hooks of a hit (incoming, pre, post, knockback) run inside the
  game's damage code, in the middle of the hit, and had no host: a listener's sound, particles, message or another
  entity hurt went nowhere, and a player's held item read as empty. Measured in the pack: Farmer's Delight's skillet
  (`LivingDamageEvent.Pre`: the attacker's main hand, `Level.playSound`), its backstabbing
  (`LivingIncomingDamageEvent`: `getWeaponItem`, its enchantment, `ServerLevel.playSound`) and its knife
  (`LivingKnockBackEvent`: `getItemInHand`), Create's extendo grip (`getMainHandItem`).
  - The game takes the acting player as their blow starts (`attack_mob`, `with_hit_player`): their place, look, game
    mode, whether they sneak, and the stacks in their hands with their data (`NativeHitHand`: the native components
    and the enchantments). The hooks' host answers from it (`player_hand`, `player_place`, the striker as an entity);
    a hit no player's blow dealt has no player.
  - What a listener changes waits for the hit to end (`native_after_hit`): sounds, particles, messages, entities hurt,
    killed, removed, pushed, moved or healed, orbs, payloads. The game makes them when the player's action that dealt
    the hit ends (`as_guest`) or the level's tick does (`tick_server`), and before another level takes its place
    (`enter_level`), so each is made in its own level.
  - A message to a player object reaches that player by UUID (`Host::tell_player`, through `native_tell` and
    `native_action_bar`), whoever acts: a mob's death, in the mobs' tick with nobody acting, now tells its killer.
  Test `damage::tests::a_hits_listener_reads_the_weapon_and_its_sound_and_message_reach_the_game` (a listener like the
  skillet's reads the striker's skillet, also as the source's weapon; its clang and its word reach the hit's host; a
  death tells its killer by UUID with nobody acting). In game (`hzcheck`'s `Pre` listener now names what the
  striking player holds and the source's weapon, plays an anvil's clang at the victim and tells the player;
  `scratchpad/fdcmd.sh`): each blow on the cow told the player in their chat `hzcheck: pre 3.0 of 1.0 on minecraft:cow,
  holding minecraft:oak_planks, weapon minecraft:oak_planks` (the planks they held), and the incoming, post and death
  lines the rounds before only logged reached the chat too, as did the drops listener's from the cow's death. The clang
  isn't heard in these runs (no audio): the test checks it reaches the hit's host. No change of a hit was left unmade,
  no panic. 358 compat, 17 translator, 42 game tests pass.
  - Gaps: what a listener changes of the weapon in a hit's hooks (its wear, its data) doesn't reach the game; a
    player's blow on another player (`pvp_land`) and their arrows have no striking player in the hooks; a mob that
    attacks still stands as an entity of its id alone, of no type.
- [x] 2026-11-05 Who hurt a mob and where it looks, for Farmer's Delight's knife and backstabbing (`damage.rs`,
  `vanilla_entities.rs`, `api_common.rs`, `api.rs`). Missing in the pack's measure (packcov4, jars/uses):
  `getLookAngle` 35/140, `getViewVector` 27/75, `getLastHurtByMob` 13/39, `getKillCredit` 6/9. In game, a wolf's blow
  on a sheep reached Farmer's Delight's own listeners and stopped at `getKillCredit` (its knife:
  `LivingKnockBackEvent`, the credited attacker's main hand) and `getViewVector` (its backstabbing,
  `isLookingBehindTarget`), and a player's level read `isClientSide` from a `ServerLevel` that had no such field.
  - A hit the runtime is told of records its causing entity as what last hurt the victim (`setLastHurtByMob`), and a
    player also as the last player that did (`lastHurtByPlayer`): at the incoming event, at the push (whose
    attacker the knockback hook is now given) and at the death. `getLastHurtByMob` and `getLastAttacker` answer the
    first, `getKillCredit` the player first, as vanilla's.
  - The game's mobs look where they face (`getLookAngle`, `getViewVector`, `getForward`: `calculateViewVector` with
    `Mth`'s table, level, as the game doesn't tell a mob's pitch).
  - A server player's level (`level()`, `getCommandSenderWorld()`) is the hooks' one level, a `ServerLevel` whose
    `isClientSide` field reads false; a level without the field answers it from the host's side.
  Test `damage::tests::a_hits_listener_finds_its_attacker_and_where_the_victim_looks` (a knife-like listener credits
  the cow's push to the player holding a flint knife and weakens it; a backstab-like one doubles a blow from behind
  the cow turned away, not one from in front; a pig nobody hurt has nobody to credit). In game (`hzcheck`'s incoming
  listener now asks, as the backstabbing does, whether the blow came from behind the victim as it looks and whether
  the striker's level is the client's; its push listener who gets the credit and what they hold): the player's blows
  on the cow, which faced south (yaw 0) with the player north of it, read `facing 0.0, from behind true, on the client
  false`, and each push `credited to minecraft:player holding minecraft:oak_planks`. `getKillCredit`, `getViewVector`
  and the `isClientSide` read left the run's list of what isn't translated; Farmer's Delight's backstabbing went on
  to the attacker's `getWeaponItem` (a player's main hand, added in the next step) and
  `EnchantmentHelper.runIterationOnItem`. 359 compat, 17 translator tests pass.
  - Gaps: what hurt a mob isn't forgotten after 100 ticks or when the attacker dies, as vanilla forgets it; a mob's
    pitch and head turn aren't told; an enchantment's own effect components (`modifyDamageFilteredValue`, which the
    backstabbing's damage goes through) aren't evaluated.
- [x] 2026-11-05 The game's attributes for mods (`attributes.rs`; game: `native_attributes.rs`, new API
  `native_attribute` and `native_change_attribute`; host: `Host::attribute`, `Host::change_attribute`). Missing in
  the pack's measure (packcov4, jars/uses): `getAttributeValue` 24/131, `AttributeInstance.getValue` 20/51,
  `getAttributes` 13/23, `addTransientModifier` 11/16, and `getAttribute(Holder)` about 54 uses; in game, Farmer's
  Delight's health overlay stopped at `Player.getAttribute(MAX_HEALTH)`.
  - An entity's `getAttribute`, `getAttributeValue`, `getAttributeBaseValue` and `getAttributes()` (an
    `AttributeMap`: `getInstance`, `getValue`, `hasAttribute`, `hasModifier`, `getModifierValue`) answer the game's
    attributes: an `AttributeInstance` names its entity and attribute, and each read (`getValue`, `getBaseValue`,
    `getModifiers`, `getModifier`, `hasModifier`) or change (`setBaseValue`, `addTransientModifier`,
    `addPermanentModifier` and their replacing kin, `removeModifier`, `removeModifiers`) goes to the game. 1.21.1's
    ids (`minecraft:generic.max_health`) name the game's (`minecraft:max_health`).
  - The player in place has the game's whole attribute map: a mod's speed modifier makes them faster, as the
    `/attribute` command's does, and reaches their client. A mob answers its plain values (maximum health, speed,
    attack damage...) and takes a new base for them; its other attributes are the map `/attribute` keeps. One a type
    doesn't have (a cow's attack damage) is null, as vanilla's.
  - Where the host can't tell (a hit's hooks, a client screen with no game lent), a player reads as vanilla's player
    by default (`Player.createAttributes`: speed 0.1, attack damage 1), unchanged; a mob has none. A hit's hooks'
    changes wait for the hit to end, as the rest of what they change.
  - A player's `getWeaponItem` is their main hand (Farmer's Delight's backstabbing asks the attacker's).
  Test `attributes::tests::a_mod_reads_and_changes_the_games_attributes` (the acting player's speed with a sprint
  modifier of the total's multiple added, read, found and taken off; their maximum health's base raised through
  their attribute map; a cow's maximum health, and no attack damage; with no host, vanilla's player's defaults) and
  `native_attributes::tests` (1.21.1's ids name the game's). 360 compat, 17 translator, 41 game tests pass. In
  game (`hzcheck`'s attack listener reads the player's speed, toggles a modifier of its own on it, `hzcheck:rush`
  adding the total once more, and reads the cow's and the player's maximum health and the cow's attack damage): the
  first blow on the cow read `speed 0.10000000149011612, rushing: 0.20000000298023224, its max health 10.0, no
  attack, the player's max health 20.0`, and the game's own `/attribute @s minecraft:movement_speed get` then said
  `0.20000000298023224`, its `modifier value get hzcheck:rush` `1.0`: the mod's modifier is the game's. The blow on
  the pig still read `0.20000000298023224`, and the next on the cow `walking again: 0.10000000149011612`. The
  generated hosts' attribute methods built at the first try; `getAttribute` and `getWeaponItem` left the run's list
  of what isn't translated, no panic. Farmer's Delight's backstabbing now stops at
  `EnchantmentHelper.runIterationOnItem` alone.
  - Gaps: a mod's own attributes (registered for its entities) aren't in the game's catalog; a mob's plain
    attributes take no modifiers; another player than the one in place has no attributes to read but vanilla's
    defaults; `AttributeMap.addTransientAttributeModifiers` (a multimap of them) isn't there.
- [x] 2026-11-05 Mob effects for mods (`mob_effects.rs`; game: `native_effects.rs`, new API `native_effects`,
  `native_add_effect` and `native_remove_effect`; host: `Host::effects`, `add_effect`, `remove_effect`). Missing in the
  pack's measure (packcov4, uses summed over `LivingEntity`, `Player`, `ServerPlayer` and `LocalPlayer`): `addEffect`
  about 330 in 40+ jars, `hasEffect` 134, `getEffect` 73, `removeEffect` 57, `getActiveEffects` 25,
  `removeEffectNoUpdate` 12, `removeAllEffects` and `getActiveEffectsMap` 6 and 8: the most used entity API left.
  - An entity's `addEffect` (with or without a source), `forceAddEffect`, `hasEffect`, `getEffect`, `removeEffect`,
    `removeEffectNoUpdate`, `removeAllEffects`, `getActiveEffects` and `getActiveEffectsMap`, on whichever class the
    mod names them, are the game's effects: the player in place's (`PlayerEffects`: what a mod gives ticks, changes
    their attributes and reaches their client as `/effect`'s does) and a mob's (its list, read by its behaviour and
    swirls; refused where `canBeAffected` refuses, a spider's poison). The game updates one given again as vanilla's
    `MobEffectInstance.update` does; a content mod's effect runs its added hooks.
  - Effects go by id: vanilla's `MobEffects` holders and a mod's `DeferredHolder`s (`farmersdelight:comfort`); an
    instance a read makes has the holder `MobEffects`' field gives (one object per effect), and
    `MobEffectInstance.is` compares ids. `endsWithin` and `getDescriptionId` answer.
  - Inside a hit's hooks the striking player's effects are those they had as the blow started (`NativeHitPlayer`
    carries them); what a hook gives or takes waits for the hit to end, as its other changes.
  - The vanilla mob stand-ins' stubs (no effects, `addEffect` false) are gone: their effects are the game's mob's.
  Test `mob_effects::tests::a_mod_gives_reads_and_takes_off_the_games_effects` (the acting player's speed from a potion
  read by holder, amplifier and duration; a stronger one given takes over and a weaker, shorter one changes nothing; a
  cow takes a mod's own effect by its `DeferredHolder`, lists it and loses it once; a wolf the host doesn't know takes
  none; everything off the player at the end). 361 compat, 17 translator, 41 game tests pass. In game (`hzcheck`'s
  attack listener gives the player night vision, a stronger one over it and then a weaker, shorter one, and the cow
  glowing; at the next blow on the cow it takes the night vision off): the first blow read `night vision given: true
  true false, the cow glows: true, the player's night vision level 2 for 1200 ticks, the cow's effects 1`. The game's
  own `/effect give @s minecraft:night_vision 30 0` then answered `Unable to apply this effect (target is either
  immune to effects, or has something stronger)`, and `/effect clear` on the cow `Removed effect Glowing from Cow`:
  both are the game's effects. The next blow read `night vision taken off: true, the player's night vision none, the
  cow's effects 0` (the command's clearing seen by the mod), and `/effect clear @s minecraft:night_vision` found
  none left. No panic; no effect call in the run's list of what isn't translated.
  - Gaps: a mod's own effects aren't declared to the game yet (its `MobEffect` registrations), so giving one is
    refused; NeoForge's `MobEffectEvent` (`Applicable`, `Added`, `Remove`, `Expired`) isn't fired; `canBeAffected`,
    `removeEffectsCuredBy` and `MobEffect.applyInstantenousEffect` aren't there; another player than the one in place
    and a mob inside a hit's hooks have no effects to read.
- [x] 2026-11-05 A mod's own mob effects in the game (`mob_effects.rs`: `effect_declarations`, `should_apply_tick`,
  `apply_tick`, `effect_added`; content: `Content::declare_effect`; translator: the content hook declares them, and the
  hooks `native_should_apply_effect_tick_this_tick`, `native_apply_effect_tick`, `native_on_effect_added` and
  `native_mob_effect_added` run their code). Until now the game refused Farmer's Delight's Comfort and Nourishment, and
  every other `MobEffect` a pack's mods register: it didn't know them.
  - The content hook declares each `mob_effect` the mod registered (outside `minecraft`) as a content mod's effect, with
    the category and colour its `new MobEffect(category, color)` took. `/effect give`, potions and a mod's `addEffect`
    then take it, and the game ticks it, shows its icon and sends it to clients as a content mod's.
  - Its code runs where vanilla's would. `shouldApplyEffectTickThisTick(duration, amplifier)` runs each tick, at once:
    it needs no world. `applyEffectTick(entity, amplifier)` runs once the entity's tick is over, with the game at hand:
    inside the tick the game is read-only. The effect is taken off when its code says it ends. `onEffectAdded` runs when
    the entity hadn't had it and `onEffectStarted` each time it's given, once the action or tick that gave it is over: a
    mod's own `addEffect` gives it from inside the mod's call, where the runtime is busy. A method the class leaves to
    vanilla's does what vanilla's does: no tick, nothing on adding.
  Tests `content::tests::code_declares_effects_as_files_do` (code's declaration replaces a file's of the same id; another
  mod's id and a colour past RGB refused) and
  `mob_effects::tests::a_mods_own_effect_is_declared_and_its_code_acts_when_the_game_asks` (a harmful effect declared
  with its colour; its `shouldApplyEffectTickThisTick` answers by duration, and vanilla's speed isn't the mod's; its
  `applyEffectTick` acts on the cow the host knows until the third act ends it, and not on an entity the host doesn't
  know; `onEffectAdded` gets the player and the amplifier, `onEffectStarted` the cow). 362 compat, 17 translator, 33
  horizon-mods tests pass. In game (`hzcheck`'s `StaticEffect`: harmful, `0x3366FF`, acts every 20 ticks, ends itself
  at its third act and logs each call): the content log said `hzcheck (translated): 0 blocks and items declared from its
  registration, 0 refused, 1 mob effects` and `farmersdelight (translated): ... 2 mob effects` (Comfort and
  Nourishment). `/effect give @s hzcheck:static 30 1` answered `Applied effect Static to Player65593` (its name from
  the mod's lang file), and the cow's `Applied effect Static to Cow`. Its code ran: `static added to minecraft:player
  (level 2)`, `static started on minecraft:player (level 2)`, then `static acts on minecraft:player (level 2), act 1,
  has it true`, act 2 and `act 3, the last`, and the same on the cow at level 1. After 90 ticks `/effect clear @s
  hzcheck:static` answered `Target doesn't have the requested effect`: its third act took it off. No panic.
  - The round's script first loaded only Create's and Farmer's Delight's content mods, not those of the addons
    translated with Create, so the game didn't know hzcheck's effect (`Can't find element 'hzcheck:static'`). It now
    loads every `content-<id>` beside Create's; the translator was right.
  - Gap: on the cow, given the effect by the command, `onEffectAdded` and `onEffectStarted` didn't run, while the
    player's did. The next round traces each effect hook as it starts, and an entity the game doesn't know.
- [x] 2026-11-06 The player's health and food for mods (`vitals.rs`; game: `native_vitals.rs`, new API
  `native_player_vitals` and `native_set_player_vitals`; host: `Host::player_vitals`, `set_player_vitals`). Missing in
  the pack's measure (packcov4): `getHealth` 87 uses in 38 jars, `getMaxHealth` 80 in 25, `isDeadOrDying` 43, `heal`
  37, `setHealth` 19, `FoodData.eat(int, float)` 19, `getSaturationLevel` 13, `getAbsorptionAmount` 8 and a few more:
  327 uses in 52 jars. Farmer's Delight's Comfort heals with them, and its Nourishment keeps the food from draining.
  - On the player in place, `getHealth`, `getMaxHealth`, `getAbsorptionAmount`, `isHurt`, `isDeadOrDying`,
    `setHealth`, `setAbsorptionAmount` and `heal` are the game's player's, on whichever class the mod names them. `heal`
    is NeoForge's: no healing by a negative amount, nor of a dead player. A mob's health stays its stand-in's.
  - `Player.getFoodData`'s `getFoodLevel`, `needsFood`, `getSaturationLevel`, `getExhaustionLevel`, `setFoodLevel`,
    `setSaturation`, `setExhaustion` and `eat` act on the game's player's food. `eat` is `FoodData.add`: the food level
    up to 20, the saturation up to the food level. `addExhaustion` tires the player as before. These methods moved
    from `api_common` to `vitals`.
  - The game keeps the health and absorption in their range, as vanilla's setters do, and sets the food as given. A
    client's player isn't changed: the server's health and food are theirs. Where the host can't tell (a hit's hooks,
    a screen with no game lent), a player reads as a new one: full health and food.
  Tests `vitals::tests::a_mod_heals_and_feeds_the_games_player` (Comfort's reads and its heal by one; a negative heal
  refused; Nourishment's exhaustion emptied; two bites, the saturation capped by the food level and the food at 20;
  the cow's health its own; health 0 is dying) and `with_no_player_to_read_a_player_is_a_new_ones`. 364 compat, 17
  translator and 41 game tests pass.
  In game (round 8, 6 November, the scenario's `vitals` steps): `/hzrun vitals hurt` (hzcheck sets the health to 10,
  the food to 17, the saturation to 0 and the exhaustion to 3.5, then heals by -4, refused) answered `health
  10.0/20.0, hurt true, dying false, absorption 0.0, food 17, saturation 0.0, exhaustion 3.5`. Farmer's Delight's
  Comfort healed by one every 80 ticks: 12.0 after 170. Its Nourishment emptied the exhaustion (0.0). The game's own
  `/data get entity @s Health`, `foodLevel` and `foodSaturationLevel` agreed: `12.0f`, `17`, `0.0f`. No panic.
  - The cow gap of part 9b, traced: `hzcheck:static given to entity Some(456), which the game doesn't know: no
    onEffectAdded`. The effect hooks wait for the action that gave the effect to end (`native_after_hit`), but the
    owner's console command (`horizon-ctl command`) and the dedicated console end with no such point. Their changes
    waited for whichever action ended next, and ran there. The next entry gives them their own.
- [x] 2026-11-06 Measured again over the pack (packcov5; 367 jars, datagen-only APIs left out), the old way (null or
  bare receivers): 83 924 of 130 683 API methods (64.2%), 279 095 of 369 268 class uses (75.6%); packcov4 had 83 313
  and 277 698: the vitals add 611 methods and 1 397 uses. Create alone: 2 330 of 3 994 methods, 21 415 of 25 380
  uses (84.4%).
  - New `PROBE=hosted` for `coverage` (`main.rs`): each call also has a game to ask (`probe_host`: a player in
    place at full health, a cow beside them, no effects), and a method is also tried on the receivers a mod's code
    finds:
    - an entity class's on that player and that cow;
    - a level class's on the runtime's level (`glue::level`);
    - any class's on the static fields of API classes the mod reads, by their declared type (`BuiltInRegistries.ITEM`
      for `DefaultedRegistry.getKey`, `MobEffects.SPEED` for `Holder.value`, `Codec.STRING` for `PrimitiveCodec.fieldOf`);
    - and on what the mod makes of the class: a new object with its constructor run, or what an API method returning
      the class gives (`NonNullList.withSize`, `new ListTag()`), with null arguments.
    Up to 3 of each kind per class.
  - Hosted: 91 772 of 130 683 methods (70.2%), 300 168 of 369 268 uses (81.3%), 13 042 distinct methods missing
    across the pack. Create: 2 585 of 3 994 methods (64.7%), 22 592 of 25 380 uses (89.0%). By step: the player and
    the cow alone give 78.4% of the uses; the level and the statics 80.5%; what the mod makes 81.3%. Still a lower
    bound: two-step chains (`Codec.fieldOf(...).forGetter`, `group(...).apply`), a menu's slots and a null entity
    argument (`Level.addFreshEntity`) count as missing.
  - The top of what is missing now, mods/uses: client entity models (`MeshDefinition.getRoot` 42/339,
    `PartDefinition.addOrReplaceChild` 41/336, `CubeListBuilder` 41/349, `LayerDefinition.create` 39/335,
    `ModelPart.getChild` 42/294), `ClientboundBlockEntityDataPacket.create` 60/163, `Camera.getPosition` 65/221,
    `Minecraft.getConnection` 57/130, `ServerGamePacketListenerImpl.send` 54/120, `Player.drop` 49/163,
    `BlockState.cycle` 46/264, `Level.getEntity(int)` 46/151, `Level.clip` 41/113, `Block.getId` 41/101,
    `ItemStack.getComponents` 41/79.
- [x] 2026-11-06 Part 10b: a console command's deferred changes are made as it ends. `Game::with_after_hits(action)`
  (`native_hits.rs`) runs an action, then the changes its hooks left (`native_after_hit`), with the player it acts as
  still in place. Changes left before it wait for their own action or tick: a guest's turn run inside a level's tick
  doesn't make the tick's, which may name an entity the tick holds out of the level for the while. A guest's action
  (`as_guest`), the owner's console command (`integrated.rs`) and the dedicated server's console (`dedicated.rs`) use
  it. The effect trace names the level (`mob_effects.rs`).
  In game (round 9, 6 November): the cow given `hzcheck:static` by the owner's console command runs its hooks now:
  `static added to minecraft:cow (level 1)`, `static started on minecraft:cow (level 1)`, then acts 1, 2 and 3, the
  last; no "doesn't know". The player's effect, the vitals, the hits, the deaths and the drops as in round 8; no panic.
- [x] 2026-11-06 Part 11: level events and block state ids. A mod's `Level.levelEvent(event, pos, data)` (also the form
  naming a player, and `globalLevelEvent`) was a no-op; 71 jars of the pack call it, from 191 classes. Vanilla's
  `LevelEvent` codes carry sounds and particles: a block breaking 2001 with its state's id (Create's drills and saws),
  a door's sound 1005, smoke 2000, bone meal 1505. New compat module `level_events.rs` gives them to the game
  (`Host::level_event`), which makes them as its own: `native_level_event` (`game/native_level_events.rs`) calls
  `level_event_at`, played where the game runs and sent to a server's players near. A hit's hooks queue theirs for
  after the hit. `Block.getId(state)` and `Block.stateById(id)` read the game's state ids (`Host::state_id`,
  `state_of_id`); a state the game doesn't know is air's 0, an id past the table air. Fragment
  `tools/native-hook-fragments/native_level_event.json` (the name `level_events` is the level load and unload hooks').
  Test `level_events::tests::a_mod_breaks_a_block_with_its_sound_and_particles`.
- [x] 2026-11-06 Part 12: entities by id and UUID, `Player.drop`, a block entity's update packet (from the gaps packcov5
  measured).
  - `Level.getEntity(int)` (46 mods): a mod's entity by its id; the game's: a player by their entity id, any other
    entity the level holds (`vanilla_entities::by_id`). `ServerLevel.getEntity(UUID)` and `Level.getPlayerByUUID`
    (18 and 23 mods): a player by their UUID, a mod's entity by its own, the game's others by the UUID the game gives
    them (`lan::entity_uuid`). A player found so carries their entity id and place (`vanilla_entities::player`, as
    `damage.rs` marks them), so hits and effects reach them, not the acting player.
  - `Player.drop(stack, includeThrowerName)` and `drop(stack, dropAround, includeThrowerName)` (49 mods): an item
    entity of the stack at the player's eyes less 0.3, thrown as 26.3's `LivingEntity.createItemStackToDrop` throws
    it (in floats, with `Mth`'s sine table, the random numbers drawn in vanilla's order), picked up after 40 ticks, in
    the level on a server; a client's is only returned. The menus' own drops (a stack thrown out of a menu, what
    `placeItemBackInInventory` can't place) throw the same way now. Not modelled: the thrower, `ItemTossEvent`, the
    statistics.
  - `ClientboundBlockEntityDataPacket.create(blockEntity[, saver])` (60 mods), what a mod's `getUpdatePacket`
    returns: the block entity's position, type and update tag (its own `getUpdateTag`, or what the saver makes);
    the data-class table answers its getters. The game sends players the update tag itself.
  Tests `mod_entities::tests::the_level_finds_its_entities_by_id_and_uuid`,
  `menus::tests::a_player_throws_a_stack_from_their_eyes_the_way_they_look` and
  `api_common::tests::a_block_entitys_update_packet_holds_its_update_tag`; the menu test's thrown stack now starts at
  vanilla's float eye height. 368 compat, 17 translator and 1 + 1 + 41 game tests pass.
- [x] 2026-11-06 Part 13: `Level.clip`, a block's outline from the game, and the state ids fixed.
  - Round 10's game build failed: the bundles' host asked `Game` for `native_state_from_text` and
    `native_state_id_text`, which are the hook level's (`GameLevel`), not the game's. New `Game::native_state_id_of(text)`
    and `native_state_of_id(id)` (`native_level_events.rs`, the same fragment) share `state_from_text` and the new
    `state_id_text` with the hook level (`native_hooks.rs`).
  - `Level.clip(ClipContext)` (41 mods, 113 uses; `BlockGetter.clip`), where a mod's ray through the world stops, was
    missing. `shapes.rs` walks the blocks the segment crosses as 26.3's `BlockGetter.traverseBlocks` does (checked with
    javap). In each block it clips the shape the context's block mode picks: the collision boxes, the outline for
    `OUTLINE`, a full block in `minecraft:fall_damage_resetting` for `FALLDAMAGE_RESETTING`. When the fluid mode takes
    the fluid there (`SOURCE_ONLY` a source, `ANY`, `WATER` one in the water tag), it also clips the fluid's box, as high
    as `FluidState.getHeight` says. The nearer hit wins. A segment that starts inside a shape hits a thousandth of the
    way in, inside, on the face looking back (`VoxelShape.clip`). A miss is at `to`, facing back along the segment
    (`Direction.getApproximateNearest`, in floats).
  - Also new: `VoxelShape.clip(from, to, pos)`, `AABB.clip(boxes, from, to, pos)`, and
    `new ClipContext(from, to, block, fluid, entity)`, which takes the entity's collision context (the empty one for
    null); the data-class table's form takes a context.
  - The outline comes from the game. New `Host::outline_boxes(pos)` is answered by the new game helper
    `native_outline_boxes(pos)` (`game/native_shapes.rs`, fragment `native_outline_boxes.json`): the block's
    `getShape` with its random offset, what the player targets, relative to the block. A vanilla block's
    `BlockState.getShape(level, pos)` (`api::state_shape`) reads it too when the state asked is the one at `pos`; it
    was a full block.
  - Not modelled: interaction shapes (a cauldron's inside, which vanilla prefers for the face hit), visual shapes apart
    from collision ones (glass's), and the context's entity in collision shapes (scaffolding).
  - The tests' `MemoryHost` gives water and lava no collision boxes (they were full cubes). `collisions` and `outlines`
    set a position's own boxes.
  Test `shapes::tests::a_ray_through_the_level_stops_where_vanillas_does`:
  - a ray hits stone's north face at z 3, through a poppy, and the poppy's outline at z 1.3125;
  - a ray through air misses at its end;
  - water's surface is hit at 60 + 8/9 (in floats) with `ANY`, `SOURCE_ONLY` and `WATER`, and missed with `NONE`;
  - lava isn't water;
  - a ray from inside stone hits at z 3.502, inside.
  369 compat, 17 translator and 1 + 1 + 41 game tests pass.
- [x] 2026-11-06 Part 14: round 10 in game; the game's entities for `getEntitiesOfClass`; positions; the eye height.
  - Round 10 checked parts 11 to 13 in game with `/hzrun world`:
    - stone's id is 1, and id 1 is `minecraft:stone` again;
    - the break event is at -22 67 0, two blocks over the player;
    - player 2 is found by id, by UUID and as a player, and is the same object;
    - `Level.clip` straight down from the eyes gives `BLOCK at -22 64 0 face up, y 65.0, inside false`, the feet at
      65.0 on grass;
    - two diamonds are thrown at y 66.31999999284744. They lay on the ground 2.45 blocks off, and the game's
      `data get` reads `{count: 2, id: "minecraft:diamond"}`.
    No panic. The screenshot shows the diamonds in front of the eyes; the break particles over the head are out of
    frame.
  - The round showed two gaps:
    - hzcheck's `getEyeY() - 0.3F` read 66.31999998807908. The runtime's eye height was 1.62 as a double, while
      vanilla's is the float 1.62F (1.27F crouching), widened. `api_common::player_eye_height` now uses the float, and
      so does the look ray of a `getPlayerPOVHitResult` miss. The drop was already right.
    - `getEntitiesOfClass(Cow.class, box)` found no cow 3 blocks off: the runtime listed only item entities and the
      mod's own. New `Host::entities_in(box)` is answered by the new game helper
      `native_entities_in(dimension, min, max)` (`game/native_entity_queries.rs`, fragment `native_entities_in.json`):
      the network ids of the server level's entities whose box meets the box, players aside. Each entity's box is its
      type's size about its feet. `getEntitiesOfClass` and `getEntities` now also give those entities, as
      `vanilla_entities`' objects of the class asked for (`LivingEntity`, `Animal`, `Cow`...), and the players whose
      box meets the box. Create's fans, crushing wheels and deployers look for entities this way.
  - Positions (packcov5: `BlockPos.getCenter` 51 mods, `toShortString` 23, `SectionPos` 16), checked against 26.3's
    bytecode:
    - `BlockPos.getCenter`, `getBottomCenter`, `atY`, `rotate` and `clampLocationWithin`;
    - `Vec3i.toShortString`, `compareTo`, `distToCenterSqr` and `closerToCenterThan`;
    - `Vec3i.toString` (`BlockPos{x=1, y=2, z=3}`), also in string concatenation, where it was
      `net.minecraft.core.BlockPos@...`; a `Vec3` concatenated is vanilla's `(1.5, 64.5, -2.5)`;
    - `SectionPos`: its `of` forms, packing, coordinates, block bounds, origin, centre, chunk, offsets and the static
      conversions.
  - Probe artefacts in packcov5's list, already answered in real use:
    - `Slot.getItem`: the probe's slot has no container;
    - `BlockState.cycle`: the probe passes a null property;
    - `Position.x()` on a `Vec3`: the probe has no receiver.
  Tests `entities::tests::a_mod_finds_the_games_mobs_and_players_by_class_and_box` and
  `api_common::tests::positions_and_sections_answer_as_vanillas`. 371 compat, 17 translator and 1 + 1 + 41 game tests
  pass.
  - [ ] Outlines and collisions from shapes checked by hand in game; the 5 slow `mod_content` tests (need
    `.local/horizon-client/26.3`, absent in this checkout; the prepared copy is `scratchpad/prepared`).
  - Gaps seen: `ItemBlockRenderTypes.setRenderLayer` (cutout layers), fluid pipes' side properties (vanilla
    `PipeBlock`'s getShape isn't Create's), item `getDefaultInstance`, title-screen panorama calls.
- [x] 2026-11-06 Part 15: round 11 in game; the player's box; data component maps and patches.
  - Round 11 checked part 14 in game with `/hzrun world`:
    - positions: `BlockPos{x=-35, y=64, z=-7}`, centre `(-34.5, 64.5, -6.5)`, short `-35, 64, -7`, section `-3 4 -1`
      from `-48, 64, -16`, and the block above compares -1. All are vanilla's.
    - the eye check: hzcheck's `getEyeY() - 0.3F` and the drop's y are both 65.31999999284744, vanilla's value.
    - `getEntitiesOfClass` still found nobody: "no cow", and "living near" was empty, without even the player. The log
      showed `ServerPlayer.getBoundingBox` unmapped. The box a mod asks around (`player.getBoundingBox().inflate(6)`)
      was null, so the query had no area.
  - The player's own box (`api_common`): `getBoundingBox` is vanilla's `EntityDimensions.makeBoundingBox`, 0.6 by 1.8
    (1.5 crouching) with half the width a float. Also `getBbWidth`, `getBbHeight`, `getEyeHeight` (the float) and
    `ServerPlayer.serverLevel()`, which was unmapped too. The entities test now asks around the player's own box,
    inflated, as hzcheck and Create do.
  - Data component maps (packcov5m: `ItemStack.getComponents` 41 mods, `applyComponents` 26 with a map and 22 with a
    patch, `DataComponentMap$Builder.set` 22 + 10, `DataComponentMap.get` 17 and `has` 14):
    - `ItemStack.getComponents()` (also through `DataComponentHolder`) is a live view: `get`, `getOrDefault` and `has`
      go through the stack's own calls, so the damage and the defaults answer as the stack's do. Also `isEmpty`,
      `size`, `keySet`, `iterator` and `stream` (each a `TypedDataComponent`), `getTyped` and `filter`.
    - `DataComponentMap.builder()`: `set` (a type or its holder; null takes it out), `addAll`, `build`. Also
      `DataComponentMap.EMPTY` and `composite`.
    - `DataComponentPatch.builder()`: `set`, `remove`, `build`. A built patch counts in `isEmpty`.
    - `ItemStack.applyComponents(map)` sets what was set on the map's stack, and its damage. Vanilla keeps a value equal
      to the item's default out of a stack's patch, so the defaults are not copied.
    - `applyComponents(patch)` and `applyComponentsAndValidate(patch)` make the patch's sets and removals. A saved or
      recipe patch is read with the types' codecs.
    - `getComponentsPatch()` keeps the stack's values on the patch, so `other.applyComponents(stack.getComponentsPatch())`
      carries a transient type's value too.
    - A patch's `entrySet`, `get` (an `Optional`, empty where it removes) and `size`.
    - `Item.components()`: what a new stack of the item reads.
    - `DataComponentType.equals` and `hashCode` work by identity.
  - Not yet: `BlockEntity$DataComponentInput` (13 + 8 mods; Create's), `collectImplicitComponents` and
    `applyImplicitComponents` (6), `FluidStack.getComponents` (7) and `applyComponents` (5 + 4),
    `PatchedDataComponentMap`'s own calls (`asPatch` 6), `DataComponentPatch.forget` (4).
  Test `components::tests::component_maps_and_patches_carry_a_stacks_components`; the entities test checks the player's
  box. 372 compat and 17 translator tests pass. The game tests were not rerun: no game code changed.
- [x] 2026-11-06 Part 16: a vanilla tool's durability; a placed mod block's `setPlacedBy` and its block entity's
  components.
  - A stack a mod makes of a vanilla tool (`new ItemStack(Items.IRON_SWORD)`) had no durability: `getMaxDamage` was 0,
    so it never wore and its damage read 0. New `Host::item_max_damage` is answered by the game's existing
    `native_item_max_damage` (API `hand_stack`). A first try added a second helper of that name to the game, and round
    12's build failed on the duplicate (`E0592`). The game's own helper is used now.
  - Placing a mod block ran only NeoForge's `EntityPlaceEvent`. Vanilla's `BlockItem.place` does two more things once
    the block is set, and the runtime now does both, in vanilla's order (`glue::place_block`, which now gets the
    placing item; the main hand's stack when it holds that item, else the off hand's):
    - the block entity reads the stack's components (`BlockEntity.applyComponentsFromItemStack`: the mod's
      `applyImplicitComponents` with a `DataComponentInput`, `get`/`getOrDefault` by type or holder). It keeps the
      components set on the stack that it didn't read, `block_entity_data` aside (`components()`);
    - the block's own `setPlacedBy(level, pos, state, placer, stack)`.
  - Who uses them, scanned in the jars: `setPlacedBy` in 32 Create classes (`KineticBlock`, the base of every kinetic
    block, `DeployerBlock`, `ToolboxBlock`, `SteamEngineBlock`, `BacktankBlock`...) and 3 of Farmer's Delight's.
    `applyImplicitComponents` and `collectImplicitComponents` in Create's toolbox, backtank and schematicannon, and in
    Farmer's Delight's cooking pot, which reads its meal and container this way.
  - A block entity's `collectComponents()` (what it kept, then its `collectImplicitComponents`: the cooking pot's
    `getAsItem`), `components()`, `setComponents`, and `applyComponentsFromItemStack` called by a mod. Vanilla's own
    `applyImplicitComponents` and `collectImplicitComponents` do nothing; packcov5m counted the mods' `super` calls
    to them as missing (6 each).
  - Not yet: the game's loot `copy_components` from a mod's block entity. The cooking pot's loot table copies
    `custom_name`, `farmersdelight:meal` and `farmersdelight:container` from it, and that needs the game to ask the
    runtime. A block entity's kept components are not saved with it.
  Test `glue::tests::a_placed_mod_block_reads_its_stack_and_hears_who_placed_it`; 374 compat and 17 translator tests
  pass.
- [x] 2026-11-06 Part 17: round 12 in game; a mod's component types in item arguments; a mod block entity's
  components on its drops.
  - Round 12 (commit `f0790e0`, hzcheck jar with the component checks) in game:
    - `getEntitiesOfClass(LivingEntity, player box inflated)` now finds the player beside the mobs: "living near:
      minecraft:cow 448, minecraft:pig 449, minecraft:player 2".
    - Components: "max 250, the copy's damage 5, built 7, the map has damage true (5), its patch empty false, a
      stick's true", as vanilla gives them.
    - A deployer placed by the player has its owner (`setPlacedBy`): `data get block` shows `Owner` as a UUID int
      array.
    - The cooking pot step failed before placing anything: `item replace ... with
      farmersdelight:cooking_pot[farmersdelight:meal={...}]` answered "Unknown item component 'farmersdelight:meal'".
  - Cause: item arguments (`ItemParser`) take the types in the command registries (`persistent_components`, the
    `data_component_type` registry), which held vanilla's only. The game registers the mods' persistent component
    types at start (`native_component_types` → `horizon_protocol::item::custom`), and its stacks already keep such a
    component (`StackData.other`) for the hand to give the runtime. `build_registries` now adds the registered types
    to both lists, so `/give` and `/item` take `farmersdelight:meal=...` and `!farmersdelight:meal`. The value is
    checked as SNBT only; the mod's codec reads it when the runtime first sees the stack.
  - Mod block entities' components on their drops. Vanilla's `copy_components` loot function with source
    `block_entity` copies `BlockEntity.collectComponents()` onto the dropped item. The game handled it for vanilla's
    own block entities only (heads, hives, banners, shulker boxes, pots). In this pack, 990 such functions in 22 jars
    use it: Refined Storage 351, FramedBlocks 232 (`camo_list`), Mekanism 113 (owner, energy, items, side
    config...), SecurityCraft 81, Domum Ornamentum 44, Waystones 38, Comforts 32, Create 19 (toolbox contents and
    UUID, backtank air, enchantments), Farmer's Delight 14 (the cooking pot's meal and container, cabinets' names),
    Create Big Cannons 13...
    - `Items::block_entity_copies` (horizon-items): the block's loot table's functions with source `block_entity`,
      each with its `include`/`exclude` and the entry's item (`BlockEntityCopies`).
    - New hook `native_block_entity_components(pos, entity, components)`, fragment
      `block_entity_components.json`. The game asks it as a player (`break_block_as_player`) or an explosion
      (`explosion_hit_block`) breaks a mod block, while the block and its block entity are still there, when the
      table copies anything from the block entity (`game/native_block_entity_drops.rs`). What the mod names and the
      table copies waits in `Pending::copied` for the dropped item (`attach`).
    - Bundle template `block_entity_components`: the runtime's `glue::block_entity_components` collects the block
      entity's components (its live object, else one made from its saved data) and writes each with its type's
      codec (`components::saved`).
    - Not yet: a mod block broken by the world itself (`GameLevel::drop_resources_now`: a lost support, `/setblock
      destroy`) runs no game hook, so its drops don't get them; vanilla components other than enchantments (a
      `custom_name` the runtime holds) aren't written back.
  - A block entity's kept components (what it didn't read of the stack it was placed from) are saved with it under
    `components`, as vanilla's `saveWithoutMetadata` writes them, and read back when it is made again from saved data
    (`loadWithComponents`): they survive the world being saved and loaded.
  Tests `chat::tests::item_arguments_take_a_mods_component_types`, `items::tests::a_tables_copies_from_the_block_entity`,
  `glue::tests::a_block_entitys_kept_components_are_saved_with_it`, and the placement test now breaking the pot. 375
  compat, 17 translator and the items tests pass; the game tests (48, with `chat::tests` and the hook catalog) pass.
- [x] 2026-11-06 Part 18: NeoForge's `FakePlayer`; the cooking pot's recipe lookup (`RecipeManager.createCheck`,
  `RecipeWrapper`, `RecipeMatcher`).
  - Round 13 in game: `/give` with `farmersdelight:meal` works; the placed pot shows the stew (display slot 6); the
    player picked the dropped pot up at once, so the scenario now reads the player's inventory for it. The log
    showed a new miss: `RecipeManager.createCheck`, so the pot found no recipe and never cooked.
  - Fake players (`compat/fake_players.rs`, first in the call chain). A mod makes one to act in the world by itself:
    Create's deployer holds and uses its item through `DeployerFakePlayer`, other mods through
    `FakePlayerFactory.get`. Before this, a call on one (`setItemInHand`, `getMainHandItem`, `getInventory`) fell to
    the player modules, which answer for the acting player: the deployer would have taken and given the real
    player's items. Now `FakePlayer.<init>` gives the object its own state: level, profile, place, look, sneaking,
    an entity id below the game's (negative), and an inventory of 41 slots of its own (`$slots`: 36, armor, off hand).
    - Its calls are answered from that state: identity through its profile (a mod's profile class overrides
      `getId`/`getName`, as Create's `DeployerGameProfile` does: called virtually), place and look, eye height, box,
      view vector and facing, hands and equipment slots, `addItem`, sneaking, and `FakePlayer`'s own overrides (no
      messages, stats or menus; invulnerable; `isFakePlayer`; rides nothing; its server). The rest go on to the
      other modules as `ServerPlayer`'s (a mod subclass's inherited calls reach the API as `FakePlayer.x`; they are
      forwarded as `ServerPlayer.x`).
    - `instanceof`: `FakePlayer` is a `ServerPlayer`, `Player`, `LivingEntity` and `Entity` (`api_is_a`).
    - `Inventory` calls on a fake player's inventory reach its own slots (`menus.rs`: the inventory call runs with
      `FAKE_INVENTORY` set, read by `inventory_get`/`inventory_set`, the selected slot, `infinite` (never),
      `whole_inventory` (always) and drops (`fake_drop`: from its place and look, into the level)).
    - New for every player's inventory: `Inventory.save(ListTag)`, `load(ListTag)` (vanilla's `Slot` numbers: 0-35,
      armor 100-103, off hand 150) and `dropAll()`. The deployer saves its fake player's inventory as `Inventory`
      and loads it when it makes the player again.
  - Recipes: `RecipeManager.createCheck(type)` gives a `CachedCheck` whose `getRecipeFor(input, level)` finds the
    type's first matching recipe, the last one found tried first (vanilla's cache). NeoForge's `RecipeWrapper`
    (an item handler as a `RecipeInput`: `getItem`, `size`, `isEmpty` through the handler's own methods, so a mod's
    handler answers) is modelled by hand now, so it left the data class table. NeoForge's
    `RecipeMatcher.findMatches(inputs, tests)` is ported from its bytecode (the claim pass, then backtracking), so a
    mod reading the pairing gets NeoForge's.
  - Tests `fake_players::tests` (hands and inventory its own, the acting player's untouched; place, look, facing,
    UUID and id; save and load), `recipes::tests::a_cached_check_finds_the_recipe_a_pots_slots_hold` (FD's `matches`
    as the mod writes it, through `RecipeWrapper` and `RecipeMatcher`), `recipe_matcher_pairs_inputs_as_neoforge_does`
    (pairings worked by hand from NeoForge's algorithm). 380 compat and 17 translator tests pass.
  - The round scenario now hands a deployer three iron ingots (`interact`) and reads what it saves (`Inventory`).
- [x] 2026-11-06 Part 19: a deployer's block item used through the game; a fake player's own attributes; NeoForge's
  click hooks for a mod's own player.
  - Round 14 in game: the cooking pot dropped and picked up keeps its meal (`farmersdelight:meal` = beef stew), and
    the placed pot shows it; the attack, attribute and effect steps hold. The hand-placed deployer failed: the
    terrain this run had nothing solid under it (the scenario now sets stone there). The powered deployer stayed
    `WAITING` with its 4 stone: its `activate` ran, then met `ServerLevel.mayInteract` (unmapped, so false: it
    gave up) and, before it, the attribute modifier calls.
  - A block item used on a block by a mod (`ItemStack.useOn`, `BlockItem.useOn`, NeoForge's `onItemUseFirst`): the
    mod item's own `useOn` where it has one; a block item places through the game (`Host::place_item_block`; the
    game's `native_place_item_block`, `game/native_actors.rs`): vanilla's `BlockItem.place` rules (the clicked block
    replaced, or the one beside it), the block entity and the item's data on it, the place sound and a mod block's
    placement hook, as the dispenser places. The stack shrinks unless a real creative player used it; `CONSUME`
    on a server.
  - `UseOnContext(player, hand, hit)` and `Item.use` read the hand and level through the whole call chain: a fake
    player's are its own (before, they were the acting player's). A fake player's item use is its own too
    (`startUsingItem`, `getUseItem`, `isUsingItem`, `getUsedItemHand`, `stopUsingItem`, `releaseUsingItem`).
  - A fake player's attributes (`attributes.rs`). It is no entity of the game's: it reads as vanilla's player by
    default, and what a mod changes stays on it (`$attributes`, by the game's id: base and modifiers). Before,
    its reads and changes went to the acting player. `AttributeView::apply` (compat-core) is now the one rule for
    a change, the memory host's and a fake player's.
  - `AttributeMap.addTransientAttributeModifiers` and `removeAttributeModifiers` (a multimap), as vanilla's: each
    modifier on its attribute in place of one of its id; taken off by id.
  - `ItemStack.getAttributeModifiers()` (NeoForge's): the stack's own component, else the mod item's
    `Item.Properties.attributes`, else `EMPTY`. A tool's `createAttributes` is now vanilla's two entries (base
    attack damage and speed in the main hand), not only the two numbers.
  - `EnchantmentHelper.forEachModifier` (by `EquipmentSlot` and by `EquipmentSlotGroup`) and
    `runIterationOnItem(stack, visitor)`: the 8 vanilla enchantments with attribute effects, read from 1.21.1's
    data (`effects."minecraft:attributes"`), their amounts as `LevelBasedValue` computes them (linear, levels
    squared, a fraction), each modifier's id the effect's with the slot after a slash.
  - `Level.mayInteract(player, pos)`: yes (a server's spawn protection and border aren't asked for mods).
  - A block state asked by a mod: `canBeReplaced(BlockPlaceContext)` (the mod block's own, else a replaceable block
    not of the item placed); `useItemOn`, `useWithoutItem` and `attack` (the mod block's own; a vanilla block's
    pass for now).
  - `CommonHooks.onRightClickBlock`, `onLeftClickBlock` and `onInteractEntity` for a mod's own player: the event,
    posted with that player's stack (`getItemStack`), given back to the caller (`isCanceled`, `getUseBlock`,
    `getUseItem`; `onInteractEntity` gives the result a listener canceled with, else null);
    `LeftClickBlock.getAction`. `ItemStack.doesSneakBypassUse`: the mod item's own, else no.
  - A vanilla block's item is a `BlockItem` where the game says it places a block (`instanceof`): Create's deployer
    checks it before it places (it won't place against a block it can't replace, and waits while its target is
    already the block it holds). Without a host to ask (none at that point), an `Item` as before.
  - Round 15 in game: the deployer misses above are gone from the log, the deployer placed by hand keeps its owner,
    and the powered one cycles (the client draws its pole moving). It placed nothing because it was punching: a
    deployer saved without `Mode` reads `PUNCH`, the first of Create's modes (`NBTHelper.readEnum`), as it would
    in Java, so the scenario now sets `Mode:"USE"`. The hand-off didn't happen because `c interact` clicked its
    front, which passes (`DeployerBlock.useItemOn` swaps hands for a click on the back, sides or top: the hit's
    distance from the back along its facing at most 0.75); the scenario clicks its top now. A third deployer,
    punching stone with an iron pickaxe, measures the breaking path.
  - Tests: `fake_players::tests::a_fake_player_places_the_block_it_holds` (stone from its own stack beside the face
    clicked, 4 to 3, `CONSUME`; the acting player's hand untouched; stone is a `BlockItem`, iron isn't), `a_fake_players_attributes_are_its_own`
    (Create's `activate` sequence: a sword's +5 and efficiency III's mining efficiency on the fake player while it
    acts, 1 + 5 and 3² + 1, then off again; the acting player's attribute untouched),
    `events::tests::a_fake_players_click_is_posted_with_its_own_stack`. 383 compat and 17 translator tests pass;
    the game tests (48) pass.
- [x] 2026-11-06 Part 20: round 16 in game; a mod's own player breaking blocks (NeoForge's break path); a right
  click's real point.
  - Round 16 in game: the `USE` deployer placed one stone in front of it and then waited, as in Java (it places only
    into a block that `canBeReplaced`, and its target holds stone now). The hand-off "failure" was the scenario's:
    the middle of a deployer's top is its filter slot (`DeployerFilterSlot`, a `ValueBoxTransform.Sided` on each
    side but its front, its back and its shaft's), so Create's `ValueSettingsInputHandler` takes that click,
    cancels `RightClickBlock` and sets the filter to the held item (`FilteringBehaviour.onShortInteract`), as in
    Java. The scenario now reads that filter, then hands the ingots over by the shaft's side (north for a
    deployer facing west) away from the front. The punching deployer's first misses were
    `BlockState.getDestroyProgress` and `BlockState.getSoundType`.
  - Found on the way: a right click gave a mod block the block's centre as the hit's location
    (`glue::use_block`); it is the click's own point now (Create's deployer takes an item only from a click at
    most 0.75 from its back along its facing).
  - `block_breaking.rs` (new module, second in the chain): NeoForge 1.21.1's break path for a mod's own player,
    as Create's deployer punches (`DeployerHandler.tryHarvestBlock`) and its drills and saws break
    (`BlockHelper.destroyBlockAs`): `BlockState.getDestroyProgress` (vanilla's: hardness, the right tool's 30 or
    100, `Player.getDigSpeed`: the tool's speed, mining efficiency, haste and conduit power, mining fatigue, the
    block break speed, submerged mining speed with its eyes in water, a fifth off the ground, then
    `PlayerEvent.BreakSpeed`), `EventHooks.doPlayerHarvestCheck` (`hasCorrectToolForDrops`, then
    `PlayerEvent.HarvestCheck`), `BlockState.canHarvestBlock` and `requiresCorrectToolForDrops`,
    `CommonHooks.fireBlockBreak` (`BreakEvent`, cancelled beforehand for a weapon in creative, a restricted game
    mode, a game master block), `ItemStack.getDestroySpeed`, `isCorrectToolForDrops` and `mineBlock` (a tool wears
    by its `damagePerBlock` on a block that doesn't break at once; shears by one on any but fire),
    `EventHooks.onPlayerDestroyItem`, `BlockState.onDestroyedByPlayer` (server: `removeBlock`; client: the
    fluid's block, flags 11), `Block.destroy`, the static `Block.getDrops` (the block's loot rolled by the game
    with the tool and its enchantments, a block entity's contents kept for the break), `spawnAfterBreak` (an
    infested block's silverfish), `Level.destroyBlockProgress` (the cracks, to the players within 32 blocks),
    `BlockState.getSoundType` (both), a player's `gameMode` (`ServerPlayerGameMode`: a fake player's is
    survival) and `blockActionRestricted`. A mod block's or item's own override of each answers first; a
    `super.` call gets vanilla's.
  - The game's side (`game/native_actors.rs`, fragment `actors.json`): `native_state_requires_tool`,
    `native_item_tool` (speed, right for drops, wear per block), `native_state_sounds`, `native_block_loot`
    (`block_loot_rolled`, split out of `block_loot_at`, which adds the mods' `BlockDropsEvent` hook),
    `native_spawn_after_break`, `native_destroy_block_progress`. The host trait's matching reads and writes, the
    memory host's for tests, and `HandStack::enchantments`.
  - Also: a block's `SoundType` from the game's sounds is vanilla's own constant (`blocks::sound_type_of`, so
    `== SoundType.STONE` holds), else a new one with them; a mod block's sound type and required tool come from
    its `Properties` or the vanilla block they copy. `Level.removeBlock` and `destroyBlock` leave the block's
    fluid (a waterlogged block leaves water). `ItemStack.hurtAndBreak` asks the entity wearing the stack about
    creative (a fake player's tool wears; before, the acting player's mode spared it). A fake player is never on
    the ground, has no water at its eyes and may not use game master blocks (`FakePlayer.tick` does nothing).
  - Not yet: a mod block's own `getDrops(state, LootParams.Builder)` (its loot table answers), a mod tool the game
    doesn't know (bare-hand speed), Unbreaking on a stack outside the acting player's hands, an adventure
    player's `can_break`, listeners that receive cancelled events, `DoublePlantBlock` checks on vanilla blocks,
    and the drills' and saws' `BlockDropsEvent` and experience.
  - Tests: `block_breaking::tests::a_fake_player_breaks_stone_with_a_pickaxe_as_the_deployer_does` (the deployer's
    calls in order: progress 6 / 5 / 1.5 / 30, stone's sound type and hit sound, survival, the `BreakEvent`
    heard with the fake player, the pickaxe worn by one, air left, cobblestone into the fake player's
    inventory, the silverfish check and the cracks told to the game, nothing unmapped),
    `break_speed_and_harvest_check_listeners_decide` (a listener's doubled speed and refused harvest: / 100; bare
    hand on stone: 1 / 5 / 1.5 / 100). 385 compat and 17 translator tests pass; the game tests (48) pass.
- [x] 2026-11-06 Part 21: round 17 in game, the deployer punching; vanilla blocks' and items' own classes for
  `instanceof`.
  - Round 17's first build failed on Part 20's drops binding: `nbt(&x?)`, where the `?` under `&` is checked
    against the `Tag` the call expects, not the `&Tag` it gives (`?` operator has incompatible types, once per
    engine). The binding passes the component's `&Tag` itself now. The unit tests don't compile the bundle's host
    bindings; only the game build does.
  - Rerun in game: the punching deployer (iron pickaxe, stone two blocks ahead, creative motor at 32 rpm, 200
    ticks) broke the stone (air left), its pickaxe took 1 damage (`minecraft:damage: 1`), and the cobblestone
    went to the deployer's `Overflow`, where Create moves what lands in its fake player's inventory past the held
    slot, as in Java. Nothing unmapped came from the break path: the log's 61 unmapped lines are the client
    screens, GL, config and Flywheel ones known before. The place-mode deployer, the entity, attribute, effect
    and vitals checks answered as in round 16.
  - `instanceof` on a vanilla block or item: the runtime stands for `Blocks.ICE` by a `Block` naming
    `minecraft:ice`, so `instanceof IceBlock`, `LeavesBlock`, `BonemealableBlock` or `EntityBlock` (a chest's)
    was false and `getClass()` gave `Block`; `instanceof SwordItem` or `TieredItem` on vanilla items likewise.
    Measured: Create and Farmer's Delight have 210 `instanceof` sites on vanilla block and item classes, 173 of
    them on classes past `Block`, `Item` and `BlockItem` (`BaseRailBlock` 10, `LiquidBlock` 6, `DoorBlock`,
    `FarmBlock` and `SpawnEggItem` 5 each, `CropBlock`, `SlabBlock`, `SimpleWaterloggedBlock` 4 each; the
    drills' and saws' `BlockHelper.destroyBlockAs` melts broken ice by `instanceof IceBlock`).
    - New `tools/vanilla_classes.py` writes `horizon-compat/src/vanilla_classes.rs` from the merged jar: each
      block's class from `Blocks`' static initialiser (`register("ice", new IceBlock(...))`; its helpers such as
      `log` or `stair` by what they construct; the six registered by a `references.Blocks` key), each item's
      from `Items`' (`registerItem` by name or key, `registerBlock` by its block's id, factories such as
      `SmithingTemplateItem.createNetheriteUpgradeTemplate`), and the 387 classes' superclasses and interfaces
      from their headers. 892 of 1060 blocks and 399 of 1333 items aren't plain `Block`, `Item` or `BlockItem`
      (those are left out: the runtime makes them anyway).
    - The core's new `Rt::type_of` gives such an object its 1.21.1 class, through the new entry point
      `Fns::vanilla_class` (`api_common::vanilla_class`, used directly when a bundle inlines the runtime).
      `instanceof` uses it (generated code's static checks still cached, now by that class), as do
      `Class.isInstance`, pattern `switch` and `getClass()`.
    - `api_is_a` walks the table first, so a mod's block extending `CropBlock` is a `BonemealableBlock`, a
      `ChestBlock` an `EntityBlock`, a `SwordItem` a `TieredItem`.
    - A block or item 1.21.1 lacks (26.3's pale oak) keeps the runtime's class.
  - Tests: `api_common::tests::vanilla_blocks_and_items_are_their_1_21_1_classes` (ice, stone, oak leaves, wheat
    and a chest against 1.21.1's declared classes; static checks for two blocks of one runtime class; sword,
    diamond, torch items; `getClass()`; a mod crop through `CropBlock`) and
    `the_vanilla_class_tables_are_sorted_for_their_searches`. 387 compat and 17 translator tests pass.
- [x] 2026-11-06 Part 22: Create's drills and saws (`BlockHelper.destroyBlockAs`): NeoForge's `BlockDropsEvent`, a
  block's experience, ice melting into water; round 18 in game.
  - What `destroyBlockAs` calls, in order (its bytecode): the fluid and block state, `levelEvent(2001)`, the block
    entity; with a player, `BreakEvent`, `ItemStack.mineBlock` and `Stats.BLOCK_MINED`; then on a `ServerLevel` with
    `doTileDrops` (and no creative player) `Block.getDrops`, `new BlockDropsEvent(level, pos, state, blockEntity,
    new ArrayList(), player, tool)` posted for any breaker (a drill's is null), `popExperience` of its
    `getDroppedExperience()`, the drops to the caller's consumer (a drill throws them out as `ItemEntity`s), silk
    touch's holder for ice, `IceBlock.meltsInto()` over a block that `blocksMotion()` or a liquid,
    `spawnAfterBreak` and `setBlockAndUpdate` to the fluid's block. Missing were the event, the experience, ice
    and `blocksMotion`.
  - `BlockDropsEvent`: NeoForge's constructor keeps level, position, state, block entity, drops, breaker and tool
    and draws the experience, `EnchantmentHelper.processBlockExperience(level, tool, state.getExpDrop(level, pos,
    blockEntity, breaker, tool))`. Its getters, `setCanceled`, and `setDroppedExperience`, which refuses a negative
    amount as NeoForge's `Preconditions.checkArgument` does (`IllegalArgumentException`, "May not set a negative
    experience drop."). `world_events` now answers `BlockEvent.getLevel`, `getPos` and `getState` on its own events.
  - `BlockState.getExpDrop` (NeoForge's): a mod block's own override; a mod block made as vanilla's
    `DropExperienceBlock(IntProvider, Properties)` draws from its range (the constructor keeps it as vanilla's
    `xpRange`) with the level's random; a vanilla block's is what the game draws (the new
    `Host::block_experience`: ore ranges, a spawner's). `Block.getExpDrop` as a mod block's `super.` call answers
    the same without the override.
  - `processBlockExperience`: silk touch's `block_experience` effect leaves none; vanilla's other enchantments
    don't change it. `Block.popExperience(level, pos, amount)` on the server: the game's `ExperienceOrb.award` at
    the block's centre (new `Host::pop_experience`), only with `doTileDrops` on, as vanilla checks.
  - `IceBlock.meltsInto()`: water's default state (ice is an `IceBlock` since Part 21). `BlockState.isSolid()`
    (vanilla's `legacySolid`) as the game has the block (new `Host::block_solid`); a mod block the game doesn't
    know is solid when its `Properties` force it. `blocksMotion()`: solid, but never a cobweb or a bamboo sapling.
  - New native helpers (`native_actors.rs`, in `tools/native-hook-fragments/actors.json`):
    `native_block_experience(state)`, `native_pop_experience(pos, amount)` and `native_state_solid(state)`, bound
    by the bundle's server host.
  - Tests: `block_breaking::tests::a_drill_pops_an_ores_experience_through_block_drops_event`: diamond ore's 5 from
    the host, 7 after a listener adds 2, the breaker kept, `popExperience` leaving one orb of 7 at the block's
    centre; a silk touch pickaxe leaves none; ice melts into water; stone `blocksMotion`, a cobweb and air don't;
    a mod's `DropExperienceBlock` with `ConstantInt.of(4)` gives 4; nothing unmapped. 388 compat, 17 translator and
    48 game tests pass.
  - Round 18 in game: the drill (a creative motor at 256 rpm behind it) broke the diamond ore in front of it within
    100 ticks (the drill's `Speed` read 256), but no diamond and no orbs appeared near it. The drops are lost
    between `destroyBlockAs`'s break and its consumer; the only unmapped call on the way is
    `Level.restoringBlockSnapshots` (an unmapped field reads 0, the false vanilla has). Traced in the harness next.
    The punching deployer works as in round 17 (stone broken, cobblestone in its `Overflow`, pickaxe damage 1).
    Corrected in Part 23: nothing was lost. The harness's drill (`DRILL=1`) drops the diamond by the ore and five
    experience at its centre; in game the test rows float at y 70 over ground at 64, so the drops fell out of the
    check's four blocks.
- [x] 2026-11-06 Part 23: a mod block's `onRemove` (new game hook `native_block_replaced`), `Level.destroyBlock` with
  its drops, `Block.dropResources`; what round 18 missed.
  - Round 18's drops, traced in the harness (`DRILL=1`, the drill's block entities ticked against `MemoryHost`): at
    256 rpm the ore breaks at tick 18, a diamond item entity lands by it, five experience orbs at its centre, level
    event 2001. The game's test rows float above the ground, so the round's check looked in the wrong place; the
    drill's step now lays a stone floor and looks within three blocks. The run's only unmapped calls were
    `BlockState.liquid()` (19 times) and the field `Level.restoringBlockSnapshots` (twice): water, lava and a mod's
    `LiquidBlock` are liquids now, and no snapshots are ever restored here (false).
  - Create calls `Level.destroyBlock(pos, true)` at 20 places: `RotationPropagator` (3, a block turning too fast
    pops), `GeneratingKineticBlockEntity.applyNewSpeed`, `BeltBlock.initBelt` (3), the `neighborChanged` of
    redstone links, hand cranks, nozzles, belt tunnels and gantry carriages that lost their support, and the
    `onRemove` of pulleys (3) and water wheels. It only set the fluid's block: nothing dropped. Now as vanilla: air
    isn't destroyed; level event 2001 (not for fire); with drops `Block.dropResources(state, level, pos,
    blockEntity, entity, ItemStack.EMPTY)`; then the fluid's legacy block.
  - `Block.dropResources`, its three static forms, as NeoForge has them: on a `ServerLevel`, `getDrops` popped about
    the block (`popResource`'s offsets) and captured, then `CommonHooks.handleBlockDrops`: a `BlockDropsEvent`
    with them; unless a listener cancels it, they join the level, `spawnAfterBreak(.., false)` runs and the event's
    experience pops. `popResource` and `popExperience` now heed `doTileDrops`, as vanilla does (Part 22 said
    `popExperience` did; it didn't).
  - `onRemove`: 61 of Create's blocks override it (`IBE.onRemove` 25 times: a smart block entity's `destroy`, where
    a basin drops what it holds, then `removeBlockEntity`; `FluidPropagator.propagateChangedPipe` 6;
    `ConnectivityHandler.splitMulti` 2), and nothing called them: the game removes blocks itself.
    - Game: new `LevelAccess::block_replaced(pos, old, new, moved)`, called by `chunk_set_block_state` after the
      write and the block entity's removal, before the neighbours hear of it (1.21.1's `onRemove` point). The
      server's `GameLevel` keeps those of mod blocks (vanilla's are the game's) in `Spill::replaced`, and
      `flush_spill` hands them to the new hook `native_block_replaced(pos, old, new, moved)` once the level
      operation is done (fragment `tools/native-hook-fragments/block_replaced.json`, with the helper
      `native_state_id_text_of(state)`: the old state is no longer in the world).
    - Bundle: the hook calls `glue::block_replaced`. A change made while the runtime is busy (the mod's own write)
      is kept (`glue::defer_replaced`); compat's own writes (`setBlock`, `setBlockAndUpdate`, `removeBlock`,
      `destroyBlock`) run the kept ones right after the write (`drain_replaced`), so a block the mod replaces
      hears of it before its call returns, as in vanilla's `LevelChunk.setBlockState`.
    - Compat: the old state's `onRemove` override, else vanilla's (`removeBlockEntity` when the block changed and
      has a block entity). While it runs, `getBlockEntity` there still finds the old block entity, as vanilla's
      chunk still holds it. `Level.removeBlockEntity` lets the runtime's go and calls its `setRemoved` (Create's
      `SmartBlockEntity.setRemoved`: `remove()`, where a kinetic one leaves its network, then `invalidate()`).
    - Left: a change the mod's code makes through another game path while it runs waits for the next write or
      hook. A mod container the game drops itself (`ModContainer`, content mods) would drop twice if its Java
      `onRemove` also dropped it; bundles don't bind `native_block_entity_container`, so no translated mod has one.
  - Tests: `glue::tests::a_mod_blocks_on_remove_finds_its_block_entity_which_then_goes` (a change of state keeps the
    block entity; broken, `onRemove` finds it, then `setRemoved` once and it's gone; replaced by the mod's own
    `setBlock`, heard before the call returns) and
    `block_breaking::tests::destroy_block_drops_through_block_drops_event_as_the_rules_allow` (one diamond about the
    block, one orb of 7 after the listener's +2, level event 2001 each time, air not destroyed, nothing with
    `doTileDrops` off, `liquid()`). 390 compat, 17 translator and 48 game tests pass.
  - Round 19 in game (this part's build, the drill's step on a floor, a new basin step): the drill broke the
    diamond ore, its diamond lay 1.6 blocks off with two experience orbs by it (the drill's `Speed` 256); a basin
    given three iron ingots by `/setblock` (`InputItems` read back) and then set to air dropped the three ingots
    where it stood (`native_block_replaced` → `BasinBlock.onRemove` → `IBE.onRemove` → `SmartBlockEntity.destroy`
    → `ItemHelper.dropContents`). The harness (`DRILL=remove`) shows the same basin, a drill stopping at once when
    the shaft between it and its motor is broken (the shaft's `setRemoved` leaves the network), and
    `destroyBlock(ore, true)`'s diamond, five experience and level event 2001, with no unmapped call. The rest of
    the round answered as in round 18; the deployer's filter slot still reads `{}` after a click on its top with
    ingots in hand, and the ingots leave the hand (round 18 the same; to trace).
- [x] 2026-11-06 Part 24: a mod block's `onPlace` through the same hook; `Containers.dropContents`.
  - 31 of Create's blocks override `onPlace`, which 1.21.1's `LevelChunk.setBlockState` calls after the old
    state's `onRemove` while the new block is still there, and nothing called them. Its pipes, valves and pumps
    schedule a tick (a pipe's tick runs `FluidPropagator.propagateChangedPipe`), water wheels schedule one too,
    tanks and item vaults reach into their block entity (`withBlockEntityDo`), `KineticBlock.onPlace` keeps a
    block's speed through a change of state that leaves its connections alone (`areStatesKineticallyEquivalent`),
    redstone links, nixie tubes, chutes, belt tunnels, deployers and arms update themselves.
  - `native_block_replaced` now fires when either state is a mod's (a mod block placed over air or a vanilla block
    too), and a bundle's hook runs when either is its own. `glue::replaced`: the old state's `onRemove` (Part 23),
    then the new state's `onPlace(level, pos, oldState, moved)` override when the block at `pos` is still the new
    one (an `onRemove` can replace it). A mod's `super.onPlace` is `BlockBehaviour`'s, which does nothing; a
    vanilla template's own stays the game's.
  - Round 19's log had one new unmapped call, from Farmer's Delight's cooking pot `onRemove` (now that it runs):
    `Containers.dropContents(level, pos, NonNullList)`. All three `dropContents` (a position and a list, a
    position and a `Container`, an entity and a `Container`) and `dropContentsOnDestroy` now drop each stack as
    `dropItemStack` does, and `dropItemStack` splits the stack itself (`ItemStack.split`): each piece keeps the
    stack's components (a damaged or enchanted tool stays so) and the stack is empty after, as in vanilla. It made
    plain stacks of the item before.
  - Tests: `glue::tests::a_mod_blocks_on_place_hears_what_it_replaced` (placed over air, then a change of its own
    property, each with the state it replaced; not after a trap block's `onRemove` set stone there) and
    `entities::tests::dropped_contents_keep_their_components_and_leave_their_slots_empty` (40 onions in pieces of
    10 to 30 about the block's corner, a sword with damage 7 still at 7, the list's stacks empty). 392 compat, 17
    translator and 48 game tests pass.
- [x] 2026-11-06 Part 25: a mod block's own `getDrops(state, LootParams.Builder)`; `LootParams.Builder`.
  - Six of Create's blocks override `getDrops(state, builder)` and nothing called them: the game rolled their loot
    tables alone. A gearbox on a horizontal axis drops as the vertical gearbox (its loot table says gearbox), a belt
    adds the shaft its pulley held, a backtank its block entity's components (its air, its enchantments), a
    clipboard its pages, a shop's table cloth its request data, a cart assembler its rail. They read the block entity
    from the builder (`getOptionalParameter(BLOCK_ENTITY)`) and call `super.getDrops` for the loot table.
  - New `loot_params` module: `LootParams.Builder` and `LootParams` as 1.21.1 has them (parameters by identity, a
    null optional one taken out, a missing one `NoSuchElementException`, dynamic drops, luck; `create` checks only
    the set's required parameters, as NeoForge lets mods pass their own). `BlockState.getDrops(builder)`: the mod
    block's own override, else `BlockBehaviour.getDrops`, the loot table, which the game rolls with the builder's
    origin and tool (`Host::block_drops`). The static `Block.getDrops(state, level, pos, blockEntity, entity,
    tool)` that Create's drills, saws and deployers call now goes through it with vanilla's parameters, so their
    breaks get the overrides too. `LootParams` left the generated data-class table (modelled here now).
  - The game's own breaks: bundles bind `native_loot_drops` (its API 75 hook). For a block of their own with the
    override, `glue::loot_drops` builds the builder `Block.getDrops` builds (the block's centre, the tool, the
    player, the block entity, an explosion's radius) and calls it; its `super.getDrops` gets back the stacks the game
    rolled (once), and a stack it leaves as it was stays the game's own stack. The game rolls a player's loot after
    the block is gone, so the block entity its `onRemove` let go is kept for that roll (until the next server tick).
  - The harness (`DRILL=drops`, Create's own code): a gearbox on the x axis drops one vertical gearbox both through
    the game's break and through `Block.getDrops`; on the y axis its loot table's gearbox, the game's own stack
    kept; a backtank its loot. The backtank's override called the one unmapped call, `DataComponentPatch.forget
    (predicate)`, now modelled: the patch without the types the predicate takes, `EMPTY` when nothing is left.
  - Tests: `loot_params::tests::a_builder_holds_its_parameters_until_create_checks_the_required_ones`,
    `components::tests::a_patch_forgets_the_types_a_predicate_takes` and
    `glue::tests::a_mod_blocks_get_drops_answers_with_the_block_entity_it_had` (a belt's override through
    `Block.getDrops` in a mod's code rolls its loot then the shaft's; through the game's break after `onRemove`, it
    gets the block entity and the player, its `super.` call the game's stack back, kept as the game's; a vanilla
    block's drops stand; a block entity kept for drops that never came goes at the next server tick). 395 compat,
    17 translator and 48 game tests pass.
  - Round 20 in game (this part's build): a gearbox on the x axis broken by the player in survival with an iron
    pickaxe dropped one `create:vertical_gearbox`, which the player picked up (no gearbox), as in Java. No step
    regressed; `Containers.dropContents` left the unmapped list (part 24), and one client GUI call joined it
    (`ObjectSelectionList.getY`, Catnip's config screen). The drill's step found no experience orb near the ore
    this time: the harness (`DRILL=1`, this build) still pops the ore's 5, so the round now prints the player's
    experience around that step (an orb that reached the player was taken in).
  - The deployer's filter slot (rounds 18 to 20: `Filter` stayed `{}`, the ingots left the hand): the round's
    click missed the slot. `interact X Y Z up` aims 0.45 from the block's centre towards the face, ~0.2 from the
    top's middle, and Create's `ValueSettingsInputHandler` takes a click only within its slot's `testHit` (~0.2),
    so `DeployerBlock.useItemOn` took the ingots, as it would in Java. The harness (`DRILL=slot`, Create's own
    handler through `RightClickBlock`) with a click at the top's middle: the filter holds the three ingots, and
    they stay in hand. The round now aims at that point (`look-at`, then `use-item`).
- [x] 2026-11-06 Part 26: a mod block's own part when the game's player breaks it (`playerWillDestroy`,
  `onDestroyedByPlayer`).
  - NeoForge's `ServerPlayerGameMode.destroyBlock`, once the `BreakEvent` lets the player break the block: the
    block's `playerWillDestroy(level, pos, state, player)`, then `removeBlock`, which is the state's
    `onDestroyedByPlayer(level, pos, player, willHarvest, fluid)` (NeoForge's default: `level.removeBlock`), then
    `destroy` and, when harvested, `playerDestroy`. The game ran vanilla's part for every block and the mods'
    `BreakEvent` listeners, but no mod block's own override: eight of Create's blocks override `playerWillDestroy`
    (its mechanical piston, extension pole and piston head take the rest of the piston with them, a water wheel's
    structure block its wheel in creative, a copycat, chain conveyor and track note a creative break, a clipboard
    drops itself in creative), and its factory panel overrides `onDestroyedByPlayer` (a panel with several gauges
    loses the one the player looks at and stays).
  - `native_destroy_block` (`glue::destroy_block`) now runs, after the `BreakEvent`, the mod block's
    `playerWillDestroy`, then its `onDestroyedByPlayer` with whether the player harvests it (never in creative,
    else `canHarvestBlock`) and the fluid there (`block_breaking::player_breaks`). The game still removes the
    block: the default `onDestroyedByPlayer`, a mod block's `super.` call, leaves the block the game's player is
    breaking to the game and answers true. An override that answers false keeps the block, which the hook tells the
    game as it tells a canceled break (unlike Java, the tool then takes no wear). A mod block's
    `super.playerWillDestroy` (`Block`'s, through any vanilla class) gives the state back: its effects (the break's
    particles, piglins angered over their gold, the `BLOCK_DESTROY` game event) are the game's, for every block.
  - The harness (`DRILL=break`, Create's own `MechanicalPistonBlock.playerWillDestroy`): a piston facing up with
    two poles and its head above, broken by the game's player in survival, takes the poles and the head with it,
    two poles dropped and a break effect for each; in creative the same without drops; the piston stays for the
    game; no unmapped call.
  - Test: `glue::tests::a_mod_block_does_its_part_when_the_player_breaks_it` (a piston's override takes the pole
    above with it, dropped in survival only, its `super.` call answering the state; a panel's override keeps the
    block while it has two gauges, its `super.` call leaves the block to the game; harvested in survival, not in
    creative). 396 compat and 17 translator tests pass; no game code changed.
  - packcov6 (this part's translator before the change, the same 367 jars of the CurseForge pack): methods
    93680/130683 (71.7%, from 69.4% in packcov5n), uses 304060/369268 (82.3%, from 80.5%); Create 2695/3994
    methods (67.5%, from 64.2%) and 22852/25380 uses (90.0%, from 88.4%); 12712 distinct missing methods (from
    13092). The probe still counts some mapped calls as missing when it has no receiver to try them on (a
    `MapCodec` from `fieldOf` for `forGetter`, a menu's `Slot`); entity model definitions (`MeshDefinition`,
    `PartDefinition`, `CubeListBuilder`, `LayerDefinition.create`, `ModelPart.getChild`, about 40 mods each) are a
    real gap.
- [x] 2026-11-06 Part 27: a block state's seed (`BlockState.getSeed`) and a mod armour's slot
  (`ArmorItem.getEquipmentSlot`).
  - Round 21's unmapped list still had two calls from Create's own code: Flywheel baking a block's model in its
    virtual level (`BlockState.getSeed(pos)`, the seed a model's random offset and texture variant draw from), and
    Create's cardboard armour (`CardboardArmorItem`, an `ArmorItem` that keeps `getEquipmentSlot`) when the game
    asked where it is worn.
  - `BlockState.getSeed(pos)`: the block's own `getSeed(state, pos)` when a mod block overrides it, else
    `Mth.getSeed(pos)`, as vanilla's `Block.getSeed`; a mod block's `super.getSeed` is the same `Mth.getSeed`.
  - `ArmorItem(material, type, properties)` keeps vanilla's fields `material` and `type`; `getType`, `getMaterial`,
    `ArmorItem.Type.getSlot` and `getEquipmentSlot` (the slot its type names) answer from them, so a mod's armour
    is worn in the slot its type takes (the game reads its `equippable` slot from it).
  - Tests: `api::tests::a_states_seed_is_its_blocks_own_else_its_positions` (stone at 10, 64, -20: vanilla's
    `Mth.getSeed`, recomputed apart from the code; a block with its own seed: that seed) and
    `blocks::tests::a_mod_armour_item_is_worn_in_the_slot_its_type_takes` (a helmet, chestplate, leggings and boots
    of a mod's own class: head, chest, legs and feet; its type and material as made).
- [x] 2026-11-06 Part 28: nested mods whose `mods.toml` headers carry a comment or spaces (Create Aeronautics).
  - packcov6 measured Create Aeronautics' jar at 0/0: it holds no class of its own, only three nested mods under
    `META-INF/jarjar/` (aeronautics 254 classes, offroad 86, simulated 795), and their `neoforge.mods.toml` write
    `[[mods]] #mandatory`. The translator compared a header with the whole trimmed line, found no mod in them, and
    took their classes for an unreached library's.
  - `bundle::toml_header` reads a table header as TOML does: whitespace inside the brackets and a trailing comment
    don't count (a `#` in a quoted key does). Everything that reads a `mods.toml` uses it: whether a jar declares a
    mod, its metadata, its mixin configs (`[[mixins]]`) and its nested mods' ids.
  - Test: `mixin_census_tests::a_bundles_nested_mods_are_its_program_however_their_headers_are_written` (a bundle
    holding a mod with a commented header, one with `[[ mods ]]` and a library: both mods' classes are translated
    and their ids read, the library's class stays out); the mixin configs test's `mods.toml` now writes its headers
    both ways. 398 compat and 18 translator tests pass.
- [x] 2026-11-06 Part 29: NeoForge's listener priorities and `receiveCanceled`.
  - The runtime's bus called an event's listeners in the order they listened and stopped at the first that
    canceled it. NeoForge's bus calls them by `EventPriority` (`HIGHEST`, `HIGH`, `NORMAL`, `LOW`, `LOWEST`; in the
    order they listen within one) and, while an event is canceled, still calls the listeners that asked for
    canceled events (`receiveCanceled = true`; one of them may let it go on). In the CurseForge pack 95 of the 367
    jars name `EventPriority` (204 classes) and 6 `receiveCanceled` (7 classes): a mod that listens at `HIGHEST`
    to act before the others, or at `LOWEST` to see what they did, got the wrong turn.
  - The translator reads `@SubscribeEvent(priority, receiveCanceled)` and registers the options of the methods
    that don't keep the default (`rt.subscriber_options`, by method key); a listener made of one carries them
    (`Rt::listen_options`), as does one `addListener(EventPriority, [boolean,] ...)` or `addListener(boolean,
    ...)` took. `EventPriority`'s constants are its enum values (they were their names, as strings).
    `neo::hearing` orders an event's listeners by priority, `neo::post` passes a canceled event only to those
    that receive canceled ones.
  - Test: `neo::tests::listeners_hear_by_priority_and_a_canceled_event_only_where_asked` (a `HIGHEST` method
    first, the `NORMAL` one that cancels, a `NORMAL` and a `LOW` listener after it don't hear it, a `LOWEST` one
    receiving canceled events does). 399 compat and 18 translator tests pass.
- [x] 2026-11-06 Part 30: a client's click as NeoForge's client takes it: `RightClickBlock` answers for the
  block and the item, the other hand waits; `InputEvent.InteractionKeyMappingTriggered`; `Direction` by name.
  - Rounds 18 to 23 left a deployer's filter empty after the player clicked its filter slot with three iron
    ingots. The server heard two clicks: the main hand's, which Create's `ValueSettingsInputHandler` cancels with
    `SUCCESS` and hands to `FilteringBehaviour.onShortInteract` (`setFilter(side, itemInHand.copy())`: the ingots),
    then the off hand's, whose empty hand cleared the filter again. On NeoForge's client
    `MultiPlayerGameMode.performUseItemOn` posts `RightClickBlock` before the block and the item act; a canceled
    event answers with its `getCancellationResult()`, the packet goes to the server either way, and
    `Minecraft.startUseItem` ends the click on a result that consumes the action (swinging on `SUCCESS`) or
    `FAIL`: the off hand is never tried. Horizon's client posted the event from the use key for both hands and
    did not act on the answer.
  - Game: a new hook `native_client_use_item_on(pos, face, point, hand, &mut NativeClientUse)` (fragment
    `client_use_item_on.json`), called in `remote_use_item_on` once the client has picked a block, for each hand it
    tries. An answer other than `Goes` sends `ServerboundUseItemOnPacket` (`send_use_item_on`, shared with the
    game's own path) and stands for the block and the item: `Success` swings and ends the click, `Consume` and
    `Fail` end it, `Pass` goes on to the item's use and the next hand. The server's click hooks now read where the
    client aimed (`native_click_hit`, fragment `click_hit.json`: the `BlockHitResult` the packet carried, kept
    while `native_right_click_block` and `native_use_item_on` run) before the look traced on the server: a value
    slot or a filter is a small part of a face, and Create tests the point.
  - Compat: `events::Answer` (`Pass`, `Fail`, `Consume`, `Success`, from the result a listener canceled with);
    `client_right_click_block` answers with it, through `glue::client_use_item_on`. The `interaction_key` hook
    now posts `InputEvent.InteractionKeyMappingTriggered` for attack, use (per hand) and pick block
    (`input::interaction_key`: `getButton`, `getKeyMapping`, `getHand`, `isAttack`, `isUseItem`, `isPickBlock`,
    `shouldSwingHand`/`setSwingHand`, cancelling): a cancel takes the action from the game and says whether
    the hand swings. 17 jars of the CurseForge pack listen to it.
  - Compat core: a `Direction`, `Direction.Axis`, `AxisDirection` or `Direction.Plane` in a string concatenation
    prints as Java's `toString` does (`up`, `x`, `Towards positive`, `HORIZONTAL`), where it printed an object
    hash (`net.minecraft.core.Direction@417f`).
  - Tests: `events::tests::a_clients_click_answers_with_the_result_it_was_canceled_with` (a listener cancelling
    with `SUCCESS` for a stick: Success; one denying both uses for a barrier: Pass; dirt: nothing),
    `input::tests` (a listener cancelling the off hand's use without a swing: Some(false); attack and the main
    hand go on, each with its key mapping and hand), the position test's `up along x`. 400 compat, 18 translator
    and 69 game tests pass.
  - Round 24 in game: the filter slot click is heard once on the client (`MAIN_HAND client, taken`) and once on
    the server (`MAIN_HAND server, taken`), no off-hand click; the deployer's filter holds the three iron ingots
    (`{count: 3, id: "minecraft:iron_ingot"}`, as Create keeps the stack's copy) and the player keeps them. A
    right click on a cow no longer posts `RightClickBlock` for the dirt behind it (round 23 did, for both hands).
    No other change from round 23 but the world's (spawn, nearby blocks).
- [x] 2026-11-06 Part 31: the coverage probe fills a call's arguments by their declared types.
  - `--coverage` asks the runtime each API method a jar calls with null arguments, on the receivers it can
    make (null, a bare object, the probe's player, cow and level, the class's statics, what the jar's own calls
    return). A method whose mapping reads an argument (a state's property, a position, a stack's item) failed on
    the null and was counted missing although a game call would have gone through.
  - Hosted (`--coverage --host`), each call is now also tried with every argument a value of its declared type
    as a game gives one (`sample`: stone's state, block, item, a stack of it and its id, the level's origin,
    up, the main hand, an empty tag, a `Vec3`, the level's registries, the probe's player, cow and level; a zero
    for a primitive), on the same receivers and on a sample of the owner's own type; a static method from its
    first argument. A sample whose own making misses doesn't count.
  - packcov7 (Part 29's tree, the old probe, the same 367 jars): methods 95569/133181 (71.8%), uses
    312625/379190 (82.4%), 12789 distinct missing; Create 2700/3994 methods (67.6%), 22861/25380 uses (90.1%);
    Create Aeronautics, now read (Part 28), 1200/1578 (76.0%) and 5821/6524 (89.2%). packcov7t (the typed
    probe): 95709/133182 (71.9%), 312855/379191 (82.5%), 12735 missing; Create 2709/3994 (67.8%), 22875/25380
    (90.1%). Still counted missing though mapped, for want of a receiver the probe can make: a codec's
    `forGetter`, `group` and `Products.apply`, a menu's `Slot` methods, `BlockState.cycle` (needs a property of
    the state's block).
- [x] 2026-11-06 Part 32: entity models, natively (`horizon-compat/src/models.rs`).
  - packcov7t counted 5032 missing uses in 82 of the pack's jars on the classes a mod builds and draws an entity
    model with: the layer builders (`CubeListBuilder`, `CubeDeformation`, `PartDefinition`, `MeshDefinition`,
    `LayerDefinition`: 3017 uses, 47 jars), `PartPose` (658), `ModelPart` (1034, 74 jars), `EntityModelSet`
    (118), the model base classes (`EntityModel`, `HierarchicalModel`, `Model`: 165) and the layer registration
    event (40 jars). A renderer's `bakeLayer` gave a bare `ModelPart` whose `getChild` and `render` missed.
  - The builders keep what vanilla's keep: a builder's texture offset and mirror, each cube's corner, size,
    deformation, texture scale and visible faces (every `addBox` overload), a part definition's cubes, pose and
    children (`addOrReplaceChild` keeps the replaced child's children), the mesh's root, the layer's texture size.
    `PartDefinition.bake` and `LayerDefinition.bakeRoot` make `ModelPart`s as vanilla does: the cubes on the
    texture's size times their scale, the texture offset truncated, each part at its pose (its initial pose too).
  - `RegisterLayerDefinitions.registerLayerDefinition` keeps the supplier; the client setup posts the event before
    `RegisterRenderers`. `EntityModelSet.bakeLayer` and both renderer contexts' `bakeLayer` build the registered
    layer afresh for each renderer; a layer nobody registered (vanilla's own, which the runtime has no definition
    of) bakes as an empty part whose children are empty too, so a renderer built on it constructs and draws the
    rest.
  - `ModelPart`: vanilla's public fields (`x`... `zRot`, the scales, `visible`, `skipDraw`), the poses
    (`storePose`, `loadPose`, `resetPose`, the initial pose), `copyFrom`, `getChild`, `hasChild`, `setPos`,
    `setRotation`, `offsetPos`/`Rotation`/`Scale`, `getAllParts`, `getRandomCube`, `isEmpty`,
    `translateAndRotate` (JOML's `rotationZYX`), `visit` and `render`: each visible part moves the pose stack to
    its pose and draws its cubes as one `Host::draw_model` of faces, laid out as `ModelPart.Cube` and
    `Polygon` lay them (the six faces' rectangles on the texture, the mirrored cube's sides swapped and flipped),
    with the sprite of the consumer's render type's texture (`ns:textures/entity/x.png` is `ns:entity/x`).
    `RenderType` factories that take a texture keep it (`textured_render_type`).
  - `Model`, `EntityModel`, `HierarchicalModel`: their constructors and fields, `renderType` (the model's
    function, or `entityCutoutNoCull`), the final four-argument `renderToBuffer`, a hierarchical model's drawing
    of the root its mod's `root()` gives, `copyPropertiesTo`, `getAnyDescendantWithName`.
  - Not yet: the renderers that draw a mob with its model (`LivingEntityRenderer`, `MobRenderer`), vanilla's own
    layers and models (`HumanoidModel`), `Material`/`Sheets`, keyframe animations.
  - Tests (`models::tests`, 406 compat and 18 translator tests pass): a baked head's six faces on the player skin's head rectangles, on the mod's sprite,
    under its part's pose; a mirrored cube's sides swapped and flipped; a part's rotation about its pivot (a
    quarter turn about y takes x to -z); `addBox`'s named, deformed, mirrored and face-limited forms (the hat
    layer's front on the skin's 40..48 by 8..16); a registered layer baked afresh for each renderer and an
    unknown one empty; a mod's `HierarchicalModel` drawing its root in white or the renderer's colour.
  - packcov8 (this part, the same 367 jars, the typed probe): methods 96505/133182 (72.5%, from 71.9% in
    packcov7t), uses 317131/379191 (83.6%, from 82.5%), 12669 distinct missing (from 12735); Create 2728/3994
    methods (68.3%) and 22902/25380 uses (90.2%). The model families' missing uses fell from 5032 to 363; what is
    left is vanilla's own models (`HumanoidModel.createMesh` 123 uses, `HumanoidModel(root)` 86, `ZombieModel`
    18...), the renderers that draw a mob with its model, and calls the probe has no receiver for.
- [x] 2026-11-06 Part 33: vanilla's own models and the renderers that draw a mob (`models.rs`, new
  `entity_render.rs`).
  - packcov8 left, on the model families, vanilla's own models (`HumanoidModel.createMesh` 123 uses,
    `HumanoidModel(root)` 86, `ZombieModel` 18...), and on the renderers a mod's mob is drawn by,
    `MobRenderer`/`LivingEntityRenderer` (255 uses) and `Material`/`Sheets` (212).
  - Models: `HumanoidModel` (its seven parts the root's children; `createMesh` the player's 64 by 64 layout,
    the hat half a pixel bigger; a young one's head scaled, 16 down), `AgeableListModel` and `ListModel`
    drawing their `headParts`/`bodyParts`/`parts` (a young one's scaled and moved by its settings),
    `HumanoidModel.setupAnim` (the head's look, the walk's swing, a rider's legs, each arm's pose, the attack's
    swing, crouching, the idle bob, a swimmer's strokes; the hat follows the head), `prepareMobModel` (the
    swim amount), `AbstractZombieModel`/`ZombieModel` (arms held out, higher while aggressive) and
    `AnimationUtils` (`bobModelPart`, `animateCrossbowHold`/`Charge`, `animateZombieArms`, `swingWeaponDown`).
    `setupAnim` and `prepareMobModel` are declared at three erasures (`Entity`, `LivingEntity`, `Monster`):
    a call at any of them reaches the mod's override at whichever one it declares, as the JVM's bridge
    methods do, without recursing on a mod's `super` call.
  - Renderers: `EntityRenderer`, `LivingEntityRenderer`, `MobRenderer`, `HumanoidMobRenderer`,
    `AbstractZombieRenderer`, `NoopRenderer`: their constructors (shadow, model, layer list; a humanoid
    renderer adds vanilla's head, elytra and item-in-hand layers, a zombie's armour), `addLayer`, `getModel`,
    and `LivingEntityRenderer.render` in vanilla's order: `RenderLivingEvent.Pre` (a listener may cancel
    it), the body's and head's yaw lerped, `scale`, `getBob`, `setupRotations` (the body's yaw, a dying one
    tipping over, shaking while frozen), the flip and the `-1.501` lift, the walk animation,
    `prepareMobModel`, `setupAnim`, `getRenderType` (none for an invisible body, translucent for one the
    player sees through), `getOverlayCoords` (the hurt red, the white flash), the model's `renderToBuffer`,
    each layer, then `.Post`. The entity's yaw, pitch, body and head yaw, hurt and death time, attack and walk
    animation come from vanilla's public fields, which mods read by name. A mod renderer that doesn't
    override `render` reaches this one.
  - `RenderLayer`: its parent model, `renderColoredCutoutModel`, `coloredCutoutModelCopyLayerRender`.
    Vanilla's own layers (head, elytra, held item, armour) construct and stay in the list but draw nothing yet.
  - Textures: `Material` (its atlas and texture, `sprite`, `renderType`, `buffer`; `equals`/`hashCode` as
    vanilla's record), `TextureAtlasSprite.wrap`, `Sheets` (its atlases; the chest, banner, shield, shulker,
    sign and hanging sign materials and render types; `chooseMaterial`). A consumer wrapped in a sprite draws
    a model's faces inside that sprite, as vanilla's `SpriteCoordinateExpander` maps them.
  - `EntityRenderDispatcher.getRenderer` builds a mod entity's renderer on first use through its registered
    provider (`glue::entity_renderer`); the dispatcher's `render` and the game's entity pass share it.
  - The API climbs vanilla's model and renderer classes past the one a mod extends (`api::api_super`: a miss
    on `ZombieModel` tries `AbstractZombieModel`, then `HumanoidModel`), and `instanceof` follows it.
  - Tests (414 compat and 18 translator tests pass): `models::tests` (a humanoid mesh on the player skin's
    layout; a young humanoid's big head on a small body; a humanoid's walk, look and swing; a zombie's raised
    arms) and `entity_render::tests` (a material's sprite on its sheet; a wrapped consumer's model drawn from
    its sprite; a mob renderer posing its model and drawing it with its texture: the head turned 30 degrees,
    the arm's idle bob, seven parts on the mod's sprite, the `1.501` lift; a canceled `Pre` drawing nothing).
- [x] 2026-11-06 Part 34: the camera, natively (new `horizon-compat/src/camera.rs`).
  - packcov8 counted 1243 missing uses in 112 keys around the camera: `Camera.getPosition` (246 uses, 66 jars),
    `GameRenderer.getMainCamera` (129, 42), the game renderer's shader getters (`getPositionTexShader` 91, 48
    jars...), `Minecraft.getCameraEntity` (68), `EntityRenderDispatcher.cameraOrientation` (38), `Camera.getEntity`
    and `rotation`, and the viewport events' getters.
  - The main camera follows the game: on the client's player while the client is in a world, at the place the game
    drew its last frame from (the `level_submit` hook's camera, kept until the client leaves the world), the
    player's eyes (1.62 up, 1.27 crouching) until then; turned as `Camera.setRotation` turns it:
    `rotationYXZ(PI - yaw, -pitch, -roll)` with JOML's arithmetic (`Math.sin`, `cosFromSin`), the forward, up and
    left vectors rotated by it (`Quaternionf.transform`), each in its own object as vanilla's final fields are.
    `getPosition`, `getBlockPosition`, `getXRot`/`getYRot`/`getRoll`, `rotation`, `getLookVector`/`getUpVector`/
    `getLeftVector`, `getEntity`, `isInitialized`, `isDetached`, `getPartialTickTime`, `getNearPlane` (and
    `NearPlane`'s corners and `getPointOnPlane`), `getFluidInCamera` (water below the surface in the camera's
    block, else lava or powder snow at the near plane's centre or corners), `getBlockAtCamera`. A camera a mod
    makes (`new Camera()`, `setup`, a subclass's `setRotation`/`setPosition`/`move`) keeps its own state.
  - Before the mods' renderers draw, the render dispatchers hold the main camera (`EntityRenderDispatcher.camera`,
    `BlockEntityRenderDispatcher.camera`, as their `prepare` sets them each frame); `cameraOrientation` is the
    camera's rotation until a mod overrides it (`overrideCameraOrientation`); `distanceToSqr` from the camera.
    `Minecraft.gameRenderer`, `cameraEntity` and `getCameraEntity` are the client's; `setCameraEntity` and
    `Options.setCameraType` leave the game's view as it is (traced). `CameraType.isMirrored` and `cycle`.
  - `GameRenderer`: `getMainCamera`, `lightTexture`, `overlayTexture`, its static shader getters (one
    `ShaderInstance` a name: `getPositionTexShader` is `position_tex`; uniforms, `apply`, `clear` are the host's),
    `getProjectionMatrix` (JOML's `perspective` of the field of view on the window's shape, 0.05 to
    `getDepthFar`), `getRenderDistance`/`getDepthFar` and `getFov` at vanilla's default options (70 degrees, 12
    chunks: the game doesn't hand its own over yet). `RegisterShadersEvent.registerShader` hands the mod its
    shader at once. `LightTexture.getBrightness` is vanilla's curve; the lightmap's switches are the host's.
  - The getters and setters of NeoForge's viewport events (`ComputeCameraAngles`, `ComputeFov`,
    `ComputeFogColor`, `RenderFog`), `RenderLevelStageEvent` and `RenderHighlightEvent`, on the fields an event
    carries (the main camera for an event that names none). The game posts none of them yet: the pack's jars that
    listen are 43 for `RenderLevelStageEvent`, 17 `RenderHighlightEvent.Block`, 10 `ComputeCameraAngles` (Create
    among them), 6 `RenderFog`, 6 `ComputeFogColor`, 4 `ComputeFov`.
  - The dispatcher's module bits are 128 wide now (63 modules), and a key that isn't `owner.name.desc` is offered
    to the chain's modules only (it was offered past the chain's end).
  - Tests (`camera::tests`, 420 compat tests pass): the camera at the player's eyes looking west (forward -x, left
    +z) and down (forward -y, up +z); the frame's camera placing it, the dispatchers holding it, the orientation
    overridden; no camera out of a world; water, lava and powder snow fog by where the eyes are (a water source's
    surface 8/9 up its block); the projection as OpenGL's perspective; the shaders one object a name.
  - packcov9 (this part, the same 367 jars, the typed probe): methods 98120/133182 (73.7%, from 72.5% in
    packcov8), uses 320014/379191 (84.4%, from 83.6%), 12388 distinct missing (from 12669); Create 2797/3994
    methods (70.0%) and 23051/25380 uses (90.8%). The camera's family (`Camera`, `GameRenderer`, shaders and
    uniforms, `LightTexture`, the viewport, level stage, highlight and shader events) fell from 1563 missing uses
    to 27; all missing uses from 67364 to 64481. Not counted by packcov: the vertices a mod writes one by one
    (`addVertex` in 112 of the pack's jars, `putBulkData` in 26), which every consumer answered by chaining and
    dropping them: no key was missing, nothing was drawn (Part 35).
- [x] 2026-11-06 Part 35: the vertices a mod writes one by one, drawn (new `horizon-compat/src/vertices.rs`).
  - A world consumer (`MultiBufferSource.getBuffer(type)` in a block entity's or an entity's renderer, maybe
    wrapped in a sprite) keeps the vertices written into it: `addVertex` in each of its forms (a position, under
    a `Matrix4f` or a `PoseStack.Pose`, a `Vector3f`, the whole vertex at once), then `setColor` (ints, floats, one
    ARGB int, `setWhiteAlpha`), `setUv`, `setLight`/`setUv2`; overlay and normals chain as before. When a face is
    whole (the next vertex begins, the renderer is done, or the mod ends the batch: `endBatch()`,
    `endBatch(type)`), the host draws it as a model face where the pose put it, each corner in its own colour,
    lit by the light the mod gave. A `QUADS` type makes a face of every four vertices; `TRIANGLES` of three;
    `TRIANGLE_STRIP` and `TRIANGLE_FAN` a triangle a vertex after the second (a strip's every other one turned to
    wind as the first); lines aren't drawn (traced once). The mode is the one a mod's own type was made with
    (`RenderType.create(name, format, mode, ...)` keeps it), else vanilla's type's (`leash()` is a strip,
    `dragonRays()` triangles).
  - A face's sprite: the one its consumer was wrapped in (UVs 0 to 1 across it); else, on the block atlas
    (`translucent()` and the other block layers, a type whose texture is an atlas, a mod's own type built with
    `setTextureState(BLOCK_SHEET_MIPPED)` as Create's fluid type is), the sprite whose cell holds its UVs
    (`sprite.getU/getV` give a UV in the sprite's cell), its UVs measured across that cell; else its type's
    texture (`entityTranslucent(texture)`, a `TextureStateShard`); else the game's white sprite (`lightning()`,
    `debugQuads()`: the vertices' colours alone). UVs running past the texture (a scrolling beam's) are moved
    back by whole textures and kept inside it, since one sprite of an atlas can't repeat.
  - `putBulkData(pose, quad, ...)` (vanilla's two forms and NeoForge's): the baked face under the pose, tinted by
    the colour given (times each corner's brightness), lit by the light given.
  - Vertices written outside the world's hooks (a screen's, into its own buffer source) are forgotten before a
    block entity or the entities are drawn, so they never land in the world.
  - Tests (`vertices::tests`): a fluid surface written as Create's `FluidRenderer` writes it (four vertices into
    `translucent()`, UVs from `sprite.getU/getV` of the still water) is one upward face of the water's sprite,
    lifted by the pose, in the mod's colour and light; a wrapped consumer's face drawn from its sprite as soon as
    the next one begins; an untextured type's face white; a line type's vertices drawing nothing; an unfinished
    face dropped; a mod's own `TRIANGLE_STRIP` type on the block sheet drawing a triangle a vertex, both facing
    the same way.
- [x] 2026-11-06 Part 36: the level's render stages (`RenderLevelStageEvent`), passed each frame.
  - 43 of the pack's jars listen (Create among them: its outliner, value boxes and schematic previews draw at
    `AFTER_PARTICLES`). Once the game has drawn the level's mod geometry (the `level_submit` hook, after the
    mods' entities), the mods hear every stage in vanilla's order, `AFTER_SKY` to `AFTER_LEVEL`, each event with
    a pose stack of its own at the camera (a mod moves it by minus the camera's place to draw at a place in the
    world, as on vanilla), `getModelViewMatrix` the view's rotation (the camera's rotation turned back, as
    `LevelRenderer`'s frustum matrix), the projection, the main camera, the frustum (everything in view: the game
    culls), the level renderer and the render tick (the game time). Nothing is made when no mod listens.
  - The stages are one object each (`RenderLevelStageEvent.Stage.AFTER_PARTICLES`...), named as NeoForge
    registers them (`toString`: `minecraft:after_particles`); `Stage.fromRenderType` gives the stage after a
    block layer (`translucent()` → `AFTER_TRANSLUCENT_BLOCKS`), null for any other type. The class left the
    data-class table (645 entries): it is modelled here.
  - What a listener writes into the client's buffers (`Minecraft.renderBuffers().bufferSource()`) or its own
    (`MultiBufferSource.immediate`, `immediateWithBuffers`, `new ByteBufferBuilder(size)`) is drawn when it ends
    the batch, else after its stage (`vertices.rs`, Part 35).
  - Not yet: a face whose texture isn't on the game's atlas is left out by the game (Create's outlines draw with
    `create:textures/special/blank.png` and the other special textures, which the atlas doesn't pack).
  - Tests (`camera::tests`, `vertices::tests`): a listener hears the eleven stages in order and compares them
    (`fromRenderType(translucent())`, the static `AFTER_TRANSLUCENT_BLOCKS`: one object); the white square it
    draws on top of a block, its pose stack moved by minus the camera's place, is drawn there relative to the
    camera, facing up; the view matrix turns the camera's forward (west) to -z; a mod's own buffer source draws
    what was written when its batch ends.
- [x] 2026-11-06 Part 37: the sprites the game's atlas lacks, and `Minecraft.getTextureAtlas`.
  - The host says whether its atlas has a sprite (`Host::has_sprite`: the game's draw host looks it up where
    `native_draw_faces` does, which leaves out a face whose sprite isn't there; true for a host that can't
    tell). A face of written vertices whose sprite the atlas lacks is drawn from the white sprite in its corners'
    colours: Create's outlines (`create:textures/special/blank.png` and the other special textures, which the
    atlas doesn't pack) and a fluid sprite only a fluid type names show as their colour instead of nothing.
  - `Minecraft.getTextureAtlas(atlas)` (40 of the pack's jars, 75 uses): a function from a location to the
    atlas's sprite (`atlas::getSprite`: one object a sprite, in a cell of its own), as fluid renderers find a
    fluid's still and flowing sprites (Create's `FluidRenderer`). One object's method as a function is a
    combinator of its own (`functions.rs`, `compat:fn/bound`).
  - Tests (`vertices::tests`): the fluid surface test takes the water's sprite as Create does,
    `getTextureAtlas(BLOCK_ATLAS).apply(still)` (the same object as the atlas's); a mod's own type on
    `create:textures/special/blank.png` draws white in its vertices' colours on a host whose atlas lacks it,
    while a texture the atlas has is drawn from.
  - In game (Create alone, creative): Parts 33 to 37 build and run with no panic; the generated
    `has_sprite` compiles. Create's fluid tanks of water and lava (`TankContent:{Fluid:{id,amount}}`, 1.21.1's
    `FluidTank` format) hold their fluid on the server and on the client (the update tags carry it), and
    their renderer runs, but drew nothing you could see: `IClientFluidTypeExtensions` was unmapped (its
    `getTintColor` gave 0, an alpha of 0). Part 38 maps it.
- [x] 2026-11-06 Part 38: NeoForge's client extensions (`client_extensions.rs`, a module of the chain).
  - What a mod tells the client about how its blocks, items, mob effects and fluid types look
    (`IClientBlockExtensions`, `IClientItemExtensions`, `IClientMobEffectExtensions`,
    `IClientFluidTypeExtensions`): gathered at the client's setup as NeoForge's `ClientExtensionsManager`
    does, first from each registered object whose class overrides `initializeClient(consumer)` (the
    deprecated way: Create's tinted fluid types, its wrench, potato cannon, sandpaper, symmetry wand and other
    tools), then from `RegisterClientExtensionsEvent` (`registerFluidType`/`registerItem`/`registerBlock`/
    `registerMobEffect`, of objects or holders, and `is…Registered`).
  - `of(fluid | state | type)`, `of(stack | item)`, `of(state | block)`, `of(instance | effect)` hand out what
    was registered, else NeoForge's own extensions for vanilla's water (`block/water_still`, `water_flow`,
    `water_overlay`, the underwater overlay, tint `0xFF3F76E4`, its biome's water colour in the level) and
    lava (`block/lava_still`, `lava_flow`), else the interface's `DEFAULT`. The interfaces' default methods
    answer as NeoForge's do, and the ones that defer to another (`getTintColor(stack)` is `getTintColor()`)
    ask the receiver, so a mod's override of the plain accessor counts.
  - Measured on the pack (packcov9): 358 missing uses in this family, `IClientFluidTypeExtensions.of(Fluid)` in
    32 jars, `RegisterClientExtensionsEvent.registerItem` in 26.
  - Tests (`client_extensions::tests`): vanilla water and lava answer NeoForge's textures and tint through the
    stack accessors Create's fluid renderer asks (empty answers the defaults); the event registers by object
    or holder, each kind apart; an `initializeClient` override hands its extension at setup and the stack
    accessors reach that extension's own plain ones; a biome's water colour in either file format.
  - In game (tank round 3, Create's fluid tanks of water and lava): the `IClientFluidTypeExtensions` misses
    are gone and the water tank's window shows its water, blue. Two gaps remain. The water is flat, with no
    ripples. The lava tank's window is white. Both look like a face sampling one texel of a white region,
    tinted by the vertex colour, rather than the fluid's sprite. `uvShrinkRatio` is still unmapped, but an
    unmapped call answers 0, so it can't cause this. Next: follow the face's UVs from `getU`/`getV` through
    `render::locate` to `native_draw_faces`.
  - Still unmapped in that round: Flywheel's GL backend (LWJGL `MemoryStack`, `GL20` shaders),
    `AbstractTickableSoundInstance.<init>`, `DynamicTexture`, `RenderType.bufferSize`,
    `Options.graphicsMode`/`OptionInstance.get`, and `new String(bytes, charset)`. The last two are
    drafted (`scratchpad/drafts/options.rs`, `string_init.rs`) and not applied yet.
- [x] 2026-11-06 Part 39: inherited statics of the game's classes, the client's options (`options.rs`, a
  module of the chain) and `new String(...)`.
  - Why the tanks' fluid was white: catnip's `PonderRenderTypes extends RenderType` reads
    `BLOCK_SHEET_MIPPED` by its simple name. The translator names a static by the first of the game's
    classes on the way up (`RenderType.BLOCK_SHEET_MIPPED`), but `RenderStateShard` declares it, and
    nothing answered for `RenderType`. The fluid's render type got no texture, so its faces were drawn
    from the white sprite in their vertex colour: water a flat blue, lava white. `api::get_static` now
    climbs the game's superclasses (`api_super`, which gains `RenderType` → `RenderStateShard`) when no
    module answers, as the JVM resolves an inherited static.
  - `Options`' getters (`fov()`, `graphicsMode()`, `renderDistance()`, `mainHand()`: 71 of them) each
    hand out one `OptionInstance` per option. It holds vanilla 1.21.1's initial value until a mod sets
    it. An enum option holds the very constant its class holds, as mods compare them by identity.
    Also mapped: `get`/`set`, and the getters computed from them (`getEffectiveRenderDistance`,
    `getCloudsType`, `getBackgroundOpacity`/`Color`, `getMenuBackgroundBlurriness`,
    `getSoundSourceVolume`). A mod's own option (its constructor, `createBoolean`) holds its initial
    value and tells its listener when it is set. The values are vanilla's defaults, because the game
    doesn't hand its settings over yet.
  - `new String(bytes, charset)`, and from chars, code points, a range of either, or another string or
    builder: the object `new` made becomes that text. This was unmapped, so the object was not a string
    and every call on it missed (`String.isEmpty`, in game).
  - Measured on the pack (packcov9): `Options` and `OptionInstance` had 400 missing uses
    (`OptionInstance.get` in 50 jars).
  - Tests: `options::tests` (vanilla's defaults, a set option changing the derived getters, a mod's
    boolean option); `jlang::tests::new_string_spells_its_arguments`;
    `vertices::tests::a_type_reading_the_sheet_through_render_type_draws_its_sprites`.
  - In game (tank round 4): the lava tank's window shows lava, orange and textured, and the water tank's
    window shows rippled water. `Options.graphicsMode`, `OptionInstance.get`,
    `new String(bytes, charset)` and `String.isEmpty` no longer miss. One new miss shows the code
    reaching further: `IntList.getInt` on an `IntArrayList`.
- [x] 2026-11-06 Part 40: NeoForge's viewport events, and what a render type is made of.
  - The game asks four of its hooks each frame. The mods' listeners now answer them:
    - `compute_fov`: `ViewportEvent.ComputeFov` sets the world's field of view.
    - `compute_fog_color`: `ComputeFogColor` sets the fog's colour.
    - `render_fog`: `RenderFog` sets the fog's distances. Like `ClientHooks.onFogRender`, it applies
      only what a listener that cancels it set. The game's fog maps to 1.21.1's two fogs: the sky's
      (`sky_end`), and the terrain's, which is the environment's (a fluid's, the Nether's) where that is
      nearer than the render distance's.
    - `camera_shake`: `ComputeCameraAngles` turns the camera. The listeners' turn past the camera's
      own is added to the frame's shake, which the game bounds.

    With no listener no event is made, so a frame runs no mod code (`neo::hearing`). Pack jars that
    listen: ComputeFov 4, ComputeFogColor 6, RenderFog 6, ComputeCameraAngles 10.
  - `RenderType.bufferSize()`, `format()`, `mode()`, `affectsCrumbling()`, `sortOnUpload()`: for a
    mod's own type, what `create` was given; for vanilla's, the values from 1.21.1's `RenderType`
    (57 types, from the source: `translucent` sorts its faces and shows cracks, in a 786432-byte
    buffer of `BLOCK` vertices). Create's `SuperRenderTypeBuffer` asks `bufferSize` (in game).
  - Tests: `camera::tests::the_viewport_events_change_the_frame_as_their_listeners_say`,
    `render::tests::a_render_type_tells_what_it_is_made_of`.
- [x] 2026-11-06 Measured (packcov10, after Parts 38 to 40, same 367 jars): missing uses 64481 → 63458
  (−1023), and no class's count grew. Methods mapped went from 73.7% to 74.2% summed over jars, and uses
  from 84.4% to 84.7%. Create went from 70.0% to 71.0% of methods and from 90.8% to 91.1% of uses. The
  biggest drops:

  | Class | packcov9 | packcov10 |
  |---|---|---|
  | `Options` | 254 | 6 |
  | `IClientFluidTypeExtensions` | 205 | 0 |
  | `OptionInstance` | 145 | 2 |
  | `RenderType` | 216 | 119 |
  | `RegisterClientExtensionsEvent` | 83 | 0 |
  | `IClientItemExtensions` | 57 | 2 |
  | `ByteBufferBuilder` | 57 | 8 |

  Largest left: `Minecraft` (795: `getConnection` in 57 jars, `getTextureManager` in 46,
  `getMainRenderTarget`, `getProfiler`, `getBlockColors`), and the codec builders (`MapCodec.forGetter`
  in 107 jars). The codec builders work in game; the probe calls them on stand-in receivers.
- [x] 2026-11-06 Part 41: the rest of a render type, and small gaps from the round and the measurement.
  - A mod's own `RenderType` subclass (`super(name, format, mode, bufferSize, affectsCrumbling,
    sortOnUpload, setupState, clearState)`, 34 jars) keeps what it was made with, which its getters
    answer. `setupRenderState`, `clearRenderState` and `draw(MeshData)` set GL state the game doesn't
    use (it draws what mods write into its consumers), so they do nothing.
  - fastutil's typed list getters (`IntList.getInt(i)`, `getLong`, `getDouble`...), which missed in game.
  - `TextureAtlasSprite.uvShrinkRatio()`: vanilla's `4 / atlasSize`, taking a sprite as 16 pixels
    wide. Create's fluid renderer insets its UVs by it.
  - `Minecraft.getProfiler()` gives vanilla's inactive profiler (15 jars). `getVersionType()` gives
    `release` (7 jars). `getToasts()` gives an object whose `addToast` does nothing (22 jars). Before,
    a toast call on the null it got threw in the mod's code. The game shows none of the mods' toasts yet.
- [x] 2026-11-06 Part 42: the client's connection. `Minecraft.getConnection()` (130 uses in 57 jars)
  gives the client's connection to its server while its player is in a world, and null otherwise, as in
  vanilla. On it:
  - `send(packet)`, also on its `getConnection()` (`Connection.send`). A mod's payload, sent bare or
    wrapped in `new ServerboundCustomPayloadPacket(payload)`, goes to the server on the mods' channel,
    as `PacketDistributor.sendToServer` sends it. A game packet a mod builds (`net/minecraft/...`) is
    the game's to send, so it is dropped. 14 jars send this way.
  - `registryAccess()` (14 jars) gives the registries object `RegistryAccess` calls already answer.
    `getLevel()` gives the client's level.
  - `getPlayerInfo(uuid)` (13 jars) gives null and `getOnlinePlayers()` an empty list. The game
    doesn't hand its tab list to the runtime yet; vanilla gives null for a player who isn't on it.
  - Test: `network::tests::a_payload_sent_on_the_clients_connection_goes_to_the_server`.
- [x] 2026-11-06 Part 43: a server player's connection, and the light a renderer draws with.
  - `serverPlayer.connection` is the player's connection, one per player object, which knows its
    player (`connection.player`, `getPlayer()`). `send(packet)` on it (55 jars), or on its
    `getConnection()`, sends a mod's payload, bare or in a `ClientboundCustomPayloadPacket`, to that
    player on the mods' channel, as `PacketDistributor.sendToPlayer` does. A fake player has no
    client, so nothing is sent. `hasChannel(type | id | payload)` (12 jars) is true for every channel
    the server registered, since the client runs the same mods. `isAcceptingMessages()` is true.
    `LocalPlayer.connection` is the client's connection from Part 42.
  - A game packet a mod sends (`net/minecraft/...`, on either side) still isn't sent. It is now
    counted as unmapped under its class (`ClientboundSetTitleTextPacket.<sent>`), so a round shows
    which ones mods send.
  - `LevelRenderer.getLightColor(level[, state], pos)` (36 jars, 25 calls in Create) is vanilla's
    `sky << 20 | block << 4`. Each layer is what the level answers to `getBrightness`, so a mod's
    own level gives its own light: Ponder's scene level answers 15, full light, as in vanilla.
    Before, the stand-in 0 drew with the light of the block being rendered, whatever place the mod
    asked about.
    Vanilla also raises the block light to the state's own emission; the game's light already
    includes it where the block stands.
  - Tests: `network::tests::a_payload_sent_on_a_players_connection_goes_to_that_player`,
    `render::tests::a_renderer_reads_the_light_its_level_gives`.
- [x] 2026-11-06 Part 44: the HUD, GL state and a section's buffers, from round 4's unmapped list
  and the measurement.
  - `Minecraft.gui` is the HUD. `setOverlayMessage(text, animate)` (12 jars) shows the text over the
    hotbar, and `getChat().addMessage(text)` (10 jars) puts it in the chat, as
    `displayClientMessage` does. `getFont()` gives the font, and `getGuiTicks()` (7 jars) the
    level's ticks, one more each tick as vanilla's. A mod's own subclass of `Gui` (Create's
    stealth overlay) keeps none of vanilla's state. Its `renderTextureOverlay(graphics, texture,
    alpha)` covers the screen with the texture at that alpha, as vanilla's does.
  - `GlStateManager`'s calls that set GL state (`_activeTexture`, `_bindTexture`, `_depthMask`...,
    up to 9 jars each) do nothing, as `RenderSystem`'s already did: the game's renderer keeps its
    own state. The ones that read GL back (`_getActiveTexture`, `_genTexture`) stay unmapped.
  - `SectionBufferBuilderPack` (catnip's buffer source takes its fixed buffers from one): one
    `ByteBufferBuilder` per render type. Its memory is the host's, as for any `ByteBufferBuilder`.
  - Test: `gui::tests::the_hud_shows_what_a_mod_gives_it`.
- [x] 2026-11-06 Part 45: a mod's own sound instances. A mod's `SoundInstance` class
  (`AbstractTickableSoundInstance` in 20 jars) now plays as vanilla's `SoundEngine` plays it.
  Create uses them for its machines' hum (`ContinuousSound`), fans' wind, steam whistles and
  trains. Before, the game played none of them.
  - `new AbstractSoundInstance(event or location, source, random)` sets vanilla's fields under their
    own names, which the mod's code reads and writes (`this.looping = true`). The getters read
    them, and `stop()` stops the instance. `SoundInstance.createUnseededRandom()` gives a random
    source.
  - `SoundManager.play(instance)` (also `playDelayed`, `queueTickingSound`) starts it if it can
    play (`canPlaySound`), with what its getters give, the mod's overrides among them (Create's
    `getVolume` follows its machines). Each client tick, a ticking instance ticks. If it stopped
    itself it is stopped; otherwise the game gets its place, volume and pitch, as vanilla's
    `SoundEngine.tick` does. `SoundManager.stop(instance)` stops it and `isActive` tells whether
    it plays.
  - The game: a host API `start_sound` / `update_sound` / `stop_sound`
    (`native_start_sound`, `native_update_sound`, `native_stop_sound`, helpers in the fragment
    `native_mod_sounds.json`). An instance of the new kind `Ticking::Owned` is moved and set by its
    owner each tick, and the engine plays it as set. `SoundEngine::holds(id)` tells whether the
    engine still plays an instance or waits to.
  - Difference: vanilla's `getVolume()` multiplies by the sound definition's own volume
    (`sounds.json`). Here the game applies that factor itself, so an override that leaves it out
    (Create's) gets it too. This only matters for definitions whose volume isn't 1.
  - With `HORIZON_COMPAT_TRACE`, the first sound of each class and event is traced: event,
    volume, pitch, looping, and whether the game started it. Test rounds run with
    `HORIZON_NO_AUDIO`, so the game has no sound library and starts none. A round shows which
    sounds a mod plays, not that they are heard. The engine side is covered by its test.
  - Tests: `sounds::tests::a_mods_ticking_sound_plays_as_it_says_until_it_stops`,
    `sound_engine::tests::an_owned_sound_plays_as_its_owner_sets_it`.
- [x] 2026-11-06 In game: tank round 5 checks Parts 40 to 45. The scene is round 4's, and the round
  script (`tankgame5.sh`) now also lists the mods' sound traces.
  - No panics. The water and lava tanks and the creative motor draw as in round 4.
  - Gone from round 4's misses: `Gui.<init>`, `SectionBufferBuilderPack`,
    `GlStateManager._activeTexture`, `AbstractTickableSoundInstance.<init>`,
    `SoundInstance.createUnseededRandom`, `TextureAtlasSprite.uvShrinkRatio`,
    `RenderType.bufferSize`, `IntList.getInt`.
  - Create's kinetic hum reaches the game. Its `ContinuousSound` plays
    `minecraft:entity.minecart.inside`, looping, at pitch 0.85 and volume 0.009 (one motor's
    share of Create's soundscape). The round has no sound library (`HORIZON_NO_AUDIO`), so the
    game starts none.
  - Left: an OpenGL shader probe in Flywheel's code (`GL20.glCreateShader`, LWJGL's
    `MemoryStack`; Horizon draws with its own renderer), a `DynamicTexture` (Flywheel's
    `NoiseTextures.reload`), and one `List.forEach` on null.
- [x] 2026-11-06 Part 46: vanilla's custom data (`minecraft:custom_data`), a stack's free-form NBT.
  packcov10: 40 mods call `CustomData.copyTag` (147 uses). Of `CustomData` only its constructor
  and `getUnsafe` were modelled. Worse, a game stack didn't carry its custom data to the runtime:
  what a mod wrote stayed on the runtime's stack and was gone once the game had the stack.
  - `CustomData` as vanilla's: `EMPTY`, `of`, `copyTag`, `getUnsafe`, `isEmpty`, `size`,
    `contains`, `matchedBy`, `update(updater)`, and the statics `update(type, stack, updater)` and
    `set(type, stack, tag)` (an empty tag takes it off the stack). It equals and hashes by its tag,
    also through `Object.equals`, so two sticks with the same data are the same
    (`isSameItemSameComponents`). Also `CustomData.CODEC`, and `NbtUtils.compareNbt` (what
    `matchedBy` uses).
  - The game: a translated mod's stacks carry `minecraft:custom_data` beside their enchantments
    (`native_vanilla_components`, `native_set_vanilla_component`; the game keeps it as a preserved
    component). It crosses as NBT, so its tags keep their kinds (a byte stays a byte), and so
    does a stack a block entity saves and reads back (`ItemStack.parse`).
  - Difference: `CustomData.CODEC` reads a compound only (vanilla also reads one written as SNBT
    text). `itemMatcher`, `loadInto` and the codec forms `read` and `update(ops, encoder, value)`
    aren't modelled.
  - Test: `components::tests::a_stacks_custom_data_is_vanillas_and_crosses_to_the_game`.
- [x] 2026-11-06 Part 47: a face's sturdiness and a block's support shape. packcov10: 41 mods call
  `BlockState.isFaceSturdy` (130 uses), 8 `BlockState.getBlockSupportShape` (Create among them), 7
  `Block.canSupportCenter`, 4 `Block.canSupportRigidBlock`, 11 a `super.getCollisionShape`. None
  was modelled.
  - A vanilla block's `isFaceSturdy` is the game's own answer, vanilla's for that state: the game
    already exports it (`StateFacts.sturdy`, a bit per direction for each of FULL, CENTER and
    RIGID), and `BlockFacts.sturdy` now carries it to the runtime (`native_state_facts`).
  - A mod block's comes from its support shape, tested as 1.21.1's `SupportType` tests a face
    (`shapes::sturdy`, on the slice `VoxelShape.getFaceShape` takes): FULL the whole face
    (`Block.isFaceFull`), CENTER its middle (`Block.box(7, 0, 7, 9, 10, 9)`), RIGID its rim.
  - `getBlockSupportShape` as 1.21.1 finds it: the mod block's own; else that of the vanilla class
    it is or extends, for a class that declares one (leaves none, mud and soul sand a full block,
    a snow layer by its layers, a closed fence gate its bar, a chorus flower its box, a hanging sign
    its shape); else `BlockBehaviour`'s, the collision shape: the mod block's own
    `getCollisionShape`, else none without collision (`noCollission`), else its shape. Which
    1.21.1 classes declare their own `getBlockSupportShape` (9) and `getCollisionShape` (21) is
    measured from 1.21.1's class files (the same scan gives the 99 classes of
    `DEFAULT_SHAPE_BLOCKS`); a class's chain comes from the generated table of 1.21.1's classes.
  - `canSupportCenter(level, pos, direction)`, never under an `unstable_bottom_center` block, and
    `canSupportRigidBlock(level, pos)`, of the block the level has there. `super.getCollisionShape`
    to a class with `BlockBehaviour`'s, and `super.getBlockSupportShape` to any vanilla block class.
  - Difference: an open shulker box holds as a closed one (vanilla reads its block entity while it
    opens). In the game itself, a content mod block's sturdy faces are still its template's (the
    game's own placement checks), not its shapes'.
  - Tests: `api::tests::a_faces_sturdiness_is_the_games_or_the_mod_blocks_support_shape`,
    `a_support_shape_is_the_classes_own_else_the_collision_shape`.
- [x] 2026-11-06 Part 48: a content mod block's sturdy faces in the game itself. Part 47 left
  the game's own support checks (torches, rails, lanterns, redstone: 60 reads of the sturdy bits
  in horizon-world) reading a translated block's template's faces: a Create shaft lying sideways
  held a torch on top like the glass block it is templated on.
  - `horizon_world::level::sturdy_faces(shape)`: the bits `StateFacts.sturdy` holds (`support * 6 +
    direction`) for a support shape, from horizon-world's own `SupportType` tests of each face's
    slice (`block_support.rs`: FULL the whole face, CENTER `Block.column(2, 0, 10)`, RIGID the
    block less `Block.column(12, 0, 16)`; 26.3's class files test the same as 1.21.1's).
  - A content block with shapes gets, per state, the bits of its shape (the one the game also
    collides with); one without collision keeps none.
  - Oracle: for all 33,764 vanilla states with a cached collision shape, `sturdy_faces` of that
    shape is the sturdiness the game exports from vanilla, but for the blocks whose class declares
    its own support shape (26.3's nine, 1.21.1's: leaves, snow layers, mud, soul sand, fence
    gates, shulker boxes, chorus flowers, hanging signs).
  - Create's declarations get their shapes at startup: the bundle's `content/` files are only the
    first declarations (stone, states), and the `native_content` hook replaces them with what
    `block_declarations` reads from the registration, shapes per state among it.
  - In game (`scratchpad/sturdygame.sh`, game built 2026-11-06 12:37): torches stand on blocks
    set under them, then each block below changes state. On Create's shaft turned from upright to
    lying (axis y to z) the torch pops, as in Java; on the build before this part it stayed (the
    shaft's glass template held it). An upright shaft (from axis x) and a casing (andesite to
    brass) keep theirs; a vanilla bottom slab (from stone) drops its own, the control.
  - Tests: `level::tests::sturdy_faces_of_a_collision_shape_are_vanillas`,
    `mod_content::tests::a_shaped_blocks_faces_hold_up_what_its_shape_does`.
- [x] 2026-11-06 Part 49: what a mod's projectile hits. Measured over the pack's 367 jars
  (`scratchpad/projscan/projscan.py`, class files and their jar-in-jar ones): 29 mods have 204
  classes below vanilla's projectiles (`Projectile` 115, `AbstractArrow` 31, `ThrowableItemProjectile`
  18, `ThrowableProjectile` 17, `AbstractHurtingProjectile` 6...: Iron's Spellbooks 58, Cataclysm 37,
  Ars Nouveau 20, Create Big Cannons 19, Create's potato), declaring `tick` 120 times, `onHitEntity`
  99, `onHit` 64, `onHitBlock` 61, `canHitEntity` 31. None could be made: a mod class's
  `super(type, level)` names its vanilla class (`ThrowableItemProjectile.<init>`), which nothing
  answered, and nothing dispatched a hit.
  - `projectiles.rs` (new): a mod's projectile is a mod entity (`mod_entities`), whose calls naming
    any vanilla projectile class (`ThrowableItemProjectile.setPos`) are the entity's. Their
    constructors by arguments: `(type, level)`; `(type, x, y, z, level)`; `(type, shooter, level)`
    at the shooter's eyes less a tenth (`0.1F`), shot by it; a hurting projectile's `(type, x, y,
    z, movement, level)` and `(type, shooter, movement, level)`, moving along `movement` at its
    `accelerationPower` (0.1). `setOwner`, `getOwner` (on a server, by UUID once the entity it
    held is gone), `ownedBy`, `getEffectSource`.
  - `EntityHitResult` (at the entity's feet unless told), `HitResult.getType` (ENTITY for it),
    `getLocation`, `distanceTo`; `instanceof HitResult` for both kinds of hit.
  - `ProjectileUtil`: `getEntityHitResult(level, projectile, from, to, box, filter[, inflation])`,
    the entity whose box grown by 0.3 (a float) the segment enters nearest; `(shooter, from, to,
    box, filter, maxDistanceSq)`, by pick radius, an entity whose box holds the start hit there;
    `getHitResultOnMoveVector` and `getHitResultOnViewVector`: the level's `clip` (colliders, no
    fluids), then the nearest entity in the projectile's box stretched along the motion and grown
    by a block, which wins over the block.
  - `Projectile.onHit` hands an entity hit to the projectile's `onHitEntity` and a block hit to its
    `onHitBlock` (the mod's own); `hitTargetOrDeflectSelf` hits (`ProjectileDeflection.NONE`);
    `canHitEntity`: `canBeHitByProjectile` (alive and pickable; a player not a spectator), never
    the owner before the projectile has left it. A mod entity's `getViewVector(partialTick)` and
    `getLookAngle`.
  - Not modelled: a target that deflects (a breeze), `PROJECTILE_LAND`, a vanilla block's answer to
    a hit (a bell rings), riding (each entity is its own vehicle). A projectile doesn't fly yet:
    its vanilla `tick` (part 50).
  - Test: `projectiles::tests::a_mod_projectile_hits_the_nearest_entity_on_its_path_else_the_block`.
- [x] 2026-11-06 Part 50: a mod's projectile flies. Measured over the pack's 204 projectile classes
  (`scratchpad/projscan/flightscan.py`): 29 have no tick of their own in their class chain (their
  vanilla class's runs), 154 have one that calls a vanilla `tick()` (`super.tick()`), 21 one that
  doesn't; so 183 run a vanilla projectile tick: `Projectile`'s 101, a thrown one's
  (`ThrowableProjectile`, `ThrowableItemProjectile`) 34, a hurting one's 6, an arrow's 31. Before
  this part each was nothing (`Entity.tick` answered void), and the level ticked only a class with
  its own `tick`: a mod's snowball never moved.
  - The level's tick runs a mod entity's vanilla class's `tick` when its class has none
    (`mod_entities::on_base`: a call named by the entity's first vanilla class, so a default that
    differs by class is that class's).
  - `Projectile.tick`: whether it has left its owner (`checkLeftOwner`: none of the pickable
    entities its box stretched along its motion and grown by a block meets is its owner), then
    the base tick. `ThrowableProjectile.tick`: what its path hits (`hitTargetOrDeflectSelf` with its
    own `canHitEntity`, unless a listener cancels `ProjectileImpactEvent`), on to the end of the
    path, `updateRotation`, slowed by 0.99 (water 0.8, bubbles), `applyGravity` (0.03).
    `AbstractHurtingProjectile.tick`: gone on a server where its chunk isn't loaded; alight
    (`shouldBurn`), the hit along its `getClipType`, `rotateTowardsMovement(0.2)`, faster by its
    `accelerationPower` along its heading times `getInertia` (0.95; `getLiquidInertia` 0.8), its
    `getTrailParticle` (smoke). Each of those is the mod's own where its class has one.
  - `shoot` (`getMovementToShoot`: the unit direction put off by triangle draws of the entity's
    `random`, spread 0.0172275 × inaccuracy, times the speed; facing along it), `shootFromRotation`
    (`Mth`'s table sines, plus the shooter's `getKnownMovement`), `updateRotation`, `lerpRotation`,
    `ProjectileUtil.rotateTowardsMovement`, a thrown item's `setItem`/`getItem` (one of the stack;
    else one of its `getDefaultItem`).
  - Every mod entity: `Entity.tick` is the base tick (`baseTick`, virtual): whether its box is in
    water (which puts its fire out) or lava (alight 15 s after its first tick), its fire burning
    down on a server, gone 64 below the world's bottom (`onBelowWorld`). Its `random`
    (`getRandom`), gravity (`setNoGravity`, `getGravity`, `getDefaultGravity`, `applyGravity`), fire
    (`igniteForSeconds`, `igniteForTicks`, `isOnFire`, `get`/`setRemainingFireTicks`,
    `clearFire`), `isInWater`, `isInLava`.
  - `ProjectileImpactEvent` (and `EventHooks.onProjectileImpact`): posted where a mod listens (6 of
    the pack's mods name it), cancelable.
  - Not modelled: an arrow's own tick (`AbstractArrow`: it stays), the blocks a projectile passes
    through (`checkInsideBlocks`), a fluid's current, fire and lava hurting a mod entity, game events
    (`PROJECTILE_SHOOT`), a vanilla subclass's own defaults (a dragon fireball's trail).
  - Test: `projectiles::tests::a_mod_projectile_flies_as_vanillas_ticks_move_it`.
  - In game: Create's mechanical bearing still assembles its contraption and turns it (angle 67.2
    at 10 ticks; its entity now runs `Entity.tick`'s base tick), no uncaught throws or panics. The
    potato cannon doesn't fire yet: its `getAmmo` stops at `Player.getProjectile` (unmapped), and
    the ammo's type after it is a datapack registry entry (`create:potato_projectile/type`), which
    nothing loads yet.
- [x] 2026-11-06 Part 51: a mod's weapon finds its ammo, and a mod's datapack registry its entries.
  Measured in game (Part 50's potato round): Create's potato cannon stopped at
  `Player.getProjectile` (unmapped); the type of the ammo it finds comes next, from a datapack
  registry (`create:potato_projectile/type`), which nothing read. Over the pack: 9 mods call
  `getProjectile` or `ProjectileWeaponItem`'s ammo methods, 15 make datapack registries
  (`DataPackRegistryEvent.NewRegistry`), and 5 jars carry potato projectile types (Create's 25,
  Farmer's Delight's 9, Garnished's 9, Bitterballen's 8, Create Crafts & Additions' 4).
  - `getProjectile(weapon)`, as NeoForge 1.21.1's: a player's ammo is what its
    `ProjectileWeaponItem` takes from a hand (`getSupportedHeldProjectiles`, off hand first), else
    the first stack of its inventory it takes (`getAllSupportedProjectiles`, vanilla's slot order;
    the slot's own stack, so a shot taken from it is gone from the slot), else in creative the
    weapon's default (`getDefaultCreativeAmmo`, an arrow); a monster's what it holds, else an arrow;
    nothing for an item that is no weapon. `ProjectileWeaponItem`'s defaults for a mod's weapon,
    `getHeldProjectile`, and `LivingGetProjectileEvent` (`CommonHooks.getProjectile`), posted where
    a mod listens: a listener's ammo is the one shot.
  - Datapack registries (`datapack.rs`): `dataPackRegistry(key, codec, ...)` keeps the registry's
    codec (the event is posted after `NewRegistryEvent`, as NeoForge does). The first time the
    registry is reached with the game's resources at hand, every
    `data/<namespace>/<registry namespace>/<registry path>/**.json` is read with the codec
    (`Registries.elementsDirPath`), a file whose `neoforge:conditions` don't hold left out (mod
    loaded, true, false, not, and, or; an unknown kind fails the file, as NeoForge can't read it),
    and its entry `<namespace>:<path>` registered in `ResourceLocation`'s order (path, then
    namespace), the order `listElements` gives them in. A file the codec refuses is left out.
  - Not modelled: the datapack registries' tags, their network codecs (a client reads the same
    files), the `RegistryBuilder` a mod configures for one; vanilla's bow and crossbow as weapons.
  - Tests: `projectiles::tests::a_players_weapon_finds_its_ammo_in_a_hand_then_the_inventory`,
    `datapack::tests::a_mods_datapack_registry_reads_its_entries_from_the_data_files`.
  - In game (the potato round, Create's data carried into its content mod as an install carries
    it: 4,081 files): the cannon's `getProjectile` is no longer unmapped, and finds the potatoes.
    The shot stops one step later: the cannon's ammo test throws `IllegalStateException: Could not
    get RegistryAccess` (Create's `GlobalRegistryAccess` asks the client's connection for the
    registries, as `Dist` is the client's, and the server's thread had none), and the search went
    on asking the other slots after the throw: Part 52.
- [x] 2026-11-06 Part 52: the client's connection on the server's thread; an exception leaves a combined function.
  Measured in game (Part 51's potato round): the potato cannon's ammo test threw `Could not get
  RegistryAccess`. Create's `GlobalRegistryAccess` asks `Minecraft.getInstance().getConnection()`
  for the registries (its client way, which `Dist.CLIENT` picks, as in NeoForge's single player),
  and on the server's thread that was null here; the ammo search then went on asking the other
  slots after the throw. Over the pack: 67 jars call `Minecraft.getConnection`, 25 ask the client's
  connection for its registries (`ClientPacketListener.registryAccess`).
  - `Minecraft.getConnection()` on the server's thread: the client's connection once a player is
    in the server, as NeoForge's integrated server sees it (it runs in the client's JVM): the one
    acting in its own hook (an item's use, where the server's list leaves it out) or another; null
    before, while the server starts. Its registries are the level's.
  - An exception leaves `Predicate.or` and `and` (Guava's too), `Function.andThen` and `compose`,
    and `Consumer.andThen` as it leaves Java's default methods: the parts after the one that threw
    aren't run. The ammo search (`getProjectile`) ends where a weapon's test throws, the exception
    the caller's, no listener told.
  - An entity's main arm (`getMainArm`, the barrel's side for Create's gadgets), a mod class's own
    aside: the right one, vanilla's default (the game tells neither a player's option nor a mob's
    handedness).
  - Traces for a round (`HORIZON_COMPAT_TRACE`): a mod item's `use` and its result, and each hurt a
    mod deals a vanilla entity (by how much, taken or refused).
  - Not modelled: a payload sent on the client's connection from the server's thread (vanilla's
    goes to the server as the local player's; here it isn't sent).
  - Tests: `network::tests::the_server_thread_sees_the_clients_connection_once_a_player_is_in`,
    `functions::tests::a_part_that_throws_ends_the_combined_call`,
    `projectiles::tests::an_exception_in_a_weapons_ammo_test_ends_the_search`.
  - In game (the potato round; its cow now tagged and mob spawning off by 26.3's `spawn_mobs`: the
    earlier rounds' `@e[type=cow,limit=1]` read another, spawned cow, whose health stood at 10):
    Create's potato cannon fires each of its 3 shots (`use`: SUCCESS), the client draws the flying
    potato, and two hit the cow for 5 each (the potato type's `damage`), which dies of them, its
    loot on the ground. No uncaught throws. Left for the shot: the potato's recovery drop
    (`spawnAtLocation`), the cow's knockback (`LivingEntity.knockback`), `setLastHurtMob`,
    `EnchantmentHelper.doPostAttackEffects`, the projectile's saved data; 6 of Create's 25
    projectile types are left out (their effects' `mob_effect` holders and food properties are not
    read by the codecs yet).
- [x] 2026-11-06 Part 53: a registry's codecs read vanilla's entries; holders; a food and an effect written out in full.
  Measured in game (Part 52's potato round): 6 of Create's 25 potato projectile types were left out,
  4 for `Unknown registry key in mob_effect: minecraft:poison` (and `slowness`, `glowing`: vanilla's
  effects, by `BuiltInRegistries.MOB_EFFECT.holderByNameCodec()`), 2 for `FoodProperties.DIRECT_CODEC`
  (not read). The codecs knew vanilla's items, blocks and fluids by name; another vanilla entry of a
  registry mods fill was unknown. Over the pack (367 jars): 14 call `holderByNameCodec` on a built-in
  registry (items 7, effects, attributes, blocks and sounds 3 each, fluids 2), 11 read effects,
  attributes, sounds, entity types or potions by `byNameCodec` (entity types 9), 5 use
  `RegistryFileCodec`.
  - A registry's codec by name reads vanilla's entry of the registries mods fill as the runtime
    models it (`neo::registry_get`): effects (vanilla's 39), attributes (1.21.1's 31), sound events
    (vanilla's 1,603), entity types (1.21.1's 130; another `minecraft:` name is refused), potions;
    items, blocks and fluids as before. Writing one back gives its name (an effect's, attribute's or
    entity type's id, a sound's location).
  - `holderByNameCodec` and `RegistryFileCodec` read the registry's holder of the entry (the one
    `MobEffects.POISON` and `Attributes.ARMOR` are), as vanilla's do, `byNameCodec` the entry;
    `RegistryFileCodec`'s inline entry (allowed unless told) is a `Holder.direct` of what its codec
    reads. A holder is written by its name, a direct one in full where the codec can.
  - `FoodProperties.DIRECT_CODEC` (`{nutrition, saturation, can_always_eat, eat_seconds,
    using_converts_to, effects}`) and `MobEffectInstance.CODEC` (`{id, amplifier, duration, ambient,
    show_particles, show_icon}`; the amplifier an unsigned byte, the icon with the particles unless
    told): read with vanilla's defaults, checks and messages, written back with what differs from
    the defaults.
  - Not modelled: an effect instance's hidden effect and NeoForge's cures (`neoforge:cures` is read
    past, not kept); other vanilla registries' entries by name (particle types, block entity types,
    menus...: still unknown, as before).
  - Tests: `codecs::tests::a_registrys_codecs_read_vanillas_entries_by_name`,
    `codecs::tests::a_food_written_out_in_full_reads_its_effects` (Create's pufferfish projectile's
    `food_property`).
- [x] 2026-11-06 Part 54: a mod's blow is the game's own; knockback, a drop at an entity's feet, what a player last hurt, what a blow can find, a projectile's saved data.
  Measured in game (Part 52's potato round, the gaps it left): Create's potato hurt the cow through
  `Host::hurt_entity` (health only: no hurt flash, sound, invulnerability frames, knockback or kill
  credit), and `spawnAtLocation`, `LivingEntity.knockback`, `setLastHurtMob`, `isPickable` and the
  projectile's `addAdditionalSaveData` were unmapped. Over the pack (367 jars), classes outside
  `net/minecraft/` call `LivingEntity.hurt(DamageSource, float)` in 58 jars (363 sites),
  `isPickable` in 16 (34), `spawnAtLocation` in 13 (17), `knockback(DDD)` in 11 (26) and
  `setLastHurtMob` in 9 (18); Create calls all five.
  - A mod's blow on a vanilla entity names its source to the game (`Host::hurt_entity_by`,
    `HurtBy`): the damage type, the player it is credited to by UUID (else the mob by id), where it
    came from (`getSourcePosition`) and, dealt by a mod's projectile, that projectile's horizontal
    movement. The game deals it as `/damage` does (`native_hurt_mob`, API @API:mob_hurt@): the
    mob's invulnerability frames, armor and effects, knockback along the projectile's path (else
    away from where the blow came from), the hurt flash and sound, its anger, and the kill credited
    to that player (a guest's blow is dealt as that guest) or mob.
  - `LivingEntity.knockback(strength, x, z)` on a vanilla mob pushes it as vanilla does, less its
    knockback resistance and as native mods' knockback hooks change it (`native_knockback_mob`).
  - `Entity.spawnAtLocation(stack[, offsetY])` (and an item's) on a mod's entity: a new item entity at
    its feet raised by the offset, with vanilla's random sideways motion, 0.2 up and the default
    pickup delay (10 ticks), added to the level; none for an empty stack or on a client.
  - A player keeps what they last hurt (`setLastHurtMob`: a living entity only, else none) and when
    (their `tickCount`), as `getLastHurtMob` and `getLastHurtMobTimestamp` ask.
  - `isPickable` as 1.21.1's classes answer it: a living entity, a vehicle, a falling block or lit TNT
    while there, the dragon's parts but not the dragon, a crystal, a hanging entity, an interaction, a
    shulker bullet, a projectile of `#redirectable_projectile` (not an arrow); an item never.
  - A mod's projectile with no save data of its own saves and loads its vanilla class's, as 1.21.1
    writes it: `Owner` (by UUID, found again when asked), `LeftOwner` (only once it has),
    `HasBeenShot`, a hurting projectile's `acceleration_power`, a thrown item's stack (`Item`; one
    with no readable stack holds its class's default item).
  - Not modelled: a mod's damage hooks fired inside the mod's own call (the bundle's runtime is busy
    then, and skips them), `EnchantmentHelper.doPostAttackEffects`, the game's tamed wolves hearing
    of `setLastHurtMob`, an armor stand that is a marker (the game doesn't tell it), whether an arrow
    is stuck, an arrow's saved data.
  - Tests: `vanilla_entities::tests::a_mods_blow_names_its_source_and_its_knockback_pushes`,
    `projectiles::tests::a_mod_projectiles_blow_follows_its_path_and_it_saves_its_owner_and_stack`,
    `mod_entities::tests::a_mod_entity_drops_a_stack_at_its_feet`.
  - In game (the potato round): none of Create's 25 potato projectile types is left out any
    more (Part 53). The cannon fires its 3 shots; two potatoes hit the cow, each dealt by the game as
    `create:potato_cannon` for 5 (`hurt by 5 (create:potato_cannon): taken`), and the cow dies of the
    second, its loot on the ground. For those blows the game fires `living_incoming_damage`,
    `living_damage_pre`, `living_damage_post` and `living_knockback`, which the bundle skips: they run
    inside Create's own call (next). Gone from the round's unmapped calls since Part 52: `isPickable`
    (an item entity), `LivingEntity.knockback` (the cow), `setLastHurtMob` (the player), the potato's
    `addAdditionalSaveData` and `readAdditionalSaveData`, and its `spawnAtLocation`; left in this
    area: `EnchantmentHelper.doPostAttackEffects`.
- [x] 2026-11-06 Part 55: a hook the game runs from inside a mod's own call runs in that call.
  Measured in game (Part 54's potato round): the game fires `living_incoming_damage`,
  `living_damage_pre`, `living_damage_post` and `living_knockback` for Create's potato's blow, inside
  Create's own call; the bundle skipped each ("the game ran the ... hook from inside one of the mod's
  own calls; skipped": its runtime was busy), so no mod's listener heard the potato's hit. Earlier
  rounds skipped `place_block` 10 times the same way (the world-events round).
  - The runtime may wait for the game: a call on the game that the game may answer by running the
    mod's hooks (`Rt::host_call`: a blow, a kill, a push, a block set or placed, a feature placed, a
    command run, an effect given or taken, an item entity or orb spawned, a player's slot written)
    leaves the runtime waiting, and a hook the game runs meanwhile runs in it (`reenter`), as the JVM
    runs an event's listeners inside the call that posted it. Its innermost wait only, and one hook
    at a time: a hook inside that hook runs only where that one waits for the game in turn. A hook
    the game runs from another call (one that doesn't wait) is still skipped, as before.
  - While the game deals a mod's blow, the damage hooks for its victim hand listeners the mod's own
    `DamageSource` (its direct entity, the mod's projectile; its damage type, which the game names
    `minecraft:generic` when it doesn't know it), not one made again from the game's ids.
  - Sound: the waiting call holds no borrow of the runtime while the game runs (the game is outside
    it), the hook's runtime comes from that call's own `&mut`, and the garbage collector doesn't run
    while a game is set (as in any hook).
  - Tests: `horizon_compat_core::tests::a_hook_gets_the_runtime_that_waits_for_the_game`,
    `damage::tests::the_game_runs_the_damage_events_of_a_mods_blow_inside_it`.
  - In game (the potato round): the four hooks the game fires for the potato's blow
    (`living_incoming_damage`, `living_damage_pre`, `living_damage_post`, `living_knockback`) run in
    Create's runtime inside its call ("run there"), where Part 54's round skipped them. The cow is hurt
    twice for 5 as `create:potato_cannon` and dies; no uncaught throws. One new unmapped call:
    `EntityType.getCategory`, which the potato's `onHitEntity` asks of a target it killed (its
    advancement's check).
- [x] 2026-11-06 Measured (packcov11, after Parts 41 to 55, same 367 jars): missing uses 63458 → 61124
  (−2334), and no class's count grew. Methods mapped went from 74.2% to 75.0% summed over jars
  (98795 → 99900 of 133194), and uses from 84.7% to 85.3%. Create went from 71.0% to 72.2% of methods
  (2836 → 2883 of 3995) and from 91.1% to 91.6% of uses. Distinct API methods missing somewhere in the
  pack: 12268 → 12091. The biggest drops:

  | Class | packcov10 | packcov11 |
  |---|---|---|
  | `EntityHitResult` | 317 | 0 |
  | `CustomData` | 306 | 9 |
  | `GlStateManager` | 288 | 56 |
  | `Minecraft` | 795 | 568 |
  | `ServerGamePacketListenerImpl` | 197 | 31 |
  | `BlockState` | 1178 | 1034 |
  | `LevelRenderer` | 229 | 111 |
  | `ClientPacketListener` | 158 | 46 |
  | `RenderType` | 119 | 13 |
  | `SoundInstance` | 118 | 17 |
  | `LivingEntity` | 936 | 850 |

  Largest left: `Level` (1746: `addFreshEntity` in 86 jars, `getCapability`), the codec builders
  (`MapCodec.forGetter` in 107 jars, `RecordCodecBuilder$Instance.group`, `Products$P2.apply`),
  `Entity` (1159), `BlockState` (1034: `cycle` in 47 jars) and `Slot` (664: `getItem` in 87 jars,
  `hasItem`, `set`). Several of these are the probe's: it calls each method on stand-ins, so a call
  the layer answers only for a real object counts as missing here though it works in game. Among them:
  the codec builders, a state's `cycle`, `addFreshEntity` of a mod's entity, an item entity or an orb,
  and a slot's item (a bare `Slot` has no container, so its `getItem` asks a null one). `addFreshEntity`
  of a vanilla entity a mod makes is a real gap: a vanilla `EntityType` has no factory, so its `create`
  gives null, and a vanilla entity's constructor isn't modelled (only an item entity's, an orb's and a
  mod's own projectile's are). In Create the most missed are `Level` (150), `BlockState` (97), `Entity` (82)
  and the fluid and item handlers (`IFluidHandler` 79, `IItemHandlerModifiable` 69, `IItemHandler` 65).
- [x] 2026-11-06 Part 56: what follows a blow, and an entity type's category.
  Measured in game (the potato round): on each hit Create's potato calls
  `EnchantmentHelper.doPostAttackEffects`, and on a kill it asks `getType().getCategory()` of the
  victim before it awards its advancement; both were unmapped. Over the pack (367 jars),
  `EntityType.getCategory` is called in 19 jars (26 sites), `doPostAttackEffects` in 8 (30),
  `MobCategory.isFriendly` in 5 and `doPostAttackEffectsWithItemSource` in 3 (6). Most of the
  post-attack callers are mods' projectiles and bosses after a hit (Cataclysm, Iron's Spellbooks,
  Block Factory's Bosses, Create).
  - `EntityType.getCategory()`: a mod's type answers the category it was built with
    (`EntityType.Builder.of(factory, category)`), and vanilla's answer their 1.21.1 one (39
    monsters, 31 creatures, the water and ambient ones, the rest `MISC`). The constant is the same
    one `MobCategory.MONSTER` gives, so `==` holds. `MobCategory`'s `getName`, `getSerializedName`,
    `getMaxInstancesPerChunk`, `isFriendly`, `isPersistent`, `getDespawnDistance` and
    `getNoDespawnDistance` answer as 1.21.1's.
  - `EnchantmentHelper.doPostAttackEffects(level, victim, source)` has the game run the
    enchantments' effects that follow the blow (`Host::post_attack_effects`, `native_post_attack`,
    API @API:post_attack@).
    - A player's blow on a mob runs the effects of the player's weapon in hand. Fire aspect sets the
      mob alight, bane of arthropods slows it, and a mod's own effect types run their hooks.
    - A blow through a projectile isn't direct (`HurtBy::direct`, the projectile's type, now also
      told with a mod's blow). It runs only the effects that don't need a direct blow, and no wind
      bursts. Fire aspect and bane of arthropods need one, as 26.3's data has them.
    - A mob's blow on a player runs the player's worn thorns on that mob.
    - A guest's effects run as that guest.
    - `doPostAttackEffectsWithItemSource` names the weapon: a stack's enchantments, or with none
      only the victim's side.
  - An item entity answers `getType()` (`EntityType.ITEM`). The category test found it missing.
  - Not modelled: the effects of a mob's blow on a mob or of a player's on a player (the game runs
    neither), a mob's worn enchantments, a mod's own entity as the victim, a mod's own
    `MobCategory` (NeoForge lets mods add some), and the categories of 26.3 types that 1.21.1 lacks
    (they answer `MISC`).
  - Tests: `mod_entities::tests::an_entity_types_category_is_vanillas_or_the_one_it_was_built_with`,
    `enchantments::tests::a_blows_enchantment_effects_go_to_the_game_with_its_source_and_weapon`.
  - In game (the potato round): `doPostAttackEffects` and `getCategory` are gone from the round's
    unmapped calls. The cannon fires its 3 shots. Two potatoes hurt the cow, 5 each as
    `create:potato_cannon`, and the second kills it. The damage hooks run inside Create's own call
    (Part 55). A potato's blow isn't direct and the cannon has no enchantments, so nothing follows
    it, as in vanilla. The cow is a creature, so Create awards no advancement for it. Left in the
    round: Flywheel's GL calls, `DynamicTexture` (3), and `List.forEach` on null after
    `create:sync_edge_group`.
- [x] 2026-11-06 Part 57: vanilla entities a mod makes and adds.
  Measured over the pack (367 jars): `EntityType.create` is called in 58 jars (202 sites), 26 of
  them (79 sites) on vanilla's own types, a lightning bolt most (9 jars, 35 sites), then skeleton
  horses, horses, armor stands, zombie horses, cows and sheep. `EntityType.spawn` is in 15 jars.
  Mods also build vanilla classes directly: `new ArmorStand(level, x, y, z)` (6 jars),
  `new AreaEffectCloud(level, x, y, z)` (7), `new PrimedTnt(level, x, y, z, owner)` (4, Create's
  mounted dispenser among them), `FireworkRocketEntity`'s constructors (7, Create's mounted
  dispenser again), `FallingBlockEntity.fall` (6, Create's potato cannon placing a block). A
  vanilla type's `create` answered null, and none of those constructors was modelled, so the mod's
  next call failed. Create's haunting (an encased fan blowing through soul fire onto a horse) makes
  a skeleton horse from the horse's saved compound.
  - The entity a mod makes (`made_entities`) is an object of the type's 1.21.1 class, so
    `instanceof` answers as in Java. It keeps what the mod sets: its place, facing and motion as
    fields, the rest in the compound `saveWithoutId` would write.
    - The setters it answers: `setPos`, `moveTo` (each form), `absMoveTo`, `teleportTo`,
      `copyPosition`, `setYRot`/`setXRot` (non-finite refused), `setDeltaMovement`, `push`,
      `Projectile.shoot` (vanilla's scatter), `setCustomName`, `setCustomNameVisible`,
      `setNoGravity`, `setSilent`, `setInvisible`, `setGlowingTag`, `setInvulnerable`,
      `addTag`/`removeTag`, fire, frost and air.
    - Per class: a bolt's `setVisualOnly`/`setCause`/`setDamage`; a TNT's fuse; a falling block's
      damage; an armor stand's arms, base plate, size and marker; a mob's health, equipment
      (`setItemSlot`, as the game's `equipment`), persistence, no-AI, left hand, baby or age, and
      `finalizeSpawn` (NeoForge's `finalizeMobSpawn` too).
    - `load`/`deserializeNBT` take a compound whole (its place, motion and facing into the fields,
      `NeoForgeData` into its persistent data), and `saveWithoutId`/`serializeNBT` give it back.
    - The constructors: any vanilla class's `(EntityType, Level)` on an object of that very class
      (a mod subclass's `super(...)` is left to it), `ArmorStand(level, x, y, z)`,
      `PrimedTnt(level, x, y, z, owner)` (80-tick fuse, vanilla's hop),
      `FallingBlockEntity(level, x, y, z, state)`, and `FireworkRocketEntity`'s four with a
      position (its lifetime from the stack's flight duration, its drift, its owner, shot at an
      angle).
  - Adding it (`addFreshEntity`, `addFreshEntityWithPassengers`, `tryAddFreshEntityWithPassengers`)
    hands its compound to the game, which makes the entity as `/summon <kind> <pos> <nbt>` does
    (`Host::summon`, `native_summon`). A mob `finalizeSpawn` was asked of is finished as the game
    adds it (`horizon:finalize`). A lightning bolt strikes instead (`Host::strike_lightning`,
    `native_strike_lightning`, API @API:mod_made_entities@), as a real strike or a flash only,
    credited to its cause. From then on the object stands for the game's entity
    (`vanilla_entities::adopt`), so the mod's later calls (`getX`, `discard`) reach it. A client's
    level, a discarded entity, or an entity the level already holds isn't added.
  - `FallingBlockEntity.fall(level, pos, state)`: the block falls from the middle of its floor, no
    longer waterlogged, what its fluid leaves takes its place, and the falling block is added.
  - `EntityType.spawn` and the `create` of the same arguments: the type's entity on the block,
    facing a random way, finalized as the game adds it when a mob, given to the consumer, then
    added.
  - The game's entities answer more (Create's haunting needs them): `saveWithoutId` and
    `serializeNBT` from the game's saved compound (`Host::entity_nbt`, `native_entity_nbt`), with
    the mod data the runtime keeps for it. Also what a living one holds and wears from its saved
    `equipment` (`getBodyArmorItem`, `getItemBySlot`, the hands), `getPosition`,
    `spawnAtLocation` and `registryAccess` (the last found missing by the haunting test).
  - `tryAddFreshEntityWithPassengers` and `addFreshEntityWithPassengers` of a mod's entity or an
    item entity add it as `addFreshEntity` does (Create's trains use the first for a carriage).
  - Inside a hit's hooks the game has no level at hand, so an entity a listener adds comes once the
    hit is over, with no id for the runtime yet.
  - Not modelled: an area effect cloud (the game doesn't make one from a compound), a firework
    attached to the entity it boosts, a TNT's owner (the game keeps none for a summoned one), a
    falling block's damage set after `fall` added it, and what `finalizeSpawn` would have set before
    the mob is added. A compound a mod loads passes to the game as given. A game entity's saved
    compound is already in the game's form, but one a mod builds in 1.21.1's form isn't upgraded.
  - Tests: `made_entities::tests::a_mods_lightning_strikes_through_the_game`,
    `a_mods_tnt_is_summoned_lit_with_its_hop`, `a_horse_haunted_by_create_becomes_a_skeleton_horse`,
    `a_block_falls_and_an_armor_stand_stands_as_set`.
  - In game (the haunting round, `hauntgame.sh`): a summoned horse in iron horse armor stands in
    front of an encased fan turning at 64 RPM through soul fire. After Create's 100 haunting ticks
    it is a skeleton horse in the same place (seen at tick 140), with the horse's health (53, the
    base a horse summoned with a compound keeps), and the armor lies beside it as an item. No new
    unmapped calls. Left in the round: Flywheel's GL calls, `DynamicTexture` (3), and
    `List.forEach` on null after `create:sync_edge_group`.
- [x] 2026-11-06 Part 58: `ModelBakery`'s constants.
  The `List.forEach` on null left in every Create round isn't Create's `sync_edge_group` packet, the
  log line before it (Parts 56 and 57 named it after that). A round with
  `HORIZON_COMPAT_BACKTRACE=List.forEach` shows it comes from Create's first world render
  (`ClientEvents.onRenderWorld`): catnip's `DefaultSuperRenderTypeBuffer` starts its buffers with
  `ModelBakery.DESTROY_TYPES.forEach(...)`, and the runtime had none of `ModelBakery`'s constants.
  Measured over the pack (367 jars): `MISSING_MODEL_LOCATION` in 7 jars (9 reads),
  `DESTROY_TYPES` 5 (10: catnip's buffers, Ponder's scenes, Flywheel's draw managers, Sable, Sodium),
  `BANNER_BASE`, `MODEL_LISTER` and `GENERATION_MARKER` 4 each, `MISSING_MODEL_VARIANT` 3,
  `BREAKING_LOCATIONS`, `FIRE_0`, `FIRE_1` and `BLOCK_ENTITY_MARKER` 2 each.
  - compat `entity_render`: `ModelBakery`'s constants as 1.21.1 makes them, one object each however
    often a mod reads it. The ten stages of a block's cracks as sprites (`DESTROY_STAGES`), textures
    (`BREAKING_LOCATIONS`) and the `RenderType.crumbling` of each texture (`DESTROY_TYPES`, the same
    object a mod asking for that type gets). The fire, lava and water materials on the block atlas,
    the banner's and shield's bases on their sheets. The missing model's location and variant
    (`minecraft:builtin/missing#missing`), the models' folder (`MODEL_LISTER`, `FileToIdConverter`),
    and the two marker models (`GENERATION_MARKER`, `BLOCK_ENTITY_MARKER`: empty models named for what
    they mark, which mods compare a model's root with; their GUI light isn't kept).
  - Tests: `entity_render::tests::a_breaking_block_draws_with_vanillas_ten_crumbling_stages`.
  - In game (the fluids round, `pumpgame.sh`, which logs in as every round does): the `List.forEach`
    on null is gone from the round's unmapped calls. Left at login: Flywheel's GL calls and
    `DynamicTexture` (3), both from Flywheel's own GL backend starting (`NoiseTextures`), which
    Horizon doesn't run.
- [x] 2026-11-06 Part 59: chunks by their coordinates, and mod blocks' own `updateShape` in the game.
  The fluids round (`pumpgame.sh`: a creative motor turns a mechanical pump, a pipe from a walled-in
  water source on one side, a pipe into a fluid tank on the other) moved no fluid. Create's pipes look
  for a fluid next door only in a chunk that is there, `level.getChunk(x, z, ChunkStatus.FULL, false)`,
  and the runtime had neither that call nor `ChunkStatus`: no chunk, so the pipe did nothing.
  Measured over the pack (367 jars): `Level.getChunk` in 28 jars (53 calls), `ServerLevel.getChunk`
  19 (40), `LevelReader.getChunk` 9 (11), `ClientLevel.getChunk` 7 (8), `Level.hasChunk` 8 (13);
  `ChunkStatus.FULL` read in 19 jars (31 reads), `STRUCTURE_STARTS` 3, `STRUCTURE_REFERENCES` 2,
  `LIGHT`, `EMPTY` and `SURFACE` 1 each.
  With the chunk found, water reached the tank, but 200 mB of the source's 1000. The pipes'
  block entities showed why: pipes put down by `/setblock` keep their default shape, open on every
  side, so each still had open ends to the north and up. In vanilla the pipe's own `updateShape`
  closes them as the pump goes in next to it. Horizon has the hook point for it
  (`native_update_shape`, which `GameLevel::before_update_shape` calls for every neighbour change),
  but nothing answered it for a translated block, so a mod block's state never followed its
  neighbours. Measured: 562 classes in 88 of the pack's jars declare their own `updateShape`
  (Create 62, Create: Things and Misc 43, FramedBlocks 35, SecurityCraft 32, Electroenergetics 31,
  TFMG 20...): pipes, shafts, cables, panes, fences, connected windows, furniture.
  - compat `api`: `ChunkStatus` as 1.21.1 has it: its twelve statuses in order, one object each;
    `getIndex`, `getParent`, `isOrAfter`, `isAfter`, `isOrBefore`, `isBefore`, `max`; `getName` and
    `toString` (the registry key, `minecraft:full`); `byName` (a name it doesn't know gives `empty`,
    as the defaulted registry does); `getChunkType`; `getStatusList`. A level's `getChunk` by a
    position, by chunk coordinates, and with a status and whether the chunk must be there, gives
    the chunk its level answers for: as `isLoaded` already says, every chunk the level reaches is
    there. `hasChunk` says so too.
  - compat `glue::update_shape`: the state a mod block takes when its neighbour on one side changes,
    by its own `updateShape`, for a block whose class has one. A throw is reported and leaves the
    game's rule.
  - translator `bundle`: the generated `update_shape` hook into `native_update_shape`, for the
    translated mod's blocks. The block reads the level; the ticks it schedules go to the level's; it
    sets no block itself (vanilla's `updateShape` gives the new state and the level sets it).
  - Tests: `api::tests::a_pipe_finds_the_chunk_next_door_and_the_water_in_it`,
    `glue::tests::a_mods_pipe_joins_the_tank_put_next_to_it`.
  - The first game build with the hook stopped in the mixin tool (`E_INVALID_HELPER`): the
    generated helper carried an `#[allow(...)]`, and a helper must be one plain private function.
    The translator's tests passed it. `bundle::tests::every_helper_is_one_plain_private_function` now
    checks every helper a bundle writes against that rule.
  - In game (`pumpgame.sh`, which now prints the pipes' full states): both pipes are
    `east=true,west=true` and closed on their other four sides from the first probe (tick 20),
    where they had stayed open to the north and up. The tank holds 416 mB at tick 20 and all
    1000 mB of the source by tick 100 (200 before), and the source is drained; at tick 20 the in-pipe's
    open end still held 552 mB on its way. `Fluid.builtInRegistryHolder` and the `Holder.is` on null
    after it no longer show in the round's unmapped calls. Left: `FluidStack.saveOptional` and
    `parseOptional`, `FluidType.getLightLevel(FluidStack)`, and login's Flywheel GL and
    `DynamicTexture`.
- [x] 2026-11-06 Part 60: a fluid's registry entry, a fluid type's overloads, `FluidStack`'s NBT.
  The fluids round's own unmapped calls, measured over the pack (367 jars), every one of them also
  Create's: `Fluid.builtInRegistryHolder` in 26 jars (42 calls; `BaseFlowingFluid`'s in 3 more),
  `FluidType.getLightLevel` 18 (31), `FluidStack.parseOptional` 11 (23), `saveOptional` 10 (21),
  `FluidType.getTemperature` 5 (11), `getViscosity` 4 (8), `getDensity` 4 (7). The type's getters
  were modelled without arguments only; NeoForge's forms with a stack or a state went unmapped.
  - compat `fluids`: a fluid's `builtInRegistryHolder()`, its registry entry, whose `is(tag)` holds
    what the fluid's tags hold (`neo`'s holders ask fluid tags as `FluidState.is` does, vanilla's
    water and lava included where no game answers). A fluid type's stack and state forms
    (`getLightLevel(stack)`, `getTemperature(state, level, pos)`, density, viscosity, rarity,
    description id) are NeoForge's `this.getX()`: the type's own getter, a mod's override of it
    included; `isLighterThanAir` asks the density the same way. `FluidStack`'s NBT in its codec's
    form, `{id, amount}`: `save` (in a new compound or the one given; of nothing it throws, as
    NeoForge's does), `saveOptional` (an empty compound for nothing), `parse` (an `Optional`) and
    `parseOptional` (`EMPTY` for an empty compound or a fluid nobody registered).
  - Tests: `fluids::tests::a_fluid_types_stack_and_state_overloads_ask_its_own_getters`,
    `a_fluid_stack_saved_as_neoforge_saves_it_reads_back_the_same`, and
    `fluid_tags_hold_vanilla_water_and_lava` (the holder).
  - In game (`pumpgame.sh`, the game built with Part 61): `FluidStack.saveOptional`,
    `parseOptional` and `FluidType.getLightLevel(FluidStack)` are gone from the round's unmapped
    calls; left are login's Flywheel GL calls and `DynamicTexture`. A pipe's saved flow now carries
    its fluid, `Flow: {In: 1b, Fluid: {amount: 1, id: "minecraft:water"}}`, where it kept only its
    direction (`Flow: {In: 1b}`). The tank still holds all 1000 mB by tick 100.
- [x] 2026-11-06 Part 61: the interfaces mods' mixins add to the game's classes, and
  `FlowingFluid.getNewLiquid` from the game.
  A Create pipe that drains a water source through an open end asks whether the source fills
  again before it takes the block: `fluid instanceof FlowingFluidAccessor` (Create's accessor mixin
  on vanilla's `FlowingFluid`), then the accessor's `create$getNewLiquid`. A source between two
  sources over solid ground stays: infinite water. The runtime said no to the `instanceof`. It
  registered a mixin's interfaces for the game class the mixin targets only when a mixin class
  implemented them, never an interface mixin (an accessor) itself, and only for that very class,
  not for the game's classes under it: vanilla's water and lava are under `FlowingFluid`, and the
  drained state Create asks with is a falling flowing water. Measured over the pack: 74 jars test
  `instanceof` against an interface their own mixins add, at 653 sites (Copycats+ 80, Sable 68,
  FancyMenu 66, Steam 'n' Rails 25, Create 11...). `getNewLiquid` itself only Create calls,
  through that accessor.
  - translator `codegen`: what a mixin adds to its targets includes an interface mixin itself
    (`mixin_adds`), for the game's classes (with the interfaces' own superinterfaces) and for the
    translated ones alike. The game's are registered with `Rt::add_game_interfaces`.
  - compat-core: `instanceof` (`Rt::is_a`) finds them on a game class and on the game's classes
    under it, by the API's families; it looks only when asked for one of those interfaces.
  - compat `fluids`: `FlowingFluid.getNewLiquid(level, pos, state)` asks the game (`Host::new_liquid`)
    and answers with the same `FluidState` objects the level's blocks give; none where no game
    answers.
  - horizon-world: `Behaviours::new_liquid_for`, the `getNewLiquid` the game's fluid ticks already
    run. The game: `Game::native_new_liquid`, which the block entity ticks' host answers with.
  - Tests: `fluids::tests::a_drained_source_asks_the_game_whether_it_fills_again`.
  - In game (`INFINITE=1 pumpgame.sh`: the round's source walled in between two more, over stone):
    the source is still `minecraft:water[level=0]` at every probe, and the tank holds 448 mB at
    tick 20, 3096 at tick 100, 6416 at tick 200 and 8000, a one-block tank's capacity, by tick 400.
    With the source alone (`pumpgame.sh`) it is drained and the tank stops at 1000 mB, as before.
    No unmapped call or throw of the round's own; login's Flywheel GL calls and `DynamicTexture`
    are left.
- [x] 2026-11-06 Part 62: a mod block's own use on the client.
  The spout round (`spoutgame.sh`: three spouts holding 1000 mB of water over three depots, a
  glass bottle, an empty bucket and dirt put on the depots with a right click in survival, and an
  item drain right-clicked with a water bucket) turned the dirt into grass, but the bottle and the
  bucket were back in the player's inventory within 40 ticks, the depots empty, and the water
  bucket ended up an empty bucket with nothing in the drain. Vanilla's client asks the clicked
  block itself (`MultiPlayerGameMode.performUseItemOn`: `useItemOn`, then `useWithoutItem` for the
  main hand) and ends the click when it takes it; Create's depot answers `SUCCESS` on the client
  and takes the item only on the server. Horizon's client predicted the click from the game's own
  blocks, so a mod block always passed: the click went on to the other hand, whose use reached
  the server with an empty hand (the depot handed the item back), and to the item's own use (the
  water bucket emptied over the drain). Measured over the pack (367 jars): 565 classes in 87 jars
  declare their own `useItemOn` (Create 71, FramedBlocks 37, Copycats+ 31, Ars Nouveau 29, Create
  Aeronautics 29, TFMG 29...), 319 in 83 jars their own `useWithoutItem` (SecurityCraft 40, Applied
  Energistics 2 23, Copycats+ 23, Refurbished Furniture 22, Create 13...).
  - compat `glue::client_use_item_on`: once `RightClickBlock` lets the click through, the mod's own
    block is used on the client level as `performUseItemOn` does it, with the hand's stack and the
    local player; a use that consumes the action answers for the click (`SUCCESS` swings,
    `CONSUME` doesn't), so the other hand and the item aren't tried. A block that passes, and the
    game's own blocks, leave the click to the game as before.
  - Tests: `glue::tests::a_mod_block_that_takes_a_click_on_the_client_ends_it_there`.
  - In game (`spoutgame.sh`): the bucket stays on its depot (`Item: {count: 1, id:
    "minecraft:bucket"}` at the end) and the dirt is a grass block, its spout down to 500 mB. The
    water bucket clicked on the drain stays a water bucket in the hand (`inventory_change: {}`), so
    nothing is poured out over it. The drain's tank stays empty and the bucket under its spout
    stays empty: NeoForge's `FluidBucketWrapper`, which both go through, isn't modelled yet. The
    glass bottle is filled (its spout is down to 750 mB) but nothing comes back, since
    `PotionContents.createItemStack` is unmapped.
- [x] 2026-11-06 Part 63: NeoForge's fluid handlers for buckets and cauldrons, and `FluidUtil`.
  After Part 62 the spout round kept the bucket on its depot, but its spout never filled it, and
  the item drain clicked with a water bucket stayed empty. Both go through the bucket's own fluid
  handler: NeoForge gives every item whose class is exactly `BucketItem` a `FluidBucketWrapper`
  (`CapabilityHooks.registerFallbackVanillaProviders`, after the mods' own providers), and Create's
  `GenericItemFilling` checks for that exact class. Nothing answered `FluidHandler.ITEM` for a
  bucket, so `canItemBeFilled` and `canItemBeEmptied` said no. Measured over the pack: 7 jars make or
  extend `FluidBucketWrapper` themselves (Mekanism, SecurityCraft, Ad Astra, Architectury...),
  CreateDragonsPlus registers its dragon's breath cauldron with `RegisterCauldronFluidContentEvent`,
  and 15 jars call `FluidUtil` (`interactWithFluidHandler` 7, `getFluidContained` 6,
  `getFluidHandler` 6, `tryFluidTransfer` 4, the container helpers 4...).
  - compat `fluid_wrappers` (new): `FluidBucketWrapper` as NeoForge writes it (a bucketful in or out
    of one bucket, the container becoming the filled or the empty bucket; its protected field
    `container`, and `getFluid`, `setFluid` and `canFillFluidType` called virtually for a mod's
    subclass); `CauldronFluidContent` (vanilla's empty, water and lava cauldrons, then
    `RegisterCauldronFluidContentEvent` for the mods', before the capabilities event as NeoForge's
    `RegistrationEvents.init`) and `CauldronWrapper` (whole increments only: a water cauldron's
    three levels hold 1000 mB, so it fills from empty to full and drains from full to empty; a
    block that is no cauldron any more throws NeoForge's `IllegalStateException`); `FluidUtil`'s
    `getFilledBucket`, `getFluidHandler` (both), `getFluidContained` and `tryFluidTransfer` (both).
    Milk stays without a handler: NeoForge's milk fluid isn't registered here.
  - compat `capabilities`: a cauldron's `FluidHandler.BLOCK` is NeoForge's wrapper, ahead of every
    mod's provider (`isBlockRegistered` says so); a stack's `FluidHandler.ITEM` falls back to the
    bucket's wrapper after them. `fluids`: `FluidType.getBucket(stack)`. `instanceof
    IFluidHandler`/`IFluidHandlerItem` knows the wrappers and NeoForge's tank. The three classes
    leave `vanilla_records`' data classes.
  - Tests: `fluid_wrappers::tests::a_bucket_takes_a_bucketful_and_gives_it_back`,
    `a_cauldron_fills_and_drains_a_bucketful_at_a_time`,
    `cauldron_contents_are_found_by_block_and_by_fluid`, `fluid_util_finds_and_moves_a_buckets_fluid`.
    In game (with Part 64's build, the spout round): the bucket under its spout became a water
    bucket and its spout went from 1000 to 0 mB; the water bucket clicked on the item drain left
    1000 mB in the drain's tank and an empty bucket in the hand. No throws.
- [x] 2026-11-06 Part 64: potion contents on mods' stacks and the game's.
  The spout round's glass bottle was filled (its spout gave 250 mB) but the depot was left holding
  nothing: Create makes the water bottle with `PotionContents.createItemStack(Items.POTION,
  Potions.WATER)`, which was unmapped. A potion's contents are vanilla's `minecraft:potion_contents`
  component, which the game's stacks didn't carry to the mods' and back either, so a potion a mod
  made or read was a bottle of nothing. Measured over the pack: 34 jars use `PotionContents` (its
  constructor 18, `getAllEffects` 14, `EMPTY` 13, `createItemStack` 13 (Create, SecurityCraft,
  PuzzlesLib, RPG Potions...), `getColor` 9, `potion()` 9, `is` 7, `hasEffects` 6...).
  - compat `food`: `PotionContents.createItemStack`, `is` (vanilla's: that potion, with no effects
    of its own) and `withPotion`; the contents' NBT form as vanilla's codec writes it (`{potion,
    custom_color, custom_effects}`, an effect `{id, amplifier, duration, ambient, show_particles,
    show_icon}`, defaults left out) and read back, a potion's id alone too.
  - compat `components`: a stack's `minecraft:potion_contents` goes to the game's stack and comes
    back (also in a block entity's saved items); vanilla's potions and tipped arrows hold
    `PotionContents.EMPTY` when nothing was set.
  - horizon-client `native_components`: the vanilla components a translated mod's stacks carry
    include the potion contents, read and set as the game's own stacks read them.
  - Left: `getAllEffects`, `hasEffects`, `getColor` and `forEachEffect` need each potion's effects,
    which the game's tables have (`horizon-client-core::potions`) and the runtime doesn't yet.
  - Tests: `food::tests::a_made_potion_carries_its_contents_to_the_game_and_back`.
  - In game (the spout round): the glass bottle's depot holds a `minecraft:potion` with
    `{"minecraft:potion_contents": {potion: "minecraft:water"}}`, drawn as the game's water bottle;
    its spout went from 1000 to 750 mB. The dirt's spout made grass (500 mB left), as before.
- [x] 2026-11-06 Part 65: potions' effects and colors; vanilla effects' category, color and kind.
  A potion a mod read had no effects and no color: `Potions.X` were bare holders, and vanilla's
  effects (`MobEffects.X`) knew their id only, so their `getColor` was 0 and `getCategory` null.
  Measured over the pack: `PotionContents.getAllEffects` 14 jars, `MobEffect.getCategory` 13,
  `PotionContents.getColor` 9, `MobEffect.isInstantenous` 9, `Potion.getEffects` 9, `Potion.<init>`
  7, `hasEffects` 6, `MobEffect.getColor` 4, `MobEffectCategory.getTooltipFormatting` 4,
  `forEachEffect` 4, `Potion.getName` 3. Create's potion fluid takes its tint from `getColor()`,
  its name from `Potion.getName`, its tooltip from `getAllEffects` and the effects' categories, and
  the open pipe's spill walks `forEachEffect`.
  - `tools/vanilla_mob_effects.py` also reads each effect's category, color and class (an
    `InstantenousMobEffect` is instantaneous) from `MobEffects`' static initialiser, and each
    potion's name and effects from `Potions`' (`vanilla_effects.rs`: 39 effects, 46 potions).
  - compat `food`: vanilla's effects answer `getColor`, `getCategory`, `isBeneficial` and
    `isInstantenous` from the table; `MobEffectCategory.getTooltipFormatting`. `Potion`: a mod's
    `new Potion([name,] effects...)`, `getEffects` (vanilla's made once from the table),
    `hasInstantEffects`, the static `getName`, `isEnabled`. `PotionContents`: `getAllEffects`,
    `forEachEffect` and `customEffects` (copies), `hasEffects`, `getColor` (the instance's and both
    statics) and `getColorOptional`: vanilla's mix, in Java's int arithmetic.
  - Tests: `food::tests::vanilla_effects_have_their_colour_category_and_kind`,
    `potions_hold_their_effects_and_mix_their_colours` (turtle master's -7503130 and the water blue
    -13083194, worked out by hand from vanilla's formula).
  - Left: `MobEffect.applyInstantenousEffect` (8 jars; Create's potato cannon and open pipes) needs
    an entity healed and harmed.
- [x] 2026-11-06 Part 66: vanilla containers as NeoForge's item handlers; `getClass` through any type.
  A Create funnel, chute, mechanical arm or deployer that meets a vanilla chest, furnace or hopper
  asks the block for `Capabilities.ItemHandler.BLOCK`, and nothing answered for the game's own
  blocks: NeoForge registers them itself (`CapabilityHooks.registerVanillaProviders`, ahead of the
  mods' providers), each as a wrapper over the block's `Container`. Measured over the pack: 25 jars
  make an `InvWrapper` (82 calls), 13 a `CombinedInvWrapper`, 11 a `SidedInvWrapper` (29), 9 a
  `RecipeWrapper`, 2 a `RangedWrapper`, 1 a `VanillaHopperItemHandler`; so the wrappers work over
  any `Container`, a mod's as well as the game's.
  - compat `item_wrappers` (new): `InvWrapper`, `SidedInvWrapper` and `VanillaHopperItemHandler` as
    NeoForge writes them: `insertItem` in its order (the slot's stack, `canPlaceItem`, the limit of
    the slot and of the stack, then `setItem` and `setChanged`), `extractItem`, `setStackInSlot`
    (`InvWrapper`'s sets the item only; `SidedInvWrapper`'s marks the container changed first),
    `getSlotLimit` and `isItemValid` (called virtually for a mod's subclass). `SidedInvWrapper` maps
    its slots through `getSlotsForFace` and asks `canPlaceItemThroughFace` and
    `canTakeItemThroughFace` (with a side only); a brewing stand's three bottle slots hold one, and
    a furnace's fuel slot takes one bucket into an empty slot. The hopper's handler gives it
    vanilla's 8-tick cooldown when it was empty and an item went in (unless it was already waiting
    longer). The container behind a wrapper is a mod's (its own methods, called virtually) or the
    game's: `compat:VanillaContainer`, one per position, answering `Container` and
    `WorldlyContainer` through the host.
  - compat `capabilities`: a vanilla block's `ItemHandler.BLOCK` is found first, as NeoForge's own
    providers are: a chest (both halves of a double one), a barrel, a dispenser, a dropper, a
    crafter, a chiseled bookshelf and a jukebox get an `InvWrapper`; a hopper its handler; a
    furnace, blast furnace, smoker, brewing stand and shulker box a `SidedInvWrapper` for the side
    asked. `InvWrapper` leaves `vanilla_records`' data classes.
  - compat-core `Host`: `container`, `container_item`, `set_container_item`, `container_changed`,
    `container_can_place` (`canPlaceItem`) and `container_can_place_through`, `container_face_slots`,
    `container_can_take` and `hopper_filled`; `MemoryHost` keeps containers by position for tests.
  - horizon-client `native_items`: `native_vanilla_container` and its slot helpers read and change
    the game's own containers (the same `View` the game's hoppers use, so a double chest and a
    hopper's cooldown stay one), and the bundle's hosts map the `Host` methods onto them
    (fragment `vanilla_item_handlers`).
  - compat `api`: `getClass()` called through an interface or any other type is `Object.getClass`
    (Create's `GenericItemFilling` compares an `IFluidHandlerItem`'s class with `FluidBucketWrapper`). `MODULE_NAMES` was one short
    since `fluid_wrappers`; it lists every module again.
  - The draw host answers `player_place` and `player_sneaking` (a mod's block placed from the
    client side asks for them).
  - Tests: `item_wrappers::tests::a_chest_fills_up_to_a_stack_and_gives_back_the_rest`,
    `a_furnace_takes_and_gives_through_its_faces`,
    `a_brewing_stand_holds_one_bottle_a_slot_and_a_shulker_box_no_shulker_box`,
    `a_hopper_filled_while_empty_waits`, `a_wrapper_of_a_mods_container_asks_the_container`;
    `api::tests::get_class_through_any_type_is_the_objects_class`.
  - Left: the composter's handler (`ForwardingItemHandler` over its `SidedInvWrapper`), entities'
    handlers (minecarts, `PlayerInvWrapper`), and `isBlockRegistered` for the vanilla providers.
  - In game (the funnel round, `funnelgame.sh`, build of a53f72f): andesite funnels facing up on a
    chest, a furnace and a hopper (kept off by a redstone block, so it can't take the items itself)
    each took the stack dropped on them: the chest holds 5 iron ingots in slot 0, the furnace 3 raw
    iron in its input slot (through `SidedInvWrapper`'s top face), the hopper 7 dirt; no item
    entity was left on them. An extracting funnel on a barrel of 10 gold ingots took one at a
    time and spat them out on top (3 out, 7 left, while the first one flew clear of the funnel).
    No throws. A healing potion emptied into an item drain gave 250 mB of `create:potion`, but
    without its `minecraft:potion_contents`: `FluidStack.set` is unmapped (fluid stacks keep no
    components yet).
- [x] 2026-11-06 Part 67: components on NeoForge's fluid stacks; vanilla records and `Optional`s equal by value.
  A healing potion emptied into Create's item drain gave `create:potion` without its potion. Create
  keeps a potion fluid's contents on the stack (`FluidStack.set(DataComponents.POTION_CONTENTS, ...)`:
  NeoForge 21.1's `FluidStack` is a `MutableDataComponentHolder`), and fluid stacks kept no
  components, so the fluid's tint and name had nothing to read. Measured over the pack: 45 jars call
  a component method of `FluidStack`: `isSameFluidSameComponents` 16 jars (52 calls),
  `copyWithAmount` 18 (42), `getHoverName` 23 (36), `getComponentsPatch` 18 (33), `get` 8 (10),
  `applyComponents` 6 (10), `getComponents` 8 (9), `getOrDefault` 3 (8), `set` 6 (8), `has` 3 (4),
  `remove` 4 (4), `update` 2 (4).
  - compat `components`: a fluid stack holds components as an item stack does, through the same
    calls (`set`, `get`, `has`, `getOrDefault`, `remove`, `update`, `getComponents`,
    `applyComponents`, `getComponentsPatch`, `isComponentsPatchEmpty`), with no item defaults and no
    damage. The shared `FluidStack.EMPTY` takes none, and an empty stack's patch is
    `DataComponentPatch.EMPTY`.
  - compat `fluids`: the components go where the stack goes, as NeoForge writes it: the constructor
    with a patch, `copy`, `copyWithAmount`, `copyAndClear`, `split` and `limitSize` keep them;
    `isSameFluidSameComponents`, `isFluidEqual`, `areFluidStackTagsEqual`, `containsFluid` and
    `isFluidStackIdentical`/`matches` compare them. `getHoverName` and `getDescriptionId` ask the
    fluid type for the stack (`FluidType.getDescription(stack)`, a mod's own override first: Create's
    potion fluid names itself by its potion).
  - compat `tanks`: `FluidTank` fills only with the same fluid and components (a tank of healing
    takes no swiftness) and gives copies that keep them. `FluidStack.save` writes
    `{id, amount, components}` (the components when there are some), and `parse`/`parseOptional`
    read them back; a recipe's fluid stack JSON reads `components` too.
  - compat `network`: `FluidStack.STREAM_CODEC` (and `OPTIONAL_STREAM_CODEC`) write the patch after
    the fluid and read it back.
  - compat `jutil` and `records`: `Object.equals` and `hashCode` of a vanilla record or data class
    made here are the class's own (component by component), and two `Optional`s compare what they
    hold. Before, two `PotionContents` of the same potion made apart were different, through
    `Objects.equals`, a map's keys and a stack's components (an item stack's
    `isSameItemSameComponents` of two potions made apart included).
  - Test: `fluids::tests::a_fluid_stacks_components_go_where_the_stack_goes` (set and read back;
    copies and splits keep them; the same potion made twice is the same fluid and swiftness isn't;
    a tank of healing fills with healing only and drains it; saved and parsed NBT; the network codec
    round trip). Compat tests 485, translator 19.
  - Left: `FluidStack.hashFluidAndComponents` still hashes the fluid only.
  - In game (Part 68's round): a healing potion right-clicked on Create's item drain in survival
    leaves 250 mB of `create:potion` in its tank with `minecraft:potion_contents` (`healing`) and
    `create:potion_fluid_bottle_type` (`regular`). The fluid was drawn white until Part 69.
- [x] 2026-11-06 Part 68: hoppers, droppers and crafters reach mods' machines (NeoForge's `VanillaInventoryCodeHooks`).
  A vanilla hopper on a Create depot did nothing: NeoForge patches `HopperBlockEntity`,
  `DropperBlock` and `CrafterBlock` so each asks the block it pushes into or pulls from for its
  `Capabilities.ItemHandler.BLOCK`, and the game's hoppers knew only the game's own containers.
  Measured over the pack: 67 jars of 369 use `Capabilities.ItemHandler.BLOCK` (224 classes), and 67
  register a block entity capability (184 `registerBlockEntity` calls, 15 `registerBlock`); a hopper
  should reach each of those machines.
  - horizon-client: four hooks, asked where the block faced has no container of the game's own (the
    game's containers keep vanilla's rules): `native_hopper_insert` in `ejectItems` (left false, a
    container entity there is tried), `native_hopper_extract` in `suckInItems` (a block with a handler
    answers, and the items lying on it stay), `native_dropper_insert` in the dropper's `dispenseFrom`
    (handled: nothing drops, no sound) and `native_crafter_output` in the crafter's `dispenseItem`
    (what is left flies out). Fragment `vanilla_inventory_hooks`.
  - compat `item_wrappers`: NeoForge's `insertHook`, `extractHook`, `dropperInsertHook` and
    `insertCrafterOutput` as written. Nothing goes into a full handler (`isFull`); one item a turn,
    the hopper's slot put back for each item the handler refuses; `putStackInInventoryAllSlots`
    offers a stack to each slot and puts it only where it fits whole (6 bread skip a slot with room
    for 2); a hopper pulls into its first slot that takes the item, past full ones. The hopper,
    dropper and crafter are the game's containers (`compat:VanillaContainer`); the handler is the
    block's (`capabilities::block_item_handler`: the providers in NeoForge's order).
  - The bundle's helpers (`hopper_insert`, `hopper_extract`, `dropper_insert`, `crafter_output`) ask
    only when the block faced is one of the mod's.
  - The `mod_screen_input` and `vanilla_item_handlers` fragments listed files without their marker,
    which the stamp refuses; they list only files that have it now.
  - Tests: `item_wrappers::tests::a_hopper_pushes_one_item_a_turn_into_a_mods_handler`,
    `a_hopper_pulls_one_item_a_turn_out_of_a_mods_handler`,
    `a_dropper_puts_one_item_into_a_mods_handler_or_keeps_it` and
    `a_crafters_output_goes_where_it_fits_whole`, against a mod's `ItemStackHandler` registered for
    its block entity type. Compat tests 489, translator 19.
  - Left: hopper minecarts (`MinecartHopper.suckInItems` runs `extractHook` too), entities'
    `ItemHandler.ENTITY_AUTOMATION` in `getItemHandlerAt`, and `insertStack`'s cooldown for a
    destination that is a `HopperBlockEntity` (a mod's subclass).
  - In game (`hoppergame.sh`, survival, blocks set by command): the chain's 5 iron ingots went
    hopper, depot, hopper, chest (the chest holds 5; the depot and both hoppers are empty); a pulsed
    dropper put one cobblestone onto its depot and kept 2; a pulsed crafter put its 4 oak planks
    onto its depot; no item entity was left lying.
- [x] 2026-11-06 Part 69: `RegisterEvent` for NeoForge's own registries: Create's fluid types, and their tints.
  Create's item drain drew a healing potion white and untextured. Catnip's fluid renderer asks
  `IClientFluidTypeExtensions.of(fluid)` for the still texture and the tint, and Create's fluid types
  hand theirs from `initializeClient`, which the client asks of each fluid type in NeoForge's
  registry (`ClientExtensionsManager.earlyInit`). Registrate registers a mod's fluid types on
  `RegisterEvent` for `neoforge:fluid_type` (`FluidBuilder.register`: `simple(this, sourceName,
  Keys.FLUID_TYPES, fluidType)`), and the runtime fired the event for the game's registries only. So
  none of Create's fluid types was registered, each got the default extension (no texture, white),
  and catnip fell back to the missing sprite. Measured over the pack, jars naming NeoForge's
  registries: `ATTACHMENT_TYPES` 17, `CONDITION_CODECS` 14, `ENTITY_DATA_SERIALIZERS` 12,
  `BIOME_MODIFIER_SERIALIZERS` 11, `FLUID_TYPES` 10, `INGREDIENT_TYPES` 9,
  `GLOBAL_LOOT_MODIFIER_SERIALIZERS` 9, `STRUCTURE_MODIFIER_SERIALIZERS` 2, `HOLDER_SET_TYPES` 1.
  - compat `neo`: `RegisterEvent` fires for NeoForge's registries after the game's, by name, as
    `GameData.getRegistrationOrder` does: attachment types, biome and structure modifier
    serializers, condition codecs, entity data serializers, fluid ingredient types, fluid types,
    global loot modifier serializers, holder set types, ingredient serializers. `attachment_types`
    was listed before, but the key named the registry `attachment_type`, so no mod's registration
    matched. The fluid type registry also finds NeoForge's types of vanilla's water, lava and empty.
  - compat `codecs`: `NeoForgeRegistries`' fields and its `Keys` name their registries as NeoForge
    does, from a table: `CONDITION_SERIALIZERS` and `Keys.CONDITION_CODECS` are both
    `condition_codecs`, `INGREDIENT_TYPES` is `ingredient_serializer`, `ATTACHMENT_TYPES` is
    `attachment_types`, `BIOME_MODIFIERS` is `biome_modifier`. Before, each name came from the field's
    own, and a registry's field and its key's could disagree.
  - compat `api_common`: `MissingTextureAtlasSprite.getLocation` (`minecraft:missingno`; 26 jars, 41
    calls), and `LocalPlayer.clientLevel` read by a mod (8 jars, 19 classes: Create's client packets,
    `BlockEntityDataPacket` among them, find their block entity in it).
  - Tests: `neo::tests::register_event_fires_for_neoforges_own_registries` (a fluid type, an
    attachment type, a condition codec, an ingredient type and a data serializer registered on the
    event, found through the registry's field; water's type found by name);
    `client_extensions::tests::initialize_client_overrides_hand_their_extensions_at_setup` registers
    its types through `NeoForgeRegistries.FLUID_TYPES`. Compat tests 490, translator 19.
  - In game (`draingame.sh`, survival): a healing potion and a honey bottle right-clicked on two
    item drains leave 250 mB of `create:potion` (with its potion) and of `create:honey`; the potion
    is drawn in healing's red, white before. Both fluids are drawn without their textures: Create
    adds its fluid sprites to the block atlas in `assets/minecraft/atlases/blocks.json`, which the
    content mod doesn't carry (Part 70).
- [x] 2026-11-06 Part 70: mods' additions to atlases: Create's fluids drawn with their textures.
  Part 69's drains drew the potion red but untextured, and honey plain white: catnip asked for
  `create:fluid/potion_still`, the game's atlas had no such sprite, and the compat drew the white
  sprite. Create lists its fluid sprites in `assets/minecraft/atlases/blocks.json`, as a resource
  pack adds to an atlas, and neither the translator's content mod (only `assets/<id>/`) nor a
  pack's carry (a mod's own namespaces and its tag additions) took that file. Measured over the pack: 51 jars ship atlas files, 37 of them add to
  `minecraft:blocks`; their sources are `single` (1,136), `directory` (29), `paletted_permutations`
  (7), `unstitch` (3) and one `framedblocks:anim_splitter`, a kind FramedBlocks' code registers.
  - translator `bundle`: a mod's content mod (and each jar-in-jar mod's) carries its
    `assets/<ns>/atlases/*.json` whatever the namespace, besides its own namespace's assets.
  - port-pack `carry`: the same for a carried pack, like a mod's tag additions; they name
    namespaces for the claim as tag additions do (a namespace whose textures its atlas additions
    list is the mod's).
  - horizon-mods: the content fence lets a mod add to any namespace's atlases (docs/mods.md).
  - client `mod_content::build`: atlases merge as resource packs' do, the base's sources first,
    then each mod's in load order. A source of a kind the game does not read is left out with a
    warning: the game's reader fails a whole atlas file on one unknown source, and FramedBlocks'
    would have dropped vanilla's blocks with it.
  - client `hud`: a file a mod lists in the block atlas outside `block/` and `item/` keeps its
    first frame and animates (Create's potion, milk, tea and chocolate stills are 16x512 strips of
    32 frames, the flows 32x1024); before, only those two folders were cut, and a strip packed
    whole. Create's block atlas also lists `create:entity/*` sprites its renderers draw (blueprints,
    couplings, the train and logistics hats).
  - Tests: `bundle::tests::a_mods_look_and_the_mods_it_bundles_become_content_mods` (its block
    atlas addition carried), `carry::tests::claims_the_namespaces_its_files_name` (an atlas
    addition carried, its textures' namespace claimed), `mod_content::tests::atlas_additions_merge_after_the_bases_sources`,
    `hud::tests::an_animated_sprite_a_mod_lists_in_the_block_atlas_packs_its_first_frame_and_animates`,
    horizon-mods' `content_only_mod_round_trips` with an atlas file.
  - Left: 1.21.1's `paletted_permutations` read palettes from `textures/`, 26.3's from
    `textures/palettes/` (vanilla's trim palettes moved to `trim/<material>` and `trim_base`), so a
    mod's trim permutations generate nothing yet: Create's cardboard armour items' in the block
    atlas, and Create's 504, Cataclysm's 144 and Biomes O' Plenty's 72 in `minecraft:armor_trims`,
    an atlas 26.3 does not have.
  - In game (`draingame.sh`, survival, the drains of Part 69): the content root's block atlas holds
    22 sources, vanilla's 4 and Create's 18 (8 of them its fluids); the potion drain's surface is
    Create's potion texture in healing's red, the honey drain's Create's honey texture, both flat
    colours before.
- [x] 2026-11-06 Part 71: NeoForge's item handlers of entities: a player's inventory, a mob's hands
  and armour, mods' entity providers. `entity.getCapability(...)` answered nothing: an entity's
  capability had no lookup, and NeoForge's player and equipment wrappers weren't modelled. Measured
  over the pack: `Capabilities.ItemHandler.ENTITY` is read by 15 jars in 121 classes (95 of them
  create-stuff-additions', its tools and armour reaching the player's inventory; sophisticatedcore,
  create_jetpack, create_things_and_misc, create_winery, Jade, ars_nouveau among the rest),
  `ENTITY_AUTOMATION` by 9; `getCapability` is called on `Entity` 116 times in 12 jars, on
  `Player` 8, `LivingEntity` 7; 13 jars register 17 entity providers (`registerEntity`). Of the
  wrappers, `new PlayerMainInvWrapper` 12 times in 8 jars (Create's toolbox returns a player's
  items through its subclass `ItemReturnInvWrapper`, which keeps the hotbar out), `RangedWrapper`
  5 in 2, `EntityHandsInvWrapper` and `EntityArmorInvWrapper` 4 each (create-stuff-additions);
  `getEquipmentSlotForItem` is called in 7 jars, `ItemStack.canEquip` 15 times in 9.
  - compat `capabilities`: `Entity.getCapability(cap[, context])` through any entity class, in
    `CapabilityHooks`' order: NeoForge's own `PlayerInvWrapper` of a player (a fake player's own
    inventory for Create's deployer) ahead of every mod; then the providers mods registered for the
    entity's type (`ICapabilityProvider.getCapability(entity, context)`), the first that answers;
    then NeoForge's fallback for every type: a living entity's hands and armour as one
    `CombinedInvWrapper`.
  - compat `item_wrappers`: `RangedWrapper`, `PlayerMainInvWrapper`, `PlayerArmorInvWrapper`,
    `PlayerOffhandInvWrapper` and `PlayerInvWrapper` as NeoForge writes them (each part a range of
    the inventory's `InvWrapper`; the armour's takes only what is worn in the slot, through
    `stack.canEquip`), and `EntityEquipmentInvWrapper` with its hands' and armour's subclasses
    (`getItemBySlot`/`setItemSlot` called virtually, one item to an armour slot, 99 to a hand,
    `IllegalArgumentException` past the slots). A mod's subclass reaches them through `super`
    (the wrappers' superclasses are in `api_super`, so `instanceof` and inherited calls climb).
    `ItemStack.canEquip(slot, entity)` and `LivingEntity.getEquipmentSlotForItem(stack)` answer
    where the stack is worn: an `Equipable` mod item's own slot (a mod's armour by its type), the
    game's for a vanilla item, else the main hand.
  - compat-core `Host::item_equip_slot` (client `native_item_equip_slot`, API @API:hand_stack@):
    the slot the game equips an item in (its `equippable` component), so a diamond helmet goes on
    the head and an elytra on the chest.
  - compat `menus`: a player's `Inventory` keeps `Container`'s defaults it doesn't override:
    `canPlaceItem`, which NeoForge's `InvWrapper` asks before it fills a slot, answered nothing.
  - compat `blocks`: NeoForge's `DyeColor.getColor(stack)` (13 jars, 35 calls): a dye's colour,
    else the first colour whose `c:dyes/<colour>` tag holds the item (NeoForge's loop stops before
    black), else null. Unmapped, it answered the stand-in of a static method returning its own
    class, a new `DyeColor`: Create's toolbox took any item it was clicked with for a dye and
    turned white instead of opening.
  - horizon-ctl: `key alt` (`lalt`, `option`), Create's toolbelt key.
  - Tests: `item_wrappers::tests::a_players_handler_is_their_inventory_whose_armour_takes_only_what_is_worn_there`
    (41 slots, dirt onto dirt, a helmet only on the head and one of two, torches out of the off
    hand, the game's slots changed), `item_wrappers::tests::a_stack_is_equipped_where_its_item_is_worn`
    (a helmet, an elytra, a mod's boots by their armour type, dirt), `capabilities::tests::an_entitys_capability_is_its_types_providers_then_neoforges_fallback`
    (the first provider that answers, a mod's handler ahead of the fallback, a zombie's sword in
    the fallback's slot 0, an armour slot holding one, none for an arrow),
    `blocks::tests::a_stack_has_a_dye_colour_only_when_it_is_a_dye`. Compat 494 tests, translator 19.
  - In game (`toolboxgame.sh`, survival, a brown toolbox, an iron pickaxe held): right-clicked, the
    toolbox opened its menu (`ToolboxScreen`: the 8 compartments around the lid, the inventory
    below) and stayed brown; before the `getColor` fix it became a white toolbox and opened
    nothing. Not reached yet: `click-slot` numbers the game's own screens' slots, so it missed the
    mod screen's (the pickaxe stayed in the hotbar), and Alt opened no radial menu: the control
    socket's keys never reach `InputEvent.Key` (the hook takes the window's own key events), which
    Create opens the menu on. The toolbox's `ToolboxEquipPacket`, which returns the hotbar's item
    through `ItemReturnInvWrapper`, waits for that.
  - Left: a horse's own inventory (NeoForge's fallback gives `InvWrapper(horse.getInventory())`)
    and a chest boat's or minecart's container (`InvWrapper(entity)`) aren't the host's to give:
    none for those. A vanilla mob's equipment reads through its saved `equipment`; the host can't
    change it (`setItemSlot` on the game's entity is unmapped). `PlayerMainInvWrapper`'s pop
    animation and menu broadcast after an insert, and `canUseSlot` for mobs, aren't modelled; the
    inventory a player's handler reaches is the acting player's (as every `Inventory` call's).
- [x] 2026-11-06 Part 72: the control socket's keys reach mods' `InputEvent.Key`. `horizon-ctl key`
  drove the game's own key handling and the open screen, but mods heard only the window's real key
  events (`native_input_key` takes a winit `KeyEvent`), so nothing a mod opens on a key in the world
  could be driven from a test: Create opens its toolbox's radial menu on the toolbelt key (left Alt)
  there. Measured over the pack: 21 jars read `InputEvent.Key` (`getAction` 20 jars, `getKey` 19,
  `getScanCode` 8, `getModifiers` 6).
  - client: hook point `native_control_key(key, action, modifiers)` (API @API:control_key@), run
    after a `key` command the game handled without error, as `native_input_key` runs after the
    window's: vanilla's name for the key (`key.keyboard.left.alt`), GLFW's action (a `tap` is a
    press, then a release; `repeat` a repeat) and the modifier bits the screen hooks pass,
    captured before handling. The window's keys keep their own hook, so a key is heard once.
  - translator: a client helper per mod for the hook, `InputEvent.Key` through
    `glue::input_key`, as the window's.
  - Tests: `control::tests::socket_keys_reach_mods_by_vanillas_name`; the hook catalog test
    (`catalog_matches_hook_points`) checks the new fragment against the hook point. Translator 19
    tests, gameplay 70 (`native_hooks`, `control` and the rest of `game-tests.sh`).
  - In game (`toolboxgame.sh`, survival, a brown toolbox): `key alt down` opened Create's radial
    toolbox menu (`RadialToolboxMenu`: its 8 compartment slots in a ring around the crosshair and
    the toolbox button beside them; the screen reads `mod`); before, nothing opened.
    The equip itself waits for the next step: the pickaxe, shift-clicked on Create's own slot (its
    tooltip under the cursor), stayed in the hotbar. `ToolboxInventory.isItemValid` asks
    `Item.canFitInsideContainerItems()`, unmapped (false), so the toolbox refused every item, and
    `1` found no compartment to pick.
- [x] 2026-11-06 Part 73: items inside items, items' bars, and another player's inventory: Create's
  toolbox end to end. Two `Item` defaults NeoForge's mods lean on were unmapped.
  `Item.canFitInsideContainerItems()` answered false (the unmapped default), so Create's toolbox
  (`ToolboxInventory.isItemValid` asks it first) refused every item: 7 jars call it (10 calls), and 8
  classes in 5 jars override it (Create's package, minecart contraption and `UncontainableBlockItem`,
  Storage Drawers' drawers, Sophisticated Backpacks' backpack and Storage's shulker box, Refurbished
  Furniture's package). `ItemStack.isBarVisible`/`getBarWidth`/`getBarColor` were unmapped too, so a
  mod screen drew no durability bar, and the game's own bar ignored a mod item's: 75 classes in 25
  jars override `isBarVisible` (Create's sequenced assembly shows its progress there, its backtank
  its air; create-stuff-additions alone 18). Then the toolbox hands the equipped tool to the player
  from its block entity's tick (`ToolboxBlockEntity.tickPlayers`: `player.getInventory().getItem(slot)`,
  `setItem(slot, ...)`), where no player acts, and that tick's host had no player slots: the tool,
  taken out of its compartment, went nowhere. And it syncs the player's toolbox data with their
  client by the player's entity id (`PersistentDataPacket`), which a server's player object said was
  0 and a client couldn't find itself by (`Entity.getPersistentData()` on null).
  - compat `api_common`: `canFitInsideContainerItems` as NeoForge chains it:
    `ItemStack.canFitInsideContainerItems()` asks the item's `canFitInsideContainerItems(stack)`
    (`IItemExtension`), which asks vanilla's `canFitInsideContainerItems()`, each virtually (a
    mod's override of either answers); vanilla's is true but for a shulker box's block item (any
    colour, or a mod's `BlockItem` of a `ShulkerBoxBlock` subclass). `Mth.hsvToRgb` is shared as
    `hsv_to_argb`.
  - compat `components`: `ItemStack.isBarVisible`, `getBarWidth` and `getBarColor` ask the item
    virtually; NeoForge's `Item` defaults: shown while damaged, `Math.round(13 - damage * 13 /
    maxDamage)` pixels, `Mth.hsvToRgb(left / 3, 1, 1)` (green whole, yellow at half, red spent).
    A mod's override calling `super` gets them.
  - compat `jlang`: `Math.round(float)` and `(double)` as the JDK computes them, from the bits:
    adding a half rounded the float just below a half up, and 2^23 + 1 to 2^23 + 2.
  - translator: a client helper for the game's `native_item_bar` (API 60): for a mod's item whose
    class overrides any of the three, its own answers where it overrides them, the game's bar (the
    durability) for the rest (`glue::item_bar`); other items are never asked (the engine's
    `ours(id)` first, then the class).
  - Another player's inventory than the acting one's, by UUID. compat-core `Host`:
    `player_selected_of(uuid)`, `player_slot_of`, `set_player_slot_of` and `update_player_slot_of`
    (none by default: a host that can't reach that player); `MemoryHost` models them. compat
    `hand::for_player(uuid)` puts the slot calls on that player's inventory while a call of theirs
    runs: their records apart from the acting player's, flushed to them by UUID when the hook ends;
    their selected slot (`getSelected`, `add` tries it first). `menus::inventory_call` and
    `Player.getInventory` pass the inventory's player's UUID; where the host doesn't reach them,
    the acting player's, as before.
  - client: `native_player_selected_slot(uuid)`, `native_player_slot_stack(uuid, slot)` and
    `native_set_player_slot_stack(uuid, slot, stack)` (API @API:slot_stack@), through
    `native_as_player`, which now finds a guest on a server thread's game (no player of its own)
    when no guest acts. translator: the block entity tick's host and the player events' hosts
    (logged in, logged out, respawned) reach other players' slots through them.
  - A server's player object's `getId()` is the game's id for that player now, found by UUID (it
    was 0 until a hit told it); a client's level finds its own player by the id the server gave it
    (`ClientLevel.getEntity`), and `LocalPlayer.getId()` says it: compat-core
    `Host::client_player_id`, client `native_client_player_id()` (API @API:client_player@), told
    by the client hooks' host.
  - Tests: `api_common::tests::only_a_shulker_box_or_what_says_so_cannot_go_inside_an_item`,
    `components::tests::a_stacks_bar_is_its_items_own_else_its_durability`,
    `components::tests::the_game_draws_a_mod_items_own_bar`, `jlang::tests::math_round_is_the_jdks`,
    `menus::tests::another_players_inventory_is_reached_by_their_uuid` (nobody acting: a stack put
    whole, one shrunk in a hook, `add` onto their stack then their first empty slot, the host's
    hand untouched; the acting player's own object still theirs),
    `mod_entities::tests::a_client_finds_its_own_player_by_the_id_the_server_gave`. Compat 500,
    translator 19, gameplay 70 (`game-tests.sh`; the hook catalog test,
    `catalog_matches_hook_points`, checks the two fragments against the hook points).
  - In game (`toolboxgame.sh`, survival, a brown toolbox, an iron pickaxe worn to 125 of 250):
    shift-clicked on Create's own slot, the pickaxe went into the toolbox's first compartment, its
    yellow bar drawn on Create's screen; with dirt in the selected hotbar slot, the toolbelt key
    (left Alt) and `1` equipped it: the dirt went to slot 9 through Create's
    `ItemReturnInvWrapper`, and the block entity's next tick put the pickaxe (damage 125) in hotbar
    slot 0, the toolbox's items empty (its compartment keeps the pickaxe as its filter). The client
    had the player's toolbox data: Create's toolbelt frame drawn on that slot, and no
    `getPersistentData` on null. The game's bars, 7 of 13 pixels each: the pickaxe's yellow, and a
    half-full copper backtank's own light grey (`create:banktank_air` 450), in the hotbar and the
    inventory.
- [x] 2026-11-06 Part 74: which faces of a block show, as a mod asks (`faces.rs`, a module of the
  chain). A survey of Create's machines never checked in game before (`kinsurvey.sh`: two creative
  motors, a speedometer, a stressometer, a mechanical drill facing stone, a mechanical saw facing an
  oak log) found the drill breaking the stone (its cobblestone dropped), the saw cutting the log, and
  the gauges' network data right (speed 64, stress 256, capacity 1048576, the speedometer's dial at
  0.5957), but two calls unmapped. One was `Block.shouldRenderFace(state, level, pos, face,
  neighbourPos)`: Create's `GaugeBlock.shouldRenderHeadOnFace` asks it for each face its dial could
  go on, and the unmapped answer (false) left every gauge without its dial and needle. 8 jars call it
  (12 calls: Create's gauges, copycats, connected textures and table cloth, DnDesires' multimeter,
  Railways' copycat headstock, Copycats+, Sable's sub-level mesher, Fabric's renderer in txnilib);
  and the `BlockState` methods it is made of were unmapped too: `canOcclude` (28 jars, 44 calls),
  `isSolidRender` (24, 42), `skipRendering` (11, 20; 38 classes in 22 jars override it),
  `hidesNeighborFace` (5, 6), `supportsExternalFaceHiding` (5, 5), `getFaceOcclusionShape` (4, 6).
  - compat `faces`: `Block.shouldRenderFace` as NeoForge's: not where the block leaves its face
    out toward that neighbour (`skipRendering`), nor where the neighbour hides it
    (`hidesNeighborFace`) and the block lets it (`supportsExternalFaceHiding`); else wherever the
    neighbour can't occlude, the block's own occlusion face is empty, or part of it is outside the
    neighbour's face toward it (`Shapes.joinIsNotEmpty(own, theirs, ONLY_FIRST)`). Each piece is
    the state's own method, mapped: `canOcclude` (a mod block's properties: `noOcclusion`,
    `noCollission`, a copy of a vanilla block that can't; a vanilla block's as the game has it),
    `isSolidRender` (a vanilla block's from the game, a mod block's: it occludes and its occlusion
    shape is the whole block), `getFaceOcclusionShape` (the occlusion shape's face,
    `Shapes.getFaceShape`; a vanilla block that occludes hides faces as the game hides them with
    it), `skipRendering` (a mod block's own override; else its 1.21.1 class's rule, read off the
    merged jar: a `HalfTransparentBlock` (glass, ice, slime, honey) and powder snow toward the same
    block, bars and panes toward the same block above or below or where both join, a liquid toward
    the same fluid, mangrove roots above or below mangrove roots; a mod block's `super.` call to its
    vanilla class gets that class's), NeoForge's `hidesNeighborFace` (a mod block's own, else no)
    and `supportsExternalFaceHiding` (a mod block's own; else yes on a server, and on a client for a
    block outside the solid layer, which isn't known here: a block is taken as in it, as most are).
  - compat-core `Host::block_occlusion(state)` (can it occlude, is it solid to the renderer) and
    `Host::face_occlusion(state, direction)` (its occlusion shape's face, as the game culls with
    it); `MemoryHost` models both. client: `native_state_occlusion(text)` (the block facts'
    `CAN_OCCLUDE` and `SOLID_RENDER` flags, on the game and on a level) and
    `native_face_occlusion(text, direction)` (the face visibility tables the mesher culls with; none
    on a server, which loads none), API @API:state_occlusion@; translator: every host that tells
    block facts tells the first, the client's (its hooks' and its drawing's) the second.
  - Tests (`faces::tests`): a dial's face shows beside air, glass, a bottom slab's side (half of it
    is past the slab) and above a bottom slab, not against stone; a pane of glass leaves out its face
    toward the same pane, and stone or a solid mod block covers it; a copycat beside a block hides
    that block's face only where the block lets it; a block's own `skipRendering` is its answer.
    What states say of themselves: `canOcclude` and `isSolidRender` of mod blocks (a plain one, one
    with `noOcclusion`, a copy of glass) and vanilla ones, a slab's occlusion faces, the vanilla
    classes' `skipRendering` (glass, ice, bars joined or not and above, water beside water and lava,
    mangrove roots, stone) and a mod block's `super.skipRendering` to `HalfTransparentBlock`. Compat 502,
    translator 19, gameplay 70 (`game-tests.sh`; `catalog_matches_hook_points` checks the new
    fragment against the hook points).
  - In game (`kinsurvey3.sh`: a creative motor at 64 RPM driving a speedometer, a stressometer and
    a mechanical drill facing stone, both gauges facing north; the shots wait out the chat and aim
    with `look-at-block`): both gauges draw their dial and needle on their north face, the
    speedometer's needle at 0.5957 of its scale, the stressometer's at rest (256 of 1,048,576 su).
    With Create's goggles on, each gauge's readout: "Rotation Speed: Moderate (64 RPM)";
    "Network Stress: Low (0%)", "Remaining Capacity: 1,048,320su / 1,048,576su". The drill broke
    the stone (its cobblestone dropped), the saw cut the log. `Block.shouldRenderFace` and the
    state methods are gone from the unmapped calls; `Font.drawInBatch8xOutline` is still there
    (next). With the goggles on, the creative motor seemed to show no overlay: the crosshair was on
    one of its value box's slots (its north face), and Create leaves the overlay out while a value
    box is highlighted (`GoggleOverlayRenderer`). Aimed at its back, it shows its generator stats
    ("Kinetic Stress Capacity: 1,048,576su at current speed"; rechecked in Part 75).
- [x] 2026-11-06 Part 75: text mods draw in the world. `Font.drawInBatch` with a matrix was drawn on
  the GUI only: text a block entity's renderer or a level stage drew in the world was measured (its
  end answered, as vanilla's) and dropped. 37 jars call it (68 calls): Create 9 (its value boxes'
  numbers, its nixie tubes, its block entities' name plates), FramedBlocks, Applied Energistics 2
  and MineColonies 4 each, JourneyMap and PolyLib 3, and 31 more. `drawInBatch8xOutline` (text over
  its outline, as a glowing sign's) was unmapped: 7 jars, 8 calls (Create 2, Farmer's Delight,
  FramedBlocks, Ars Nouveau, Create Aeronautics, Moonlight, Refurbished Furniture).
  - compat `gui`: outside the GUI, `drawInBatch` gives the host its text (`Host::world_text`): its
    runs, its pose, where it starts, its colour, the drop shadow and its light, and still answers
    vanilla's end. `drawInBatch8xOutline` on the GUI draws the text eight times a pixel around in
    the outline's colour (the runs' own colours dropped, as vanilla's `withColor` drops them), then
    the text over them; in the world, one text with its outline. `Font.getFontSet` (3 jars, 6
    calls), `FontSet.whiteGlyph` (2, 4) and `BakedGlyph.renderType` (3, 5), which Create's nixie
    tubes end their text's batch with after each digit: a font set for each font, a white glyph,
    the text render type of each display mode, so that ending it ends the text's batch alone
    (unmapped, the type was null and ended every batch, a face another type was gathering too).
    `BakedGlyph` leaves the vanilla records' data classes, modelled by hand now.
  - compat-core: `WorldText` (runs, a column-major pose from font pixels, the start, `0xAARRGGBB`,
    shadow, outline, `LightTexture.pack` light) and `Host::world_text` (false by default: no text
    drawn there); `MemoryHost` keeps them in order.
  - client: `native_draw_text(draw, pieces, at, shadow, outline)` (API @API:block_entity_draw_text@)
    draws `TextPiece`s (text, `0xAARRGGBB`, bold) in a row under the draw's pose in the game's font,
    lit by the draw's light, with the sign renderer's glyph quads (`text_vertices` of
    `block_entity_render`, shared now): vanilla's drop shadow (a pixel right and down, a quarter as
    bright, the text 0.03 font pixels in front, `Font.SHADOW_OFFSET`), bold (each glyph again a
    pixel to its right, a pixel more advance), the outline (eight copies, the text in front of them
    toward the side it reads from, where vanilla's polygon offset puts it); an alpha below 4 is
    opaque (`Font.adjustColor`), text fainter than a tenth isn't drawn (vanilla's text shader
    discards it). It answers where the text ends. Text shows only from the side it reads from, as
    vanilla's text render types cull back faces: a glyph's quad turns counterclockwise seen from
    that side (`BakedGlyph.render`), and the frame knows where the camera is (a block entity's from
    its block's corner, a level stage's and a mob's at the origin of their camera-relative poses);
    a block entity's frame that draws text isn't kept (`keep`), as what shows depends on the camera.
  - translator: the drawing host (block entities' renderers, mod entities and the level stages)
    answers `world_text`: the runs, each in its own colour or the text's with the text's alpha,
    under its pose, lit by its light (0: the block's own).
  - Not drawn yet: `drawInBatch`'s background box (a name plate's dark box), underline and
    strikethrough, italic's slant, `SEE_THROUGH` text (a name plate seen through walls), and alpha
    (text draws opaque).
  - Tests: `gui::tests::text_a_renderer_draws_goes_to_the_world_and_outlined_text_over_its_outline`
    (a string with a shadow and an outlined sequence outside the GUI reach the host as they were
    drawn, the sequence's run in its own colour, nothing on the GUI; on the GUI, the outlined one
    is eight copies in the outline's colour around it, then the text),
    `vertices::tests::ending_the_text_batch_leaves_the_other_types_faces` (the nixie tube's
    `endBatch` of the white glyph's type leaves a face gathered for another type until that one's
    batch ends), in `native_block_entity_draw::tests`
    `text_lands_in_its_glyphs_with_vanillas_shadow_bold_and_outline` (pieces in a row under the
    pose, a blank space, colours in vertex order, the light; the shadow's place, colour and the
    text in front; bold's doubled glyphs; the outline's eight copies behind the text, toward the
    camera; too faint a text drawn not at all but taking its room) and
    `text_shows_only_from_the_side_it_reads_from` (a sign's text and its shadow from in front, none
    from behind though it takes its room; a nixie tube's digit and its mirror image for the back,
    each seen only from its own side; an outline's text in front of it under a sign's pose).
    Compat 504, translator 19, gameplay 70 (`game-tests.sh`; `catalog_matches_hook_points` checks
    the new fragment), `native_block_entity_draw` 9, `block_entity_render` 7.
  - In game (`p75round.sh`: a creative motor at 64 RPM driving a mechanical drill, and a nixie tube
    facing north beside a redstone block): the motor's value box shows "64" over its outline on the
    slot the crosshair is on, Create's tip under it ("Generated Speed in RPM", "Click and hold to
    edit"). The nixie tube's two digits read "0" from its west side, each over its shadow, and the
    mirrored copies for the other side are left out (before, both showed through each other, the
    digits unreadable); from north and south, edge-on, nothing shows. With the goggles on, the
    drill shows its kinetic stats ("Kinetic Stress Impact: 256su at current speed") and the motor,
    aimed at its back, its generator stats. `Font.getFontSet`, `whiteGlyph` and `renderType` are
    gone from the unmapped calls; `Level.getSignal(BlockPos, Direction)` is still there, so the
    tube shows 0 beside the redstone block, not 15 (next).
- [x] 2026-11-07 Part 76: redstone reaches mods' blocks: the signal a block sends one way, and a
  mod's block hearing its neighbours change. Only the neighbour queries were mapped
  (`hasNeighborSignal`, `getBestNeighborSignal`); `Level.getSignal(BlockPos, Direction)` was not:
  14 jars, 29 calls (Create 5: a nixie tube shows the strongest signal around it,
  `NixieTubeBlock.getPower`, as its redstone link does; Create Aeronautics 4, Create Propulsion 4,
  Aeroworks 3, Tilting Control 2, Create Big Cannons 2 and 3 more through `ServerLevel`,
  Electroenergetics 2, and ExtendedAE, Jade, Mekanism, SecurityCraft and Ars Nouveau 1 each), nor
  `hasSignal` (7 jars, 8 calls), `getDirectSignal` (3, 3) or `getDirectSignalTo` (2, 2: Create 1).
  Mapped, the tube still showed 0 beside a redstone block: it reads the signal when it hears a
  neighbour change (`neighborChanged` schedules its tick), and no mod's block ever heard one. The
  game had only an observation hook (`native_neighbor_changed`, which can't change the level), and
  bundles wired none: `glue::neighbor_changed` was never called.
  - world: `Behaviours::signal(level, ticks, pos, direction, direct)`, the signal the block at
    `pos` sends back along `direction` (the redstone simulation's `signal`, which takes a
    conductor's strongest strong signal in, or `direct_signal`). Neighbour updates carry where
    they come from, as 1.21.1's `neighborChanged(state, level, pos, changed, from, moved)` takes
    it and 26.3 dropped it: the block that changed for the six `updateNeighborsAt` tells, the
    diode or observer for the block in front of it, the comparator's source, the detector rail
    for its connections, the cactus for the block above it, the block itself for a piston's or
    melting ice's own update, and a piston head passes its own on to its base.
    `LevelAccess::before_neighbor_changed(ticks, pos, state, changed, from, moved, handled)` runs
    after the observers, in the level's order of updates; the level may change blocks and
    schedule ticks there, and `handled` skips the block's own behaviour.
  - compat-core: `Host::signal(pos, direction, direct)`, `direction` as `Direction`'s data value,
    the way from the block asking to `pos` (0 by default); `MemoryHost.sent_signals` answers it,
    0 on a client.
  - compat `level_env`: `getSignal`, `hasSignal` and `getDirectSignal` ask the host;
    `getDirectSignalTo` is the strongest strong signal the six neighbours send the block, in
    vanilla's order (down, up, north, south, west, east), done at 15 (`SignalGetter`).
  - client: `native_signal(dimension, pos, direction, direct)` and
    `GameLevel::native_signal_here(ticks, pos, direction, direct)` for level hooks (API
    @API:native_signal@): the server's world's answer, 0 on a client and for another dimension.
    A new hook, `GameLevel::native_block_neighbor_changed(ticks, dimension, pos, state, changed,
    from, moved, handled)` (API @API:block_neighbor_changed@): a mod's block (outside
    `minecraft:`) hears a neighbour change on the server where it may change the level. And
    `GameLevel::native_block_entity_sync_here(pos)` (API @API:native_block_entity_sync_here@):
    from a level hook, players are sent the block and its block entity's update tag again, with
    the level's other block changes.
  - translator: both hosts answer `signal` (level hooks through `native_signal_here`, other code
    through `native_signal` in the player's dimension); bundles inject `block_neighbor_changed`,
    which runs the block's own `neighborChanged` (`glue::neighbor_changed`, with `fromPos`) and
    marks it handled, for blocks whose classes override it. A level hook's host passes
    `block_changed` on (`native_block_entity_sync_here`): a block entity's data changed in a
    scheduled tick (the tube's `sendBlockUpdated`) went nowhere before, so the server held 15 while
    players saw 0.
  - Tests: `level_env::tests::what_a_block_sends_one_way_comes_from_the_world` (a redstone block's
    weak signal one way and nothing the other ways, its strong signal 0; a lever's strong signal
    reaching the block under it through `getDirectSignalTo`; `hasSignal`; all 0 on a client; no
    unmapped call), `level::tests::neighbor_changed_hook_hears_where_updates_come_from_and_can_handle_them`
    (a redstone block placed beside a lamp: the six around it hear it from it, west first; the
    stone the hook places above the lamp tells its own six from where it is before the rest of
    the first six; the handled lamp stays dark though powered, and lights when left to itself).
    Compat 505, translator 19, world 295, gameplay 70 (`catalog_matches_hook_points` checks the
    new fragments).
  - In game (`p76round.sh`): the nixie tube beside a redstone block holds `RedstoneStrength` 15,
    the one beside the last of three redstone dust from another redstone block 13 (the dust 15,
    14 and 13, as vanilla's). Players see the digits: from the west, where Create draws the tubes'
    back (each digit readable, the pair in reverse order), "5 1" and "3 1". Before, 0 on the
    server, and once the server had 15, "0 0" on the client. No throws; `Level.getSignal` is gone
    from the unmapped calls.
- [x] 2026-11-07 Part 77: a mod's block answers redstone by its own code, and the neighbour
  updates a mod asks for reach the level. The game asked a mod's block only its template
  (`minecraft:stone` for all of Create's redstone blocks), which sends nothing: its own
  `getSignal` (73 classes in 27 jars; Create 13 and 11 in its two jars, SecurityCraft 16),
  `getDirectSignal` (42 in 17), `isSignalSource` (64 in 26), `hasAnalogOutputSignal` (143 in 45)
  and `getAnalogOutputSignal` (153 in 47) never ran, though the catalog had their hook points. And
  the level's neighbour updates did nothing: `updateNeighborsAt` (110 classes in 33 jars),
  `updateNeighbourForOutputSignal` (92 in 42), `neighborChanged(pos, block, from)` (48 in 28),
  `blockUpdated` (25 in 17), `updateNeighborsAtExceptFromFacing` (4 in 4), and a block entity's
  `setChanged`, which in vanilla tells the comparators beside it (273 classes in 88 jars call a
  `setChanged`). A redstone link's receiver, an analog lever or a contact changing its output told
  no one.
  - compat `glue`: `is_signal_source(state)`, `signal(pos, state, side, direct)` (`side` as
    `Direction`'s data value, the way from the block asking), `has_analog_output(state)` and
    `analog_output(pos, state)`: the block's own `BlockBehaviour` method where its class has one,
    with the level and position, a signal clamped to 0 to 15. None where it has none (its
    template's answer stands), for another mod's or vanilla's block, and when its code throws (the
    throw is reported, not left pending).
  - compat-core: `NeighborUpdate` (`Around { pos, skip }`: `updateNeighborsAt`, `blockUpdated`,
    `updateNeighborsAtExceptFromFacing`; `One { pos, from }`: `neighborChanged`;
    `Output { pos }`: `updateNeighbourForOutputSignal`) and `Host::update_neighbors(update,
    block)`; `MemoryHost.neighbor_updates` keeps them, none on a client.
  - compat `api`: the five level calls go to the host. `BlockEntity.setChanged()` tells the
    comparators beside the block entity (`Output`) once it has a level and a position, unless
    its block is air (vanilla's `setChanged`). These and the level's signal reads (`getSignal`,
    `hasSignal`, `getDirectSignal`, `getDirectSignalTo`, `hasNeighborSignal`,
    `getBestNeighborSignal`) are the game's calls (`host_call`): the game may run the mod's own
    code inside them, a nixie tube's `neighborChanged` for an analog lever's update, the lever's
    `getSignal` for the tube's read.
  - compat `functions`: Guava's `Predicate.test(t)` is its `apply(t)`; a lambda answered only
    its functional method's name.
  - world: `Behaviours::update_neighbors_at_except(level, ticks, pos, block, skip)` and
    `neighbor_changed_at(level, ticks, pos, block, from)`, through the level's collecting
    updater, as the other updates.
  - client: `GameLevel::native_update_neighbors_here(ticks, pos, block, skip)`,
    `native_neighbor_changed_here(ticks, pos, block, from)` and
    `native_update_output_neighbors_here(ticks, pos)` for level hooks, and
    `Game::native_update_neighbors(dimension, pos, block, skip)` and
    `native_neighbor_changed(dimension, pos, block, from)` for other code (API
    @API:native_neighbor_updates@): the server's level runs them, nothing on a client or in
    another dimension.
  - translator: bundles wire the catalog's `block_signal`, `is_signal_source`,
    `has_analog_output` and `analog_output` to the four glue functions for their own blocks, and
    both hosts pass `update_neighbors` on (level hooks to the `_here` helpers, other code to the
    player's dimension).
  - Tests: `glue::tests::a_mod_blocks_own_redstone_answers_reach_the_world` (a latch sends 15
    only east while powering and has no strong signal of its own; an analog output of 7 over
    gold and 99 clamped to 15; a block whose code throws, a block without these methods and
    vanilla's give None), `api::tests::a_mods_neighbour_updates_reach_the_level` (each call as
    its update, a block entity's `setChanged` only once it has a level, none on a client),
    `api::tests::the_game_reaches_the_mods_own_code_inside_its_updates_and_signal_reads` (a host
    whose answers need the runtime gets it inside each update and read), Guava `test` in
    `functions::tests::predicates_combine_as_in_java`. Compat 508, translator 19, gameplay 70.
  - In game (`redsurvey2.sh`): the comparator behind a speedometer at 64 RPM (dial 0.596) gives
    the dust 9; the smart observer on a chest holding one stack powers and lights the lamp beside
    it; the two contacts facing each other hold `powered=true`. Before, nothing of these reached
    the world. Still dark: the contacts' lamps and the nixie tube by the analog lever (the lever
    at 6 after 7 clicks): the log said "the game ran the block_neighbor_changed hook from inside
    one of the mod's own calls; skipped". The updates went to the host without the game being able
    to run the mod's hooks inside them (`host_mut`): now `host_call` (unit-tested above; in game
    with Part 78). The latches stay off: their neighbour handling, placement and strong signal are
    vanilla `DiodeBlock`'s (Part 78). The threshold switch reads `CurrentAmount` -1: its inventory
    filter wants the chest's block entity, and `level.getBlockEntity` gives null for a vanilla
    block's. The redstone link's receiver holds `Receive` 0 (its network). No throws.
- [x] 2026-11-07 Part 78: a mod's diode is vanilla's `DiodeBlock`. 8 classes in 4 of the pack's jars
  extend it (Create's toggle and powered latches and brass diodes, Aeronautics' accumulator and
  inductor, Create Connected's sequenced pulse generator, Create Propulsion's redstone converter);
  none has its own `neighborChanged`, `onPlace`, `onRemove`, `getDirectSignal`,
  `getStateForPlacement` or `canSurvive`, 5 have their own `tick` (most calling `super.tick`), 4
  their own `checkTickOnNeighbor`. With only their template answering, a latch never heard its
  input, never scheduled its change, and told no one when it changed. Mods also ask the level's
  tick queue: `willTickThisTick` (14 classes in 4 jars), `hasScheduledTick` (36 in 12), a
  `scheduleTick` with a `TickPriority` (25 in 12), `getControlInputSignal` (2 in 2).
  - compat `diodes` (new, first in the chain): `DiodeBlock`'s 21 methods as NeoForge 1.21.1 has
    them, each calling the block's own overrides as the JVM dispatches (`tick`, `getSignal` forward
    only while powered, `getDirectSignal` = the block's own `getSignal`, `neighborChanged` (drops
    without a rigid floor, else `checkTickOnNeighbor`), `checkTickOnNeighbor` (no second tick
    while one is about to run; priority high, very high while on, extremely high feeding a diode),
    `shouldTurnOn`, `getInputSignal` (a wire's power counts), `getAlternateSignal`, `isLocked`,
    `isSignalSource`, `getStateForPlacement` (input on the placer's side), `setPlacedBy`,
    `onPlace`/`onRemove` (`updateNeighborsInFront`: the block in front, then its neighbours but
    the diode), `shouldPrioritize`, `isDiode`, `canSurvive`/`canSurviveOn`, `getOutputSignal` 15).
    `inherited(rt, class, method)`: the `DiodeBlock` key the game's hooks fall back to for a block
    whose first vanilla class is `DiodeBlock` and that has no such method of its own.
  - compat `glue`: `hook_key` picks the block's own method, else the inherited one, for the
    scheduled tick, the neighbour change, `onPlace`/`onRemove`, the signal answers,
    `getStateForPlacement` and `setPlacedBy`; `blocks::survives_by_code` and `survival_blocks`
    take a diode's `canSurvive`.
  - compat `level_env`: `scheduleTick(pos, block, delay, priority)` keeps its priority;
    `getBlockTicks().hasScheduledTick` and `willTickThisTick` ask the host (before: always false);
    `getControlInputSignal(pos, direction, diodesOnly)` (a redstone block's 15, a wire's power, a
    source's strong signal; only a diode's where `diodesOnly`).
  - compat-core: `Host::schedule_block_tick_at_priority(pos, block, delay, priority)` (default:
    without one) and `Host::block_tick_queued(pos, block, this_tick)`; `MemoryHost` keeps the
    priorities and the ticks running now.
  - client: `GameLevel::native_schedule_prioritized_block_tick` and `native_block_tick_queued`,
    `Game::native_schedule_prioritized_block_tick_at` and `native_block_tick_queued_at` (API
    @API:native_block_tick_queue@); the bundles' three hosts pass them on.
  - Tests (`diodes::tests`, 8): a mod diode on and off a delay after its input at vanilla's
    priorities, forward only, strong as weak, a pulse passing as one as long, a wire's power as
    input; no second tick while one runs; the highest priority feeding a diode; the block in
    front and its neighbours hear each change and the removal, none when moved; it drops off a
    floor that isn't rigid; a toggle latch's own `tick` runs vanilla's with its own delay (on,
    stays, off); a powered latch's own `checkTickOnNeighbor` and a lock by side inputs (redstone
    block, wire, torch); placement facing and `setPlacedBy`'s tick. `level_env`'s tick test now
    expects the queued tick. Compat 516, translator 19, gameplay 70.
  - In game (`latchsurvey.sh`: Part 77's rows, then new ones): the powered latch and the toggle
    latch with a redstone block behind them hold `powered` and `powering` and light their lamps
    (before: dark). A toggle latch fed on, off, on and off again goes powering, stays powering,
    stops, stays stopped, its lamp with it. A pulse extender lights its lamp while fed and stops
    within 4 ticks of its input going (its delay of 2). A toggle latch the player places standing
    west of the spot faces west (its input on the player's side; the script waits a few ticks after
    turning the player, as the server reads the look a tick later). Part 77's `host_call` in game:
    the nixie tube by the analog lever shows the lever's 6, and both contacts' lamps light (placed
    before the contacts: a lamp `/setblock` puts beside a block already sending stays dark until a
    neighbour changes, as in vanilla; placed after, one stayed dark). Left: a latch whose floor goes
    is removed but drops nothing, as its `dropResources` runs in a level hook (the neighbour
    change) whose host spawns no item entity (the bundles' level host has no `spawn_item_entity`;
    next). The threshold switch still reads -1 (no block entity object for a vanilla chest), the
    link's receiver 0. Unmapped: two reads of a pulse extender's `BlockEntity.level` where the
    runtime had none set. No throws.
- [x] 2026-11-07 Part 79: what a mod's code drops inside a level hook lands, and a vanilla
  container's block entity is there for it. Measured on the pack (`p79scan/drops.py`, per method,
  direct calls): 58 level-hook methods (a block's `tick`, `randomTick`, `neighborChanged` or
  `performBonemeal`) in 58 classes of 20 jars drop items themselves: `destroyBlock` 40,
  `addFreshEntity` 12, `dropResources` 5, `popResource` 4 (Farmer's Delight's tomatoes, Create's
  nozzle, hand crank, redstone link and belt tunnel, SecurityCraft's mines, Aeronautics' handle and
  nameplate). These hooks run on the level (`GameLevel`), whose bundle host had no
  `spawn_item_entity`, no `block_drops` and no `pop_experience`: the block went, its drops never
  came (a Create latch losing its floor, Part 78). And 2 of Create's inventory behaviours take an
  inventory only where `Level.getBlockEntity` finds a block entity (the threshold switch's
  `isSuitableInventory`, the packager's); 32 methods in 30 classes of 15 jars take a block entity
  as a `Container`. Vanilla blocks had none: the threshold switch read -1 by any chest.
  - client: `GameLevel::native_spawn_item_entity_here(stack, pos, motion, delay)`: the item entity
    gets its ID at once from the game's counter (`GameLevel` now holds it) and joins with it when the
    level operation ends (the spill's spawns carry an optional ID; `Game::spawn_item_as`).
    `native_block_loot_here(pos, state, tool)`: `Block.getDrops` rolled in the level (its random
    sequence, what the block entity copies into the stacks; `GameLevel::roll_block_drops`, which
    `drop_resources_now` now shares). `native_pop_experience_here(pos, amount)`. The level has the
    game's item component helpers too (`native_component`, `native_set_component`,
    `native_vanilla_components`... as on `Game`, one implementation over the item registry), so a
    stack crosses with its mod data. API @API:level_item_entities@.
  - translator: the bundles' level host answers `block_drops`, `spawn_item_entity`,
    `spawn_stack_entity` and `pop_experience`; its stacks cross by the same helpers as the game's
    hosts (`stack_helpers_on`, parametrized by host type and receiver).
  - compat `item_wrappers`: `Level.getBlockEntity` at a vanilla container the game holds (chest,
    trapped and copper chests, barrel, dispenser, dropper, hopper, crafter, furnaces, brewing stand,
    shulker boxes, chiseled bookshelf, jukebox) gives its block entity, of vanilla's class, one object
    per position and block, with its type, position, state and level; its `Container` methods (by
    the interface or by its own class) are the container's through the host. 1.21.1's container
    block entity classes are `Container`s, the furnaces', brewing stand's and shulker box's
    `WorldlyContainer`s (a mod's subclass of one with them). `glue::block_entity_at` gives it for
    a `minecraft:` block.
  - Tests: compat `item_wrappers::tests::a_vanilla_containers_block_entity_is_its_container` (a
    chest's block entity is vanilla's class and the same object again, with the chest's position
    and state; it is a `Container` of 27 slots holding the chest's iron, by the interface and by
    its own class, and not a `WorldlyContainer`; a furnace's is one, of 3 slots; stone and a chest
    without a container have none). The gameplay run now includes `spill::tests` and
    `native_components`. Compat 517, translator 19, gameplay 72.
  - In game (`p79survey.sh`): four stockpile switches, each with a container east of it and a lamp
    west. A chest of 27 stacks of cobblestone, one of 14 stacks, a barrel of 16 logs and an empty
    chest read 1728, 896, 16 and 0 items and show levels 5, 3, 1 and 0 (before: -1 by any chest).
    The two over the switch's default 128 items light their lamps; the two under 64 stay dark.
    Emptying the full chest (`/data merge`, `Items:[]`) takes its switch to 0 and its lamp off. A
    smart observer on a chest of logs is powered and lights its lamp. A powered latch on a stone
    over air: the stone goes, the latch is removed and drops one `create:powered_latch` item entity
    by its position (before: nothing). Unmapped: the 16 client-side GL and texture calls of the
    earlier surveys (LWJGL's shader compile, `DynamicTexture`), none from a level hook. No throws.
- [x] 2026-11-07 Part 80: a block entity has no level until it joins one, as Java's field starts;
  and Create's redstone links checked the ways a player makes a receiver. Measured on the pack
  (`p80scan/levelreads.py`, per method, direct `getfield`): 3112 methods in 874 classes of 108 jars
  read a block entity's `level` field; 75 of them in 70 classes of 32 jars are load methods
  (`loadAdditional`, `read`, `applyImplicitComponents`...), which vanilla runs before the block
  entity joins its level (a chunk's load), where the field is null. None reads it in a constructor
  itself, but Create's do through calls: a `ScrollValueBehaviour` taking its default in
  `addBehaviours` (the constructor's) calls back `sendData`, `level != null && !level.isClientSide`.
  The runtime had no `level` at all until `setLevel`: the read went to the API as an unmapped call
  (`BlockEntity.$level`, twice per pulse extender placed, Part 78) and answered nothing.
  - compat: `BlockEntity.<init>` sets `level` null and `remove` false, as the JVM's fields start;
    `setLevel` and the game's hooks set the level as before.
  - Redstone links: Create makes a link's `LinkBehaviour` only on its first data read
    (`addBehavioursDeferred`) or when its block's mode differs from the one it keeps
    (`RedstoneLinkBlockEntity.tick`); a link is placed a transmitter (`receiver=false` is its
    default state) and toggled to a receiver by the wrench (`toggleMode`: `setBlock` with `receiver`
    cycled). Part 78's receiver was `/setblock` as a receiver without data: in Create too it never
    gets a link, and reads 0.
  - Tests: compat `api_common::tests::a_new_block_entity_has_no_level_until_it_joins_one` (what
    the generated code reads for `this.level` and `this.remove` after `BlockEntity.<init>`: null
    and false, `hasLevel` false; after `setLevel`, that level), no unmapped call. Compat 518,
    translator 19.
  - In game (`p80survey.sh`: a transmitter link beside which a redstone block is placed, four links
    east of it, a lamp east of each): R1 placed a transmitter then `/setblock` a receiver (the same
    block, the block entity kept), R2 placed a receiver with `{Transmitter:0b}`, R3 placed a
    transmitter then wrenched by the player (`use-item` with Create's wrench: `onWrenched`,
    `toggleMode`) are receivers; with the transmitter powered (`Transmit` 15) each holds `Receive`
    15, `powered`, and lights its lamp, and all three go dark when the redstone block goes. R0, a
    receiver placed without data, holds `Receive` 0 and no frequency data, as in Create. A pulse
    extender placed: `ScrollValue` 2, `State` 0, and no unmapped call (Part 78 had two). No
    throws; unmapped: only the 16 client GL and texture calls of the earlier surveys.
- [x] 2026-11-07 Part 81: what every class takes from `Object`, and what Create's moving
  contraptions read on their way. A kinetics survey (`kinsurvey4.sh`: a mechanical piston with two
  extension poles, two rope pulleys, two encased chain drives) found the chain drives passing 64
  RPM sideways to a speedometer (0.5957 of its dial, as before), but the piston and the pulleys
  still: `Running` 0, `Offset` 0, the planks where they were. A pulley assembled its contraption (a
  `ControlledContraptionEntity`, drawn by its renderer) and took it apart the same tick
  (`contraption_disassemble`). Create's `ContraptionCollider.collideBlocks` looks for the other
  contraptions in the way by `e -> !e.equals(self)`, and `equals` through
  `ControlledContraptionEntity` was unmapped: it answered false, so the contraption found itself in
  its own way and stopped. In Java the call resolves to `Object.equals`, identity: no class below
  `Object` declares it. Measured on the pack (`p80scan2/objmethods.py`, per call, through a class
  that doesn't declare the method, into a game class): `equals` 2035 calls in 1442 classes of 181
  jars, `hashCode` 180 in 154 classes of 61 jars, `toString` 2104 in 1437 classes of 198 jars; most
  reach classes the runtime models (`BlockPos` 873, `ResourceLocation` 484, `Vec3` 67), 67 reach
  `Entity` and `LivingEntity`, which declare none. With them, four more calls on the contraption's
  path were unmapped: `Entity.noPhysics` read as a field (the mod entity had no such field; 17
  classes in 6 jars read it), `MutablePair.left` of a `new MutablePair<>()` (its halves were never
  set), `StateHolder.owner` read as a field, and the virtual world Create builds for a contraption's
  blocks (`Level.getProfilerSupplier`, a `LevelLightEngine` and its `runLightUpdates`).
  - compat `api`: a call missed on its class and every class and interface above it, of
    `equals(Object)`, `hashCode()` or `toString()`, is `Object`'s (identity, or the runtime's own
    values' content), as the JVM resolves it; no longer counted unmapped.
  - compat: a mod's entity starts with `Entity`'s fields as Java does (`noPhysics`, `hurtMarked`,
    `hasImpulse`, `noCulling`, `horizontalCollision`, `verticalCollision` false,
    `invulnerableTime` 0, `fallDistance` 0); `new MutablePair<>()` holds null and null;
    `StateHolder.owner` of a block state is its block; `Level.getProfilerSupplier()` gives
    vanilla's inactive profiler; a mod level's `LevelLightEngine` keeps nothing (light is the
    game's, read where the blocks are): `runLightUpdates` and `hasLightWork` 0, `getRawBrightness`
    the game's, `getLayerListener` vanilla's dummy listener.
  - Tests: compat 523 (5 new: `equals` and `hashCode` through a class without its own are
    `Object`'s; a mod's virtual level gets a profiler and a light engine that keeps nothing; a
    state's `owner` is its block; `new MutablePair<>()` holds nulls; a mod's entity starts with
    `Entity`'s fields), translator 19.
  - In game (`kinsurvey5.sh`: two mechanical pistons facing east, two extension poles behind each
    and planks in front, two rope pulleys with planks right under them, each driven by a creative
    motor at 64 RPM, one each way): the piston at −64 RPM extended, `Offset` 2, its planks moved
    from +1 to +3 and the piston `extended`; the one at +64 RPM stayed retracted with its planks at
    +1, as Create's do. The pulley at +64 RPM lowered its planks 3 blocks to the floor (`Offset` 3,
    rope above them, the magnet below the rope); the one at −64 RPM kept them where they were. Nothing threw,
    nothing was unmapped. Not yet right: no piston head at +2, where Create puts it (Part 82).
  - All of Create's blocks (`allblocks.sh`, 643 from its blockstates, one `/setblock` each, then
    100 ticks, a read-back and `/setblock ... air destroy`): 639 stand. A lone `belt` is destroyed
    by Create's own `initBelt` (a belt is a chain), `factory_gauge` and `gantry_carriage` can't
    survive with nothing to hang on, and `fake_track` was gone before the read-back (Part 82
    measures why). 211 hold block-entity data, the destroyed blocks dropped 607 items, nothing
    threw, and 15 calls in 10 blocks were unmapped: `BlockState.handleNeighborChanged` (andesite
    door), `ServerChunkCache.getGenerator` (belt), `StringDecomposer.iterateFormatted` (display
    board), `getTicker` and `saveAdditional` by their short keys (fake track, mechanical
    harvester), fastutil's `ObjectHeapPriorityQueue` (hose pulley), the lectern controller's item
    model (`BakedModelWrapper.getOriginalModel`, `ItemStack.hasFoil`, `BakedModel.getRenderPasses`),
    `ChunkSource.hasChunk` (mechanical arm), Guava's `Objects.equal` and `Collectors.summingInt`
    (track, train door).
- [x] 2026-11-07 Part 82: the calls Part 81's survey of Create's blocks left unmapped, each made
  general: Java's collectors and priority queues, text read a character at a time, a level's chunk
  generator, an item's shine and its model's render passes. Counted on the pack's 367 jars and the
  jars inside them (`p82scan/refs.py`: every member reference, with the jars that make it): 22
  `Collectors` methods (`toList` in 101 jars), `new PriorityQueue(comparator)` in 13 jars and
  fastutil's heap queues in 3, `ServerChunkCache.getGenerator` 26, `ItemStack.hasFoil` 29,
  `BakedModel.getRenderPasses` and `getRenderTypes(ItemStack, boolean)` 14 each,
  `FormattedCharSequence.accept` 11, `Component.visit` 8, Guava's `Objects.equal` 12,
  `BlockState.handleNeighborChanged` 6, `StringDecomposer.iterateFormatted` 4.
  - compat `collectors` (new): `Collectors` whole, as the JDK's: `toList`/`toSet`/unmodifiable
    forms, `joining`, `toMap`/`toUnmodifiableMap`/`toConcurrentMap` (a key twice throws
    `IllegalStateException` with Java's message unless a merge is given; with a map factory),
    `toCollection`, `groupingBy`/`groupingByConcurrent` with any downstream and map factory,
    `partitioningBy` (`{false=…, true=…}`), `collectingAndThen`, `mapping`, `filtering`,
    `flatMapping`, `counting`, `summingInt/Long/Double` (doubles summed with Java's compensation),
    `averaging*`, `minBy`/`maxBy`, `reducing`, `Collector.of`, Guava's `toImmutableList/Set/Map/
    BiMap`; a mod's own `Collector` runs through its supplier, accumulator and finisher; the
    three-argument `collect(supplier, accumulator, combiner)` of object and primitive streams.
  - compat `heaps` (new): `java.util.PriorityQueue`, `PriorityBlockingQueue` and fastutil's
    `*HeapPriorityQueue` are binary heaps in the JDK's (or fastutil's) sift order, by their
    comparator or natural order: `poll`/`peek`/`remove(o)`/`removeIf` and iteration in the heap's
    array order, as Java's. Before, a priority queue was a list in insertion order.
  - compat `char_sinks` (new): `StringDecomposer.iterate`, `iterateBackwards` and
    `iterateFormatted` (its four forms: `§` codes become styles, `§r` back to the default, a code
    point at the index of its first UTF-16 unit, a broken pair as U+FFFD, the sink's false stops the
    walk), `getPlainText`, `filterBrokenSurrogates`; `FormattedCharSequence.accept(sink)` and
    `Component`/`FormattedText.visit(consumer[, style])` on the runtime's texts.
  - compat: `level.getChunkSource().getGenerator()` is a `NoiseBasedChunkGenerator` (a
    `ChunkGenerator`, not the debug world's `DebugLevelSource`) with its dimension's sea level,
    lowest y and depth from vanilla's noise settings; `ChunkSource.hasChunk` is true;
    `BlockState.handleNeighborChanged` runs the level's neighbour update; `ItemStack.hasFoil` is the
    glint override, else the item's own `isFoil` (a mod's override first), else enchanted or a
    lodestone compass; `BakedModel.getRenderPasses(stack, fabulous)` is the model itself and
    `getRenderTypes(stack, fabulous)` NeoForge's fallback item type; `EntityBlock.getTicker` and
    `getListener` default to none, and a block or block entity without its own `getTicker`,
    `loadAdditional` or `saveAdditional` falls back by the full key (the short one never resolved).
  - compat core: `String.valueOf` of a list, set, map or map entry prints it as Java does (`[a, b]`,
    `{k=v}`, `k=v`, "(this Collection)" for one holding itself), a number a collection holds unboxed
    prints as its box (it printed "null"), a `Boolean` box `true`/`false`.
  - Tests: compat 538 (15 new: a priority queue polls least first in Java's heap order, follows
    its comparator, is heapified from a collection, and fastutil's dequeues its least;
    `groupingBy` with its downstreams, `toCollection`/`partitioningBy`/`collectingAndThen`, `toMap`
    refusing a key twice; `iterateFormatted` styles by `§` codes and stops when told,
    `iterateBackwards`, `visit` until the consumer answers; a stack's shine; a block entity
    without its own ticker or data ticks on vanilla's; a state's neighbour change reaches the
    level; the level's generator; an item model's passes and fallback type), translator 19, core 7.
  - In game (`allblocks.sh` again, all 643 of Create's blocks): nothing threw, and of Part 81's 15
    unmapped calls only the lectern controller's `getOriginalModel` is left, now followed by
    `ItemRenderer.renderQuadList` (Part 83). The display board, which reaches its text through
    `iterateFormatted` now, asks the font for single glyphs (`FontSet.getGlyphInfo` and
    `getGlyph`, `GlyphInfo.getAdvance`, `BakedGlyph.render`), which the game doesn't give mods yet.
    `kinsurvey6.sh` reads the two pistons cell by cell and watches five lone fake tracks. The
    extending piston ends `extended` with its pole at +1 and its planks at +3, but air at +2 where
    Create puts the head (Part 83). The fake tracks stay through 100 ticks with random ticks off and
    200 at the default speed (3), and all are gone within 40 at 4096, as Create's: a fake track lives
    three random ticks unless something keeps it (`keepAlive`).
- [x] 2026-11-07 Part 83: the baked models a mod finds by location, as vanilla's `ModelManager` keeps
  them after the mods' model events, and the item models built on them. Counted on the pack's 367
  jars: `ItemRenderer.getModel` in 50, `ModelManager.getModel(location)` 26, `new
  ModelResourceLocation` 22 and its `standalone`/`inventory`/`vanilla` factories 23/9/2,
  `ModelEvent.ModifyBakingResult` 18 (its `getModels` 17) and `BakingCompleted` 12, `getMissingModel`
  11, `ItemRenderer.renderModelLists` 10, `ItemModelShaper.getItemModel` 6, `renderQuadList` 4.
  - compat `baked_models` (new): `ModelResourceLocation` as vanilla's record (its variant in lower
    case, printed `id#variant`, the three factories); one registry per runtime, keyed by those
    locations: the mod's blocks (each at its default state's location) and items are in it before
    the mods' `ModifyBakingResult` listeners run, then their `BakingCompleted` ones (posted now
    too); a location no mod put a model at gets the game's when asked for (`get`,
    `getOrDefault`, `containsKey`, `ModelManager.getModel`): an item's `item/` model, a
    `standalone` model by its name, else the block state's model. `ItemRenderer.getModel` and
    `ItemModelShaper.getItemModel` give the model at the item's location (the trident's and
    spyglass's in-hand ones as vanilla's), so a model a mod wrapped there is the one it gets back,
    and the wrapped one draws the item's faces; `renderModelLists` and `renderQuadList` put each
    face into the consumer under the stack's pose (`putBulkData`, as vanilla's).
  - `BlockModelShaper.stateToModelLocation` makes a location record with the state's properties in
    name order (it was a string, and the registry was keyed by `name#` alone, so a swap Create put
    at a block state with properties was never found); `glue::render_shape` looks up the state it
    is asked about. A block's probe model now answers its state's faces outside the probe.
  - compat: a vanilla enum value a mod sets in a state (`setValue`, a property's `getName(value)`)
    is spelled by its serialized name, as `getSerializedName` gives it: `PistonType.DEFAULT` is
    `normal` (it was `default`, the constant's own name in lower case, which the game refuses).
    Create's mechanical piston sets its head so where its extension ends (`PistonType.DEFAULT` in 2
    of the pack's jars, `STICKY` in 4); the head was missing after every extension. Measured with a
    trace of the mod's block changes: the head's `setBlock` at +2 answered false, the poles' and
    the planks' true.
  - The mods' model events wait for the client: a runtime's registry made before its setup (the
    probe runtime that reads render shapes, a mod's client code run early) posts
    `ModifyBakingResult` and `BakingCompleted` once the setup is done and the client's resources
    load, as vanilla bakes its models then, and the item models found in it before are found
    again. Create filters the items whose models it swaps by their client extensions, which the
    setup registers, once, the first time it is asked: asked before them, it kept none.
  - compat: `Registry.containsValue(value)` is whether the value is registered there (7 of the
    pack's jars). Create keeps only the custom-rendered items that are: unmapped, it answered
    false and Create dropped every one, so its lectern drew no controller.
  - Tests: compat 543 (6 new: a location is vanilla's record, with its factories; a state's
    location lists its properties by name, replacing render's string test of it; the registry has
    the game's model where no mod put one; an item model a mod wrapped is the one the item renderer
    gives, and it draws the original's faces; the model events wait for the client's setup; a
    vanilla enum value is set by its serialized name; and the registry key test asks
    `containsValue`), translator 19, core 7.
  - In game (`lectern.sh`): Create's swap lands in the registry (3870 of its 5313 locations hold a
    model of Create's after its listeners; `create:linked_controller#inventory` its
    `CustomRenderedItemModel`), and the lectern draws the controller on it, without anything
    unmapped but `Entity.position` on the player. It draws dark: the controller's block is declared
    an opaque full cube (Create's `LecternControllerBlock` extends vanilla's `LecternBlock`, whose
    shape the declaration didn't know), so the light inside it, which lights its renderer, is 0
    (Part 85). `kinsurvey7.sh`: the extending piston's head is at +2 (`type=normal`).
- [x] 2026-11-07 Part 84: a font's glyphs, which a renderer draws one character at a time (Create's
  display boards, `FlapDisplayRenderer`). Counted on the pack's 367 jars: `FontSet` in 5 (Create,
  Jade, Bits 'n' Bobs, DragonLib, GuideME), `getGlyphInfo` 5, `GlyphInfo.getAdvance(bold)` 5,
  `getGlyph` 3, `BakedGlyph.render` 3, `renderType` 3, `whiteGlyph` 2, `getRandomGlyph` 2,
  `renderEffect` and `BakedGlyph$Effect` 2, `EmptyGlyph` 2.
  - compat `gui`: a font set's glyphs by code point, one object each as a font set keeps them:
    `getGlyphInfo` (its advance is the game's width of the character, a bold one a pixel more; the
    bold and shadow offsets a pixel), `getGlyph` (the default font's spaces, U+0020 and U+200C, are
    `EmptyGlyph`s, which `instanceof` tells apart and which draw nothing), `getRandomGlyph` (an
    obfuscated glyph: a printable one as wide, at random, as vanilla's `glyphsByWidth`).
    `BakedGlyph.render(italic, x, y, matrix, ...)` draws its character from (x, y) under the matrix
    in the colour given, as the host draws a renderer's text (on the GUI when a mod draws there); a
    colour too faint to see draws nothing, as vanilla's glyph does. `renderEffect` puts the
    effect's rectangle into the buffer as vanilla's four vertices (white in the effect's colour, as
    the text types' texture isn't the game's atlas), a fill on the GUI.
  - Tests: compat 545 (2 new: a font's glyphs draw their characters, the spaces drawing nothing;
    a glyph's effect is a rectangle in its colour), translator 19, core 7.
  - In game (`allblocks.sh`, every Create block placed, with Part 83): the display board's four
    glyph calls and the lectern controller's two model calls are gone from the unmapped list. The
    blocks their renderer draws are 101 (98 before): the cogwheel, the large cogwheel and the shaft
    join them, as the models Create swaps in for them have no faces of their own (Part 83). Four
    lines are new: Flywheel's shader sources throw in the probe runtime that reads render shapes,
    whose host has no resources, and a renderer there asks the unplaced player's `position`, as the
    probe runtime now sets the client up (Part 83; both Part 85); `DebugPackets
    .sendNeighborsUpdatePacket` from Create's fluid pipes, and `BellBlock.updateShape` from its
    peculiar bell as its neighbour breaks (Part 85).
- [x] 2026-11-07 Part 85: mod blocks that inherit a vanilla class's own shape, and the loose ends
  of the every-block survey. Counted with a scan of the pack's 367 jars (`shapes_pack.py`, every
  class file, nested jars too): 153 mod block classes draw no shape of their own and extend a
  vanilla class whose shape isn't a full cube (13 jars' liquids, 9 saplings, plants, signs,
  pipes, cauldrons, Create's diodes, lectern and rail); 23 of them reach a class whose shape the
  runtime keeps (diodes 5, chests 4, lecterns 3, rails 5, redstone torches 4, a hopper, an ender
  chest). Two have no template while a vanilla class above theirs has one (Create's weathering
  copper stairs, Farmer's Delight's hay).
  - compat `blocks`: such a block was declared an opaque full cube (stone), so the light inside it,
    which lights what its renderer draws, was 0: Create's lectern controller drew its controller
    black, and its latches, pulse repeaters and fluid pipes were dark. It is now declared
    see-through unless its first vanilla class is one whose shape is `BlockBehaviour`'s, and its
    states' shapes are read through the nearest vanilla class above it whose shape the runtime
    keeps (a repeater's is its diode's), as vanilla dispatches `getShape` to it. The runtime keeps
    three more: a diode's plate, a rail's (half a block where it climbs) and a lectern's (base,
    post and the sloped top that stands above the block, by where it faces). The template is the
    nearest vanilla class's above it that has one, so weathering copper stairs are stairs.
  - The probe runtime that reads render shapes sets the client up without reloading resources:
    its host has none, so the mods' reload listeners read nothing (Flywheel's shader sources
    threw in the every-block survey, Part 84); the models are baked as before.
  - compat: a player the game hasn't placed yet (the client's before its world loads, the probe
    runtime drawing a renderer) is where a new entity is: `position`, `getX`, `blockPosition` and
    the rest at the origin, looking south (only the look and the eyes were). Create's factory
    panel renderer asks for it.
  - compat: vanilla's `DebugPackets` do nothing, as in a released game (Create's fluid pipes send
    `sendNeighborsUpdatePacket`), but for game tests' markers.
  - Measured, not done (`supers_pack.py`): 69 mod classes call `super.updateShape` into one of 25
    vanilla classes with a rule of their own (Create's peculiar bell into `BellBlock`, the
    survey's last unmapped line), and 375 inherit one of 44 such rules without overriding it
    (bushes 41, slabs 39, stairs 34, face-attached blocks 27, liquids 21, trapdoors 17, falling
    blocks 14).
  - Tests: compat 547 (2 new: a block draws the shape of the vanilla class it extends, a lectern's
    by where it faces and a repeater's plate, none for one whose first vanilla class's shape is a
    full cube, and weathering copper stairs are stairs; a player the game hasn't placed is at the
    origin), translator 19, core 7.
  - In game (`p85scene.sh`, each block beside the vanilla one it extends): the sky light inside
    Create's lectern controller, latches, pulse repeater, controller rail, fluid pipe and copper
    shingle stairs is 15, as inside the vanilla lectern, repeater, rail and stairs (0 inside the
    controller in Part 84's run); the lectern draws the controller as lit as the lectern beside it,
    the block below each is the floor, and nothing is unmapped but Flywheel's GL calls. The
    controller rail draws without its colour (the mods' colour handlers: next). Create's display
    board (`boardscene.sh`) draws its board and its cogwheel, not yet its text.
- [x] 2026-11-07 Part 86: a block set with data sends that data (Create's display board showed no
  text). `/setblock` with a mod block entity's data placed the block, whose `onPlace` made its block
  entity (Create's kinetic blocks ask for theirs), and the game took the data after; players were
  sent the block entity's update tag before its first tick loaded that data, and nothing sent it
  again: the board's client read four blank flaps (`StringDecomposer` was handed "    ").
  - compat `glue`: what the game holds of a live block entity is taken before its update tag is
    sent too, when it is news to it: made when something asked for it, it loads the game's data
    first, as vanilla's has loaded it before its chunk sends it; changed since it last saved
    (`/data merge block`), it loads that, once (the tick after doesn't load it again).
  - translator: the update tag hook hands the runtime what the game holds of the block entity.
  - compat: when tracing, a component's JSON the runtime can't read is told (`component JSON
    unread`), as `fromJson` answers null for it.
  - Tests: compat 548 (1 new: a block entity made before its data was set sends that data, and a
    later change of the data once), translator 19, core 7.
  - In game (`boardscene.sh`: a display board set with `RUST` and `MODS` on its two lines): the
    board shows both words in its flaps' font, each letter in its flap, where it showed nothing;
    its client decomposes "RUST" and "MODS" (it was handed "    "), and nothing is unmapped but
    Flywheel's GL calls.
- [x] 2026-11-07 Part 87: mod blocks tinted by their colour handlers (Create's controller rail drew
  its wire uncoloured, as every mod block drew its tinted faces: the mesher tints by vanilla's
  names only). Measured over the 466 scanned jars: `RegisterColorHandlersEvent$Item.register` 34,
  `Minecraft.getBlockColors` 31, `BlockColors.getColor(state, getter, pos, index)` 31,
  `$Block.register` 29; their handlers read `BiomeColors` (foliage 10, water 8, grass 7),
  `getBlockTint` (8), `GrassColor.get` (9), `FoliageColor`'s constants (7/5/5/3) and
  `RedStoneWireBlock.getColorForPower` (3). None of it was answered.
  - compat `colors` (new module): the client's `BlockColors` and `ItemColors`
    (`Minecraft.getBlockColors()`, the events'), registration through
    `RegisterColorHandlersEvent.Block`/`Item` (posted as the client is set up, after its client
    extensions) and the deprecated `register`; `BlockColors.getColor` (the handler, else vanilla's
    own blocks as 1.21.1's `BlockColors.createDefault` registers them, else -1) and its 3-argument
    form, `ItemColors.getColor`; `BiomeColors`' averages, its three resolvers and any getter's
    `getBlockTint(pos, resolver)`, a mod's own resolver run on the biome there; a biome's colours
    from its data (`effects`' colours, the colour map at its climate, dark forest's modifier, swamp
    grass's lighter shade; no blend across biomes, plains' where the host has no biome);
    `GrassColor`/`FoliageColor` (the colour maps read from the resources, the constants) and
    `getColorForPower` in vanilla's floats.
  - compat `colors::block_tints(rt, state)`: runs the block's handler for tint indices 0 to 2 in a
    level of that block, three times: twice with marker biome colours (equal to the marker both
    times: that biome colour; equal across the runs: a constant, white none), once with plains'
    (a colour derived from a biome one without being it). The last source holds for higher
    indices.
  - horizon-mods: a block's `properties.tints` (all states, or by one own state property), each
    source `grass`, `foliage`, `dry_foliage`, `water`, `none` or a colour number; checked like
    `render_shape`. docs/mods.md says it.
  - translator: the content hook asks the tints as it asks render shapes (the default state, then
    one property at a time; one property deciding gives its values' own).
  - client: a mod state's facts row carries its `tints`; horizon-items `StateFacts.tints`; the
    mesher tints a mod state by them (two sources kept, the second for any higher index), with the
    game's own biome blend.
  - Tests: compat 554 (6 new: a registered handler answers for its block; vanilla's blocks their
    own colours; `getColorForPower` in vanilla's floats, worked by hand; the probe tells a biome
    colour from constants and from a state's own (a rail's power); the resolvers through
    `getBlockTint`; a biome's data colours and modifiers), translator 19, core 7, horizon-mods 34
    (1 new: declared tints, all states or by a property, and what is refused), horizon-items 53
    (the facts row's tints read, what it can't read untinted).
  - In game (`p87scene.sh`): Create's controller rails set to powers 0 to 14 draw their wire from
    dark to bright red as vanilla's dust beside them does; power 15's brightest pixel is
    (250, 50, 1) and power 0's (75, 1, 1) (`0xFF3200` and `0x4C0000` on a near-white texel),
    where the wire drew grey. Nothing new is unmapped. Seen: a controller rail beside a redstone
    block placed after it stays at power 0 (vanilla's `BaseRailBlock.neighborChanged`, which runs
    Create's `updateState`, isn't run for a mod rail): next.
- [ ] Next: standalone harness (`scratchpad/harness`: links only the engine) checks Create registers its blocks and
  block entities at runtime; then the kinetic test in game (creative motor + shafts, read Speed).
- Note: the horizon-ctl socket path must be short (SUN_LEN), so use `mktemp -d /tmp/hz.XXXX` for it.
- Disk: a game build with Create's engine under `--profile fast` (incremental) filled ~100 GB across a few target dirs and
  stopped the session (ENOSPC). Build it with `CARGO_INCREMENTAL=0 CARGO_PROFILE_FAST_INCREMENTAL=false
  CARGO_PROFILE_FAST_DEBUG=0`, keep one target dir for it, and delete stage/target dirs you no longer need.
