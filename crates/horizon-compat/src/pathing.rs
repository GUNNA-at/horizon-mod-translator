//! Mobs' paths, as a mod's mob walks them: vanilla's `PathNavigation` (`GroundPathNavigation`, the
//! one `Mob.createNavigation` makes), `Path` and `Node`, 1.21.1's A* `PathFinder` over a
//! `WalkNodeEvaluator`'s nodes (the blocks a mob fits in and stands on: it steps up a block where
//! there is room above it, drops as far as it may fall, floats or wades in water, keeps off fences,
//! lava, fire and cactus, and each node costs its `PathType`'s malus for the mob), and the random
//! places goals walk to (`DefaultRandomPos`, `LandRandomPos`).
//!
//! A navigation is the object of its class (`GroundPathNavigation`, or a mod's under it), its
//! state in fields of vanilla's names (`mob`, `path`, `speedModifier`); a path's nodes are `Node`s
//! whose public fields (`x`, `y`, `z`, `type`) mods read. Where the evaluator reads a block's
//! `isPathfindable(LAND)`, here a block is open when its collision is lower than half a block
//! (a carpet, a low snow layer) and blocked when it is higher (a slab, stairs, a chest).
use crate::{Data, Rt, V, api, jutil, mobs};

pub(crate) const NAVIGATION: &str = "net/minecraft/world/entity/ai/navigation/PathNavigation";
pub(crate) const GROUND: &str = "net/minecraft/world/entity/ai/navigation/GroundPathNavigation";
const PATH: &str = "net/minecraft/world/level/pathfinder/Path";
const NODE: &str = "net/minecraft/world/level/pathfinder/Node";
pub(crate) const PATH_TYPE: &str = "net/minecraft/world/level/pathfinder/PathType";
const DEFAULT_RANDOM_POS: &str = "net/minecraft/world/entity/ai/util/DefaultRandomPos";
const LAND_RANDOM_POS: &str = "net/minecraft/world/entity/ai/util/LandRandomPos";
const RANDOM_POS: &str = "net/minecraft/world/entity/ai/util/RandomPos";

/// `PathType`'s constants in order, with each one's default malus (`PathType.getMalus`).
pub(crate) const PATH_TYPES: [(&str, f32); 26] = [
    ("BLOCKED", -1.0),
    ("OPEN", 0.0),
    ("WALKABLE", 0.0),
    ("WALKABLE_DOOR", 0.0),
    ("TRAPDOOR", 0.0),
    ("POWDER_SNOW", -1.0),
    ("DANGER_POWDER_SNOW", 0.0),
    ("FENCE", -1.0),
    ("LAVA", -1.0),
    ("WATER", 8.0),
    ("WATER_BORDER", 8.0),
    ("RAIL", 0.0),
    ("UNPASSABLE_RAIL", -1.0),
    ("DANGER_FIRE", 8.0),
    ("DAMAGE_FIRE", 16.0),
    ("DANGER_OTHER", 8.0),
    ("DAMAGE_OTHER", -1.0),
    ("DOOR_OPEN", 0.0),
    ("DOOR_WOOD_CLOSED", -1.0),
    ("DOOR_IRON_CLOSED", -1.0),
    ("BREACH", 4.0),
    ("LEAVES", -1.0),
    ("STICKY_HONEY", 8.0),
    ("COCOA", 0.0),
    ("DAMAGE_CAUTIOUS", 0.0),
    ("DANGER_TRAPDOOR", 0.0),
];

/// `PathType`, by ordinal.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
#[repr(u8)]
pub(crate) enum Kind {
    Blocked,
    Open,
    Walkable,
    WalkableDoor,
    Trapdoor,
    PowderSnow,
    DangerPowderSnow,
    Fence,
    Lava,
    Water,
    WaterBorder,
    Rail,
    UnpassableRail,
    DangerFire,
    DamageFire,
    DangerOther,
    DamageOther,
    DoorOpen,
    DoorWoodClosed,
    DoorIronClosed,
    Breach,
    Leaves,
    StickyHoney,
    Cocoa,
    DamageCautious,
    DangerTrapdoor,
}

const KINDS: [Kind; 26] = [
    Kind::Blocked,
    Kind::Open,
    Kind::Walkable,
    Kind::WalkableDoor,
    Kind::Trapdoor,
    Kind::PowderSnow,
    Kind::DangerPowderSnow,
    Kind::Fence,
    Kind::Lava,
    Kind::Water,
    Kind::WaterBorder,
    Kind::Rail,
    Kind::UnpassableRail,
    Kind::DangerFire,
    Kind::DamageFire,
    Kind::DangerOther,
    Kind::DamageOther,
    Kind::DoorOpen,
    Kind::DoorWoodClosed,
    Kind::DoorIronClosed,
    Kind::Breach,
    Kind::Leaves,
    Kind::StickyHoney,
    Kind::Cocoa,
    Kind::DamageCautious,
    Kind::DangerTrapdoor,
];

/// The ordinal of the path type named `name` (`DANGER_FIRE`).
pub(crate) fn kind_named(name: &str) -> Option<usize> {
    PATH_TYPES.iter().position(|(n, _)| *n == name)
}

/// `nodeEvaluator.getPathType(mob, pos)`: the name of what block `p` is to a path by itself
/// (`WalkNodeEvaluator.getPathTypeStatic`).
pub(crate) fn static_kind(rt: &Rt, p: [i32; 3]) -> &'static str {
    let k = World::new(rt).kind(rt, p);
    PATH_TYPES[k as usize].0
}

/// A path type's constant.
pub(crate) fn path_type(rt: &mut Rt, i: usize) -> V {
    let i = i.min(PATH_TYPES.len() - 1);
    api::api_enum(rt, PATH_TYPE, PATH_TYPES[i].0, i as i32)
}

// ---- small helpers ----

fn int(rt: &Rt, o: V, f: &'static str) -> i32 {
    match rt.get_field(o, f) {
        V::I(i) => i,
        V::J(j) => j as i32,
        _ => 0,
    }
}

fn dbl(rt: &Rt, o: V, f: &'static str) -> f64 {
    match rt.get_field(o, f) {
        V::D(d) => d,
        V::F(x) => f64::from(x),
        V::I(i) => f64::from(i),
        _ => 0.0,
    }
}

fn obj(rt: &Rt, o: V, f: &'static str) -> V {
    match rt.get_field(o, f) {
        v @ V::R(_) => v,
        _ => V::NULL,
    }
}

fn vec3(rt: &mut Rt, p: [f64; 3]) -> V {
    crate::api_common::vec3(rt, p[0], p[1], p[2])
}

fn block_pos(rt: &mut Rt, p: [i32; 3]) -> V {
    api::block_pos(rt, i64::from(p[0]), i64::from(p[1]), i64::from(p[2]))
}

fn floor3(p: [f64; 3]) -> [i32; 3] {
    p.map(|c| c.floor() as i32)
}

/// The level's lowest block y and the first above its top (`getMinBuildHeight`,
/// `getMaxBuildHeight`).
pub(crate) fn limits(rt: &Rt) -> (i32, i32) {
    let (min, height) = rt.host().map_or((-64, 384), |h| h.world_height());
    (min, min + height)
}

// ---- the level as paths see it ----

/// What a path search reads of the world: block states and collision boxes, each asked once.
pub(crate) struct World {
    states: crate::HashMap<[i32; 3], String>,
    boxes: crate::HashMap<[i32; 3], Vec<[f64; 6]>>,
    kinds: crate::HashMap<[i32; 3], Kind>,
    pub bottom: i32,
    pub top: i32,
}

impl World {
    pub(crate) fn new(rt: &Rt) -> World {
        let (bottom, top) = limits(rt);
        World { states: Default::default(), boxes: Default::default(), kinds: Default::default(), bottom, top }
    }

    fn state(&mut self, rt: &Rt, p: [i32; 3]) -> &str {
        self.states.entry(p).or_insert_with(|| rt.host().map(|h| h.block_state(p)).unwrap_or_else(|| "minecraft:air".into()))
    }

    fn name(&mut self, rt: &Rt, p: [i32; 3]) -> String {
        let s = self.state(rt, p);
        s.split('[').next().unwrap_or(s).to_owned()
    }

    pub(crate) fn collision(&mut self, rt: &Rt, p: [i32; 3]) -> &[[f64; 6]] {
        self.boxes.entry(p).or_insert_with(|| rt.host().and_then(|h| h.collision_boxes(p)).unwrap_or_default())
    }

    /// The highest point of block `p`'s collision, from its bottom; none without one.
    fn top_of(&mut self, rt: &Rt, p: [i32; 3]) -> Option<f64> {
        self.collision(rt, p).iter().map(|b| b[4]).reduce(f64::max)
    }

    /// Whether block `p`'s collision is the whole cube (`isCollisionShapeFullBlock`).
    pub(crate) fn full(&mut self, rt: &Rt, p: [i32; 3]) -> bool {
        crate::shapes::full_block(self.collision(rt, p))
    }

    pub(crate) fn is_air(&mut self, rt: &Rt, p: [i32; 3]) -> bool {
        matches!(self.state(rt, p), "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air")
    }

    /// Whether block `p` holds water (`getFluidState().is(FluidTags.WATER)`).
    pub(crate) fn water(&mut self, rt: &Rt, p: [i32; 3]) -> bool {
        let s = self.state(rt, p);
        let name = s.split('[').next().unwrap_or(s);
        matches!(name, "minecraft:water" | "minecraft:bubble_column" | "minecraft:kelp" | "minecraft:kelp_plant" | "minecraft:seagrass" | "minecraft:tall_seagrass")
            || s.contains("waterlogged=true")
    }

    fn lava(&mut self, rt: &Rt, p: [i32; 3]) -> bool {
        self.state(rt, p).starts_with("minecraft:lava")
    }

    /// `WalkNodeEvaluator.getFloorLevel(pos)`: where one stands in block `p`: in water a mob floats
    /// in, half way up it; else on the block below's collision.
    pub(crate) fn floor_level(&mut self, rt: &Rt, p: [i32; 3], float: bool) -> f64 {
        if float && self.water(rt, p) {
            return f64::from(p[1]) + 0.5;
        }
        let below = [p[0], p[1] - 1, p[2]];
        f64::from(below[1]) + self.top_of(rt, below).unwrap_or(0.0)
    }

    /// `WalkNodeEvaluator.getPathTypeFromState`: what block `p` is to a path by itself.
    fn own_kind(&mut self, rt: &Rt, p: [i32; 3]) -> Kind {
        if self.is_air(rt, p) {
            return Kind::Open;
        }
        let name = self.name(rt, p);
        let state = self.state(rt, p).to_owned();
        let tagged = |tag: &str| rt.host().is_some_and(|h| h.block_in_tag(&name, tag));
        let path = name.split_once(':').map_or(name.as_str(), |(_, p)| p);
        let vanilla = name.starts_with("minecraft:");
        if path.ends_with("_trapdoor") || tagged("minecraft:trapdoors") || (vanilla && matches!(path, "lily_pad" | "big_dripleaf")) {
            return Kind::Trapdoor;
        }
        if vanilla {
            match path {
                "powder_snow" => return Kind::PowderSnow,
                "cactus" | "sweet_berry_bush" => return Kind::DamageOther,
                "honey_block" => return Kind::StickyHoney,
                "cocoa" => return Kind::Cocoa,
                "wither_rose" | "pointed_dripstone" => return Kind::DamageCautious,
                _ => {}
            }
        }
        if self.lava(rt, p) {
            return Kind::Lava;
        }
        let lit_campfire = path.ends_with("campfire") && state.contains("lit=true");
        if (vanilla && matches!(path, "fire" | "soul_fire" | "magma_block" | "lava_cauldron")) || lit_campfire || tagged("minecraft:fire") {
            return Kind::DamageFire;
        }
        if (path.ends_with("_door") || tagged("minecraft:doors")) && state.contains("open=") {
            return if state.contains("open=true") {
                Kind::DoorOpen
            } else if path == "iron_door" {
                Kind::DoorIronClosed
            } else {
                Kind::DoorWoodClosed
            };
        }
        if path.ends_with("rail") || tagged("minecraft:rails") {
            return Kind::Rail;
        }
        if path.ends_with("_leaves") || tagged("minecraft:leaves") {
            return Kind::Leaves;
        }
        let gate = path.ends_with("_fence_gate") || tagged("minecraft:fence_gates");
        if (path.ends_with("_fence") || path.ends_with("_wall") || tagged("minecraft:fences") || tagged("minecraft:walls")) && !gate {
            return Kind::Fence;
        }
        if gate && !state.contains("open=true") {
            return Kind::Fence;
        }
        // `isPathfindable(LAND)`: a block whose collision isn't low enough to stand in.
        let low = self.top_of(rt, p).is_none_or(|top| top < 0.5) && !self.full(rt, p);
        if !low {
            return Kind::Blocked;
        }
        if self.water(rt, p) { Kind::Water } else { Kind::Open }
    }

    /// `WalkNodeEvaluator.getPathTypeStatic`: an open block over one that isn't is walkable, unless
    /// what it stands on is to be kept off, or fire, lava, cactus or water is beside it
    /// (`checkNeighbourBlocks`).
    pub(crate) fn kind(&mut self, rt: &Rt, p: [i32; 3]) -> Kind {
        if let Some(&k) = self.kinds.get(&p) {
            return k;
        }
        let own = self.own_kind(rt, p);
        let k = if own != Kind::Open || p[1] < self.bottom + 1 {
            own
        } else {
            match self.own_kind(rt, [p[0], p[1] - 1, p[2]]) {
                Kind::Open | Kind::Water | Kind::Lava | Kind::Walkable => Kind::Open,
                Kind::DamageFire => Kind::DamageFire,
                Kind::DamageOther => Kind::DamageOther,
                Kind::StickyHoney => Kind::StickyHoney,
                Kind::PowderSnow => Kind::DangerPowderSnow,
                Kind::DamageCautious => Kind::DamageCautious,
                Kind::Trapdoor => Kind::DangerTrapdoor,
                _ => self.neighbours(rt, p),
            }
        };
        self.kinds.insert(p, k);
        k
    }

    fn neighbours(&mut self, rt: &Rt, p: [i32; 3]) -> Kind {
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if dx == 0 && dz == 0 {
                        continue;
                    }
                    match self.own_kind(rt, [p[0] + dx, p[1] + dy, p[2] + dz]) {
                        Kind::DamageOther => return Kind::DangerOther,
                        Kind::DamageFire | Kind::Lava => return Kind::DangerFire,
                        Kind::Water => return Kind::WaterBorder,
                        Kind::DamageCautious => return Kind::DamageCautious,
                        _ => {}
                    }
                }
            }
        }
        Kind::Walkable
    }
}

// ---- the mob's view of path types ----

/// What a mob makes of path type `k`: its malus (`Mob.getPathfindingMalus`: what the mob was
/// given, else the type's default); below 0, a block it never paths through.
pub(crate) fn malus(rt: &Rt, mob: V, k: usize) -> f32 {
    let (name, default) = PATH_TYPES[k.min(PATH_TYPES.len() - 1)];
    let m = rt.get_field(mob, "$malus");
    match rt.heap.get(m.r() as usize).map(|o| &o.data) {
        Some(Data::Map(map)) if m.r() != 0 => match map.get(name) {
            Some(V::F(f)) => *f,
            _ => default,
        },
        _ => default,
    }
}

/// `Mob.setPathfindingMalus(type, malus)`.
pub(crate) fn set_malus(rt: &mut Rt, mob: V, k: usize, value: f32) {
    let m = match rt.get_field(mob, "$malus") {
        v @ V::R(r) if r != 0 => v,
        _ => {
            let m = rt.alloc("compat/PathfindingMalus", Data::Map(Default::default()));
            rt.put_field(mob, "$malus", m);
            m
        }
    };
    let name = PATH_TYPES[k.min(PATH_TYPES.len() - 1)].0.to_owned();
    if let Some(Data::Map(map)) = rt.heap.get_mut(m.r() as usize).map(|o| &mut o.data) {
        map.insert(name, V::F(value));
    }
}

/// A node the evaluator found: where, its kind, and its cost (`costMalus`).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Step {
    pos: [i32; 3],
    kind: Kind,
    malus: f32,
}

/// What the evaluator knows of the mob while it searches.
#[derive(Clone)]
struct Walker {
    /// `entityWidth`, `entityHeight`: whole blocks a little over its box.
    width: i32,
    height: i32,
    bb_width: f64,
    bb_height: f64,
    can_float: bool,
    can_open_doors: bool,
    can_pass_doors: bool,
    step: i32,
    jump: f64,
    max_fall: i32,
    malus: [f32; 26],
}

impl Walker {
    fn of(rt: &mut Rt, mob: V, nav: V) -> Walker {
        let [w, h, _] = crate::mod_entities::own_size(rt, mob);
        let step = mobs::max_up_step(rt, mob);
        let mut malus = [0.0; 26];
        for (k, m) in malus.iter_mut().enumerate() {
            *m = self::malus(rt, mob, k);
        }
        Walker {
            width: (w + 1.0).floor() as i32,
            height: (h + 1.0).floor() as i32,
            bb_width: f64::from(w),
            bb_height: f64::from(h),
            can_float: can_float(rt, nav),
            can_open_doors: rt.get_field(nav, "$doors").i() != 0,
            can_pass_doors: !matches!(rt.get_field(nav, "$passDoors"), V::I(0)),
            step: f64::from(step.max(1.0)).floor() as i32,
            jump: f64::from(step).max(1.125),
            max_fall: mobs::max_fall_distance(rt, mob),
            malus,
        }
    }

    fn m(&self, k: Kind) -> f32 {
        self.malus[k as usize]
    }

    /// `getPathTypeOfMob`: the kind of the mob's footprint with its corner at `p`: a fence or a
    /// rail it can't cross first, then the first kind it never paths through, else the costliest
    /// (by ordinal among equals); a narrow mob over an open block is in the open.
    fn kind(&self, rt: &Rt, w: &mut World, p: [i32; 3]) -> Kind {
        let mut set = [false; 26];
        for dx in 0..self.width {
            for dy in 0..self.height {
                for dz in 0..self.width {
                    let mut k = w.kind(rt, [p[0] + dx, p[1] + dy, p[2] + dz]);
                    if k == Kind::DoorWoodClosed && self.can_open_doors && self.can_pass_doors {
                        k = Kind::WalkableDoor;
                    }
                    if k == Kind::DoorOpen && !self.can_pass_doors {
                        k = Kind::Blocked;
                    }
                    set[k as usize] = true;
                }
            }
        }
        if set[Kind::Fence as usize] {
            return Kind::Fence;
        }
        if set[Kind::UnpassableRail as usize] {
            return Kind::UnpassableRail;
        }
        let mut worst = Kind::Blocked;
        for (i, &present) in set.iter().enumerate() {
            if !present {
                continue;
            }
            let k = KINDS[i];
            if self.m(k) < 0.0 {
                return k;
            }
            if self.m(k) >= self.m(worst) {
                worst = k;
            }
        }
        if self.width <= 1 && worst != Kind::Open && self.m(worst) == 0.0 && w.kind(rt, p) == Kind::Open {
            return Kind::Open;
        }
        worst
    }

    fn node(&self, p: [i32; 3], k: Kind) -> Step {
        Step { pos: p, kind: k, malus: self.m(k) }
    }

    /// `WalkNodeEvaluator.findAcceptedNode`: where a step from a node whose floor is at
    /// `floor_here` into column `(x, z)` at height `y` lands: there, up a block (`up` more times
    /// at most), or down to where it may stand within its fall. None where it can't go; a node it
    /// can't stand in is none too (vanilla's blocked node, never a neighbour).
    fn accept(&self, rt: &Rt, w: &mut World, p: [i32; 3], up: i32, floor_here: f64, dir: (i32, i32)) -> Option<Step> {
        if w.floor_level(rt, p, self.can_float) - floor_here > self.jump {
            return None;
        }
        let k = self.kind(rt, w, p);
        let mut node = (self.m(k) >= 0.0).then(|| self.node(p, k));
        if k == Kind::Walkable {
            return node;
        }
        if node.is_none() && up > 0 && k != Kind::Fence && k != Kind::UnpassableRail && k != Kind::Trapdoor && k != Kind::PowderSnow {
            node = self.jump_on(rt, w, p, up, floor_here, dir);
        } else if k == Kind::Water && !self.can_float {
            // Wading: down to the water's bottom.
            let mut q = p;
            while q[1] - 1 > w.bottom {
                let below = [q[0], q[1] - 1, q[2]];
                let kb = self.kind(rt, w, below);
                if kb != Kind::Water {
                    break;
                }
                node = Some(self.node(below, kb));
                q = below;
            }
        } else if k == Kind::Open {
            node = self.ground_below(rt, w, p);
        }
        node.filter(|n| n.malus >= 0.0)
    }

    /// `tryJumpOn`: a block up, if the mob's head clears what is above it on the way.
    fn jump_on(&self, rt: &Rt, w: &mut World, p: [i32; 3], up: i32, floor_here: f64, dir: (i32, i32)) -> Option<Step> {
        let node = self.accept(rt, w, [p[0], p[1] + 1, p[2]], up - 1, floor_here, dir)?;
        if self.bb_width >= 1.0 || !matches!(node.kind, Kind::Open | Kind::Walkable) {
            return Some(node);
        }
        let (cx, cz) = (f64::from(p[0] - dir.0) + 0.5, f64::from(p[2] - dir.1) + 0.5);
        let half = self.bb_width / 2.0;
        let from = w.floor_level(rt, [cx.floor() as i32, p[1] + 1, cz.floor() as i32], self.can_float) + 0.001;
        let to = self.bb_height + w.floor_level(rt, node.pos, self.can_float) - 0.002;
        let b = [cx - half, from, cz - half, cx + half, to, cz + half];
        (!collides(rt, w, b)).then_some(node)
    }

    /// `tryFindFirstGroundNodeBelow`: down from an open block to the first it may stand on, within
    /// its fall.
    fn ground_below(&self, rt: &Rt, w: &mut World, p: [i32; 3]) -> Option<Step> {
        let mut y = p[1] - 1;
        while y >= w.bottom {
            if p[1] - y > self.max_fall {
                return None;
            }
            let q = [p[0], y, p[2]];
            let k = self.kind(rt, w, q);
            if k != Kind::Open {
                return (self.m(k) >= 0.0).then(|| self.node(q, k));
            }
            y -= 1;
        }
        None
    }

    /// `WalkNodeEvaluator.getNeighbors`: the four sides (south, west, north, east), then the
    /// diagonals between two sides it can take at its height or below.
    fn neighbours(&self, rt: &Rt, w: &mut World, from: Step) -> Vec<Step> {
        let p = from.pos;
        let above = self.kind(rt, w, [p[0], p[1] + 1, p[2]]);
        let up = if self.m(above) >= 0.0 && from.kind != Kind::StickyHoney { self.step } else { 0 };
        let floor = w.floor_level(rt, p, self.can_float);
        let dirs = [(0, 1), (-1, 0), (0, -1), (1, 0)];
        let mut sides: [Option<Step>; 4] = [None; 4];
        let mut out = Vec::with_capacity(8);
        for (i, &(dx, dz)) in dirs.iter().enumerate() {
            let n = self.accept(rt, w, [p[0] + dx, p[1], p[2] + dz], up, floor, (dx, dz));
            sides[i] = n;
            if let Some(n) = n {
                out.push(n);
            }
        }
        for i in 0..4 {
            let j = (i + 1) % 4;
            let (Some(a), Some(b)) = (sides[i], sides[j]) else { continue };
            if a.pos[1] > p[1] || b.pos[1] > p[1] || a.kind == Kind::WalkableDoor || b.kind == Kind::WalkableDoor {
                continue;
            }
            let (dx, dz) = (dirs[i].0 + dirs[j].0, dirs[i].1 + dirs[j].1);
            if let Some(n) = self.accept(rt, w, [p[0] + dx, p[1], p[2] + dz], up, floor, dirs[i])
                && n.kind != Kind::WalkableDoor
            {
                out.push(n);
            }
        }
        out
    }
}

/// Whether box `b` meets any block's collision.
fn collides(rt: &Rt, w: &mut World, b: [f64; 6]) -> bool {
    for x in b[0].floor() as i32..=(b[3].ceil() as i32 - 1) {
        for y in (b[1].floor() as i32 - 1)..=(b[4].ceil() as i32 - 1) {
            for z in b[2].floor() as i32..=(b[5].ceil() as i32 - 1) {
                let o = [f64::from(x), f64::from(y), f64::from(z)];
                let hit = w.collision(rt, [x, y, z]).iter().any(|s| {
                    s[0] + o[0] < b[3] && s[3] + o[0] > b[0] && s[1] + o[1] < b[4] && s[4] + o[1] > b[1] && s[2] + o[2] < b[5] && s[5] + o[2] > b[2]
                });
                if hit {
                    return true;
                }
            }
        }
    }
    false
}

// ---- the search ----

#[derive(Clone, Copy)]
struct Found {
    step: Step,
    g: f32,
    f: f32,
    walked: f32,
    came_from: Option<usize>,
    closed: bool,
}

fn distance(a: [i32; 3], b: [i32; 3]) -> f32 {
    let d = [0, 1, 2].map(|i| (b[i] - a[i]) as f32);
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

fn manhattan(a: [i32; 3], b: [i32; 3]) -> f32 {
    ((b[0] - a[0]).abs() + (b[1] - a[1]).abs() + (b[2] - a[2]).abs()) as f32
}

/// A search's answer: the nodes from the start, and whether the last reaches the target.
struct Route {
    nodes: Vec<Step>,
    reached: bool,
}

/// `PathFinder.findPath`: A* from `start` to `target`, nodes within `range` of the start (and
/// walked) and `max_nodes` visited; a node within `accuracy` (Manhattan) of the target reaches it.
/// Unreached, the path to the node that came nearest it.
fn find(rt: &Rt, w: &mut World, walker: &Walker, start: Step, target: [i32; 3], range: f32, accuracy: i32, max_nodes: usize) -> Route {
    use std::cmp::Reverse;
    let mut nodes: Vec<Found> = Vec::new();
    let mut index: crate::HashMap<[i32; 3], usize> = Default::default();
    let h0 = distance(start.pos, target);
    nodes.push(Found { step: start, g: 0.0, f: h0, walked: 0.0, came_from: None, closed: false });
    index.insert(start.pos, 0);
    let mut best = (h0, 0usize);
    let mut open: std::collections::BinaryHeap<(Reverse<OrdF>, Reverse<usize>)> = Default::default();
    open.push((Reverse(OrdF(h0)), Reverse(0)));
    let mut visited = 0;
    let mut reached = None;
    while let Some((Reverse(OrdF(f)), Reverse(i))) = open.pop() {
        if nodes[i].closed || f != nodes[i].f {
            continue;
        }
        visited += 1;
        if visited >= max_nodes {
            break;
        }
        nodes[i].closed = true;
        let here = nodes[i];
        if manhattan(here.step.pos, target) <= accuracy as f32 {
            reached = Some(i);
            break;
        }
        if distance(here.step.pos, start.pos) >= range {
            continue;
        }
        for n in walker.neighbours(rt, w, here.step) {
            let d = distance(here.step.pos, n.pos);
            let walked = here.walked + d;
            let g = here.g + d + n.malus;
            if walked >= range {
                continue;
            }
            let h = distance(n.pos, target);
            let j = match index.get(&n.pos).copied() {
                Some(j) if nodes[j].closed || g >= nodes[j].g => continue,
                Some(j) => j,
                None => {
                    nodes.push(Found { step: n, g, f: 0.0, walked, came_from: None, closed: false });
                    index.insert(n.pos, nodes.len() - 1);
                    nodes.len() - 1
                }
            };
            nodes[j].g = g;
            nodes[j].walked = walked;
            nodes[j].came_from = Some(i);
            nodes[j].f = g + h * 1.5;
            open.push((Reverse(OrdF(nodes[j].f)), Reverse(j)));
            if h < best.0 {
                best = (h, j);
            }
        }
    }
    let (end, reached) = match reached {
        Some(i) => (i, true),
        None => (best.1, false),
    };
    let mut route = Vec::new();
    let mut at = Some(end);
    while let Some(i) = at {
        route.push(nodes[i].step);
        at = nodes[i].came_from;
    }
    route.reverse();
    Route { nodes: route, reached }
}

/// A float the open set orders by.
#[derive(Clone, Copy, PartialEq)]
struct OrdF(f32);

impl Eq for OrdF {}

impl PartialOrd for OrdF {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for OrdF {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&o.0)
    }
}

// ---- paths as objects ----

fn new_node(rt: &mut Rt, s: Step) -> V {
    let n = rt.new_object(NODE);
    rt.put_field(n, "x", V::I(s.pos[0]));
    rt.put_field(n, "y", V::I(s.pos[1]));
    rt.put_field(n, "z", V::I(s.pos[2]));
    rt.put_field(n, "costMalus", V::F(s.malus));
    let t = path_type(rt, s.kind as usize);
    rt.put_field(n, "type", t);
    n
}

fn node_pos(rt: &Rt, n: V) -> [i32; 3] {
    [int(rt, n, "x"), int(rt, n, "y"), int(rt, n, "z")]
}

fn node_kind(rt: &Rt, n: V) -> Kind {
    let t = rt.get_field(n, "type");
    KINDS[rt.get_field(t, "$ordinal").i().clamp(0, 25) as usize]
}

/// A `Path` of `nodes` toward `target`.
fn new_path(rt: &mut Rt, nodes: Vec<V>, target: [i32; 3], reached: bool) -> V {
    let p = rt.new_object(PATH);
    let dist = nodes.last().map_or(f32::MAX, |&n| manhattan(node_pos(rt, n), target));
    let list = jutil::new_list(rt, nodes);
    rt.put_field(p, "nodes", list);
    rt.put_field(p, "nextNodeIndex", V::I(0));
    let t = block_pos(rt, target);
    rt.put_field(p, "target", t);
    rt.put_field(p, "distToTarget", V::F(dist));
    rt.put_field(p, "reached", V::I(i32::from(reached)));
    p
}

fn path_nodes(rt: &Rt, path: V) -> Vec<V> {
    if path.r() == 0 {
        return Vec::new();
    }
    jutil::elements(rt, obj(rt, path, "nodes"))
}

fn set_path_nodes(rt: &mut Rt, path: V, items: Vec<V>) {
    let l = obj(rt, path, "nodes");
    if let Some(Data::List(held)) = rt.heap.get_mut(l.r() as usize).map(|o| &mut o.data) {
        *held = items;
    }
}

fn next_index(rt: &Rt, path: V) -> i32 {
    int(rt, path, "nextNodeIndex")
}

fn path_done(rt: &Rt, path: V) -> bool {
    path.r() == 0 || next_index(rt, path) >= path_nodes(rt, path).len() as i32
}

fn same_path(rt: &Rt, a: V, b: V) -> bool {
    if a.r() == 0 || b.r() == 0 {
        return false;
    }
    let (x, y) = (path_nodes(rt, a), path_nodes(rt, b));
    x.len() == y.len() && x.iter().zip(&y).all(|(&p, &q)| node_pos(rt, p) == node_pos(rt, q))
}

/// The last node of `path`, as a position.
pub(crate) fn end_node(rt: &Rt, path: V) -> Option<[i32; 3]> {
    path_nodes(rt, path).last().map(|&n| node_pos(rt, n))
}

/// `Path.getEntityPosAtNode(entity, i)`: the node's corner, half a block in for each whole block of
/// the entity's width plus one.
fn entity_pos_at(rt: &mut Rt, path: V, e: V, i: i32) -> [f64; 3] {
    let nodes = path_nodes(rt, path);
    let Some(&n) = nodes.get(i.max(0) as usize) else { return mobs::position(rt, e) };
    let [x, y, z] = node_pos(rt, n);
    let w = crate::mod_entities::own_size(rt, e)[0];
    let half = f64::from((w + 1.0) as i32) * 0.5;
    [f64::from(x) + half, f64::from(y), f64::from(z) + half]
}

fn path_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let p = a(0);
    if name == "<init>" {
        // `(List<Node> nodes, BlockPos target, boolean reached)`.
        let list = jutil::elements(rt, a(1));
        let target = api::pos_of(rt, a(2));
        let made = new_path(rt, list, target, a(3).i() != 0);
        for f in ["nodes", "nextNodeIndex", "target", "distToTarget", "reached"] {
            let v = rt.get_field(made, f);
            rt.put_field(p, f, v);
        }
        return Some(V::Void);
    }
    let nodes = path_nodes(rt, p);
    let next = next_index(rt, p);
    let at = |i: i32| nodes.get(i.max(0) as usize).copied().unwrap_or(V::NULL);
    Some(match name {
        "advance" => {
            rt.put_field(p, "nextNodeIndex", V::I(next + 1));
            V::Void
        }
        "notStarted" => V::I(i32::from(next <= 0)),
        "isDone" => V::I(i32::from(path_done(rt, p))),
        "getEndNode" => nodes.last().copied().unwrap_or(V::NULL),
        "getNode" => at(a(1).i()),
        "truncateNodes" => {
            let keep: Vec<V> = nodes.iter().copied().take(a(1).i().max(0) as usize).collect();
            set_path_nodes(rt, p, keep);
            V::Void
        }
        "replaceNode" => {
            let mut items = nodes.clone();
            if let Some(slot) = items.get_mut(a(1).i().max(0) as usize) {
                *slot = a(2);
            }
            set_path_nodes(rt, p, items);
            V::Void
        }
        "getNodeCount" => V::I(nodes.len() as i32),
        "getNextNodeIndex" => V::I(next),
        "setNextNodeIndex" => {
            rt.put_field(p, "nextNodeIndex", a(1));
            V::Void
        }
        "getEntityPosAtNode" => {
            let pos = entity_pos_at(rt, p, a(1), a(2).i());
            vec3(rt, pos)
        }
        "getNextEntityPos" => {
            let pos = entity_pos_at(rt, p, a(1), next);
            vec3(rt, pos)
        }
        "getNodePos" => {
            let pos = node_pos(rt, at(a(1).i()));
            block_pos(rt, pos)
        }
        "getNextNodePos" => {
            let pos = node_pos(rt, at(next));
            block_pos(rt, pos)
        }
        "getNextNode" => at(next),
        "getPreviousNode" => if next > 0 { at(next - 1) } else { V::NULL },
        "sameAs" => V::I(i32::from(same_path(rt, p, a(1)))),
        "canReach" => V::I(int(rt, p, "reached")),
        "getTarget" => obj(rt, p, "target"),
        "getDistToTarget" => V::F(rt.get_field(p, "distToTarget").f()),
        "copy" => {
            let target = api::pos_of(rt, obj(rt, p, "target"));
            let reached = int(rt, p, "reached") != 0;
            let n = new_path(rt, nodes.clone(), target, reached);
            rt.put_field(n, "nextNodeIndex", V::I(next));
            n
        }
        _ => return None,
    })
}

fn node_call(rt: &mut Rt, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let n = a(0);
    let at = node_pos(rt, n);
    Some(match name {
        "<init>" => {
            rt.put_field(n, "x", a(1));
            rt.put_field(n, "y", a(2));
            rt.put_field(n, "z", a(3));
            rt.put_field(n, "costMalus", V::F(0.0));
            let t = path_type(rt, Kind::Blocked as usize);
            rt.put_field(n, "type", t);
            V::Void
        }
        "asBlockPos" => block_pos(rt, at),
        "asVec3" => vec3(rt, at.map(f64::from)),
        "cloneAndMove" => {
            let (kind, m) = (rt.get_field(n, "type"), rt.get_field(n, "costMalus"));
            let c = rt.new_object(NODE);
            for (f, v) in [("x", a(1)), ("y", a(2)), ("z", a(3)), ("type", kind), ("costMalus", m)] {
                rt.put_field(c, f, v);
            }
            c
        }
        "distanceTo" | "distanceToSqr" | "distanceManhattan" | "distanceToXZ" => {
            let other = if rt.class_of(a(1)) == NODE { node_pos(rt, a(1)) } else { api::pos_of(rt, a(1)) };
            let d = [0, 1, 2].map(|i| (other[i] - at[i]) as f32);
            V::F(match name {
                "distanceTo" => (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt(),
                "distanceToSqr" => d[0] * d[0] + d[1] * d[1] + d[2] * d[2],
                "distanceToXZ" => (d[0] * d[0] + d[2] * d[2]).sqrt(),
                _ => d[0].abs() + d[1].abs() + d[2].abs(),
            })
        }
        "equals" => V::I(i32::from(rt.class_of(a(1)) == NODE && node_pos(rt, a(1)) == at)),
        // `Node.createHash(x, y, z)`.
        "hashCode" => V::I(
            (at[1] & 255) | (at[0] & 32767) << 8 | (at[2] & 32767) << 24 | if at[0] < 0 { i32::MIN } else { 0 } | if at[2] < 0 { 32768 } else { 0 },
        ),
        _ => return None,
    })
}

// ---- navigation ----

/// A mob's navigation: `GroundPathNavigation(mob, level)`.
pub(crate) fn new_ground(rt: &mut Rt, mob: V, level: V) -> V {
    let n = rt.new_object(GROUND);
    init_navigation(rt, n, mob, level);
    n
}

fn init_navigation(rt: &mut Rt, n: V, mob: V, level: V) {
    rt.put_field(n, "mob", mob);
    rt.put_field(n, "level", level);
    rt.put_field(n, "path", V::NULL);
    rt.put_field(n, "speedModifier", V::D(0.0));
    rt.put_field(n, "tick", V::I(0));
    rt.put_field(n, "maxVisitedNodesMultiplier", V::F(1.0));
    rt.put_field(n, "$canFloat", V::I(0));
    // `GroundPathNavigation.createPathFinder`: its evaluator passes doors.
    rt.put_field(n, "$passDoors", V::I(1));
    let range = mobs::attribute_base(rt, mob, "minecraft:generic.follow_range");
    rt.put_field(n, "$maxNodes", V::I((range * 16.0).floor() as i32));
}

/// `setCanFloat(b)`: the evaluator's (whether its paths go through water).
pub(crate) fn set_can_float(rt: &mut Rt, nav: V, b: bool) {
    if nav.r() != 0 {
        rt.put_field(nav, "$canFloat", V::I(i32::from(b)));
    }
}

fn can_float(rt: &Rt, nav: V) -> bool {
    int(rt, nav, "$canFloat") != 0
}

fn nav_mob(rt: &Rt, nav: V) -> V {
    obj(rt, nav, "mob")
}

pub(crate) fn is_done(rt: &Rt, nav: V) -> bool {
    path_done(rt, obj(rt, nav, "path"))
}

pub(crate) fn stop(rt: &mut Rt, nav: V) {
    if nav.r() != 0 {
        rt.put_field(nav, "path", V::NULL);
    }
}

pub(crate) fn set_speed(rt: &mut Rt, nav: V, speed: f64) {
    if nav.r() != 0 {
        rt.put_field(nav, "speedModifier", V::D(speed));
    }
}

/// `GroundPathNavigation.canUpdatePath()`: on the ground, in a liquid, or riding.
fn can_update_path(rt: &mut Rt, nav: V) -> bool {
    let mob = nav_mob(rt, nav);
    mobs::on_ground(rt, mob) || mobs::in_water(rt, mob) || mobs::in_lava(rt, mob)
}

/// `GroundPathNavigation.getTempMobPos()`: its x and z, at its surface's height.
fn temp_mob_pos(rt: &mut Rt, nav: V) -> [f64; 3] {
    let mob = nav_mob(rt, nav);
    let p = mobs::position(rt, mob);
    [p[0], f64::from(surface_y(rt, nav, mob)), p[2]]
}

/// `GroundPathNavigation.getSurfaceY()`: in water it floats in, the first block above that isn't
/// water (at most 16 up); else its feet's block, rounded.
fn surface_y(rt: &mut Rt, nav: V, mob: V) -> i32 {
    let p = mobs::position(rt, mob);
    if mobs::in_water(rt, mob) && can_float(rt, nav) {
        let mut w = World::new(rt);
        let (x, z) = (p[0].floor() as i32, p[2].floor() as i32);
        let mut y = p[1].floor() as i32;
        let mut n = 0;
        while w.water(rt, [x, y, z]) {
            y += 1;
            n += 1;
            if n > 16 {
                return p[1].floor() as i32;
            }
        }
        return y;
    }
    (p[1] + 0.5).floor() as i32
}

/// `WalkNodeEvaluator.getStart()`: the block the mob's path starts from: on the water it floats
/// on, where it stands, or the first block below it falls to.
fn start_of(rt: &mut Rt, w: &mut World, walker: &Walker, nav: V, mob: V) -> Step {
    let p = mobs::position(rt, mob);
    let (x, z) = (p[0].floor() as i32, p[2].floor() as i32);
    let mut y = p[1].floor() as i32;
    if can_float(rt, nav) && mobs::in_water(rt, mob) {
        while w.water(rt, [x, y, z]) && y < w.top {
            y += 1;
        }
        y -= 1;
    } else if mobs::on_ground(rt, mob) {
        y = (p[1] + 0.5).floor() as i32;
    } else {
        let mut q = (p[1] + 1.0).floor() as i32;
        while q > w.bottom {
            y = q;
            q -= 1;
            // Down to a block it can't walk through (`!isPathfindable(LAND)`: a full block, or one
            // as tall as a slab).
            if !w.is_air(rt, [x, q, z]) && (w.full(rt, [x, q, z]) || w.top_of(rt, [x, q, z]).is_some_and(|t| t >= 0.5)) {
                break;
            }
        }
    }
    let k = walker.kind(rt, w, [x, y, z]);
    Step { pos: [x, y, z], kind: k, malus: walker.m(k) }
}

/// `PathNavigation.createPath(targets, regionOffset, offsetUpward, accuracy)` for one target.
fn create_path(rt: &mut Rt, nav: V, target: [i32; 3], accuracy: i32) -> V {
    let mob = nav_mob(rt, nav);
    if mob.r() == 0 {
        return V::NULL;
    }
    let (bottom, _) = limits(rt);
    if mobs::position(rt, mob)[1] < f64::from(bottom) || !can_update_path(rt, nav) {
        return V::NULL;
    }
    // The path it follows already leads there.
    let current = obj(rt, nav, "path");
    if current.r() != 0 && !path_done(rt, current) && int(rt, nav, "$targetSet") != 0 && [int(rt, nav, "$tx"), int(rt, nav, "$ty"), int(rt, nav, "$tz")] == target {
        return current;
    }
    let range = mobs::attribute(rt, mob, "minecraft:generic.follow_range") as f32;
    let multiplier = match rt.get_field(nav, "maxVisitedNodesMultiplier") {
        V::F(m) => m,
        _ => 1.0,
    };
    let max_nodes = (int(rt, nav, "$maxNodes").max(16) as f32 * multiplier) as usize;
    let walker = Walker::of(rt, mob, nav);
    let mut w = World::new(rt);
    let start = start_of(rt, &mut w, &walker, nav, mob);
    let route = find(rt, &mut w, &walker, start, target, range, accuracy, max_nodes.max(1));
    let nodes: Vec<V> = route.nodes.iter().map(|&s| new_node(rt, s)).collect();
    let path = new_path(rt, nodes, target, route.reached);
    rt.put_field(nav, "$tx", V::I(target[0]));
    rt.put_field(nav, "$ty", V::I(target[1]));
    rt.put_field(nav, "$tz", V::I(target[2]));
    rt.put_field(nav, "$targetSet", V::I(1));
    rt.put_field(nav, "reachRange", V::I(accuracy));
    reset_stuck_timeout(rt, nav);
    path
}

/// `GroundPathNavigation.createPath(pos, accuracy)`: in the air, the ground below it (or, with
/// none below, the first block above the air); in a solid block, the first free block above it.
pub(crate) fn create_path_to_block(rt: &mut Rt, nav: V, pos: [i32; 3], accuracy: i32) -> V {
    let mut w = World::new(rt);
    let mut pos = pos;
    if w.is_air(rt, pos) {
        let mut q = [pos[0], pos[1] - 1, pos[2]];
        while q[1] > w.bottom && w.is_air(rt, q) {
            q[1] -= 1;
        }
        if q[1] > w.bottom {
            return create_path(rt, nav, [q[0], q[1] + 1, q[2]], accuracy);
        }
        while q[1] < w.top && w.is_air(rt, q) {
            q[1] += 1;
        }
        pos = q;
    }
    // `BlockState.isSolid()`: a block that blocks motion (its collision, not water's).
    let solid = |w: &mut World, p: [i32; 3]| !w.collision(rt, p).is_empty();
    if !solid(&mut w, pos) {
        return create_path(rt, nav, pos, accuracy);
    }
    let mut up = [pos[0], pos[1] + 1, pos[2]];
    while up[1] < w.top && solid(&mut w, up) {
        up[1] += 1;
    }
    create_path(rt, nav, up, accuracy)
}

pub(crate) fn create_path_to(rt: &mut Rt, nav: V, at: [f64; 3], accuracy: i32) -> V {
    create_path_to_block(rt, nav, floor3(at), accuracy)
}

pub(crate) fn create_path_to_entity(rt: &mut Rt, nav: V, e: V, accuracy: i32) -> V {
    if e.r() == 0 {
        return V::NULL;
    }
    let at = mobs::block_position(rt, e);
    create_path_to_block(rt, nav, at, accuracy)
}

/// `moveTo(path, speed)`: it follows `path` (unless it follows the same one), at `speed` times its
/// own; false for no path or one it has walked.
pub(crate) fn move_along(rt: &mut Rt, nav: V, path: V, speed: f64) -> bool {
    if path.r() == 0 {
        rt.put_field(nav, "path", V::NULL);
        return false;
    }
    if !same_path(rt, path, obj(rt, nav, "path")) {
        rt.put_field(nav, "path", path);
    }
    if is_done(rt, nav) {
        return false;
    }
    trim_path(rt, nav);
    if path_nodes(rt, obj(rt, nav, "path")).is_empty() {
        return false;
    }
    rt.put_field(nav, "speedModifier", V::D(speed));
    let at = temp_mob_pos(rt, nav);
    let t = int(rt, nav, "tick");
    rt.put_field(nav, "lastStuckCheck", V::I(t));
    put3(rt, nav, ["$stuckX", "$stuckY", "$stuckZ"], at);
    true
}

/// `PathNavigation.trimPath()`: a cauldron on the way is stepped over.
fn trim_path(rt: &mut Rt, nav: V) {
    let path = obj(rt, nav, "path");
    let mut nodes = path_nodes(rt, path);
    let mut w = World::new(rt);
    let mut changed = false;
    for i in 0..nodes.len() {
        let p = node_pos(rt, nodes[i]);
        if !(w.name(rt, p).ends_with("cauldron") || rt.host().is_some_and(|h| h.block_in_tag(&w.name(rt, p), "minecraft:cauldrons"))) {
            continue;
        }
        let up = node_call(rt, "cloneAndMove", &[nodes[i], V::I(p[0]), V::I(p[1] + 1), V::I(p[2])]).unwrap_or(nodes[i]);
        nodes[i] = up;
        changed = true;
        if let Some(&next) = nodes.get(i + 1) {
            let q = node_pos(rt, next);
            if p[1] >= q[1] {
                nodes[i + 1] = node_call(rt, "cloneAndMove", &[next, V::I(q[0]), V::I(p[1] + 1), V::I(q[2])]).unwrap_or(next);
            }
        }
    }
    if changed {
        set_path_nodes(rt, path, nodes);
    }
}

fn put3(rt: &mut Rt, o: V, f: [&'static str; 3], v: [f64; 3]) {
    for (f, v) in f.into_iter().zip(v) {
        rt.put_field(o, f, V::D(v));
    }
}

/// `moveTo(x, y, z, speed)`.
pub(crate) fn move_to(rt: &mut Rt, nav: V, at: [f64; 3], speed: f64) -> bool {
    let path = create_path_to(rt, nav, at, 1);
    move_along(rt, nav, path, speed)
}

/// `moveTo(entity, speed)`.
pub(crate) fn move_to_entity(rt: &mut Rt, nav: V, e: V, speed: f64) -> bool {
    let path = create_path_to_entity(rt, nav, e, 1);
    path.r() != 0 && move_along(rt, nav, path, speed)
}

fn reset_stuck_timeout(rt: &mut Rt, nav: V) {
    rt.put_field(nav, "$timeoutSet", V::I(0));
    rt.put_field(nav, "timeoutTimer", V::J(0));
    rt.put_field(nav, "timeoutLimit", V::D(0.0));
    rt.put_field(nav, "isStuck", V::I(0));
}

/// `PathNavigation.tick()`: it follows its path (where it may update it) and tells the mob's move
/// control where to go next, at the next node's floor.
pub(crate) fn tick(rt: &mut Rt, nav: V) {
    let t = int(rt, nav, "tick") + 1;
    rt.put_field(nav, "tick", V::I(t));
    if is_done(rt, nav) {
        return;
    }
    let mob = nav_mob(rt, nav);
    if can_update_path(rt, nav) {
        follow_the_path(rt, nav, mob);
    } else {
        let path = obj(rt, nav, "path");
        let here = temp_mob_pos(rt, nav);
        let next = entity_pos_at(rt, path, mob, next_index(rt, path));
        if here[1] > next[1] && !mobs::on_ground(rt, mob) && here[0].floor() == next[0].floor() && here[2].floor() == next[2].floor() {
            advance(rt, path);
        }
    }
    if is_done(rt, nav) {
        return;
    }
    let path = obj(rt, nav, "path");
    let next = entity_pos_at(rt, path, mob, next_index(rt, path));
    let y = ground_y(rt, next);
    let speed = dbl(rt, nav, "speedModifier");
    let control = mobs::move_control(rt, mob);
    mobs::set_wanted_position(rt, control, [next[0], y, next[2]], speed);
}

fn advance(rt: &mut Rt, path: V) {
    let n = next_index(rt, path);
    rt.put_field(path, "nextNodeIndex", V::I(n + 1));
}

/// `PathNavigation.getGroundY(at)`: over air, the point's own height; else the floor there.
fn ground_y(rt: &mut Rt, at: [f64; 3]) -> f64 {
    let p = floor3(at);
    let mut w = World::new(rt);
    if w.is_air(rt, [p[0], p[1] - 1, p[2]]) {
        return at[1];
    }
    w.floor_level(rt, p, false)
}

/// `PathNavigation.followThePath()`: a node reached (within its waypoint distance across, a block
/// up or down) is passed, or one it may cut a corner past; then whether it is stuck.
fn follow_the_path(rt: &mut Rt, nav: V, mob: V) {
    let here = temp_mob_pos(rt, nav);
    let w = crate::mod_entities::own_size(rt, mob)[0];
    let max = if w > 0.75 { w / 2.0 } else { 0.75 - w / 2.0 };
    rt.put_field(nav, "maxDistanceToWaypoint", V::F(max));
    let path = obj(rt, nav, "path");
    let nodes = path_nodes(rt, path);
    let Some(&node) = nodes.get(next_index(rt, path).max(0) as usize) else { return };
    let [nx, ny, nz] = node_pos(rt, node);
    let p = mobs::position(rt, mob);
    let (dx, dy, dz) = ((p[0] - (f64::from(nx) + 0.5)).abs(), (p[1] - f64::from(ny)).abs(), (p[2] - (f64::from(nz) + 0.5)).abs());
    let reached = dx < f64::from(max) && dz < f64::from(max) && dy < 1.0;
    // `canCutCorner(type)`: on land, past any node but water.
    let cut = !matches!(node_kind(rt, node), Kind::Water | Kind::WaterBorder) && should_target_next(rt, nav, path, mob, here);
    if reached || cut {
        advance(rt, path);
    }
    stuck_detection(rt, nav, mob, here);
}

/// `PathNavigation.shouldTargetNextNodeInDirection(here)`: within two blocks of the next node, it
/// may head straight to the one after (`canMoveDirectly`), or that one is nearer, or the next is
/// very near, and they lie on opposite sides.
fn should_target_next(rt: &mut Rt, nav: V, path: V, mob: V, here: [f64; 3]) -> bool {
    let nodes = path_nodes(rt, path);
    let next = next_index(rt, path).max(0) as usize;
    if next + 1 >= nodes.len() {
        return false;
    }
    let centre = |rt: &Rt, n: V| {
        let [x, y, z] = node_pos(rt, n);
        [f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5]
    };
    let a = centre(rt, nodes[next]);
    let d2 = |p: [f64; 3], q: [f64; 3]| (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2);
    if d2(here, a) >= 4.0 {
        return false;
    }
    let _ = nav;
    let _ = mob;
    let b = centre(rt, nodes[next + 1]);
    let va = [a[0] - here[0], a[1] - here[1], a[2] - here[2]];
    let vb = [b[0] - here[0], b[1] - here[1], b[2] - here[2]];
    let (la, lb) = (va.iter().map(|c| c * c).sum::<f64>(), vb.iter().map(|c| c * c).sum::<f64>());
    if !(lb < la) && !(la < 0.5) {
        return false;
    }
    let norm = |v: [f64; 3], l: f64| if l < 1.0e-8 { [0.0; 3] } else { v.map(|c| c / l.sqrt()) };
    let (na, nb) = (norm(va, la), norm(vb, lb));
    na[0] * nb[0] + na[1] * nb[1] + na[2] * nb[2] < 0.0
}

/// `PathNavigation.doStuckDetection(here)`: every 100 ticks, one that moved less than a quarter of
/// what its speed would carry it is stuck and stops; one that takes three times as long to reach
/// a node as its speed needs gives the path up.
fn stuck_detection(rt: &mut Rt, nav: V, mob: V, here: [f64; 3]) {
    let t = int(rt, nav, "tick");
    let speed = mobs::speed(rt, mob);
    if t - int(rt, nav, "lastStuckCheck") > 100 {
        let f = if speed >= 1.0 { speed } else { speed * speed };
        let f1 = f64::from(f * 100.0 * 0.25);
        let last = ["$stuckX", "$stuckY", "$stuckZ"].map(|f| dbl(rt, nav, f));
        let d = (here[0] - last[0]).powi(2) + (here[1] - last[1]).powi(2) + (here[2] - last[2]).powi(2);
        if d < f1 * f1 {
            rt.put_field(nav, "isStuck", V::I(1));
            stop(rt, nav);
        } else {
            rt.put_field(nav, "isStuck", V::I(0));
        }
        rt.put_field(nav, "lastStuckCheck", V::I(t));
        put3(rt, nav, ["$stuckX", "$stuckY", "$stuckZ"], here);
    }
    let path = obj(rt, nav, "path");
    if path_done(rt, path) {
        return;
    }
    let nodes = path_nodes(rt, path);
    let Some(&node) = nodes.get(next_index(rt, path).max(0) as usize) else { return };
    let np = node_pos(rt, node);
    let now = mobs::game_time(rt);
    let cached = [int(rt, nav, "$cx"), int(rt, nav, "$cy"), int(rt, nav, "$cz")];
    if int(rt, nav, "$timeoutSet") != 0 && cached == np {
        let timer = rt.get_field(nav, "timeoutTimer").j() + (now - rt.get_field(nav, "lastTimeoutCheck").j());
        rt.put_field(nav, "timeoutTimer", V::J(timer));
    } else {
        rt.put_field(nav, "$timeoutSet", V::I(1));
        for (f, v) in [("$cx", np[0]), ("$cy", np[1]), ("$cz", np[2])] {
            rt.put_field(nav, f, V::I(v));
        }
        let c = [f64::from(np[0]) + 0.5, f64::from(np[1]), f64::from(np[2]) + 0.5];
        let d = ((here[0] - c[0]).powi(2) + (here[1] - c[1]).powi(2) + (here[2] - c[2]).powi(2)).sqrt();
        let limit = if speed > 0.0 { d / f64::from(speed) * 20.0 } else { 0.0 };
        rt.put_field(nav, "timeoutLimit", V::D(limit));
    }
    let limit = dbl(rt, nav, "timeoutLimit");
    if limit > 0.0 && rt.get_field(nav, "timeoutTimer").j() as f64 > limit * 3.0 {
        reset_stuck_timeout(rt, nav);
        stop(rt, nav);
    }
    rt.put_field(nav, "lastTimeoutCheck", V::J(now));
}

// ---- random places ----

/// `RandomPos.generateRandomDirection(random, h, v)`: an offset within `h` across and `v` up or
/// down.
fn random_direction(rt: &mut Rt, mob: V, h: i32, v: i32) -> [i32; 3] {
    let x = mobs::random_int(rt, mob, 2 * h + 1) - h;
    let y = mobs::random_int(rt, mob, 2 * v + 1) - v;
    let z = mobs::random_int(rt, mob, 2 * h + 1) - h;
    [x, y, z]
}

/// `RandomPos.generateRandomDirectionWithinRadians(random, h, v, 0, x, z, angle)`: toward (x, z)
/// within `angle` either way; none past `h`.
fn random_direction_toward(rt: &mut Rt, mob: V, h: i32, v: i32, dir: [f64; 2], angle: f64) -> Option<[i32; 3]> {
    let a = dir[1].atan2(dir[0]) - f64::from(std::f32::consts::FRAC_PI_2);
    let b = a + f64::from(2.0 * mobs::random_float(rt, mob) - 1.0) * angle;
    let l = mobs::random_double(rt, mob).sqrt() * f64::from(std::f32::consts::SQRT_2) * f64::from(h);
    let (dx, dz) = (-l * b.sin(), l * b.cos());
    if dx.abs() > f64::from(h) || dz.abs() > f64::from(h) {
        return None;
    }
    let dy = mobs::random_int(rt, mob, 2 * v + 1) - v;
    Some([dx.floor() as i32, dy, dz.floor() as i32])
}

/// `RandomPos.generateRandomPos(supplier, value)`: of ten tries, the one the mob values most
/// (`getWalkTargetValue`; the first among equals), at its block's bottom centre.
fn best_of(rt: &mut Rt, mob: V, mut make: impl FnMut(&mut Rt) -> Option<[i32; 3]>) -> Option<V> {
    let mut best: Option<([i32; 3], f32)> = None;
    for _ in 0..10 {
        if let Some(p) = make(rt) {
            let value = mobs::walk_target_value(rt, mob, p);
            if best.is_none_or(|(_, b)| value > b) {
                best = Some((p, value));
            }
        }
    }
    best.map(|(p, _)| vec3(rt, [f64::from(p[0]) + 0.5, f64::from(p[1]), f64::from(p[2]) + 0.5]))
}

/// `RandomPos.generateRandomPosTowardDirection(mob, h, random, offset)`: the offset from where the
/// mob is (toward its restriction's centre, when it has one).
fn toward(rt: &mut Rt, mob: V, h: i32, offset: [i32; 3]) -> [i32; 3] {
    let (mut x, mut z) = (offset[0], offset[2]);
    let here = mobs::position(rt, mob);
    if let Some((centre, _)) = mobs::restriction(rt, mob)
        && h > 1
    {
        let r = mobs::random_int(rt, mob, h / 2);
        x += if here[0] > f64::from(centre[0]) { -r } else { r };
        let r = mobs::random_int(rt, mob, h / 2);
        z += if here[2] > f64::from(centre[2]) { -r } else { r };
    }
    floor3([f64::from(x) + here[0], f64::from(offset[1]) + here[1], f64::from(z) + here[2]])
}

/// `GoalUtils.mobRestricted(mob, h)`: it has a restriction and is near it.
fn near_restriction(rt: &mut Rt, mob: V, h: i32) -> bool {
    let Some((centre, radius)) = mobs::restriction(rt, mob) else { return false };
    let p = mobs::position(rt, mob);
    let c = [f64::from(centre[0]) + 0.5, f64::from(centre[1]) + 0.5, f64::from(centre[2]) + 0.5];
    let d = (p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2);
    let r = f64::from(radius) + f64::from(h) + 1.0;
    d < r * r
}

/// The checks `DefaultRandomPos` makes of a place: within the world, within its restriction (when
/// near it), on something solid (`isStableDestination`), and of no malus.
fn takes(rt: &mut Rt, w: &mut World, mob: V, p: [i32; 3], restricted: bool, malus_check: bool) -> bool {
    if p[1] < w.bottom || p[1] > w.top || (restricted && !mobs::within_restriction(rt, mob, p)) {
        return false;
    }
    let below = [p[0], p[1] - 1, p[2]];
    if !w.full(rt, below) {
        return false;
    }
    !malus_check || malus(rt, mob, w.kind(rt, p) as usize) == 0.0
}

/// `DefaultRandomPos.getPos(mob, h, v)`: a random place within `h` across and `v` up or down that
/// the mob may stand at, the one it values most of ten.
pub(crate) fn default_random_pos(rt: &mut Rt, mob: V, h: i32, v: i32) -> Option<V> {
    let restricted = near_restriction(rt, mob, h);
    let mut w = World::new(rt);
    best_of(rt, mob, |rt| {
        let d = random_direction(rt, mob, h, v);
        let p = toward(rt, mob, h, d);
        takes(rt, &mut w, mob, p, restricted, true).then_some(p)
    })
}

/// `LandRandomPos.getPos(mob, h, v)`: as [`default_random_pos`], then up out of the ground
/// (`movePosUpOutOfSolidOrWater`), never in water nor where it costs.
pub(crate) fn land_random_pos(rt: &mut Rt, mob: V, h: i32, v: i32) -> Option<V> {
    let restricted = near_restriction(rt, mob, h);
    let mut w = World::new(rt);
    best_of(rt, mob, |rt| {
        let d = random_direction(rt, mob, h, v);
        let p = toward(rt, mob, h, d);
        if !takes(rt, &mut w, mob, p, restricted, false) {
            return None;
        }
        let mut q = p;
        while q[1] < w.top && !w.collision(rt, q).is_empty() {
            q[1] += 1;
        }
        let ok = !w.water(rt, q) && malus(rt, mob, w.kind(rt, q) as usize) == 0.0;
        ok.then_some(q)
    })
}

/// `DefaultRandomPos.getPosAway(mob, h, v, from)`: a random place away from `from` (within 90°
/// of the way from it to the mob).
pub(crate) fn random_pos_away(rt: &mut Rt, mob: V, h: i32, v: i32, from: [f64; 3]) -> Option<V> {
    let here = mobs::position(rt, mob);
    random_pos_toward(rt, mob, h, v, [here[0] - from[0], here[2] - from[2]], std::f64::consts::FRAC_PI_2)
}

fn random_pos_toward(rt: &mut Rt, mob: V, h: i32, v: i32, dir: [f64; 2], angle: f64) -> Option<V> {
    let restricted = near_restriction(rt, mob, h);
    let mut w = World::new(rt);
    best_of(rt, mob, |rt| {
        let d = random_direction_toward(rt, mob, h, v, dir, angle)?;
        let p = toward(rt, mob, h, d);
        takes(rt, &mut w, mob, p, restricted, true).then_some(p)
    })
}

/// `PanicGoal.lookForWater`: the nearest water within `r` across and a block up or down.
pub(crate) fn water_near(rt: &mut Rt, mob: V, r: i32) -> Option<[i32; 3]> {
    let here = mobs::block_position(rt, mob);
    let mut w = World::new(rt);
    let mut best: Option<([i32; 3], i32)> = None;
    for dy in -1..=1 {
        for dx in -r..=r {
            for dz in -r..=r {
                let p = [here[0] + dx, here[1] + dy, here[2] + dz];
                let d = dx * dx + dy * dy + dz * dz;
                if best.is_none_or(|(_, b)| d < b) && w.water(rt, p) {
                    best = Some((p, d));
                }
            }
        }
    }
    best.map(|(p, _)| p)
}

// ---- dispatch ----

pub(crate) fn claims(owner: &str, _name: &str, _desc: &str) -> bool {
    owner.starts_with("net/minecraft/world/entity/ai/navigation/")
        || matches!(owner, PATH | NODE | PATH_TYPE | DEFAULT_RANDOM_POS | LAND_RANDOM_POS | RANDOM_POS)
}

/// `PathType`'s statics.
pub(crate) fn get_static(rt: &mut Rt, owner: &str, field: &str) -> Option<V> {
    if owner != PATH_TYPE {
        return None;
    }
    let i = PATH_TYPES.iter().position(|(n, _)| *n == field)?;
    Some(path_type(rt, i))
}

pub fn call(rt: &mut Rt, key: &'static str, args: &[V]) -> Option<V> {
    let (owner, name, desc) = crate::parts(key)?;
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    match owner {
        PATH => return path_call(rt, name, args),
        NODE => return node_call(rt, name, args),
        PATH_TYPE => {
            return match name {
                "getMalus" => {
                    let i = rt.get_field(a(0), "$ordinal").i().clamp(0, 25) as usize;
                    Some(V::F(PATH_TYPES[i].1))
                }
                _ => None,
            };
        }
        DEFAULT_RANDOM_POS | LAND_RANDOM_POS | RANDOM_POS => return random_pos_call(rt, owner, name, args),
        _ => {}
    }
    // A navigation: its own state, by vanilla's methods.
    let nav = a(0);
    if name == "<init>" {
        init_navigation(rt, nav, a(1), a(2));
        return Some(V::Void);
    }
    if nav.r() == 0 {
        return None;
    }
    let target = |rt: &Rt| [int(rt, nav, "$tx"), int(rt, nav, "$ty"), int(rt, nav, "$tz")];
    Some(match name {
        "tick" => {
            tick(rt, nav);
            V::Void
        }
        "isDone" => V::I(i32::from(is_done(rt, nav))),
        "isInProgress" => V::I(i32::from(!is_done(rt, nav))),
        "stop" => {
            stop(rt, nav);
            V::Void
        }
        "getPath" => obj(rt, nav, "path"),
        "getTargetPos" => {
            if int(rt, nav, "$targetSet") == 0 {
                return Some(V::NULL);
            }
            let t = target(rt);
            block_pos(rt, t)
        }
        "setSpeedModifier" => {
            set_speed(rt, nav, a(1).d());
            V::Void
        }
        "setCanFloat" => {
            set_can_float(rt, nav, a(1).i() != 0);
            V::Void
        }
        "canFloat" => V::I(i32::from(can_float(rt, nav))),
        "setCanOpenDoors" => {
            rt.put_field(nav, "$doors", V::I(i32::from(a(1).i() != 0)));
            V::Void
        }
        "setCanPassDoors" => {
            rt.put_field(nav, "$passDoors", V::I(i32::from(a(1).i() != 0)));
            V::Void
        }
        "setCanWalkOverFences" | "setAvoidSun" => V::Void,
        "canOpenDoors" => V::I(i32::from(rt.get_field(nav, "$doors").i() != 0)),
        "canPassDoors" => V::I(i32::from(!matches!(rt.get_field(nav, "$passDoors"), V::I(0)))),
        "setMaxVisitedNodesMultiplier" => {
            rt.put_field(nav, "maxVisitedNodesMultiplier", V::F(a(1).f()));
            V::Void
        }
        "resetMaxVisitedNodesMultiplier" => {
            rt.put_field(nav, "maxVisitedNodesMultiplier", V::F(1.0));
            V::Void
        }
        "isStuck" => V::I(i32::from(rt.get_field(nav, "isStuck").i() != 0)),
        "recomputePath" => {
            if int(rt, nav, "$targetSet") != 0 {
                let (t, accuracy) = (target(rt), int(rt, nav, "reachRange"));
                rt.put_field(nav, "path", V::NULL);
                let p = create_path(rt, nav, t, accuracy);
                rt.put_field(nav, "path", p);
            }
            V::Void
        }
        "moveTo" => V::I(i32::from(match desc {
            "(DDDD)Z" => move_to(rt, nav, [a(1).d(), a(2).d(), a(3).d()], a(4).d()),
            "(DDDID)Z" => {
                let path = create_path_to(rt, nav, [a(1).d(), a(2).d(), a(3).d()], a(4).i());
                move_along(rt, nav, path, a(5).d())
            }
            "(Lnet/minecraft/world/entity/Entity;D)Z" => move_to_entity(rt, nav, a(1), a(2).d()),
            "(Lnet/minecraft/world/level/pathfinder/Path;D)Z" => move_along(rt, nav, a(1), a(2).d()),
            _ => return None,
        })),
        "createPath" => match desc {
            "(DDDI)Lnet/minecraft/world/level/pathfinder/Path;" => create_path_to(rt, nav, [a(1).d(), a(2).d(), a(3).d()], a(4).i()),
            "(Lnet/minecraft/core/BlockPos;I)Lnet/minecraft/world/level/pathfinder/Path;" => {
                let p = api::pos_of(rt, a(1));
                create_path_to_block(rt, nav, p, a(2).i())
            }
            "(Lnet/minecraft/core/BlockPos;II)Lnet/minecraft/world/level/pathfinder/Path;" => {
                let p = api::pos_of(rt, a(1));
                create_path_to_block(rt, nav, p, a(3).i())
            }
            "(Lnet/minecraft/world/entity/Entity;I)Lnet/minecraft/world/level/pathfinder/Path;" => create_path_to_entity(rt, nav, a(1), a(2).i()),
            "(Ljava/util/Set;I)Lnet/minecraft/world/level/pathfinder/Path;" | "(Ljava/util/stream/Stream;I)Lnet/minecraft/world/level/pathfinder/Path;" => {
                let first = jutil::elements(rt, a(1)).first().copied().unwrap_or(V::NULL);
                if first.r() == 0 {
                    return Some(V::NULL);
                }
                let p = api::pos_of(rt, first);
                create_path_to_block(rt, nav, p, a(2).i())
            }
            _ => return None,
        },
        // `isStableDestination(pos)`: on a solid block.
        "isStableDestination" => {
            let p = api::pos_of(rt, a(1));
            let mut w = World::new(rt);
            V::I(i32::from(w.full(rt, [p[0], p[1] - 1, p[2]])))
        }
        "shouldRecomputePath" => V::I(0),
        "getMaxDistanceToWaypoint" => V::F(rt.get_field(nav, "maxDistanceToWaypoint").f()),
        _ => return None,
    })
}

fn random_pos_call(rt: &mut Rt, owner: &str, name: &str, args: &[V]) -> Option<V> {
    let a = |i: usize| args.get(i).copied().unwrap_or_default();
    let mob = a(0);
    Some(match (owner, name) {
        (DEFAULT_RANDOM_POS | LAND_RANDOM_POS, "getPos") if args.len() >= 3 => {
            let p = if owner == LAND_RANDOM_POS { land_random_pos(rt, mob, a(1).i(), a(2).i()) } else { default_random_pos(rt, mob, a(1).i(), a(2).i()) };
            p.unwrap_or(V::NULL)
        }
        (DEFAULT_RANDOM_POS | LAND_RANDOM_POS, "getPosAway") => {
            let from = crate::api_common::xyz(rt, a(3));
            random_pos_away(rt, mob, a(1).i(), a(2).i(), from).unwrap_or(V::NULL)
        }
        (DEFAULT_RANDOM_POS | LAND_RANDOM_POS, "getPosTowards") => {
            let to = crate::api_common::xyz(rt, a(3));
            let here = mobs::position(rt, mob);
            let angle = if owner == DEFAULT_RANDOM_POS { a(4).d() } else { std::f64::consts::FRAC_PI_2 };
            random_pos_toward(rt, mob, a(1).i(), a(2).i(), [to[0] - here[0], to[2] - here[2]], angle).unwrap_or(V::NULL)
        }
        (RANDOM_POS, "generateRandomDirection") => {
            let (r, h, v) = (a(0), a(1).i(), a(2).i());
            let next = |rt: &mut Rt, bound: i32| api::call(rt, "net/minecraft/util/RandomSource.nextInt.(I)I", &[r, V::I(bound)]).i();
            let x = next(rt, 2 * h + 1) - h;
            let y = next(rt, 2 * v + 1) - v;
            let z = next(rt, 2 * h + 1) - h;
            block_pos(rt, [x, y, z])
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_follow_path_types_order() {
        for (i, k) in KINDS.iter().enumerate() {
            assert_eq!(*k as usize, i, "{:?} is {}", k, PATH_TYPES[i].0);
        }
    }
}
