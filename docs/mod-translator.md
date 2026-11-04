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
    gained `native_move_item_entity`, `native_spawn_item_entity`, `native_game_time`,
    `native_dimension`, and `NativeItemEntity` carries motion, `on_ground` and age. Contacts: a mod
    block with a block entity that overrides `updateEntityAfterFallOn` or `entityInside` is told
    of item entities resting on it or in its cell from its block entity's tick (vanilla's
    `Entity.move` reports them to the block; Create's basin, belt, depot, chute, saw, millstone,
    crushing wheel and funnel take items this way). An item resting in the cell counts as on the
    block, since the game holds it on the general collision box where Create's item-only shape
    (a basin's raised floor) would put it on the block itself.
    Also traced (with `HORIZON_COMPAT_TRACE`): the first update tags the server sends and the
    client loads for each block entity, with the items they hold.
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
