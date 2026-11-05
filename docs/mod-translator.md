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
