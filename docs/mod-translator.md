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
  - [ ] Outlines and collisions from shapes checked by hand in game; the 5 slow `mod_content` tests (need
    `.local/horizon-client/26.3`, absent in this checkout; the prepared copy is `scratchpad/prepared`).
  - Gaps seen: `ItemBlockRenderTypes.setRenderLayer` (cutout layers), fluid pipes' side properties (vanilla
    `PipeBlock`'s getShape isn't Create's), item `getDefaultInstance`, title-screen panorama calls.
  (`KineticBlockEntityRenderer`, catnip's `SuperByteBuffer`), which need: a client runtime (the render thread's own
  `with_runtime`, `FMLClientSetupEvent` registering renderers), block entity update tags to the client
  (`native_block_entity_update_tag`, `blockChanged` → `native_block_entity_sync`), the client tick
  (`ClientTickEvent`, catnip's `AnimationTickHolder`), and drawing through `native_block_entity_submit`
  (PoseStack, JOML, catnip's buffers as natives: one draw call per buffer, no per-vertex translated code).
- [ ] Next: standalone harness (`scratchpad/harness`: links only the engine) checks Create registers its blocks and
  block entities at runtime; then the kinetic test in game (creative motor + shafts, read Speed).
- Note: the horizon-ctl socket path must be short (SUN_LEN), so use `mktemp -d /tmp/hz.XXXX` for it.
- Disk: a game build with Create's engine under `--profile fast` (incremental) filled ~100 GB across a few target dirs and
  stopped the session (ENOSPC). Build it with `CARGO_INCREMENTAL=0 CARGO_PROFILE_FAST_INCREMENTAL=false
  CARGO_PROFILE_FAST_DEBUG=0`, keep one target dir for it, and delete stage/target dirs you no longer need.
