//! Vanilla 1.21.1's structure templates (`StructureTemplate`), with NeoForge's additions: a box of
//! the world's blocks and entities kept relative to its lowest corner, saved and loaded as
//! vanilla's structure files are, and placed back turned, mirrored, clipped and processed as a
//! `StructurePlaceSettings` says. Create's schematics are these: the schematic table and cannon
//! load one and place it into Create's own level, which takes the blocks through its own
//! `setBlock`; the schematic and quill fills one from the world and saves it.
//!
//! - Filling (`fillFromWorld`): the box's blocks (but those of the block to leave out), by position
//!   from its corner, with their block entities' data (`saveWithId`); plain full blocks first, then
//!   the other blocks, then those with data, each by y, x, z, as vanilla orders them; and the
//!   entities in it other than players, with their saved data.
//! - Placing (`placeInWorld`): one of the palettes (the settings' random picks it), each block
//!   moved to where the settings turn it about their pivot and to the offset, the processors'
//!   `process` (a mod's own) and `finalizeProcessing` run over them, its state mirrored then
//!   turned. A block with data first clears what stands there for a barrier, then loads the data
//!   (a loot container a new seed); a waterloggable block in water takes it, from a source beside
//!   it when its own is flowing; unless the settings know the shape, the outer faces of what was
//!   placed and each placed block take their neighbours' shapes. The entities are made from their
//!   data, turned, mirrored and added.
//! - The transforms as vanilla's formulas: `transform` (a position, a point), the settings' relative
//!   position, `getBoundingBox`, `getZeroPositionWithTransform`, `getSize(rotation)`.
//! - `StructurePlaceSettings`: vanilla's fields (a mod's accessors read them), `copy`, the setters
//!   and getters, `getRandom(pos)` (the source set, else one seeded from the position) and the
//!   palette it picks.
//! - `StructureProcessor`'s defaults: `process` asks `processBlock`, which keeps the block;
//!   `finalizeProcessing` and `processEntity` keep what they are given.
//! - The server's templates by id (`StructureTemplateManager`, `getStructureManager()`): `getOrCreate`
//!   keeps a new one under its id, `get` gives the one kept, `readStructure` loads one from a tag.
//!   Loading the files a mod or data pack ships (`structure/*.nbt`) is not here yet.
use crate::nbt::Nbt;
use crate::{Data, Rt, V, api};
use std::collections::BTreeMap;

const TEMPLATE: &str =
    "net/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate";
const PALETTE: &str =
    "net/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$Palette";
const BLOCK_INFO: &str = "net/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureBlockInfo";
const SETTINGS: &str =
    "net/minecraft/world/level/levelgen/structure/templatesystem/StructurePlaceSettings";
const PROCESSOR: &str =
    "net/minecraft/world/level/levelgen/structure/templatesystem/StructureProcessor";
const MANAGER: &str =
    "net/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplateManager";
const MANAGER_DESC: &str =
    "()Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplateManager;";
const ENTITY: &str = "net/minecraft/world/entity/Entity";
const VEC3I: &str = "net/minecraft/core/Vec3i";
const OBJECT_ARRAY_LIST: &str = "it/unimi/dsi/fastutil/objects/ObjectArrayList";
/// `StructureEntityInfo(pos, blockPos, nbt)`.
const ENTITY_INFO_INIT: &str = "(Lnet/minecraft/world/phys/Vec3;Lnet/minecraft/core/BlockPos;Lnet/minecraft/nbt/CompoundTag;)V";
/// 1.21.1's data version, which `save` stamps (`NbtUtils.addCurrentDataVersion`).
const DATA_VERSION: i32 = 3955;

const MIRROR_NONE: &str = "net/minecraft/world/level/block/Mirror.NONE";
const ROTATION_NONE: &str = "net/minecraft/world/level/block/Rotation.NONE";
const APPLY_WATERLOGGING: &str =
    "net/minecraft/world/level/levelgen/structure/templatesystem/LiquidSettings.APPLY_WATERLOGGING";
const EMPTY_GETTER: &str = "net/minecraft/world/level/EmptyBlockGetter.INSTANCE";

const GET_BLOCK_STATE: (&str, &str) = (
    "net/minecraft/world/level/BlockGetter.getBlockState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;",
    "getBlockState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;",
);
const GET_BLOCK_ENTITY: (&str, &str) = (
    "net/minecraft/world/level/BlockGetter.getBlockEntity.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/entity/BlockEntity;",
    "getBlockEntity.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/entity/BlockEntity;",
);
const GET_FLUID_STATE: (&str, &str) = (
    "net/minecraft/world/level/BlockGetter.getFluidState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/material/FluidState;",
    "getFluidState.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/material/FluidState;",
);
const SET_BLOCK: (&str, &str) = (
    "net/minecraft/world/level/LevelWriter.setBlock.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;I)Z",
    "setBlock.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;I)Z",
);
const REGISTRY_ACCESS: (&str, &str) = (
    "net/minecraft/world/level/Level.registryAccess.()Lnet/minecraft/core/RegistryAccess;",
    "registryAccess.()Lnet/minecraft/core/RegistryAccess;",
);
const IS_CLIENT_SIDE: (&str, &str) = (
    "net/minecraft/world/level/LevelAccessor.isClientSide.()Z",
    "isClientSide.()Z",
);
const BLOCK_UPDATED: (&str, &str) = (
    "net/minecraft/world/level/LevelAccessor.blockUpdated.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Block;)V",
    "blockUpdated.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Block;)V",
);
const SCHEDULE_FLUID_TICK: (&str, &str) = (
    "net/minecraft/world/level/LevelAccessor.scheduleTick.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/material/Fluid;I)V",
    "scheduleTick.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/material/Fluid;I)V",
);
const ENTITIES_OF_CLASS: (&str, &str) = (
    "net/minecraft/world/level/Level.getEntitiesOfClass.(Ljava/lang/Class;Lnet/minecraft/world/phys/AABB;)Ljava/util/List;",
    "getEntitiesOfClass.(Ljava/lang/Class;Lnet/minecraft/world/phys/AABB;)Ljava/util/List;",
);
const ADD_ENTITY: (&str, &str) = (
    "net/minecraft/server/level/ServerLevel.addFreshEntityWithPassengers.(Lnet/minecraft/world/entity/Entity;)V",
    "addFreshEntityWithPassengers.(Lnet/minecraft/world/entity/Entity;)V",
);
const GET_LEVEL: (&str, &str) = (
    "net/minecraft/world/level/ServerLevelAccessor.getLevel.()Lnet/minecraft/server/level/ServerLevel;",
    "getLevel.()Lnet/minecraft/server/level/ServerLevel;",
);
const NEXT_INT: (&str, &str) = (
    "net/minecraft/util/RandomSource.nextInt.(I)I",
    "nextInt.(I)I",
);
const NEXT_LONG: (&str, &str) = (
    "net/minecraft/util/RandomSource.nextLong.()J",
    "nextLong.()J",
);
const SET_CHANGED: (&str, &str) = (
    "net/minecraft/world/level/block/entity/BlockEntity.setChanged.()V",
    "setChanged.()V",
);
const ENTITY_SAVE: (&str, &str) = (
    "net/minecraft/world/entity/Entity.save.(Lnet/minecraft/nbt/CompoundTag;)Z",
    "save.(Lnet/minecraft/nbt/CompoundTag;)Z",
);
const ROTATE_DESC: &str = "(Lnet/minecraft/world/level/block/Rotation;)F";
const MIRROR_DESC: &str = "(Lnet/minecraft/world/level/block/Mirror;)F";
const ENTITY_ROTATE: (&str, &str) = (
    "net/minecraft/world/entity/Entity.rotate.(Lnet/minecraft/world/level/block/Rotation;)F",
    "rotate.(Lnet/minecraft/world/level/block/Rotation;)F",
);
const ENTITY_MIRROR: (&str, &str) = (
    "net/minecraft/world/entity/Entity.mirror.(Lnet/minecraft/world/level/block/Mirror;)F",
    "mirror.(Lnet/minecraft/world/level/block/Mirror;)F",
);
const GET_Y_ROT: (&str, &str) = (
    "net/minecraft/world/entity/Entity.getYRot.()F",
    "getYRot.()F",
);
const GET_X_ROT: (&str, &str) = (
    "net/minecraft/world/entity/Entity.getXRot.()F",
    "getXRot.()F",
);
const MOVE_TO: (&str, &str) = (
    "net/minecraft/world/entity/Entity.moveTo.(DDDFF)V",
    "moveTo.(DDDFF)V",
);
const HANGING_POS: (&str, &str) = (
    "net/minecraft/world/entity/decoration/HangingEntity.getPos.()Lnet/minecraft/core/BlockPos;",
    "getPos.()Lnet/minecraft/core/BlockPos;",
);
const FINALIZE_SPAWN: (&str, &str) = (
    "net/minecraft/world/entity/Mob.finalizeSpawn.(Lnet/minecraft/world/level/ServerLevelAccessor;Lnet/minecraft/world/DifficultyInstance;Lnet/minecraft/world/entity/MobSpawnType;Lnet/minecraft/world/entity/SpawnGroupData;)Lnet/minecraft/world/entity/SpawnGroupData;",
    "finalizeSpawn.(Lnet/minecraft/world/level/ServerLevelAccessor;Lnet/minecraft/world/DifficultyInstance;Lnet/minecraft/world/entity/MobSpawnType;Lnet/minecraft/world/entity/SpawnGroupData;)Lnet/minecraft/world/entity/SpawnGroupData;",
);
const DIFFICULTY_AT: (&str, &str) = (
    "net/minecraft/world/level/ServerLevelAccessor.getCurrentDifficultyAt.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/DifficultyInstance;",
    "getCurrentDifficultyAt.(Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/DifficultyInstance;",
);
const PROCESS: (&str, &str) = (
    "net/minecraft/world/level/levelgen/structure/templatesystem/StructureProcessor.process.(Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureBlockInfo;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureBlockInfo;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructurePlaceSettings;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate;)Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureBlockInfo;",
    "process.(Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureBlockInfo;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureBlockInfo;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructurePlaceSettings;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate;)Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureBlockInfo;",
);
const PROCESS_BLOCK: (&str, &str) = (
    "net/minecraft/world/level/levelgen/structure/templatesystem/StructureProcessor.processBlock.(Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureBlockInfo;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureBlockInfo;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructurePlaceSettings;)Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureBlockInfo;",
    "processBlock.(Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureBlockInfo;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureBlockInfo;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructurePlaceSettings;)Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureBlockInfo;",
);
const FINALIZE: (&str, &str) = (
    "net/minecraft/world/level/levelgen/structure/templatesystem/StructureProcessor.finalizeProcessing.(Lnet/minecraft/world/level/ServerLevelAccessor;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/BlockPos;Ljava/util/List;Ljava/util/List;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructurePlaceSettings;)Ljava/util/List;",
    "finalizeProcessing.(Lnet/minecraft/world/level/ServerLevelAccessor;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/BlockPos;Ljava/util/List;Ljava/util/List;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructurePlaceSettings;)Ljava/util/List;",
);
const PROCESS_ENTITY: (&str, &str) = (
    "net/minecraft/world/level/levelgen/structure/templatesystem/StructureProcessor.processEntity.(Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureEntityInfo;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureEntityInfo;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructurePlaceSettings;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate;)Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureEntityInfo;",
    "processEntity.(Lnet/minecraft/world/level/LevelReader;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureEntityInfo;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureEntityInfo;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructurePlaceSettings;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate;)Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate$StructureEntityInfo;",
);
const PLACE_LIQUID: &str = "placeLiquid.(Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/material/FluidState;)Z";
const PLACE_LIQUID_KEY: &str = "net/minecraft/world/level/block/LiquidBlockContainer.placeLiquid.(Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/material/FluidState;)Z";
const SAVE_WITH_ID: &str = "net/minecraft/world/level/block/entity/BlockEntity.saveWithId.(Lnet/minecraft/core/HolderLookup$Provider;)Lnet/minecraft/nbt/CompoundTag;";
const LOAD_WITH_COMPONENTS: &str = "net/minecraft/world/level/block/entity/BlockEntity.loadWithComponents.(Lnet/minecraft/nbt/CompoundTag;Lnet/minecraft/core/HolderLookup$Provider;)V";
const TRY_CLEAR: &str = "net/minecraft/world/Clearable.tryClear.(Ljava/lang/Object;)V";
const FULL_BLOCK: &str = "net/minecraft/world/level/block/state/BlockState.isCollisionShapeFullBlock.(Lnet/minecraft/world/level/BlockGetter;Lnet/minecraft/core/BlockPos;)Z";
const STATE_FLUID: &str = "net/minecraft/world/level/block/state/BlockState.getFluidState.()Lnet/minecraft/world/level/material/FluidState;";
const IS_SOURCE: &str = "net/minecraft/world/level/material/FluidState.isSource.()Z";
const GET_BLOCK: &str = "net/minecraft/world/level/block/state/BlockState.getBlock.()Lnet/minecraft/world/level/block/Block;";
const FROM_NEIGHBOUR_SHAPES: &str = "net/minecraft/world/level/block/Block.updateFromNeighbourShapes.(Lnet/minecraft/world/level/block/state/BlockState;Lnet/minecraft/world/level/LevelAccessor;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/block/state/BlockState;";
const PUT_LONG: &str = "net/minecraft/nbt/CompoundTag.putLong.(Ljava/lang/String;J)V";
const PUT_TAG: &str = "net/minecraft/nbt/CompoundTag.put.(Ljava/lang/String;Lnet/minecraft/nbt/Tag;)Lnet/minecraft/nbt/Tag;";
const REMOVE_KEY: &str = "net/minecraft/nbt/CompoundTag.remove.(Ljava/lang/String;)V";
const CREATE_ENTITY: &str = "net/minecraft/world/entity/EntityType.create.(Lnet/minecraft/nbt/CompoundTag;Lnet/minecraft/world/level/Level;)Ljava/util/Optional;";
const MOB_SPAWN_STRUCTURE: &str = "net/minecraft/world/entity/MobSpawnType.STRUCTURE";

/// Vanilla's blocks whose shape changes as they stand (`Properties.dynamicShape()`), besides the
/// shulker boxes: a template keeps them with the other blocks that aren't plain full ones.
const DYNAMIC_SHAPES: [&str; 5] = [
    "minecraft:moving_piston",
    "minecraft:bamboo",
    "minecraft:scaffolding",
    "minecraft:powder_snow",
    "minecraft:pointed_dripstone",
];

/// The settings' fields a setter sets, by setter, as vanilla names them.
const SETTERS: [(&str, &str); 9] = [
    ("setMirror", "mirror"),
    ("setRotation", "rotation"),
    ("setRotationPivot", "rotationPivot"),
    ("setIgnoreEntities", "ignoreEntities"),
    ("setBoundingBox", "boundingBox"),
    ("setRandom", "random"),
    ("setLiquidSettings", "liquidSettings"),
    ("setKnownShape", "knownShape"),
    ("setFinalizeEntities", "finalizeEntities"),
];

/// Offsets of the six directions, by ordinal (down, up, north, south, west, east).
const STEPS: [[i32; 3]; 6] = [
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [-1, 0, 0],
    [1, 0, 0],
];
const OPPOSITE: [usize; 6] = [1, 0, 3, 2, 5, 4];

fn step(p: [i32; 3], d: usize) -> [i32; 3] {
    let s = STEPS[d];
    [p[0] + s[0], p[1] + s[1], p[2] + s[2]]
}

fn add(p: [i32; 3], q: [i32; 3]) -> [i32; 3] {
    [p[0] + q[0], p[1] + q[1], p[2] + q[2]]
}

fn block_pos(rt: &mut Rt, p: [i32; 3]) -> V {
    api::block_pos(rt, p[0].into(), p[1].into(), p[2].into())
}

/// A `Vec3i` (a template's size).
fn vec3i(rt: &mut Rt, p: [i32; 3]) -> V {
    rt.alloc(
        VEC3I,
        Data::Native("BlockPos", [p[0].into(), p[1].into(), p[2].into(), 0]),
    )
}

fn at(rt: &Rt, v: V) -> [i32; 3] {
    api::pos_of(rt, v)
}

/// A `Rotation`'s or `Mirror`'s ordinal (none for null).
fn ordinal(rt: &Rt, v: V) -> usize {
    if v.r() == 0 {
        0
    } else {
        rt.get_field(v, "$ordinal").i().max(0) as usize
    }
}

fn text(rt: &Rt, state: V) -> String {
    rt.str_of(state).unwrap_or("minecraft:air").to_owned()
}

fn same_state(rt: &Rt, a: V, b: V) -> bool {
    a == b || (rt.str_of(a).is_some() && rt.str_of(a) == rt.str_of(b))
}

/// The list in field `name` of `o`, made empty when it has none yet.
fn list_field(rt: &mut Rt, o: V, name: &'static str) -> V {
    let l = rt.get_field(o, name);
    if l.r() != 0 {
        return l;
    }
    let l = crate::jutil::new_list(rt, Vec::new());
    rt.put_field(o, name, l);
    l
}

/// The list `l` holding `items` instead of what it held (the same list object, as vanilla's final
/// fields keep theirs).
fn set_items(rt: &mut Rt, l: V, items: Vec<V>) {
    if let Some(Data::List(old)) = rt.heap.get_mut(l.r() as usize).map(|o| &mut o.data) {
        *old = items;
    }
}

// ---- transforms ----

/// `StructureTemplate.transform(pos, mirror, rotation, pivot)`: mirrored (left-right negates z,
/// front-back x), then turned about the pivot's x and z.
fn transform(p: [i32; 3], mirror: usize, rotation: usize, pivot: [i32; 3]) -> [i32; 3] {
    let [mut i, j, mut k] = p;
    match mirror {
        1 => k = -k,
        2 => i = -i,
        _ => {}
    }
    let (l, i1) = (pivot[0], pivot[2]);
    match rotation {
        3 => [l - i1 + k, j, l + i1 - i],
        1 => [l + i1 - k, j, i1 - l + i],
        2 => [l + l - i, j, i1 + i1 - k],
        _ => [i, j, k],
    }
}

/// The same for a point: a mirror flips it within its block (`1 - d`).
fn transform_point(v: [f64; 3], mirror: usize, rotation: usize, pivot: [i32; 3]) -> [f64; 3] {
    let [mut d0, d1, mut d2] = v;
    match mirror {
        1 => d2 = 1.0 - d2,
        2 => d0 = 1.0 - d0,
        _ => {}
    }
    let (i, j) = (pivot[0], pivot[2]);
    match rotation {
        3 => [f64::from(i - j) + d2, d1, f64::from(i + j + 1) - d0],
        1 => [f64::from(i + j + 1) - d2, d1, f64::from(j - i) + d0],
        2 => [f64::from(i + i + 1) - d0, d1, f64::from(j + j + 1) - d2],
        _ => [d0, d1, d2],
    }
}

/// `getZeroPositionWithTransform(pos, mirror, rotation, sizeX, sizeZ)`: where the template's own
/// corner lands for a box placed from `p`.
fn zero_position(
    p: [i32; 3],
    mirror: usize,
    rotation: usize,
    size_x: i32,
    size_z: i32,
) -> [i32; 3] {
    let (sx, sz) = (size_x - 1, size_z - 1);
    let i = if mirror == 2 { sx } else { 0 };
    let j = if mirror == 1 { sz } else { 0 };
    let d = match rotation {
        3 => [j, sx - i],
        1 => [sz - j, i],
        2 => [sx - i, sz - j],
        _ => [i, j],
    };
    [p[0] + d[0], p[1], p[2] + d[1]]
}

/// `getBoundingBox(start, rotation, pivot, mirror, size)`: the box between the corners turned, moved
/// to `start`.
fn bounds(
    start: [i32; 3],
    rotation: usize,
    pivot: [i32; 3],
    mirror: usize,
    size: [i32; 3],
) -> crate::shapes::IntBox {
    let a = transform([0, 0, 0], mirror, rotation, pivot);
    let b = transform(
        [size[0] - 1, size[1] - 1, size[2] - 1],
        mirror,
        rotation,
        pivot,
    );
    let lo = [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])];
    let hi = [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])];
    [
        lo[0] + start[0],
        lo[1] + start[1],
        lo[2] + start[2],
        hi[0] + start[0],
        hi[1] + start[1],
        hi[2] + start[2],
    ]
}

fn inside(b: crate::shapes::IntBox, p: [i32; 3]) -> bool {
    (0..3).all(|k| p[k] >= b[k] && p[k] <= b[k + 3])
}

// ---- settings ----

/// `new StructurePlaceSettings()`: vanilla's defaults.
fn init_settings(rt: &mut Rt, s: V) {
    let mirror = api::get_static(rt, MIRROR_NONE);
    let rotation = api::get_static(rt, ROTATION_NONE);
    let pivot = block_pos(rt, [0; 3]);
    let liquid = api::get_static(rt, APPLY_WATERLOGGING);
    let processors = crate::jutil::new_list(rt, Vec::new());
    for (k, v) in [
        ("mirror", mirror),
        ("rotation", rotation),
        ("rotationPivot", pivot),
        ("ignoreEntities", V::I(0)),
        ("boundingBox", V::NULL),
        ("liquidSettings", liquid),
        ("random", V::NULL),
        ("palette", V::I(0)),
        ("processors", processors),
        ("knownShape", V::I(0)),
        ("finalizeEntities", V::I(0)),
    ] {
        rt.put_field(s, k, v);
    }
}

/// The settings' field `name`, else what a new one has.
fn setting(rt: &mut Rt, s: V, name: &'static str) -> V {
    match rt.get_field(s, name) {
        V::Void => match name {
            "mirror" => api::get_static(rt, MIRROR_NONE),
            "rotation" => api::get_static(rt, ROTATION_NONE),
            "rotationPivot" => block_pos(rt, [0; 3]),
            "liquidSettings" => api::get_static(rt, APPLY_WATERLOGGING),
            "processors" => list_field(rt, s, "processors"),
            "boundingBox" | "random" => V::NULL,
            _ => V::I(0),
        },
        v => v,
    }
}

fn flag(rt: &mut Rt, s: V, name: &'static str) -> bool {
    setting(rt, s, name).i() != 0
}

/// Mirror and rotation ordinals and the pivot.
fn turn_of(rt: &mut Rt, s: V) -> (usize, usize, [i32; 3]) {
    let (m, r, p) = (
        setting(rt, s, "mirror"),
        setting(rt, s, "rotation"),
        setting(rt, s, "rotationPivot"),
    );
    (ordinal(rt, m), ordinal(rt, r), at(rt, p))
}

/// `calculateRelativePosition(settings, pos)`.
fn relative(rt: &mut Rt, s: V, p: [i32; 3]) -> [i32; 3] {
    let (m, r, pivot) = turn_of(rt, s);
    transform(p, m, r, pivot)
}

fn clip_box(rt: &mut Rt, s: V) -> Option<crate::shapes::IntBox> {
    let b = setting(rt, s, "boundingBox");
    (b.r() != 0).then(|| crate::shapes::int_box_of(rt, b))
}

/// `shouldApplyWaterlogging()`: the liquid settings are `APPLY_WATERLOGGING`.
fn waterlogs(rt: &mut Rt, s: V) -> bool {
    let l = setting(rt, s, "liquidSettings");
    l.r() != 0 && ordinal(rt, l) == 1
}

/// `getRandom(pos)`: the source set, else a new one seeded from the position (`Mth.getSeed`), else
/// from the clock.
fn settings_random(rt: &mut Rt, s: V, at_pos: V) -> V {
    let r = setting(rt, s, "random");
    if r.r() != 0 {
        return r;
    }
    if at_pos.r() == 0 {
        return api::call(
            rt,
            "net/minecraft/util/RandomSource.create.()Lnet/minecraft/util/RandomSource;",
            &[],
        );
    }
    let p = at(rt, at_pos);
    let seed = api::call(
        rt,
        "net/minecraft/util/Mth.getSeed.(III)J",
        &[V::I(p[0]), V::I(p[1]), V::I(p[2])],
    )
    .j();
    crate::api_common::seeded_random(rt, seed)
}

/// `getRandomPalette(palettes, pos)`: one drawn from the settings' source; null (and
/// `IllegalStateException`) for none.
fn random_palette(rt: &mut Rt, s: V, palettes: V, at_pos: V) -> V {
    let all = crate::jutil::elements(rt, palettes);
    if all.is_empty() {
        rt.throw_new("java/lang/IllegalStateException");
        return V::NULL;
    }
    let r = settings_random(rt, s, at_pos);
    let n = i32::try_from(all.len()).unwrap_or(i32::MAX);
    let i = api::virtual_call(rt, NEXT_INT, &[r, V::I(n)]).i();
    all.get(i.max(0) as usize).copied().unwrap_or(all[0])
}

fn settings_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let this = a(0);
    if let Some(&(_, field)) = SETTERS.iter().find(|(n, _)| *n == name) {
        rt.put_field(this, field, a(1));
        return Some(this);
    }
    Some(match name {
        "<init>" if desc == "()V" => {
            init_settings(rt, this);
            V::Void
        }
        "copy" => {
            let c = rt.new_object(SETTINGS);
            for (_, field) in SETTERS {
                let v = setting(rt, this, field);
                rt.put_field(c, field, v);
            }
            let palette = setting(rt, this, "palette");
            rt.put_field(c, "palette", palette);
            let own = setting(rt, this, "processors");
            let items = crate::jutil::elements(rt, own);
            let processors = crate::jutil::new_list(rt, items);
            rt.put_field(c, "processors", processors);
            c
        }
        "clearProcessors" | "addProcessor" | "popProcessor" => {
            let l = setting(rt, this, "processors");
            let mut items = crate::jutil::elements(rt, l);
            match name {
                "clearProcessors" => items.clear(),
                "addProcessor" => items.push(a(1)),
                _ => {
                    if let Some(i) = items.iter().position(|&p| p == a(1)) {
                        items.remove(i);
                    }
                }
            }
            set_items(rt, l, items);
            this
        }
        "getMirror" => setting(rt, this, "mirror"),
        "getRotation" => setting(rt, this, "rotation"),
        "getRotationPivot" => setting(rt, this, "rotationPivot"),
        "getBoundingBox" => setting(rt, this, "boundingBox"),
        "getProcessors" => setting(rt, this, "processors"),
        "isIgnoreEntities" => V::I(i32::from(flag(rt, this, "ignoreEntities"))),
        "getKnownShape" => V::I(i32::from(flag(rt, this, "knownShape"))),
        "shouldFinalizeEntities" => V::I(i32::from(flag(rt, this, "finalizeEntities"))),
        "shouldApplyWaterlogging" => V::I(i32::from(waterlogs(rt, this))),
        "getRandom" => settings_random(rt, this, a(1)),
        "getRandomPalette" => random_palette(rt, this, a(1), a(2)),
        _ => return None,
    })
}

// ---- templates ----

fn new_template(rt: &mut Rt) -> V {
    let t = rt.new_object(TEMPLATE);
    init_template(rt, t);
    t
}

/// `new StructureTemplate()`: no palettes or entities, size zero, author `?`.
fn init_template(rt: &mut Rt, t: V) {
    let palettes = crate::jutil::new_list(rt, Vec::new());
    let entities = crate::jutil::new_list(rt, Vec::new());
    let size = vec3i(rt, [0; 3]);
    let author = rt.string("?");
    rt.put_field(t, "palettes", palettes);
    rt.put_field(t, "entityInfoList", entities);
    rt.put_field(t, "size", size);
    rt.put_field(t, "author", author);
}

fn size_of(rt: &Rt, t: V) -> [i32; 3] {
    let s = rt.get_field(t, "size");
    if s.r() == 0 { [0; 3] } else { at(rt, s) }
}

fn size_value(rt: &mut Rt, t: V) -> V {
    match rt.get_field(t, "size") {
        s if s.r() != 0 => s,
        _ => vec3i(rt, [0; 3]),
    }
}

fn new_palette(rt: &mut Rt, infos: Vec<V>) -> V {
    let p = rt.new_object(PALETTE);
    let blocks = crate::jutil::new_list(rt, infos);
    rt.put_field(p, "blocks", blocks);
    p
}

fn block_info(rt: &mut Rt, pos: V, state: V, nbt: V) -> V {
    crate::records::make(rt, BLOCK_INFO, &[pos, state, nbt])
}

/// A block info's position, state and data.
fn info_parts(rt: &Rt, info: V) -> (V, V, V) {
    let nbt = match rt.get_field(info, "nbt") {
        V::Void => V::NULL,
        n => n,
    };
    (rt.get_field(info, "pos"), rt.get_field(info, "state"), nbt)
}

fn block_of(rt: &Rt, state: V) -> String {
    crate::host::split_state(&text(rt, state)).0.to_owned()
}

/// Whether the block's shape changes as it stands (`hasDynamicShape()`): vanilla's (the shulker
/// boxes and [`DYNAMIC_SHAPES`]), a mod's when its properties say so.
fn dynamic_shape(rt: &Rt, state: &str) -> bool {
    let block = crate::host::split_state(state).0;
    if DYNAMIC_SHAPES.contains(&block)
        || (block.starts_with("minecraft:") && block.ends_with("shulker_box"))
    {
        return true;
    }
    crate::glue::mod_block(rt, state).is_some_and(|(b, _)| {
        let props = rt.get_field(b, "$properties");
        props.r() != 0 && rt.get_field(props, "$dynamicShape").i() != 0
    })
}

/// Which of vanilla's lists a block info goes in (`addToLists`): 0 a plain full block, 1 another
/// block, 2 one with data. Whether its collision shape is a full block is asked of `getter` at
/// `pos` (vanilla asks an empty getter at the origin; where the block stands answers the same for
/// the game's blocks, and knows their shapes).
fn list_kind(rt: &mut Rt, state: V, nbt: V, getter: V, pos: V) -> u8 {
    if nbt.r() != 0 {
        return 2;
    }
    if dynamic_shape(rt, &text(rt, state)) {
        return 1;
    }
    let full = api::call(rt, FULL_BLOCK, &[state, getter, pos]).i() != 0;
    if full { 0 } else { 1 }
}

/// `buildInfoList`: by list, then by y, x, z, stable.
fn ordered(rt: &Rt, mut infos: Vec<(u8, V)>) -> Vec<V> {
    infos.sort_by_key(|&(kind, info)| {
        let p = at(rt, rt.get_field(info, "pos"));
        (kind, p[1], p[0], p[2])
    });
    infos.into_iter().map(|(_, i)| i).collect()
}

fn fill_from_world(rt: &mut Rt, t: V, [level, from, size]: [V; 3], with_entities: bool, ignore: V) {
    let (p, s) = (at(rt, from), at(rt, size));
    if s.iter().any(|&n| n < 1) {
        return;
    }
    let end = [p[0] + s[0] - 1, p[1] + s[1] - 1, p[2] + s[2] - 1];
    let lo = [p[0].min(end[0]), p[1].min(end[1]), p[2].min(end[2])];
    let hi = [p[0].max(end[0]), p[1].max(end[1]), p[2].max(end[2])];
    rt.put_field(t, "size", size);
    let ignored = (ignore.r() != 0).then(|| api::block_name(rt, ignore));
    let mut registries = None;
    let mut infos = Vec::new();
    for z in lo[2]..=hi[2] {
        for y in lo[1]..=hi[1] {
            for x in lo[0]..=hi[0] {
                let world = block_pos(rt, [x, y, z]);
                let state = api::virtual_call(rt, GET_BLOCK_STATE, &[level, world]);
                if ignored.as_deref().is_some_and(|b| block_of(rt, state) == b) {
                    continue;
                }
                let be = api::virtual_call(rt, GET_BLOCK_ENTITY, &[level, world]);
                let nbt = if be.r() != 0 {
                    let r = *registries
                        .get_or_insert_with(|| api::virtual_call(rt, REGISTRY_ACCESS, &[level]));
                    api::call(rt, SAVE_WITH_ID, &[be, r])
                } else {
                    V::NULL
                };
                if rt.thrown.is_some() {
                    return;
                }
                let kind = list_kind(rt, state, nbt, level, world);
                let rel = block_pos(rt, [x - lo[0], y - lo[1], z - lo[2]]);
                let info = block_info(rt, rel, state, nbt);
                infos.push((kind, info));
            }
        }
    }
    let list = ordered(rt, infos);
    let palette = new_palette(rt, list);
    let palettes = list_field(rt, t, "palettes");
    set_items(rt, palettes, vec![palette]);
    let entities = list_field(rt, t, "entityInfoList");
    let found = if with_entities {
        entities_in(rt, level, lo, hi)
    } else {
        Vec::new()
    };
    set_items(rt, entities, found);
}

/// `fillEntityList`: the entities in the box but players, each where it stands from the corner,
/// its block (a painting's own) and its saved data.
fn entities_in(rt: &mut Rt, level: V, lo: [i32; 3], hi: [i32; 3]) -> Vec<V> {
    let class = crate::jlang::class_object(rt, ENTITY);
    let area = crate::shapes::aabb(
        rt,
        [lo[0], lo[1], lo[2], hi[0] + 1, hi[1] + 1, hi[2] + 1].map(f64::from),
    );
    let found = api::virtual_call(rt, ENTITIES_OF_CLASS, &[level, class, area]);
    let mut out = Vec::new();
    for e in crate::jutil::elements(rt, found) {
        if e.r() == 0 || rt.instance_of(e, "net/minecraft/world/entity/player/Player") {
            continue;
        }
        let x = ["getX", "getY", "getZ"].map(|n| {
            let key = rt.static_key(&format!("{ENTITY}.{n}.()D"));
            api::call(rt, key, &[e]).d()
        });
        let v = [
            x[0] - f64::from(lo[0]),
            x[1] - f64::from(lo[1]),
            x[2] - f64::from(lo[2]),
        ];
        let tag = crate::tags::new_compound(rt);
        api::virtual_call(rt, ENTITY_SAVE, &[e, tag]);
        let block = if rt.instance_of(e, "net/minecraft/world/entity/decoration/Painting") {
            let hanging = api::virtual_call(rt, HANGING_POS, &[e]);
            let h = at(rt, hanging);
            [h[0] - lo[0], h[1] - lo[1], h[2] - lo[2]]
        } else {
            v.map(|c| c.floor() as i32)
        };
        let pos = crate::api_common::vec3(rt, v[0], v[1], v[2]);
        let block = block_pos(rt, block);
        let copy = crate::tags::copy(rt, tag);
        out.push(crate::records::make_data(
            rt,
            crate::records::STRUCTURE_ENTITY_INFO,
            ENTITY_INFO_INIT,
            &[pos, block, copy],
        ));
    }
    out
}

/// `filterBlocks(pos, settings, block[, relative])`: the blocks of that block in the palette the
/// settings pick, at their place in the world (relative) or in the template, inside the settings'
/// box, each state turned.
fn filter_blocks(
    rt: &mut Rt,
    t: V,
    [pos, settings, block]: [V; 3],
    relative_to_world: bool,
    class: &'static str,
) -> V {
    let mut out = Vec::new();
    let palettes = list_field(rt, t, "palettes");
    if !crate::jutil::elements(rt, palettes).is_empty() {
        let palette = random_palette(rt, settings, palettes, pos);
        let name = api::block_name(rt, block);
        let clip = clip_box(rt, settings);
        let rotation = setting(rt, settings, "rotation");
        let origin = at(rt, pos);
        let blocks = list_field(rt, palette, "blocks");
        for info in crate::jutil::elements(rt, blocks) {
            let (p, state, nbt) = info_parts(rt, info);
            if block_of(rt, state) != name {
                continue;
            }
            let here = if relative_to_world {
                let q = at(rt, p);
                let r = relative(rt, settings, q);
                add(r, origin)
            } else {
                at(rt, p)
            };
            if clip.is_some_and(|b| !inside(b, here)) {
                continue;
            }
            let placed = if relative_to_world {
                block_pos(rt, here)
            } else {
                p
            };
            let turned = crate::rotation::turn_state(rt, state, rotation, false);
            out.push(block_info(rt, placed, turned, nbt));
        }
    }
    rt.alloc(class, Data::List(out))
}

/// `processBlockInfos(level, offset, pos, settings, infos, template)`: each block moved to the
/// world and run through the processors (`process`; dropped at the first that gives null), then
/// each processor's `finalizeProcessing` over the whole.
fn process_block_infos(rt: &mut Rt, [level, offset, pos, settings, infos, template]: [V; 6]) -> V {
    let processors = setting(rt, settings, "processors");
    let processors = crate::jutil::elements(rt, processors);
    let off = at(rt, offset);
    let (mut originals, mut processed) = (Vec::new(), Vec::new());
    for info in crate::jutil::elements(rt, infos) {
        let (p, state, nbt) = info_parts(rt, info);
        let q = at(rt, p);
        let moved = add(relative(rt, settings, q), off);
        let moved = block_pos(rt, moved);
        let nbt = if nbt.r() != 0 {
            crate::tags::copy(rt, nbt)
        } else {
            V::NULL
        };
        let mut current = block_info(rt, moved, state, nbt);
        for &processor in &processors {
            if current.r() == 0 {
                break;
            }
            current = api::virtual_call(
                rt,
                PROCESS,
                &[
                    processor, level, offset, pos, info, current, settings, template,
                ],
            );
            if rt.thrown.is_some() {
                return V::NULL;
            }
        }
        if current.r() != 0 {
            processed.push(current);
            originals.push(info);
        }
    }
    let originals = crate::jutil::new_list(rt, originals);
    let mut list = crate::jutil::new_list(rt, processed);
    for processor in processors {
        list = api::virtual_call(
            rt,
            FINALIZE,
            &[processor, level, offset, pos, originals, list, settings],
        );
    }
    list
}

/// `processEntityInfos(template, level, offset, settings, infos)` (NeoForge's): each entity moved to
/// the world and run through the processors' `processEntity`.
fn process_entity_infos(rt: &mut Rt, [template, level, offset, settings, infos]: [V; 5]) -> V {
    let processors = setting(rt, settings, "processors");
    let processors = crate::jutil::elements(rt, processors);
    let (m, r, pivot) = turn_of(rt, settings);
    let off = at(rt, offset);
    let mut out = Vec::new();
    for info in crate::jutil::elements(rt, infos) {
        let (pos, block, nbt) = (
            rt.get_field(info, "pos"),
            rt.get_field(info, "blockPos"),
            rt.get_field(info, "nbt"),
        );
        let v = transform_point(crate::api_common::xyz(rt, pos), m, r, pivot);
        let v = crate::api_common::vec3(
            rt,
            v[0] + f64::from(off[0]),
            v[1] + f64::from(off[1]),
            v[2] + f64::from(off[2]),
        );
        let b = add(transform(at(rt, block), m, r, pivot), off);
        let b = block_pos(rt, b);
        let mut current = crate::records::make_data(
            rt,
            crate::records::STRUCTURE_ENTITY_INFO,
            ENTITY_INFO_INIT,
            &[v, b, nbt],
        );
        for &processor in &processors {
            current = api::virtual_call(
                rt,
                PROCESS_ENTITY,
                &[processor, level, offset, info, current, settings, template],
            );
            if current.r() == 0 {
                break;
            }
        }
        if current.r() != 0 {
            out.push(current);
        }
    }
    crate::jutil::new_list(rt, out)
}

fn is_source(rt: &mut Rt, fluid: V) -> bool {
    fluid.r() != 0 && api::call(rt, IS_SOURCE, &[fluid]).i() != 0
}

/// Whether the state's block takes a liquid (`LiquidBlockContainer`): a mod's block of that kind, and
/// vanilla's that can be waterlogged (its others, kelp and seagrass, take none).
fn takes_liquid(rt: &Rt, state: V) -> bool {
    let t = text(rt, state);
    if crate::host::split_state(&t)
        .1
        .iter()
        .any(|(k, _)| *k == "waterlogged")
    {
        return true;
    }
    crate::glue::mod_block(rt, &t).is_some_and(|(b, _)| {
        rt.instance_of(b, "net/minecraft/world/level/block/LiquidBlockContainer")
    })
}

/// `placeLiquid(level, pos, state, fluid)`: the mod block's own, else `SimpleWaterloggedBlock`'s:
/// a dry one takes water's source (on a server: waterlogged, its water ticking).
fn place_liquid(rt: &mut Rt, level: V, pos: V, state: V, fluid: V) {
    let t = text(rt, state);
    if let Some((b, class)) = crate::glue::mod_block(rt, &t)
        && rt.overrides.is_some_and(|o| o(class, PLACE_LIQUID))
        && let Some(vc) = rt.vcall
    {
        vc(
            rt,
            PLACE_LIQUID_KEY,
            PLACE_LIQUID,
            &[b, level, pos, state, fluid],
        );
        return;
    }
    let (block, props) = crate::host::split_state(&t);
    if !props
        .iter()
        .any(|(k, v)| *k == "waterlogged" && *v == "false")
        || fluid.r() == 0
    {
        return;
    }
    let kind = rt.get_field(fluid, "$fluid");
    if crate::fluids::name_of(rt, kind) != "minecraft:water"
        || api::virtual_call(rt, IS_CLIENT_SIDE, &[level]).i() != 0
    {
        return;
    }
    let wet: Vec<(&str, &str)> = props
        .iter()
        .map(|&(k, v)| {
            if k == "waterlogged" {
                (k, "true")
            } else {
                (k, v)
            }
        })
        .collect();
    let wet = crate::host::join_state(block, wet);
    let wet = api::state(rt, &wet);
    api::virtual_call(rt, SET_BLOCK, &[level, pos, wet, V::I(3)]);
    let water = crate::fluids::fluid(rt, "minecraft:water");
    api::virtual_call(rt, SCHEDULE_FLUID_TICK, &[level, pos, water, V::I(5)]);
}

/// `DiscreteVoxelShape.forAllFaces` over a box of `size` whose filled cells `full` says: each outer
/// face (direction, cell), along z, then y, then x, as vanilla walks them.
fn for_all_faces(
    size: [i32; 3],
    full: impl Fn([i32; 3]) -> bool,
    mut face: impl FnMut(usize, [i32; 3]),
) {
    // `AxisCycle`: a cycle's axis and a cell's coordinate on an axis (none, forward, backward).
    let cycle_axis = |c: usize, axis: usize| (axis + c) % 3;
    let cycle = |c: usize, [x, y, z]: [i32; 3], axis: usize| match c {
        1 => [z, x, y][axis],
        2 => [y, z, x][axis],
        _ => [x, y, z][axis],
    };
    for rotation in [0, 1, 2] {
        let ac = [0, 2, 1][rotation];
        let along = cycle_axis(ac, 2);
        let (n, m, k) = (
            size[cycle_axis(ac, 0)],
            size[cycle_axis(ac, 1)],
            size[along],
        );
        let (negative, positive) = [(4, 5), (0, 1), (2, 3)][along];
        for l in 0..n {
            for i1 in 0..m {
                let mut was = false;
                for j1 in 0..=k {
                    let cell = |j: i32| [0, 1, 2].map(|axis| cycle(ac, [l, i1, j], axis));
                    let is = j1 != k && full(cell(j1));
                    if !was && is {
                        face(negative, cell(j1));
                    }
                    if was && !is {
                        face(positive, cell(j1 - 1));
                    }
                    was = is;
                }
            }
        }
    }
}

/// `updateShapeAtEdge`: across each outer face of what was placed, the block inside and the one
/// outside take each other's shape.
fn update_shape_at_edge(
    rt: &mut Rt,
    level: V,
    flags: i32,
    placed: &[([i32; 3], V)],
    lo: [i32; 3],
    hi: [i32; 3],
) {
    let size = [hi[0] - lo[0] + 1, hi[1] - lo[1] + 1, hi[2] - lo[2] + 1];
    let index = |c: [i32; 3]| ((c[0] * size[1] + c[1]) * size[2] + c[2]) as usize;
    let mut cells = vec![false; (size[0] as usize) * (size[1] as usize) * (size[2] as usize)];
    for (p, _) in placed {
        cells[index([p[0] - lo[0], p[1] - lo[1], p[2] - lo[2]])] = true;
    }
    let mut faces = Vec::new();
    for_all_faces(size, |c| cells[index(c)], |d, c| faces.push((d, c)));
    for (d, c) in faces {
        let p = add(lo, c);
        let (pv, nv) = (block_pos(rt, p), block_pos(rt, step(p, d)));
        let state = api::virtual_call(rt, GET_BLOCK_STATE, &[level, pv]);
        let neighbour = api::virtual_call(rt, GET_BLOCK_STATE, &[level, nv]);
        let dir = api::direction(rt, d as i64);
        let shaped = api::update_shape(rt, [state, dir, neighbour, level, pv, nv]);
        if !same_state(rt, state, shaped) {
            api::virtual_call(rt, SET_BLOCK, &[level, pv, shaped, V::I(flags & !1)]);
        }
        let back = api::direction(rt, OPPOSITE[d] as i64);
        let other = api::update_shape(rt, [neighbour, back, shaped, level, nv, pv]);
        if !same_state(rt, neighbour, other) {
            api::virtual_call(rt, SET_BLOCK, &[level, nv, other, V::I(flags & !1)]);
        }
    }
}

/// `placeInWorld(level, offset, pos, settings, random, flags)`.
fn place_in_world(
    rt: &mut Rt,
    t: V,
    [level, offset, pos, settings, random]: [V; 5],
    flags: i32,
) -> bool {
    let palettes = list_field(rt, t, "palettes");
    if crate::jutil::elements(rt, palettes).is_empty() {
        return false;
    }
    let palette = random_palette(rt, settings, palettes, offset);
    let blocks = list_field(rt, palette, "blocks");
    let entities = list_field(rt, t, "entityInfoList");
    let has_blocks = !crate::jutil::elements(rt, blocks).is_empty();
    let ignore_entities = flag(rt, settings, "ignoreEntities");
    let has_entities = !ignore_entities && !crate::jutil::elements(rt, entities).is_empty();
    if !(has_blocks || has_entities) || size_of(rt, t).iter().any(|&n| n < 1) {
        return false;
    }
    let clip = clip_box(rt, settings);
    let waterlog = waterlogs(rt, settings);
    let (mirror, rotation) = (
        setting(rt, settings, "mirror"),
        setting(rt, settings, "rotation"),
    );
    let (mut flowing, mut sources, mut placed) = (Vec::new(), Vec::new(), Vec::new());
    let (mut lo, mut hi) = ([i32::MAX; 3], [i32::MIN; 3]);
    let processed = process_block_infos(rt, [level, offset, pos, settings, blocks, t]);
    for info in crate::jutil::elements(rt, processed) {
        if rt.thrown.is_some() {
            return false;
        }
        let (pv, state, nbt) = info_parts(rt, info);
        let p = at(rt, pv);
        if clip.is_some_and(|b| !inside(b, p)) {
            continue;
        }
        let fluid = waterlog.then(|| api::virtual_call(rt, GET_FLUID_STATE, &[level, pv]));
        let mirrored = crate::rotation::turn_state(rt, state, mirror, true);
        let state = crate::rotation::turn_state(rt, mirrored, rotation, false);
        if nbt.r() != 0 {
            let be = api::virtual_call(rt, GET_BLOCK_ENTITY, &[level, pv]);
            api::call(rt, TRY_CLEAR, &[be]);
            let barrier = api::state(rt, "minecraft:barrier");
            api::virtual_call(rt, SET_BLOCK, &[level, pv, barrier, V::I(20)]);
        }
        if api::virtual_call(rt, SET_BLOCK, &[level, pv, state, V::I(flags)]).i() == 0 {
            continue;
        }
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
        placed.push((p, nbt));
        if nbt.r() != 0 {
            let be = api::virtual_call(rt, GET_BLOCK_ENTITY, &[level, pv]);
            if be.r() != 0 {
                if rt.instance_of(be, "net/minecraft/world/RandomizableContainer") {
                    let seed = api::virtual_call(rt, NEXT_LONG, &[random]);
                    let key = rt.string("LootTableSeed");
                    api::call(rt, PUT_LONG, &[nbt, key, seed]);
                }
                let registries = api::virtual_call(rt, REGISTRY_ACCESS, &[level]);
                api::call(rt, LOAD_WITH_COMPONENTS, &[be, nbt, registries]);
            }
        }
        if let Some(fluid) = fluid {
            let own = api::call(rt, STATE_FLUID, &[state]);
            if is_source(rt, own) {
                sources.push(p);
            } else if takes_liquid(rt, state) {
                place_liquid(rt, level, pv, state, fluid);
                if !is_source(rt, fluid) {
                    flowing.push(p);
                }
            }
        }
    }
    // A waterloggable block placed in flowing water takes a source beside it (above, north, east,
    // south, west) that the template didn't place.
    let mut changed = true;
    while changed && !flowing.is_empty() {
        changed = false;
        let mut i = 0;
        while i < flowing.len() {
            let p = flowing[i];
            let pv = block_pos(rt, p);
            let mut fluid = api::virtual_call(rt, GET_FLUID_STATE, &[level, pv]);
            for d in [1, 2, 5, 3, 4] {
                if is_source(rt, fluid) {
                    break;
                }
                let n = step(p, d);
                let nv = block_pos(rt, n);
                let there = api::virtual_call(rt, GET_FLUID_STATE, &[level, nv]);
                if is_source(rt, there) && !sources.contains(&n) {
                    fluid = there;
                }
            }
            if is_source(rt, fluid) {
                let state = api::virtual_call(rt, GET_BLOCK_STATE, &[level, pv]);
                if takes_liquid(rt, state) {
                    place_liquid(rt, level, pv, state, fluid);
                    changed = true;
                    flowing.remove(i);
                    continue;
                }
            }
            i += 1;
        }
    }
    if lo[0] <= hi[0] {
        let known = flag(rt, settings, "knownShape");
        if !known {
            update_shape_at_edge(rt, level, flags, &placed, lo, hi);
        }
        for (p, nbt) in placed {
            let pv = block_pos(rt, p);
            if !known {
                let state = api::virtual_call(rt, GET_BLOCK_STATE, &[level, pv]);
                let shaped = api::call(rt, FROM_NEIGHBOUR_SHAPES, &[state, level, pv]);
                if !same_state(rt, state, shaped) {
                    api::virtual_call(rt, SET_BLOCK, &[level, pv, shaped, V::I((flags & !1) | 16)]);
                }
                let block = api::call(rt, GET_BLOCK, &[shaped]);
                api::virtual_call(rt, BLOCK_UPDATED, &[level, pv, block]);
            }
            if nbt.r() != 0 {
                let be = api::virtual_call(rt, GET_BLOCK_ENTITY, &[level, pv]);
                if be.r() != 0 {
                    api::virtual_call(rt, SET_CHANGED, &[be]);
                }
            }
        }
    }
    if !ignore_entities {
        add_entities(rt, t, level, offset, settings);
    }
    true
}

/// `addEntitiesToWorld`: each entity (inside the settings' box) made from its data at its new place
/// (a new UUID), turned and mirrored, finalized as a structure's mob when the settings say so.
fn add_entities(rt: &mut Rt, t: V, level: V, offset: V, settings: V) {
    let infos = list_field(rt, t, "entityInfoList");
    let processed = process_entity_infos(rt, [t, level, offset, settings, infos]);
    let clip = clip_box(rt, settings);
    let (mirror, rotation) = (
        setting(rt, settings, "mirror"),
        setting(rt, settings, "rotation"),
    );
    let finalize = flag(rt, settings, "finalizeEntities");
    for info in crate::jutil::elements(rt, processed) {
        let b = rt.get_field(info, "blockPos");
        if clip.is_some_and(|c| !inside(c, at(rt, b))) {
            continue;
        }
        let nbt = rt.get_field(info, "nbt");
        let tag = crate::tags::copy(rt, nbt);
        let v = crate::api_common::xyz(rt, rt.get_field(info, "pos"));
        let list = crate::nbt::element_to_runtime(
            rt,
            &Nbt::List(v.iter().map(|&c| Nbt::Double(c)).collect()),
        )
        .unwrap_or(V::NULL);
        let (pos_key, uuid_key) = (rt.string("Pos"), rt.string("UUID"));
        api::call(rt, PUT_TAG, &[tag, pos_key, list]);
        api::call(rt, REMOVE_KEY, &[tag, uuid_key]);
        let server = api::virtual_call(rt, GET_LEVEL, &[level]);
        let made = api::call(rt, CREATE_ENTITY, &[tag, server]);
        // `createEntityIgnoreException`: an entity that fails to load is left out.
        if rt.thrown.is_some() {
            rt.thrown = None;
            continue;
        }
        let e = if made.r() == 0 {
            V::NULL
        } else {
            rt.get_field(made, "value")
        };
        if e.r() == 0 {
            continue;
        }
        let turned = api::virtual_call(rt, ENTITY_ROTATE, &[e, rotation]).f();
        let mirrored = api::virtual_call(rt, ENTITY_MIRROR, &[e, mirror]).f();
        let y = api::virtual_call(rt, GET_Y_ROT, &[e]).f();
        let x = api::virtual_call(rt, GET_X_ROT, &[e]).f();
        api::virtual_call(
            rt,
            MOVE_TO,
            &[
                e,
                V::D(v[0]),
                V::D(v[1]),
                V::D(v[2]),
                V::F(turned + mirrored - y),
                V::F(x),
            ],
        );
        if finalize && rt.instance_of(e, "net/minecraft/world/entity/Mob") {
            let at_block = block_pos(rt, v.map(|c| c.floor() as i32));
            let difficulty = api::virtual_call(rt, DIFFICULTY_AT, &[level, at_block]);
            let kind = api::get_static(rt, MOB_SPAWN_STRUCTURE);
            api::virtual_call(rt, FINALIZE_SPAWN, &[e, level, difficulty, kind, V::NULL]);
        }
        api::virtual_call(rt, ADD_ENTITY, &[level, e]);
    }
}

// ---- saving and loading ----

fn int_list(p: [i32; 3]) -> Nbt {
    Nbt::List(p.iter().map(|&c| Nbt::Int(c)).collect())
}

/// A list tag's elements when it is one (`getList(key, kind)` gives an empty one otherwise).
fn list_of(n: Option<&Nbt>) -> &[Nbt] {
    match n {
        Some(Nbt::List(items)) => items,
        _ => &[],
    }
}

/// `getList(key, 3)` read as three ints (`getInt(i)`: 0 where it has none).
fn ints(n: Option<&Nbt>) -> [i32; 3] {
    let items = list_of(n);
    if !items.iter().all(|i| matches!(i, Nbt::Int(_))) {
        return [0; 3];
    }
    [0, 1, 2].map(|i| match items.get(i) {
        Some(Nbt::Int(v)) => *v,
        _ => 0,
    })
}

fn doubles(n: Option<&Nbt>) -> [f64; 3] {
    let items = list_of(n);
    if !items.iter().all(|i| matches!(i, Nbt::Double(_))) {
        return [0.0; 3];
    }
    [0, 1, 2].map(|i| match items.get(i) {
        Some(Nbt::Double(v)) => *v,
        _ => 0.0,
    })
}

/// A list element as `getCompound(i)` reads it: an empty compound for anything else.
fn compound(n: &Nbt) -> BTreeMap<String, Nbt> {
    match n {
        Nbt::Compound(c) => c.clone(),
        _ => BTreeMap::new(),
    }
}

/// `CompoundTag.getInt(key)`: a number of any kind, else 0.
fn int_at(c: &BTreeMap<String, Nbt>, key: &str) -> i32 {
    match c.get(key) {
        Some(Nbt::Byte(v)) => i32::from(*v),
        Some(Nbt::Short(v)) => i32::from(*v),
        Some(Nbt::Int(v)) => *v,
        Some(Nbt::Long(v)) => *v as i32,
        Some(Nbt::Float(v)) => v.floor() as i32,
        Some(Nbt::Double(v)) => v.floor() as i32,
        _ => 0,
    }
}

/// `NbtUtils.writeBlockState`: `Name`, and `Properties` when it has any.
fn state_nbt(state: &str) -> Nbt {
    let (block, props) = crate::host::split_state(state);
    let mut c = BTreeMap::from([("Name".to_owned(), Nbt::String(block.to_owned()))]);
    if !props.is_empty() {
        let p = props
            .iter()
            .map(|(k, v)| ((*k).to_owned(), Nbt::String((*v).to_owned())))
            .collect();
        c.insert("Properties".to_owned(), Nbt::Compound(p));
    }
    Nbt::Compound(c)
}

/// `NbtUtils.readBlockState`: air without a name; the properties given as text.
fn state_text(c: &BTreeMap<String, Nbt>) -> String {
    let Some(Nbt::String(name)) = c.get("Name") else {
        return "minecraft:air".to_owned();
    };
    let props: Vec<(&str, &str)> = match c.get("Properties") {
        Some(Nbt::Compound(p)) => p
            .iter()
            .filter_map(|(k, v)| match v {
                Nbt::String(s) => Some((k.as_str(), s.as_str())),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    crate::host::join_state(name, props)
}

/// `save(tag)`: the blocks of the first palette by position, state (its id in the palette, in the
/// order first met) and data; the palette (or each palette, its ids the first one's); the entities;
/// the size and the data version. Written into `tag`.
fn save(rt: &mut Rt, t: V, tag: V) {
    let mut out = BTreeMap::new();
    let palettes = list_field(rt, t, "palettes");
    let palettes = crate::jutil::elements(rt, palettes);
    if palettes.is_empty() {
        out.insert("blocks".to_owned(), Nbt::List(Vec::new()));
        out.insert("palette".to_owned(), Nbt::List(Vec::new()));
    } else {
        let lists: Vec<Vec<V>> = palettes
            .iter()
            .map(|&p| {
                let l = list_field(rt, p, "blocks");
                crate::jutil::elements(rt, l)
            })
            .collect();
        let mut first: Vec<String> = Vec::new();
        let mut others: Vec<Vec<Option<String>>> = vec![Vec::new(); palettes.len()];
        let mut blocks = Vec::new();
        for (j, &info) in lists[0].iter().enumerate() {
            let (p, state, nbt) = info_parts(rt, info);
            let s = text(rt, state);
            let id = match first.iter().position(|f| *f == s) {
                Some(id) => id,
                None => {
                    first.push(s);
                    first.len() - 1
                }
            };
            let mut b = BTreeMap::from([
                ("pos".to_owned(), int_list(at(rt, p))),
                ("state".to_owned(), Nbt::Int(id as i32)),
            ]);
            if nbt.r() != 0 {
                b.insert(
                    "nbt".to_owned(),
                    Nbt::Compound(crate::nbt::from_runtime(rt, nbt)),
                );
            }
            blocks.push(Nbt::Compound(b));
            for (l, list) in lists.iter().enumerate().skip(1) {
                if let Some(&other) = list.get(j) {
                    let s = text(rt, rt.get_field(other, "state"));
                    let ids = &mut others[l];
                    if ids.len() <= id {
                        ids.resize(id + 1, None);
                    }
                    ids[id] = Some(s);
                }
            }
        }
        out.insert("blocks".to_owned(), Nbt::List(blocks));
        let first = Nbt::List(first.iter().map(|s| state_nbt(s)).collect());
        if palettes.len() == 1 {
            out.insert("palette".to_owned(), first);
        } else {
            let mut all = vec![first];
            all.extend(
                others
                    .iter()
                    .skip(1)
                    .map(|ids| Nbt::List(ids.iter().flatten().map(|s| state_nbt(s)).collect())),
            );
            out.insert("palettes".to_owned(), Nbt::List(all));
        }
    }
    let entities = list_field(rt, t, "entityInfoList");
    let mut list = Vec::new();
    for e in crate::jutil::elements(rt, entities) {
        let (pos, block, nbt) = (
            rt.get_field(e, "pos"),
            rt.get_field(e, "blockPos"),
            rt.get_field(e, "nbt"),
        );
        let v = crate::api_common::xyz(rt, pos);
        let mut c = BTreeMap::from([
            (
                "pos".to_owned(),
                Nbt::List(v.iter().map(|&d| Nbt::Double(d)).collect()),
            ),
            ("blockPos".to_owned(), int_list(at(rt, block))),
        ]);
        if nbt.r() != 0 {
            c.insert(
                "nbt".to_owned(),
                Nbt::Compound(crate::nbt::from_runtime(rt, nbt)),
            );
        }
        list.push(Nbt::Compound(c));
    }
    out.insert("entities".to_owned(), Nbt::List(list));
    out.insert("size".to_owned(), int_list(size_of(rt, t)));
    out.insert("DataVersion".to_owned(), Nbt::Int(DATA_VERSION));
    let fresh = crate::nbt::to_runtime(rt, &out);
    let entries: Vec<(String, V)> = match rt.heap.get(fresh.r() as usize).map(|o| &o.data) {
        Some(Data::Map(m)) => m.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        _ => Vec::new(),
    };
    if let Some(Data::Map(m)) = rt.heap.get_mut(tag.r() as usize).map(|o| &mut o.data) {
        m.extend(entries);
    }
}

/// `load(blocks, tag)`: what `save` wrote, back (a palette's missing state is air); an entity
/// without data is left out.
fn load(rt: &mut Rt, t: V, tag: V) {
    let root = crate::nbt::from_runtime(rt, tag);
    let size = vec3i(rt, ints(root.get("size")));
    rt.put_field(t, "size", size);
    let blocks: Vec<BTreeMap<String, Nbt>> = match root.get("blocks") {
        Some(Nbt::List(items)) if items.iter().all(|i| matches!(i, Nbt::Compound(_))) => {
            items.iter().map(compound).collect()
        }
        _ => Vec::new(),
    };
    let mut palettes = Vec::new();
    match root.get("palettes") {
        Some(Nbt::List(lists)) => {
            for l in lists {
                let entries = match l {
                    Nbt::List(e) => e.as_slice(),
                    _ => &[],
                };
                palettes.push(load_palette(rt, entries, &blocks));
            }
        }
        _ => {
            let entries = match root.get("palette") {
                Some(Nbt::List(e)) if e.iter().all(|i| matches!(i, Nbt::Compound(_))) => {
                    e.as_slice()
                }
                _ => &[],
            };
            palettes.push(load_palette(rt, entries, &blocks));
        }
    }
    let list = list_field(rt, t, "palettes");
    set_items(rt, list, palettes);
    let mut entities = Vec::new();
    if let Some(Nbt::List(items)) = root.get("entities") {
        for c in items.iter().map(compound) {
            let Some(nbt) = c.get("nbt") else { continue };
            let v = doubles(c.get("pos"));
            let pos = crate::api_common::vec3(rt, v[0], v[1], v[2]);
            let block = block_pos(rt, ints(c.get("blockPos")));
            let nbt = crate::nbt::to_runtime(rt, &compound(nbt));
            entities.push(crate::records::make_data(
                rt,
                crate::records::STRUCTURE_ENTITY_INFO,
                ENTITY_INFO_INIT,
                &[pos, block, nbt],
            ));
        }
    }
    let list = list_field(rt, t, "entityInfoList");
    set_items(rt, list, entities);
}

/// `loadPalette`: each block's state by its id among `entries`.
fn load_palette(rt: &mut Rt, entries: &[Nbt], blocks: &[BTreeMap<String, Nbt>]) -> V {
    let states: Vec<String> = entries.iter().map(|e| state_text(&compound(e))).collect();
    let getter = api::get_static(rt, EMPTY_GETTER);
    let origin = block_pos(rt, [0; 3]);
    let mut infos = Vec::new();
    for b in blocks {
        let pos = block_pos(rt, ints(b.get("pos")));
        let id = int_at(b, "state");
        let s = usize::try_from(id)
            .ok()
            .and_then(|i| states.get(i))
            .map_or("minecraft:air", String::as_str);
        let state = api::state(rt, s);
        let nbt = match b.get("nbt") {
            Some(n) => crate::nbt::to_runtime(rt, &compound(n)),
            None => V::NULL,
        };
        let kind = list_kind(rt, state, nbt, getter, origin);
        let info = block_info(rt, pos, state, nbt);
        infos.push((kind, info));
    }
    let list = ordered(rt, infos);
    new_palette(rt, list)
}

fn template_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "<init>" if desc == "()V" => {
            init_template(rt, a(0));
            V::Void
        }
        "getSize" if desc.starts_with("()") => size_value(rt, a(0)),
        "getSize" => {
            let s = size_of(rt, a(0));
            if matches!(ordinal(rt, a(1)), 1 | 3) {
                vec3i(rt, [s[2], s[1], s[0]])
            } else {
                size_value(rt, a(0))
            }
        }
        "setAuthor" => {
            rt.put_field(a(0), "author", a(1));
            V::Void
        }
        "getAuthor" => match rt.get_field(a(0), "author") {
            V::Void => rt.string("?"),
            v => v,
        },
        "fillFromWorld" => {
            fill_from_world(rt, a(0), [a(1), a(2), a(3)], a(4).i() != 0, a(5));
            V::Void
        }
        "filterBlocks" if desc.ends_with(";Z)Lit/unimi/dsi/fastutil/objects/ObjectArrayList;") => {
            filter_blocks(
                rt,
                a(0),
                [a(1), a(2), a(3)],
                a(4).i() != 0,
                OBJECT_ARRAY_LIST,
            )
        }
        "filterBlocks" => filter_blocks(rt, a(0), [a(1), a(2), a(3)], true, "java/util/ArrayList"),
        "calculateConnectedPosition" => {
            let (p, q) = (at(rt, a(2)), at(rt, a(4)));
            let (p, q) = (relative(rt, a(1), p), relative(rt, a(3), q));
            block_pos(rt, [p[0] - q[0], p[1] - q[1], p[2] - q[2]])
        }
        "calculateRelativePosition" => {
            let p = at(rt, a(1));
            let r = relative(rt, a(0), p);
            block_pos(rt, r)
        }
        "transformedVec3d" => {
            let (m, r, pivot) = turn_of(rt, a(0));
            let v = transform_point(crate::api_common::xyz(rt, a(1)), m, r, pivot);
            crate::api_common::vec3(rt, v[0], v[1], v[2])
        }
        "placeInWorld" => V::I(i32::from(place_in_world(
            rt,
            a(0),
            [a(1), a(2), a(3), a(4), a(5)],
            a(6).i(),
        ))),
        "processBlockInfos" => process_block_infos(
            rt,
            [
                a(0),
                a(1),
                a(2),
                a(3),
                a(4),
                args.get(5)
                    .copied()
                    .filter(|v| *v != V::Void)
                    .unwrap_or(V::NULL),
            ],
        ),
        "processEntityInfos" => process_entity_infos(rt, [a(0), a(1), a(2), a(3), a(4)]),
        // `transform(pos, mirror, rotation, pivot)`: the same position when nothing changes it.
        "transform" if desc.starts_with("(Lnet/minecraft/core/BlockPos;") => {
            let (m, r) = (ordinal(rt, a(1)), ordinal(rt, a(2)));
            if m == 0 && r == 0 {
                return Some(a(0));
            }
            let p = transform(at(rt, a(0)), m, r, at(rt, a(3)));
            block_pos(rt, p)
        }
        "transform" => {
            let (m, r) = (ordinal(rt, a(1)), ordinal(rt, a(2)));
            if m == 0 && r == 0 {
                return Some(a(0));
            }
            let v = transform_point(crate::api_common::xyz(rt, a(0)), m, r, at(rt, a(3)));
            crate::api_common::vec3(rt, v[0], v[1], v[2])
        }
        "getZeroPositionWithTransform" if desc.contains(";II)") => {
            let p = zero_position(
                at(rt, a(0)),
                ordinal(rt, a(1)),
                ordinal(rt, a(2)),
                a(3).i(),
                a(4).i(),
            );
            block_pos(rt, p)
        }
        "getZeroPositionWithTransform" => {
            let s = size_of(rt, a(0));
            let p = zero_position(
                at(rt, a(1)),
                ordinal(rt, a(2)),
                ordinal(rt, a(3)),
                s[0],
                s[2],
            );
            block_pos(rt, p)
        }
        "getBoundingBox" => {
            let (start, rotation, pivot, mirror, size) = match desc {
                d if d.starts_with("(Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructurePlaceSettings;") => {
                    let (m, r, pivot) = turn_of(rt, a(1));
                    (at(rt, a(2)), r, pivot, m, size_of(rt, a(0)))
                }
                d if d.contains("Lnet/minecraft/core/Vec3i;)") => (at(rt, a(0)), ordinal(rt, a(1)), at(rt, a(2)), ordinal(rt, a(3)), at(rt, a(4))),
                _ => (at(rt, a(1)), ordinal(rt, a(2)), at(rt, a(3)), ordinal(rt, a(4)), size_of(rt, a(0))),
            };
            crate::shapes::bounding_box(rt, bounds(start, rotation, pivot, mirror, size))
        }
        "save" => {
            save(rt, a(0), a(1));
            a(1)
        }
        "load" => {
            load(rt, a(0), a(2));
            V::Void
        }
        _ => return None,
    })
}

fn palette_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let blocks = list_field(rt, a(0), "blocks");
    Some(match (name, desc) {
        ("blocks", "()Ljava/util/List;") => blocks,
        ("blocks", "(Lnet/minecraft/world/level/block/Block;)Ljava/util/List;") => {
            let name = api::block_name(rt, a(1));
            let all = crate::jutil::elements(rt, blocks);
            let found: Vec<V> = all
                .into_iter()
                .filter(|&i| block_of(rt, rt.get_field(i, "state")) == name)
                .collect();
            crate::jutil::new_list(rt, found)
        }
        _ => return None,
    })
}

// ---- the server's templates ----

fn manager(rt: &mut Rt) -> V {
    rt.intern("compat:structure-manager".into(), |rt| {
        rt.new_object(MANAGER)
    })
}

fn kept_key(rt: &Rt, id: V) -> String {
    format!("compat:structure/{}", crate::neo::location(rt, id))
}

fn manager_call(rt: &mut Rt, name: &str, desc: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    Some(match name {
        "getOrCreate" => {
            let key = kept_key(rt, a(1));
            if let Some(&t) = rt.interned.get(&key) {
                return Some(t);
            }
            let t = new_template(rt);
            rt.interned.insert(key, t);
            t
        }
        "get" if desc == "(Lnet/minecraft/resources/ResourceLocation;)Ljava/util/Optional;" => {
            let key = kept_key(rt, a(1));
            let t = rt.interned.get(&key).copied().unwrap_or(V::NULL);
            crate::api_common::optional(rt, t)
        }
        "readStructure" if desc.starts_with("(Lnet/minecraft/nbt/CompoundTag;)") => {
            let t = new_template(rt);
            load(rt, t, a(1));
            t
        }
        _ => return None,
    })
}

/// `Entity.rotate(rotation)` and `mirror(mirror)`: the yaw a template turns an entity to
/// (`Mth.wrapDegrees` of its own, plus the turn; mirrored about the axis).
fn entity_turn(rt: &mut Rt, name: &str, args: &[V]) -> V {
    let (e, how) = (
        args.first().copied().unwrap_or_default(),
        args.get(1).copied().unwrap_or_default(),
    );
    let y = api::virtual_call(rt, GET_Y_ROT, &[e]).f();
    let mut f = y % 360.0;
    if f >= 180.0 {
        f -= 360.0;
    }
    if f < -180.0 {
        f += 360.0;
    }
    V::F(match (name, ordinal(rt, how)) {
        ("rotate", 2) => f + 180.0,
        ("rotate", 3) => f + 270.0,
        ("rotate", 1) => f + 90.0,
        ("mirror", 2) => -f,
        ("mirror", 1) => 180.0 - f,
        _ => f,
    })
}

/// The keys [`call`] may answer, by owner, name and descriptor alone: the dispatcher offers a key only
/// to the modules that claim it ([`crate::api`]'s chain), so this covers every arm, guards aside.
pub(crate) fn claims(owner: &str, name: &str, desc: &str) -> bool {
    matches!(owner, TEMPLATE | PALETTE | SETTINGS | MANAGER)
        || (owner == PROCESSOR
            && matches!(
                name,
                "<init>" | "processBlock" | "process" | "finalizeProcessing" | "processEntity"
            ))
        || (name == "getStructureManager" && desc == MANAGER_DESC)
        || (owner == ENTITY
            && ((name == "rotate" && desc == ROTATE_DESC)
                || (name == "mirror" && desc == MIRROR_DESC)))
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    if !claims(owner, name, desc) {
        return None;
    }
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match owner {
        TEMPLATE => template_call(rt, name, desc, args),
        PALETTE => palette_call(rt, name, desc, args),
        SETTINGS => settings_call(rt, name, desc, args),
        MANAGER => manager_call(rt, name, desc, args),
        PROCESSOR => Some(match name {
            "<init>" => V::Void,
            // `process` asks `processBlock` (a mod's own), which keeps the block.
            "process" => api::virtual_call(rt, PROCESS_BLOCK, &args[..args.len().min(7)]),
            "processBlock" => a(5),
            "finalizeProcessing" => a(5),
            _ => a(4),
        }),
        ENTITY => Some(entity_turn(rt, name, args)),
        _ if name == "getStructureManager" => Some(manager(rt)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    fn key(rt: &mut Rt, class: &str, method: &str) -> &'static str {
        rt.static_key(&format!("{class}.{method}"))
    }

    fn t_call(rt: &mut Rt, method: &str, args: &[V]) -> V {
        let k = key(rt, TEMPLATE, method);
        api::call(rt, k, args)
    }

    fn s_call(rt: &mut Rt, method: &str, args: &[V]) -> V {
        let k = key(rt, SETTINGS, method);
        api::call(rt, k, args)
    }

    fn rotation(rt: &mut Rt, name: &str) -> V {
        let k = rt.static_key(&format!("net/minecraft/world/level/block/Rotation.{name}"));
        api::get_static(rt, k)
    }

    fn mirror(rt: &mut Rt, name: &str) -> V {
        let k = rt.static_key(&format!("net/minecraft/world/level/block/Mirror.{name}"));
        api::get_static(rt, k)
    }

    fn states_of(rt: &Rt, template: V) -> Vec<([i32; 3], String)> {
        let palettes = crate::jutil::elements(rt, rt.get_field(template, "palettes"));
        let blocks = crate::jutil::elements(rt, rt.get_field(palettes[0], "blocks"));
        blocks
            .iter()
            .map(|&i| {
                (
                    at(rt, rt.get_field(i, "pos")),
                    text(rt, rt.get_field(i, "state")),
                )
            })
            .collect()
    }

    /// A 2×2×2 box of the world filled into a template (the air left out), saved as vanilla's
    /// structure files are, loaded back into another, and placed a quarter turn clockwise
    /// elsewhere: each block where vanilla's formula puts it, its state turned with it.
    #[test]
    fn a_template_from_the_world_saves_loads_and_places_turned() {
        let mut rt = Rt::new();
        crate::install();
        let mut host = MemoryHost::default();
        host.blocks.insert([10, 64, 10], "minecraft:stone".into());
        host.blocks.insert(
            [11, 64, 10],
            "minecraft:furnace[facing=north,lit=false]".into(),
        );
        host.blocks
            .insert([10, 64, 11], "minecraft:oak_log[axis=x]".into());
        host.blocks.insert([10, 65, 10], "minecraft:torch".into());
        // A torch has no collision shape: not a plain full block.
        host.collisions.insert([10, 65, 10], Vec::new());
        rt.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            let template = rt.new_object(TEMPLATE);
            t_call(rt, "<init>.()V", &[template]);
            let from = block_pos(rt, [10, 64, 10]);
            let size = vec3i(rt, [2, 2, 2]);
            let air = api::get_static(rt, "net/minecraft/world/level/block/Blocks.AIR");
            t_call(rt, "fillFromWorld.(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/Vec3i;ZLnet/minecraft/world/level/block/Block;)V", &[template, level, from, size, V::I(0), air]);
            // Full blocks by y, x, z, then the torch.
            let expected = vec![
                ([0, 0, 0], "minecraft:stone".to_owned()),
                ([0, 0, 1], "minecraft:oak_log[axis=x]".to_owned()),
                ([1, 0, 0], "minecraft:furnace[facing=north,lit=false]".to_owned()),
                ([0, 1, 0], "minecraft:torch".to_owned()),
            ];
            assert_eq!(states_of(rt, template), expected);

            let tag = crate::tags::new_compound(rt);
            t_call(rt, "save.(Lnet/minecraft/nbt/CompoundTag;)Lnet/minecraft/nbt/CompoundTag;", &[template, tag]);
            let saved = crate::nbt::from_runtime(rt, tag);
            let state = |name: &str, props: &[(&str, &str)]| {
                let mut c = BTreeMap::from([("Name".to_owned(), Nbt::String(name.to_owned()))]);
                if !props.is_empty() {
                    c.insert("Properties".to_owned(), Nbt::Compound(props.iter().map(|(k, v)| ((*k).to_owned(), Nbt::String((*v).to_owned()))).collect()));
                }
                Nbt::Compound(c)
            };
            assert_eq!(
                saved.get("palette"),
                Some(&Nbt::List(vec![
                    state("minecraft:stone", &[]),
                    state("minecraft:oak_log", &[("axis", "x")]),
                    state("minecraft:furnace", &[("facing", "north"), ("lit", "false")]),
                    state("minecraft:torch", &[]),
                ]))
            );
            let block = |p: [i32; 3], id: i32| Nbt::Compound(BTreeMap::from([("pos".to_owned(), Nbt::List(p.iter().map(|&c| Nbt::Int(c)).collect())), ("state".to_owned(), Nbt::Int(id))]));
            assert_eq!(saved.get("blocks"), Some(&Nbt::List(vec![block([0, 0, 0], 0), block([0, 0, 1], 1), block([1, 0, 0], 2), block([0, 1, 0], 3)])));
            assert_eq!(saved.get("size"), Some(&Nbt::List(vec![Nbt::Int(2), Nbt::Int(2), Nbt::Int(2)])));
            assert_eq!(saved.get("entities"), Some(&Nbt::List(Vec::new())));
            assert_eq!(saved.get("DataVersion"), Some(&Nbt::Int(3955)));

            let manager = api::call(rt, "net/minecraft/server/MinecraftServer.getStructureManager.()Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplateManager;", &[V::NULL]);
            let loaded = api::call(rt, "net/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplateManager.readStructure.(Lnet/minecraft/nbt/CompoundTag;)Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate;", &[manager, tag]);
            assert_eq!(states_of(rt, loaded), expected);

            let settings = rt.new_object(SETTINGS);
            s_call(rt, "<init>.()V", &[settings]);
            let cw = rotation(rt, "CLOCKWISE_90");
            s_call(rt, "setRotation.(Lnet/minecraft/world/level/block/Rotation;)Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructurePlaceSettings;", &[settings, cw]);
            let offset = block_pos(rt, [100, 70, 100]);
            let random = crate::api_common::seeded_random(rt, 1);
            let ok = t_call(rt, "placeInWorld.(Lnet/minecraft/world/level/ServerLevelAccessor;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructurePlaceSettings;Lnet/minecraft/util/RandomSource;I)Z", &[loaded, level, offset, offset, settings, random, V::I(2)]);
            assert_eq!(ok, V::I(1));
            let b = t_call(rt, "getBoundingBox.(Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructurePlaceSettings;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/level/levelgen/structure/BoundingBox;", &[loaded, settings, offset]);
            assert_eq!(crate::shapes::int_box_of(rt, b), [99, 70, 100, 100, 71, 101]);
            assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        });
        // (x, y, z) turned clockwise about the origin is (-z, y, x).
        assert_eq!(
            host.blocks.get(&[100, 70, 100]).map(String::as_str),
            Some("minecraft:stone")
        );
        assert_eq!(
            host.blocks.get(&[99, 70, 100]).map(String::as_str),
            Some("minecraft:oak_log[axis=z]")
        );
        assert_eq!(
            host.blocks.get(&[100, 70, 101]).map(String::as_str),
            Some("minecraft:furnace[facing=east,lit=false]")
        );
        assert_eq!(
            host.blocks.get(&[100, 71, 100]).map(String::as_str),
            Some("minecraft:torch")
        );
        assert_eq!(host.blocks.len(), 8);
    }

    /// Vanilla's formulas, worked by hand: a position turned and mirrored about a pivot, a point
    /// turned within its block, the corner a turned box starts from, the box a turned template
    /// takes and its size.
    #[test]
    fn templates_turn_positions_as_vanillas_formulas() {
        let mut rt = Rt::new();
        crate::install();
        const TRANSFORM: &str = "transform.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Mirror;Lnet/minecraft/world/level/block/Rotation;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/core/BlockPos;";
        let p = block_pos(&mut rt, [1, 2, 3]);
        let (zero, pivot) = (block_pos(&mut rt, [0; 3]), block_pos(&mut rt, [2, 0, 5]));
        let cases = [
            ("NONE", "CLOCKWISE_90", zero, [-3, 2, 1]),
            ("LEFT_RIGHT", "NONE", zero, [1, 2, -3]),
            ("FRONT_BACK", "COUNTERCLOCKWISE_90", pivot, [0, 2, 8]),
            ("NONE", "CLOCKWISE_180", pivot, [3, 2, 7]),
        ];
        for (m, r, around, expected) in cases {
            let (m, r) = (mirror(&mut rt, m), rotation(&mut rt, r));
            let q = t_call(&mut rt, TRANSFORM, &[p, m, r, around]);
            assert_eq!(at(&rt, q), expected);
        }
        let point = crate::api_common::vec3(&mut rt, 0.5, 1.0, 0.25);
        let (none, cw) = (mirror(&mut rt, "NONE"), rotation(&mut rt, "CLOCKWISE_90"));
        let turned = t_call(
            &mut rt,
            "transform.(Lnet/minecraft/world/phys/Vec3;Lnet/minecraft/world/level/block/Mirror;Lnet/minecraft/world/level/block/Rotation;Lnet/minecraft/core/BlockPos;)Lnet/minecraft/world/phys/Vec3;",
            &[point, none, cw, zero],
        );
        assert_eq!(crate::api_common::xyz(&rt, turned), [0.75, 1.0, 0.5]);
        let start = block_pos(&mut rt, [10, 64, 10]);
        let half = rotation(&mut rt, "CLOCKWISE_180");
        let corner = t_call(
            &mut rt,
            "getZeroPositionWithTransform.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Mirror;Lnet/minecraft/world/level/block/Rotation;II)Lnet/minecraft/core/BlockPos;",
            &[start, none, half, V::I(3), V::I(5)],
        );
        assert_eq!(at(&rt, corner), [12, 64, 14]);
        let template = rt.new_object(TEMPLATE);
        t_call(&mut rt, "<init>.()V", &[template]);
        let size = vec3i(&mut rt, [3, 4, 5]);
        rt.put_field(template, "size", size);
        let at5 = block_pos(&mut rt, [5, 0, 5]);
        let b = t_call(
            &mut rt,
            "getBoundingBox.(Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Rotation;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/Mirror;)Lnet/minecraft/world/level/levelgen/structure/BoundingBox;",
            &[template, at5, cw, zero, none],
        );
        assert_eq!(crate::shapes::int_box_of(&rt, b), [1, 0, 5, 5, 3, 7]);
        let turned = t_call(
            &mut rt,
            "getSize.(Lnet/minecraft/world/level/block/Rotation;)Lnet/minecraft/core/Vec3i;",
            &[template, cw],
        );
        assert_eq!(at(&rt, turned), [5, 4, 3]);
        assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
    }

    const STONE_TO_GRANITE: &str = "test/StoneToGranite";

    fn overrides(class: &str, key: &str) -> bool {
        class == STONE_TO_GRANITE && key.starts_with("processBlock.")
    }

    /// A mod's processor, as its own `processBlock`: stone becomes granite, a torch is left out.
    fn vcall(rt: &mut Rt, key: &'static str, short: &'static str, args: &[V]) -> V {
        let mods = args
            .first()
            .is_some_and(|&v| v.r() != 0 && rt.class_of(v) == STONE_TO_GRANITE);
        if mods && short.starts_with("processBlock.") {
            let info = args[5];
            let (pos, state, nbt) = info_parts(rt, info);
            return match text(rt, state).as_str() {
                "minecraft:torch" => V::NULL,
                "minecraft:stone" => {
                    let granite = api::state(rt, "minecraft:granite");
                    block_info(rt, pos, granite, nbt)
                }
                _ => info,
            };
        }
        api::call(rt, key, args)
    }

    /// The settings' processors run as vanilla runs them (a mod's own `processBlock`, through
    /// `process`), the settings copy what they hold, and the server keeps a template by id.
    #[test]
    fn a_mods_processor_changes_what_a_template_places() {
        let mut rt = Rt::new();
        crate::install();
        rt.overrides = Some(overrides);
        rt.vcall = Some(vcall);
        rt.supers.insert(STONE_TO_GRANITE, PROCESSOR);
        let mut host = MemoryHost::default();
        rt.with_host(&mut host, |rt| {
            let level = crate::glue::level(rt);
            let template = rt.new_object(TEMPLATE);
            t_call(rt, "<init>.()V", &[template]);
            let size = vec3i(rt, [1, 2, 1]);
            rt.put_field(template, "size", size);
            let infos = [([0, 0, 0], "minecraft:stone"), ([0, 1, 0], "minecraft:torch")].map(|(p, s)| {
                let (p, s) = (block_pos(rt, p), api::state(rt, s));
                block_info(rt, p, s, V::NULL)
            });
            let palette = new_palette(rt, infos.to_vec());
            let palettes = rt.get_field(template, "palettes");
            set_items(rt, palettes, vec![palette]);
            let settings = rt.new_object(SETTINGS);
            s_call(rt, "<init>.()V", &[settings]);
            let processor = rt.new_object(STONE_TO_GRANITE);
            api::call(rt, "net/minecraft/world/level/levelgen/structure/templatesystem/StructureProcessor.<init>.()V", &[processor]);
            s_call(rt, "addProcessor.(Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureProcessor;)Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructurePlaceSettings;", &[settings, processor]);
            let lr = mirror(rt, "LEFT_RIGHT");
            s_call(rt, "setMirror.(Lnet/minecraft/world/level/block/Mirror;)Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructurePlaceSettings;", &[settings, lr]);
            let copy = s_call(rt, "copy.()Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructurePlaceSettings;", &[settings]);
            assert_eq!(s_call(rt, "getMirror.()Lnet/minecraft/world/level/block/Mirror;", &[copy]), lr);
            let cleared = s_call(rt, "clearProcessors.()Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructurePlaceSettings;", &[copy]);
            assert_eq!(cleared, copy);
            let own = s_call(rt, "getProcessors.()Ljava/util/List;", &[settings]);
            assert_eq!(crate::jutil::elements(rt, own), vec![processor]);
            assert_eq!(s_call(rt, "shouldApplyWaterlogging.()Z", &[settings]), V::I(1));

            let offset = block_pos(rt, [0, 64, 0]);
            let random = crate::api_common::seeded_random(rt, 7);
            let ok = t_call(rt, "placeInWorld.(Lnet/minecraft/world/level/ServerLevelAccessor;Lnet/minecraft/core/BlockPos;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructurePlaceSettings;Lnet/minecraft/util/RandomSource;I)Z", &[template, level, offset, offset, settings, random, V::I(2)]);
            assert_eq!(ok, V::I(1));

            let id = rt.string("test:house");
            let manager = api::call(rt, "net/minecraft/server/level/ServerLevel.getStructureManager.()Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplateManager;", &[level]);
            let get = "net/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplateManager.get.(Lnet/minecraft/resources/ResourceLocation;)Ljava/util/Optional;";
            let none = api::call(rt, get, &[manager, id]);
            assert_eq!(rt.get_field(none, "value"), V::NULL);
            let made = api::call(rt, "net/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplateManager.getOrCreate.(Lnet/minecraft/resources/ResourceLocation;)Lnet/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate;", &[manager, id]);
            let kept = api::call(rt, get, &[manager, id]);
            assert_eq!(rt.get_field(kept, "value"), made);
            assert!(rt.unmapped.is_empty(), "{:?}", rt.unmapped);
        });
        assert_eq!(
            host.blocks.get(&[0, 64, 0]).map(String::as_str),
            Some("minecraft:granite")
        );
        assert_eq!(host.blocks.get(&[0, 65, 0]), None);
    }
}
